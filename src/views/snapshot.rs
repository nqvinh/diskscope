//! Compact per-root snapshot (folders of 1 MiB or more: relative path + size) saved after each
//! full GUI scan, used by the "Grew since last scan" view. Stored in %LOCALAPPDATA%\diskscope.

use crate::model::Tree;
use std::collections::HashMap;
use std::io::{BufRead, BufWriter, Write};
use std::path::Path;

const MIN_DIR: u64 = 1 << 20;
const HEADER_V1: &str = "#diskscope-snapshot v1";
/// v2 adds the folder's label: `size \t label \t relative path`.
const HEADER: &str = "#diskscope-snapshot v2";

pub struct Snapshot {
    pub taken_at: u32,
    /// The scanned root as written in the file ("" for "This PC").
    pub root: String,
    /// Lowercase relative folder path -> size in bytes.
    pub sizes: HashMap<String, u64>,
    /// Lowercase relative path -> (path as written, label as u8; 0 for v1 files).
    pub meta: HashMap<String, (String, u8)>,
}

#[derive(Clone, Copy, Debug)]
pub struct Growth {
    pub node: u32,
    pub old: u64,
    pub new: u64,
}

/// File-name key of a scanned root: `C:\Users` -> `c_users`, "This PC" -> `all-drives`.
pub fn root_key(root: &Path) -> String {
    if root.as_os_str().is_empty() {
        return "all-drives".into();
    }
    let key: String = root.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
    key.trim_matches('_').to_string()
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
        writeln!(w, "{}\t{}\t{}", tree.node(i).size, tree.node(i).label, rel)?;
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
    let v2 = match h.next()? {
        HEADER => true,
        HEADER_V1 => false,
        _ => return None,
    };
    let taken_at = h.next()?.parse().ok()?;
    let root = h.next().unwrap_or("").to_string();
    let (mut sizes, mut meta) = (HashMap::new(), HashMap::new());
    for line in lines.map_while(Result::ok) {
        let mut f = line.splitn(if v2 { 3 } else { 2 }, '\t');
        let (Some(size), label) = (f.next().and_then(|s| s.parse::<u64>().ok()), if v2 { f.next().and_then(|l| l.parse::<u8>().ok()) } else { Some(0) }) else { continue };
        let (Some(label), Some(rel)) = (label, f.next()) else { continue };
        let key = rel.to_lowercase();
        sizes.insert(key.clone(), size);
        meta.insert(key, (rel.to_string(), label));
    }
    Some(Snapshot { taken_at, root, sizes, meta })
}

/// The scan time and root total written in a snapshot file, without reading the whole file.
pub fn peek(file: &Path) -> Option<(u32, u64)> {
    let f = std::fs::File::open(file).ok()?;
    let mut lines = std::io::BufReader::new(f).lines();
    let head = lines.next()?.ok()?;
    let taken_at = head.split('\t').nth(1)?.parse().ok()?;
    let first = lines.next()?.ok()?;
    Some((taken_at, first.split('\t').next()?.parse().ok()?))
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
    assert_eq!(peek(&file).map(|p| p.1), Some(5 * mb), "root total is the first line");
    std::fs::write(&file, "#diskscope-snapshot v1\t5\tQ:\\\n7\t\n3\tOld\\Dir\n").unwrap();
    let v1 = load(&file).expect("v1 files still load");
    assert_eq!((v1.sizes.get("old\\dir"), v1.meta.get("old\\dir").map(|m| m.0.as_str())), (Some(&3), Some("Old\\Dir")));
    std::fs::remove_file(&file).ok();
    assert_eq!(snap.sizes.get("a"), Some(&(2 * mb)));
    let after = build("Q:\\", &[("a\\x.bin", 2 * mb, 0), ("b\\y.bin", 3 * mb, 0), ("b\\c\\z.bin", 40 * mb, 0)]);
    let g = grew(&after, &snap, mb, 10);
    let paths: Vec<_> = g.iter().map(|g| after.path(g.node).display().to_string()).collect();
    assert_eq!(paths, ["Q:\\b\\c"]);
    assert_eq!(g[0].old, 0);
}
