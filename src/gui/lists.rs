//! Recent, Least used, Grew and Candidates tabs. Results are cached per (scan generation,
//! settings) and recomputed only when one of them changes.

use super::list_table::{self, ListRow};
use super::loaded::Loaded;
use super::widgets::Act;
use crate::cleanup::export::{write_csv, ExportRow};
use crate::cleanup::review::ReviewList;
use crate::fmt::{date, size};
use crate::views::queries::{files_where, folders_where, TimeField};
use crate::views::snapshot::grew;
use eframe::egui::{self, Color32, RichText, Ui};
use std::path::Path;

const LIMIT: usize = 2000;

pub struct TimeQuery {
    pub days: u32,
    pub field: TimeField,
    pub folders: bool,
    pub under: String,
    key: Option<(u64, u32, TimeField, bool, String)>,
    rows: Vec<ListRow>,
    error: Option<String>,
}

impl TimeQuery {
    pub fn new(days: u32, field: TimeField) -> Self {
        Self { days, field, folders: false, under: String::new(), key: None, rows: Vec::new(), error: None }
    }

    /// `recent` = newer than N days; otherwise older than N days (least used).
    fn compute(&mut self, data: &Loaded, recent: bool) {
        let key = (data.generation, self.days, self.field, self.folders, self.under.clone());
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.key = Some(key);
        self.error = None;
        let tree = &data.tree;
        let root = if self.under.trim().is_empty() { Some(0) } else { tree.find(Path::new(self.under.trim())) };
        let Some(root) = root else {
            self.error = Some("that folder is not part of this scan".into());
            self.rows.clear();
            return;
        };
        let cut = crate::fmt::now().saturating_sub(self.days.saturating_mul(86_400));
        let f = self.field;
        let pred = move |n: &crate::model::Node| if recent { f.of(n) >= cut } else { f.of(n) < cut };
        let hits = if self.folders { folders_where(tree, root, LIMIT, 1 << 20, pred) } else { files_where(tree, root, LIMIT, pred) };
        self.rows = hits
            .iter()
            .map(|h| ListRow {
                node: h.node,
                bytes: h.bytes,
                amount: size(h.bytes),
                when: f.of(tree.node(h.node)),
                extra: if self.folders { format!("of {}", size(tree.node(h.node).size)) } else { String::new() },
                note: String::new(),
            })
            .collect();
    }

    fn controls(&mut self, ui: &mut Ui, recent: bool, fields: &[(TimeField, &str)]) {
        ui.horizontal(|ui| {
            ui.label(if recent { "Changed in the last" } else { "Not used for" });
            ui.add(egui::DragValue::new(&mut self.days).range(1..=3650).suffix(" days"));
            for &(f, label) in fields {
                ui.selectable_value(&mut self.field, f, label);
            }
            ui.separator();
            ui.selectable_value(&mut self.folders, false, "Files");
            ui.selectable_value(&mut self.folders, true, "Folders");
            ui.separator();
            ui.label("Under:");
            ui.add(egui::TextEdit::singleline(&mut self.under).desired_width(260.0).hint_text("whole scan, or a sub-folder path"));
        });
    }

    pub fn ui_recent(&mut self, ui: &mut Ui, data: &Loaded, review: &ReviewList, acts: &mut Vec<Act>) {
        self.controls(ui, true, &[(TimeField::Changed, "created or modified"), (TimeField::Created, "created"), (TimeField::Modified, "modified")]);
        self.body(ui, data, true, review, acts, "Recent");
    }

    pub fn ui_least_used(&mut self, ui: &mut Ui, data: &Loaded, review: &ReviewList, acts: &mut Vec<Act>) {
        if data.last_access != Some(true) {
            self.field = TimeField::Modified;
            ui.colored_label(Color32::from_rgb(240, 190, 90),
                "NTFS last-access updates are disabled (or unknown) on this PC, so 'last used' falls back to the modified time.");
            self.controls(ui, false, &[(TimeField::Modified, "by modified time")]);
        } else {
            self.controls(ui, false, &[(TimeField::Accessed, "by last access"), (TimeField::Modified, "by modified time")]);
        }
        self.body(ui, data, false, review, acts, "Least used");
    }

