//! Shared result table for the Recent, Least used, Grew, Candidates and By label tabs: sortable
//! headers (size, path, date, label; click again to flip) and a label chip per row.

use super::loaded::Loaded;
use super::widgets::{date_cell, label_chip, node_menu, review_tick, sort_header, Act};
use crate::cleanup::review::ReviewList;
use crate::rules::labels::Label;
use crate::views::sorting::{sort_list, SortKey, SortState};
use eframe::egui::{self, RichText, Ui};
use egui_extras::{Column, TableBuilder};

#[derive(Clone)]
pub struct ListRow {
    pub node: u32,
    /// Main amount (size, matching bytes or growth): raw for sorting, text for display.
    pub bytes: u64,
    pub amount: String,
    /// Date shown in the date column (0 = none).
    pub when: u32,
    /// Extra text column (confidence, before/after sizes, ...).
    pub extra: String,
    pub note: String,
}

/// `heads` = [amount, extra, note] column titles.
pub fn ui(ui: &mut Ui, id: &str, data: &Loaded, rows: &[ListRow], heads: [&str; 3], review: &ReviewList, acts: &mut Vec<Act>) {
    if rows.is_empty() {
        ui.weak("Nothing matches.");
        return;
    }
    let tree = &data.tree;
    let state_id = egui::Id::unique(("list-sort", id));
    let mut sort: SortState = ui.data(|d| d.get_temp(state_id)).unwrap_or_default();
    let before = sort;
    let mut order: Vec<usize> = (0..rows.len()).collect();
    let label_of = |r: &ListRow| Label::from_u8(tree.node(r.node).label);
    match sort.key {
        SortKey::Name => sort_list(&mut order, sort.desc, |&k| tree.path(rows[k].node), |&k| k),
        SortKey::Modified => sort_list(&mut order, sort.desc, |&k| rows[k].when, |&k| rows[k].bytes),
        SortKey::Label => sort_list(&mut order, sort.desc, |&k| label_of(&rows[k]).name(), |&k| std::cmp::Reverse(rows[k].bytes)),
        _ => sort_list(&mut order, sort.desc, |&k| rows[k].bytes, |&k| k),
    }
    TableBuilder::new(ui)
        .id_salt(id)
        .striped(true)
        .resizable(true)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(20.0))
        .column(Column::initial(84.0))
        .column(Column::initial(420.0).clip(true))
        .column(Column::initial(90.0))
        .column(Column::initial(120.0))
        .column(Column::initial(120.0).clip(true))
        .column(Column::remainder().clip(true))
        .header(20.0, |mut h| {
            h.col(|_| {});
            h.col(|ui| sort_header(ui, heads[0], SortKey::Size, &mut sort));
            h.col(|ui| sort_header(ui, "Path", SortKey::Name, &mut sort));
            h.col(|ui| sort_header(ui, "Last change", SortKey::Modified, &mut sort));
            h.col(|ui| sort_header(ui, "Label", SortKey::Label, &mut sort));
            h.col(|ui| { ui.label(RichText::new(heads[1]).strong()); });
            h.col(|ui| { ui.label(RichText::new(heads[2]).strong()); });
        })
        .body(|body| {
            body.rows(18.0, order.len(), |mut row| {
                let r = &rows[order[row.index()]];
                let path = tree.path(r.node).display().to_string();
                row.col(|ui| review_tick(ui, tree, r.node, review, if r.note.is_empty() { "picked in list" } else { &r.note }, acts));
                row.col(|ui| { ui.label(&r.amount); });
                row.col(|ui| { ui.label(&path).on_hover_text(&path); });
                row.col(|ui| date_cell(ui, r.when, ""));
                row.col(|ui| label_chip(ui, label_of(r)));
                row.col(|ui| { ui.label(&r.extra); });
                row.col(|ui| { ui.label(&r.note).on_hover_text(&r.note); });
                let resp = row.response();
                if resp.clicked() {
                    acts.push(Act::Select { node: r.node, reveal: true });
                }
                resp.context_menu(|ui| node_menu(ui, tree, r.node, acts));
            });
        });
    if sort != before {
        ui.data_mut(|d| d.insert_temp(state_id, sort));
    }
}
