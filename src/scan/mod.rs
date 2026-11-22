pub mod aggregate;
pub mod builder;
pub mod entry;
pub mod extensions;
pub mod mft;
pub mod prepare;
pub mod profile;
pub mod walker;

use crate::model::Tree;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};

/// Live counters shared with the UI while a scan runs; `cancel` stops it early.
#[derive(Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub denied: AtomicU64,
    pub cancel: AtomicBool,
    /// When set, cleanup suggestions are streamed here while the scan runs.
    pub live: std::sync::OnceLock<std::sync::Arc<crate::rules::candidates::LiveSink>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Method {
    /// NTFS MFT when elevated and possible, otherwise the directory walker.
    Auto,
    Walker,
    /// MFT only; an error when it is not possible.
    Mft,
}

/// Scans a drive root or folder with the fastest available method.
pub fn scan(root: &Path, progress: &Progress) -> Tree {
    scan_with(root, progress, Method::Auto).expect("auto scan always yields a tree")
}

pub fn scan_with(root: &Path, progress: &Progress, method: Method) -> Result<Tree, String> {
    let root = normalize_root(root);
    let mut note = String::new();
    if method != Method::Walker {
        match profile::time("mft total", || mft::scan(&root, progress)) {
            Ok(t) => return Ok(t),
            Err(e) if method == Method::Mft => return Err(e),
            Err(e) => note = e,
        }
        for c in [&progress.files, &progress.dirs, &progress.bytes, &progress.denied] {
            c.store(0, Relaxed);
        }
    }
    let mut t = profile::time("walker total", || walker::walk(&root, progress));
    t.method_note = note;
    Ok(t)
}

/// Scans several roots (drives) into one tree under a virtual "This PC" root. Each root runs its
/// own pipeline on its own thread (DISKSCOPE_DRIVES_PAR=0: one after another, for comparisons);
/// progress, cancel and live suggestions are shared. One root = a normal scan.
pub fn scan_many(roots: &[PathBuf], progress: &Progress, method: Method) -> Result<Tree, String> {
    if roots.len() == 1 {
        return scan_with(&roots[0], progress, method);
    }
    let start = std::time::Instant::now();
    let one = |r: &PathBuf| scan_with(r, progress, method);
    let trees: Vec<Result<Tree, String>> = if profile::knob("DISKSCOPE_DRIVES_PAR", 1) != 0 {
        std::thread::scope(|s| {
            let hs: Vec<_> = roots.iter().map(|r| s.spawn(move || one(r))).collect();
            hs.into_iter().map(|h| h.join().unwrap_or_else(|_| Err("scan thread panicked".into()))).collect()
        })
    } else {
        roots.iter().map(one).collect()
    };
    let mut all = Tree::multi_root();
    for t in trees.into_iter().rev() {
        all.graft(t?); // reversed: the first root ends up first among the drive nodes
    }
    all.scanned_at = crate::fmt::now();
    all.elapsed_ms = start.elapsed().as_millis() as u64;
    profile::report("all drives (scan + graft)", start.elapsed());
    Ok(all)
}

/// `C:` -> `C:\` (a bare drive letter means the current directory on that drive otherwise).
fn normalize_root(root: &Path) -> PathBuf {
    let s = root.to_string_lossy();
    if s.len() == 2 && s.ends_with(':') {
        return PathBuf::from(format!("{s}\\"));
    }
    root.to_path_buf()
}
