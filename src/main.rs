#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod version;
mod config;
mod xmltree;
mod formatter;
mod files;
mod mdb;
mod post;
mod model;
mod parser;
mod patterns;
mod theme;
mod timeline;
mod update;

use eframe::egui;
use std::sync::Arc;

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let try_font = |fonts: &mut egui::FontDefinitions, name: &str, path: &str, fam: egui::FontFamily| {
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(name.to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
            fonts.families.entry(fam).or_default().insert(0, name.to_owned());
        }
    };
    // Windows system fonts if present (look native); egui's built-in fonts stay as fallback
    try_font(&mut fonts, "segoe", "C:\\Windows\\Fonts\\segoeui.ttf", egui::FontFamily::Proportional);
    try_font(&mut fonts, "consolas", "C:\\Windows\\Fonts\\consola.ttf", egui::FontFamily::Monospace);
    if let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\seguisym.ttf") {
        fonts.font_data.insert("segsym".to_owned(), Arc::new(egui::FontData::from_owned(bytes)));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().push("segsym".to_owned());
    }
    // let proportional text fall back to the monospace font (has ● ◆ → ▲ ▼ …) and vice versa
    fonts.families.entry(egui::FontFamily::Proportional).or_default().push("Hack".to_owned());
    fonts.families.entry(egui::FontFamily::Monospace).or_default().push("Ubuntu-Light".to_owned());
    ctx.set_fonts(fonts);
}

pub const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

pub fn icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(ICON_PNG).unwrap_or_default()
}

fn main() -> eframe::Result<()> {
    let initial = std::env::args().nth(1).map(std::path::PathBuf::from);
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("Philips Log Viewer {}", version::VERSION))
            .with_inner_size([1500.0, 900.0])
            .with_min_inner_size([900.0, 500.0])
            .with_drag_and_drop(true)
            .with_icon(Arc::new(icon())),
        vsync: true,
        ..Default::default()
    };
    eframe::run_native(
        "Philips Log Viewer",
        opts,
        Box::new(move |cc| {
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc, initial)))
        }),
    )
}
