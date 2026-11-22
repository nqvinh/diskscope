//! Shared tree builder for both scan paths (directory walker and NTFS MFT reader): appends one
//! folder's entries at a time, groups many small files per extension, never descends reparse
//! points, and finishes the tree (roll-up, timings) the same way.

use super::{aggregate, entry::RawEntry, extensions::ext_of, Progress};
use crate::model::{Node, Tree, F_CANDIDATE, F_DENIED, F_DIR, F_GROUP, F_REPARSE, NONE};
use crate::rules::candidate_rules::{classify, old_versions, Confidence};
use crate::rules::candidates::{LiveSink, LiveSuggestion};
use std::sync::Arc;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::time::Instant;

const MAX_DENIED_PATHS: usize = 2000;
const MAX_EXTS: usize = 60_000;
/// Files below this size are grouped when a folder holds more than GROUP_MIN_COUNT of them...
const GROUP_MAX_SIZE: u64 = 64 * 1024;
const GROUP_MIN_COUNT: usize = 16;
/// ...and only extensions with at least this many small files there (a lone file stays itself).
const GROUP_MIN_PER_EXT: usize = 4;

fn is_small(e: &RawEntry) -> bool {
    !e.is_dir() && e.size < GROUP_MAX_SIZE
}

pub struct Builder {
    pub tree: Tree,
    ext_ids: HashMap<String, u16>,
    live: Option<Arc<LiveSink>>,
    /// Live suggestion node -> index in the sink's list.
    slots: HashMap<u32, usize>,
}

impl Builder {
    /// A tree holding only its root folder (times from `root_entry`).
    pub fn new(root: PathBuf, cluster: u64, root_entry: Option<&RawEntry>, live: Option<Arc<LiveSink>>) -> Self {
        let mut tree = Tree::new(root.clone(), cluster);
        let mut node = Node { flags: F_DIR, ..Node::default() };
        if let Some(e) = root_entry {
            node.modified = e.modified;
            node.created = e.created;
            node.accessed = e.accessed;
        }
        tree.push(&root.display().to_string(), node, NONE);
        Self { tree, ext_ids: HashMap::new(), live, slots: HashMap::new() }
    }

    fn ext_id(&mut self, name: &str) -> u16 {
        let Some(ext) = ext_of(name) else { return 0 };
        if let Some(&id) = self.ext_ids.get(&ext) {
            return id;
        }
        if self.tree.exts.len() >= MAX_EXTS {
            return 0;
        }
        let id = self.tree.exts.len() as u16;
        self.tree.exts.push(ext.clone());
        self.ext_ids.insert(ext, id);
        id
    }

    /// Appends one folder's entries; returns (node, name, entry id) of the sub-folders to descend.
    /// A folder with many small files keeps them as one entry per extension: exact totals and
    /// newest dates, a fraction of the memory (node_modules can hold millions of tiny files).
    pub fn add(&mut self, parent: u32, batch: Vec<RawEntry>) -> Vec<(u32, String, u64)> {
        let group = batch.iter().filter(|e| is_small(e)).count() > GROUP_MIN_COUNT;
        let mut per_ext: HashMap<Option<String>, usize> = HashMap::new();
        if group {
            for e in batch.iter().filter(|e| is_small(e)) {
                *per_ext.entry(ext_of(&e.name)).or_default() += 1;
            }
        }
        let mut groups: HashMap<u16, Node> = HashMap::new();
        let mut descend = Vec::new();
        let (mut new_dirs, mut bytes, mut newest) = (Vec::new(), 0u64, 0u32);
        for e in batch {
            if !e.is_dir() {
                bytes += e.size;
                newest = newest.max(e.modified);
            }
            if group && is_small(&e) && per_ext.get(&ext_of(&e.name)).copied().unwrap_or(0) >= GROUP_MIN_PER_EXT {
                let ext = self.ext_id(&e.name);
                let g = groups.entry(ext).or_insert_with(|| Node { flags: F_GROUP, ext, ..Node::default() });
                g.size += e.size;
                g.alloc += e.alloc;
                g.files += 1;
                g.modified = g.modified.max(e.modified);
                g.created = g.created.max(e.created);
                g.accessed = g.accessed.max(e.accessed);
                continue;
            }
            let mut node = Node { modified: e.modified, created: e.created, accessed: e.accessed, ..Node::default() };
            if e.is_dir() {
                node.flags = F_DIR;
                if e.is_reparse() {
                    node.flags |= F_REPARSE;
                    self.tree.reparse_skipped += 1;
                }
            } else {
                node.size = e.size;
                node.alloc = e.alloc;
                node.files = 1;
                node.ext = self.ext_id(&e.name);
                if e.is_reparse() {
                    node.flags |= F_REPARSE;
                }
            }
            let idx = self.tree.push(&e.name, node, parent);
            if e.is_dir() && !e.is_reparse() {
                new_dirs.push(idx);
                descend.push((idx, e.name, e.id));
            }
        }
        for (ext, mut g) in groups {
            g.items = g.files - 1; // the roll-up adds 1 per entry, so the folder counts every file
            let ext = &self.tree.exts[ext as usize];
            let name = if ext.is_empty() { format!("[{} small files]", g.files) } else { format!("[{} small {ext} files]", g.files) };
            self.tree.push(&name, g, parent);
        }
        if let Some(live) = self.live.clone() {
            self.live_update(&live, parent, &new_dirs, bytes, newest);
        }
        descend
    }

