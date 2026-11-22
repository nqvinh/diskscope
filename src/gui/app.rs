//! Window layout (WizTree-like): header on top, tabs on the left, extensions on the right,
//! treemap at the bottom, status bar. While scanning, the centre shows live cleanup suggestions.
//! Views push `Act`s; `actions` applies them after the frame.

#[path = "app_actions.rs"]
mod actions;

use super::dialogs::Dialogs;
use super::header::Header;
use super::files_view::FilesView;
use super::history_view::HistoryView;
use super::labels_view::LabelsView;
use super::lists::{candidates_ui, GrewView, TimeQuery};
use super::loaded::{Loaded, ScanJob};
use super::tree_view::TreeView;
use super::tools_view::ToolsView;
use super::treemap_view::TreemapView;
use super::vm_disks_view::VmDisksView;
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
    Tools,
    VmDisks,
    Files,
    Review,
    History,
    Compare,
    Projects,
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
    compare: super::compare_view::CompareView,
    projects: super::projects_view::ProjectsView,
    labels: LabelsView,
    files: FilesView,
    tools: ToolsView,
    vms: VmDisksView,
    /// Extensions listed in the Files tab.
    ext_sel: Vec<u16>,
    /// Extensions to select once the first scan finishes (`--ext`).
    pending_ext: Option<String>,
    dialogs: Dialogs,
    toast: Option<(String, Instant)>,
    settings: super::settings::Settings,
    settings_path: Option<PathBuf>,
    settings_open: bool,
    lists: super::settings_lists::ListsUi,
    /// What the last scan covered ("Scan again to apply" in Settings).
    last_root: Option<PathBuf>,
    /// Item to select in the tree once the first scan finishes (`--select`).
    pending_select: Option<PathBuf>,
    /// Tab to open once the first scan finishes (`--tab`).
    pending_tab: Option<String>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>, select: Option<PathBuf>, tab: Option<String>, ext: Option<String>, settings_path: Option<PathBuf>) -> Self {
        let settings = super::settings::Settings::load(&settings_path);
        settings.apply(&cc.egui_ctx);
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
            compare: Default::default(),
            projects: Default::default(),
            labels: LabelsView::default(),
            files: FilesView::default(),
            tools: ToolsView::default(),
            vms: VmDisksView::default(),
            ext_sel: Vec::new(),
            pending_ext: ext,
            dialogs: Dialogs::default(),
            toast: None,
            settings,
            settings_path,
            // `--tab settings` opens the Settings window at start (screenshots without any input).
            settings_open: tab.as_deref() == Some("settings"),
            lists: Default::default(),
            last_root: None,
            pending_select: select,
            pending_tab: tab,
        };
        if let Some(p) = initial {
            app.start(p);
        }
        app
    }

    fn start(&mut self, path: PathBuf) {
        if !path.exists() && !super::loaded::is_all_drives(&path) {
            self.toast(format!("{} does not exist", path.display()));
            return;
        }
        self.data = None; // free the previous scan before the new one fills memory
        self.tree.reset();
        self.map.reset();
        self.review.clear();
        self.review_sel.clear();
        self.ext_sel.clear();
        self.last_root = Some(path.clone());
        self.job = Some(ScanJob::start(path, &self.settings.excludes, self.settings.never_suggest.clone()));
    }

    fn toast(&mut self, msg: String) {
        self.toast = Some((msg, Instant::now()));
    }

    fn tabs(&mut self, ui: &mut Ui, data: &Loaded) {
        ui.horizontal_wrapped(|ui| {
            for (t, label) in [
                (Tab::Tree, "Tree".to_string()),
                (Tab::Recent, "Recent".into()),
                (Tab::LeastUsed, "Least used".into()),
                (Tab::Grew, "Grew since last scan".into()),
                (Tab::Compare, "Compare scans".into()),
                (Tab::Candidates, format!("Candidates ({})", data.candidates.len())),
                (Tab::Labels, "By label".into()),
                (Tab::Projects, format!("Projects ({})", data.projects.len())),
                (Tab::Tools, "Dev tools".into()),
                (Tab::VmDisks, "VM disks".into()),
                (Tab::Files, if self.ext_sel.is_empty() { "Files".into() } else { format!("Files ({})", self.ext_sel.iter().map(|&e| data.tree.exts[e as usize].as_str()).collect::<Vec<_>>().join(" ")) }),
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
            if !self.settings.show_extensions && ui.small_button("Show extension list").clicked() {
                self.settings.show_extensions = true;
                self.settings.save(&self.settings_path);
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
        let mut start = Panel::top("header").show(ui, |ui| self.header.ui(ui, &self.job, &self.data, &mut self.settings_open)).inner;
        if self.settings_open {
            let (mut open, mut changed, mut lists) = (true, false, Default::default());
            let can_rescan = self.last_root.is_some() && self.job.is_none();
            egui::Window::new("Settings").open(&mut open).collapsible(false).resizable(false).show(ui.ctx(), |ui| {
                changed = self.settings.ui(ui);
                ui.separator();
                lists = self.lists.ui(ui, &mut self.settings, can_rescan);
            });
            let lists: super::settings_lists::ListsOut = lists;
            if changed {
                self.settings.apply(ui.ctx());
            }
            if changed || lists.changed {
                self.settings.save(&self.settings_path);
            }
            if let Some(d) = &mut self.data {
                d.set_never_suggest(&self.settings.never_suggest);
            }
            if lists.rescan {
                start = self.last_root.clone();
            }
            self.settings_open = open;
        }
        Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if let (Some(d), Some(s)) = (&self.data, self.tree.selected) {
                    ui.label(format!("Selected: {}  ({})  · Del = Recycle Bin, Shift+Del = permanent (both ask first)", d.tree.path(s).display(), size(d.tree.node(s).size)));
                }
                if let Some((m, t)) = &self.toast {
                    if t.elapsed() < Duration::from_secs(6) {
                        ui.separator();
                        ui.label(RichText::new(m).color(ui.visuals().hyperlink_color).strong());
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
            if self.settings.show_extensions {
                // Bigger text = narrower extension list, so the tree keeps its columns.
                let share = if self.settings.font <= 14.0 { 0.30 } else if self.settings.font <= 17.0 { 0.21 } else { 0.18 };
                let max = (ui.available_width() * share).max(180.0);
                let mut hide = false;
                Panel::right("extensions").resizable(true).min_size(180.0).default_size(max.min(330.0)).max_size(max).show(ui, |ui| {
                    hide = ui.small_button("Hide this list").on_hover_text("more room for the tree; bring it back from the tab bar").clicked();
                    egui::ScrollArea::horizontal().id_salt("ext-h").auto_shrink([false, false]).show(ui, |ui| ext_table::ui(ui, &data, &self.ext_sel, &mut acts));
                });
                if hide {
                    self.settings.show_extensions = false;
                    self.settings.save(&self.settings_path);
                }
            }
            CentralPanel::default().show(ui, |ui| {
                self.tabs(ui, &data);
                match self.tab {
                    Tab::Tree => {
                        if self.tree.ui(ui, &data, &self.review, &mut self.settings.tree_hidden, &mut acts) {
                            self.settings.save(&self.settings_path);
                        }
                    }
                    Tab::Recent => self.recent.ui_recent(ui, &data, &self.review, &mut acts),
                    Tab::LeastUsed => self.least.ui_least_used(ui, &data, &self.review, &mut acts),
                    Tab::Grew => self.grew.ui(ui, &data, &self.review, &mut acts),
                    Tab::Candidates => {
                        self.tools.start_probe();
                        candidates_ui(ui, &data, &self.review, &mut acts, &|r, ui, a| self.tools.row_button(ui, &data, r.node, a));
                    }
                    Tab::Tools => self.tools.ui(ui, &data, &mut acts),
                    Tab::VmDisks => self.vms.ui(ui, &data, &mut acts),
                    Tab::Labels => self.labels.ui(ui, &data, &self.review, &mut acts),
                    Tab::Files => self.files.ui(ui, &data, &self.ext_sel, &self.review, &mut acts),
                    Tab::Review => review_view::ui(ui, &self.review, &mut self.review_sel, &targets, &mut acts),
                    Tab::History => self.history.ui(ui, &mut acts),
                    Tab::Compare => self.compare.ui(ui, &data, &mut acts),
                    Tab::Projects => self.projects.ui(ui, &data, &self.review, &mut acts),
                }
            });
            self.delete_keys(ui, &data, &mut acts);
            self.data = Some(data);
        } else {
            CentralPanel::default().show(ui, |ui| match self.job.as_ref().and_then(|j| j.progress.live.get().cloned()) {
                Some(sink) => live_view::ui(ui, &sink, &self.review, &self.settings.never_suggest, &mut acts),
                None if self.job.is_some() => {
                    ui.centered_and_justified(|ui| ui.label("Starting the scan…"));
                }
                None => {
                    if let Some(p) = super::home::ui(ui, self.header.drives(), self.header.elevated()) {
                        start = Some(p);
                    }
                }
            });
        }
        if let Some(out) = self.dialogs.ui(ui.ctx(), self.header.recycle_bytes()) {
            self.after_cleanup(out);
        }
        if let Some(msg) = self.tools.dialog_ui(ui.ctx()) {
            self.history.invalidate();
            self.toast(msg);
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
