//! Runs a confirmed plan. Every item is re-checked against the blocklist right before it is
//! touched; missing, protected, in-use or denied items are skipped and reported, never retried.

use super::action_log::ActionLog;
use super::confirm::{ConfirmedPlan, Mode};
use super::fast_delete::{in_delete_pool, remove_file_force, remove_tree_here, DeleteProgress};
use crate::rules::blocklist::blocked_reason;
use rayon::prelude::*;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;

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

/// Executes `plan` (consumed: a plan runs once). Recycle Bin: one batched Windows operation for
/// every valid item, then each item is verified. Permanent: the items run in parallel
/// on one shared delete pool (so different volumes overlap too); Cancel (in `progress`) is
/// honoured between items and between files.
pub fn execute(plan: ConfirmedPlan, log: &ActionLog, progress: &DeleteProgress) -> Vec<Outcome> {
    let mode = plan.mode();
    let items = plan.items();
    let mut results: Vec<Option<ItemResult>> = items.iter().map(|it| precheck(&it.path, it.is_dir).err().map(ItemResult::Skipped)).collect();
    match mode {
        Mode::RecycleBin => {
            let ok: Vec<&PathBuf> = items.iter().zip(&results).filter(|(_, r)| r.is_none()).map(|(it, _)| &it.path).collect();
            if !ok.is_empty() && trash::delete_all(&ok).is_err() {
                // The batch failed as a whole: retry one by one so each item gets its own reason.
                for (it, r) in items.iter().zip(results.iter_mut()).filter(|(_, r)| r.is_none()) {
                    if let Err(e) = trash::delete(&it.path) {
                        *r = Some(ItemResult::Skipped(explain_trash(&e)));
                    }
                }
            }
            progress.items_done.store(items.len() as u64, Relaxed);
        }
        Mode::Permanent => in_delete_pool(|| {
            items.par_iter().zip(results.par_iter_mut()).with_max_len(1).for_each(|(it, r)| {
                if r.is_none() {
                    *r = if progress.cancel.load(Relaxed) {
                        Some(ItemResult::Skipped("cancelled before this item".into()))
                    } else {
                        delete_permanently(&it.path, progress).err().map(ItemResult::Skipped)
                    };
                }
                progress.items_done.fetch_add(1, Relaxed);
            })
        }),
    }
    let action = if mode == Mode::RecycleBin { "recycle" } else { "delete-permanent" };
    items
        .iter()
        .zip(results)
        .map(|(it, r)| {
            // The shell can report success while skipping an item (e.g. too big for the bin): verify.
            let result = r.unwrap_or_else(|| {
                if std::fs::symlink_metadata(&it.path).is_ok() { ItemResult::Skipped("still there after the operation (skipped by Windows)".into()) } else { ItemResult::Done }
            });
            let (res, detail) = match &result { ItemResult::Done => ("done", String::new()), ItemResult::Skipped(w) => ("skipped", w.clone()) };
            log.record(action, &it.path, it.size, res, &detail);
            Outcome { path: it.path.clone(), size: it.size, result }
        })
        .collect()
}

fn precheck(path: &Path, is_dir: bool) -> Result<(), String> {
    if let Some(why) = blocked_reason(path) {
        return Err(format!("protected: {why}"));
    }
    let md = std::fs::symlink_metadata(path).map_err(|_| "not found (already gone?)".to_string())?;
    if md.is_dir() != is_dir && !md.file_type().is_symlink() {
        return Err("changed since the scan (file/folder type differs)".into());
    }
    Ok(())
}

fn delete_permanently(path: &Path, progress: &DeleteProgress) -> Result<(), String> {
    let md = std::fs::symlink_metadata(path).map_err(|e| explain(&e))?;
    if md.file_type().is_symlink() || md.file_attributes() & 0x400 != 0 {
        // A link: remove the link itself, never what it points to.
        return if md.is_dir() || md.file_attributes() & 0x10 != 0 { std::fs::remove_dir(path) } else { std::fs::remove_file(path) }.map_err(|e| explain(&e));
    }
    if md.is_dir() {
        remove_tree_here(path, progress)
    } else {
        remove_file_force(path).map_err(|e| explain(&e))?;
        progress.files_done.fetch_add(1, Relaxed);
        progress.bytes_done.fetch_add(md.len(), Relaxed);
        Ok(())
    }
}
pub fn explain(e: &std::io::Error) -> String {
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
        let out = execute(plan, &log, &DeleteProgress::default());
        assert_eq!(out[0].result, ItemResult::Done);
        assert!(matches!(out[1].result, ItemResult::Skipped(_)));
        assert!(!gone.exists() && keep.exists());
        let lines = std::fs::read_to_string(base.join("log").join("actions.jsonl")).unwrap();
        assert_eq!(lines.lines().count(), 2);
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn many_items_delete_in_parallel_and_only_those() {
        let base = std::env::temp_dir().join(format!("diskscope-exec-par-{}", std::process::id()));
        let mut list = ReviewList::default();
        for i in 0..40 {
            let d = base.join(format!("item{i:02}")).join("deep").join("er");
            std::fs::create_dir_all(&d).unwrap();
            for f in 0..5 {
                std::fs::write(d.join(format!("f{f}.bin")), [1u8; 100]).unwrap();
            }
            list.add(ReviewItem { path: base.join(format!("item{i:02}")), size: 500, is_dir: true, items: 0, note: String::new() }).unwrap();
        }
        std::fs::create_dir_all(base.join("keep")).unwrap();
        let plan = confirm_permanent(&list, true, true, "DELETE").unwrap();
        let progress = DeleteProgress::default();
        let out = execute(plan, &ActionLog::new(base.join("log")), &progress);
        assert_eq!(out.len(), 40);
        assert!(out.iter().all(|o| o.result == ItemResult::Done), "{out:?}");
        assert_eq!((progress.items_done.load(Relaxed), progress.files_done.load(Relaxed)), (40, 200));
        assert!(base.join("keep").exists() && !base.join("item00").exists());
        std::fs::remove_dir_all(&base).ok();
    }
}
