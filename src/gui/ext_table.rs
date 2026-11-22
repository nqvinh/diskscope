//! Right panel: size per file extension with its treemap colour. Click an extension to list its
//! files (Ctrl+click adds to the selection); group chips (Video, Images, ...) select whole groups.

use super::loaded::Loaded;
use super::widgets::{pct_bar, rgb, Act};
use crate::fmt::{count, size};
use crate::rules::labels::Label;
use crate::scan::extensions::type_name;
use crate::views::ext_files::exts_of_group;
use eframe::egui::{self, RichText, Sense, Ui};
use egui_extras::{Column, TableBuilder};

const GROUPS: [Label; 7] = [Label::Video, Label::Images, Label::Audio, Label::Documents, Label::Archives, Label::Installers, Label::Vms];

pub fn ui(ui: &mut Ui, data: &Loaded, selected: &[u16], acts: &mut Vec<Act>) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Groups:").weak());
        for g in GROUPS {
            let exts = exts_of_group(&data.tree, g);
            if !exts.is_empty() && ui.small_button(g.name()).on_hover_text("list every file of this kind").clicked() {
                acts.push(Act::ExtSelect { exts, add: false });
            }
        }
    });
    let total = data.tree.node(0).size.max(1);
    let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.shift);
    TableBuilder::new(ui)
        .id_salt("ext-table")
        .striped(true)
        .resizable(true)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(16.0))
        .column(Column::initial(70.0).clip(true))
        .column(Column::initial(120.0).clip(true))
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
            body.rows(super::settings::row_h(), data.ext_stats.len(), |mut row| {
                let s = &data.ext_stats[row.index()];
                let ext = &data.tree.exts[s.ext as usize];
                row.set_selected(selected.contains(&s.ext));
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
                if row.response().on_hover_text("click: list these files · Ctrl+click: add to the list").clicked() {
                    acts.push(Act::ExtSelect { exts: vec![s.ext], add: ctrl });
                }
            });
        });
}
