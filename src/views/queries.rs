//! Read-only queries behind the Recent, Least used and Top views (GUI and CLI share them).
//!
//! Folder lists use a "frontier" rule so the same bytes are not listed again for every parent:
//! a folder is listed when no single sub-folder holds 90% or more of its matching bytes.

use crate::model::{Node, Tree};
use crate::scan::aggregate::for_each_below;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeField {
    Modified,
    Created,
    /// Newest of created and modified (copied-in files keep an old modified date).
    Changed,
    Accessed,
}

impl TimeField {
    pub fn of(self, n: &Node) -> u32 {
        match self {
            TimeField::Modified => n.modified,
            TimeField::Created => n.created,
            TimeField::Changed => n.modified.max(n.created),
            TimeField::Accessed => n.accessed,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub node: u32,
    pub bytes: u64,
}

/// Files under `root` matching `pred`, biggest first.
pub fn files_where(tree: &Tree, root: u32, limit: usize, pred: impl Fn(&Node) -> bool) -> Vec<Hit> {
    let mut hits = Vec::new();
    for_each_below(tree, root, |i| {
        let n = tree.node(i);
        if !n.is_dir() && pred(n) {
            hits.push(Hit { node: i, bytes: n.size });
        }
    });
    top(hits, limit)
}

/// Folders under `root` where bytes of files matching `pred` concentrate, biggest first.
pub fn folders_where(tree: &Tree, root: u32, limit: usize, min_bytes: u64, pred: impl Fn(&Node) -> bool) -> Vec<Hit> {
    let mut order = Vec::new();
    for_each_below(tree, root, |i| order.push(i));
    let mut pos = vec![u32::MAX; tree.len()];
    for (k, &i) in order.iter().enumerate() {
        pos[i as usize] = k as u32;
    }
    let mut bytes = vec![0u64; order.len()];
    let mut biggest_child = vec![0u64; order.len()];
    for k in (0..order.len()).rev() {
        let n = tree.node(order[k]);
        if !n.is_dir() && pred(n) {
            bytes[k] += n.size;
        }
        if k > 0 && n.parent != crate::model::NONE && pos[n.parent as usize] != u32::MAX {
            let pk = pos[n.parent as usize] as usize;
            {
                bytes[pk] += bytes[k];
                if n.is_dir() {
                    biggest_child[pk] = biggest_child[pk].max(bytes[k]);
                }
            }
        }
    }
    let mut hits = Vec::new();
    for (k, &i) in order.iter().enumerate() {
        let b = bytes[k];
        if tree.node(i).is_dir() && b >= min_bytes.max(1) && (biggest_child[k] as f64) < 0.9 * b as f64 {
            hits.push(Hit { node: i, bytes: b });
        }
    }
    top(hits, limit)
}

/// Every folder from 1 to `depth` levels below `root`, biggest first (like `du --max-depth`).
pub fn folders_to_depth(tree: &Tree, root: u32, depth: usize, limit: usize) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut stack = vec![(root, 0usize)];
    while let Some((i, d)) = stack.pop() {
        if d > 0 {
            hits.push(Hit { node: i, bytes: tree.node(i).size });
        }
        if d < depth {
            stack.extend(tree.children(i).filter(|&c| tree.node(c).is_dir()).map(|c| (c, d + 1)));
        }
    }
    top(hits, limit)
}

fn top(mut hits: Vec<Hit>, limit: usize) -> Vec<Hit> {
    hits.sort_unstable_by(|a, b| b.bytes.cmp(&a.bytes).then(a.node.cmp(&b.node)));
    hits.truncate(limit);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::fixture::build;

    fn names(t: &Tree, hits: &[Hit]) -> Vec<String> {
        hits.iter().map(|h| t.path(h.node).display().to_string()).collect()
    }

    #[test]
    fn recent_files_and_growth_frontier() {
        let t = build(
            "D:\\",
            &[
                ("a\\b\\new1.bin", 900, 100),
                ("a\\b\\new2.bin", 50, 100),
                ("a\\old.bin", 5000, 1),
                ("c\\new3.bin", 400, 100),
                ("c\\d\\new4.bin", 300, 100),
            ],
        );
        let since = 50;
        let files = files_where(&t, 0, 10, |n| TimeField::Modified.of(n) >= since);
        assert_eq!(names(&t, &files), ["D:\\a\\b\\new1.bin", "D:\\c\\new3.bin", "D:\\c\\d\\new4.bin", "D:\\a\\b\\new2.bin"]);
        let dirs = folders_where(&t, 0, 10, 1, |n| TimeField::Modified.of(n) >= since);
        // a holds 950 recent bytes all inside a\b -> only a\b is listed; c is split 400/300 -> c listed, and c\d too.
        assert_eq!(names(&t, &dirs), ["D:\\", "D:\\a\\b", "D:\\c", "D:\\c\\d"]);
        assert_eq!(dirs[1].bytes, 950);
    }

    #[test]
    fn least_used_by_access_time() {
        let t = build("D:\\", &[("x\\cold.bin", 10, 5), ("x\\warm.bin", 99, 500)]);
        let cold = files_where(&t, 0, 10, |n| TimeField::Accessed.of(n) < 100);
        assert_eq!(names(&t, &cold), ["D:\\x\\cold.bin"]);
    }
}
