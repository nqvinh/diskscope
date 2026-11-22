pub mod aggregate;
pub mod builder;
pub mod entry;
pub mod extensions;
pub mod mft;
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
        match mft::scan(&root, progress) {
            Ok(t) => return Ok(t),
            Err(e) if method == Method::Mft => return Err(e),
            Err(e) => note = e,
        }
        for c in [&progress.files, &progress.dirs, &progress.bytes, &progress.denied] {
            c.store(0, Relaxed);
        }
    }
    let mut t = walker::walk(&root, progress);
    t.method_note = note;
    Ok(t)
}

/// `C:` -> `C:\` (a bare drive letter means the current directory on that drive otherwise).
fn normalize_root(root: &Path) -> PathBuf {
    let s = root.to_string_lossy();
    if s.len() == 2 && s.ends_with(':') {
        return PathBuf::from(format!("{s}\\"));
    }
    root.to_path_buf()
}
