//! User excludes and "never suggest" entries (both saved in settings).
//! A pattern without `\`, `/` or `:` matches an item's NAME (`node_modules`, `*.iso`); otherwise it
//! matches the FULL PATH (`D:\VMs`, `C:\Users\*\Downloads`). `*` = any text, `?` = one character,
//! case-insensitive. Skip = not scanned at all (not counted); Hide = scanned and counted in folder
//! totals, but not shown in any view, list or suggestion.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum ExcludeMode {
    Skip,
    Hide,
}

impl ExcludeMode {
    pub fn label(self) -> &'static str {
        match self {
            ExcludeMode::Skip => "Skip during scan",
            ExcludeMode::Hide => "Hide from views",
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct Exclude {
    pub pattern: String,
    pub mode: ExcludeMode,
}

/// Lowercase, `/` -> `\`, no trailing `\` (except a drive root such as `c:\`).
fn norm(s: &str) -> String {
    let s = s.trim().to_lowercase().replace('/', "\\");
    if s.len() > 3 { s.trim_end_matches('\\').to_string() } else { s }
}

/// Glob match of lowercase strings: `*` any run (also across `\`), `?` one character.
pub fn glob(pat: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pat.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            (pi, ti) = (pi + 1, ti + 1);
        } else if pi < p.len() && p[pi] == '*' {
            (star, mark, pi) = (Some(pi), ti, pi + 1);
        } else if let Some(s) = star {
            (pi, mark, ti) = (s + 1, mark + 1, mark + 1);
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// Excludes compiled for a scan.
#[derive(Default, Debug)]
pub struct ScanFilter {
    names: Vec<(String, ExcludeMode)>,
    paths: Vec<(String, ExcludeMode)>,
}

impl ScanFilter {
    pub fn new(list: &[Exclude]) -> Self {
        let mut f = ScanFilter::default();
        for e in list.iter().filter(|e| !e.pattern.trim().is_empty()) {
            let p = norm(&e.pattern);
            if p.contains('\\') || p.contains(':') { f.paths.push((p, e.mode)) } else { f.names.push((p, e.mode)) }
        }
        f
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty() && self.paths.is_empty()
    }

    /// True when some pattern needs the item's full path.
    pub fn has_paths(&self) -> bool {
        !self.paths.is_empty()
    }

    /// What to do with the item `name` inside folder `dir` (`dir` is only used by path patterns).
    /// Skip wins over Hide when both match.
    pub fn mode(&self, name: &str, dir: Option<&Path>) -> Option<ExcludeMode> {
        let lname = name.to_lowercase();
        let mut hit = self.names.iter().filter(|(p, _)| glob(p, &lname)).map(|x| x.1).collect::<Vec<_>>();
        if let (Some(d), true) = (dir, self.has_paths()) {
            let full = norm(&d.join(name).to_string_lossy());
            hit.extend(self.paths.iter().filter(|(p, _)| glob(p, &full)).map(|x| x.1));
        }
        if hit.contains(&ExcludeMode::Skip) { Some(ExcludeMode::Skip) } else { hit.first().copied() }
    }
}

/// True when `path` is a "never suggest" entry or lies inside one.
pub fn never_suggested(list: &[String], path: &Path) -> bool {
    let p = norm(&path.to_string_lossy());
    list.iter().map(|e| norm(e)).any(|e| !e.is_empty() && (p == e || p.starts_with(&format!("{}\\", e.trim_end_matches('\\')))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_names_and_paths() {
        assert!(glob("*.iso", "win11.iso") && !glob("*.iso", "win11.iso.part"));
        assert!(glob("node_modules", "node_modules") && glob("n?de*", "node_modules"));
        let f = ScanFilter::new(&[
            Exclude { pattern: "node_modules".into(), mode: ExcludeMode::Skip },
            Exclude { pattern: "*.ISO".into(), mode: ExcludeMode::Hide },
            Exclude { pattern: r"D:/VMs/".into(), mode: ExcludeMode::Hide },
            Exclude { pattern: r"c:\users\*\downloads".into(), mode: ExcludeMode::Skip },
            Exclude { pattern: "  ".into(), mode: ExcludeMode::Skip },
        ]);
        assert!(f.has_paths() && !f.is_empty());
        assert_eq!(f.mode("Node_Modules", None), Some(ExcludeMode::Skip));
        assert_eq!(f.mode("disk.iso", None), Some(ExcludeMode::Hide));
        assert_eq!(f.mode("VMs", Some(Path::new(r"D:\"))), Some(ExcludeMode::Hide));
        assert_eq!(f.mode("Downloads", Some(Path::new(r"C:\Users\ann"))), Some(ExcludeMode::Skip));
        assert_eq!(f.mode("Downloads", None), None, "path patterns need the folder");
        assert_eq!(f.mode("src", Some(Path::new(r"D:\code"))), None);
    }

    #[test]
    fn skip_wins_and_never_suggest_covers_children() {
        let f = ScanFilter::new(&[
            Exclude { pattern: "*.log".into(), mode: ExcludeMode::Hide },
            Exclude { pattern: "debug.*".into(), mode: ExcludeMode::Skip },
        ]);
        assert_eq!(f.mode("debug.log", None), Some(ExcludeMode::Skip));
        let never = vec![r"D:\code\app\node_modules".to_string()];
        assert!(never_suggested(&never, Path::new(r"d:\CODE\app\node_modules")));
        assert!(never_suggested(&never, Path::new(r"D:\code\app\node_modules\.cache")));
        assert!(!never_suggested(&never, Path::new(r"D:\code\app\node_modules2")));
    }
}
