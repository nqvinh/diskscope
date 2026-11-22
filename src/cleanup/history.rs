//! Delete history for the History tab: one JSON line per deleted (or skipped) item in
//! %LOCALAPPDATA%\diskscope\history\history.jsonl. Entries older than 90 days are dropped at
//! startup and after every write, so the file stays small.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;

pub const KEEP_DAYS: u32 = 90;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HistoryEntry {
    /// Unix seconds (for pruning and sorting) and the local time it was written.
    pub at_unix: u32,
    pub at: String,
    pub path: String,
    pub size: u64,
    pub items: u64,
    /// "recycle" or "permanent".
    pub mode: String,
    /// "ok", "skipped" (missing, protected, changed) or "error" (in use, denied, other).
    pub result: String,
    pub error: String,
}

pub struct History {
    file: PathBuf,
}

/// Maps a skip reason to "skipped" (nothing to do) or "error" (something stopped the delete).
pub fn result_of(skip_reason: Option<&str>) -> &'static str {
    match skip_reason {
        None => "ok",
        Some(r) if r.starts_with("protected") || r.starts_with("not found") || r.starts_with("changed") => "skipped",
        Some(_) => "error",
    }
}

impl History {
    pub fn new(dir: PathBuf) -> Self {
        Self { file: dir.join("history.jsonl") }
    }

    pub fn default_location() -> Self {
        Self::new(crate::sys::data_dir().join("history"))
    }

    pub fn load(&self) -> Vec<HistoryEntry> {
        std::fs::read_to_string(&self.file)
            .map(|s| s.lines().filter_map(|l| serde_json::from_str(l).ok()).collect())
            .unwrap_or_default()
    }

    pub fn append(&self, entries: &[HistoryEntry], now: u32) -> std::io::Result<()> {
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&self.file)?;
        for e in entries {
            writeln!(f, "{}", serde_json::to_string(e).unwrap_or_default())?;
        }
        drop(f);
        self.prune(now).map(|_| ())
    }

    /// Drops entries older than KEEP_DAYS; returns how many were removed.
    pub fn prune(&self, now: u32) -> std::io::Result<usize> {
        let all = self.load();
        let cutoff = now.saturating_sub(KEEP_DAYS * 86_400);
        let keep: Vec<&HistoryEntry> = all.iter().filter(|e| e.at_unix >= cutoff).collect();
        let removed = all.len() - keep.len();
        if removed > 0 {
            let tmp = self.file.with_extension("tmp");
            let mut f = std::fs::File::create(&tmp)?;
            for e in keep {
                writeln!(f, "{}", serde_json::to_string(e).unwrap_or_default())?;
            }
            drop(f);
            std::fs::rename(&tmp, &self.file)?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(at_unix: u32, path: &str) -> HistoryEntry {
        HistoryEntry { at_unix, at: String::new(), path: path.into(), size: 1, items: 0, mode: "recycle".into(), result: "ok".into(), error: String::new() }
    }

    #[test]
    fn appends_and_prunes_entries_older_than_90_days() {
        let dir = std::env::temp_dir().join(format!("diskscope-history-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let h = History::new(dir.clone());
        let now = 2_000_000_000u32;
        h.append(&[entry(now - 100 * 86_400, "old"), entry(now - 89 * 86_400, "recent")], now).unwrap();
        let left: Vec<String> = h.load().into_iter().map(|e| e.path).collect();
        assert_eq!(left, ["recent"], "the write itself prunes");
        h.append(&[entry(now, "new")], now).unwrap();
        assert_eq!(h.load().len(), 2);
        assert_eq!(h.prune(now + 10 * 86_400).unwrap(), 1, "startup prune later drops 'recent'");
        assert_eq!(h.load()[0].path, "new");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn skip_reasons_map_to_results() {
        assert_eq!(result_of(None), "ok");
        assert_eq!(result_of(Some("not found (already gone?)")), "skipped");
        assert_eq!(result_of(Some("in use by another program")), "error");
    }
}
