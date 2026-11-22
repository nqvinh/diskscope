//! History tab: every delete action of the last 90 days (time, path, size, items, mode, result,
//! error), filterable by path text, mode and age, exportable to CSV.

use super::widgets::{sort_header, Act};
use crate::views::sorting::{sort_list, SortKey, SortState};
use crate::cleanup::history::{History, HistoryEntry, KEEP_DAYS};
use crate::fmt::{count, csv_field, size};
use eframe::egui::{self, Color32, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use std::io::Write;

pub struct HistoryView {
    entries: Vec<HistoryEntry>,
    loaded: bool,
    filter: String,
    mode: usize,
    days: u32,
    sort: SortState,
}

impl Default for HistoryView {
    fn default() -> Self {
        Self { entries: Vec::new(), loaded: false, filter: String::new(), mode: 0, days: KEEP_DAYS, sort: SortState { key: SortKey::Modified, desc: true } }
    }
}

const MODES: [&str; 3] = ["all", "recycle", "permanent"];

impl HistoryView {
    /// Re-read the file the next time the tab is shown (after a delete).
    pub fn invalidate(&mut self) {
        self.loaded = false;
    }

    fn visible(&self) -> Vec<&HistoryEntry> {
        let cutoff = crate::fmt::now().saturating_sub(self.days * 86_400);
        let needle = self.filter.to_lowercase();
        let mut v: Vec<&HistoryEntry> = self
            .entries
            .iter()
            .filter(|e| e.at_unix >= cutoff && (self.mode == 0 || e.mode == MODES[self.mode]) && (needle.is_empty() || e.path.to_lowercase().contains(&needle)))
            .collect();
        let d = self.sort.desc;
        match self.sort.key {
            SortKey::Size => sort_list(&mut v, d, |e| e.size, |e| std::cmp::Reverse(e.at_unix)),
            SortKey::Items => sort_list(&mut v, d, |e| e.items, |e| std::cmp::Reverse(e.at_unix)),
            SortKey::Name => sort_list(&mut v, d, |e| e.path.to_lowercase(), |e| std::cmp::Reverse(e.at_unix)),
            _ => sort_list(&mut v, d, |e| e.at_unix, |e| e.path.clone()),
        }
        v
    }

    pub fn ui(&mut self, ui: &mut Ui, acts: &mut Vec<Act>) {
        if !self.loaded {
            self.entries = History::default_location().load();
            self.loaded = true;
        }
        ui.horizontal(|ui| {
            ui.label("Path contains:");
            ui.add(egui::TextEdit::singleline(&mut self.filter).desired_width(240.0));
            for (k, m) in MODES.iter().enumerate() {
                ui.selectable_value(&mut self.mode, k, *m);
            }
            ui.label("last");
            ui.add(egui::DragValue::new(&mut self.days).range(1..=KEEP_DAYS).suffix(" days"));
            if ui.button("Export CSV…").clicked() {
                self.export(acts);
            }
            if ui.button("Open Recycle Bin").on_hover_text("Recycled items can be restored there (right-click > Restore).").clicked() {
                let _ = std::process::Command::new("explorer.exe").arg("shell:RecycleBinFolder").spawn();
            }
        });
        let rows: Vec<HistoryEntry> = self.visible().into_iter().cloned().collect();
        let recycled_ok = rows.iter().filter(|e| e.mode == "recycle" && e.result == "ok").count();
        if rows.is_empty() {
            ui.label(format!("No deletions in the last {} days. Everything you delete with diskscope will be listed here.", self.days));
        }
        ui.weak(format!("{} actions shown; kept for {KEEP_DAYS} days. {recycled_ok} recycled items can still be restored from the Recycle Bin until it is emptied.", rows.len()));
        TableBuilder::new(ui)
            .id_salt("history")
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .columns(Column::initial(118.0), 1)
            .columns(Column::initial(72.0), 2)
            .columns(Column::initial(76.0), 2)
            .column(Column::initial(420.0).clip(true))
            .column(Column::remainder().clip(true))
            .header(20.0, |mut h| {
                let s = &mut self.sort;
                h.col(|ui| sort_header(ui, "Time", SortKey::Modified, s));
                h.col(|ui| { ui.label(RichText::new("Mode").strong()); });
                h.col(|ui| { ui.label(RichText::new("Result").strong()); });
                h.col(|ui| sort_header(ui, "Size", SortKey::Size, s));
                h.col(|ui| sort_header(ui, "Items", SortKey::Items, s));
                h.col(|ui| sort_header(ui, "Path", SortKey::Name, s));
                h.col(|ui| { ui.label(RichText::new("Error / note").strong()); });
            })
            .body(|body| {
                body.rows(super::settings::row_h(), rows.len(), |mut row| {
                    let e = &rows[row.index()];
                    row.col(|ui| { ui.label(&e.at); });
                    row.col(|ui| { ui.label(&e.mode); });
                    row.col(|ui| {
                        let dark = super::settings::is_dark();
                        let c = match (e.result.as_str(), dark) { ("ok", true) => Color32::from_rgb(120, 210, 130), ("ok", false) => Color32::from_rgb(20, 120, 50), ("skipped", _) => super::settings::dim(), _ => super::settings::warn() };
                        ui.colored_label(c, &e.result);
                    });
                    row.col(|ui| { ui.label(size(e.size)); });
                    row.col(|ui| { ui.label(count(e.items)); });
                    row.col(|ui| { ui.label(&e.path).on_hover_text(&e.path); });
                    row.col(|ui| {
                        let note = if e.mode == "recycle" && e.result == "ok" { "in the Recycle Bin (restorable until emptied)" } else { e.error.as_str() };
                        ui.label(note);
                    });
                });
            });
    }

    fn export(&self, acts: &mut Vec<Act>) {
        let Some(file) = rfd::FileDialog::new().set_file_name("diskscope-history.csv").add_filter("CSV", &["csv"]).save_file() else { return };
        let write = || -> std::io::Result<usize> {
            let mut w = std::io::BufWriter::new(std::fs::File::create(&file)?);
            writeln!(w, "time,mode,result,size_bytes,items,path,error")?;
            let rows = self.visible();
            for e in &rows {
                writeln!(w, "{},{},{},{},{},{},{}", e.at, e.mode, e.result, e.size, e.items, csv_field(&e.path), csv_field(&e.error))?;
            }
            w.flush()?;
            Ok(rows.len())
        };
        acts.push(Act::Toast(match write() {
            Ok(n) => format!("exported {n} history rows"),
            Err(e) => format!("export failed: {e}"),
        }));
    }
}
