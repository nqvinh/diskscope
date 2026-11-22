//! Sorting for every list: tree children are sorted one level at a time (never the whole volume),
//! ties are broken by name so the order is stable, and flat lists share one generic sorter.

use crate::model::Tree;
use std::cmp::Ordering;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortKey {
    Name,
    Size,
    Alloc,
    Items,
    Files,
    Modified,
    Accessed,
    Created,
    Label,
    /// Ranked order (reclaimable size x confidence) of suggestion lists.
    Score,
}

impl SortKey {
    /// Ascending is the natural first click for text columns; numbers and dates start descending.
    pub fn default_desc(self) -> bool {
        !matches!(self, SortKey::Name | SortKey::Label)
    }
}

/// Header-click state shared by all tables: same key flips the direction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SortState {
    pub key: SortKey,
    pub desc: bool,
}

impl Default for SortState {
    fn default() -> Self {
        Self { key: SortKey::Size, desc: true }
    }
}

impl SortState {
    pub fn click(&mut self, key: SortKey) {
        if self.key == key {
            self.desc = !self.desc;
        } else {
            self.key = key;
            self.desc = key.default_desc();
        }
    }
}

/// Children of `i` in the requested order (one level only).
pub fn sorted_children(tree: &Tree, i: u32, s: SortState) -> Vec<u32> {
    let mut kids: Vec<u32> = tree.children(i).collect();
    let num = |c: u32| -> u64 {
        let n = tree.node(c);
        match s.key {
            SortKey::Size | SortKey::Name => n.size,
            SortKey::Alloc => n.alloc,
            SortKey::Items => n.items as u64,
            SortKey::Files => n.files as u64,
            SortKey::Modified => n.modified as u64,
            SortKey::Accessed => n.accessed as u64,
            SortKey::Created => n.created as u64,
            SortKey::Label => n.label as u64,
            SortKey::Score => n.size,
        }
    };
    let names: Vec<String> = kids.iter().map(|&c| tree.name(c).to_lowercase()).collect();
    let mut idx: Vec<usize> = (0..kids.len()).collect();
    idx.sort_by(|&a, &b| {
        let primary = if s.key == SortKey::Name {
            names[a].cmp(&names[b])
        } else if s.key == SortKey::Label {
            crate::rules::labels::Label::from_u8(num(kids[a]) as u8).name().cmp(crate::rules::labels::Label::from_u8(num(kids[b]) as u8).name())
        } else {
            num(kids[a]).cmp(&num(kids[b]))
        };
        let primary = if s.desc { primary.reverse() } else { primary };
        primary.then_with(|| names[a].cmp(&names[b]))
    });
    kids = idx.into_iter().map(|k| kids[k]).collect();
    kids
}

/// Sorts any list by a key; `desc` flips; ties keep the tie-breaker order.
pub fn sort_list<T, K: Ord, B: Ord>(list: &mut [T], desc: bool, key: impl Fn(&T) -> K, tie: impl Fn(&T) -> B) {
    list.sort_by(|a, b| {
        let o = key(a).cmp(&key(b));
        let o = if desc { o.reverse() } else { o };
        if o == Ordering::Equal { tie(a).cmp(&tie(b)) } else { o }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::fixture::build;

    fn names(t: &Tree, v: &[u32]) -> Vec<String> {
        v.iter().map(|&c| t.name(c).to_string()).collect()
    }

    #[test]
    fn sorts_children_asc_desc_with_name_ties() {
        let t = build("X:\\", &[("b.bin", 10, 3), ("a.bin", 10, 1), ("c.bin", 30, 2), ("D.bin", 5, 9)]);
        let mut s = SortState::default();
        assert_eq!(names(&t, &sorted_children(&t, 0, s)), ["c.bin", "a.bin", "b.bin", "D.bin"], "size desc, ties by name");
        s.click(SortKey::Size);
        assert_eq!(names(&t, &sorted_children(&t, 0, s)), ["D.bin", "a.bin", "b.bin", "c.bin"], "second click flips; ties still by name");
        s.click(SortKey::Name);
        assert!(!s.desc);
        assert_eq!(names(&t, &sorted_children(&t, 0, s)), ["a.bin", "b.bin", "c.bin", "D.bin"], "name asc, case-insensitive");
        s.click(SortKey::Modified);
        assert_eq!(names(&t, &sorted_children(&t, 0, s)), ["D.bin", "b.bin", "c.bin", "a.bin"], "newest first");
    }

    #[test]
    fn generic_list_sort_with_ties() {
        let mut v = vec![("x", 2), ("a", 1), ("m", 2)];
        sort_list(&mut v, true, |r| r.1, |r| r.0);
        assert_eq!(v, [("m", 2), ("x", 2), ("a", 1)]);
        sort_list(&mut v, false, |r| r.1, |r| r.0);
        assert_eq!(v, [("a", 1), ("m", 2), ("x", 2)]);
    }
}
