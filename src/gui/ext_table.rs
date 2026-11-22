//! Right panel: size per file extension, with the colour used in the treemap.

use super::loaded::Loaded;
use super::widgets::{pct_bar, rgb};
use crate::fmt::{count, size};
use crate::scan::extensions::type_name;
use eframe::egui::{self, RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};

pub fn ui(ui: &mut Ui, data: &Loaded) {
    let total = data.tree.node(0).size.max(1);
    TableBuilder::new(ui)
        .id_salt("ext-table")
        .striped(true)
        .resizable(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(16.0))
        .column(Column::initial(70.0).clip(true))
        .column(Column::initial(130.0).clip(true))
        .column(Column::initial(70.0))
        .column(Column::initial(70.0))
        .column(Column::remainder().at_least(60.0))
        .header(20.0, |mut h| {
            for t in ["", "Extension", "Type", "Percent", "Size", "Files"] {
                h.col(|ui| {
                    ui.label(RichText::new(t).strong());
                });
            }
        })
        .body(|body| {
            body.rows(18.0, data.ext_stats.len(), |mut row| {
                let s = &data.ext_stats[row.index()];
                let ext = &data.tree.exts[s.ext as usize];
                row.col(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
                    ui.painter().rect_filled(r, 2.0, rgb(data.palette[s.ext as usize]));
                });
                row.col(|ui| {
                    ui.label(if ext.is_empty() { "(none)" } else { ext });
                });
                row.col(|ui| {
                    ui.label(type_name(ext));
                });
                row.col(|ui| pct_bar(ui, s.size as f32 / total as f32));
                row.col(|ui| {
                    ui.label(size(s.size));
                });
                row.col(|ui| {
                    ui.label(count(s.files));
                });
            });
        });
}
