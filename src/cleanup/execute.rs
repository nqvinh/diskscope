//! Runs a confirmed plan. Every item is re-checked against the blocklist right before it is
//! touched; missing, protected, in-use or denied items are skipped and reported, never retried.

use super::action_log::ActionLog;
use super::confirm::{ConfirmedPlan, Mode};
use crate::rules::blocklist::blocked_reason;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub enum ItemResult {
    Done,
    Skipped(String),
}

#[derive(Clone, Debug)]
pub struct Outcome {
    pub path: PathBuf,
    pub size: u64,
    pub result: ItemResult,
}

/// Executes `plan` (consumed: a plan runs once). `on_item` gets the index of each finished item.
pub fn execute(plan: ConfirmedPlan, log: &ActionLog, mut on_item: impl FnMut(usize, &Outcome)) -> Vec<Outcome> {
    let action = match plan.mode() {
        Mode::RecycleBin => "recycle",
        Mode::Permanent => "delete-permanent",
    };
    let mut out = Vec::with_capacity(plan.items().len());
    for (k, it) in plan.items().iter().enumerate() {
        let result = run_one(&it.path, it.is_dir, plan.mode());
        let (res, detail) = match &result {
            ItemResult::Done => ("done", String::new()),
            ItemResult::Skipped(why) => ("skipped", why.clone()),
        };
        log.record(action, &it.path, it.size, res, &detail);
        let o = Outcome { path: it.path.clone(), size: it.size, result };
        on_item(k, &o);
        out.push(o);
    }
    out
}

fn run_one(path: &Path, is_dir: bool, mode: Mode) -> ItemResult {
    if let Some(why) = blocked_reason(path) {
        return ItemResult::Skipped(format!("protected: {why}"));
    }
    let Ok(md) = std::fs::symlink_metadata(path) else {
        return ItemResult::Skipped("not found (already gone?)".into());
    };
    if md.is_dir() != is_dir {
        return ItemResult::Skipped("changed since the scan (file/folder type differs)".into());
    }
    let r = match mode {
        Mode::RecycleBin => trash::delete(path).map_err(|e| explain_trash(&e)),
        Mode::Permanent if md.is_dir() && !md.file_type().is_symlink() => std::fs::remove_dir_all(path).map_err(|e| explain(&e)),
        Mode::Permanent if md.is_dir() => std::fs::remove_dir(path).map_err(|e| explain(&e)),
        Mode::Permanent => std::fs::remove_file(path).map_err(|e| explain(&e)),
    };
    match r {
        // The shell can report success while skipping an item (e.g. too big for the bin): verify.
        Ok(()) if std::fs::symlink_metadata(path).is_ok() => ItemResult::Skipped("still there after the operation (skipped by Windows)".into()),
        Ok(()) => ItemResult::Done,
        Err(why) => ItemResult::Skipped(why),
    }
}

fn explain(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(32) | Some(33) => "in use by another program".into(),
        Some(5) => "access denied".into(),
        _ => e.to_string(),
    }
}

fn explain_trash(e: &trash::Error) -> String {
    let s = e.to_string();
    if s.contains("0x80070020") || s.to_lowercase().contains("being used") {
        "in use by another program".into()
    } else if s.contains("0x80070005") {
        "access denied".into()
    } else {
        format!("could not move to the Recycle Bin: {s}")
    }
}

#[cfg(test)]
mod tests {
    use super::super::confirm::{confirm_permanent, confirm_recycle};
    use super::super::review::{ReviewItem, ReviewList};
    use super::*;

    #[test]
    fn permanent_plan_deletes_only_listed_items_and_logs() {
        let base = std::env::temp_dir().join(format!("diskscope-exec-{}", std::process::id()));
        let (gone, keep) = (base.join("gone"), base.join("keep"));
        std::fs::create_dir_all(gone.join("sub")).unwrap();
        std::fs::create_dir_all(&keep).unwrap();
        std::fs::write(gone.join("sub").join("f.txt"), b"x").unwrap();
        let mut list = ReviewList::default();
        list.add(ReviewItem { path: gone.clone(), size: 1, is_dir: true, items: 0, note: String::new() }).unwrap();
        list.add(ReviewItem { path: base.join("missing"), size: 0, is_dir: true, items: 0, note: String::new() }).unwrap();
        assert!(confirm_recycle(&list, false).is_err(), "no plan without the review tick");
        let plan = confirm_permanent(&list, true, true, "DELETE").unwrap();
        let log = ActionLog::new(base.join("log"));
        let out = execute(plan, &log, |_, _| {});
        assert_eq!(out[0].result, ItemResult::Done);
        assert!(matches!(out[1].result, ItemResult::Skipped(_)));
        assert!(!gone.exists() && keep.exists());
        let lines = std::fs::read_to_string(base.join("log").join("actions.jsonl")).unwrap();
        assert_eq!(lines.lines().count(), 2);
        std::fs::remove_dir_all(&base).ok();
    }
}
