//! The window (eframe/egui).

mod action_bar;
mod app;
mod dialogs;
mod ext_table;
mod compare_view;
mod projects_view;
mod files_view;
mod header;
mod labels_view;
mod history_view;
mod home;
mod list_table;
mod live_view;
mod lists;
pub mod loaded;
mod review_view;
mod settings;
mod settings_lists;
mod tool_dialog;
mod tools_view;
mod tree_columns;
mod tree_view;
mod treemap_view;
mod vm_disks_view;
mod widgets;

use eframe::egui;
use std::path::PathBuf;

/// Opens the window; scans `initial` right away when given.
pub fn run(initial: Option<PathBuf>, select: Option<PathBuf>, tab: Option<String>, ext: Option<String>, settings: Option<PathBuf>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1500.0, 950.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("diskscope"),
        ..Default::default()
    };
    eframe::run_native("diskscope", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, initial, select, tab, ext, settings)))))
}
