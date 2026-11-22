//! Table -> tree tests: parent links, sub-folder roots, reparse folders, orphans and grouping.

use super::*;
use crate::scan::mft::record::R_HAS_DATA;

struct T {
    recs: Vec<Rec>,
    names: String,
}

impl T {
    fn new() -> Self {
        Self::with(64)
    }
    fn with(records: usize) -> Self {
        let mut t = T { recs: vec![Rec::default(); records], names: String::new() };
        t.set(ROOT_RECORD, ROOT_RECORD, ".", true, 0, 0);
        t
    }
    fn set(&mut self, i: u32, parent: u32, name: &str, dir: bool, size: u64, attrs: u32) {
        let off = self.names.len() as u32;
        self.names.push_str(name);
        self.recs[i as usize] = Rec {
            parent,
            name_off: off,
            name_len: name.len() as u16,
            name_rank: 3,
            flags: R_IN_USE | R_HAS_SI | R_HAS_DATA | if dir { R_DIR } else { 0 },
            size,
            alloc: size.div_ceil(4096) * 4096,
            modified: 1_760_000_000 + i,
            attrs,
            ..Rec::default()
        };
    }
    fn table(self) -> Table {
        Table { recs: self.recs, names: vec![self.names] }
    }
}

fn sample() -> Table {
    let mut t = T::new();
    t.set(16, 5, "Users", true, 0, 0);
    t.set(17, 16, "alice", true, 0, 0);
    t.set(18, 17, "video.mp4", false, 5_000_000, 0x20);
    t.set(19, 17, "link", true, 0, 0x400); // junction: listed, never descended
    t.set(20, 19, "behind-link.bin", false, 9_999, 0);
    t.set(21, 5, "$MFT", false, 1 << 20, 0x6);
    t.set(22, 999, "orphan", false, 1, 0); // parent outside the table
    for k in 0..20u32 {
        t.set(30 + k, 17, &format!("s{k}.js"), false, 100, 0);
    }
    t.table()
}

#[test]
fn builds_the_volume_tree_from_parent_links() {
    let tree = build(&sample(), Path::new(""), PathBuf::from("X:\\"), 4096, &Progress::default(), Instant::now()).unwrap();
    let r = tree.node(0);
    assert_eq!(r.size, 5_000_000 + (1 << 20) + 2000, "orphan and the file behind the junction are not counted");
    assert_eq!(r.files, 22);
    assert_eq!(tree.reparse_skipped, 1);
    assert!(tree.find(Path::new("Users\\alice\\video.mp4")).is_some());
    assert!(tree.find(Path::new("Users\\alice\\[20 small .js files]")).is_some(), "same grouping as the walker");
    let link = tree.find(Path::new("Users\\alice\\link")).unwrap();
    assert!(tree.node(link).is_reparse() && tree.node(link).first_child == crate::model::NONE);
}

#[test]
fn parallel_build_matches_serial_node_for_node() {
    // 5000 folders in one level (more than one slice), each with files, sub-folders and rule names.
    let n = 5000u32;
    let mut t = T::with(16 + n as usize * 4);
    t.set(15, 5, "projects", true, 0, 0);
    for k in 0..n {
        let base = 16 + k * 4;
        t.set(base, 15, &format!("p{k}"), true, 0, 0);
        t.set(base + 1, base, if k % 3 == 0 { "node_modules" } else { "src" }, true, 0, 0);
        t.set(base + 2, base, "Cargo.toml", false, 300 + k as u64, 0);
        t.set(base + 3, base + 1, &format!("f{k}.png"), false, 70_000 + k as u64, 0);
    }
    let table = t.table();
    let run = |par| build_with(&table, Path::new(""), PathBuf::from("X:\\"), 4096, &Progress::default(), Instant::now(), par).unwrap();
    let (a, b) = (run(false), run(true));
    assert_eq!(a.len(), b.len());
    for i in 0..a.len() as u32 {
        let (x, y) = (a.node(i), b.node(i));
        assert_eq!((a.name(i), x.parent, x.size, x.label, x.flags, x.ext), (b.name(i), y.parent, y.size, y.label, y.flags, y.ext), "node {i}");
    }
    assert_eq!(a.exts, b.exts);
}

#[test]
fn sub_folder_roots_and_missing_folders() {
    let tree = build(&sample(), Path::new("users\\ALICE"), PathBuf::from("X:\\Users\\alice"), 4096, &Progress::default(), Instant::now()).unwrap();
    assert_eq!(tree.node(0).size, 5_000_000 + 2000);
    assert_eq!(tree.path(tree.find(Path::new("video.mp4")).unwrap()), PathBuf::from("X:\\Users\\alice\\video.mp4"));
    assert!(build(&sample(), Path::new("Nope"), PathBuf::from("X:\\Nope"), 4096, &Progress::default(), Instant::now()).is_err());
}
