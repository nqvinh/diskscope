//! Test helper: builds a rolled-up tree from relative paths. A trailing `\` marks a folder.

use super::{Node, Tree, F_DIR, NONE};
use std::path::PathBuf;

pub fn build(root: &str, entries: &[(&str, u64, u32)]) -> Tree {
    let mut t = Tree::new(PathBuf::from(root), 4096);
    t.push(root, Node { flags: F_DIR, ..Node::default() }, NONE);
    for &(path, size, modified) in entries {
        let is_dir = path.ends_with('\\');
        let parts: Vec<&str> = path.trim_end_matches('\\').split('\\').collect();
        let mut cur = 0u32;
        for (k, part) in parts.iter().enumerate() {
            let last = k + 1 == parts.len();
            let existing = t.children(cur).find(|&c| t.name(c) == *part);
            cur = match existing {
                Some(c) => c,
                None if last && !is_dir => {
                    let node = Node { size, alloc: size.div_ceil(4096) * 4096, files: 1, modified, created: modified, accessed: modified, ..Node::default() };
                    let ext = crate::scan::extensions::ext_of(part).map(|e| {
                        t.exts.iter().position(|x| *x == e).unwrap_or_else(|| { t.exts.push(e); t.exts.len() - 1 })
                    });
                    let i = t.push(part, node, cur);
                    t.node_mut(i).ext = ext.unwrap_or(0) as u16;
                    i
                }
                None => t.push(part, Node { flags: F_DIR, modified, ..Node::default() }, cur),
            };
        }
    }
    crate::scan::aggregate::roll_up(&mut t);
    t
}
