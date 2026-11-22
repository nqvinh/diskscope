//! Tool-aware cleanup: for caches a developer tool manages, run the tool's own command
//! (`pnpm store prune`, `npm cache clean --force`, ...) instead of deleting the folder. This file
//! is pure (specs, command building, output parsing, matching a path to a tool); `tool_probe`
//! finds the installed tools and `tool_runner` runs a command after the user confirmed it.

use std::path::{Path, PathBuf};

pub use super::tool_parse::{cargo_home, parse_docker_size, parse_probe};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Pnpm,
    Npm,
    Yarn,
    Nuget,
    Pip,
    Docker,
    Cargo,
}

pub const ALL: [Tool; 7] = [Tool::Pnpm, Tool::Npm, Tool::Yarn, Tool::Nuget, Tool::Pip, Tool::Docker, Tool::Cargo];

/// Seconds a read-only probe (`pnpm store path`, ...) may take.
pub const PROBE_TIMEOUT_SECS: u64 = 20;
/// Seconds a cleanup command may take before it is stopped.
pub const CLEAN_TIMEOUT_SECS: u64 = 20 * 60;

pub struct Spec {
    pub name: &'static str,
    /// Ways to start the tool, tried in order: program name (looked up on PATH) + leading args.
    pub launchers: &'static [&'static [&'static str]],
    /// Read-only query that prints the cache location(s) (or Docker's disk usage); empty = none.
    pub probe: &'static [&'static str],
    /// The cleanup command, `None` when diskscope only gives a hint (Cargo).
    pub clean: Option<&'static [&'static str]>,
    /// One sentence for the confirm dialog.
    pub what: &'static str,
}

pub fn spec(t: Tool) -> Spec {
    match t {
        Tool::Pnpm => Spec {
            name: "pnpm",
            launchers: &[&["pnpm"]],
            probe: &["store", "path"],
            clean: Some(&["store", "prune"]),
            what: "Removes packages from the pnpm store that no project references any more. Packages still used by your projects stay, so their node_modules keep working.",
        },
        Tool::Npm => Spec {
            name: "npm",
            launchers: &[&["npm"]],
            probe: &["config", "get", "cache"],
            clean: Some(&["cache", "clean", "--force"]),
            what: "Empties npm's download cache. Installed node_modules are not touched; npm downloads packages again when it needs them.",
        },
        Tool::Yarn => Spec {
            name: "yarn",
            launchers: &[&["yarn"]],
            probe: &["cache", "dir"],
            clean: Some(&["cache", "clean"]),
            what: "Empties Yarn's global download cache. Installed node_modules are not touched; Yarn downloads packages again when it needs them.",
        },
        Tool::Nuget => Spec {
            name: "NuGet",
            launchers: &[&["dotnet"]],
            probe: &["nuget", "locals", "all", "--list"],
            clean: Some(&["nuget", "locals", "all", "--clear"]),
            what: "Runs through the dotnet CLI and clears all NuGet caches: global packages, HTTP cache, temp and plugins cache. The next restore or build downloads the packages it needs again.",
        },
        Tool::Pip => Spec {
            name: "pip",
            launchers: &[&["pip"], &["py", "-m", "pip"]],
            probe: &["cache", "dir"],
            clean: Some(&["cache", "purge"]),
            what: "Empties pip's cache of downloaded and built wheels. Installed packages are not touched.",
        },
        Tool::Docker => Spec {
            name: "Docker",
            launchers: &[&["docker"]],
            probe: &["system", "df", "--format", "{{.Type}}\t{{.Size}}\t{{.Reclaimable}}"],
            clean: Some(&["builder", "prune", "-f"]),
            what: "Removes Docker's build cache that no image needs (images, containers and volumes stay). The space is freed INSIDE Docker's virtual disk: Windows gets it back only after that disk is compacted (see the VM disks tab).",
        },
        Tool::Cargo => Spec {
            name: "Cargo (Rust)",
            launchers: &[&["cargo"]],
            probe: &[], // hint only: being on PATH is enough, nothing is run
            clean: None,
            what: "Cargo has no built-in cache cleaner. Rust target folders are listed in Candidates (deleting one = 'cargo clean'). registry\\cache holds downloaded .crate files and registry\\src their unpacked sources: both are re-downloaded or re-created on the next build.",
        },
    }
}

/// What a probe found for one installed tool.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolInfo {
    pub tool: Tool,
    /// Resolved program (full path) and the leading args of the launcher that worked.
    pub program: PathBuf,
    pub prefix: Vec<String>,
    /// Cache folders the tool reported (empty for Docker).
    pub caches: Vec<PathBuf>,
    /// Bytes the tool itself says it can reclaim (Docker build cache).
    pub reclaimable: Option<u64>,
    /// Problem to show instead of the action ("installed but not running", ...); empty = usable.
    pub note: String,
}

impl ToolInfo {
    pub fn can_clean(&self) -> bool {
        spec(self.tool).clean.is_some() && self.note.is_empty()
    }

    /// The exact command shown to the user, e.g. `pnpm store prune`.
    pub fn command_line(&self) -> String {
        command_line(self.tool, &self.prefix)
    }

    /// Args passed to `program` for the cleanup command.
    pub fn clean_args(&self) -> Vec<String> {
        let mut a = self.prefix.clone();
        a.extend(spec(self.tool).clean.unwrap_or(&[]).iter().map(|s| s.to_string()));
        a
    }
}

/// `pnpm store prune`; `py -m pip cache purge` when pip runs through the launcher.
pub fn command_line(t: Tool, prefix: &[String]) -> String {
    let s = spec(t);
    let head = if prefix.is_empty() { s.launchers[0][0].to_string() } else { prefix.join(" ") };
    let tail = s.clean.unwrap_or(&[]).join(" ");
    format!("{head} {tail}").trim().to_string()
}

fn norm(p: &Path) -> String {
    crate::rules::blocklist::normalize(p)
}

/// The usable tool whose primary cache (the first one reported) is `path` itself or one version
/// folder below it: pnpm reports `store\v10` while the suggestion is `store`, Yarn `Cache\v6`.
/// Deeper matches are refused so the user Temp folder never offers "Clean with NuGet".
pub fn tool_for_path<'a>(infos: &'a [ToolInfo], path: &Path) -> Option<&'a ToolInfo> {
    let row = norm(path);
    infos.iter().filter(|i| i.can_clean()).find(|i| {
        let Some(cache) = i.caches.first().map(|c| norm(c)) else { return false };
        cache == row || cache.strip_prefix(&format!("{row}\\")).is_some_and(|rest| !rest.is_empty() && !rest.contains(['\\', '/']))
    })
}

#[cfg(test)]
#[path = "tool_actions_tests.rs"]
mod tests;
