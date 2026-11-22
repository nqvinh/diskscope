//! Parallel directory walker (the path used without admin rights or off NTFS). Each directory is
//! listed by one rayon task (std `read_dir`, which on Windows returns size, attributes and times
//! from FindNextFile without extra system calls), then its entries are appended to the shared
//! tree under a short lock. Reparse points (junctions, symlinks, mount points, cloud placeholders)
//! are recorded but never descended. Unreadable folders are counted, flagged and listed.

use super::prepare::{prepare, Ctx};
use super::profile::{self, Sum};
use super::{builder::Builder, entry::RawEntry, Progress};
use crate::model::Tree;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Mutex;
use std::time::Instant;

const DEFAULT_THREADS: usize = 32;

/// Profile sums (DISKSCOPE_PROFILE): listing, waiting for the builder lock, holding it.
static LIST: Sum = Sum::new();
static LOCK_WAIT: Sum = Sum::new();
static LOCK_HOLD: Sum = Sum::new();

/// Walks `root` (a drive root like `C:\` or any folder).
pub fn walk(root: &Path, progress: &Progress) -> Tree {
    let start = Instant::now();
    for s in [&LIST, &LOCK_WAIT, &LOCK_HOLD] {
        s.reset();
    }
    let cluster = crate::sys::cluster_size(root).unwrap_or(4096);
    let root_entry = std::fs::metadata(root).ok().map(|md| RawEntry::from_metadata(String::new(), &md, cluster, root));
    let shared = Mutex::new(Builder::new(root.to_path_buf(), cluster, root_entry.as_ref(), progress.live.get().cloned()));
    // Cold scans wait on the disk, not the CPU: more listings in flight keep an SSD's queue full.
    let threads = profile::knob("DISKSCOPE_THREADS", DEFAULT_THREADS);
    let root_buf = root.to_path_buf();
    let ctx = shared.lock().expect("scan lock").ctx_of(0);
    let w = Walk { shared: &shared, progress, cluster, outside: profile::knob("DISKSCOPE_WALKER_PREPARE", 1) != 0 };
    match rayon::ThreadPoolBuilder::new().num_threads(threads).thread_name(|i| format!("scan-{i}")).build() {
        Ok(pool) => pool.scope(|s| scan_dir(s, root_buf.clone(), 0, ctx, &w)),
        Err(_) => rayon::scope(|s| scan_dir(s, root_buf.clone(), 0, ctx, &w)),
    }
    profile::report("walker walk (wall)", start.elapsed());
    LIST.report("walker listing (sum, threads)");
    LOCK_WAIT.report("walker lock wait (sum)");
    LOCK_HOLD.report("walker lock hold (sum)");
    shared.into_inner().expect("scan lock").finish(progress, start)
}

/// What every listing task shares.
struct Walk<'a> {
    shared: &'a Mutex<Builder>,
    progress: &'a Progress,
    cluster: u64,
    /// Prepare (labels, grouping) outside the builder lock; false = old behaviour, for A/B runs.
    outside: bool,
}

fn scan_dir<'s>(s: &rayon::Scope<'s>, dir: PathBuf, idx: u32, ctx: Ctx, w: &'s Walk<'s>) {
    let (shared, progress, cluster) = (w.shared, w.progress, w.cluster);
    if progress.cancel.load(Relaxed) {
        return;
    }
    let t_list = Instant::now();
    let read = match std::fs::read_dir(&dir) {
        Ok(r) => r,
        Err(_) => {
            progress.denied.fetch_add(1, Relaxed);
            shared.lock().expect("scan lock").deny(idx, &dir);
            return;
        }
    };
    let mut batch = Vec::new();
    let (mut files, mut bytes) = (0u64, 0u64);
    for ent in read.flatten() {
        let Ok(md) = ent.metadata() else { continue };
        let name = ent.file_name().to_string_lossy().into_owned();
        let e = RawEntry::from_metadata(name, &md, cluster, &dir);
        if !e.is_dir() {
            files += 1;
            bytes += e.size;
        }
        batch.push(e);
    }
    LIST.add(t_list);
    progress.files.fetch_add(files, Relaxed);
    progress.bytes.fetch_add(bytes, Relaxed);
    progress.dirs.fetch_add(1, Relaxed);
    let depth = ctx.depth;
    let filter = progress.filter.get().map(|f| &**f);
    let prepared = w.outside.then(|| prepare(&ctx, std::mem::take(&mut batch), filter, Some(&dir)));
    let t_wait = Instant::now();
    let mut b = shared.lock().expect("scan lock");
    LOCK_WAIT.add(t_wait);
    let t_hold = Instant::now();
    let prepared = prepared.unwrap_or_else(|| prepare(&ctx, batch, filter, Some(&dir)));
    let descend = b.commit(idx, depth, prepared);
    drop(b);
    LOCK_HOLD.add(t_hold);
    for c in descend {
        let path = dir.join(&c.ctx.name);
        s.spawn(move |s| scan_dir(s, path, c.node, c.ctx, w));
    }
}

#[cfg(test)]
#[path = "walker_tests.rs"]
mod tests;
