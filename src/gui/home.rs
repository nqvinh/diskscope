//! First screen: one big card per drive (name, used/free bar, Scan button) plus "Scan a folder".

use super::settings::{dim, warn};
use crate::fmt::size;
use crate::sys::Drive;
use eframe::egui::{self, Color32, CornerRadius, RichText, Ui, Vec2};
use std::path::PathBuf;

/// Returns the path to scan when the user picks one.
pub fn ui(ui: &mut Ui, drives: &[Drive], elevated: bool) -> Option<PathBuf> {
    let mut pick = None;
    ui.add_space(12.0);
    ui.heading(RichText::new("Where did my disk space go?").size(26.0));
    ui.label(RichText::new("Pick a drive to see what takes up space and what you can safely clean up. Nothing is deleted without your confirmation.").color(dim()));
    if !elevated {
        ui.colored_label(warn(), "Tip: use \"Restart as administrator\" (top bar) for the fast scan: a whole drive in seconds instead of minutes.");
    }
    ui.add_space(14.0);
    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP).with_main_wrap(true), |ui| {
        for d in drives {
            let used = d.total.saturating_sub(d.free);
            let frac = used as f32 / d.total.max(1) as f32;
            egui::Frame::group(ui.style()).corner_radius(CornerRadius::same(10)).inner_margin(16.0).show(ui, |ui| ui.vertical(|ui| {
                ui.set_width(260.0);
                ui.label(RichText::new(format!("Drive {}", d.root.trim_end_matches('\\'))).size(20.0).strong());
                let bar = if frac > 0.9 { Color32::from_rgb(200, 60, 50) } else { ui.visuals().selection.bg_fill };
                ui.add(egui::ProgressBar::new(frac).fill(bar).desired_height(14.0));
                ui.label(format!("{} used of {}", size(used), size(d.total)));
                ui.label(RichText::new(format!("{} free", size(d.free))).strong().color(if frac > 0.9 { warn() } else { ui.visuals().text_color() }));
                ui.add_space(6.0);
                let btn = egui::Button::new(RichText::new("Scan this drive").size(16.0).strong()).min_size(Vec2::new(228.0, 36.0));
                if ui.add(btn).on_hover_text(if elevated { "Fast NTFS scan" } else { "Folder scan (minutes); run as administrator for the fast scan" }).clicked() {
                    pick = Some(PathBuf::from(&d.root));
                }
            }));
        }
    });
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if drives.len() > 1 {
            let all = egui::Button::new(RichText::new("Scan all drives at once").size(15.0).strong()).min_size(Vec2::new(220.0, 32.0));
            if ui.add(all).on_hover_text("Every local NTFS drive is scanned at the same time and shown in one tree (\"This PC\"). Cloud drives such as Google Drive are left out.").clicked() {
                pick = Some(PathBuf::from(crate::model::graft::ALL_DRIVES));
            }
        }
        if ui.add(egui::Button::new(RichText::new("Scan a folder…").size(15.0)).min_size(Vec2::new(180.0, 32.0))).clicked() {
            pick = rfd::FileDialog::new().pick_folder();
        }
    });
    pick
}
