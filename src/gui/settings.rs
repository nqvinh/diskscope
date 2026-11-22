//! User settings (font size, theme, accent, row density), saved as JSON in
//! %LOCALAPPDATA%\diskscope\settings.json and applied live. Font size scales the whole UI.

use eframe::egui::{self, Color32, RichText, Ui};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Accent {
    Blue,
    Orange,
    Green,
}

impl Accent {
    fn color(self, dark: bool) -> Color32 {
        match (self, dark) {
            (Accent::Blue, true) => Color32::from_rgb(40, 110, 200),
            (Accent::Blue, false) => Color32::from_rgb(25, 90, 180),
            (Accent::Orange, true) => Color32::from_rgb(200, 100, 20),
            (Accent::Orange, false) => Color32::from_rgb(175, 80, 10),
            (Accent::Green, true) => Color32::from_rgb(40, 140, 80),
            (Accent::Green, false) => Color32::from_rgb(25, 115, 60),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)] // fields missing in older settings files take their default
pub struct Settings {
    /// Base text size in points (12-24); 14 = Normal.
    pub font: f32,
    pub theme: Theme,
    pub accent: Accent,
    pub comfortable: bool,
    /// Tree columns the user hid (bit mask, see tree_columns).
    pub tree_hidden: u16,
    pub show_extensions: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { font: 14.0, theme: Theme::System, accent: Accent::Blue, comfortable: false, tree_hidden: 0, show_extensions: true }
    }
}

static ROW_H: AtomicU32 = AtomicU32::new(18);
static DARK: AtomicBool = AtomicBool::new(true);

/// Table row height for the current density (unscaled; the zoom factor scales it).
pub fn row_h() -> f32 {
    ROW_H.load(Relaxed) as f32
}

pub fn is_dark() -> bool {
    DARK.load(Relaxed)
}

/// Warning text colour with readable contrast in both themes.
pub fn warn() -> Color32 {
    if is_dark() { Color32::from_rgb(255, 160, 100) } else { Color32::from_rgb(170, 60, 0) }
}

/// Secondary text (dimmed folders etc.) readable in both themes.
pub fn dim() -> Color32 {
    if is_dark() { Color32::from_gray(150) } else { Color32::from_gray(95) }
}

fn system_dark() -> bool {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let w = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (key, val) = (w("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"), w("AppsUseLightTheme"));
    let (mut data, mut len) = (1u32, 4u32);
    let rc = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), val.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), &mut data as *mut u32 as *mut _, &mut len) };
    rc != 0 || data == 0
}

impl Settings {
    pub fn file(over: &Option<PathBuf>) -> PathBuf {
        over.clone().unwrap_or_else(|| crate::sys::data_dir().join("settings.json"))
    }

    pub fn load(over: &Option<PathBuf>) -> Self {
        std::fs::read_to_string(Self::file(over)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self, over: &Option<PathBuf>) {
        let f = Self::file(over);
        if let Some(d) = f.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(f, serde_json::to_string_pretty(self).unwrap_or_default());
    }

    /// Applies theme, accent, density and font size to the whole window, right away.
    pub fn apply(&self, ctx: &egui::Context) {
        let dark = match self.theme {
            Theme::System => system_dark(),
            Theme::Light => false,
            Theme::Dark => true,
        };
        DARK.store(dark, Relaxed);
        ROW_H.store(if self.comfortable { 24 } else { 18 }, Relaxed);
        let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        let a = self.accent.color(dark);
        v.selection.bg_fill = a;
        v.selection.stroke.color = Color32::WHITE;
        v.hyperlink_color = a;
        ctx.set_visuals(v);
        ctx.set_zoom_factor((self.font / 14.0).clamp(12.0 / 14.0, 24.0 / 14.0));
    }

    /// Settings panel contents; returns true when something changed.
    pub fn ui(&mut self, ui: &mut Ui) -> bool {
        let before = *self;
        ui.label(RichText::new("Text size").strong());
        ui.horizontal(|ui| {
            for (name, pt) in [("Small", 12.0), ("Normal", 14.0), ("Large", 17.0), ("Extra large", 20.0)] {
                if ui.selectable_label((self.font - pt).abs() < 0.1, name).clicked() {
                    self.font = pt;
                }
            }
        });
        ui.add(egui::Slider::new(&mut self.font, 12.0..=24.0).step_by(1.0).suffix(" pt").text("custom"));
        ui.add_space(6.0);
        ui.label(RichText::new("Theme").strong());
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.theme, Theme::System, "Same as Windows");
            ui.selectable_value(&mut self.theme, Theme::Light, "Light");
            ui.selectable_value(&mut self.theme, Theme::Dark, "Dark");
        });
        ui.label(RichText::new("Accent colour").strong());
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.accent, Accent::Blue, "Blue");
            ui.selectable_value(&mut self.accent, Accent::Orange, "Orange");
            ui.selectable_value(&mut self.accent, Accent::Green, "Green");
        });
        ui.label(RichText::new("Rows").strong());
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.comfortable, false, "Compact");
            ui.selectable_value(&mut self.comfortable, true, "Comfortable");
        });
        ui.add_space(6.0);
        if ui.button("Reset to defaults").clicked() {
            *self = Settings::default();
        }
        ui.weak("Changes apply immediately and are remembered.");
        *self != before
    }
}

#[test]
fn settings_round_trip_and_defaults() {
    let f = std::env::temp_dir().join(format!("diskscope-settings-{}.json", std::process::id()));
    let over = Some(f.clone());
    assert_eq!(Settings::load(&over), Settings::default(), "missing file = defaults");
    let s = Settings { font: 20.0, theme: Theme::Light, accent: Accent::Green, comfortable: true, tree_hidden: 0b100, show_extensions: false };
    s.save(&over);
    assert_eq!(Settings::load(&over), s);
    std::fs::write(&f, r#"{"font":17.0,"theme":"Dark","accent":"Blue","comfortable":false}"#).unwrap();
    let old = Settings::load(&over);
    assert_eq!((old.font, old.tree_hidden, old.show_extensions), (17.0, 0, true), "a v0.3.0 file loads; new fields default");
    std::fs::write(&f, "not json").unwrap();
    assert_eq!(Settings::load(&over), Settings::default(), "a broken file falls back to defaults");
    std::fs::remove_file(&f).ok();
}
