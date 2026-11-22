//! Parses the read-only probe output of each tool into cache folders (and Docker's reclaimable
//! build cache). Pure functions, covered by tool_actions_tests.

use super::tool_actions::Tool;
use std::path::PathBuf;

fn looks_absolute(s: &str) -> bool {
    let b = s.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')) || s.starts_with("\\\\")
}

fn clean_path(s: &str) -> PathBuf {
    let s = s.trim().trim_matches('"');
    let s = if s.len() > 3 { s.trim_end_matches(['\\', '/']) } else { s };
    PathBuf::from(s)
}

/// Parses a probe's stdout into (cache folders, reclaimable bytes). Err = nothing usable.
pub fn parse_probe(t: Tool, out: &str) -> Result<(Vec<PathBuf>, Option<u64>), String> {
    match t {
        Tool::Nuget => {
            // global-packages first: it is the primary cache a suggestion can match.
            let mut v: Vec<(bool, PathBuf)> = out
                .lines()
                .filter_map(|l| l.split_once(": "))
                .filter(|(_, p)| looks_absolute(p.trim()))
                .map(|(k, p)| (k.trim() != "global-packages", clean_path(p)))
                .collect();
            v.sort_by_key(|(rest, _)| *rest);
            if v.is_empty() { Err("no NuGet cache folders reported".into()) } else { Ok((v.into_iter().map(|(_, p)| p).collect(), None)) }
        }
        Tool::Docker => out
            .lines()
            .map(|l| l.split('\t').collect::<Vec<_>>())
            .find(|c| c.len() >= 3 && c[0].trim().eq_ignore_ascii_case("build cache"))
            .map(|c| (Vec::new(), parse_docker_size(c[2])))
            .ok_or_else(|| "Docker did not report its disk usage (is Docker Desktop running?)".into()),
        Tool::Cargo => Ok((cargo_home().map(|h| vec![h.join("registry")]).unwrap_or_default(), None)),
        _ => out.lines().map(str::trim).find(|l| looks_absolute(l)).map(|l| (vec![clean_path(l)], None)).ok_or_else(|| "the tool did not print a cache folder".into()),
    }
}

/// Docker sizes are decimal: "2.3GB", "800MB (66%)", "0B", "1.2kB".
pub fn parse_docker_size(s: &str) -> Option<u64> {
    let s = s.split_whitespace().next()?.trim();
    let split = s.find(|c: char| c.is_ascii_alphabetic())?;
    let (num, unit) = s.split_at(split);
    let n: f64 = num.parse().ok()?;
    let mul = match unit.to_ascii_lowercase().as_str() {
        "b" => 1.0,
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        _ => return None,
    };
    Some((n * mul).round() as u64)
}

/// %CARGO_HOME% or %USERPROFILE%\.cargo.
pub fn cargo_home() -> Option<PathBuf> {
    std::env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".cargo")))
}
