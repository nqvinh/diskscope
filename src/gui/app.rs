//! Window layout (WizTree-like): header on top, tabs on the left, extensions on the right,
//! treemap at the bottom, status bar. While scanning, the centre shows live cleanup suggestions.
//! Views push `Act`s; `actions` applies them after the frame.

#[path = "app_actions.rs"]
mod actions;

use super::dialogs::Dialogs;
use super::header::Header;
use super::history_view::HistoryView;
use super::labels_view::LabelsView;
use super::lists::{candidates_ui, GrewView, TimeQuery};
use super::loaded::{Loaded, ScanJob};
use super::tree_view::TreeView;
use super::treemap_view::TreemapView;
use super::{action_bar, ext_table, live_view, review_view};
use crate::cleanup::review::ReviewList;
use crate::fmt::size;
use crate::views::queries::TimeField;
use eframe::egui::{self, CentralPanel, Panel, RichText, Ui};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Tree,
    Recent,
    LeastUsed,
    Grew,
    Candidates,
    Labels,
    Review,
    History,
}

pub struct App {
    header: Header,
    job: Option<ScanJob>,
    data: Option<Loaded>,
    tab: Tab,
    tree: TreeView,
    map: TreemapView,
    recent: TimeQuery,
    least: TimeQuery,
    grew: GrewView,
    review: ReviewList,
    review_sel: HashSet<PathBuf>,
    history: HistoryView,
    labels: LabelsView,
    dialogs: Dialogs,
    toast: Option<(String, Instant)>,
    /// Item to select in the tree once the first scan finishes (`--select`).
    pending_select: Option<PathBuf>,
    /// Tab to open once the first scan finishes (`--tab`).
    pending_tab: Option<String>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>, select: Option<PathBuf>, tab: Option<String>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let _ = crate::cleanup::history::History::default_location().prune(crate::fmt::now());
        let mut app = Self {
            header: Header::new(initial.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "C:\\".into())),
            job: None,
            data: None,
            tab: Tab::Tree,
            tree: TreeView::default(),
            map: TreemapView::default(),
            recent: TimeQuery::new(7, TimeField::Changed),
            least: TimeQuery::new(180, TimeField::Accessed),
            grew: GrewView::new(100),
            review: ReviewList::default(),
            review_sel: HashSet::new(),
            history: HistoryView::default(),
            labels: LabelsView::default(),
            dialogs: Dialogs::default(),
            toast: None,
            pending_select: select,
            pending_tab: tab,
        };
        if let Some(p) = initial {
            app.start(p);
        }
        app
    }

    fn start(&mut self, path: PathBuf) {
        if !path.exists() {
            self.toast(format!("{} does not exist", path.display()));
            return;
        }
        self.data = None; // free the previous scan before the new one fills memory
        self.tree.reset();
        self.map.reset();
        self.review.clear();
        self.review_sel.clear();
        self.job = Some(ScanJob::start(path));
    }

    fn toast(&mut self, msg: String) {
        self.toast = Some((msg, Instant::now()));
    }

    fn tabs(&mut self, ui: &mut Ui, data: &Loaded) {
        ui.horizontal(|ui| {
            for (t, label) in [
                (Tab::Tree, "Tree".to_string()),
                (Tab::Recent, "Recent".into()),
                (Tab::LeastUsed, "Least used".into()),
                (Tab::Grew, "Grew since last scan".into()),
                (Tab::Candidates, format!("Candidates ({})", data.candidates.len())),
                (Tab::Labels, "By label".into()),
                (Tab::Review, String::new()),
                (Tab::History, "History".into()),
            ] {
                let text = if t == Tab::Review && !self.review.is_empty() {
                    RichText::new(format!("Review  [{} · {}]", self.review.len(), size(self.review.total()))).strong().color(action_bar::ORANGE)
                } else if t == Tab::Review {
                    RichText::new("Review (0)").strong()
                } else {
                    RichText::new(label).strong()
                };
                ui.selectable_value(&mut self.tab, t, text);
            }
        });
        ui.separator();
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        if let Some(loaded) = self.job.as_mut().and_then(|j| j.poll()) {
            self.toast(loaded.snapshot_note.clone());
            self.scan_finished(loaded);
        }
        let mut acts = Vec::new();
        let start = Panel::top("header").show(ui, |ui| self.header.ui(ui, &self.job, &self.data)).inner;
        Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if let (Some(d), Some(s)) = (&self.data, self.tree.selected) {
                    ui.label(format!("Selected: {}  ({})  · Del = Recycle Bin, Shift+Del = permanent (both ask first)", d.tree.path(s).display(), size(d.tree.node(s).size)));
                }
                if let Some((m, t)) = &self.toast {
                    if t.elapsed() < Duration::from_secs(6) {
                        ui.separator();
                        ui.label(RichText::new(m).color(egui::Color32::from_rgb(140, 200, 255)));
                    }
                }
            });
        });
        let targets = self.targets();
        if self.data.is_some() || self.job.is_some() {
            Panel::bottom("actions").show(ui, |ui| action_bar::bar(ui, &targets, &mut acts));
        }
        if let Some(data) = self.data.take() {
            Panel::bottom("treemap").resizable(true).default_size(320.0).min_size(120.0).show(ui, |ui| {
                self.map.ui(ui, &data, self.tree.selected, &mut acts);
            });
            Panel::right("extensions").resizable(true).default_size(330.0).show(ui, |ui| ext_table::ui(ui, &data));
            CentralPanel::default().show(ui, |ui| {
                self.tabs(ui, &data);
                match self.tab {
                    Tab::Tree => self.tree.ui(ui, &data, &self.review, &mut acts),
                    Tab::Recent => self.recent.ui_recent(ui, &data, &self.review, &mut acts),
                    Tab::LeastUsed => self.least.ui_least_used(ui, &data, &self.review, &mut acts),
                    Tab::Grew => self.grew.ui(ui, &data, &self.review, &mut acts),
                    Tab::Candidates => candidates_ui(ui, &data, &self.review, &mut acts),
                    Tab::Labels => self.labels.ui(ui, &data, &self.review, &mut acts),
                    Tab::Review => review_view::ui(ui, &self.review, &mut self.review_sel, &targets, &mut acts),
                    Tab::History => self.history.ui(ui, &mut acts),
                }
            });
            self.delete_keys(ui, &data, &mut acts);
            self.data = Some(data);
        } else {
            CentralPanel::default().show(ui, |ui| match self.job.as_ref().and_then(|j| j.progress.live.get().cloned()) {
                Some(sink) => live_view::ui(ui, &sink, &self.review, &mut acts),
                None => {
                    ui.centered_and_justified(|ui| {
                        ui.label(if self.job.is_some() { "Starting…" } else { "Choose a drive or folder above and press Scan." });
                    });
                }
            });
        }
        if let Some(out) = self.dialogs.ui(ui.ctx(), self.header.recycle_bytes()) {
            self.after_cleanup(out);
        }
        self.apply(acts);
        if let Some(p) = start {
            self.start(p);
        }
        if self.job.is_some() || self.toast.as_ref().is_some_and(|t| t.1.elapsed() < Duration::from_secs(7)) {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
    }
}