    fn inside_suggestion(&self, i: u32) -> bool {
        let mut c = self.tree.node(i).parent;
        while c != NONE {
            if self.tree.node(c).flags & F_CANDIDATE != 0 {
                return true;
            }
            c = self.tree.node(c).parent;
        }
        false
    }

    /// Classifies the folders just added (same rules as the final list) and adds the batch's
    /// bytes to every suggestion above it, so the live list grows while the scan runs.
    fn live_update(&mut self, live: &LiveSink, parent: u32, new_dirs: &[u32], bytes: u64, newest: u32) {
        let mut found: Vec<(u32, &'static str, String, Confidence)> = Vec::new();
        for &d in new_dirs {
            if let Some((kind, why, conf)) = classify(&self.tree, d, &live.ao, live.now, self.inside_suggestion(d)) {
                found.push((d, kind, why, conf));
            }
        }
        if self.tree.node(parent).flags & F_CANDIDATE == 0 && !self.inside_suggestion(parent) {
            for (d, why) in old_versions(&self.tree, parent) {
                found.push((d, "old-app-version", why, Confidence::Likely));
            }
        }
        let mut hits = Vec::new();
        let mut c = parent;
        while c != NONE && bytes > 0 {
            if let Some(&s) = self.slots.get(&c) {
                hits.push(s);
            }
            c = self.tree.node(c).parent;
        }
        if found.is_empty() && hits.is_empty() {
            return;
        }
        let Ok(mut list) = live.list.lock() else { return };
        for (d, kind, reason, confidence) in found {
            self.tree.node_mut(d).flags |= F_CANDIDATE;
            self.slots.insert(d, list.len());
            let modified = self.tree.node(d).modified;
            list.push(LiveSuggestion { path: self.tree.path(d), kind, reason, confidence, bytes: 0, modified });
        }
        for s in hits {
            list[s].bytes += bytes;
            list[s].modified = list[s].modified.max(newest);
        }
    }

    pub fn deny(&mut self, idx: u32, path: &Path) {
        self.tree.node_mut(idx).flags |= F_DENIED;
        self.tree.denied += 1;
        if self.tree.denied_paths.len() < MAX_DENIED_PATHS {
            self.tree.denied_paths.push(path.display().to_string());
        }
    }

    /// Roll-up, timings and cancel state; the tree is ready for the views.
    pub fn finish(self, progress: &Progress, start: Instant) -> Tree {
        let mut tree = self.tree;
        tree.nodes.shrink_last();
        tree.names.shrink_last();
        aggregate::roll_up(&mut tree);
        tree.cancelled = progress.cancel.load(Relaxed);
        tree.scanned_at = crate::fmt::now();
        tree.elapsed_ms = start.elapsed().as_millis() as u64;
        tree
    }
}
