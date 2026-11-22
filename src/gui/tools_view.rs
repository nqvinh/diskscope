//! Dev tools tab + the state behind "Clean with pnpm": which tools are installed and where their
//! caches are (probed once, read-only, when Candidates or Dev tools is first shown), the per-row
//! button for the Candidates list, and the confirm/run dialog.

use super::loaded::Loaded;
use super::settings::{dim, warn};
use super::tool_dialog::ToolDialog;
use super::widgets::Act;
use crate::cleanup::tool_actions::{spec, tool_for_path, Tool, ToolInfo, ALL};
use crate::fmt::size;
use eframe::egui::{self, RichText, Ui};
use crate::cleanup::tool_probe::{start_all, Probes};
use std::sync::Arc;

#[derive(Default)]
pub struct ToolsView {
    probes: Option<Arc<Probes>>,
    /// Copy of the results so far (refreshed every frame).
    infos: Vec<ToolInfo>,
    done: Vec<Tool>,
    dialog: ToolDialog,
}

impl ToolsView {
    /// Starts the read-only probes once (or again after "Check again").
    pub fn start_probe(&mut self) {
        if self.probes.is_none() {
            self.probes = Some(start_all());
        }
    }

    fn running(&self) -> bool {
        self.probes.as_ref().is_some_and(|p| !p.finished())
    }

    fn poll(&mut self) {
        if let Some(p) = &self.probes {
            self.infos = p.found.lock().map(|v| v.clone()).unwrap_or_default();
            self.done = p.done.lock().map(|v| v.clone()).unwrap_or_default();
        }
    }

    pub fn is_open(&self) -> bool {
        self.dialog.is_open()
    }

    /// Opens the confirm dialog for `tool` (from a Candidates row or this tab).
    pub fn request(&mut self, tool: Tool, estimate: Option<u64>) {
        if self.dialog.is_open() {
            return; // never replace a confirm or a running job
        }
        if let Some(info) = self.infos.iter().find(|i| i.tool == tool && i.can_clean()) {
            self.dialog = ToolDialog::Confirm { info: info.clone(), estimate };
        }
    }

    /// Call every frame: finishes probes and draws the dialog; Some(summary) when a run ended.
    pub fn dialog_ui(&mut self, ctx: &egui::Context) -> Option<String> {
        self.poll();
        if self.running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
        self.dialog.ui(ctx)
    }

    /// "Clean with pnpm" in a Candidates row whose folder is that tool's cache. False = no button.
    pub fn row_button(&self, ui: &mut Ui, data: &Loaded, node: u32, acts: &mut Vec<Act>) -> bool {
        let Some(info) = tool_for_path(&self.infos, &data.tree.path(node)) else { return false };
        let tip = format!("Runs '{}' after you confirm (instead of deleting the folder)", info.command_line());
        if ui.button(RichText::new(format!("Clean with {}", spec(info.tool).name)).strong()).on_hover_text(tip).clicked() {
            acts.push(Act::ToolClean { tool: info.tool, estimate: Some(data.tree.node(node).size) });
        }
        true
    }

    /// Size of `path` in this scan, if the scan covers it.
    fn in_scan(data: &Loaded, path: &std::path::Path) -> Option<u64> {
        data.tree.find(path).map(|i| data.tree.node(i).size)
    }

    pub fn ui(&mut self, ui: &mut Ui, data: &Loaded, acts: &mut Vec<Act>) {
        self.start_probe();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Developer tools that can clean their own caches").strong());
            if self.running() {
                ui.spinner();
                ui.label(RichText::new("checking (read-only)…").color(dim()));
            } else if ui.button("Check again").clicked() {
                self.probes = None;
                self.start_probe();
            }
        });
        ui.label(RichText::new("The tool's own command is gentler than deleting the folder (pnpm keeps packages your projects still use). Nothing runs until you confirm.").color(dim()));
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("dev-tools").striped(true).num_columns(4).spacing([16.0, 8.0]).show(ui, |ui| {
                for h in ["Tool", "Cache", "In this scan", "Action"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for t in ALL {
                    if !self.done.contains(&t) {
                        ui.label(RichText::new(spec(t).name).strong());
                        ui.label(RichText::new("checking…").color(dim()));
                        ui.end_row();
                        continue;
                    }
                    Self::row(ui, data, t, self.infos.iter().find(|i| i.tool == t), acts);
                    ui.end_row();
                }
            });
        });
    }

    fn row(ui: &mut Ui, data: &Loaded, t: Tool, info: Option<&ToolInfo>, acts: &mut Vec<Act>) {
        let s = spec(t);
        ui.label(RichText::new(s.name).strong());
        let Some(info) = info else {
            ui.label(RichText::new("not installed (not on PATH)").color(dim()));
            return;
        };
        let line = |ui: &mut Ui, text: RichText| ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend));
        ui.vertical(|ui| {
            if let Some(main) = info.caches.first() {
                ui.horizontal(|ui| {
                    line(ui, RichText::new(main.display().to_string()));
                    if info.caches.len() > 1 {
                        let rest: Vec<String> = info.caches[1..].iter().map(|c| c.display().to_string()).collect();
                        line(ui, RichText::new(format!("+{} more", rest.len())).color(dim())).on_hover_text(rest.join("\n"));
                    }
                });
            }
            if let Some(r) = info.reclaimable {
                line(ui, RichText::new(format!("build cache: {} reclaimable, inside Docker's virtual disk", size(r))));
            }
            if !info.note.is_empty() {
                line(ui, RichText::new(&info.note).color(warn()));
            }
        });
        let scanned: Vec<u64> = info.caches.iter().filter_map(|c| Self::in_scan(data, c)).collect();
        if t == Tool::Cargo {
            let targets: Vec<_> = data.candidates.iter().filter(|c| c.kind == "rust-target").collect();
            ui.label(format!("{} Rust target folders, {} (see Candidates)", targets.len(), size(targets.iter().map(|c| c.size).sum())));
        } else if scanned.is_empty() {
            ui.label(RichText::new("not in this scan").color(dim()));
        } else {
            ui.label(size(scanned.iter().sum()));
        }
        if info.can_clean() {
            if ui.button(RichText::new(format!("Clean with {}", s.name)).strong()).on_hover_text(info.command_line()).clicked() {
                acts.push(Act::ToolClean { tool: t, estimate: info.caches.first().and_then(|c| Self::in_scan(data, c)) });
            }
        } else if t == Tool::Cargo {
            let tip = format!("{}\nOptional: 'cargo install cargo-cache', then 'cargo cache --autoclean'.", s.what);
            ui.label(RichText::new("hint only: no built-in cleaner (hover)").color(dim())).on_hover_text(tip);
        } else {
            ui.label("");
        }
    }
}
