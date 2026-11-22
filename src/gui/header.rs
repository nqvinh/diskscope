//! Top bar: what to scan (drive list, folder path, Browse), Scan/Cancel and progress, plus the
//! drive totals, Recycle Bin size and memory use (refreshed every few seconds).

use super::loaded::{Loaded, ScanJob};
use crate::fmt::{count, size};
use crate::sys::{self, Drive, Memory};
use eframe::egui::{self, Color32, RichText, Ui};
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

pub struct Header {
    pub path: String,
    drives: Vec<Drive>,
    recycle: Option<(u64, u64)>,
    mem: Memory,
    refreshed: Option<Instant>,
    elevated: bool,
}

impl Header {
    pub fn new(path: String) -> Self {
        Self { path, drives: Vec::new(), recycle: None, mem: Memory::default(), refreshed: None, elevated: sys::is_elevated() }
    }

    pub fn recycle_bytes(&self) -> Option<u64> {
        self.recycle.map(|r| r.0)
    }

    fn refresh(&mut self) {
        if self.refreshed.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            self.drives = sys::drives();
            self.recycle = sys::recycle_bin();
            self.mem = sys::memory();
            self.refreshed = Some(Instant::now());
        }
    }

    /// Returns a path when the user asked to start a scan.
    pub fn drives(&self) -> &[Drive] {
        &self.drives
    }

    pub fn elevated(&self) -> bool {
        self.elevated
    }

    /// Returns a path when the user asked to start a scan. `settings_open` toggles the settings window.
    pub fn ui(&mut self, ui: &mut Ui, job: &Option<ScanJob>, data: &Option<Loaded>, settings_open: &mut bool) -> Option<PathBuf> {
        self.refresh();
        let mut start = None;
        ui.horizontal(|ui| {
            let gear = ui.add(egui::Button::new("      Settings")).on_hover_text("Text size, theme, colours, row height");
            super::widgets::gear_icon(ui, gear.rect.left_center() + egui::vec2(14.0, 0.0), 7.0, ui.visuals().text_color());
            if gear.clicked() {
                *settings_open = !*settings_open;
            }
            ui.separator();
            ui.label("Scan:");
            egui::ComboBox::from_id_salt("drive").selected_text("Drive").width(70.0).show_ui(ui, |ui| {
                if self.drives.len() > 1 && ui.selectable_label(false, "All drives (This PC)").clicked() {
                    self.path = crate::model::graft::ALL_DRIVES.to_string();
                }
                for d in &self.drives {
                    let label = format!("{}  {} free of {}", d.root, size(d.free), size(d.total));
                    if ui.selectable_label(false, label).clicked() {
                        self.path = d.root.clone();
                    }
                }
            });
            ui.add(egui::TextEdit::singleline(&mut self.path).desired_width(360.0).hint_text("C:\\ or any folder"));
            if ui.button("Browse…").clicked() {
                if let Some(p) = rfd::FileDialog::new().set_directory(&self.path).pick_folder() {
                    self.path = p.display().to_string();
                }
            }
            match job {
                Some(j) => {
                    if ui.button("Cancel").clicked() {
                        j.cancel();
                    }
                }
                None => {
                    let ok = !self.path.trim().is_empty();
                    if ui.add_enabled(ok, egui::Button::new(RichText::new("  Scan  ").strong())).clicked() {
                        start = Some(PathBuf::from(self.path.trim()));
                    }
                }
            }
            if !self.elevated
                && ui.button("Restart as administrator").on_hover_text("Fast NTFS scan straight from the MFT (like WizTree). Windows asks for permission.").clicked()
                && sys::restart_elevated(&format!("\"{}\"", self.path.trim().trim_end_matches('\\')))
            {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ui.separator();
            self.status(ui, job, data);
        });
        ui.horizontal(|ui| self.totals(ui, data));
        start
    }

    fn status(&self, ui: &mut Ui, job: &Option<ScanJob>, data: &Option<Loaded>) {
        if let Some(j) = job {
            let p = &j.progress;
            ui.spinner();
            let secs = j.started.elapsed().as_secs_f64().max(0.1);
            let items = p.files.load(Relaxed) + p.dirs.load(Relaxed);
            ui.label(format!(
                "Scanning {}: {} items ({} items/s), {}, {} folders could not be read, {:.0} s. Cancel stops it.",
                j.path.display(), count(items), count((items as f64 / secs) as u64),
                size(p.bytes.load(Relaxed)), p.denied.load(Relaxed), secs
            ));
        } else if let Some(d) = data {
            let t = &d.tree;
            let r = t.node(0);
            let done = if t.cancelled { "Scan CANCELLED (partial)" } else { "Scan complete" };
            let how = if t.method == "ntfs-mft" { "fast NTFS scan".to_string() } else if t.method_note.is_empty() { "folder scan".into() } else { format!("folder scan; {}", t.method_note.to_lowercase()) };
            let took = if t.elapsed_ms < 1000 { format!("{} ms", t.elapsed_ms) } else { format!("{:.1} s", t.elapsed_ms as f64 / 1000.0) };
            let text = format!(
                "{done} in {took} · {} files, {} items · {} denied folders · {} links not followed · {how}",
                count(r.files as u64), count(r.items as u64), t.denied, t.reparse_skipped
            );
            let denied = if t.denied_paths.is_empty() { String::new() } else {
                format!("\n\nAccess denied (first {}):\n{}", t.denied_paths.len().min(30), t.denied_paths.iter().take(30).cloned().collect::<Vec<_>>().join("\n"))
            };
            // Truncated with "…" when the window is narrow; the full text is in the tooltip.
            ui.add(egui::Label::new(text.clone()).truncate()).on_hover_text(format!("{text}{denied}"));
        } else {
            ui.label("Pick a drive or folder and press Scan. Nothing is ever deleted without your review.");
        }
    }

    fn totals(&self, ui: &mut Ui, data: &Option<Loaded>) {
        let root = data.as_ref().map(|d| d.tree.root_path.clone()).unwrap_or_else(|| PathBuf::from(&self.path));
        if let Some(d) = sys::drive_root(&root).and_then(|r| self.drives.iter().find(|d| d.root == r)) {
            let used = d.total.saturating_sub(d.free);
            ui.label(format!("{}  total {}  used {} ({:.1} %)", d.root, size(d.total), size(used), used as f64 * 100.0 / d.total.max(1) as f64));
            let free = RichText::new(format!("free {}", size(d.free)));
            ui.label(if d.free < 5 << 30 { free.color(Color32::from_rgb(255, 120, 90)).strong() } else { free });
            ui.separator();
        }
        if let Some((bytes, items)) = self.recycle {
            ui.label(format!("Recycle Bin: {} ({} items)", size(bytes), count(items)))
                .on_hover_text("Files in the Recycle Bin still use disk space until the bin is emptied.");
            ui.separator();
        }
        let m = self.mem;
        let low = m.system_free < 1 << 30;
        let txt = RichText::new(format!("RAM: diskscope {}  ·  system free {} of {}", size(m.process), size(m.system_free), size(m.system_total)));
        ui.label(if low { txt.color(Color32::from_rgb(255, 120, 90)) } else { txt })
            .on_hover_text("Scanning a single folder instead of a whole drive uses much less memory.");
    }
}
