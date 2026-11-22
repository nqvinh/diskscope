//! Parallel delete tests on temporary folders only.

use super::*;

fn temp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("diskscope-del-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn deletes_nested_read_only_and_long_paths() {
    let root = temp("tree");
    for d in 0..5 {
        let dir = root.join(format!("pkg{d}")).join("lib");
        std::fs::create_dir_all(&dir).unwrap();
        for f in 0..20 {
            std::fs::write(dir.join(format!("f{f}.js")), b"x").unwrap();
        }
    }
    let ro = root.join("pkg0").join("readonly.txt");
    std::fs::write(&ro, b"locked").unwrap();
    let mut perm = std::fs::metadata(&ro).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&ro, perm).unwrap();
    let mut long = root.clone();
    for k in 0..12 {
        long = long.join(format!("a-rather-long-folder-name-{k:02}"));
    }
    std::fs::create_dir_all(&long).unwrap();
    std::fs::write(long.join("deep.bin"), b"deep").unwrap();
    assert!(long.as_os_str().len() > 300, "the test path must exceed MAX_PATH");
    let progress = DeleteProgress::default();
    remove_tree(&root, &progress).unwrap();
    assert!(!root.exists());
    assert_eq!(progress.files_done.load(Relaxed), 100 + 1 + 1);
}

#[test]
fn never_follows_a_junction_inside_the_tree() {
    let root = temp("junction");
    let outside = temp("junction-target");
    std::fs::write(outside.join("keep.txt"), b"must survive").unwrap();
    let ok = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(root.join("link")).arg(&outside).output().map(|o| o.status.success()).unwrap_or(false);
    assert!(ok, "could not create a junction");
    std::fs::write(root.join("own.txt"), b"x").unwrap();
    remove_tree(&root, &DeleteProgress::default()).unwrap();
    assert!(!root.exists());
    assert!(outside.join("keep.txt").exists(), "the junction target was not touched");
    std::fs::remove_dir_all(&outside).ok();
}

#[test]
fn cancel_stops_before_deleting() {
    let root = temp("cancel");
    for f in 0..50 {
        std::fs::write(root.join(format!("f{f}")), b"x").unwrap();
    }
    let progress = DeleteProgress::default();
    progress.cancel.store(true, Relaxed);
    assert!(remove_tree(&root, &progress).is_err());
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 50, "nothing deleted after cancel");
    std::fs::remove_dir_all(&root).ok();
}
