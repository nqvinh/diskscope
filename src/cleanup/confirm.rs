//! Confirmation gate. `execute` only accepts a `ConfirmedPlan`, and a `ConfirmedPlan` can only be
//! built here (its fields are private), after the user's explicit answers:
//! - Recycle Bin: the review list was shown and the user ticked "I reviewed this list".
//! - Permanent: the same, plus a second confirmation dialog where the user typed DELETE.

use super::review::{ReviewItem, ReviewList};
use crate::rules::blocklist::blocked_reason;

pub const PERMANENT_WORD: &str = "DELETE";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    RecycleBin,
    Permanent,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfirmError {
    EmptyList,
    NotReviewed,
    NeedsSecondConfirmation,
    WrongWord,
    Blocked(String, &'static str),
}

impl std::fmt::Display for ConfirmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfirmError::EmptyList => write!(f, "the review list is empty"),
            ConfirmError::NotReviewed => write!(f, "tick 'I reviewed every path in this list' first"),
            ConfirmError::NeedsSecondConfirmation => write!(f, "permanent delete needs the second confirmation"),
            ConfirmError::WrongWord => write!(f, "type {PERMANENT_WORD} exactly to delete permanently"),
            ConfirmError::Blocked(p, why) => write!(f, "{p} is protected ({why})"),
        }
    }
}

/// A list the user explicitly confirmed. Not constructible outside this module.
#[derive(Debug)]
pub struct ConfirmedPlan {
    items: Vec<ReviewItem>,
    mode: Mode,
}

impl ConfirmedPlan {
    pub fn items(&self) -> &[ReviewItem] {
        &self.items
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
}

fn checked(list: &ReviewList, reviewed: bool) -> Result<Vec<ReviewItem>, ConfirmError> {
    if list.is_empty() {
        return Err(ConfirmError::EmptyList);
    }
    if !reviewed {
        return Err(ConfirmError::NotReviewed);
    }
    for it in list.items() {
        if let Some(why) = blocked_reason(&it.path) {
            return Err(ConfirmError::Blocked(it.path.display().to_string(), why));
        }
    }
    Ok(list.items().to_vec())
}

/// First (and for the Recycle Bin, only) confirmation.
pub fn confirm_recycle(list: &ReviewList, reviewed: bool) -> Result<ConfirmedPlan, ConfirmError> {
    Ok(ConfirmedPlan { items: checked(list, reviewed)?, mode: Mode::RecycleBin })
}

/// Permanent delete: needs the first confirmation, the second dialog, and the typed word.
pub fn confirm_permanent(
    list: &ReviewList,
    reviewed: bool,
    second_dialog_confirmed: bool,
    typed: &str,
) -> Result<ConfirmedPlan, ConfirmError> {
    let items = checked(list, reviewed)?;
    if !second_dialog_confirmed {
        return Err(ConfirmError::NeedsSecondConfirmation);
    }
    if typed.trim() != PERMANENT_WORD {
        return Err(ConfirmError::WrongWord);
    }
    Ok(ConfirmedPlan { items, mode: Mode::Permanent })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn list() -> ReviewList {
        let mut r = ReviewList::default();
        r.add(ReviewItem { path: PathBuf::from("D:\\tmp\\x"), size: 5, is_dir: true, items: 0, note: String::new() }).unwrap();
        r
    }

    #[test]
    fn recycle_needs_a_reviewed_non_empty_list() {
        assert_eq!(confirm_recycle(&ReviewList::default(), true).unwrap_err(), ConfirmError::EmptyList);
        assert_eq!(confirm_recycle(&list(), false).unwrap_err(), ConfirmError::NotReviewed);
        let plan = confirm_recycle(&list(), true).unwrap();
        assert_eq!(plan.mode(), Mode::RecycleBin);
        assert_eq!(plan.items().len(), 1);
    }

    #[test]
    fn permanent_needs_both_confirmations_and_the_word() {
        assert_eq!(confirm_permanent(&list(), false, true, "DELETE").unwrap_err(), ConfirmError::NotReviewed);
        assert_eq!(confirm_permanent(&list(), true, false, "DELETE").unwrap_err(), ConfirmError::NeedsSecondConfirmation);
        assert_eq!(confirm_permanent(&list(), true, true, "delete").unwrap_err(), ConfirmError::WrongWord);
        assert_eq!(confirm_permanent(&list(), true, true, "").unwrap_err(), ConfirmError::WrongWord);
        assert_eq!(confirm_permanent(&list(), true, true, "DELETE").unwrap().mode(), Mode::Permanent);
    }
}
