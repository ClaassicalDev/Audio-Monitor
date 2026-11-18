#![windows_subsystem = "windows"]

mod audio;
mod config;
mod tray;
mod ui;

use parking_lot::RwLock;
use std::sync::Arc;
use ui::AudioMonitorApp;

fn configure_fonts(ctx: &eframe::egui::Context) {
    let mut fonts = eframe::egui::FontDefinitions::default();

    // 1. Native Windows Segoe UI for crisp proportional typography
    if let Ok(data) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
        fonts.font_data.insert(
            "segoe_ui".to_owned(),
            std::sync::Arc::new(eframe::egui::FontData::from_owned(data)),
        );
        fonts
            .families
            .entry(eframe::egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "segoe_ui".to_owned());
    }

    // 2. Native Windows Segoe UI Symbol for clean technical symbols
    if let Ok(data) = std::fs::read(r"C:\Windows\Fonts\seguisym.ttf") {
        fonts.font_data.insert(
            "segoe_ui_symbol".to_owned(),
            std::sync::Arc::new(eframe::egui::FontData::from_owned(data)),
        );
        fonts
            .families
            .entry(eframe::egui::FontFamily::Proportional)
            .or_default()
            .push("segoe_ui_symbol".to_owned());
        fonts
            .families
            .entry(eframe::egui::FontFamily::Monospace)
            .or_default()
            .push("segoe_ui_symbol".to_owned());
    }

    // 3. Native Windows Consolas for high-precision monospace dB & percentage readouts
    if let Ok(data) = std::fs::read(r"C:\Windows\Fonts\consola.ttf") {
        fonts.font_data.insert(
            "consolas".to_owned(),
            std::sync::Arc::new(eframe::egui::FontData::from_owned(data)),
        );
        fonts
            .families
            .entry(eframe::egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "consolas".to_owned());
    }

    ctx.set_fonts(fonts);
}

fn main() -> eframe::Result {
    let config = config::load_config();
    let config_arc = Arc::new(RwLock::new(config));

    // Studio Icon in RGBA for both window and system tray
    let (icon_rgba, icon_w, icon_h) = tray::generate_icon_rgba();
    let window_icon = eframe::egui::IconData {
        rgba: icon_rgba,
        width: icon_w,
        height: icon_h,
    };

    // Start background system tray thread
    let tray_handle = tray::start_tray(config_arc.clone());

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1140.0, 740.0])
            .with_min_inner_size([480.0, 480.0])
            .with_title("Audio Monitor — Studio WASAPI Mixer")
            .with_icon(window_icon),
        ..Default::default()
    };

    eframe::run_native(
        "AudioMonitor",
        native_options,
        Box::new(move |cc| {
            configure_fonts(&cc.egui_ctx);

            let mut visuals = eframe::egui::Visuals::dark();
            visuals.panel_fill = eframe::egui::Color32::from_rgb(17, 19, 24);
            visuals.window_fill = eframe::egui::Color32::from_rgb(20, 22, 28);
            cc.egui_ctx.set_visuals(visuals);

            Ok(Box::new(AudioMonitorApp::new(config_arc, tray_handle)))
        }),
    )
}
