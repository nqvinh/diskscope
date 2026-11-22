//! Fast NTFS scan by reading the Master File Table directly (needs administrator rights), the
//! way WizTree does. Read-only: the volume handle is GENERIC_READ and only a layout query is
//! issued. Pipeline: one thread reads $MFT sequentially in 8 MiB chunks, rayon parses records in
//! parallel, one collector merges them into a table indexed by record number, then the shared
//! builder turns the table into the tree. Any failure returns Err and the caller walks instead.

mod record;
mod tree_build;
mod volume;

use super::profile::{self, Sum};
use super::Progress;
use crate::model::Tree;
use rayon::iter::{ParallelBridge, ParallelIterator};
use record::{apply_fixup, merge, parse_record, Parsed, Rec};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::mpsc;
use std::time::Instant;
use tree_build::Table;
use volume::Volume;

pub const NOT_ELEVATED: &str = "Run as administrator for the fast NTFS scan";
const CHUNK: usize = 8 << 20;
/// Parallel $MFT readers (DISKSCOPE_MFT_READERS overrides). Measured on NVMe: 1 reader 3.8 s of
/// reads on C:, 4 readers 1.9-2.5 s wall; peak memory +1-2 %.
const READERS: usize = 4;
/// Chunks queued between the readers and the parsers (bounds memory: CHUNK each).
const IN_FLIGHT: usize = 4;

struct ChunkOut {
    items: Vec<Parsed>,
    names: String,
}

fn parse_chunk(buf: &mut [u8], first: u32, record: usize, sector: usize) -> ChunkOut {
    let mut out = ChunkOut { items: Vec::with_capacity(buf.len() / record), names: String::new() };
    for (k, r) in buf.chunks_exact_mut(record).enumerate() {
        if apply_fixup(r, sector) {
            if let Some(p) = parse_record(r, first + k as u32, &mut out.names) {
                out.items.push(p);
            }
        }
    }
    out
}

/// (disk byte offset, length, first record number) of every read, in $MFT stream order: each
/// data run split into CHUNK-sized pieces, stopping at the valid end of the stream.
fn plan_chunks(runs: &[(u64, u64)], cluster: u64, valid: u64, record: usize) -> Vec<(u64, usize, u32)> {
    let mut out = Vec::new();
    let mut pos = 0u64; // byte position in the $MFT stream
    for &(lcn, len) in runs {
        let (disk, bytes) = (lcn * cluster, len * cluster);
        let mut done = 0u64;
        while done < bytes && pos < valid {
            let take = (CHUNK as u64).min(bytes - done).min(valid - pos);
            out.push((disk + done, take as usize, (pos / record as u64) as u32));
            done += take;
            pos += take;
        }
    }
    out
}

