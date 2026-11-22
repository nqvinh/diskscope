//! Tree table columns: which exist, which the user hid (bit mask saved in settings), widths, and
//! how each data cell is drawn. Name and Size are always shown.

use super::widgets::{date_cell, label_chip, pct_bar, ACCESS_NOTE};
use crate::fmt::{count, size};
use crate::model::{Node, Tree};
use crate::rules::labels::Label;
use crate::views::sorting::SortKey;
use eframe::egui::Ui;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Col {
    Pct,
    Size,
    Alloc,
    Items,
    Files,
    Modified,
    Accessed,
    Label,
}

pub const DATA_COLS: [Col; 8] = [Col::Pct, Col::Size, Col::Alloc, Col::Items, Col::Files, Col::Modified, Col::Accessed, Col::Label];

impl Col {
    pub fn title(self) -> &'static str {
        match self {
            Col::Pct => "% of parent",
            Col::Size => "Size",
            Col::Alloc => "Allocated",
            Col::Items => "Items",
            Col::Files => "Files",
            Col::Modified => "Last modified",
            Col::Accessed => "Last accessed",
            Col::Label => "Label",
        }
    }
    pub fn sort_key(self) -> Option<SortKey> {
        match self {
            Col::Pct => None,
            Col::Size => Some(SortKey::Size),
            Col::Alloc => Some(SortKey::Alloc),
            Col::Items => Some(SortKey::Items),
            Col::Files => Some(SortKey::Files),
            Col::Modified => Some(SortKey::Modified),
            Col::Accessed => Some(SortKey::Accessed),
            Col::Label => Some(SortKey::Label),
        }
    }
    pub fn width(self) -> f32 {
        match self {
            Col::Pct => 84.0,
            Col::Size | Col::Alloc => 72.0,
            Col::Items | Col::Files => 62.0,
            Col::Modified | Col::Accessed => 92.0,
            Col::Label => 120.0,
        }
    }
    /// Bit in the hidden-columns mask (Size cannot be hidden).
    fn bit(self) -> u16 {
        match self {
            Col::Size => 0,
            c => 1 << (DATA_COLS.iter().position(|x| *x == c).unwrap_or(0) as u16),
        }
    }
    pub fn visible(self, hidden: u16) -> bool {
        self.bit() == 0 || hidden & self.bit() == 0
    }
}

pub fn visible_cols(hidden: u16) -> Vec<Col> {
    DATA_COLS.iter().copied().filter(|c| c.visible(hidden)).collect()
}

/// Right-click menu on the header: tick columns on/off. Returns true when the mask changed.
pub fn chooser(ui: &mut Ui, hidden: &mut u16) -> bool {
    ui.label(eframe::egui::RichText::new("Show columns").strong());
    let mut changed = false;
    for c in DATA_COLS.iter().copied().filter(|c| c.bit() != 0) {
        let mut on = c.visible(*hidden);
        if ui.checkbox(&mut on, c.title()).changed() {
            *hidden ^= c.bit();
            changed = true;
        }
    }
    changed
}

pub fn cell(ui: &mut Ui, c: Col, tree: &Tree, i: u32, n: &Node) {
    match c {
        Col::Pct => {
            let parent = if i == 0 { n.size } else { tree.node(n.parent).size };
            pct_bar(ui, if parent == 0 { 0.0 } else { n.size as f32 / parent as f32 });
        }
        Col::Size => { ui.label(size(n.size)); }
        Col::Alloc => { ui.label(size(n.alloc)); }
        Col::Items => { if n.is_dir() { ui.label(count(n.items as u64)); } }
        Col::Files => { if n.is_dir() { ui.label(count(n.files as u64)); } }
        Col::Modified => date_cell(ui, n.modified, if n.is_dir() { "newest change inside" } else { "" }),
        Col::Accessed => date_cell(ui, n.accessed, ACCESS_NOTE),
        Col::Label => label_chip(ui, Label::from_u8(n.label)),
    }
}

#[test]
fn hide_mask_keeps_size_and_toggles_others() {
    assert_eq!(visible_cols(0).len(), 8);
    let hidden = Col::Alloc.bit() | Col::Accessed.bit();
    let v = visible_cols(hidden);
    assert!(!v.contains(&Col::Alloc) && !v.contains(&Col::Accessed) && v.contains(&Col::Label));
    assert!(visible_cols(u16::MAX).contains(&Col::Size), "Size can never be hidden");
}
