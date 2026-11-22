//! Compact per-root snapshot (folders of 1 MiB or more: relative path + size) saved after each
//! full GUI scan, used by the "Grew since last scan" view. Stored in %LOCALAPPDATA%\diskscope.

use crate::model::Tree;
use std::collections::HashMap;
use std::io::{BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};

const MIN_DIR: u64 = 1 << 20;
const HEADER: &str = "#diskscope-snapshot v1";

pub struct Snapshot {
    pub taken_at: u32,
    /// Lowercase relative folder path -> size in bytes.
    pub sizes: HashMap<String, u64>,
}

#[derive(Clone, Copy, Debug)]
pub struct Growth {
    pub node: u32,
    pub old: u64,
    pub new: u64,
}

pub fn snapshot_file(root: &Path) -> PathBuf {
    if root.as_os_str().is_empty() {
        return crate::sys::data_dir().join("snapshots").join("all-drives.tsv"); // "This PC" scans
    }
    let key: String = root
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    crate::sys::data_dir().join("snapshots").join(format!("{}.tsv", key.trim_matches('_')))
}

/// Folders of 1 MiB+ with their relative paths (the root is "").
fn big_dirs(tree: &Tree) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let mut stack = vec![(0u32, String::new())];
    while let Some((i, rel)) = stack.pop() {
        for c in tree.children(i) {
            let n = tree.node(c);
            if n.is_dir() && n.size >= MIN_DIR {
                let r = if rel.is_empty() { tree.name(c).to_string() } else { format!("{rel}\\{}", tree.name(c)) };
                stack.push((c, r));
            }
        }
        out.push((i, rel));
    }
    out
}

pub fn save(tree: &Tree, file: &Path) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension("tmp");
    let mut w = BufWriter::new(std::fs::File::create(&tmp)?);
    writeln!(w, "{HEADER}\t{}\t{}", tree.scanned_at, tree.root_path.display())?;
    for (i, rel) in big_dirs(tree) {
        writeln!(w, "{}\t{}", tree.node(i).size, rel)?;
    }
    w.flush()?;
    drop(w);
    std::fs::rename(&tmp, file)
}

pub fn load(file: &Path) -> Option<Snapshot> {
    let f = std::fs::File::open(file).ok()?;
    let mut lines = std::io::BufReader::new(f).lines();
    let head = lines.next()?.ok()?;
    let mut h = head.split('\t');
    if h.next()? != HEADER {
        return None;
    }
    let taken_at = h.next()?.parse().ok()?;
    let mut sizes = HashMap::new();
    for line in lines.map_while(Result::ok) {
        if let Some((size, rel)) = line.split_once('\t') {
            if let Ok(s) = size.parse() {
                sizes.insert(rel.to_lowercase(), s);
            }
        }
    }
    Some(Snapshot { taken_at, sizes })
}

/// Folders that grew by at least `min_delta` since `prev`, using the same frontier rule as the
/// other folder views (a folder is skipped when one sub-folder explains 90%+ of its growth).
pub fn grew(tree: &Tree, prev: &Snapshot, min_delta: u64, limit: usize) -> Vec<Growth> {
    let dirs = big_dirs(tree);
    let mut delta: HashMap<u32, (u64, u64)> = HashMap::with_capacity(dirs.len());
    for (i, rel) in &dirs {
        let old = prev.sizes.get(&rel.to_lowercase()).copied().unwrap_or(0);
        delta.insert(*i, (old, tree.node(*i).size));
    }
    let gain = |i: u32| delta.get(&i).map(|&(o, n)| n.saturating_sub(o)).unwrap_or(0);
    let mut out: Vec<Growth> = dirs
        .iter()
        .filter_map(|&(i, _)| {
            let g = gain(i);
            let biggest = tree.children(i).map(gain).max().unwrap_or(0);
            (g >= min_delta.max(1) && (biggest as f64) < 0.9 * g as f64).then(|| {
                let (old, new) = delta[&i];
                Growth { node: i, old, new }
            })
        })
        .collect();
    out.sort_by(|a, b| (b.new - b.old).cmp(&(a.new - a.old)));
    out.truncate(limit);
    out
}

#[test]
fn snapshot_round_trip_and_growth() {
    use crate::model::fixture::build;
    let mb = 1u64 << 20;
    let before = build("Q:\\", &[("a\\x.bin", 2 * mb, 0), ("b\\y.bin", 3 * mb, 0)]);
    let file = std::env::temp_dir().join(format!("diskscope-test-{}.tsv", std::process::id()));
    save(&before, &file).unwrap();
    let snap = load(&file).unwrap();
    std::fs::remove_file(&file).ok();
    assert_eq!(snap.sizes.get("a"), Some(&(2 * mb)));
    let after = build("Q:\\", &[("a\\x.bin", 2 * mb, 0), ("b\\y.bin", 3 * mb, 0), ("b\\c\\z.bin", 40 * mb, 0)]);
    let g = grew(&after, &snap, mb, 10);
    let paths: Vec<_> = g.iter().map(|g| after.path(g.node).display().to_string()).collect();
    assert_eq!(paths, ["Q:\\b\\c"]);
    assert_eq!(g[0].old, 0);
}
