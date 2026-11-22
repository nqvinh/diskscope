//! Scanned file tree in one arena. Children always have a higher index than their parent, so a
//! reverse index pass is a post-order roll-up. 64 bytes per node + names in a shared store.

use super::{chunk_vec::ChunkVec, names::NameStore};
use std::path::{Path, PathBuf};

pub const NONE: u32 = u32::MAX;
pub const F_DIR: u8 = 1;
pub const F_REPARSE: u8 = 2;
pub const F_DENIED: u8 = 4;
/// Many small files of one extension in one folder, kept as a single entry (not a real path).
pub const F_GROUP: u8 = 8;
/// Marked as a live cleanup suggestion while the scan runs.
pub const F_CANDIDATE: u8 = 16;

#[derive(Clone, Copy, Default, Debug)]
pub struct Node {
    pub name_ref: u32,
    pub name_len: u16,
    /// Extension id into `Tree::exts` (0 = no extension). Files only.
    pub ext: u16,
    pub parent: u32,
    pub first_child: u32,
    pub next_sibling: u32,
    /// Logical bytes (folders: sum of the subtree after the roll-up).
    pub size: u64,
    /// Bytes on disk (cluster-rounded estimate; folders: subtree sum).
    pub alloc: u64,
    /// Unix seconds. Folders: newest of the folder itself and everything under it.
    pub modified: u32,
    pub created: u32,
    pub accessed: u32,
    /// Files under a folder (a file counts itself as 1).
    pub files: u32,
    /// Files and folders under a folder (0 for a file).
    pub items: u32,
    pub flags: u8,
}

impl Node {
    pub fn is_dir(&self) -> bool {
        self.flags & F_DIR != 0
    }
    pub fn is_reparse(&self) -> bool {
        self.flags & F_REPARSE != 0
    }
    pub fn is_denied(&self) -> bool {
        self.flags & F_DENIED != 0
    }
    pub fn is_group(&self) -> bool {
        self.flags & F_GROUP != 0
    }
}

pub struct Tree {
    pub nodes: ChunkVec<Node>,
    pub names: NameStore,
    /// Extension strings, lowercase with the dot; index 0 = "" (no extension).
    pub exts: Vec<String>,
    pub root_path: PathBuf,
    pub cluster: u64,
    pub denied_paths: Vec<String>,
    pub denied: u64,
    pub reparse_skipped: u64,
    pub scanned_at: u32,
    pub elapsed_ms: u64,
    pub cancelled: bool,
    /// "ntfs-mft" (fast, admin) or "walker".
    pub method: &'static str,
    /// Why the fast path was not used (empty when it was).
    pub method_note: String,
    /// Used bytes of the whole volume (MFT scans of a drive root), for comparison with the scan.
    pub volume_used: Option<u64>,
}

impl Tree {
    pub fn new(root_path: PathBuf, cluster: u64) -> Self {
        Self {
            nodes: ChunkVec::default(),
            names: NameStore::default(),
            exts: vec![String::new()],
            root_path,
            cluster: cluster.max(1),
            denied_paths: Vec::new(),
            denied: 0,
            reparse_skipped: 0,
            scanned_at: 0,
            elapsed_ms: 0,
            cancelled: false,
            method: "walker",
            method_note: String::new(),
            volume_used: None,
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn node(&self, i: u32) -> &Node {
        &self.nodes[i as usize]
    }

    pub fn node_mut(&mut self, i: u32) -> &mut Node {
        &mut self.nodes[i as usize]
    }

    /// Adds a node as the first child of `parent` (or the root when `parent == NONE`).
    pub fn push(&mut self, name: &str, mut node: Node, parent: u32) -> u32 {
        let (r, l) = self.names.push(name);
        node.name_ref = r;
        node.name_len = l;
        node.parent = parent;
        node.first_child = NONE;
        node.next_sibling = if parent == NONE { NONE } else { self.node(parent).first_child };
        let idx = self.nodes.push(node) as u32;
        if parent != NONE {
            self.node_mut(parent).first_child = idx;
        }
        idx
    }

    pub fn name(&self, i: u32) -> &str {
        let n = self.node(i);
        self.names.get(n.name_ref, n.name_len)
    }

    pub fn ext(&self, i: u32) -> &str {
        &self.exts[self.node(i).ext as usize]
    }

    pub fn children(&self, i: u32) -> Children<'_> {
        Children { tree: self, next: self.node(i).first_child }
    }

    /// Full path of a node (the root node's name is the scanned path itself).
    pub fn path(&self, i: u32) -> PathBuf {
        let mut parts = Vec::new();
        let mut c = i;
        while c != 0 && c != NONE {
            parts.push(self.name(c));
            c = self.node(c).parent;
        }
        let mut p = self.root_path.clone();
        for s in parts.iter().rev() {
            p.push(s);
        }
        p
    }

    /// True when `ancestor` is `i` or one of its parents.
    pub fn is_within(&self, i: u32, ancestor: u32) -> bool {
        let mut c = i;
        while c != NONE {
            if c == ancestor {
                return true;
            }
            c = self.node(c).parent;
        }
        false
    }

    /// Finds a node by absolute path (or a path relative to the root), case-insensitively.
    pub fn find(&self, path: &Path) -> Option<u32> {
        let rel = strip_prefix_ci(path, &self.root_path).unwrap_or(path);
        let mut cur = 0u32;
        for comp in rel.components() {
            let want = comp.as_os_str().to_string_lossy().to_lowercase();
            if want.is_empty() || want == "\\" || want == "." {
                continue;
            }
            cur = self.children(cur).find(|&c| self.name(c).to_lowercase() == want)?;
        }
        Some(cur)
    }

    /// Detaches a node after it was deleted and subtracts its totals from every ancestor.
    pub fn remove_subtree(&mut self, i: u32) {
        if i == 0 {
            return;
        }
        let n = *self.node(i);
        let mut p = n.parent;
        while p != NONE {
            let a = self.node_mut(p);
            a.size = a.size.saturating_sub(n.size);
            a.alloc = a.alloc.saturating_sub(n.alloc);
            a.files = a.files.saturating_sub(n.files);
            a.items = a.items.saturating_sub(n.items + 1);
            p = a.parent;
        }
        let parent = n.parent;
        if self.node(parent).first_child == i {
            self.node_mut(parent).first_child = n.next_sibling;
        } else {
            let mut c = self.node(parent).first_child;
            while c != NONE {
                if self.node(c).next_sibling == i {
                    self.node_mut(c).next_sibling = n.next_sibling;
                    break;
                }
                c = self.node(c).next_sibling;
            }
        }
    }

    /// Approximate memory held by the tree.
    pub fn heap_bytes(&self) -> usize {
        self.nodes.heap_bytes() + self.names.heap_bytes()
    }
}

fn strip_prefix_ci<'a>(path: &'a Path, root: &Path) -> Option<&'a Path> {
    let p = path.to_string_lossy().to_lowercase().replace('/', "\\");
    let r = root.to_string_lossy().to_lowercase().replace('/', "\\");
    let r = r.trim_end_matches('\\');
    if p == r {
        return Some(Path::new(""));
    }
    let rest = p.strip_prefix(r)?.strip_prefix('\\')?;
    let start = path.as_os_str().len().checked_sub(rest.len())?;
    path.to_str().and_then(|s| s.get(start..)).map(Path::new)
}

pub struct Children<'a> {
    tree: &'a Tree,
    next: u32,
}

impl Iterator for Children<'_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        if self.next == NONE {
            return None;
        }
        let c = self.next;
        self.next = self.tree.node(c).next_sibling;
        Some(c)
    }
}
