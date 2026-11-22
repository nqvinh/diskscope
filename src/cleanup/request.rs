//! Explorer-style delete keys. Del = move to the Recycle Bin, Shift+Del = delete permanently.
//! Either way the result is only a REQUEST: it opens the confirmation dialog for exactly these
//! targets (permanent still needs the typed word); nothing is deleted here.

use super::confirm::Mode;
use super::review::{ReviewItem, ReviewList};

#[derive(Debug)]
pub struct DeleteRequest {
    pub mode: Mode,
    /// The targets, checked against the blocklist and de-duplicated like the review list.
    pub list: ReviewList,
}

pub fn mode_for(shift: bool) -> Mode {
    if shift { Mode::Permanent } else { Mode::RecycleBin }
}

/// Builds the request for Del (`shift` = false) or Shift+Del on the selected targets.
pub fn delete_request(shift: bool, targets: Vec<ReviewItem>) -> Result<DeleteRequest, String> {
    if targets.is_empty() {
        return Err("nothing selected".into());
    }
    let mut list = ReviewList::default();
    for t in targets {
        let p = t.path.display().to_string();
        list.add(t).map_err(|e| format!("{p}: {e}"))?;
    }
    Ok(DeleteRequest { mode: mode_for(shift), list })
}

#[cfg(test)]
mod tests {
    use super::super::confirm::{confirm_permanent, confirm_recycle};
    use super::*;
    use std::path::PathBuf;

    fn item(p: &str) -> ReviewItem {
        ReviewItem { path: PathBuf::from(p), size: 10, is_dir: true, items: 1, note: String::new() }
    }

    #[test]
    fn del_routes_to_recycle_bin_and_shift_del_to_permanent() {
        let del = delete_request(false, vec![item("D:\\tmp\\a")]).unwrap();
        assert_eq!(del.mode, Mode::RecycleBin);
        let shift_del = delete_request(true, vec![item("D:\\tmp\\a"), item("D:\\tmp\\b")]).unwrap();
        assert_eq!(shift_del.mode, Mode::Permanent);
        assert_eq!(shift_del.list.len(), 2);
        // Still only a request: the dialogs decide.
        assert!(confirm_recycle(&del.list, false).is_err());
        assert!(confirm_permanent(&shift_del.list, true, true, "").is_err(), "Shift+Del still needs the typed word");
        assert!(confirm_permanent(&shift_del.list, true, true, "DELETE").is_ok());
    }

    #[test]
    fn empty_or_protected_selections_are_refused() {
        assert!(delete_request(false, vec![]).is_err());
        let e = delete_request(true, vec![item("D:\\tmp\\a"), item("C:\\Windows")]).unwrap_err();
        assert!(e.contains("protected"), "{e}");
    }
}
