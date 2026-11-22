use super::*;
use crate::model::fixture::build;

const MB: u64 = 1 << 20;

fn kinds(tree: &Tree, ao: &AoSessions) -> Vec<(String, &'static str)> {
    let mut v: Vec<_> = detect(tree, ao, 2_000_000_000)
        .into_iter()
        .map(|c| (tree.path(c.node).display().to_string(), c.kind))
        .collect();
    v.sort();
    v
}

#[test]
fn finds_dev_caches_once_at_the_top() {
    let t = build(
        "C:\\dev",
        &[
            ("web\\package.json", 1, 0),
            ("web\\node_modules\\a\\node_modules\\b\\x.js", 3 * MB, 0),
            ("web\\dist\\app.js", 2 * MB, 0),
            ("rs\\Cargo.toml", 1, 0),
            ("rs\\target\\release\\big.rlib", 5 * MB, 0),
            ("other\\target\\keep.txt", 5 * MB, 0),
            ("game\\Assets\\a.png", 1, 0),
            ("game\\ProjectSettings\\p.asset", 1, 0),
            ("game\\Library\\Bee\\cache.bin", 4 * MB, 0),
            ("game\\Library\\ArtifactDB", 2 * MB, 0),
            ("py\\__pycache__\\m.pyc", 2 * MB, 0),
            ("tiny\\node_modules\\x.js", 10, 0),
        ],
    );
    let list = detect(&t, &AoSessions::default(), 0);
    let lib = list.iter().find(|c| c.kind == "unity-library").unwrap().size;
    let naive: u64 = list.iter().map(|c| c.size).sum();
    assert_eq!(total_without_overlap(&t, &list), naive - list.iter().find(|c| c.kind == "unity-bee").unwrap().size);
    assert!(lib >= 6 * MB);
    let got = kinds(&t, &AoSessions::default());
    assert_eq!(
        got,
        vec![
            ("C:\\dev\\game\\Library".to_string(), "unity-library"),
            ("C:\\dev\\game\\Library\\Bee".to_string(), "unity-bee"),
            ("C:\\dev\\py\\__pycache__".to_string(), "pycache"),
            ("C:\\dev\\rs\\target".to_string(), "rust-target"),
            ("C:\\dev\\web\\dist".to_string(), "web-dist"),
            ("C:\\dev\\web\\node_modules".to_string(), "node_modules"),
        ]
    );
}

#[test]
fn suggests_older_app_versions_in_appdata_only() {
    let t = build(
        "C:\\Users\\alice",
        &[
            ("AppData\\Local\\CapCut\\Apps\\5.8.0.2216\\capcut.exe", 3 * MB, 0),
            ("AppData\\Local\\CapCut\\Apps\\9.5.0.4050\\capcut.exe", 3 * MB, 0),
            ("AppData\\Local\\CapCut\\Apps\\6.1.2.2338\\capcut.exe", 3 * MB, 0),
            ("dev\\sdk\\1.0\\a.bin", 3 * MB, 0),
            ("dev\\sdk\\2.0\\a.bin", 3 * MB, 0),
        ],
    );
    let c = detect(&t, &AoSessions::default(), 0);
    let names: Vec<_> = c.iter().map(|c| t.name(c.node).to_string()).collect();
    assert_eq!(c.len(), 2, "{names:?}");
    assert!(names.contains(&"5.8.0.2216".to_string()) && names.contains(&"6.1.2.2338".to_string()));
    assert!(c[0].reason.contains("older version of CapCut; newest installed is 9.5.0.4050"));
}

#[test]
fn ao_worktrees_follow_session_status() {
    let t = build(
        "C:\\Users\\alice\\.ao",
        &[
            ("data\\worktrees\\proj\\proj-12\\f.bin", 2 * MB, 0),
            ("data\\worktrees\\proj\\proj-185\\f.bin", 2 * MB, 0),
            ("data\\worktrees\\proj\\proj-99\\f.bin", 2 * MB, 0),
            ("data\\worktrees\\proj\\gone-1\\f.bin", 2 * MB, 0),
            ("data\\worktrees\\proj\\orchestrator\\proj-orchestrator\\f.bin", 2 * MB, 0),
        ],
    );
    let ao = AoSessions::parse(
        r#"{"data":[{"id":"proj-12","isTerminated":true,"status":"terminated","lastActivityAt":"2026-11-01T00:00:00Z"},
        {"id":"proj-185","status":"working"},{"id":"proj-99","status":"exited"}]}"#,
    );
    let got: Vec<_> = kinds(&t, &ao).into_iter().map(|(p, k)| (p.rsplit('\\').next().unwrap().to_string(), k)).collect();
    assert_eq!(
        got,
        vec![
            ("gone-1".to_string(), "ao-worktree"),
            ("proj-12".to_string(), "ao-worktree"),
            ("proj-99".to_string(), "ao-worktree-exited"),
        ]
    );
}

#[test]
fn ranking_weighs_size_by_confidence() {
    let c = |size: u64, confidence: Confidence| Candidate { node: 0, kind: "x", reason: String::new(), size, modified: 0, confidence };
    let mut v = vec![c(100 * MB, Confidence::Safe), c(20_000 * MB, Confidence::Check), c(2_000 * MB, Confidence::Safe), c(10_000 * MB, Confidence::Likely)];
    rank(&mut v, Candidate::score);
    let sizes: Vec<u64> = v.iter().map(|c| c.size / MB).collect();
    assert_eq!(sizes, vec![10_000, 20_000, 2_000, 100], "6 GB-equivalent > 4 > 2 > 0.1");
    let sink = LiveSink::new(AoSessions::default());
    sink.list.lock().unwrap().extend([
        LiveSuggestion { path: "a".into(), kind: "npm-cache", reason: String::new(), confidence: Confidence::Safe, bytes: 3 * MB, modified: 0 },
        LiveSuggestion { path: "b".into(), kind: "node_modules", reason: String::new(), confidence: Confidence::Check, bytes: 10 * MB, modified: 0 },
        LiveSuggestion { path: "c".into(), kind: "pycache", reason: String::new(), confidence: Confidence::Safe, bytes: 1000, modified: 0 },
    ]);
    let live: Vec<String> = sink.ranked(MB).iter().map(|s| s.path.display().to_string()).collect();
    assert_eq!(live, ["a", "b"], "3 MB safe (3.0) beats 10 MB check-first (2.0); tiny ones hidden");
}

#[test]
fn stale_node_modules_rank_as_likely_and_recent_as_check() {
    let now = 2_000_000_000u32;
    let t = build("C:\\dev", &[("old\\node_modules\\x.js", 5 * MB, now - 90 * 86_400), ("new\\node_modules\\y.js", 5 * MB, now - 86_400)]);
    let list = detect(&t, &AoSessions::default(), now);
    let conf = |p: &str| list.iter().find(|c| t.path(c.node).display().to_string().contains(p)).unwrap().confidence;
    assert_eq!(conf("old"), Confidence::Likely);
    assert_eq!(conf("new"), Confidence::Check);
}
