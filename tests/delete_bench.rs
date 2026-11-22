//! Delete benchmark on a GENERATED junk tree only (refuses any other path). Ignored by default:
//! cargo test --release --test delete_bench -- --ignored --nocapture
//! Note: the Recycle Bin rows move generated junk files into the Recycle Bin.

use diskscope::cleanup::fast_delete::{remove_tree, DeleteProgress};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

const BASE: &str = r"D:\tmp\ds-del-bench";

fn generate(root: &Path, dirs: usize, files_per_dir: usize, big: usize) {
    assert!(root.starts_with(BASE), "benchmark trees live under {BASE} only");
    (0..dirs).into_par_iter().for_each(|d| {
        let dir = root.join(format!("pkg{:05}", d / 50)).join(format!("mod{d:05}"));
        std::fs::create_dir_all(&dir).unwrap();
        for f in 0..files_per_dir {
            std::fs::write(dir.join(format!("file{f:03}.js")), [b'x'; 700]).unwrap();
        }
    });
    for b in 0..big {
        std::fs::write(root.join(format!("big{b}.bin")), vec![7u8; 200 << 20]).unwrap();
    }
}

fn timed(label: &str, f: impl FnOnce()) -> f64 {
    let t = Instant::now();
    f();
    let s = t.elapsed().as_secs_f64();
    println!("{label:<62} {s:>8.2} s");
    s
}

#[test]
#[ignore]
fn delete_benchmark() {
    let base = PathBuf::from(BASE);
    std::fs::create_dir_all(&base).unwrap();
    println!("\n== permanent delete: 200,000 small files in 10,000 folders + 3 x 200 MB");
    for (name, new) in [("old: std::fs::remove_dir_all", false), ("new: parallel remove_tree", true)] {
        let root = base.join(if new { "perm-new" } else { "perm-old" });
        let _ = std::fs::remove_dir_all(&root);
        generate(&root, 10_000, 20, 3);
        timed(name, || {
            if new { remove_tree(&root, &DeleteProgress::default()).unwrap() } else { std::fs::remove_dir_all(&root).unwrap() }
        });
        assert!(!root.exists());
    }
    println!("== Recycle Bin: 10,000 small files in 500 folders (selection = the 500 folders)");
    let subfolders = |root: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(root).unwrap().flatten().flat_map(|p| std::fs::read_dir(p.path()).unwrap().flatten().map(|e| e.path())).collect()
    };
    let r1 = base.join("bin-per-item");
    generate(&r1, 500, 20, 0);
    let items = subfolders(&r1);
    timed("old: one shell operation per item (500 x trash::delete)", || items.iter().for_each(|p| trash::delete(p).unwrap()));
    let r2 = base.join("bin-batch");
    generate(&r2, 500, 20, 0);
    let items = subfolders(&r2);
    timed("new: one batched shell operation (trash::delete_all)", || trash::delete_all(&items).unwrap());
    let r3 = base.join("bin-folder");
    generate(&r3, 500, 20, 0);
    timed("whole parent folder as one item (trash::delete)", || trash::delete(&r3).unwrap());
    let _ = std::fs::remove_dir_all(&r1);
    let _ = std::fs::remove_dir_all(&r2);

    println!("== through the real executor (confirm dialog plan -> execute)");
    use diskscope::cleanup::{action_log::ActionLog, confirm, execute::execute, review::{ReviewItem, ReviewList}};
    let log = ActionLog::new(base.join("log"));
    let list_of = |paths: Vec<PathBuf>| {
        let mut l = ReviewList::default();
        for p in paths {
            l.add(ReviewItem { path: p, size: 0, is_dir: true, items: 0, note: String::new() }).unwrap();
        }
        l
    };
    let p1 = base.join("exec-perm");
    generate(&p1, 10_000, 20, 3);
    let plan = confirm::confirm_permanent(&list_of(vec![p1.clone()]), true, true, "DELETE").unwrap();
    timed("execute(): permanent, 200k files + 3 x 200 MB", || { execute(plan, &log, &DeleteProgress::default()); });
    assert!(!p1.exists());
    let p2 = base.join("exec-bin");
    generate(&p2, 500, 20, 0);
    let plan = confirm::confirm_recycle(&list_of(subfolders(&p2)), true).unwrap();
    timed("execute(): Recycle Bin, 500 selected folders / 10k files", || { execute(plan, &log, &DeleteProgress::default()); });
    let _ = std::fs::remove_dir_all(&p2);
    println!("Recycle Bin limit on D: {:?}", diskscope::sys::recycle_capacity("D:\\"));
}
