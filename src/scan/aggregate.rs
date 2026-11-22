//! Folder totals: one reverse pass over the arena (children always come after their parent).

use crate::model::{Tree, NONE};

pub fn roll_up(tree: &mut Tree) {
    for i in (1..tree.len() as u32).rev() {
        let c = *tree.node(i);
        if c.parent == NONE {
            continue;
        }
        let p = tree.node_mut(c.parent);
        p.size += c.size;
        p.alloc += c.alloc;
        p.files += c.files;
        p.items += c.items + 1;
        p.modified = p.modified.max(c.modified);
        p.created = p.created.max(c.created);
        p.accessed = p.accessed.max(c.accessed);
    }
}

/// Visits `root` and every node below it that is still attached (deleted subtrees are skipped).
pub fn for_each_below(tree: &Tree, root: u32, mut f: impl FnMut(u32)) {
    let mut stack = vec![root];
    while let Some(i) = stack.pop() {
        f(i);
        stack.extend(tree.children(i));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Node, F_DIR};
    use std::path::PathBuf;

    fn file(size: u64, modified: u32) -> Node {
        Node { size, alloc: size.div_ceil(4096) * 4096, files: 1, modified, ..Node::default() }
    }

    /// root/{a.txt 100, sub/{b.bin 5000, c.bin 10}}
    fn sample() -> Tree {
        let mut t = Tree::new(PathBuf::from("X:\\data"), 4096);
        let root = t.push("X:\\data", Node { flags: F_DIR, ..Node::default() }, NONE);
        t.push("a.txt", file(100, 10), root);
        let sub = t.push("sub", Node { flags: F_DIR, modified: 5, ..Node::default() }, root);
        t.push("b.bin", file(5000, 50), sub);
        t.push("c.bin", file(10, 20), sub);
        roll_up(&mut t);
        t
    }

    #[test]
    fn folder_totals_and_newest_times() {
        let t = sample();
        let root = t.node(0);
        assert_eq!(root.size, 5110);
        assert_eq!(root.alloc, 4096 + 8192 + 4096);
        assert_eq!(root.files, 3);
        assert_eq!(root.items, 4);
        assert_eq!(root.modified, 50);
        let sub = t.find(std::path::Path::new("X:\\data\\SUB")).unwrap();
        assert_eq!(t.node(sub).size, 5010);
        assert_eq!(t.node(sub).items, 2);
        assert_eq!(t.path(t.find(std::path::Path::new("sub\\b.bin")).unwrap()), PathBuf::from("X:\\data\\sub\\b.bin"));
    }

    #[test]
    fn removing_a_subtree_updates_ancestors() {
        let mut t = sample();
        let sub = t.find(std::path::Path::new("sub")).unwrap();
        t.remove_subtree(sub);
        let root = t.node(0);
        assert_eq!((root.size, root.files, root.items), (100, 1, 1));
        assert!(t.find(std::path::Path::new("sub")).is_none());
        let mut seen = 0;
        for_each_below(&t, 0, |_| seen += 1);
        assert_eq!(seen, 2);
    }
}
