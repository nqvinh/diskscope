//! Parallel directory walker (the path used without admin rights or off NTFS). Each directory is
//! listed by one rayon task (std `read_dir`, which on Windows returns size, attributes and times
//! from FindNextFile without extra system calls), then its entries are appended to the shared
//! tree under a short lock. Reparse points (junctions, symlinks, mount points, cloud placeholders)
//! are recorded but never descended. Unreadable folders are counted, flagged and listed.

use super::{builder::Builder, entry::RawEntry, Progress};
use crate::model::Tree;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Mutex;
use std::time::Instant;

const DEFAULT_THREADS: usize = 32;

/// Walks `root` (a drive root like `C:\` or any folder).
pub fn walk(root: &Path, progress: &Progress) -> Tree {
    let start = Instant::now();
    let cluster = crate::sys::cluster_size(root).unwrap_or(4096);
    let root_entry = std::fs::metadata(root).ok().map(|md| RawEntry::from_metadata(String::new(), &md, cluster, root));
    let shared = Mutex::new(Builder::new(root.to_path_buf(), cluster, root_entry.as_ref(), progress.live.get().cloned()));
    // Cold scans wait on the disk, not the CPU: more listings in flight keep an SSD's queue full.
    let threads = std::env::var("DISKSCOPE_THREADS").ok().and_then(|s| s.parse().ok()).unwrap_or(DEFAULT_THREADS);
    let root_buf = root.to_path_buf();
    match rayon::ThreadPoolBuilder::new().num_threads(threads).thread_name(|i| format!("scan-{i}")).build() {
        Ok(pool) => pool.scope(|s| scan_dir(s, root_buf.clone(), 0, &shared, progress, cluster)),
        Err(_) => rayon::scope(|s| scan_dir(s, root_buf.clone(), 0, &shared, progress, cluster)),
    }
    shared.into_inner().expect("scan lock").finish(progress, start)
}

fn scan_dir<'s>(s: &rayon::Scope<'s>, dir: PathBuf, idx: u32, shared: &'s Mutex<Builder>, progress: &'s Progress, cluster: u64) {
    if progress.cancel.load(Relaxed) {
        return;
    }
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
    progress.files.fetch_add(files, Relaxed);
    progress.bytes.fetch_add(bytes, Relaxed);
    progress.dirs.fetch_add(1, Relaxed);
    let descend = shared.lock().expect("scan lock").add(idx, batch);
    for (child, name, _) in descend {
        let path = dir.join(name);
        s.spawn(move |s| scan_dir(s, path, child, shared, progress, cluster));
    }
}

#[cfg(test)]
#[path = "walker_tests.rs"]
mod tests;
