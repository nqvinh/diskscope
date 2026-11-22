//! Confirmation dialogs and the delete job. Flow:
//! Review button or Del / Shift+Del -> dialog 1 (every path + total, "I reviewed" tick) ->
//!   Recycle Bin: run.   Permanent: dialog 2 (type DELETE) -> run.
//! Only `confirm_recycle` / `confirm_permanent` can produce the plan `execute` needs.

use crate::cleanup::action_log::ActionLog;
use crate::cleanup::confirm::{confirm_permanent, confirm_recycle, ConfirmedPlan, Mode, PERMANENT_WORD};
use crate::cleanup::history::{result_of, History, HistoryEntry};
use crate::cleanup::request::DeleteRequest;
use crate::cleanup::execute::{execute, ItemResult, Outcome};
use crate::cleanup::review::ReviewList;
use crate::fmt::size;
use eframe::egui::{self, Color32, RichText, Ui};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::Arc;
use std::thread::JoinHandle;

pub enum Stage {
    Closed,
    First { permanent: bool, reviewed: bool },
    Second { typed: String },
    Running { done: Arc<AtomicUsize>, total: usize, handle: Option<JoinHandle<Vec<Outcome>>> },
    Results(Vec<Outcome>),
}

pub struct Dialogs {
    pub stage: Stage,
    error: Option<String>,
    /// Exactly what this dialog would delete (the selection or the whole review list).
    target: ReviewList,
}

impl Default for Dialogs {
    fn default() -> Self {
        Self { stage: Stage::Closed, error: None, target: ReviewList::default() }
    }
}

const WARN: Color32 = Color32::from_rgb(255, 150, 90);

fn path_list(ui: &mut Ui, review: &ReviewList) {
    egui::ScrollArea::vertical().max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
        for it in review.items() {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{:>10}", size(it.size))).monospace());
                ui.label(it.path.display().to_string());
            });
        }
    });
}

impl Dialogs {
    pub fn open(&mut self, req: DeleteRequest) {
        self.stage = Stage::First { permanent: req.mode == Mode::Permanent, reviewed: false };
        self.target = req.list;
        self.error = None;
    }

    pub fn is_open(&self) -> bool {
        !matches!(self.stage, Stage::Closed)
    }

    fn run(&mut self, plan: ConfirmedPlan) {
        let done = Arc::new(AtomicUsize::new(0));
        let total = plan.items().len();
        let d = done.clone();
        let mode = if plan.mode() == Mode::Permanent { "permanent" } else { "recycle" };
        let items: Vec<u64> = plan.items().iter().map(|i| i.items).collect();
        let handle = std::thread::spawn(move || {
            let out = execute(plan, &ActionLog::default_location(), |k, _| d.store(k + 1, Relaxed));
            let now = crate::fmt::now();
            let entries: Vec<HistoryEntry> = out.iter().zip(items).map(|(o, items)| {
                let why = match &o.result { ItemResult::Skipped(w) => Some(w.as_str()), ItemResult::Done => None };
                HistoryEntry { at_unix: now, at: crate::fmt::date(now), path: o.path.display().to_string(), size: o.size, items,
                    mode: mode.into(), result: result_of(why).into(), error: why.unwrap_or("").into() }
            }).collect();
            if let Err(e) = History::default_location().append(&entries, now) {
                eprintln!("diskscope: history not written: {e}");
            }
            out
        });
        self.stage = Stage::Running { done, total, handle: Some(handle) };
    }

