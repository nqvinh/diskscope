//! Review tab: everything the user added, with the total, row selection, CSV export and the two
//! Explorer-style delete actions. Delete (Del) = Recycle Bin, Permanently delete (Shift+Del) =
//! bypass the bin. Both act on the selected rows (or the whole list when none is selected) and
//! always open the confirmation dialog first.

use super::widgets::{sort_header, Act};
use crate::views::sorting::{sort_list, SortKey, SortState};
use crate::cleanup::export::{write_csv, ExportRow};
use crate::cleanup::review::{ReviewItem, ReviewList};
use crate::fmt::size;
use eframe::egui::{self, RichText, Ui};
use std::collections::HashSet;
use std::path::PathBuf;

/// The selected rows, or every row when nothing is selected.
pub fn targets(review: &ReviewList, selected: &HashSet<PathBuf>, all_if_none: bool) -> Vec<ReviewItem> {
    let sel: Vec<ReviewItem> = review.items().iter().filter(|i| selected.contains(&i.path)).cloned().collect();
    if sel.is_empty() && all_if_none { review.items().to_vec() } else { sel }
}

pub fn ui(ui: &mut Ui, review: &ReviewList, selected: &mut HashSet<PathBuf>, bar: &super::action_bar::Targets, acts: &mut Vec<Act>) {
    selected.retain(|p| review.items().iter().any(|i| &i.path == p));
    super::action_bar::buttons(ui, bar, acts);
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("Review list: {} items, {} in total", review.len(), size(review.total()))).strong());
        if ui.add_enabled(!review.is_empty(), egui::Button::new("Export list to CSV…")).clicked() {
            export(review, acts);
        }
    });
    ui.weak("Click rows to select them (Del / Shift+Del act on the selection). Nothing is deleted without the confirmation dialog. Protected system and profile folders can never be added.");
    ui.separator();
    if review.is_empty() {
        ui.label("Add items from the Tree, Recent, Least used, Grew or Candidates tabs, or from the live suggestions while scanning.");
        return;
    }
    let state_id = egui::Id::unique("review-sort");
    let mut sort: SortState = ui.data(|d| d.get_temp(state_id)).unwrap_or_default();
    let before = sort;
    let mut items: Vec<&ReviewItem> = review.items().iter().collect();
    match sort.key {
        SortKey::Name => sort_list(&mut items, sort.desc, |i| i.path.clone(), |i| i.size),
        _ => sort_list(&mut items, sort.desc, |i| i.size, |i| i.path.clone()),
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Grid::new("review-grid").striped(true).num_columns(4).show(ui, |ui| {
            ui.label("");
            ui.horizontal(|ui| sort_header(ui, "Size", SortKey::Size, &mut sort));
            ui.horizontal(|ui| sort_header(ui, "Path (click rows to select)", SortKey::Name, &mut sort));
            ui.label(RichText::new("Why").strong());
            ui.end_row();
            for it in items {
                if ui.small_button("Remove").clicked() {
                    acts.push(Act::Unreview(it.path.clone()));
                }
                ui.label(RichText::new(size(it.size)).monospace());
                let on = selected.contains(&it.path);
                if ui.selectable_label(on, it.path.display().to_string()).clicked() {
                    if on { selected.remove(&it.path); } else { selected.insert(it.path.clone()); }
                }
                ui.weak(&it.note);
                ui.end_row();
            }
        });
    });
    if sort != before {
        ui.data_mut(|d| d.insert_temp(state_id, sort));
    }
}

fn export(review: &ReviewList, acts: &mut Vec<Act>) {
    let Some(file) = rfd::FileDialog::new().set_file_name("diskscope-review.csv").add_filter("CSV", &["csv"]).save_file() else { return };
    let rows: Vec<ExportRow> = review
        .items()
        .iter()
        .map(|it| ExportRow { path: it.path.display().to_string(), size: it.size, kind: if it.is_dir { "folder" } else { "file" }.into(), reason: it.note.clone() })
        .collect();
    match write_csv(&file, &rows) {
        Ok(()) => {
            crate::cleanup::action_log::ActionLog::default_location().record("export-review", &file, rows.len() as u64, "done", "");
            acts.push(Act::Toast(format!("exported {} rows", rows.len())));
        }
        Err(e) => acts.push(Act::Toast(format!("export failed: {e}"))),
    }
}
