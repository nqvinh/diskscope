//! "Files" tab: every scanned file of the selected extension(s) or group, with a name/path filter,
//! a minimum size, sortable columns (default size desc) and row actions. Only the selected subset
//! is filtered and sorted; paths are built for visible rows only.

use super::loaded::Loaded;
use super::widgets::{date_cell, label_chip, node_menu, review_tick, sort_header, Act, ACCESS_NOTE};
use crate::cleanup::export::{write_csv, ExportRow};
use crate::cleanup::review::{ReviewItem, ReviewList};
use crate::fmt::{count, size};
use crate::rules::labels::Label;
use crate::views::ext_files::{select, totals, Query};
use crate::views::sorting::{SortKey, SortState};
use eframe::egui::{self, Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};

#[derive(Default)]
pub struct FilesView {
    filter: String,
    min_mb: u32,
    sort: SortState,
    key: Option<(u64, Vec<u16>, String, u32, SortState)>,
    rows: Vec<u32>,
}

impl FilesView {
    pub fn ui(&mut self, ui: &mut Ui, data: &Loaded, exts: &[u16], review: &ReviewList, acts: &mut Vec<Act>) {
        let tree = &data.tree;
        if exts.is_empty() {
            ui.label("Click an extension in the table on the right (Ctrl+click to add more, or pick a group such as Video) to list its files here.");
            return;
        }
        let key = (data.generation, exts.to_vec(), self.filter.clone(), self.min_mb, self.sort);
        if self.key.as_ref() != Some(&key) {
            let q = Query { exts, filter: &self.filter, min_size: self.min_mb as u64 * (1 << 20), sort: self.sort };
            self.rows = select(tree, &data.ext_index, &q);
            self.key = Some(key);
        }
        let (files, bytes) = totals(tree, &self.rows);
        let names: Vec<&str> = exts.iter().map(|&e| tree.exts[e as usize].as_str()).collect();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} {} files, {}", count(files), names.join(" "), size(bytes))).strong().size(15.0));
            ui.label("Filter:");
            ui.add(egui::TextEdit::singleline(&mut self.filter).desired_width(200.0).hint_text("name or path contains"));
            ui.label("Min size:");
            ui.add(egui::DragValue::new(&mut self.min_mb).range(0..=1_000_000).suffix(" MB"));
            if ui.button("Add all shown to Review").on_hover_text("Only fills the Review list; nothing is deleted").clicked() {
                acts.extend(self.rows.iter().filter(|&&i| !tree.node(i).is_group()).map(|&i| {
                    let n = tree.node(i);
                    Act::AddItem(ReviewItem { path: tree.path(i), size: n.size, is_dir: false, items: 0, note: format!("{} file", tree.ext(i)) })
                }));
            }
            if ui.button("Export CSV…").clicked() {
                export(data, &self.rows, acts);
            }
        });
        ui.weak("Double-click a row to show it in the tree. Select a row, then Del / Shift+Del or the buttons below (always with confirmation).");
        let mut sort = self.sort;
        TableBuilder::new(ui)
            .id_salt("files")
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(20.0))
            .column(Column::initial(220.0).clip(true))
            .column(Column::initial(330.0).clip(true))
            .column(Column::initial(80.0))
            .columns(Column::initial(90.0), 2)
            .column(Column::remainder().at_least(110.0))
            .header(20.0, |mut h| {
                h.col(|_| {});
                h.col(|ui| sort_header(ui, "Name", SortKey::Name, &mut sort));
                h.col(|ui| { ui.label(RichText::new("Folder").strong()); });
                h.col(|ui| sort_header(ui, "Size", SortKey::Size, &mut sort));
                h.col(|ui| sort_header(ui, "Last modified", SortKey::Modified, &mut sort));
                h.col(|ui| sort_header(ui, "Last accessed", SortKey::Accessed, &mut sort));
                h.col(|ui| sort_header(ui, "Label", SortKey::Label, &mut sort));
            })
            .body(|body| {
                body.rows(18.0, self.rows.len(), |mut row| {
                    let i = self.rows[row.index()];
                    let n = tree.node(i);
                    row.col(|ui| review_tick(ui, tree, i, review, "picked in Files", acts));
                    row.col(|ui| { ui.label(tree.name(i)); });
                    row.col(|ui| {
                        let folder = tree.path(n.parent).display().to_string();
                        ui.label(RichText::new(&folder).color(Color32::GRAY)).on_hover_text(folder);
                    });
                    row.col(|ui| { ui.label(size(n.size)); });
                    row.col(|ui| date_cell(ui, n.modified, ""));
                    row.col(|ui| date_cell(ui, n.accessed, ACCESS_NOTE));
                    row.col(|ui| label_chip(ui, Label::from_u8(n.label)));
                    let resp = row.response();
                    if resp.clicked() {
                        acts.push(Act::Select { node: i, reveal: false });
                    }
                    if resp.double_clicked() {
                        acts.push(Act::Select { node: i, reveal: true });
                    }
                    resp.context_menu(|ui| node_menu(ui, tree, i, acts));
                });
            });
        self.sort = sort;
    }
}

fn export(data: &Loaded, rows: &[u32], acts: &mut Vec<Act>) {
    let Some(file) = rfd::FileDialog::new().set_file_name("diskscope-files.csv").add_filter("CSV", &["csv"]).save_file() else { return };
    let t = &data.tree;
    let out: Vec<ExportRow> = rows
        .iter()
        .map(|&i| ExportRow { path: t.path(i).display().to_string(), size: t.node(i).size, kind: t.ext(i).to_string(), reason: Label::from_u8(t.node(i).label).name().to_string() })
        .collect();
    acts.push(Act::Toast(match write_csv(&file, &out) {
        Ok(()) => format!("exported {} rows", out.len()),
        Err(e) => format!("export failed: {e}"),
    }));
}
