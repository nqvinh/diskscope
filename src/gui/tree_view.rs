//! Left "Tree" tab: folder tree with % of parent, size, allocated, items, files, newest modified
//! and created dates (newest of everything inside), expand/collapse and sortable columns.

use super::loaded::Loaded;
use super::tree_columns::{cell, chooser, visible_cols};
use super::widgets::{folder_icon, node_menu, review_tick, sort_header, Act};
use crate::views::sorting::{sorted_children, SortKey, SortState};
use crate::cleanup::review::ReviewList;
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

    /// Draws the tree. `hidden` is the user's hidden-columns mask (settings); returns true when the
    /// user changed it through the header's right-click column chooser.
    pub fn ui(&mut self, ui: &mut Ui, data: &Loaded, review: &ReviewList, hidden: &mut u16, acts: &mut Vec<Act>) -> bool {
        let tree = &data.tree;
        if self.dirty || self.built_for != Some((data.generation, self.sort)) {
            self.rebuild(tree, data.generation);
        }
        let scroll_to = self.scroll_node.take().and_then(|n| self.rows.iter().position(|r| r.0 == n));
        let mut toggle = None;
        let before = self.sort;
        let cols = visible_cols(*hidden);
        let mut mask_changed = false;
        // Fixed column widths inside a horizontal scroll area: every column stays reachable at any text size.
        egui::ScrollArea::horizontal()
            .id_salt("tree-h")
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .show(ui, |ui| {
            // Leave room under the rows for the horizontal scrollbar.
            let body_h = (ui.available_height() - 40.0).max(60.0);
            let mut table = TableBuilder::new(ui)
                .max_scroll_height(body_h)
                .striped(true)
                .resizable(true)
                .sense(egui::Sense::click())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::exact(20.0))
                .column(Column::initial(260.0).at_least(120.0).clip(true));
            for c in &cols {
                table = table.column(Column::initial(c.width()).at_least(40.0).clip(true));
            }
            if let Some(r) = scroll_to {
                table = table.scroll_to_row(r, Some(egui::Align::Center));
            }
            let sort = &mut self.sort;
            table
                .header(20.0, |mut h| {
                    let menu = |resp: egui::Response, hidden: &mut u16, changed: &mut bool| {
                        resp.on_hover_text("right-click: choose columns").context_menu(|ui| *changed |= chooser(ui, hidden));
                    };
                    h.col(|_| {});
                    let (_, r) = h.col(|ui| sort_header(ui, "Name", SortKey::Name, sort));
                    menu(r, hidden, &mut mask_changed);
                    for c in &cols {
                        let (_, r) = h.col(|ui| match c.sort_key() {
                            Some(k) => sort_header(ui, c.title(), k, sort),
                            None => {
                                ui.label(RichText::new(c.title()).strong());
                            }
                        });
                        menu(r, hidden, &mut mask_changed);
                    }
                })
                .body(|body| {
                    body.rows(super::settings::row_h(), self.rows.len(), |mut row| {
                        let (i, depth) = self.rows[row.index()];
                        let n = *tree.node(i);
                        row.set_selected(self.selected == Some(i));
                        row.col(|ui| review_tick(ui, tree, i, review, "picked in tree", acts));
                        row.col(|ui| {
                            ui.add_space(depth as f32 * 14.0);
                            if n.is_dir() && tree.children(i).next().is_some() {
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
                            if n.is_denied() { t = t.color(super::settings::warn()); }
                            if n.is_reparse() || n.is_group() { t = t.italics().color(super::settings::dim()); }
                            let tip = if n.is_denied() { "access denied: not scanned" } else if n.is_reparse() { "link / junction / mount point: not followed" } else { "" };
                            let l = ui.label(t);
                            if !tip.is_empty() { l.on_hover_text(tip); }
                        });
                        for c in &cols {
                            row.col(|ui| cell(ui, *c, tree, i, &n));
                        }
                        let resp = row.response();
                        if resp.clicked() { acts.push(Act::Select { node: i, reveal: false }); }
                        if resp.double_clicked() && n.is_dir() { toggle = Some(i); }
                        resp.context_menu(|ui| node_menu(ui, tree, i, acts));
                    });
                });
        });
        if let Some(i) = toggle {
            if !self.expanded.remove(&i) { self.expanded.insert(i); }
            self.dirty = true;
        }
        if self.sort != before {
            self.dirty = true;
        }
        mask_changed
    }
}