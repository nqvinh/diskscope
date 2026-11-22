//! "Projects" tab: project roots found in the scan (see rules::projects) with total size, what
//! could be rebuilt (build outputs, dependencies, caches), last change and last git commit.
//! "Clean reclaimable…" adds those folders to Review and opens the usual confirm dialog.

use super::loaded::Loaded;
use super::widgets::{date_cell, node_menu, sort_header, Act};
use crate::cleanup::review::{ReviewItem, ReviewList};
use crate::fmt::size;
use crate::rules::git_commit_date::last_commit;
use crate::views::sorting::{sort_list, SortKey, SortState};
use eframe::egui::{self, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use std::collections::HashMap;

// Column ids reuse SortKey: Name = project, Label = kind, Size = size, Alloc = reclaimable,
// Modified = last change, Created = last commit.
pub struct ProjectsView {
    sort: SortState,
    filter: String,
    commits: HashMap<u32, Option<u32>>,
    commits_for: Option<u64>,
}

impl Default for ProjectsView {
    fn default() -> Self {
        Self { sort: SortState { key: SortKey::Alloc, desc: true }, filter: String::new(), commits: HashMap::new(), commits_for: None }
    }
}

fn items(data: &Loaded, nodes: &[u32], project: &str) -> Vec<ReviewItem> {
    nodes
        .iter()
        .map(|&n| {
            let node = data.tree.node(n);
            ReviewItem { path: data.tree.path(n), size: node.size, is_dir: true, items: node.items as u64, note: format!("rebuildable part of project {project}") }
        })
        .collect()
}

impl ProjectsView {
    pub fn ui(&mut self, ui: &mut Ui, data: &Loaded, review: &ReviewList, acts: &mut Vec<Act>) {
        if self.commits_for != Some(data.generation) {
            // Read once per scan: only the tail of each repository's .git/logs/HEAD.
            self.commits = data.projects.iter().filter(|p| p.kinds.contains(&"git")).map(|p| (p.node, last_commit(&data.tree.path(p.node)))).collect();
            self.commits_for = Some(data.generation);
        }
        if data.projects.is_empty() {
            ui.add_space(20.0);
            ui.label(RichText::new("No projects found in this scan.").strong());
            ui.label("A project is a folder with .git, package.json, Cargo.toml, a .sln/.csproj, pyproject.toml, Gradle files, or a Unity project.");
            return;
        }
        let total: u64 = data.projects.iter().map(|p| p.reclaimable).sum();
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} projects, {} could be rebuilt (build outputs, dependencies, caches)", data.projects.len(), size(total))).strong());
            ui.add(egui::TextEdit::singleline(&mut self.filter).desired_width(200.0).hint_text("filter by name or path"));
        });
        let needle = self.filter.trim().to_lowercase();
        let mut rows: Vec<&crate::rules::projects::Project> = data.projects.iter().filter(|p| needle.is_empty() || data.tree.path(p.node).to_string_lossy().to_lowercase().contains(&needle)).collect();
        let commit = |p: &&crate::rules::projects::Project| self.commits.get(&p.node).copied().flatten().unwrap_or(0);
        let name = |p: &&crate::rules::projects::Project| data.tree.name(p.node).to_lowercase();
        match self.sort.key {
            SortKey::Name => sort_list(&mut rows, self.sort.desc, name, |p| p.size),
            SortKey::Label => sort_list(&mut rows, self.sort.desc, |p| p.kinds.join(","), name),
            SortKey::Size => sort_list(&mut rows, self.sort.desc, |p| p.size, name),
            SortKey::Modified => sort_list(&mut rows, self.sort.desc, |p| p.modified, name),
            SortKey::Created => sort_list(&mut rows, self.sort.desc, commit, name),
            _ => sort_list(&mut rows, self.sort.desc, |p| p.reclaimable, name),
        }
        let mut sort = self.sort;
        TableBuilder::new(ui)
            .id_salt("projects")
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::initial(150.0))
            .column(Column::initial(220.0).clip(true))
            .column(Column::initial(110.0).clip(true))
            .column(Column::initial(80.0))
            .column(Column::initial(95.0))
            .column(Column::initial(95.0))
            .column(Column::initial(95.0))
            .column(Column::remainder().clip(true))
            .header(20.0, |mut h| {
                h.col(|_| {});
                h.col(|ui| sort_header(ui, "Project", SortKey::Name, &mut sort));
                h.col(|ui| sort_header(ui, "Kind", SortKey::Label, &mut sort));
                h.col(|ui| sort_header(ui, "Size", SortKey::Size, &mut sort));
                h.col(|ui| sort_header(ui, "Reclaimable", SortKey::Alloc, &mut sort));
                h.col(|ui| sort_header(ui, "Last change", SortKey::Modified, &mut sort));
                h.col(|ui| sort_header(ui, "Last commit", SortKey::Created, &mut sort));
                h.col(|ui| { ui.label(RichText::new("Folder").strong()); });
            })
            .body(|body| {
                body.rows(super::settings::row_h(), rows.len(), |mut row| {
                    let p = rows[row.index()];
                    let pname = data.tree.name(p.node).to_string();
                    let path = data.tree.path(p.node);
                    row.col(|ui| {
                        let todo: Vec<u32> = p.reclaim.iter().copied().filter(|&n| !review.contains(&data.tree.path(n))).collect();
                        let tip = format!("Adds {} folder(s) ({}) to Review and asks before moving them to the Recycle Bin", p.reclaim.len(), size(p.reclaimable));
                        if ui.add_enabled(!todo.is_empty(), egui::Button::new("Clean reclaimable…").small()).on_hover_text(tip).on_disabled_hover_text("nothing to rebuild, or already in Review").clicked() {
                            let targets = items(data, &p.reclaim, &pname);
                            acts.extend(todo.iter().map(|&n| Act::Review { node: n, note: format!("rebuildable part of project {pname}") }));
                            acts.push(Act::Delete { shift: false, targets });
                        }
                    });
                    row.col(|ui| { ui.label(RichText::new(&pname).strong()).on_hover_text(path.display().to_string()); });
                    row.col(|ui| { ui.label(p.kinds.join(", ")); });
                    row.col(|ui| { ui.label(size(p.size)); });
                    row.col(|ui| { ui.label(if p.reclaimable == 0 { "-".into() } else { size(p.reclaimable) }); });
                    row.col(|ui| date_cell(ui, p.modified, "newest change inside"));
                    row.col(|ui| match self.commits.get(&p.node).copied().flatten() {
                        Some(t) => date_cell(ui, t, "from .git/logs/HEAD"),
                        None => { ui.weak("-"); }
                    });
                    row.col(|ui| { ui.label(path.display().to_string()); });
                    let resp = row.response();
                    if resp.clicked() {
                        acts.push(Act::Select { node: p.node, reveal: false });
                    }
                    resp.context_menu(|ui| node_menu(ui, &data.tree, p.node, acts));
                });
            });
        self.sort = sort;
    }
}
