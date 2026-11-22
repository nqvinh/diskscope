//! Applies the actions views asked for: review list changes, selection, Del / Shift+Del requests
//! (always through the confirmation dialog), and the tree update after a cleanup.

use super::{App, Tab};
use crate::cleanup::execute::{ItemResult, Outcome};
use crate::cleanup::request::delete_request;
use crate::cleanup::review::ReviewItem;
use crate::fmt::size;
use crate::gui::loaded::Loaded;
use crate::gui::review_view;
use crate::gui::action_bar::Targets;
use crate::gui::widgets::{open_in_explorer, Act};
use crate::rules::candidates::Confidence;
use eframe::egui::{self, Ui};

fn node_item(d: &Loaded, node: u32, note: String) -> Result<ReviewItem, String> {
    let n = d.tree.node(node);
    if n.is_group() {
        return Err("a group of small files cannot be listed on its own: pick its folder".into());
    }
    Ok(ReviewItem { path: d.tree.path(node), size: n.size, is_dir: n.is_dir(), items: n.items as u64, note })
}

impl App {
    /// What the big delete buttons act on: selected Review rows, else the whole Review list,
    /// else the item selected in the tree.
    pub(super) fn targets(&self) -> Targets {
        let sel = review_view::targets(&self.review, &self.review_sel, false);
        if !sel.is_empty() {
            return Targets { label: "Selected in Review".into(), items: sel };
        }
        if !self.review.is_empty() {
            return Targets { label: "Review list".into(), items: self.review.items().to_vec() };
        }
        let tree_item = self.data.as_ref().zip(self.tree.selected).and_then(|(d, n)| node_item(d, n, "selected in tree".into()).ok());
        match tree_item {
            Some(i) => Targets { label: "Selected in tree".into(), items: vec![i] },
            None => Targets { label: String::new(), items: Vec::new() },
        }
    }

    pub(super) fn scan_finished(&mut self, loaded: Loaded) {
        // Items added from the live list carry partial sizes: refresh them from the finished tree.
        self.review.update(|p| loaded.tree.find(p).map(|i| (loaded.tree.node(i).size, loaded.tree.node(i).items as u64)));
        if let Some(i) = self.pending_select.take().and_then(|p| loaded.tree.find(&p)) {
            self.tree.reveal(&loaded.tree, i);
        }
        if let Some(list) = self.pending_ext.take() {
            for want in list.split([',', ' ']).filter(|s| !s.is_empty()) {
                let want = if want.starts_with('.') { want.to_lowercase() } else { format!(".{}", want.to_lowercase()) };
                if let Some(e) = loaded.tree.exts.iter().position(|x| *x == want) {
                    self.ext_sel.push(e as u16);
                }
            }
            if !self.ext_sel.is_empty() {
                self.tab = Tab::Files;
            }
        }
        if let Some(name) = self.pending_tab.take() {
            self.tab = match name.to_lowercase().as_str() {
                "recent" => Tab::Recent,
                "least-used" => Tab::LeastUsed,
                "grew" => Tab::Grew,
                "candidates" => Tab::Candidates,
                "labels" | "by-label" => Tab::Labels,
                "tools" | "dev-tools" => Tab::Tools,
                "vm-disks" | "vms" => Tab::VmDisks,
                "files" => Tab::Files,
                "review" => Tab::Review,
                "history" => Tab::History,
                "compare" => Tab::Compare,
                "projects" => Tab::Projects,
                _ => Tab::Tree,
            };
        }
        self.data = Some(loaded);
        self.job = None;
    }

    fn add(&mut self, item: ReviewItem) -> bool {
        match self.review.add(item) {
            Ok(_) => true,
            Err(e) => {
                self.toast(e.to_string());
                false
            }
        }
    }

