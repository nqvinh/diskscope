//! Left "Tree" tab: folder tree with % of parent, size, allocated, items, files, newest modified
//! and created dates (newest of everything inside), expand/collapse and sortable columns.

use super::loaded::Loaded;
use super::widgets::{date_cell, folder_icon, label_chip, node_menu, pct_bar, review_tick, sort_header, Act, ACCESS_NOTE};
use crate::rules::labels::Label;
use crate::views::sorting::{sorted_children, SortKey, SortState};
use crate::cleanup::review::ReviewList;
use crate::fmt::{count, size};
use crate::model::Tree;
use eframe::egui::{self, Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use std::collections::HashSet;


pub struct TreeView {
    expanded: HashSet<u32>,
    rows: Vec<(u32, u16)>,
    built_for: Option<(u64, SortState)>,
    dirty: bool,
    sort: SortState,
    pub selected: Option<u32>,
    scroll_node: Option<u32>,
}

impl Default for TreeView {
    fn default() -> Self {
        Self { expanded: HashSet::from([0]), rows: Vec::new(), built_for: None, dirty: true, sort: SortState::default(), selected: None, scroll_node: None }
    }
}


impl TreeView {
    pub fn reset(&mut self) {
        *self = Self { sort: self.sort, ..Self::default() };
    }

    /// Expands every parent of `node`, selects it and scrolls to it.
    pub fn reveal(&mut self, tree: &Tree, node: u32) {
        let mut p = tree.node(node).parent;
        while p != crate::model::NONE {
            self.expanded.insert(p);
            p = tree.node(p).parent;
        }
        self.selected = Some(node);
        self.dirty = true;
        self.scroll_node = Some(node);
    }

    fn rebuild(&mut self, tree: &Tree, generation: u64) {
        self.rows.clear();
        let mut stack = vec![(0u32, 0u16)];
        while let Some((i, d)) = stack.pop() {
            self.rows.push((i, d));
            if self.expanded.contains(&i) {
                for c in sorted_children(tree, i, self.sort).into_iter().rev() {
                    stack.push((c, d + 1));
                }
            }
        }
        self.built_for = Some((generation, self.sort));
        self.dirty = false;
    }

    pub fn ui(&mut self, ui: &mut Ui, data: &Loaded, review: &ReviewList, acts: &mut Vec<Act>) {
        let tree = &data.tree;
        if self.dirty || self.built_for != Some((data.generation, self.sort)) {
            self.rebuild(tree, data.generation);
        }
        let scroll_to = self.scroll_node.take().and_then(|n| self.rows.iter().position(|r| r.0 == n));
        let mut toggle = None;
        let before = self.sort;
        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(20.0))
            .column(Column::initial(250.0).at_least(120.0).clip(true))
            .column(Column::initial(84.0))
            .columns(Column::initial(68.0), 2)
            .columns(Column::initial(58.0), 2)
            .columns(Column::initial(86.0), 2)
            .column(Column::remainder().at_least(110.0));
        if let Some(r) = scroll_to {
            table = table.scroll_to_row(r, Some(egui::Align::Center));
        }
        let heads = [("", None), ("Name", Some(SortKey::Name)), ("% of parent", None), ("Size", Some(SortKey::Size)),
            ("Allocated", Some(SortKey::Alloc)), ("Items", Some(SortKey::Items)), ("Files", Some(SortKey::Files)),
            ("Last modified", Some(SortKey::Modified)), ("Last accessed", Some(SortKey::Accessed)),
            ("Label", Some(SortKey::Label))];
        let sort = &mut self.sort;
        table
            .header(20.0, |mut h| {
                for (label, key) in heads {
                    h.col(|ui| match key {
                        Some(k) => {
                            sort_header(ui, label, k, sort);
                            if k == SortKey::Accessed {
                                ui.label("").on_hover_text(ACCESS_NOTE);
                            }
                        }
                        None => {
                            ui.label(RichText::new(label).strong());
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(18.0, self.rows.len(), |mut row| {
                    let (i, depth) = self.rows[row.index()];
                    let n = *tree.node(i);
                    row.set_selected(self.selected == Some(i));
                    row.col(|ui| review_tick(ui, tree, i, review, "picked in tree", acts));
                    row.col(|ui| {
                        ui.add_space(depth as f32 * 14.0);
                        if n.is_dir() && n.first_child != crate::model::NONE {
                            let open = self.expanded.contains(&i);
                            if ui.add(egui::Button::new(if open { "−" } else { "+" }).small().frame(false)).clicked() {
                                toggle = Some(i);
                            }
                        } else {
                            ui.add_space(14.0);
                        }
                        if n.is_dir() {
                            folder_icon(ui, if n.is_reparse() { Color32::GRAY } else { Color32::from_rgb(230, 190, 70) });
                        } else {
                            ui.add_space(14.0);
                        }
                        let mut t = RichText::new(tree.name(i));
                        if n.is_denied() { t = t.color(Color32::from_rgb(230, 110, 90)); }
                        if n.is_reparse() || n.is_group() { t = t.italics().color(Color32::GRAY); }
                        let tip = if n.is_denied() { "access denied: not scanned" } else if n.is_reparse() { "link / junction / mount point: not followed" } else { "" };
                        let l = ui.label(t);
                        if !tip.is_empty() { l.on_hover_text(tip); }
                    });
                    let parent = if i == 0 { n.size } else { tree.node(n.parent).size };
                    row.col(|ui| pct_bar(ui, if parent == 0 { 0.0 } else { n.size as f32 / parent as f32 }));
                    row.col(|ui| { ui.label(size(n.size)); });
                    row.col(|ui| { ui.label(size(n.alloc)); });
                    row.col(|ui| { if n.is_dir() { ui.label(count(n.items as u64)); } });
                    row.col(|ui| { if n.is_dir() { ui.label(count(n.files as u64)); } });
                    row.col(|ui| date_cell(ui, n.modified, if n.is_dir() { "newest change inside" } else { "" }));
                    row.col(|ui| date_cell(ui, n.accessed, ACCESS_NOTE));
                    row.col(|ui| label_chip(ui, Label::from_u8(n.label)));
                    let resp = row.response();
                    if resp.clicked() { acts.push(Act::Select { node: i, reveal: false }); }
                    if resp.double_clicked() && n.is_dir() { toggle = Some(i); }
                    resp.context_menu(|ui| node_menu(ui, tree, i, acts));
                });
            });
        if let Some(i) = toggle {
            if !self.expanded.remove(&i) { self.expanded.insert(i); }
            self.dirty = true;
        }
        if self.sort != before {
            self.dirty = true;
        }
    }
}
