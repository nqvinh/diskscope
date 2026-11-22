//! "By label" tab: total size per automatic label (biggest first); click a label to list its
//! biggest items. Labels only describe content; nothing is selected for deletion.

use super::list_table::{self, ListRow};
use super::loaded::Loaded;
use super::widgets::{label_chip, pct_bar, Act};
use crate::cleanup::review::ReviewList;
use crate::fmt::{count, size};
use crate::model::Tree;
use crate::rules::labels::{Label, ALL};
use crate::scan::aggregate::for_each_below;
use eframe::egui::{self, RichText, Ui};

/// (label, bytes, files) over all files, biggest first.
pub fn label_totals(tree: &Tree) -> Vec<(Label, u64, u64)> {
    let mut acc = [(0u64, 0u64); ALL.len()];
    for_each_below(tree, 0, |i| {
        let n = tree.node(i);
        if !n.is_dir() {
            let a = &mut acc[n.label as usize % ALL.len()];
            a.0 += n.size;
            a.1 += n.files as u64;
        }
    });
    let mut v: Vec<(Label, u64, u64)> = ALL.iter().map(|&l| (l, acc[l as usize].0, acc[l as usize].1)).filter(|x| x.2 > 0).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

/// Top-most items carrying `label` (a folder whose parent has another label, or a single file).
fn label_roots(tree: &Tree, label: Label, limit: usize) -> Vec<u32> {
    let mut out = Vec::new();
    let mut stack = vec![0u32];
    while let Some(i) = stack.pop() {
        let n = tree.node(i);
        if n.label == label as u8 && (i == 0 || tree.node(n.parent).label != label as u8) {
            out.push(i);
            if label.is_strong() || !n.is_dir() {
                continue;
            }
        }
        stack.extend(tree.children(i));
    }
    out.sort_by(|a, b| tree.node(*b).size.cmp(&tree.node(*a).size));
    out.truncate(limit);
    out
}

#[derive(Default)]
pub struct LabelsView {
    selected: Option<Label>,
    key: Option<(u64, Label)>,
    rows: Vec<ListRow>,
}

impl LabelsView {
    pub fn ui(&mut self, ui: &mut Ui, data: &Loaded, review: &ReviewList, acts: &mut Vec<Act>) {
        let totals = &data.label_totals;
        let all: u64 = totals.iter().map(|t| t.1).sum::<u64>().max(1);
        let sel = *self.selected.get_or_insert_with(|| totals.first().map(|t| t.0).unwrap_or_default());
        ui.horizontal_wrapped(|ui| {
            for &(l, bytes, files) in totals {
                let text = format!("{}  {}  ({:.1} %, {} files)", l.name(), size(bytes), bytes as f64 * 100.0 / all as f64, count(files));
                if ui.selectable_label(sel == l, RichText::new(text).strong()).on_hover_text("show the biggest items with this label").clicked() {
                    self.selected = Some(l);
                }
            }
        });
        ui.separator();
        egui::Grid::new("label-bars").num_columns(3).show(ui, |ui| {
            for &(l, bytes, _) in totals.iter().take(8) {
                label_chip(ui, l);
                ui.allocate_ui(egui::vec2(260.0, 16.0), |ui| pct_bar(ui, bytes as f32 / all as f32));
                ui.label(size(bytes));
                ui.end_row();
            }
        });
        ui.separator();
        if self.key != Some((data.generation, sel)) {
            self.key = Some((data.generation, sel));
            let tree = &data.tree;
            self.rows = label_roots(tree, sel, 2000)
                .into_iter()
                .map(|i| {
                    let n = tree.node(i);
                    ListRow { node: i, bytes: n.size, amount: size(n.size), when: n.modified, extra: if n.is_dir() { format!("{} files", count(n.files as u64)) } else { String::new() }, note: String::new() }
                })
                .collect();
        }
        ui.label(RichText::new(format!("Biggest '{}' items (labels only describe content; nothing is selected for you)", sel.name())).strong());
        list_table::ui(ui, "by-label", data, &self.rows, ["Size", "Files", ""], review, acts);
    }
}