    pub(super) fn apply(&mut self, acts: Vec<Act>) {
        let before = (self.review.len(), self.review.total());
        let mut added_any = false;
        for a in acts {
            match a {
                Act::Toast(m) => self.toast(m),
                Act::Explorer(p) => open_in_explorer(&p),
                Act::Unreview(p) => {
                    self.review.remove(&p);
                    self.review_sel.remove(&p);
                }
                Act::Zoom(n) => self.map.zoom = n,
                Act::Select { node, reveal } => {
                    self.tree.selected = Some(node);
                    if let (true, Some(d)) = (reveal, &self.data) {
                        self.tree.reveal(&d.tree, node);
                        self.tab = Tab::Tree;
                    }
                }
                Act::Review { node, note } => {
                    let Some(d) = &self.data else { continue };
                    match node_item(d, node, note) {
                        Ok(item) => added_any |= self.add(item),
                        Err(e) => self.toast(e),
                    }
                }
                Act::AddItem(item) => added_any |= self.add(item),
                Act::AddAllSafe => {
                    let Some(d) = &self.data else { continue };
                    let safe: Vec<ReviewItem> = d
                        .candidates
                        .iter()
                        .filter(|c| c.confidence == Confidence::Safe)
                        .filter_map(|c| node_item(d, c.node, format!("{}: {}", c.kind, c.reason)).ok())
                        .collect();
                    added_any |= safe.into_iter().filter(|i| self.review.add(i.clone()).is_ok()).count() > 0;
                }
                Act::ExtSelect { exts, add } => {
                    if !add {
                        self.ext_sel.clear();
                    }
                    for e in exts {
                        if !self.ext_sel.contains(&e) {
                            self.ext_sel.push(e);
                        }
                    }
                    self.tab = Tab::Files;
                }
                Act::ToolClean { tool, estimate } => self.tools.request(tool, estimate),
                Act::NeverSuggest(p) => {
                    let s = p.display().to_string();
                    if !self.settings.never_suggest.iter().any(|x| x.eq_ignore_ascii_case(&s)) {
                        self.settings.never_suggest.push(s);
                        self.settings.save(&self.settings_path);
                    }
                    if let Some(d) = &mut self.data {
                        d.set_never_suggest(&self.settings.never_suggest);
                    }
                    self.toast(format!("{} will not be suggested again (undo in Settings)", p.display()));
                }
                Act::Exclude { pattern, mode } => {
                    if !self.settings.excludes.iter().any(|e| e.pattern.eq_ignore_ascii_case(&pattern)) {
                        self.settings.excludes.push(crate::rules::excludes::Exclude { pattern: pattern.clone(), mode });
                        self.settings.save(&self.settings_path);
                    }
                    self.toast(format!("{pattern}: {} from the next scan (undo in Settings)", mode.label().to_lowercase()));
                }
                Act::Delete { shift, targets } => match delete_request(shift, targets) {
                    Ok(req) => self.dialogs.open(req),
                    Err(e) => self.toast(e),
                },
            }
        }
        if added_any {
            let n = self.review.len().saturating_sub(before.0);
            let bytes = self.review.total().saturating_sub(before.1);
            self.toast(format!("Added {n} items ({}) - press Move to Recycle Bin or Delete permanently (bottom bar)", size(bytes)));
        }
    }

    /// Del / Shift+Del on the current selection (Tree: the selected item; Review: selected rows).
    /// Ignored while typing in a text box or while a dialog is open.
    pub(super) fn delete_keys(&mut self, ui: &Ui, data: &Loaded, acts: &mut Vec<Act>) {
        if self.dialogs.is_open() || self.tools.is_open() || ui.ctx().memory(|m| m.focused().is_some()) {
            return;
        }
        let Some(shift) = ui.input(|i| i.key_pressed(egui::Key::Delete).then_some(i.modifiers.shift)) else { return };
        let targets = match self.tab {
            Tab::Review => review_view::targets(&self.review, &self.review_sel, false),
            Tab::Tree | Tab::Files => match self.tree.selected.map(|n| node_item(data, n, "Del in tree".into())) {
                Some(Ok(item)) => vec![item],
                Some(Err(e)) => return acts.push(Act::Toast(e)),
                None => Vec::new(),
            },
            _ => Vec::new(),
        };
        if targets.is_empty() {
            acts.push(Act::Toast("select something in the Tree or rows in Review first".into()));
        } else {
            acts.push(Act::Delete { shift, targets });
        }
    }

    pub(super) fn after_cleanup(&mut self, out: Vec<Outcome>) {
        self.history.invalidate();
        let Some(d) = &mut self.data else { return };
        for o in out.iter().filter(|o| o.result == ItemResult::Done) {
            d.forget(&o.path);
            self.review.remove(&o.path);
            self.review_sel.remove(&o.path);
        }
        d.refresh();
        let freed: u64 = out.iter().filter(|o| o.result == ItemResult::Done).map(|o| o.size).sum();
        self.toast(format!("cleaned {} (skipped items stay in the review list; see History)", size(freed)));
    }
}
