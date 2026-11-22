//! A finished scan plus everything derived from it. Built on the scan thread so the window stays
//! responsive; refreshed on the UI thread after a cleanup removes items.

use crate::model::{graft::ALL_DRIVES, Tree};
use crate::rules::{ao_sessions::AoSessions, candidates::{detect, Candidate, LiveSink}};
use crate::scan::extensions::{ext_stats, palette, ExtStat};
use crate::scan::{scan_many, Method, Progress};
use crate::views::snapshot::{self, Snapshot};
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
}

impl Loaded {
    pub fn build(tree: Tree, ao: AoSessions) -> Self {
        let file = snapshot::snapshot_file(&tree.root_path);
        let prev_snapshot = snapshot::load(&file);
        let snapshot_note = if tree.cancelled {
            "scan cancelled: baseline not updated".to_string()
        } else {
            match snapshot::save(&tree, &file) {
                Ok(()) => "baseline saved for the next 'Grew' comparison".to_string(),
                Err(e) => format!("baseline not saved: {e}"),
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
        };
        l.refresh();
        l
    }

    pub fn refresh(&mut self) {
        let d = Derived::compute(&self.tree, &self.ao);
        (self.ext_stats, self.palette, self.candidates, self.label_totals, self.ext_index) = (d.ext_stats, d.palette, d.candidates, d.label_totals, d.ext_index);
        self.generation += 1;
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
    pub fn compute(tree: &Tree, ao: &AoSessions) -> Self {
        use crate::scan::profile::time;
        let t = std::time::Instant::now();
        // Four independent read-only passes over the tree, run side by side.
        let ((ext_stats, candidates), (label_totals, ext_index)) = rayon::join(
            || rayon::join(|| time("derive: extension table", || ext_stats(tree, 0)), || time("derive: suggestions", || detect(tree, ao, crate::fmt::now()))),
            || rayon::join(|| time("derive: label totals", || super::labels_view::label_totals(tree)), || time("derive: extension file index", || crate::views::ext_files::build_index(tree))),
        );
        let palette = palette(tree, &ext_stats);
        crate::scan::profile::report("derive: total (parallel)", t.elapsed());
        Self { ext_stats, palette, candidates, label_totals, ext_index }
    }
}

/// "This PC" means every local NTFS drive (scanned side by side); anything else is one path.
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
    pub fn start(path: PathBuf) -> Self {
        let progress = Arc::new(Progress::default());
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
            Loaded::build(tree, sink.ao.clone())
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
