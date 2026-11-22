//! The two big delete buttons, always visible in a bar at the bottom of the window (and again at
//! the top of the Review tab). They only open the confirmation dialog; nothing is deleted here.

use super::widgets::Act;
use crate::cleanup::review::ReviewItem;
use crate::fmt::size;
use eframe::egui::{self, Color32, Rect, RichText, Stroke, Ui, Vec2};

pub const ORANGE: Color32 = Color32::from_rgb(214, 120, 30);
pub const RED: Color32 = Color32::from_rgb(196, 46, 46);

/// What the buttons act on, and how to describe it.
pub struct Targets {
    pub label: String,
    pub items: Vec<ReviewItem>,
}

impl Targets {
    pub fn summary(&self) -> String {
        let total: u64 = self.items.iter().map(|i| i.size).sum();
        if self.items.is_empty() {
            "Nothing selected: tick or Add items (Tree, Candidates, live list), then delete here".into()
        } else {
            format!("{}: {}, {}", self.label, crate::fmt::items(self.items.len()), size(total))
        }
    }
}

/// Small trash-can icon painted inside a button (the default fonts have no such glyph).
fn trash_icon(ui: &Ui, r: Rect, c: Color32) {
    let p = ui.painter();
    let s = Stroke::new(2.0, c);
    let lid = Rect::from_min_size(r.min + egui::vec2(1.0, 3.0), egui::vec2(r.width() - 2.0, 2.0));
    let body = Rect::from_min_max(r.min + egui::vec2(3.0, 7.0), r.max - egui::vec2(3.0, 0.0));
    p.rect_filled(lid, 1.0, c);
    p.rect_filled(Rect::from_min_size(r.center_top() + egui::vec2(-3.0, 0.0), egui::vec2(6.0, 3.0)), 1.0, c);
    p.rect_stroke(body, 2.0, s, egui::StrokeKind::Inside);
    for k in 1..3 {
        let x = body.left() + body.width() * k as f32 / 3.0;
        p.line_segment([egui::pos2(x, body.top() + 3.0), egui::pos2(x, body.bottom() - 3.0)], Stroke::new(1.5, c));
    }
}

fn big_button(ui: &mut Ui, text: &str, fill: Color32, enabled: bool, tip: &str) -> bool {
    let label = RichText::new(format!("      {text}")).strong().size(16.0).color(Color32::WHITE);
    let btn = egui::Button::new(label).fill(if enabled { fill } else { Color32::from_gray(60) }).min_size(Vec2::new(280.0, 44.0));
    let resp = ui.add_enabled(enabled, btn);
    let icon = Rect::from_center_size(resp.rect.left_center() + egui::vec2(24.0, 0.0), egui::vec2(16.0, 18.0));
    trash_icon(ui, icon, if enabled { Color32::WHITE } else { Color32::from_gray(120) });
    let resp = if enabled { resp.on_hover_text(tip) } else { resp.on_disabled_hover_text("Add or select items first (Tree tick, Candidates, live suggestions, Review rows)") };
    resp.clicked()
}

/// The summary line plus both buttons; pushes the delete request when one is clicked.
pub fn buttons(ui: &mut Ui, t: &Targets, acts: &mut Vec<Act>) {
    ui.horizontal(|ui| {
        let on = !t.items.is_empty();
        if big_button(ui, "Move to Recycle Bin (Del)", ORANGE, on, "Opens the confirmation dialog: every path + total; you tick 'I reviewed'.") {
            acts.push(Act::Delete { shift: false, targets: t.items.clone() });
        }
        if big_button(ui, "Delete permanently (Shift+Del)", RED, on, "Opens the confirmation dialog, then a second one where you type DELETE. Cannot be undone.") {
            acts.push(Act::Delete { shift: true, targets: t.items.clone() });
        }
        ui.add_space(12.0);
        let txt = RichText::new(t.summary()).size(15.0).strong();
        ui.label(if on { txt.color(if super::settings::is_dark() { Color32::from_rgb(255, 210, 140) } else { Color32::from_rgb(140, 60, 0) }) } else { txt.weak() });
    });
}

/// The sticky bar at the bottom of the window.
pub fn bar(ui: &mut Ui, t: &Targets, acts: &mut Vec<Act>) {
    ui.add_space(4.0);
    buttons(ui, t, acts);
    ui.add_space(4.0);
}