    /// Shows the active dialog. Returns the outcomes once a delete job has finished.
    pub fn ui(&mut self, ctx: &egui::Context, recycle_bytes: Option<u64>) -> Option<Vec<Outcome>> {
        let review = &self.target;
        let mut finished = None;
        let mut next: Option<Stage> = None;
        let mut plan: Option<ConfirmedPlan> = None;
        let (n, total) = (crate::fmt::items(review.len()), size(review.total()));
        match &mut self.stage {
            Stage::Closed => return None,
            Stage::First { permanent, reviewed } => {
                let permanent = *permanent;
                egui::Modal::new(egui::Id::unique("confirm-1")).show(ctx, |ui| {
                    ui.set_width(720.0);
                    let title = if permanent { format!("Permanently delete {n} ({total})?") } else { format!("Move {n} ({total}) to the Recycle Bin?") };
                    ui.heading(title);
                    path_list(ui, review);
                    if permanent {
                        ui.colored_label(WARN, "Permanent delete cannot be undone. A second confirmation follows.");
                    } else {
                        ui.colored_label(WARN, format!(
                            "Moving to the Recycle Bin does NOT free disk space until the bin is emptied (it holds {} now). \
                             Items too big for the bin are skipped, never deleted permanently.",
                            recycle_bytes.map(size).unwrap_or_else(|| "an unknown amount".into())));
                    }
                    ui.label("Files that are in use are skipped and listed afterwards. Every action is logged.");
                    ui.checkbox(reviewed, "I reviewed every path in this list");
                    if let Some(e) = &self.error { ui.colored_label(WARN, e); }
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() { next = Some(Stage::Closed); }
                        let label = if permanent { "Continue to permanent delete…" } else { "Move to Recycle Bin" };
                        if ui.add_enabled(*reviewed, egui::Button::new(RichText::new(label).strong())).clicked() {
                            if permanent {
                                next = Some(Stage::Second { typed: String::new() });
                            } else {
                                match confirm_recycle(review, *reviewed) { Ok(p) => plan = Some(p), Err(e) => self.error = Some(e.to_string()) }
                            }
                        }
                    });
                });
            }
            Stage::Second { typed } => {
                egui::Modal::new(egui::Id::unique("confirm-2")).show(ctx, |ui| {
                    ui.set_width(520.0);
                    ui.heading(RichText::new(format!("Second confirmation: delete {n} ({total}) for good")).color(WARN));
                    ui.label(format!("This skips the Recycle Bin and cannot be undone. Type {PERMANENT_WORD} to continue."));
                    ui.add(egui::TextEdit::singleline(typed).hint_text(PERMANENT_WORD));
                    if let Some(e) = &self.error { ui.colored_label(WARN, e); }
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() { next = Some(Stage::Closed); }
                        let ok = typed.trim() == PERMANENT_WORD;
                        if ui.add_enabled(ok, egui::Button::new(RichText::new("Delete permanently").color(WARN).strong())).clicked() {
                            match confirm_permanent(review, true, true, typed) { Ok(p) => plan = Some(p), Err(e) => self.error = Some(e.to_string()) }
                        }
                    });
                });
            }
            Stage::Running { done, total, handle } => {
                let (k, t) = (done.load(Relaxed), *total);
                egui::Modal::new(egui::Id::unique("running")).show(ctx, |ui| {
                    ui.heading("Cleaning up…");
                    ui.add(egui::ProgressBar::new(k as f32 / t.max(1) as f32).text(format!("{k} / {t}")));
                });
                if handle.as_ref().is_some_and(|h| h.is_finished()) {
                    let out = handle.take().and_then(|h| h.join().ok()).unwrap_or_default();
                    finished = Some(out.clone());
                    next = Some(Stage::Results(out));
                }
                ctx.request_repaint();
            }
            Stage::Results(out) => {
                egui::Modal::new(egui::Id::unique("results")).show(ctx, |ui| {
                    ui.set_width(720.0);
                    let ok = out.iter().filter(|o| o.result == ItemResult::Done).count();
                    ui.heading(format!("Done: {ok} of {} items", out.len()));
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for o in out.iter() {
                            if let ItemResult::Skipped(why) = &o.result {
                                ui.colored_label(WARN, format!("skipped {}: {why}", o.path.display()));
                            }
                        }
                    });
                    ui.label(format!("Log: {}", ActionLog::default_location().dir().join("actions.csv").display()));
                    if ui.button("Close").clicked() { next = Some(Stage::Closed); }
                });
            }
        }
        if let Some(p) = plan {
            self.run(p);
        } else if let Some(s) = next {
            self.error = None;
            self.stage = s;
        }
        finished
    }
}
