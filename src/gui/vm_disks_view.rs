//! VM disks tab: WSL, Docker Desktop, Hyper-V, VirtualBox and VMware disks found in the scan or
//! in their usual places, with size and owner, and a "How to reclaim" panel of copyable steps for
//! the selected disk. Hints only: nothing is run, and the files are protected from deletion.

use super::loaded::Loaded;
use super::settings::{dim, warn};
use super::widgets::Act;
use crate::fmt::size;
use crate::rules::vm_disk_steps::reclaim_steps;
use crate::rules::vm_disks::{detect, VmDisk};
use eframe::egui::{self, RichText, Ui};

#[derive(Default)]
pub struct VmDisksView {
    /// Scan generation the list was built for.
    key: Option<u64>,
    disks: Vec<VmDisk>,
    selected: usize,
}

impl VmDisksView {
    pub fn ui(&mut self, ui: &mut Ui, data: &Loaded, acts: &mut Vec<Act>) {
        if self.key != Some(data.generation) {
            self.key = Some(data.generation);
            self.disks = detect(&data.tree, &data.ext_index.by_ext);
            self.selected = 0;
        }
        ui.horizontal(|ui| {
            let total: u64 = self.disks.iter().map(|d| d.size).sum();
            ui.label(RichText::new(format!("{} virtual machine / container disks, {}", self.disks.len(), size(total))).strong());
            if ui.button("Look again").clicked() {
                self.key = None;
            }
        });
        ui.label(RichText::new("These files grow when the VM or Docker writes data, but never shrink by themselves. Free space inside first, then compact. diskscope never deletes them: deleting one destroys the whole VM or distro.").color(dim()));
        ui.separator();
        if self.disks.is_empty() {
            ui.label("No .vhdx / .vhd / .vdi / .vmdk files in this scan or in the usual WSL, Docker, Hyper-V, VirtualBox and VMware folders.");
            return;
        }
        let list_h = (ui.available_height() * 0.45).max(120.0);
        egui::ScrollArea::vertical().id_salt("vm-list").max_height(list_h).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("vm-disks").striped(true).num_columns(4).spacing([16.0, 6.0]).show(ui, |ui| {
                for h in ["Size", "Owner", "Disk file", "Found"] {
                    ui.label(RichText::new(h).strong());
                }
                ui.end_row();
                for (k, d) in self.disks.iter().enumerate() {
                    ui.label(size(d.size));
                    ui.label(d.owner.name());
                    let path = d.path.display().to_string();
                    let resp = ui.selectable_label(self.selected == k, &path).on_hover_text("show how to reclaim its space");
                    if resp.clicked() {
                        self.selected = k;
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Open in Explorer").clicked() {
                            acts.push(Act::Explorer(d.path.clone()));
                            ui.close();
                        }
                        if ui.button("Copy path").clicked() {
                            ui.ctx().copy_text(path.clone());
                            ui.close();
                        }
                    });
                    ui.label(if d.in_scan { "in this scan" } else { "usual location" });
                    ui.end_row();
                }
            });
        });
        ui.separator();
        if let Some(d) = self.disks.get(self.selected) {
            steps_panel(ui, d, acts);
        }
    }
}

fn steps_panel(ui: &mut Ui, d: &VmDisk, acts: &mut Vec<Act>) {
    ui.label(RichText::new(format!("How to reclaim: {} ({}, {})", d.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), d.owner.name(), size(d.size))).strong());
    ui.colored_label(warn(), "Stop the VM / WSL / Docker before compacting. Run these yourself; diskscope does not run them.");
    egui::ScrollArea::vertical().id_salt("vm-steps").auto_shrink([false, false]).show(ui, |ui| {
        for (n, s) in reclaim_steps(d).iter().enumerate() {
            ui.label(format!("{}. {}", n + 1, s.text));
            if let Some(cmd) = &s.cmd {
                ui.horizontal(|ui| {
                    egui::Frame::new().fill(ui.visuals().extreme_bg_color).inner_margin(6).show(ui, |ui| {
                        ui.label(RichText::new(cmd).monospace());
                    });
                    if ui.button("Copy").clicked() {
                        ui.ctx().copy_text(cmd.clone());
                        acts.push(Act::Toast("command copied".into()));
                    }
                });
            }
            ui.add_space(4.0);
        }
    });
}
