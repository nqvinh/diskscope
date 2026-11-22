//! A finished scan plus everything derived from it. Built on the scan thread so the window stays
//! responsive; refreshed on the UI thread after a cleanup removes items.

use crate::model::{graft::ALL_DRIVES, Tree, F_CANDIDATE};
use crate::rules::excludes::{never_suggested, Exclude, ScanFilter};
use crate::rules::{ao_sessions::AoSessions, candidates::{detect, Candidate, LiveSink}};
use crate::scan::extensions::{ext_stats, palette, ExtStat};
use crate::scan::{scan_many, Method, Progress};
use crate::views::snapshot::Snapshot;
use crate::views::snapshot_history;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

pub struct Loaded {
    pub tree: Tree,
    /// Bumped whenever the tree changes, so views know to rebuild their caches.
    pub generation: u64,
    pub ext_stats: Vec<ExtStat>,
    pub palette: Vec<[u8; 3]>,
    pub candidates: Vec<Candidate>,
    /// (label, bytes, files) for the By label tab.
    pub label_totals: Vec<(crate::rules::labels::Label, u64, u64)>,
    /// File node ids per extension, for the Files tab.
    pub ext_index: crate::views::ext_files::ExtIndex,
    pub prev_snapshot: Option<Snapshot>,
    pub snapshot_note: String,
    pub ao: AoSessions,
    pub last_access: Option<bool>,
    /// Paths the user never wants suggested (Settings); filtered out of `candidates`.
    pub never_suggest: Vec<String>,
}

impl Loaded {
    pub fn build(tree: Tree, ao: AoSessions, never_suggest: Vec<String>) -> Self {
        let prev_snapshot = snapshot_history::latest(&tree.root_path);
        let snapshot_note = if tree.cancelled {
            "scan cancelled: not added to the scan history".to_string()
        } else {
            match snapshot_history::save(&tree) {
                Ok(_) => "saved in the scan history (Grew since last scan, Compare scans)".to_string(),
                Err(e) => format!("scan history not saved: {e}"),
            }
        };
        let mut l = Self {
            tree,
            generation: 0,
            ext_stats: Vec::new(),
            palette: Vec::new(),
            candidates: Vec::new(),
            label_totals: Vec::new(),
            ext_index: crate::views::ext_files::ExtIndex { by_ext: Vec::new() },
            prev_snapshot,
            snapshot_note,
            ao,
            last_access: crate::sys::last_access_enabled(),
            never_suggest,
        };
        l.refresh();
        l
    }

    pub fn refresh(&mut self) {
        let d = Derived::compute(&self.tree, &self.ao, &self.never_suggest);
        (self.ext_stats, self.palette, self.candidates, self.label_totals, self.ext_index) = (d.ext_stats, d.palette, d.candidates, d.label_totals, d.ext_index);
        // Marks the final suggestions, so their right-click menu offers "Never suggest this".
        for c in &self.candidates {
            self.tree.node_mut(c.node).flags |= F_CANDIDATE;
        }
        self.generation += 1;
    }

    /// New "never suggest" list (from Settings): the suggestions are recomputed right away.
    pub fn set_never_suggest(&mut self, list: &[String]) {
        if self.never_suggest != list {
            self.never_suggest = list.to_vec();
            self.refresh();
        }
    }

    /// Detaches a deleted path from the tree (call `refresh` after a batch).
    pub fn forget(&mut self, path: &Path) {
        if let Some(i) = self.tree.find(path) {
            self.tree.remove_subtree(i);
        }
    }
}

/// Everything the views derive from a tree (extension table, colours, suggestions, label totals,
/// per-extension file index). Each is a read-only pass over the tree.
pub struct Derived {
    pub ext_stats: Vec<ExtStat>,
    pub palette: Vec<[u8; 3]>,
    pub candidates: Vec<Candidate>,
    pub label_totals: Vec<(crate::rules::labels::Label, u64, u64)>,
    pub ext_index: crate::views::ext_files::ExtIndex,
}

impl Derived {
    pub fn compute(tree: &Tree, ao: &AoSessions, never_suggest: &[String]) -> Self {
        use crate::scan::profile::time;
        let t = std::time::Instant::now();
        // Four independent read-only passes over the tree, run side by side.
        let ((ext_stats, mut candidates), (label_totals, ext_index)) = rayon::join(
            || rayon::join(|| time("derive: extension table", || ext_stats(tree, 0)), || time("derive: suggestions", || detect(tree, ao, crate::fmt::now()))),
            || rayon::join(|| time("derive: label totals", || super::labels_view::label_totals(tree)), || time("derive: extension file index", || crate::views::ext_files::build_index(tree))),
        );
        let palette = palette(tree, &ext_stats);
        if !never_suggest.is_empty() {
            candidates.retain(|c| !never_suggested(never_suggest, &tree.path(c.node)));
        }
        crate::scan::profile::report("derive: total (parallel)", t.elapsed());
        Self { ext_stats, palette, candidates, label_totals, ext_index }
    }
}

/// "This PC" means every local NTFS drive (one tree); anything else is one path.
pub fn roots_of(path: &Path) -> Vec<PathBuf> {
    if is_all_drives(path) {
        crate::sys::all_drive_roots()
    } else {
        vec![path.to_path_buf()]
    }
}

pub fn is_all_drives(path: &Path) -> bool {
    path.as_os_str().eq_ignore_ascii_case(ALL_DRIVES)
}

pub struct ScanJob {
    pub path: PathBuf,
    pub progress: Arc<Progress>,
    pub started: Instant,
    handle: Option<JoinHandle<Loaded>>,
}

impl ScanJob {
    /// Starts a scan of `path` with the user's excludes and "never suggest" list.
    pub fn start(path: PathBuf, excludes: &[Exclude], never_suggest: Vec<String>) -> Self {
        let progress = Arc::new(Progress::default());
        progress.filter.set(Arc::new(ScanFilter::new(excludes))).ok();
        let (p, root) = (progress.clone(), path.clone());
        let handle = std::thread::spawn(move || {
            // Session status first, so AO worktrees are classified correctly in the live list.
            let sink = Arc::new(LiveSink::new(AoSessions::load()));
            p.live.set(sink.clone()).ok();
            let tree = scan_many(&roots_of(&root), &p, Method::Auto).unwrap_or_else(|e| {
                let mut t = Tree::multi_root();
                t.method_note = e;
                t
            });
            Loaded::build(tree, sink.ao.clone(), never_suggest)
        });
        Self { path, progress, started: Instant::now(), handle: Some(handle) }
    }

    pub fn cancel(&self) {
        self.progress.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// The finished scan, once the thread is done.
    pub fn poll(&mut self) -> Option<Loaded> {
        if self.handle.as_ref().is_some_and(|h| h.is_finished()) {
            return self.handle.take().and_then(|h| h.join().ok());
        }
        None
    }
}

#[test]
fn never_suggest_filters_derived_suggestions() {
    let tree = crate::model::fixture::build("Q:\\", &[("rs\\Cargo.toml", 10, 1), ("rs\\target\\release\\app.exe", 50 << 20, 1)]);
    let ao = AoSessions::default();
    let all = Derived::compute(&tree, &ao, &[]).candidates;
    assert!(all.iter().any(|c| tree.path(c.node) == Path::new("Q:\\rs\\target")), "rust target is suggested");
    let kept = Derived::compute(&tree, &ao, &["q:\\RS".to_string()]).candidates;
    assert!(kept.is_empty(), "everything inside a never-suggest entry is dropped");
}
