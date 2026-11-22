//! Shown while a scan runs: the best cleanup suggestions found so far, ranked by reclaimable size
//! x confidence, growing as files are counted. Nothing is ticked; rows can be added to Review.

use super::widgets::Act;
use crate::cleanup::review::{ReviewItem, ReviewList};
use crate::fmt::{date, size};
use crate::rules::candidates::{Confidence, LiveSink, LiveSuggestion};
use eframe::egui::{self, Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};

fn item(s: &LiveSuggestion) -> ReviewItem {
    ReviewItem { path: s.path.clone(), size: s.bytes, is_dir: true, items: 0, note: format!("{}: {}", s.kind, s.reason) }
}

pub fn confidence_color(c: Confidence) -> Color32 {
    match c {
        Confidence::Safe => Color32::from_rgb(120, 210, 130),
        Confidence::Likely => Color32::from_rgb(230, 200, 100),
        Confidence::Check => Color32::from_rgb(240, 140, 100),
    }
}

pub fn ui(ui: &mut Ui, sink: &LiveSink, review: &ReviewList, acts: &mut Vec<Act>) {
    let list = sink.ranked(1 << 20);
    let safe: Vec<&LiveSuggestion> = list.iter().filter(|s| s.confidence == Confidence::Safe && !review.contains(&s.path)).collect();
    ui.horizontal(|ui| {
        ui.heading("Best cleanup suggestions so far");
        ui.weak("(sizes grow while the scan runs; nothing is ticked for you)");
        let total: u64 = safe.iter().map(|s| s.bytes).sum();
        if ui.add_enabled(!safe.is_empty(), egui::Button::new(format!("Add all safe ({}, {})", safe.len(), size(total))))
            .on_hover_text("Adds the 'safe cache' rows to the Review list only. Nothing is deleted.")
            .clicked()
        {
            acts.extend(safe.iter().map(|s| Act::AddItem(item(s))));
        }
    });
    if list.is_empty() {
        ui.label("Looking for caches, build outputs, old app versions and finished agent worktrees…");
        return;
    }
    TableBuilder::new(ui)
        .id_salt("live")
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(52.0))
        .column(Column::initial(84.0))
        .column(Column::initial(84.0))
        .column(Column::initial(110.0))
        .column(Column::initial(118.0))
        .column(Column::initial(430.0).clip(true))
        .column(Column::remainder().clip(true))
        .header(20.0, |mut h| {
            for t in ["", "Size so far", "Confidence", "Kind", "Last change", "Path", "Why"] {
                h.col(|ui| {
                    ui.label(RichText::new(t).strong());
                });
            }
        })
        .body(|body| {
            body.rows(18.0, list.len(), |mut row| {
                let s = &list[row.index()];
                let listed = review.contains(&s.path);
                row.col(|ui| {
                    if ui.add_enabled(!listed, egui::Button::new(if listed { "Added" } else { "Add" }).small()).on_hover_text("add to the Review list").clicked() {
                        acts.push(Act::AddItem(item(s)));
                    }
                });
                row.col(|ui| { ui.label(size(s.bytes)); });
                row.col(|ui| { ui.colored_label(confidence_color(s.confidence), s.confidence.label()); });
                row.col(|ui| { ui.label(s.kind); });
                row.col(|ui| { ui.label(date(s.modified)); });
                row.col(|ui| { ui.label(s.path.display().to_string()).on_hover_text(s.path.display().to_string()); });
                row.col(|ui| { ui.label(&s.reason).on_hover_text(&s.reason); });
            });
        });
}
