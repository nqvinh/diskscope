//! The CPU part of adding one folder to the tree, with no shared state: extensions, labels and
//! small-file grouping. It runs in parallel (walker threads outside the builder lock, MFT folders
//! in parallel slices); `Builder::commit` then only pushes the nodes, in a fixed order.

use super::{entry::RawEntry, extensions::ext_of};
use crate::model::{Node, F_DIR, F_GROUP, F_REPARSE};
use crate::rules::labels::{self, Label};
use std::collections::HashMap;

/// Files below this size are grouped when a folder holds more than GROUP_MIN_COUNT of them...
const GROUP_MAX_SIZE: u64 = 64 * 1024;
const GROUP_MIN_COUNT: usize = 16;
/// ...and only extensions with at least this many small files there (a lone file stays itself).
const GROUP_MIN_PER_EXT: usize = 4;

/// What the rules need to know about the folder being added.
#[derive(Clone, Debug)]
pub struct Ctx {
    pub label: Label,
    pub name: String,
    /// Path depth of the folder's children (drive-root children = 1).
    pub depth: usize,
}

pub struct Item {
    pub name: String,
    /// Lowercase extension of a file (interned by the builder into `node.ext`).
    pub ext: Option<String>,
    pub node: Node,
    pub id: u64,
}

pub struct Group {
    pub ext: Option<String>,
    pub node: Node,
}

pub struct Prepared {
    pub items: Vec<Item>,
    /// One entry per extension of grouped small files, in order of first appearance.
    pub groups: Vec<Group>,
    /// Bytes and newest modified time of the folder's own files (for live suggestions).
    pub bytes: u64,
    pub newest: u32,
}

fn is_small(e: &RawEntry) -> bool {
    !e.is_dir() && e.size < GROUP_MAX_SIZE
}

/// A folder with many small files keeps them as one entry per extension: exact totals and
/// newest dates, a fraction of the memory (node_modules can hold millions of tiny files).
pub fn prepare(ctx: &Ctx, batch: Vec<RawEntry>) -> Prepared {
    let exts: Vec<Option<String>> = batch.iter().map(|e| if e.is_dir() { None } else { ext_of(&e.name) }).collect();
    let group = batch.iter().filter(|e| is_small(e)).count() > GROUP_MIN_COUNT;
    let mut per_ext: HashMap<&Option<String>, usize> = HashMap::new();
    if group {
        for (e, x) in batch.iter().zip(&exts) {
            if is_small(e) {
                *per_ext.entry(x).or_default() += 1;
            }
        }
    }
    let grouped: Vec<bool> = batch.iter().zip(&exts).map(|(e, x)| group && is_small(e) && per_ext.get(x).copied().unwrap_or(0) >= GROUP_MIN_PER_EXT).collect();
    // Sibling names only matter for folder rules (`target` next to Cargo.toml, ...).
    let siblings: Vec<String> = if batch.iter().any(|e| e.is_dir()) { batch.iter().map(|e| e.name.to_lowercase()).collect() } else { Vec::new() };
    let mut out = Prepared { items: Vec::with_capacity(batch.len()), groups: Vec::new(), bytes: 0, newest: 0 };
    let mut slot: HashMap<Option<String>, usize> = HashMap::new();
    for ((e, ext), grouped) in batch.into_iter().zip(exts).zip(grouped) {
        if !e.is_dir() {
            out.bytes += e.size;
            out.newest = out.newest.max(e.modified);
        }
        if grouped {
            let label = labels::file(&e.name, ctx.label) as u8;
            let k = *slot.entry(ext.clone()).or_insert_with(|| {
                out.groups.push(Group { ext, node: Node { flags: F_GROUP, label, ..Node::default() } });
                out.groups.len() - 1
            });
            let g = &mut out.groups[k].node;
            g.size += e.size;
            g.alloc += e.alloc;
            g.files += 1;
            g.modified = g.modified.max(e.modified);
            g.created = g.created.max(e.created);
            g.accessed = g.accessed.max(e.accessed);
            continue;
        }
        let label = if e.is_dir() { labels::folder(&e.name, &ctx.name, ctx.label, &siblings, ctx.depth) } else { labels::file(&e.name, ctx.label) };
        let mut node = Node { modified: e.modified, created: e.created, accessed: e.accessed, label: label as u8, ..Node::default() };
        if e.is_dir() {
            node.flags = F_DIR;
        } else {
            node.size = e.size;
            node.alloc = e.alloc;
            node.files = 1;
        }
        if e.is_reparse() {
            node.flags |= F_REPARSE;
        }
        out.items.push(Item { name: e.name, ext, node, id: e.id });
    }
    out
}