/// Reads $MFT and returns one merged record per record number. `readers` threads (each with its
/// own read-only handle) pull chunks from a shared counter, so several reads are in flight.
fn read_table(vol: Volume, runs: Vec<(u64, u64)>, progress: &Progress) -> Result<Table, String> {
    let (record, sector, valid) = (vol.record, vol.sector, vol.mft_valid);
    let total = (valid / record as u64) as usize;
    let chunks = plan_chunks(&runs, vol.cluster, valid, record);
    let readers = profile::knob("DISKSCOPE_MFT_READERS", READERS).clamp(1, 8);
    let mut vols = Vec::with_capacity(readers);
    for _ in 1..readers {
        match vol.reopen() {
            Ok(v) => vols.push(v),
            Err(_) => break, // fewer readers is still correct
        }
    }
    vols.push(vol);
    let next = AtomicUsize::new(0);
    let (io, parse, merge_t) = (Sum::new(), Sum::new(), Sum::new());
    let mut table = Table { recs: vec![Rec::default(); total], names: Vec::new() };
    let t_read = Instant::now();
    std::thread::scope(|s| -> Result<(), String> {
        let (tx, rx) = mpsc::sync_channel::<(u32, Vec<u8>)>(IN_FLIGHT.max(vols.len()));
        let handles: Vec<_> = vols
            .into_iter()
            .map(|v| {
                let (tx, chunks, next, io) = (tx.clone(), &chunks, &next, &io);
                s.spawn(move || -> Result<(), String> {
                    while !progress.cancel.load(Relaxed) {
                        let Some(&(disk, len, first)) = chunks.get(next.fetch_add(1, Relaxed)) else { break };
                        let mut buf = vec![0u8; len];
                        let t = Instant::now();
                        v.read_at(disk, &mut buf)?;
                        io.add(t);
                        if tx.send((first, buf)).is_err() {
                            break;
                        }
                    }
                    Ok(())
                })
            })
            .collect();
        drop(tx);
        let (rtx, rrx) = mpsc::channel::<ChunkOut>();
        let parse = &parse;
        s.spawn(move || {
            rx.into_iter().par_bridge().for_each_with(rtx, |rtx, (first, mut buf)| {
                let t = Instant::now();
                let out = parse_chunk(&mut buf, first, record, sector);
                parse.add(t);
                let _ = rtx.send(out);
            });
        });
        for out in rrx {
            let t = Instant::now();
            let chunk = table.names.len() as u32;
            table.names.push(out.names);
            let (mut bytes, count) = (0u64, out.items.len() as u64);
            for p in out.items {
                let mut r = p.rec;
                r.name_chunk = chunk;
                let slot = p.base.unwrap_or(p.number) as usize;
                if slot < total {
                    merge(&mut table.recs[slot], &r, p.base.is_none());
                    bytes += r.size;
                }
            }
            progress.files.fetch_add(count, Relaxed);
            progress.bytes.fetch_add(bytes, Relaxed);
            merge_t.add(t);
        }
        for h in handles {
            h.join().map_err(|_| "MFT reader panicked".to_string())??;
        }
        Ok(())
    })?;
    profile::report(&format!("mft read+parse ({readers} readers, wall)"), t_read.elapsed());
    io.report("mft disk reads (sum)");
    parse.report("mft record parse (sum, threads)");
    merge_t.report("mft merge into table");
    Ok(table)
}

/// Scans `root` (a drive root or a folder on it) from the MFT. Err = use the walker instead.
pub fn scan(root: &Path, progress: &Progress) -> Result<Tree, String> {
    let start = Instant::now();
    if !crate::sys::is_elevated() {
        return Err(NOT_ELEVATED.into());
    }
    let drive = crate::sys::drive_root(root).ok_or("not a local drive path")?;
    if crate::sys::filesystem_name(&drive).as_deref() != Some("NTFS") {
        return Err("not an NTFS volume; using the directory walker".into());
    }
    let t_open = Instant::now();
    let vol = Volume::open(drive.chars().next().unwrap_or('C'))?;
    let used = vol.total_clusters.saturating_sub(vol.free_clusters) * vol.cluster;
    let cluster = vol.cluster;
    let runs = vol.mft_runs()?;
    profile::report("mft open + map $MFT runs", t_open.elapsed());
    let table = read_table(vol, runs, progress)?;
    let rel: PathBuf = root.components().filter(|c| matches!(c, Component::Normal(_))).collect();
    let mut tree = tree_build::build(&table, &rel, root.to_path_buf(), cluster, progress, start)?;
    drop(table);
    tree.method = "ntfs-mft";
    if rel.as_os_str().is_empty() {
        tree.volume_used = Some(used);
    }
    Ok(tree)
}

#[test]
fn chunks_cover_the_valid_stream_once_in_order() {
    // Two runs (4 KiB clusters): 3000 clusters at lcn 100, 2000 at lcn 9000; 1 KiB records.
    let (cluster, record) = (4096u64, 1024usize);
    let runs = [(100u64, 3000u64), (9000, 2000)];
    let valid = 4500 * cluster; // the stream ends inside the second run
    let c = plan_chunks(&runs, cluster, valid, record);
    assert_eq!(c.iter().map(|x| x.1 as u64).sum::<u64>(), valid);
    assert!(c.iter().all(|x| x.1 <= CHUNK && x.1 % record == 0));
    let mut pos = 0u64;
    for &(disk, len, first) in &c {
        assert_eq!(first as u64, pos / record as u64, "record numbers follow the stream");
        assert_eq!(Some(disk), volume::stream_to_disk(&runs, pos, cluster));
        pos += len as u64;
    }
}
