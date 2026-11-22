//! Shared result table for the Recent, Least used, Grew and Candidates tabs.

use super::loaded::Loaded;
use super::widgets::{node_menu, review_tick, Act};
use crate::cleanup::review::ReviewList;
use eframe::egui::{self, RichText, Ui};
use egui_extras::{Column, TableBuilder};

#[derive(Clone)]
pub struct ListRow {
    pub node: u32,
    /// Main amount (size, matching bytes or growth), already formatted.
    pub amount: String,
    pub time: String,
    pub note: String,
}

pub fn ui(ui: &mut Ui, id: &str, data: &Loaded, rows: &[ListRow], heads: [&str; 3], review: &ReviewList, acts: &mut Vec<Act>) {
    if rows.is_empty() {
        ui.weak("Nothing matches.");
        return;
    }
    let tree = &data.tree;
    TableBuilder::new(ui)
        .id_salt(id)
        .striped(true)
        .resizable(true)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(20.0))
        .column(Column::initial(84.0))
        .column(Column::initial(480.0).clip(true))
        .column(Column::initial(120.0))
        .column(Column::remainder().clip(true))
        .header(20.0, |mut h| {
            for t in ["", heads[0], "Path", heads[1], heads[2]] {
                h.col(|ui| {
                    ui.label(RichText::new(t).strong());
                });
            }
        })
        .body(|body| {
            body.rows(18.0, rows.len(), |mut row| {
                let r = &rows[row.index()];
                let path = tree.path(r.node).display().to_string();
                row.col(|ui| review_tick(ui, tree, r.node, review, if r.note.is_empty() { "picked in list" } else { &r.note }, acts));
                row.col(|ui| {
                    ui.label(&r.amount);
                });
                row.col(|ui| {
                    ui.label(&path).on_hover_text(&path);
                });
                row.col(|ui| {
                    ui.label(&r.time);
                });
                row.col(|ui| {
                    ui.label(&r.note).on_hover_text(&r.note);
                });
                let resp = row.response();
                if resp.clicked() {
                    acts.push(Act::Select { node: r.node, reveal: true });
                }
                resp.context_menu(|ui| node_menu(ui, tree, r.node, acts));
            });
        });
}
