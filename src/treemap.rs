//! Squarified treemap (Bruls, Huizing, van Wijk 2000) and a breadth-first layout of the scanned
//! tree. Breadth-first with a cell cap keeps big trees balanced and cheap to draw.

use crate::model::Tree;
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TmRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl TmRect {
    pub fn area(&self) -> f32 {
        self.w * self.h
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

/// Lays out `values` (positive, sorted biggest first) inside `r`; areas are proportional to values.
pub fn squarify(values: &[f64], r: TmRect) -> Vec<TmRect> {
    let n = values.len();
    let mut out = vec![TmRect::default(); n];
    let total: f64 = values.iter().sum();
    if n == 0 || total <= 0.0 || r.w <= 0.0 || r.h <= 0.0 {
        return out;
    }
    let scale = (r.w as f64 * r.h as f64) / total;
    let areas: Vec<f64> = values.iter().map(|v| v * scale).collect();
    let (mut x, mut y, mut w, mut h) = (r.x as f64, r.y as f64, r.w as f64, r.h as f64);
    let mut i = 0;
    while i < n {
        let side = w.min(h);
        let mut j = i + 1;
        let mut best = worst(&areas[i..j], side);
        while j < n {
            let next = worst(&areas[i..j + 1], side);
            if next > best {
                break;
            }
            best = next;
            j += 1;
        }
        let row: f64 = areas[i..j].iter().sum();
        if w >= h {
            let cw = if h > 0.0 { row / h } else { 0.0 };
            let mut cy = y;
            for k in i..j {
                let ch = if cw > 0.0 { areas[k] / cw } else { 0.0 };
                out[k] = TmRect { x: x as f32, y: cy as f32, w: cw as f32, h: ch as f32 };
                cy += ch;
            }
            x += cw;
            w -= cw;
        } else {
            let rh = if w > 0.0 { row / w } else { 0.0 };
            let mut cx = x;
            for k in i..j {
                let cw = if rh > 0.0 { areas[k] / rh } else { 0.0 };
                out[k] = TmRect { x: cx as f32, y: y as f32, w: cw as f32, h: rh as f32 };
                cx += cw;
            }
            y += rh;
            h -= rh;
        }
        i = j;
    }
    out
}

/// Worst aspect ratio of a row laid along a side of length `side`.
fn worst(row: &[f64], side: f64) -> f64 {
    let s: f64 = row.iter().sum();
    let (mut lo, mut hi) = (f64::MAX, 0.0f64);
    for &a in row {
        lo = lo.min(a);
        hi = hi.max(a);
    }
    if lo <= 0.0 || s <= 0.0 || side <= 0.0 {
        return f64::MAX;
    }
    let (s2, w2) = (s * s, side * side);
    (w2 * hi / s2).max(s2 / (w2 * lo))
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub node: u32,
    pub rect: TmRect,
    pub depth: u16,
}

pub struct LayoutOptions {
    /// Cells smaller than this (px²) are not subdivided further.
    pub min_area: f32,
    /// Folder name band height; only drawn when the folder is tall enough.
    pub header: f32,
    pub max_cells: usize,
}

/// Lays out `root`'s subtree into `rect`. Folders are returned before their contents.
pub fn layout_tree(tree: &Tree, root: u32, rect: TmRect, opt: &LayoutOptions) -> Vec<Cell> {
    let mut cells = vec![Cell { node: root, rect, depth: 0 }];
    let mut queue = VecDeque::from([(root, rect, 0u16)]);
    while let Some((dir, r, depth)) = queue.pop_front() {
        if cells.len() >= opt.max_cells {
            break;
        }
        let inner = inner_rect(r, opt.header);
        if inner.area() < opt.min_area {
            continue;
        }
        let mut kids: Vec<(u64, u32)> =
            tree.children(dir).map(|c| (tree.node(c).size, c)).filter(|(s, _)| *s > 0).collect();
        kids.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        let values: Vec<f64> = kids.iter().map(|k| k.0 as f64).collect();
        for (rc, &(_, c)) in squarify(&values, inner).into_iter().zip(&kids) {
            if rc.area() < 0.5 {
                continue;
            }
            cells.push(Cell { node: c, rect: rc, depth: depth + 1 });
            if tree.node(c).is_dir() && rc.area() >= opt.min_area {
                queue.push_back((c, rc, depth + 1));
            }
        }
    }
    cells
}

/// Space inside a folder cell: a 1 px border, plus a name band when the cell is big enough.
pub fn inner_rect(r: TmRect, header: f32) -> TmRect {
    let band = if r.h > header * 3.0 && r.w > 60.0 { header } else { 0.0 };
    let pad = if r.w > 6.0 && r.h > 6.0 { 1.0 } else { 0.0 };
    TmRect { x: r.x + pad, y: r.y + pad + band, w: (r.w - 2.0 * pad).max(0.0), h: (r.h - 2.0 * pad - band).max(0.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn areas_are_proportional_inside_bounds_and_disjoint() {
        let r = TmRect { x: 10.0, y: 20.0, w: 600.0, h: 400.0 };
        let vals = [500.0, 300.0, 120.0, 50.0, 20.0, 6.0, 3.0, 1.0];
        let total: f64 = vals.iter().sum();
        let rects = squarify(&vals, r);
        for (v, rc) in vals.iter().zip(&rects) {
            let want = (v / total) * (600.0 * 400.0);
            assert!((rc.area() as f64 - want).abs() < want * 0.01 + 0.5, "area {} vs {}", rc.area(), want);
            assert!(rc.x >= r.x - 0.01 && rc.y >= r.y - 0.01);
            assert!(rc.x + rc.w <= r.x + r.w + 0.01 && rc.y + rc.h <= r.y + r.h + 0.01);
        }
        for a in 0..rects.len() {
            for b in a + 1..rects.len() {
                let (p, q) = (rects[a], rects[b]);
                let ox = (p.x + p.w).min(q.x + q.w) - p.x.max(q.x);
                let oy = (p.y + p.h).min(q.y + q.h) - p.y.max(q.y);
                assert!(ox <= 0.01 || oy <= 0.01, "rects {a} and {b} overlap");
            }
        }
    }

    #[test]
    fn squares_stay_square_ish() {
        let rects = squarify(&[1.0; 16], TmRect { x: 0.0, y: 0.0, w: 400.0, h: 400.0 });
        for rc in rects {
            assert!((rc.w / rc.h).max(rc.h / rc.w) < 2.0);
        }
    }

    #[test]
    fn empty_and_degenerate_inputs() {
        assert!(squarify(&[], TmRect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 }).is_empty());
        let z = squarify(&[1.0, 2.0], TmRect::default());
        assert!(z.iter().all(|r| r.area() == 0.0));
    }
}
