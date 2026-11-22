//! Delete benchmark on a GENERATED junk tree only (refuses any other path). Ignored by default:
//! cargo test --release --test delete_bench -- --ignored --nocapture
//! The Recycle Bin rows move generated junk into the Recycle Bin, so they only run with
//! DISKSCOPE_BENCH_RECYCLE=1; the default run deletes permanently and leaves the bin alone.

use diskscope::cleanup::fast_delete::{remove_tree, DeleteProgress};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

const BASE: &str = r"D:\tmp\ds-del-bench";
/// Second volume for the cross-volume row (opt-in, capped, removed afterwards).
const BASE_E: &str = r"E:\tmp\ds-del-bench";

fn generate(root: &Path, dirs: usize, files_per_dir: usize, big: usize) {
    assert!(root.starts_with(BASE) || root.starts_with(BASE_E), "benchmark trees live under {BASE} or {BASE_E} only");
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
    let subfolders = |root: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(root).unwrap().flatten().flat_map(|p| std::fs::read_dir(p.path()).unwrap().flatten().map(|e| e.path())).collect()
    };
    use diskscope::cleanup::{action_log::ActionLog, confirm, execute::execute, review::{ReviewItem, ReviewList}};
    let log = ActionLog::new(base.join("log"));
    let list_of = |paths: Vec<PathBuf>| {
        let mut l = ReviewList::default();
        for p in paths {
            l.add(ReviewItem { path: p, size: 0, is_dir: true, items: 0, note: String::new() }).unwrap();
        }
        l
    };
    for (dirs, files) in [(500, 20), (2_000, 5)] {
        println!("== permanent delete of {dirs} selected folders ({files} files each)");
        let r = base.join("many-old");
        generate(&r, dirs, files, 0);
        let items = subfolders(&r);
        timed("old: one item after another (remove_tree per item)", || items.iter().for_each(|p| remove_tree(p, &DeleteProgress::default()).unwrap()));
        let _ = std::fs::remove_dir_all(&r);
        let r = base.join("many-new");
        generate(&r, dirs, files, 0);
        let plan = confirm::confirm_permanent(&list_of(subfolders(&r)), true, true, "DELETE").unwrap();
        timed("new: execute(), items in parallel on one pool", || { execute(plan, &log, &DeleteProgress::default()); });
        assert!(subfolders(&r).is_empty());
        let _ = std::fs::remove_dir_all(&r);
    }
    println!("== through the real executor (confirm dialog plan -> execute)");
    let p1 = base.join("exec-perm");
    generate(&p1, 10_000, 20, 3);
    let plan = confirm::confirm_permanent(&list_of(vec![p1.clone()]), true, true, "DELETE").unwrap();
    timed("execute(): permanent, 200k files + 3 x 200 MB", || { execute(plan, &log, &DeleteProgress::default()); });
    assert!(!p1.exists());
    if std::env::var_os("DISKSCOPE_BENCH_RECYCLE").is_none() {
        println!("(Recycle Bin rows skipped: set DISKSCOPE_BENCH_RECYCLE=1)");
        return;
    }
    println!("== Recycle Bin: 10,000 small files in 500 folders (selection = the 500 folders)");
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

    println!("== Recycle Bin through the real executor");
    let p2 = base.join("exec-bin");
    generate(&p2, 500, 20, 0);
    let plan = confirm::confirm_recycle(&list_of(subfolders(&p2)), true).unwrap();
    timed("execute(): Recycle Bin, 500 selected folders / 10k files", || { execute(plan, &log, &DeleteProgress::default()); });
    let _ = std::fs::remove_dir_all(&p2);
    println!("Recycle Bin limit on D: {:?}", diskscope::sys::recycle_capacity("D:\\"));
}

fn free_on(root: &str) -> u64 {
    diskscope::sys::drives().into_iter().find(|d| d.root.eq_ignore_ascii_case(root)).map(|d| d.free).unwrap_or(0)
}

/// Permanent delete of a selection spread over two volumes (D: NVMe, E: SATA SSD). Opt-in with
/// DISKSCOPE_BENCH_E=1; E: is capped (~50 MB on disk), checked for 2 GB free, and cleaned up.
#[test]
#[ignore]
fn cross_volume_delete_benchmark() {
    if std::env::var_os("DISKSCOPE_BENCH_E").is_none() {
        println!("skipped: set DISKSCOPE_BENCH_E=1");
        return;
    }
    let before = free_on("E:\\");
    println!("E: free before: {:.2} GB", before as f64 / 1e9);
    assert!(before >= 2_000_000_000, "E: has less than 2 GB free: not generating anything there");
    use diskscope::cleanup::{action_log::ActionLog, confirm, execute::execute, review::{ReviewItem, ReviewList}};
    let log = ActionLog::new(PathBuf::from(BASE).join("log"));
    let tops = |root: &Path| -> Vec<PathBuf> { std::fs::read_dir(root).unwrap().flatten().map(|e| e.path()).collect() };
    // 30 package folders per volume, 600 module folders x 20 files of 700 bytes (~50 MB on disk).
    let make = |tag: &str| -> Vec<PathBuf> {
        [BASE, BASE_E].iter().flat_map(|b| {
            let r = Path::new(b).join(tag);
            generate(&r, 600, 20, 0);
            tops(&r)
        }).collect()
    };
    for round in 1..=2 {
        let items = make("xvol-old");
        timed(&format!("r{round} old: items one after another (D: then E:)"), || items.iter().for_each(|p| remove_tree(p, &DeleteProgress::default()).unwrap()));
        let items = make("xvol-new");
        let mut l = ReviewList::default();
        for p in items {
            l.add(ReviewItem { path: p, size: 0, is_dir: true, items: 0, note: String::new() }).unwrap();
        }
        let plan = confirm::confirm_permanent(&l, true, true, "DELETE").unwrap();
        timed(&format!("r{round} new: execute(), items in parallel across D: and E:"), || { execute(plan, &log, &DeleteProgress::default()); });
    }
    std::fs::remove_dir_all(BASE_E).unwrap();
    let _ = std::fs::remove_dir(r"E:\tmp"); // only succeeds when empty (it did not exist before)
    for tag in ["xvol-old", "xvol-new"] {
        let _ = std::fs::remove_dir_all(Path::new(BASE).join(tag));
    }
    println!("E: free after: {:.2} GB ({} removed)", free_on("E:\\") as f64 / 1e9, BASE_E);
}
