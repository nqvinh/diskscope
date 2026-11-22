//! Settings sections for the user's lists: excludes (skip during scan / hide from views) and
//! "never suggest". Both are saved with the other settings; removing an entry undoes it.

use super::settings::{dim, Settings};
use crate::rules::excludes::{Exclude, ExcludeMode};
use eframe::egui::{self, RichText, Ui};

#[derive(Default)]
pub struct ListsUi {
    new_pattern: String,
    new_mode: Option<ExcludeMode>,
}

/// What the user did in these sections.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub struct ListsOut {
    pub changed: bool,
    /// "Scan again to apply" was pressed.
    pub rescan: bool,
}

impl ListsUi {
    pub fn ui(&mut self, ui: &mut Ui, s: &mut Settings, can_rescan: bool) -> ListsOut {
        let mut out = ListsOut::default();
        ui.label(RichText::new("Excluded from scans").strong());
        ui.label(RichText::new("A name (node_modules, *.iso) or a full path (D:\\VMs). Skip = not scanned and not counted. Hide = counted in folder sizes but never shown. Takes effect on the next scan.").color(dim()));
        let mut remove = None;
        egui::ScrollArea::vertical().id_salt("excl-list").max_height(140.0).show(ui, |ui| {
            if s.excludes.is_empty() {
                ui.weak("Nothing excluded.");
            }
            for (i, e) in s.excludes.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                    for m in [ExcludeMode::Skip, ExcludeMode::Hide] {
                        out.changed |= ui.selectable_value(&mut e.mode, m, if m == ExcludeMode::Skip { "Skip" } else { "Hide" }).changed();
                    }
                    ui.label(&e.pattern);
                });
            }
        });
        if let Some(i) = remove {
            s.excludes.remove(i);
            out.changed = true;
        }
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.new_pattern).desired_width(220.0).hint_text("name or full path"));
            let mode = self.new_mode.get_or_insert(ExcludeMode::Skip);
            ui.selectable_value(mode, ExcludeMode::Skip, "Skip");
            ui.selectable_value(mode, ExcludeMode::Hide, "Hide");
            let pattern = self.new_pattern.trim().to_string();
            let dup = s.excludes.iter().any(|e| e.pattern.eq_ignore_ascii_case(&pattern));
            if ui.add_enabled(!pattern.is_empty() && !dup, egui::Button::new("Add")).clicked() {
                s.excludes.push(Exclude { pattern, mode: *mode });
                self.new_pattern.clear();
                out.changed = true;
            }
        });
        out.rescan = ui.add_enabled(can_rescan, egui::Button::new("Scan again to apply")).on_disabled_hover_text("scan something first").clicked();
        ui.add_space(8.0);
        ui.label(RichText::new("Never suggested").strong());
        ui.label(RichText::new("Right-click a suggestion and pick \"Never suggest this\". Remove an entry to see it suggested again.").color(dim()));
        let mut remove = None;
        egui::ScrollArea::vertical().id_salt("never-list").max_height(120.0).show(ui, |ui| {
            if s.never_suggest.is_empty() {
                ui.weak("Nothing here yet.");
            }
            for (i, p) in s.never_suggest.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                    ui.label(p);
                });
            }
        });
        if let Some(i) = remove {
            s.never_suggest.remove(i);
            out.changed = true;
        }
        out
    }
}
