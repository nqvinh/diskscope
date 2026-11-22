//! Difference between two snapshots of the same root: folders that grew, shrank, appeared or
//! disappeared. Like "Grew since last scan", a folder is left out when one of its sub-folders
//! explains 90 %+ of its change, so the list points at where the change really is.

use super::snapshot::Snapshot;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Change {
    Grew,
    Shrank,
    New,
    Gone,
}

impl Change {
    pub fn label(self) -> &'static str {
        match self {
            Change::Grew => "grew",
            Change::Shrank => "shrank",
            Change::New => "new",
            Change::Gone => "gone",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DiffRow {
    /// Relative path as written (the root itself is "").
    pub rel: String,
    pub before: u64,
    pub after: u64,
    pub change: Change,
    /// Label as u8 (from the newer snapshot when present).
    pub label: u8,
}

impl DiffRow {
    pub fn delta(&self) -> i64 {
        self.after as i64 - self.before as i64
    }
}

fn parent(rel: &str) -> Option<&str> {
    if rel.is_empty() { None } else { Some(rel.rsplit_once('\\').map(|x| x.0).unwrap_or("")) }
}

/// Folders whose size changed by at least `min_change` bytes between `old` and `new`.
/// Snapshots only list folders of 1 MiB or more, so a folder below that counts as 0 / missing.
pub fn diff(old: &Snapshot, new: &Snapshot, min_change: u64) -> Vec<DiffRow> {
    let mut keys: Vec<&String> = old.sizes.keys().chain(new.sizes.keys()).collect();
    keys.sort();
    keys.dedup();
    let delta: HashMap<&str, i64> = keys
        .iter()
        .map(|k| (k.as_str(), new.sizes.get(*k).copied().unwrap_or(0) as i64 - old.sizes.get(*k).copied().unwrap_or(0) as i64))
        .collect();
    // Largest same-direction change among each folder's direct sub-folders.
    let mut child_max: HashMap<&str, (i64, i64)> = HashMap::new();
    for (k, d) in &delta {
        if let Some(p) = parent(k) {
            let e = child_max.entry(p).or_default();
            e.0 = e.0.max(*d);
            e.1 = e.1.min(*d);
        }
    }
    keys.into_iter()
        .filter_map(|k| {
            let d = delta[k.as_str()];
            if d.unsigned_abs() < min_change.max(1) {
                return None;
            }
            let (up, down) = child_max.get(k.as_str()).copied().unwrap_or_default();
            let explained = if d > 0 { up as f64 >= 0.9 * d as f64 } else { (down as f64) <= 0.9 * d as f64 };
            if explained {
                return None;
            }
            let (before, after) = (old.sizes.get(k).copied(), new.sizes.get(k).copied());
            let change = match (before, after) {
                (None, Some(_)) => Change::New,
                (Some(_), None) => Change::Gone,
                _ if d > 0 => Change::Grew,
                _ => Change::Shrank,
            };
            let meta = new.meta.get(k).or_else(|| old.meta.get(k));
            Some(DiffRow { rel: meta.map(|m| m.0.clone()).unwrap_or_else(|| k.clone()), before: before.unwrap_or(0), after: after.unwrap_or(0), change, label: meta.map(|m| m.1).unwrap_or(0) })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(entries: &[(&str, u64)]) -> Snapshot {
        Snapshot {
            taken_at: 0,
            root: "Q:\\".into(),
            sizes: entries.iter().map(|(k, s)| (k.to_lowercase(), *s)).collect(),
            meta: entries.iter().map(|(k, _)| (k.to_lowercase(), (k.to_string(), 3))).collect(),
        }
    }

    #[test]
    fn grew_shrank_new_gone_at_the_right_level() {
        let mb = 1u64 << 20;
        let old = snap(&[("", 100 * mb), ("Proj", 60 * mb), ("Proj\\target", 50 * mb), ("Old", 30 * mb), ("Docs", 10 * mb)]);
        let new = snap(&[("", 115 * mb), ("Proj", 110 * mb), ("Proj\\target", 100 * mb), ("Docs", 4 * mb), ("Videos", 1 * mb), ("Cache", 0)]);
        let rows = diff(&old, &new, mb);
        let got: Vec<(&str, Change, i64)> = rows.iter().map(|r| (r.rel.as_str(), r.change, r.delta() / mb as i64)).collect();
        assert!(got.contains(&("Proj\\target", Change::Grew, 50)), "{got:?}");
        assert!(!got.iter().any(|g| g.0 == "Proj"), "explained by its target folder");
        assert!(got.contains(&("Old", Change::Gone, -30)) && got.contains(&("Docs", Change::Shrank, -6)));
        assert!(got.contains(&("Videos", Change::New, 1)));
        assert!(!got.iter().any(|g| g.0.is_empty()), "the root's +15 is explained by Proj's +50");
        assert!(rows.iter().all(|r| r.label == 3));
        assert!(diff(&old, &new, 40 * mb).iter().all(|r| r.delta().unsigned_abs() >= 40 * mb));
    }
}
