//! Several drives in one tree: a virtual "This PC" root (empty root path) with one child per
//! scanned drive, each grafted from its own scan. Paths stay real (`""` + `C:\` + `Users`).

use super::tree::{Node, Tree, F_DIR, NONE};
use std::collections::HashMap;
use std::path::PathBuf;

pub const ALL_DRIVES: &str = "This PC";
/// Extension table limit shared with the builder (ids are u16).
const MAX_EXTS: usize = 60_000;

impl Tree {
    /// An empty multi-drive tree: just the virtual root.
    pub fn multi_root() -> Self {
        let mut t = Tree::new(PathBuf::new(), 4096);
        t.push(ALL_DRIVES, Node { flags: F_DIR, ..Node::default() }, NONE);
        t
    }

    pub fn is_multi_root(&self) -> bool {
        self.root_path.as_os_str().is_empty()
    }

    /// The root as shown to people: the scanned path, or "This PC" for several drives.
    pub fn root_display(&self) -> String {
        if self.is_multi_root() { ALL_DRIVES.to_string() } else { self.root_path.display().to_string() }
    }

    /// Appends a whole scan (`other`, a drive) under the virtual root; returns the drive's node.
    /// Children still come after their parents, so the roll-up order holds.
    pub fn graft(&mut self, other: Tree) -> u32 {
        let mut ids: HashMap<String, u16> = self.exts.iter().enumerate().map(|(i, e)| (e.clone(), i as u16)).collect();
        let ext_map: Vec<u16> = other
            .exts
            .iter()
            .map(|e| {
                if let Some(&id) = ids.get(e) {
                    return id;
                }
                if self.exts.len() >= MAX_EXTS {
                    return 0;
                }
                let id = self.exts.len() as u16;
                self.exts.push(e.clone());
                ids.insert(e.clone(), id);
                id
            })
            .collect();
        let base = self.len() as u32;
        let map = |x: u32| if x == NONE { NONE } else { base + x };
        let first_drive = self.node(0).first_child;
        let drive_name = other.root_path.display().to_string();
        for i in 0..other.len() as u32 {
            let mut n = *other.node(i);
            let (r, l) = self.names.push(if i == 0 { &drive_name } else { other.name(i) });
            (n.name_ref, n.name_len) = (r, l);
            n.parent = if i == 0 { 0 } else { map(n.parent) };
            n.first_child = map(n.first_child);
            n.next_sibling = if i == 0 { first_drive } else { map(n.next_sibling) };
            n.ext = ext_map.get(n.ext as usize).copied().unwrap_or(0);
            self.nodes.push(n);
        }
        let d = *self.node(base);
        let root = self.node_mut(0);
        root.first_child = base;
        root.size += d.size;
        root.alloc += d.alloc;
        root.files += d.files;
        root.items += d.items + 1;
        root.modified = root.modified.max(d.modified);
        root.created = root.created.max(d.created);
        root.accessed = root.accessed.max(d.accessed);
        self.denied += other.denied;
        self.denied_paths.extend(other.denied_paths);
        self.reparse_skipped += other.reparse_skipped;
        self.cancelled |= other.cancelled;
        self.volume_used = match (self.volume_used, other.volume_used, base == 1) {
            (_, Some(v), true) => Some(v),
            (Some(a), Some(b), false) => Some(a + b),
            _ => None,
        };
        if base == 1 || other.method != "ntfs-mft" {
            self.method = other.method;
        }
        if !other.method_note.is_empty() {
            let sep = if self.method_note.is_empty() { "" } else { "; " };
            self.method_note = format!("{}{sep}{}: {}", self.method_note, drive_name, other.method_note);
        }
        base
    }
}

#[test]
fn grafted_drives_keep_paths_totals_and_extensions() {
    use super::fixture::build;
    let c = build("C:\\", &[("Users\\a.png", 100, 5), ("x.txt", 10, 1)]);
    let d = build("D:\\", &[("tmp\\b.png", 1000, 9), ("tmp\\c.zip", 1, 2)]);
    let mut all = Tree::multi_root();
    all.graft(c);
    all.graft(d);
    assert!(all.is_multi_root() && all.root_display() == ALL_DRIVES);
    let r = all.node(0);
    assert_eq!((r.size, r.files, r.modified), (1111, 4, 9));
    let png = all.find(std::path::Path::new("D:\\tmp\\b.png")).expect("find through the drive node");
    assert_eq!(all.path(png), PathBuf::from("D:\\tmp\\b.png"));
    assert_eq!(all.ext(png), ".png");
    assert_eq!(all.ext(all.find(std::path::Path::new("c:\\users\\a.png")).unwrap()), ".png", "extension ids remapped");
    let drives: Vec<_> = all.children(0).map(|i| all.name(i).to_string()).collect();
    assert_eq!(drives, ["D:\\", "C:\\"], "the last graft is listed first (scan_many grafts in reverse)");
    for i in 1..all.len() as u32 {
        assert!(all.node(i).parent < i, "children come after their parents");
    }
}
