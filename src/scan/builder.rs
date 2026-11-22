//! Shared tree builder for both scan paths (directory walker and NTFS MFT reader): commits one
//! prepared folder at a time (see `prepare` for grouping and labels), never descends reparse
//! points, and finishes the tree (roll-up, timings) the same way.

use super::prepare::{prepare, Ctx, Prepared};
use super::{aggregate, entry::RawEntry, Progress};
use crate::model::{Node, Tree, F_CANDIDATE, F_DENIED, F_DIR, F_REPARSE, NONE};
use crate::rules::candidate_rules::{classify, old_versions, Confidence};
use crate::rules::candidates::{LiveSink, LiveSuggestion};
use crate::rules::labels::{self, Label};
use std::sync::Arc;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::time::Instant;

const MAX_DENIED_PATHS: usize = 2000;
const MAX_EXTS: usize = 60_000;

/// A sub-folder to descend: its node, source id (MFT record) and rule context (holds its name).
pub struct Child {
    pub node: u32,
    pub id: u64,
    pub ctx: Ctx,
}

pub struct Builder {
    pub tree: Tree,
    ext_ids: HashMap<String, u16>,
    live: Option<Arc<LiveSink>>,
    /// Live suggestion node -> index in the sink's list.
    slots: HashMap<u32, usize>,
    /// Path depth of the scan root (C:\ = 0), for drive-root label rules.
    root_depth: usize,
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
        node.label = labels::root(&root) as u8;
        let root_depth = root.components().filter(|c| matches!(c, std::path::Component::Normal(_))).count();
        tree.push(&root.display().to_string(), node, NONE);
        Self { tree, ext_ids: HashMap::new(), live, slots: HashMap::new(), root_depth }
    }

    /// Interned id of a lowercase extension (0 = none, or past the table limit).
    fn ext_id(&mut self, ext: Option<&str>) -> u16 {
        let Some(ext) = ext else { return 0 };
        if let Some(&id) = self.ext_ids.get(ext) {
            return id;
        }
        if self.tree.exts.len() >= MAX_EXTS {
            return 0;
        }
        let id = self.tree.exts.len() as u16;
        self.tree.exts.push(ext.to_string());
        self.ext_ids.insert(ext.to_string(), id);
        id
    }

    /// Rule context of an existing folder (label, name, depth of its children).
    pub fn ctx_of(&self, parent: u32) -> Ctx {
        let name = if parent == 0 {
            self.tree.root_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
        } else {
            self.tree.name(parent).to_string()
        };
        Ctx { label: Label::from_u8(self.tree.node(parent).label), name, depth: self.depth(parent) + 1 }
    }

    /// Appends one folder's entries (prepare + commit in one step).
    pub fn add(&mut self, parent: u32, batch: Vec<RawEntry>) -> Vec<Child> {
        let ctx = self.ctx_of(parent);
        self.commit(parent, ctx.depth, prepare(&ctx, batch))
    }

    /// Pushes a prepared folder (`depth` = its children's depth); returns the sub-folders to
    /// descend, each with the context its own batch needs.
    pub fn commit(&mut self, parent: u32, depth: usize, p: Prepared) -> Vec<Child> {
        let (mut descend, mut new_dirs) = (Vec::new(), Vec::new());
        for it in p.items {
            let mut node = it.node;
            let (dir, reparse) = (node.flags & F_DIR != 0, node.flags & F_REPARSE != 0);
            if !dir {
                node.ext = self.ext_id(it.ext.as_deref());
            } else if reparse {
                self.tree.reparse_skipped += 1;
            }
            let idx = self.tree.push(&it.name, node, parent);
            if dir && !reparse {
                new_dirs.push(idx);
                let ctx = Ctx { label: Label::from_u8(node.label), name: it.name, depth: depth + 1 };
                descend.push(Child { node: idx, id: it.id, ctx });
            }
        }
        for g in p.groups {
            let mut node = g.node;
            node.ext = self.ext_id(g.ext.as_deref());
            node.items = node.files - 1; // the roll-up adds 1 per entry, so the folder counts every file
            let ext = &self.tree.exts[node.ext as usize];
            let name = if ext.is_empty() { format!("[{} small files]", node.files) } else { format!("[{} small {ext} files]", node.files) };
            self.tree.push(&name, node, parent);
        }
        if let Some(live) = self.live.clone() {
            self.live_update(&live, parent, &new_dirs, p.bytes, p.newest);
        }
        descend
    }

    /// Path depth of a node (drive-root children = 1).
    fn depth(&self, i: u32) -> usize {
        let mut d = self.root_depth;
        let mut c = i;
        while c != 0 && c != NONE {
            d += 1;
            c = self.tree.node(c).parent;
        }
        d
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
        super::profile::time("roll-up (folder totals)", || aggregate::roll_up(&mut tree));
        tree.cancelled = progress.cancel.load(Relaxed);
        tree.scanned_at = crate::fmt::now();
        tree.elapsed_ms = start.elapsed().as_millis() as u64;
        tree
    }
}
