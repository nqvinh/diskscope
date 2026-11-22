//! The review list: the ONLY way a path can become deletable. Items are added one by one by the
//! user; blocked paths are refused; a folder already covered by a listed parent is refused, and
//! adding a parent drops its listed children (so totals never double count).

use crate::rules::blocklist::{blocked_reason, normalize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewItem {
    pub path: PathBuf,
    pub size: u64,
    pub is_dir: bool,
    /// Files and folders inside (0 for a file or when unknown).
    pub items: u64,
    /// Why it was added (candidate reason or "picked in tree").
    pub note: String,
}

#[derive(Debug, PartialEq)]
pub enum AddError {
    Blocked(&'static str),
    AlreadyCovered(PathBuf),
}

impl std::fmt::Display for AddError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AddError::Blocked(why) => write!(f, "protected: {why}"),
            AddError::AlreadyCovered(p) => write!(f, "already covered by {}", p.display()),
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct ReviewList {
    items: Vec<ReviewItem>,
}

/// True when `child` is `parent` or inside it (case-insensitive, component-wise).
pub fn is_within(child: &Path, parent: &Path) -> bool {
    let (c, p) = (normalize(child), normalize(parent));
    c == p || c.starts_with(&format!("{p}\\"))
}

impl ReviewList {
    pub fn add(&mut self, item: ReviewItem) -> Result<usize, AddError> {
        if let Some(why) = blocked_reason(&item.path) {
            return Err(AddError::Blocked(why));
        }
        if let Some(cover) = self.items.iter().find(|x| is_within(&item.path, &x.path)) {
            return Err(AddError::AlreadyCovered(cover.path.clone()));
        }
        let before = self.items.len();
        self.items.retain(|x| !is_within(&x.path, &item.path));
        let dropped = before - self.items.len();
        self.items.push(item);
        Ok(dropped)
    }

    pub fn remove(&mut self, path: &Path) {
        self.items.retain(|x| normalize(&x.path) != normalize(path));
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.items.iter().any(|x| is_within(path, &x.path))
    }

    /// Refreshes sizes (e.g. items added from the live list before their folder was fully scanned).
    pub fn update(&mut self, f: impl Fn(&Path) -> Option<(u64, u64)>) {
        for it in &mut self.items {
            if let Some((size, items)) = f(&it.path) {
                it.size = size;
                it.items = items;
            }
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn items(&self) -> &[ReviewItem] {
        &self.items
    }

    pub fn total(&self) -> u64 {
        self.items.iter().map(|x| x.size).sum()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn item(p: &str, size: u64) -> ReviewItem {
        ReviewItem { path: PathBuf::from(p), size, is_dir: true, items: 0, note: String::new() }
    }

    #[test]
    fn refuses_blocked_and_covered_paths_and_dedupes_children() {
        let mut r = ReviewList::default();
        assert_eq!(r.add(item("C:\\Windows\\Temp", 1)), Err(AddError::Blocked("Windows system folder")));
        assert!(r.add(item("D:\\tmp\\a\\x", 10)).is_ok());
        assert!(r.add(item("D:\\tmp\\a\\y", 20)).is_ok());
        assert_eq!(r.add(item("D:\\tmp\\a", 100)), Ok(2));
        assert_eq!(r.len(), 1);
        assert_eq!(r.total(), 100);
        assert!(matches!(r.add(item("d:/TMP/a/z", 5)), Err(AddError::AlreadyCovered(_))));
        assert!(r.add(item("D:\\tmp\\ab", 7)).is_ok(), "sibling with a common prefix is not covered");
        r.remove(Path::new("d:\\tmp\\A"));
        assert_eq!(r.total(), 7);
    }
}
