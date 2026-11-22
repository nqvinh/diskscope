//! Bottom treemap: squarified, coloured by extension, with folder name bands. Hover shows path and
//! size; click selects in the tree; double-click zooms into a folder; right-click opens actions.

use super::loaded::Loaded;
use super::widgets::{node_menu, rgb, Act};
use crate::fmt::size;
use crate::scan::extensions::DIR_COLOR;
use crate::treemap::{inner_rect, layout_tree, Cell, LayoutOptions, TmRect};
use eframe::egui::{self, epaint::Mesh, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui};
use std::sync::Arc;

const HEADER: f32 = 14.0;

#[derive(Default)]
pub struct TreemapView {
    pub zoom: u32,
    cells: Vec<Cell>,
    mesh: Option<Arc<Mesh>>,
    key: Option<(u64, u32, [i32; 4])>,
}

fn shade(c: Color32, k: f32) -> Color32 {
    let f = |v: u8| ((v as f32) * k).clamp(0.0, 255.0) as u8;
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

fn to_rect(r: &TmRect) -> Rect {
    Rect::from_min_size(Pos2::new(r.x, r.y), egui::vec2(r.w, r.h))
}

impl TreemapView {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    fn relayout(&mut self, data: &Loaded, rect: Rect) {
        let tree = &data.tree;
        let r = TmRect { x: rect.min.x, y: rect.min.y, w: rect.width(), h: rect.height() };
        let opt = LayoutOptions { min_area: 12.0, header: HEADER, max_cells: 120_000 };
        self.cells = layout_tree(tree, self.zoom, r, &opt);
        let mut mesh = Mesh::default();
        for c in &self.cells {
            let n = tree.node(c.node);
            let base = if n.is_dir() { rgb(DIR_COLOR) } else { rgb(data.palette[n.ext as usize]) };
            let rc = to_rect(&c.rect).shrink(if c.rect.w > 3.0 && c.rect.h > 3.0 { 0.5 } else { 0.0 });
            // Cushion-like light: brighter top-left, darker bottom-right.
            let (hi, lo) = if n.is_dir() { (shade(base, 1.15), shade(base, 0.8)) } else { (shade(base, 1.25), shade(base, 0.6)) };
            let k = mesh.vertices.len() as u32;
            mesh.colored_vertex(rc.left_top(), hi);
            mesh.colored_vertex(rc.right_top(), base);
            mesh.colored_vertex(rc.right_bottom(), lo);
            mesh.colored_vertex(rc.left_bottom(), base);
            mesh.add_triangle(k, k + 1, k + 2);
            mesh.add_triangle(k, k + 2, k + 3);
        }
        self.mesh = Some(Arc::new(mesh));
    }

    /// Deepest cell under the pointer.
    fn hit(&self, p: Pos2) -> Option<&Cell> {
        self.cells.iter().rev().find(|c| c.rect.contains(p.x, p.y))
    }

    pub fn ui(&mut self, ui: &mut Ui, data: &Loaded, selected: Option<u32>, acts: &mut Vec<Act>) {
        let tree = &data.tree;
        if self.zoom as usize >= tree.len() {
            self.zoom = 0;
        }
        ui.horizontal(|ui| {
            if ui.add_enabled(self.zoom != 0, egui::Button::new("Up one level")).clicked() {
                self.zoom = tree.node(self.zoom).parent;
            }
            if ui.add_enabled(self.zoom != 0, egui::Button::new("Whole scan")).clicked() {
                self.zoom = 0;
            }
            ui.label(format!("{}  ({})", tree.path(self.zoom).display(), size(tree.node(self.zoom).size)));
            ui.weak("· click selects · double-click zooms into a folder · right-click for actions");
        });
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click());
        let key = (data.generation, self.zoom, [rect.min.x as i32, rect.min.y as i32, rect.width() as i32, rect.height() as i32]);
        if self.key != Some(key) {
            self.relayout(data, rect);
            self.key = Some(key);
        }
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_gray(20));
        if let Some(m) = &self.mesh {
            painter.add(Shape::mesh(m.clone()));
        }
        self.labels(&painter, data);
        if let Some(sel) = selected.and_then(|s| self.cells.iter().find(|c| c.node == s)) {
            painter.rect_stroke(to_rect(&sel.rect), 0.0, Stroke::new(2.0, Color32::WHITE), StrokeKind::Inside);
        }
        let hovered = resp.hover_pos().and_then(|p| self.hit(p)).copied();
        if let Some(h) = hovered {
            painter.rect_stroke(to_rect(&h.rect), 0.0, Stroke::new(1.5, Color32::from_rgb(255, 230, 120)), StrokeKind::Inside);
            let path = tree.path(h.node);
            resp.clone().on_hover_ui_at_pointer(|ui| {
                ui.label(egui::RichText::new(path.display().to_string()).strong());
                ui.label(format!("{}  ·  {}", size(tree.node(h.node).size), crate::scan::extensions::type_name(tree.ext(h.node))));
            });
            if resp.clicked() {
                acts.push(Act::Select { node: h.node, reveal: true });
            }
            if resp.double_clicked() {
                let n = tree.node(h.node);
                self.zoom = if n.is_dir() { h.node } else { n.parent };
            }
        }
        if let Some(h) = hovered {
            resp.context_menu(|ui| node_menu(ui, tree, h.node, acts));
        }
    }

    /// Folder name bands ("name (size)") where there is room, capped to keep frames cheap.
    fn labels(&self, painter: &egui::Painter, data: &Loaded) {
        let tree = &data.tree;
        let font = FontId::proportional(11.0);
        let mut drawn = 0;
        for c in &self.cells {
            let inner = inner_rect(c.rect, HEADER);
            if !tree.node(c.node).is_dir() || inner.y - c.rect.y < HEADER || drawn > 400 {
                continue;
            }
            let band = Rect::from_min_size(Pos2::new(c.rect.x + 2.0, c.rect.y), egui::vec2(c.rect.w - 4.0, HEADER));
            let name = if c.node == self.zoom { tree.path(c.node).display().to_string() } else { tree.name(c.node).to_string() };
            painter.with_clip_rect(band).text(band.left_center(), egui::Align2::LEFT_CENTER,
                format!("{name}\\ ({})", size(tree.node(c.node).size)), font.clone(), Color32::from_gray(230));
            drawn += 1;
        }
    }
}
