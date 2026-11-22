//! A finished scan plus everything derived from it. Built on the scan thread so the window stays
//! responsive; refreshed on the UI thread after a cleanup removes items.

use crate::model::Tree;
use crate::rules::{ao_sessions::AoSessions, candidates::{detect, Candidate, LiveSink}};
use crate::scan::extensions::{ext_stats, palette, ExtStat};
use crate::scan::{scan, Progress};
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
            prev_snapshot,
            snapshot_note,
            ao,
            last_access: crate::sys::last_access_enabled(),
        };
        l.refresh();
        l
    }

    pub fn refresh(&mut self) {
        self.ext_stats = ext_stats(&self.tree, 0);
        self.palette = palette(&self.tree, &self.ext_stats);
        self.candidates = detect(&self.tree, &self.ao, crate::fmt::now());
        self.generation += 1;
    }

    /// Detaches a deleted path from the tree (call `refresh` after a batch).
    pub fn forget(&mut self, path: &Path) {
        if let Some(i) = self.tree.find(path) {
            self.tree.remove_subtree(i);
        }
    }
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
            let tree = scan(&root, &p);
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
