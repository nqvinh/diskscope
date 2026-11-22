//! Fast NTFS scan by reading the Master File Table directly (needs administrator rights), the
//! way WizTree does. Read-only: the volume handle is GENERIC_READ and only a layout query is
//! issued. Pipeline: one thread reads $MFT sequentially in 8 MiB chunks, rayon parses records in
//! parallel, one collector merges them into a table indexed by record number, then the shared
//! builder turns the table into the tree. Any failure returns Err and the caller walks instead.

mod record;
mod tree_build;
mod volume;

use super::Progress;
use crate::model::Tree;
use rayon::iter::{ParallelBridge, ParallelIterator};
use record::{apply_fixup, merge, parse_record, Parsed, Rec};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::mpsc;
use std::time::Instant;
use tree_build::Table;
use volume::Volume;

pub const NOT_ELEVATED: &str = "Run as administrator for the fast NTFS scan";
const CHUNK: usize = 8 << 20;

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

/// Reads $MFT and returns one merged record per record number.
fn read_table(vol: Volume, runs: Vec<(u64, u64)>, progress: &Progress) -> Result<Table, String> {
    let (record, sector, cluster, valid) = (vol.record, vol.sector, vol.cluster, vol.mft_valid);
    let total = (valid / record as u64) as usize;
    let mut table = Table { recs: vec![Rec::default(); total], names: Vec::new() };
    std::thread::scope(|s| -> Result<(), String> {
        let (tx, rx) = mpsc::sync_channel::<(u32, Vec<u8>)>(4);
        let reader = s.spawn(move || -> Result<(), String> {
            let mut pos = 0u64; // byte position in the $MFT stream
            for (lcn, len) in runs {
                let (disk, bytes) = (lcn * cluster, len * cluster);
                let mut done = 0u64;
                while done < bytes && pos < valid {
                    if progress.cancel.load(Relaxed) {
                        return Ok(());
                    }
                    let take = (CHUNK as u64).min(bytes - done).min(valid - pos) as usize;
                    let mut buf = vec![0u8; take];
                    vol.read_at(disk + done, &mut buf)?;
                    if tx.send(((pos / record as u64) as u32, buf)).is_err() {
                        return Ok(());
                    }
                    done += take as u64;
                    pos += take as u64;
                }
            }
            Ok(())
        });
        let (rtx, rrx) = mpsc::channel::<ChunkOut>();
        s.spawn(move || {
            rx.into_iter().par_bridge().for_each_with(rtx, |rtx, (first, mut buf)| {
                let _ = rtx.send(parse_chunk(&mut buf, first, record, sector));
            });
        });
        for out in rrx {
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
        }
        reader.join().map_err(|_| "MFT reader panicked".to_string())?
    })?;
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
    let vol = Volume::open(drive.chars().next().unwrap_or('C'))?;
    let used = vol.total_clusters.saturating_sub(vol.free_clusters) * vol.cluster;
    let cluster = vol.cluster;
    let runs = vol.mft_runs()?;
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
