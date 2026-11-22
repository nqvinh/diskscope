//! "Compare scans" tab: pick two saved scans of the current root and see which folders grew,
//! shrank, appeared or disappeared (see views::compare). Sortable; right-click opens the folder.

use super::loaded::Loaded;
use super::widgets::{label_chip, sort_header, Act};
use crate::fmt::{date, size};
use crate::rules::labels::Label;
use crate::views::compare::{diff, Change, DiffRow};
use crate::views::snapshot::load;
use crate::views::snapshot_history::{list, SnapInfo};
use crate::views::sorting::{sort_list, SortKey, SortState};
use eframe::egui::{self, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use std::path::PathBuf;

// Column ids reuse SortKey: Name = folder, Size = difference, Alloc = before, Items = after,
// Score = kind of change, Label = label.
pub struct CompareView {
    snaps: Vec<SnapInfo>,
    listed_for: Option<(PathBuf, u64)>,
    older: usize,
    newer: usize,
    min_mb: u64,
    rows: Vec<DiffRow>,
    root: String,
    computed_for: Option<(PathBuf, PathBuf, u64)>,
    sort: SortState,
    note: String,
}

impl Default for CompareView {
    fn default() -> Self {
        Self { snaps: Vec::new(), listed_for: None, older: 1, newer: 0, min_mb: 10, rows: Vec::new(), root: String::new(), computed_for: None, sort: SortState { key: SortKey::Size, desc: true }, note: String::new() }
    }
}

fn pick(ui: &mut Ui, id: &str, snaps: &[SnapInfo], sel: &mut usize) {
    let newest = snaps.first().map(|s| s.taken_at);
    let text = |s: &SnapInfo| format!("{}{}  ({})", if Some(s.taken_at) == newest { "latest: " } else { "" }, date(s.taken_at), size(s.size));
    egui::ComboBox::from_id_salt(id).selected_text(snaps.get(*sel).map(text).unwrap_or_default()).show_ui(ui, |ui| {
        for (i, s) in snaps.iter().enumerate() {
            ui.selectable_value(sel, i, text(s));
        }
    });
}

impl CompareView {
    fn refresh(&mut self, data: &Loaded) {
        let key = (data.tree.root_path.clone(), data.generation);
        if self.listed_for.as_ref() != Some(&key) {
            self.snaps = list(&data.tree.root_path);
            (self.newer, self.older) = (0, 1);
            self.listed_for = Some(key);
            self.computed_for = None;
        }
        let (Some(a), Some(b)) = (self.snaps.get(self.older), self.snaps.get(self.newer)) else { return };
        let (old, new) = if a.taken_at <= b.taken_at { (a, b) } else { (b, a) };
        let want = (old.file.clone(), new.file.clone(), self.min_mb);
        if self.computed_for.as_ref() == Some(&want) {
            return;
        }
        match (load(&old.file), load(&new.file)) {
            (Some(o), Some(n)) => {
                self.rows = diff(&o, &n, self.min_mb << 20);
                self.root = n.root;
                self.note = format!("{} -> {}: {} ({}{})", date(o.taken_at), date(n.taken_at), size(new.size), if new.size >= old.size { "+" } else { "-" }, size(new.size.abs_diff(old.size)));
            }
            _ => (self.rows, self.note) = (Vec::new(), "could not read one of the saved scans".into()),
        }
        self.computed_for = Some(want);
    }

    fn full(&self, rel: &str) -> PathBuf {
        if self.root.is_empty() { PathBuf::from(rel) } else if rel.is_empty() { PathBuf::from(&self.root) } else { PathBuf::from(&self.root).join(rel) }
    }

    pub fn ui(&mut self, ui: &mut Ui, data: &Loaded, acts: &mut Vec<Act>) {
        self.refresh(data);
        if self.snaps.len() < 2 {
            ui.add_space(20.0);
            ui.label(RichText::new("Only one scan of this folder or drive is saved so far.").strong());
            ui.label("Every finished scan is kept (up to 20 per folder or drive). Scan it again later to see what grew, shrank, appeared or disappeared.");
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Compare");
            pick(ui, "cmp-old", &self.snaps, &mut self.older);
            ui.label("with");
            pick(ui, "cmp-new", &self.snaps, &mut self.newer);
            ui.label("changes of at least");
            ui.add(egui::DragValue::new(&mut self.min_mb).range(1..=100_000).suffix(" MB"));
            ui.weak(&self.note);
        });
        let count = |c: Change| self.rows.iter().filter(|r| r.change == c).count();
        ui.label(format!("{} grew · {} shrank · {} new · {} gone  (a folder is left out when one of its sub-folders explains 90 % of its change)", count(Change::Grew), count(Change::Shrank), count(Change::New), count(Change::Gone)));
        match self.sort.key {
            SortKey::Alloc => sort_list(&mut self.rows, self.sort.desc, |r| r.before, |r| r.rel.clone()),
            SortKey::Items => sort_list(&mut self.rows, self.sort.desc, |r| r.after, |r| r.rel.clone()),
            SortKey::Name => sort_list(&mut self.rows, self.sort.desc, |r| r.rel.to_lowercase(), |r| r.before),
            SortKey::Score => sort_list(&mut self.rows, self.sort.desc, |r| r.change, |r| r.rel.clone()),
            SortKey::Label => sort_list(&mut self.rows, self.sort.desc, |r| Label::from_u8(r.label).name(), |r| r.rel.clone()),
            _ => sort_list(&mut self.rows, self.sort.desc, |r| r.delta().unsigned_abs(), |r| r.rel.clone()),
        }
        let mut sort = self.sort;
        TableBuilder::new(ui)
            .id_salt("compare")
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(70.0))
            .column(Column::initial(90.0))
            .column(Column::initial(80.0))
            .column(Column::initial(80.0))
            .column(Column::initial(120.0))
            .column(Column::remainder().clip(true))
            .header(20.0, |mut h| {
                h.col(|ui| sort_header(ui, "Change", SortKey::Score, &mut sort));
                h.col(|ui| sort_header(ui, "Difference", SortKey::Size, &mut sort));
                h.col(|ui| sort_header(ui, "Before", SortKey::Alloc, &mut sort));
                h.col(|ui| sort_header(ui, "After", SortKey::Items, &mut sort));
                h.col(|ui| sort_header(ui, "Label", SortKey::Label, &mut sort));
                h.col(|ui| sort_header(ui, "Folder", SortKey::Name, &mut sort));
            })
            .body(|body| {
                body.rows(super::settings::row_h(), self.rows.len(), |mut row| {
                    let r = &self.rows[row.index()];
                    let path = self.full(&r.rel);
                    let warm = matches!(r.change, Change::Grew | Change::New);
                    row.col(|ui| { ui.label(RichText::new(r.change.label()).color(if warm { super::settings::warn() } else { super::settings::dim() })); });
                    row.col(|ui| { ui.label(format!("{}{}", if r.delta() >= 0 { "+" } else { "-" }, size(r.delta().unsigned_abs()))); });
                    row.col(|ui| { ui.label(size(r.before)); });
                    row.col(|ui| { ui.label(size(r.after)); });
                    row.col(|ui| label_chip(ui, Label::from_u8(r.label)));
                    row.col(|ui| { ui.label(path.display().to_string()); });
                    row.response().context_menu(|ui| {
                        if let Some(n) = data.tree.find(&path) {
                            if ui.button("Show in tree").clicked() {
                                acts.push(Act::Select { node: n, reveal: true });
                                ui.close();
                            }
                        }
                        if ui.button("Open in Explorer").clicked() {
                            acts.push(Act::Explorer(path.clone()));
                            ui.close();
                        }
                    });
                });
            });
        self.sort = sort;
    }
}
