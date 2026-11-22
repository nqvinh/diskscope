//! Per-folder cleanup rules shared by the final candidate list and the live list built during a
//! scan. Every rule needs only the folder, its parents and its siblings (already in the tree when a
//! folder's batch is added), so the scanner can call it the moment a folder appears.

use super::ao_sessions::AoSessions;
use crate::model::{Tree, NONE};

/// How sure a suggestion is; ranking multiplies the size by `weight`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    /// Rebuildable caches: recreated automatically or by the next build.
    Safe,
    /// Usually fine to remove, costs a re-download/re-import or holds old leftovers.
    Likely,
    /// Look first: may still be in use.
    Check,
}

impl Confidence {
    pub fn weight(self) -> f64 {
        match self {
            Confidence::Safe => 1.0,
            Confidence::Likely => 0.6,
            Confidence::Check => 0.2,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Confidence::Safe => "safe cache",
            Confidence::Likely => "likely safe",
            Confidence::Check => "check first",
        }
    }
}

pub type Rule = (&'static str, String, Confidence);

/// Lowercase folder name; the root's name is its last path component (its stored name is the full path).
pub fn lower(tree: &Tree, i: u32) -> String {
    match i {
        NONE => String::new(),
        0 => tree.root_path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default(),
        _ => tree.name(i).to_lowercase(),
    }
}

pub fn up(tree: &Tree, i: u32) -> u32 {
    if i == NONE { NONE } else { tree.node(i).parent }
}

fn is_system(tree: &Tree, i: u32) -> bool {
    crate::rules::labels::Label::from_u8(tree.node(i).label) == crate::rules::labels::Label::System
}

fn has_child(tree: &Tree, dir: u32, name: &str) -> bool {
    dir != NONE && tree.children(dir).any(|c| tree.name(c).eq_ignore_ascii_case(name))
}

fn is_unity_project(tree: &Tree, dir: u32) -> bool {
    has_child(tree, dir, "Assets") && has_child(tree, dir, "ProjectSettings")
}

const STALE_DAYS: u32 = 30;

/// The rule matching folder `i`, if any. `inside` = an ancestor is already a suggestion (only a
/// Unity Library\Bee is reported inside another suggestion).
pub fn classify(tree: &Tree, i: u32, ao: &AoSessions, now: u32, inside: bool) -> Option<Rule> {
    use Confidence::*;
    if is_system(tree, i) {
        return None; // never suggest anything under Windows, Program Files, ...
    }
    let name = lower(tree, i);
    let p = up(tree, i);
    let (pname, gp) = (lower(tree, p), up(tree, p));
    if inside {
        return (name == "bee" && pname == "library" && is_unity_project(tree, gp))
            .then(|| ("unity-bee", "Unity build cache (Library/Bee); the next build recreates it".into(), Safe));
    }
    let r = |kind: &'static str, why: &str, c: Confidence| Some((kind, why.to_string(), c));
    match name.as_str() {
        "node_modules" => {
            let days = now.saturating_sub(tree.node(i).modified) / 86_400;
            if days >= STALE_DAYS {
                Some(("node_modules", format!("npm/pnpm packages of a project untouched for {days} days; 'pnpm install' restores them"), Likely))
            } else {
                r("node_modules", "npm/pnpm packages of a project used recently; 'pnpm install' restores them", Check)
            }
        }
        "npm-cache" => r("npm-cache", "npm download cache; 'npm cache clean --force' or delete, npm re-downloads", Safe),
        "cache" if pname == "yarn" => r("yarn-cache", "Yarn download cache; 'yarn cache clean' or delete, Yarn re-downloads", Safe),
        "cache" if pname == "pip" => r("pip-cache", "pip download/wheel cache; 'pip cache purge' or delete, pip re-downloads", Safe),
        "packages" if pname == ".nuget" => r("nuget-packages", "NuGet global packages; 'dotnet nuget locals all --clear', the next restore re-downloads", Likely),
        "target" if has_child(tree, p, "Cargo.toml") => r("rust-target", "Rust build output; 'cargo build' recreates it", Safe),
        "library" if is_unity_project(tree, p) => r("unity-library", "Unity Library cache; Unity re-imports everything on the next open (slow)", Likely),
        "temp" | "obj" | "logs" if is_unity_project(tree, p) => r("unity-temp", "Unity Temp/obj/Logs; recreated when Unity runs", Safe),
        "temp" if pname == "local" && lower(tree, gp) == "appdata" => r("user-temp", "user temp folder; files still in use are skipped", Safe),
        ".gradle" => r("gradle", "Gradle cache; re-downloaded by the next build", Safe),
        "__pycache__" => r("pycache", "Python bytecode cache; recreated automatically", Safe),
        ".venv" | "venv" if has_child(tree, i, "pyvenv.cfg") => r("python-venv", "Python virtual env; recreate with 'python -m venv' + pip install", Likely),
        ".pnpm-store" => r("pnpm-store", "pnpm package store; 'pnpm store prune' is the gentler way to shrink it", Safe),
        "store" if pname == "pnpm" => r("pnpm-store", "pnpm package store; 'pnpm store prune' is the gentler way to shrink it", Safe),
        n if (n == "dist" || n.starts_with("dist-") || n.starts_with("dist_")) && has_child(tree, p, "package.json") => {
            r("web-dist", "web build output; the build script recreates it", Safe)
        }
        _ if lower(tree, gp) == "worktrees" && lower(tree, up(tree, gp)) == "data" && lower(tree, up(tree, up(tree, gp))) == ".ao" => {
            ao_worktree(tree, i, ao, now)
        }
        _ => None,
    }
}

fn ao_worktree(tree: &Tree, i: u32, ao: &AoSessions, now: u32) -> Option<Rule> {
    let id = tree.name(i);
    // `worktrees\<project>\orchestrator` holds the live orchestrator checkout(s), not a session.
    if id.eq_ignore_ascii_case("orchestrator") || id.to_lowercase().ends_with("-orchestrator") {
        return None;
    }
    let prune = "afterwards run 'git worktree prune' in its repo";
    if !ao.available {
        let days = now.saturating_sub(tree.node(i).modified) / 86_400;
        return (days >= 7).then(|| ("ao-worktree", format!("AO session status unknown; untouched for {days} days; {prune}"), Confidence::Check));
    }
    let date = |s: &str| s.get(..10).unwrap_or(s).to_string();
    match ao.by_id.get(id) {
        Some(s) if s.is_terminated => {
            Some(("ao-worktree", format!("AO session {id} is terminated (last activity {}); {prune}", date(&s.last_activity_at)), Confidence::Likely))
        }
        Some(s) if matches!(s.status.as_str(), "exited" | "killed" | "done" | "merged") => Some((
            "ao-worktree-exited",
            format!("AO session {id} is '{}' but NOT terminated (last activity {}); check before removing", s.status, date(&s.last_activity_at)),
            Confidence::Check,
        )),
        Some(_) => None,
        None => Some(("ao-worktree", format!("no AO session named {id} any more; {prune}"), Confidence::Likely)),
    }
}

/// Folders inside AppData holding several version-named folders (CapCut\Apps\5.8.0.2216, ...):
/// every version but the newest, with its reason.
pub fn old_versions(tree: &Tree, dir: u32) -> Vec<(u32, String)> {
    if is_system(tree, dir) {
        return Vec::new();
    }
    let versions: Vec<(Vec<u64>, u32)> = tree
        .children(dir)
        .filter(|&c| tree.node(c).is_dir())
        .filter_map(|c| parse_version(tree.name(c)).map(|v| (v, c)))
        .collect();
    if versions.len() < 2 || !crate::rules::blocklist::normalize(&tree.path(dir)).contains("\\appdata\\") {
        return Vec::new();
    }
    let newest = versions.iter().max_by(|a, b| a.0.cmp(&b.0)).map(|v| v.1).expect("non-empty");
    let dname = lower(tree, dir);
    let app_node = if matches!(dname.as_str(), "apps" | "app" | "versions") { up(tree, dir) } else { dir };
    let app = if app_node == NONE { String::new() } else { tree.name(app_node).to_string() };
    versions
        .iter()
        .filter(|v| v.1 != newest)
        .map(|&(_, c)| (c, format!("older version of {app}; newest installed is {}", tree.name(newest))))
        .collect()
}

fn parse_version(name: &str) -> Option<Vec<u64>> {
    if !name.contains('.') || !name.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    name.split('.').map(|p| p.parse().ok()).collect()
}
