//! Project roots found in a scan (git repos, Node, Rust, .NET, Python, Unity, Gradle) with what
//! could be rebuilt: the top-most build outputs, dependency folders and caches inside them.
//! The outermost root wins (a monorepo shows once), and dependency / build / cache / temp /
//! system folders are never searched (node_modules holds thousands of package.json files).

use super::labels::Label;
use crate::model::Tree;

/// Names that mark a project root. They are never grouped with other small files.
pub fn is_project_marker(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    matches!(n.as_str(), ".git" | "package.json" | "cargo.toml" | "pyproject.toml" | "build.gradle" | "build.gradle.kts" | "settings.gradle" | "settings.gradle.kts")
        || n.ends_with(".sln")
        || n.ends_with(".csproj")
}

#[derive(Clone, Debug)]
pub struct Project {
    pub node: u32,
    /// What marked it: "git", "Node.js", "Rust", ".NET", "Python", "Gradle", "Unity".
    pub kinds: Vec<&'static str>,
    pub size: u64,
    pub reclaimable: u64,
    /// Top-most build / dependency / cache folders inside (what "Clean reclaimable" lists).
    pub reclaim: Vec<u32>,
    pub modified: u32,
}

fn kinds_of(tree: &Tree, dir: u32) -> Vec<&'static str> {
    let mut k: Vec<&'static str> = Vec::new();
    let (mut assets, mut settings) = (false, false);
    for c in tree.children(dir) {
        let n = tree.name(c);
        let is = |s: &str| n.eq_ignore_ascii_case(s);
        let lower_end = |s: &str| n.len() >= s.len() && n[n.len() - s.len()..].eq_ignore_ascii_case(s);
        let kind = if is(".git") {
            Some("git")
        } else if is("package.json") {
            Some("Node.js")
        } else if is("Cargo.toml") {
            Some("Rust")
        } else if lower_end(".sln") || lower_end(".csproj") {
            Some(".NET")
        } else if is("pyproject.toml") {
            Some("Python")
        } else if is("build.gradle") || is("build.gradle.kts") || is("settings.gradle") || is("settings.gradle.kts") {
            Some("Gradle")
        } else {
            assets |= tree.node(c).is_dir() && is("Assets");
            settings |= tree.node(c).is_dir() && is("ProjectSettings");
            None
        };
        if let Some(kind) = kind.filter(|k2| !k.contains(k2)) {
            k.push(kind);
        }
    }
    if assets && settings {
        k.push("Unity");
    }
    k
}

fn rebuildable(l: Label) -> bool {
    matches!(l, Label::Build | Label::DevDeps | Label::Cache)
}

fn reclaim_of(tree: &Tree, root: u32) -> Vec<u32> {
    let (mut out, mut stack) = (Vec::new(), vec![root]);
    while let Some(i) = stack.pop() {
        for c in tree.children(i) {
            let n = tree.node(c);
            if !n.is_dir() || n.is_reparse() {
                continue;
            }
            if rebuildable(Label::from_u8(n.label)) { out.push(c) } else { stack.push(c) }
        }
    }
    out
}

/// Every project root under the scan root, biggest reclaimable first.
pub fn detect(tree: &Tree) -> Vec<Project> {
    let skip = |l: Label| rebuildable(l) || matches!(l, Label::Temp | Label::System);
    let (mut out, mut stack) = (Vec::new(), vec![0u32]);
    while let Some(i) = stack.pop() {
        let kinds = kinds_of(tree, i);
        if !kinds.is_empty() {
            let n = tree.node(i);
            let reclaim = reclaim_of(tree, i);
            let reclaimable = reclaim.iter().map(|&r| tree.node(r).size).sum();
            out.push(Project { node: i, kinds, size: n.size, reclaimable, reclaim, modified: n.modified });
            continue;
        }
        for c in tree.children(i) {
            let n = tree.node(c);
            if n.is_dir() && !n.is_reparse() && !skip(Label::from_u8(n.label)) {
                stack.push(c);
            }
        }
    }
    out.sort_by(|a, b| b.reclaimable.cmp(&a.reclaimable).then(b.size.cmp(&a.size)));
    out
}

#[cfg(test)]
#[path = "projects_tests.rs"]
mod tests;
