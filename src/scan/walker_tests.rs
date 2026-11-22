//! Walker tests on a real temporary folder: totals, small-file grouping, junctions never followed.

use super::*;
use crate::model::NONE;

fn temp(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("diskscope-walk-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn groups_small_files_and_never_follows_junctions() {
    let root = temp("root");
    let outside = temp("outside");
    for k in 0..20 {
        std::fs::write(root.join(format!("f{k}.txt")), b"0123456789").unwrap();
    }
    std::fs::write(root.join("big.bin"), vec![7u8; 100_000]).unwrap();
    std::fs::write(root.join("lone.md"), b"#").unwrap();
    std::fs::create_dir(root.join("sub")).unwrap();
    std::fs::write(root.join("sub").join("a.txt"), b"abc").unwrap();
    std::fs::write(root.join("sub").join("b.txt"), b"abcd").unwrap();
    std::fs::write(outside.join("hidden.bin"), vec![1u8; 50_000]).unwrap();
    let ok = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(root.join("link"))
        .arg(&outside)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(ok, "could not create a junction for the test");

    let tree = walk(&root, &Progress::default());
    let r = tree.node(0);
    assert_eq!(r.files, 24, "20 grouped + big.bin + lone.md + 2 in sub; nothing behind the junction");
    assert_eq!(r.size, 200 + 100_000 + 1 + 7);
    assert_eq!(r.items, 24 + 2, "files + sub + link");
    assert!(tree.find(Path::new("lone.md")).is_some(), "a lone small file of its kind is not grouped");
    assert_eq!(tree.reparse_skipped, 1);
    let group = tree.find(Path::new("[20 small .txt files]")).expect("group entry");
    assert!(tree.node(group).is_group());
    assert_eq!(tree.node(group).size, 200);
    let link = tree.find(Path::new("link")).expect("junction entry");
    assert!(tree.node(link).is_reparse() && tree.node(link).first_child == NONE);
    assert!(tree.find(Path::new("sub\\a.txt")).is_some(), "few small files stay individual");
    let stats = crate::scan::extensions::ext_stats(&tree, 0);
    let txt = stats.iter().find(|s| tree.exts[s.ext as usize] == ".txt").unwrap();
    assert_eq!((txt.files, txt.size), (22, 207));

    let _ = std::fs::remove_dir(root.join("link"));
    std::fs::remove_dir_all(&root).ok();
    std::fs::remove_dir_all(&outside).ok();
}

#[test]
fn streams_live_suggestions_while_scanning() {
    use crate::rules::{ao_sessions::AoSessions, candidates::{Confidence, LiveSink}};
    // Not under %TEMP%: everything there is (correctly) labelled Temp.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-tmp").join(format!("live-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(root.join("app").join("node_modules").join("pkg")).unwrap();
    std::fs::write(root.join("app").join("node_modules").join("pkg").join("index.js"), vec![1u8; 300_000]).unwrap();
    std::fs::create_dir_all(root.join("rs").join("target").join("release")).unwrap();
    std::fs::write(root.join("rs").join("Cargo.toml"), b"[package]").unwrap();
    std::fs::write(root.join("rs").join("target").join("release").join("app.exe"), vec![2u8; 500_000]).unwrap();
    let progress = Progress::default();
    let sink = std::sync::Arc::new(LiveSink::new(AoSessions::default()));
    progress.live.set(sink.clone()).ok();
    let tree = walk(&root, &progress);
    let label = |p: &str| crate::rules::labels::Label::from_u8(tree.node(tree.find(Path::new(p)).unwrap()).label);
    assert_eq!(label("app\\node_modules"), crate::rules::labels::Label::DevDeps);
    assert_eq!(label("app\\node_modules\\pkg\\index.js"), crate::rules::labels::Label::DevDeps, "inherited");
    assert_eq!(label("rs\\target"), crate::rules::labels::Label::Build);
    assert_eq!(label("rs\\Cargo.toml"), crate::rules::labels::Label::Other);
    let live = sink.ranked(0);
    let kinds: Vec<(&str, u64)> = live.iter().map(|s| (s.kind, s.bytes)).collect();
    assert_eq!(kinds, vec![("rust-target", 500_000), ("node_modules", 300_000)]);
    assert_eq!(live[0].confidence, Confidence::Safe);
    std::fs::remove_dir_all(&root).ok();
}
