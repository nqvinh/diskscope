//! Project detection on a real folder (labels come from the scan): outermost root wins, nothing
//! is found inside node_modules, reclaimable = top-most build / dependency / cache folders.

use super::*;
use crate::scan::{walker::walk, Progress};
use std::path::Path;

#[test]
fn finds_outermost_projects_and_their_rebuildable_folders() {
    // Not under %TEMP%: everything there is (correctly) labelled Temp and skipped.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test-tmp").join(format!("proj-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let w = |p: &str, n: usize| {
        let f = root.join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, vec![1u8; n]).unwrap();
    };
    // A git monorepo with a Node package (dist + node_modules) and a Rust crate (target).
    std::fs::create_dir_all(root.join("mono").join(".git")).unwrap();
    w("mono/web/package.json", 10);
    for k in 0..20 {
        w(&format!("mono/web/cfg{k}.json"), 10); // many small .json files: package.json must stay visible
    }
    w("mono/web/dist/app.js", 300_000);
    w("mono/web/node_modules/dep/package.json", 10);
    w("mono/web/node_modules/dep/index.js", 200_000);
    w("mono/rs/Cargo.toml", 10);
    w("mono/rs/target/release/app.exe", 500_000);
    w("mono/rs/src/main.rs", 1000);
    // A Unity project and a plain folder.
    std::fs::create_dir_all(root.join("game").join("Assets")).unwrap();
    std::fs::create_dir_all(root.join("game").join("ProjectSettings")).unwrap();
    w("game/Library/cache.bin", 400_000);
    w("photos/a.jpg", 5000);
    let tree = walk(&root, &Progress::default());
    assert!(tree.find(Path::new("mono\\web\\package.json")).is_some(), "markers are never grouped");
    let found = detect(&tree);
    let names: Vec<String> = found.iter().map(|p| tree.name(p.node).to_string()).collect();
    assert_eq!(names, ["mono", "game"], "outermost roots only, biggest reclaimable first");
    let mono = &found[0];
    assert_eq!(mono.kinds, ["git"]);
    assert_eq!(mono.reclaimable, 300_000 + 200_010 + 500_000, "dist + node_modules + target");
    assert_eq!(mono.reclaim.len(), 3);
    assert!(found[1].kinds.contains(&"Unity") && found[1].reclaimable == 400_000);
    std::fs::remove_dir_all(&root).ok();
}