    fn body(&mut self, ui: &mut Ui, data: &Loaded, recent: bool, review: &ReviewList, acts: &mut Vec<Act>, id: &str) {
        self.compute(data, recent);
        if let Some(e) = &self.error {
            ui.colored_label(Color32::from_rgb(230, 110, 90), e);
            return;
        }
        let what = if self.folders { "folders where those bytes are" } else { "files, biggest first" };
        ui.weak(format!("{} {what} (max {LIMIT})", self.rows.len()));
        let heads = if self.folders { ["Matching", "Folder total", ""] } else { ["Size", "", ""] };
        list_table::ui(ui, id, data, &self.rows, heads, review, acts);
    }
}

pub struct GrewView {
    pub min_mb: u32,
    key: Option<(u64, u32)>,
    rows: Vec<ListRow>,
}

impl GrewView {
    pub fn new(min_mb: u32) -> Self {
        Self { min_mb, key: None, rows: Vec::new() }
    }

    pub fn ui(&mut self, ui: &mut Ui, data: &Loaded, review: &ReviewList, acts: &mut Vec<Act>) {
        let Some(prev) = &data.prev_snapshot else {
            ui.label("No earlier scan of this path yet. The baseline was saved now; scan again later to see what grew.");
            return;
        };
        ui.horizontal(|ui| {
            ui.label(format!("Compared with the scan of {}. Show folders that grew by at least", date(prev.taken_at)));
            ui.add(egui::DragValue::new(&mut self.min_mb).range(1..=100_000).suffix(" MB"));
        });
        if self.key != Some((data.generation, self.min_mb)) {
            self.key = Some((data.generation, self.min_mb));
            let tree = &data.tree;
            self.rows = grew(tree, prev, self.min_mb as u64 * (1 << 20), LIMIT)
                .iter()
                .map(|g| ListRow { node: g.node, bytes: g.new - g.old, amount: format!("+{}", size(g.new - g.old)), when: tree.node(g.node).modified, extra: format!("now {}", size(g.new)), note: format!("was {}", size(g.old)) })
                .collect();
        }
        list_table::ui(ui, "grew", data, &self.rows, ["Grew", "Now", "Before"], review, acts);
    }
}

/// `action` draws "Clean with <tool>" on rows whose folder is a tool's own cache.
pub fn candidates_ui(ui: &mut Ui, data: &Loaded, review: &ReviewList, acts: &mut Vec<Act>, action: list_table::RowAction) {
    let total = crate::rules::candidates::total_without_overlap(&data.tree, &data.candidates);
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} suggestions, {} in total", data.candidates.len(), size(total))).strong());
        let safe: Vec<_> = data.candidates.iter().filter(|c| c.confidence == crate::rules::candidates::Confidence::Safe && !review.contains(&data.tree.path(c.node))).collect();
        let safe_total: u64 = safe.iter().map(|c| c.size).sum();
        if ui.add_enabled(!safe.is_empty(), egui::Button::new(format!("Add all safe ({}, {})", safe.len(), size(safe_total))))
            .on_hover_text("Adds every 'safe cache' suggestion to the Review list. Nothing is deleted.")
            .clicked()
        {
            acts.push(Act::AddAllSafe);
        }
        ui.weak("Ranked by size x confidence. Nothing is ticked for you.");
        if ui.button("Export to CSV…").clicked() {
            export(data, acts);
        }
    });
    let rows: Vec<ListRow> = data
        .candidates
        .iter()
        .map(|c| ListRow {
            node: c.node,
            bytes: c.size,
            amount: size(c.size),
            when: c.modified,
            extra: c.confidence.label().to_string(),
            note: format!("{}: {}", c.kind, c.reason),
        })
        .collect();
    list_table::ui_with(ui, "candidates", data, &rows, ["Size", "Confidence / action", "Kind: why"], review, acts, action);
}

fn export(data: &Loaded, acts: &mut Vec<Act>) {
    let Some(file) = rfd::FileDialog::new().set_file_name("diskscope-candidates.csv").add_filter("CSV", &["csv"]).save_file() else { return };
    let rows: Vec<ExportRow> = data
        .candidates
        .iter()
        .map(|c| ExportRow { path: data.tree.path(c.node).display().to_string(), size: c.size, kind: c.kind.into(), reason: c.reason.clone() })
        .collect();
    let log = crate::cleanup::action_log::ActionLog::default_location();
    match write_csv(&file, &rows) {
        Ok(()) => {
            log.record("export-candidates", &file, rows.len() as u64, "done", "");
            acts.push(Act::Toast(format!("exported {} rows to {}", rows.len(), file.display())));
        }
        Err(e) => acts.push(Act::Toast(format!("export failed: {e}"))),
    }
}
