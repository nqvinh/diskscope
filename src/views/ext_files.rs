//! Extension drill-down: an index of file node ids per extension (built once per scan/refresh, no
//! paths stored) and the filtered, sorted file list for one or more selected extensions.
//! Grouped small files ("[500 small .js files]") appear as their group row, so counts and sizes
//! always match the extension table.

use super::sorting::{sort_list, SortKey, SortState};
use crate::model::Tree;
use crate::scan::aggregate::for_each_below;

pub struct ExtIndex {
    /// Node ids of files (and small-file groups) per extension id.
    pub by_ext: Vec<Vec<u32>>,
}

pub fn build_index(tree: &Tree) -> ExtIndex {
    let mut by_ext = vec![Vec::new(); tree.exts.len()];
    for_each_below(tree, 0, |i| {
        let n = tree.node(i);
        if !n.is_dir() {
            if let Some(v) = by_ext.get_mut(n.ext as usize) {
                v.push(i);
            }
        }
    });
    ExtIndex { by_ext }
}

pub struct Query<'a> {
    pub exts: &'a [u16],
    /// Case-insensitive substring of the name or the full path ("" = all).
    pub filter: &'a str,
    pub min_size: u64,
    pub sort: SortState,
}

/// Files of the selected extensions that pass the filters, in the requested order.
pub fn select(tree: &Tree, idx: &ExtIndex, q: &Query) -> Vec<u32> {
    let needle = q.filter.trim().to_lowercase();
    let mut rows: Vec<u32> = q
        .exts
        .iter()
        .filter_map(|&e| idx.by_ext.get(e as usize))
        .flatten()
        .copied()
        .filter(|&i| tree.node(i).size >= q.min_size)
        .filter(|&i| {
            needle.is_empty()
                || tree.name(i).to_lowercase().contains(&needle)
                || tree.path(i).to_string_lossy().to_lowercase().contains(&needle)
        })
        .collect();
    let d = q.sort.desc;
    let name = |i: &u32| tree.name(*i).to_lowercase();
    match q.sort.key {
        SortKey::Name => sort_list(&mut rows, d, name, |i| *i),
        SortKey::Modified => sort_list(&mut rows, d, |i| tree.node(*i).modified, name),
        SortKey::Accessed => sort_list(&mut rows, d, |i| tree.node(*i).accessed, name),
        SortKey::Label => sort_list(&mut rows, d, |i| crate::rules::labels::Label::from_u8(tree.node(*i).label).name(), |i| std::cmp::Reverse(tree.node(*i).size)),
        _ => sort_list(&mut rows, d, |i| tree.node(*i).size, name),
    }
    rows
}

/// (files, bytes) of a result list (a group row counts all its files).
pub fn totals(tree: &Tree, rows: &[u32]) -> (u64, u64) {
    rows.iter().fold((0, 0), |(f, b), &i| (f + tree.node(i).files as u64, b + tree.node(i).size))
}

/// Extension ids whose files get `label` by extension (e.g. Video = .mp4 .mkv .mov ...).
pub fn exts_of_group(tree: &Tree, label: crate::rules::labels::Label) -> Vec<u16> {
    (0..tree.exts.len() as u16)
        .filter(|&e| !tree.exts[e as usize].is_empty())
        .filter(|&e| crate::rules::labels::file(&format!("x{}", tree.exts[e as usize]), crate::rules::labels::Label::Other) == label)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::fixture::build;
    use crate::rules::labels::Label;

    fn sample() -> Tree {
        build("X:\\", &[("v\\a.mp4", 500, 3), ("v\\b.MKV", 300, 1), ("v\\c.mp4", 100, 2), ("d\\r.pdf", 50, 5), ("d\\holiday.mp4", 700, 4)])
    }

    fn ext(t: &Tree, e: &str) -> u16 {
        t.exts.iter().position(|x| x == e).unwrap() as u16
    }

    fn names(t: &Tree, v: &[u32]) -> Vec<String> {
        v.iter().map(|&i| t.name(i).to_string()).collect()
    }

    #[test]
    fn index_matches_extension_table_totals() {
        let t = sample();
        let idx = build_index(&t);
        for s in crate::scan::extensions::ext_stats(&t, 0) {
            let rows = &idx.by_ext[s.ext as usize];
            assert_eq!(totals(&t, rows), (s.files, s.size), "extension {}", t.exts[s.ext as usize]);
        }
    }

    #[test]
    fn union_filter_and_sort() {
        let t = sample();
        let idx = build_index(&t);
        let (mp4, mkv) = (ext(&t, ".mp4"), ext(&t, ".mkv"));
        let q = |exts: &[u16], filter: &str, min: u64, sort: SortState| select(&t, &idx, &Query { exts, filter, min_size: min, sort });
        let all = q(&[mp4, mkv], "", 0, SortState::default());
        assert_eq!(names(&t, &all), ["holiday.mp4", "a.mp4", "b.MKV", "c.mp4"], "union, size desc");
        assert_eq!(totals(&t, &all), (4, 1600));
        assert_eq!(names(&t, &q(&[mp4], "HOLI", 0, SortState::default())), ["holiday.mp4"], "name filter, any case");
        assert_eq!(names(&t, &q(&[mp4], "\\v\\", 0, SortState::default())), ["a.mp4", "c.mp4"], "path filter");
        assert_eq!(names(&t, &q(&[mp4, mkv], "", 300, SortState::default())), ["holiday.mp4", "a.mp4", "b.MKV"], "min size");
        let by_date = SortState { key: SortKey::Modified, desc: false };
        assert_eq!(names(&t, &q(&[mp4, mkv], "", 0, by_date)), ["b.MKV", "c.mp4", "a.mp4", "holiday.mp4"], "oldest first");
        assert_eq!(exts_of_group(&t, Label::Video).len(), 2, "Video group = .mp4 + .mkv");
    }
}
