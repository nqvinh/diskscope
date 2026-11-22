//! Fallback font from Windows, so file names with Vietnamese letters such as "ấ" or "ợ" (and
//! Greek, Cyrillic) show instead of boxes: egui's built-in fonts lack them. A missing file is
//! skipped. CJK fonts are not loaded: they would add ~30 MB of memory for every user.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

const FALLBACKS: [&str; 1] = ["segoeui.ttf"];

pub fn install(ctx: &egui::Context) {
    let dir = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into()).join("Fonts");
    let mut fonts = FontDefinitions::default();
    for file in FALLBACKS {
        let Ok(bytes) = std::fs::read(dir.join(file)) else { continue };
        fonts.font_data.insert(file.to_string(), Arc::new(FontData::from_owned(bytes)));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts.families.entry(family).or_default().push(file.to_string()); // after the built-in fonts
        }
    }
    ctx.set_fonts(fonts);
}
