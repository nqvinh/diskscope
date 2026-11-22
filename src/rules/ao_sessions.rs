//! Session status from the `ao` agent-orchestrator CLI (if installed), so git worktrees of finished agent sessions can be suggested.
//! Read-only: runs `ao session ls --all --include-terminated --json` once; absent `ao` = no data.

use serde::Deserialize;
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AoSession {
    pub id: String,
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub is_terminated: bool,
    #[serde(default)]
    pub last_activity_at: String,
}

#[derive(Default, Clone)]
pub struct AoSessions {
    pub by_id: HashMap<String, AoSession>,
    pub available: bool,
}

#[derive(Deserialize)]
struct Listing {
    data: Vec<AoSession>,
}

impl AoSessions {
    pub fn parse(json: &str) -> Self {
        match serde_json::from_str::<Listing>(json) {
            Ok(l) => Self { by_id: l.data.into_iter().map(|s| (s.id.clone(), s)).collect(), available: true },
            Err(_) => Self::default(),
        }
    }

    pub fn load() -> Self {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let out = std::process::Command::new("ao")
            .args(["session", "ls", "--all", "--include-terminated", "--json"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        match out {
            Ok(o) if o.status.success() => Self::parse(&String::from_utf8_lossy(&o.stdout)),
            _ => Self::default(),
        }
    }
}

#[test]
fn parses_listing() {
    let s = AoSessions::parse(
        r#"{"data":[{"id":"proj-12","projectId":"proj","status":"terminated","isTerminated":true,"lastActivityAt":"2026-11-01T10:00:00Z"},{"id":"x-1","status":"working"}]}"#,
    );
    assert!(s.available);
    assert!(s.by_id["proj-12"].is_terminated);
    assert!(!s.by_id["x-1"].is_terminated);
    assert!(!AoSessions::parse("not json").available);
}
