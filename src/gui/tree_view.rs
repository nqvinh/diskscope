//! Left "Tree" tab: folder tree with % of parent, size, allocated, items, files, newest modified
//! and created dates (newest of everything inside), expand/collapse and sortable columns.

use super::loaded::Loaded;
use super::widgets::{folder_icon, node_menu, pct_bar, review_tick, sort_arrow, Act};
use crate::cleanup::review::ReviewList;
use crate::fmt::{count, date, size};
use crate::model::Tree;
use eframe::egui::{self, Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Size,
    Alloc,
    Items,
    Files,
    Modified,
    Created,
}

pub struct TreeView {
    expanded: HashSet<u32>,
    rows: Vec<(u32, u16)>,
    built_for: Option<(u64, SortKey, bool)>,
    dirty: bool,
    sort: SortKey,
    desc: bool,
    pub selected: Option<u32>,
    scroll_node: Option<u32>,
}

impl Default for TreeView {
    fn default() -> Self {
        Self { expanded: HashSet::from([0]), rows: Vec::new(), built_for: None, dirty: true, sort: SortKey::Size, desc: true, selected: None, scroll_node: None }
    }
}

pub fn sorted_children(tree: &Tree, i: u32, key: SortKey, desc: bool) -> Vec<u32> {
    let mut kids: Vec<u32> = tree.children(i).collect();
    let k = |c: &u32| {
        let n = tree.node(*c);
        match key {
            SortKey::Size | SortKey::Name => n.size,
            SortKey::Alloc => n.alloc,
            SortKey::Items => n.items as u64,
            SortKey::Files => n.files as u64,
            SortKey::Modified => n.modified as u64,
            SortKey::Created => n.created as u64,
        }
    };
    if key == SortKey::Name {
        kids.sort_by_cached_key(|c| tree.name(*c).to_lowercase());
    } else {
        kids.sort_by_key(k);
    }
    if desc != (key == SortKey::Name) {
        kids.reverse();
    }
    kids
}

impl TreeView {
    pub fn reset(&mut self) {
        *self = Self { sort: self.sort, desc: self.desc, ..Self::default() };
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
                for c in sorted_children(tree, i, self.sort, self.desc).into_iter().rev() {
                    stack.push((c, d + 1));
                }
            }
        }
        self.built_for = Some((generation, self.sort, self.desc));
        self.dirty = false;
    }

    pub fn ui(&mut self, ui: &mut Ui, data: &Loaded, review: &ReviewList, acts: &mut Vec<Act>) {
        let tree = &data.tree;
        if self.dirty || self.built_for != Some((data.generation, self.sort, self.desc)) {
            self.rebuild(tree, data.generation);
        }
        let scroll_to = self.scroll_node.take().and_then(|n| self.rows.iter().position(|r| r.0 == n));
        let mut toggle = None;
        let mut sort_click = None;
        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(20.0))
            .column(Column::initial(330.0).at_least(120.0).clip(true))
            .column(Column::initial(96.0))
            .columns(Column::initial(72.0), 4)
            .columns(Column::initial(112.0), 2);
        if let Some(r) = scroll_to {
            table = table.scroll_to_row(r, Some(egui::Align::Center));
        }
        let heads = [("", None), ("Folder", Some(SortKey::Name)), ("% of parent", Some(SortKey::Size)), ("Size", Some(SortKey::Size)),
            ("Allocated", Some(SortKey::Alloc)), ("Items", Some(SortKey::Items)), ("Files", Some(SortKey::Files)),
            ("Modified (newest)", Some(SortKey::Modified)), ("Created (newest)", Some(SortKey::Created))];
        table
            .header(20.0, |mut h| {
                for (label, key) in heads {
                    h.col(|ui| {
                        let Some(k) = key else { return };
                        if ui.selectable_label(false, RichText::new(label).strong()).clicked() {
                            sort_click = Some(k);
                        }
                        if self.sort == k && label != "% of parent" {
                            sort_arrow(ui, self.desc);
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
                    row.col(|ui| { ui.label(date(n.modified)); });
                    row.col(|ui| { ui.label(date(n.created)); });
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
        if let Some(k) = sort_click {
            if self.sort == k { self.desc = !self.desc } else { self.sort = k; self.desc = k != SortKey::Name; }
            self.dirty = true;
        }
    }
}
