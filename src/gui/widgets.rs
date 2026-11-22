//! Small shared UI pieces and the action queue that views use to talk to the app.

use crate::model::Tree;
use eframe::egui::{self, Color32, Rect, Sense, Ui};
use std::path::PathBuf;

/// Things a view asks the app to do; applied after the frame's UI is built.
pub enum Act {
    /// Select a node; `reveal` expands its parents and scrolls the tree to it.
    Select { node: u32, reveal: bool },
    Zoom(u32),
    Review { node: u32, note: String },
    /// Add a ready item (from the live list, where no tree node exists yet).
    AddItem(crate::cleanup::review::ReviewItem),
    /// Add every "safe cache" suggestion of the finished scan.
    AddAllSafe,
    /// Del (`shift` = false) or Shift+Del on these targets: opens the confirm dialog.
    Delete { shift: bool, targets: Vec<crate::cleanup::review::ReviewItem> },
    Unreview(PathBuf),
    Explorer(PathBuf),
    Toast(String),
}

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// A "% of parent" bar with the number on top.
pub fn pct_bar(ui: &mut Ui, frac: f32) {
    let w = ui.available_width().max(40.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 14.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 2.0, Color32::from_gray(45));
    let fill = Rect::from_min_size(rect.min, egui::vec2(rect.width() * frac.clamp(0.0, 1.0), rect.height()));
    p.rect_filled(fill, 2.0, Color32::from_gray(110));
    p.text(rect.right_center() - egui::vec2(3.0, 0.0), egui::Align2::RIGHT_CENTER, format!("{:.1} %", frac * 100.0),
        egui::FontId::proportional(11.0), Color32::from_gray(235));
}

/// Small painted folder icon (the default fonts have no folder glyph).
pub fn folder_icon(ui: &mut Ui, color: Color32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 12.0), Sense::hover());
    let body = Rect::from_min_max(r.min + egui::vec2(1.0, 3.0), r.max - egui::vec2(1.0, 1.0));
    let tab = Rect::from_min_size(r.min + egui::vec2(1.0, 1.0), egui::vec2(5.0, 3.0));
    ui.painter().rect_filled(tab, 1.0, color);
    ui.painter().rect_filled(body, 1.5, color);
}

/// Painted sort triangle (down = descending).
pub fn sort_arrow(ui: &mut Ui, desc: bool) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
    let c = r.center();
    let pts = if desc {
        vec![c + egui::vec2(-4.0, -2.0), c + egui::vec2(4.0, -2.0), c + egui::vec2(0.0, 3.0)]
    } else {
        vec![c + egui::vec2(-4.0, 2.0), c + egui::vec2(4.0, 2.0), c + egui::vec2(0.0, -3.0)]
    };
    ui.painter().add(egui::Shape::convex_polygon(pts, Color32::from_gray(200), egui::Stroke::NONE));
}

/// The right-click menu shared by the tree, lists and treemap.
pub fn node_menu(ui: &mut Ui, tree: &Tree, node: u32, acts: &mut Vec<Act>) {
    if tree.node(node).is_group() {
        let folder = tree.path(tree.node(node).parent);
        if ui.button("Open its folder in Explorer").clicked() {
            acts.push(Act::Explorer(folder));
            ui.close();
        }
        return;
    }
    let path = tree.path(node);
    if ui.button("Add to review list").clicked() {
        acts.push(Act::Review { node, note: "picked by hand".into() });
        ui.close();
    }
    if tree.node(node).is_dir() && ui.button("Show in treemap").clicked() {
        acts.push(Act::Zoom(node));
        ui.close();
    }
    if ui.button("Open in Explorer").clicked() {
        acts.push(Act::Explorer(path.clone()));
        ui.close();
    }
    if ui.button("Copy path").clicked() {
        ui.ctx().copy_text(path.display().to_string());
        acts.push(Act::Toast("path copied".into()));
        ui.close();
    }
}

/// Review tick for one node: shows protected paths as a disabled box with the reason.
pub fn review_tick(ui: &mut Ui, tree: &Tree, node: u32, review: &crate::cleanup::review::ReviewList, note: &str, acts: &mut Vec<Act>) {
    if tree.node(node).is_group() {
        ui.add_enabled(false, egui::Checkbox::without_text(&mut false)).on_disabled_hover_text("a group of small files: tick its folder instead");
        return;
    }
    let path = tree.path(node);
    if let Some(why) = crate::rules::blocklist::blocked_reason(&path) {
        ui.add_enabled(false, egui::Checkbox::without_text(&mut false)).on_disabled_hover_text(format!("protected: {why}"));
        return;
    }
    let mut on = review.contains(&path);
    if ui.checkbox(&mut on, "").on_hover_text("add to / remove from the review list").changed() {
        acts.push(if on { Act::Review { node, note: note.to_string() } } else { Act::Unreview(path) });
    }
}

pub fn open_in_explorer(path: &std::path::Path) {
    let arg = format!("/select,{}", path.display());
    let _ = std::process::Command::new("explorer.exe").arg(arg).spawn();
}

/// Clickable column header: click sorts by `key`, clicking again flips; arrow on the active column.
pub fn sort_header(ui: &mut Ui, text: &str, key: crate::views::sorting::SortKey, state: &mut crate::views::sorting::SortState) {
    let active = state.key == key;
    if ui.selectable_label(active, egui::RichText::new(text).strong()).clicked() {
        state.click(key);
    }
    if active {
        sort_arrow(ui, state.desc);
    }
}

/// Coloured label chip ("Dev dependencies", "Cache", ...).
pub fn label_chip(ui: &mut Ui, label: crate::rules::labels::Label) {
    let c = rgb(label.color());
    egui::Frame::new().fill(c.gamma_multiply(0.35)).stroke(egui::Stroke::new(1.0, c)).corner_radius(4.0).inner_margin(egui::Margin::symmetric(5, 0)).show(ui, |ui| {
        ui.label(egui::RichText::new(label.name()).small().color(egui::Color32::WHITE));
    });
}

/// Date cell: day only, "N days ago" in the tooltip.
pub fn date_cell(ui: &mut Ui, unix: u32, note: &str) {
    if unix == 0 {
        return;
    }
    let tip = if note.is_empty() { crate::fmt::ago(unix) } else { format!("{} · {note}", crate::fmt::ago(unix)) };
    ui.label(crate::fmt::day(unix)).on_hover_text(tip);
}

pub const ACCESS_NOTE: &str = "Windows may not update last-access times (NTFS setting), so this can be older than real use";
