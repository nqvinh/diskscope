//! Confirm + run window for a tool cleanup ("Clean with pnpm"). Flow: confirm (exact command,
//! what it does, cache folder, estimate, time limit) -> running (live output, Cancel) -> result
//! (exit status, free space before/after). Nothing runs until the user presses the Run button.

use super::settings::{dim, warn};
use crate::cleanup::tool_actions::{spec, ToolInfo, CLEAN_TIMEOUT_SECS};
use crate::cleanup::tool_runner::{result_word, End, RunResult, ToolJob};
use crate::fmt::size;
use eframe::egui::{self, RichText, Ui};

#[derive(Default)]
pub enum ToolDialog {
    #[default]
    Closed,
    Confirm { info: ToolInfo, estimate: Option<u64> },
    Running(ToolJob),
    Done { command: String, lines: Vec<String>, result: RunResult },
}

fn output(ui: &mut Ui, lines: &[String]) {
    egui::Frame::new().fill(ui.visuals().extreme_bg_color).inner_margin(6).show(ui, |ui| {
        egui::ScrollArea::vertical().max_height(260.0).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
            for l in lines {
                ui.label(RichText::new(l).monospace().small());
            }
        });
    });
}

fn freed_text(r: &RunResult) -> String {
    match (r.free_before, r.free_after, r.freed()) {
        (Some(b), Some(a), Some(d)) => {
            let change = if d >= 0 { format!("+{}", size(d as u64)) } else { format!("-{}", size(d.unsigned_abs())) };
            format!("Free space on {}: {} before, {} after ({change})", r.drive, size(b), size(a))
        }
        _ => format!("Free space on {} could not be measured", r.drive),
    }
}

impl ToolDialog {
    pub fn is_open(&self) -> bool {
        !matches!(self, ToolDialog::Closed)
    }

    /// Draws the window; returns a one-line summary when a run has just ended.
    pub fn ui(&mut self, ctx: &egui::Context) -> Option<String> {
        let mut next = None;
        let mut summary = None;
        match self {
            ToolDialog::Closed => return None,
            ToolDialog::Confirm { info, estimate } => {
                let s = spec(info.tool);
                let cmd = info.command_line();
                egui::Modal::new(egui::Id::unique("tool-confirm")).show(ctx, |ui| {
                    ui.set_width(640.0);
                    ui.heading(format!("Clean with {}?", s.name));
                    ui.horizontal(|ui| {
                        ui.label("Command:");
                        ui.label(RichText::new(&cmd).monospace().strong());
                        if ui.small_button("Copy").clicked() {
                            ui.ctx().copy_text(cmd.clone());
                        }
                    });
                    ui.label(s.what);
                    if let Some(c) = info.caches.first() {
                        ui.label(format!("Cache folder: {}", c.display()));
                    }
                    let est = match (estimate, info.reclaimable) {
                        (Some(b), _) => format!("Estimated: up to {} (the cache's size in this scan; the tool only removes what it no longer needs, so it may free less)", size(*b)),
                        (None, Some(b)) => format!("Estimated: {} ({} reports this as reclaimable)", size(b), s.name),
                        _ => "Estimated: unknown (the cache is not part of this scan)".into(),
                    };
                    ui.label(est);
                    ui.label(RichText::new(format!("Runs {} in the background with its output shown here. Time limit {} min; you can cancel. The run is written to History.",
                        info.program.display(), CLEAN_TIMEOUT_SECS / 60)).color(dim()));
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            next = Some(ToolDialog::Closed);
                        }
                        if ui.button(RichText::new("Run this command").strong()).clicked() {
                            next = Some(ToolDialog::Running(ToolJob::start(info.clone())));
                        }
                    });
                });
            }
            ToolDialog::Running(job) => {
                let lines = job.lines.lock().map(|v| v.clone()).unwrap_or_default();
                egui::Modal::new(egui::Id::unique("tool-running")).show(ctx, |ui| {
                    ui.set_width(720.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.heading(format!("Running {} · {:.0} s", job.command_line, job.started.elapsed().as_secs_f64()));
                    });
                    output(ui, &lines);
                    if ui.button("Cancel").on_hover_text("Stops the tool and everything it started").clicked() {
                        job.cancel();
                    }
                });
                if let Some(result) = job.poll() {
                    let (word, detail) = result_word(&result.end);
                    summary = Some(format!("{}: {word}{} · {}", job.command_line, if detail.is_empty() { String::new() } else { format!(" ({detail})") }, freed_text(&result)));
                    let lines = job.lines.lock().map(|v| v.clone()).unwrap_or_default();
                    next = Some(ToolDialog::Done { command: job.command_line.clone(), lines, result });
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            ToolDialog::Done { command, lines, result } => {
                egui::Modal::new(egui::Id::unique("tool-done")).show(ctx, |ui| {
                    ui.set_width(720.0);
                    let (title, color) = match &result.end {
                        End::Ok => (format!("Done: {command}"), ui.visuals().text_color()),
                        End::Cancelled => (format!("Cancelled: {command}"), warn()),
                        End::TimedOut => (format!("Stopped after {} min: {command}", CLEAN_TIMEOUT_SECS / 60), warn()),
                        End::Failed(e) => (format!("Failed ({e}): {command}"), warn()),
                    };
                    ui.heading(RichText::new(title).color(color));
                    output(ui, lines);
                    ui.label(RichText::new(freed_text(result)).strong());
                    ui.label(RichText::new("Other programs writing at the same time also change free space. Scan again to update the tree.").color(dim()));
                    if ui.button("Close").clicked() {
                        next = Some(ToolDialog::Closed);
                    }
                });
            }
        }
        if let Some(n) = next {
            *self = n;
        }
        summary
    }
}
