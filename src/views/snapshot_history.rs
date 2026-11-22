//! Scan history: one compact snapshot per finished scan in
//! %LOCALAPPDATA%\diskscope\snapshots\<root key>\<scan time>.tsv, pruned to MAX_PER_ROOT per root,
//! MAX_TOTAL bytes overall and MAX_AGE_DAYS. Used by "Grew since last scan" (the newest one) and
//! by "Compare scans" (any two of the same root). Older versions kept one <root key>.tsv per root;
//! it is moved into the history the first time that root is listed.

use super::snapshot::{self, root_key, Snapshot};
use crate::model::Tree;
use std::path::{Path, PathBuf};

const MAX_PER_ROOT: usize = 20;
const MAX_TOTAL: u64 = 200 << 20;
const MAX_AGE_DAYS: u32 = 365;

#[derive(Clone, Debug, PartialEq)]
pub struct SnapInfo {
    pub file: PathBuf,
    pub taken_at: u32,
    /// Total size of the scanned root at that time.
    pub size: u64,
}

pub fn base() -> PathBuf {
    crate::sys::data_dir().join("snapshots")
}

fn dir_in(base: &Path, root: &Path) -> PathBuf {
    base.join(root_key(root))
}

/// Snapshots of `root`, newest first (moves an old single-file snapshot into the history).
pub fn list_in(base: &Path, root: &Path) -> Vec<SnapInfo> {
    let dir = dir_in(base, root);
    let legacy = base.join(format!("{}.tsv", root_key(root)));
    if let Some((t, _)) = snapshot::peek(&legacy) {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::rename(&legacy, dir.join(format!("{t}.tsv")));
    }
    let mut out: Vec<SnapInfo> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "tsv"))
        .filter_map(|e| snapshot::peek(&e.path()).map(|(taken_at, size)| SnapInfo { file: e.path(), taken_at, size }))
        .collect();
    out.sort_by(|a, b| b.taken_at.cmp(&a.taken_at));
    out
}

pub fn list(root: &Path) -> Vec<SnapInfo> {
    list_in(&base(), root)
}

/// The newest saved snapshot of `root` (the baseline for "Grew since last scan").
pub fn latest(root: &Path) -> Option<Snapshot> {
    list(root).first().and_then(|s| snapshot::load(&s.file))
}

/// Saves `tree` as a new history entry and prunes; returns the file.
pub fn save_in(base: &Path, tree: &Tree, now: u32) -> std::io::Result<PathBuf> {
    let file = dir_in(base, &tree.root_path).join(format!("{}.tsv", tree.scanned_at));
    snapshot::save(tree, &file)?;
    prune(base, now);
    Ok(file)
}

pub fn save(tree: &Tree) -> std::io::Result<PathBuf> {
    save_in(&base(), tree, crate::fmt::now())
}

/// Keeps at most MAX_PER_ROOT per root, nothing older than MAX_AGE_DAYS, and MAX_TOTAL bytes
/// overall (oldest removed first). Only *.tsv files inside the history folders are touched.
pub fn prune(base: &Path, now: u32) {
    let mut all: Vec<(u32, u64, PathBuf)> = Vec::new();
    for d in std::fs::read_dir(base).into_iter().flatten().flatten().filter(|e| e.path().is_dir()) {
        let mut files: Vec<(u32, u64, PathBuf)> = std::fs::read_dir(d.path())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "tsv"))
            .filter_map(|e| Some((snapshot::peek(&e.path())?.0, e.metadata().ok()?.len(), e.path())))
            .collect();
        files.sort_by(|a, b| b.0.cmp(&a.0));
        for (i, f) in files.into_iter().enumerate() {
            let old = now.saturating_sub(f.0) > MAX_AGE_DAYS * 86_400;
            if i >= MAX_PER_ROOT || (old && i > 0) {
                let _ = std::fs::remove_file(&f.2); // the newest of a root is kept even when old
            } else {
                all.push(f);
            }
        }
    }
    all.sort_by(|a, b| b.0.cmp(&a.0));
    let mut total = 0u64;
    for (_, len, path) in all {
        total += len;
        if total > MAX_TOTAL {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[test]
fn history_keeps_newest_migrates_and_prunes() {
    use crate::model::fixture::build;
    let base = std::env::temp_dir().join(format!("diskscope-hist-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let mut t = build("Q:\\", &[("a\\x.bin", 3 << 20, 0)]);
    // An old-style single snapshot is moved into the history.
    t.scanned_at = 1_000;
    std::fs::create_dir_all(&base).unwrap();
    snapshot::save(&t, &base.join("q.tsv")).unwrap();
    assert_eq!(list_in(&base, Path::new("Q:\\")).len(), 1);
    assert!(!base.join("q.tsv").exists());
    let day = 86_400u32;
    let now = 2_000 * day;
    for k in 0..25u32 {
        t.scanned_at = now - (25 - k) * 60;
        save_in(&base, &t, now).unwrap();
    }
    let l = list_in(&base, Path::new("Q:\\"));
    assert_eq!(l.len(), MAX_PER_ROOT, "per-root cap");
    assert!(l.windows(2).all(|w| w[0].taken_at > w[1].taken_at), "newest first");
    assert_eq!(l[0].size, 3 << 20);
    assert!(l.iter().all(|s| s.taken_at > 1_000), "the year-old migrated one is pruned");
    std::fs::remove_dir_all(&base).ok();
}
