use crate::audio::dsp::{slider_to_volume, volume_to_db, volume_to_slider};
use crate::audio::AudioEngine;
use crate::config::{save_config, AppConfig, ChannelConfig, CompressorConfig};
use crate::tray::TrayHandle;
use crate::ui::widgets::{
    channel_status_badge, draw_led_dot, gain_reduction_meter, pill_toggle_button,
    remove_channel_button, studio_segmented_vu_meter, vector_mute_button, vu_scale_labels,
    PillColor,
};
use eframe::egui::{
    self, pos2, Color32, CornerRadius, Rect, RichText, Sense, Stroke, StrokeKind, Vec2,
};
use parking_lot::RwLock;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;
use windows::Win32::Foundation::HWND;

pub struct AudioMonitorApp {
    config: AppConfig,
    config_arc: Arc<RwLock<AppConfig>>,
    engine: AudioEngine,
    tray: TrayHandle,
    main_hwnd: Option<HWND>,
    exit_requested: bool,

    // Tray & Flyout Mode states (EarTrumpet behavior)
    flyout_mode: bool,
    auto_hide_on_blur: bool,
    startup_frames: u32,

    // Filter and UI view states
    search_filter_channels: String,
    show_add_drawer: bool,
    show_comp_drawer: bool,

    // Add app drawer states
    add_search_query: String,
    show_system_procs: bool,
    manual_exe_input: String,
    manual_name_input: String,

    // Autosave
    last_save_time: Instant,
    config_dirty: bool,
}

impl AudioMonitorApp {
    pub fn new(config_arc: Arc<RwLock<AppConfig>>, tray: TrayHandle) -> Self {
        let config = { config_arc.read().clone() };
        let engine = AudioEngine::new(config.clone());
        let flyout_mode = tray.flyout_mode.load(Ordering::SeqCst);
        let auto_hide_on_blur = tray.auto_hide_enabled.load(Ordering::SeqCst);
        Self {
            config,
            config_arc,
            engine,
            tray,
            main_hwnd: None,
            exit_requested: false,
            flyout_mode,
            auto_hide_on_blur,
            startup_frames: 0,
            search_filter_channels: String::new(),
            show_add_drawer: false,
            show_comp_drawer: false,
            add_search_query: String::new(),
            show_system_procs: false,
            manual_exe_input: String::new(),
            manual_name_input: String::new(),
            last_save_time: Instant::now(),
            config_dirty: false,
        }
    }

    fn mark_dirty(&mut self) {
        self.config_dirty = true;
        *self.config_arc.write() = self.config.clone();
        self.engine.update_config(self.config.clone());
    }

    fn check_autosave(&mut self) {
        if self.config_dirty && self.last_save_time.elapsed().as_millis() > 500 {
            let _ = save_config(&self.config);
            self.config_dirty = false;
            self.last_save_time = Instant::now();
        }
    }

    fn add_channel_by_exe(&mut self, exe: &str, friendly_name: Option<&str>) {
        let exe_clean = exe.trim();
        if exe_clean.is_empty() {
            return;
        }

        // Avoid exact duplicates
        let lower = exe_clean.to_lowercase();
        if self
            .config
            .channels
            .iter()
            .any(|c| c.exe_name.to_lowercase() == lower)
        {
            return;
        }

        let name = match friendly_name {
            Some(fn_name) if !fn_name.trim().is_empty() => fn_name.trim().to_string(),
            _ => exe_clean.trim_end_matches(".exe").to_string(),
        };

        let id = format!("custom_{}", lower.replace('.', "_"));
        let is_discord = lower.contains("discord");
        self.config.channels.push(ChannelConfig {
            id,
            name,
            exe_name: exe_clean.to_string(),
            device_match: None,
            volume: 0.20, // ~ -14 dB default studio volume
            muted: false,
            is_trigger: is_discord,
            use_compressor: !is_discord,
            exempt_master: is_discord,
            is_custom: true,
            voice_clarity: is_discord,
            is_priority2: false,
        });

        self.mark_dirty();
    }
}

impl eframe::App for AudioMonitorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Request repaint ~33 FPS for silky-smooth studio VU meters
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));

        // Sync external tray updates (Voice Clarity, Spectral Ducking, Focus Mode)
        {
            let arc_cfg = self.config_arc.read();
            if arc_cfg.voice_clarity_enabled != self.config.voice_clarity_enabled
                || arc_cfg.compressor.spectral_ducking != self.config.compressor.spectral_ducking
                || arc_cfg.focus_mode_enabled != self.config.focus_mode_enabled
            {
                self.config.voice_clarity_enabled = arc_cfg.voice_clarity_enabled;
                self.config.compressor.spectral_ducking = arc_cfg.compressor.spectral_ducking;
                self.config.focus_mode_enabled = arc_cfg.focus_mode_enabled;
                self.engine.update_config(self.config.clone());
            }
        }

        // Sync flyout mode if toggled from tray click or context menu
        let tray_flyout = self.tray.flyout_mode.load(Ordering::SeqCst);
        if tray_flyout != self.flyout_mode {
            self.flyout_mode = tray_flyout;
            self.auto_hide_on_blur = self.tray.auto_hide_enabled.load(Ordering::SeqCst);
            if self.flyout_mode {
                self.show_add_drawer = false;
                self.show_comp_drawer = false;
            }
        }

        // Handle Tray Exit request
        if self.tray.exit_requested.load(Ordering::Relaxed) {
            let _ = save_config(&self.config);
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Handle Tray Show request
        if self.tray.show_requested.swap(false, Ordering::Relaxed) {
            self.flyout_mode = self.tray.flyout_mode.load(Ordering::SeqCst);
            self.auto_hide_on_blur = self.tray.auto_hide_enabled.load(Ordering::SeqCst);
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ui.ctx().request_repaint();
        }

        // Cache main HWND once available
        if self.main_hwnd.is_none() {
            if let Some(hwnd) = unsafe { crate::tray::find_main_window() } {
                self.main_hwnd = Some(hwnd);
                self.tray.set_main_hwnd(hwnd);
            }
        }

        // Handle pending reposition on the owner thread (zero cross-thread SendMessage!)
        let repos = self
            .tray
            .pending_reposition
            .swap(crate::tray::REPOS_NONE, Ordering::SeqCst);
        if repos != crate::tray::REPOS_NONE {
            if let Some(hwnd) = self.main_hwnd {
                unsafe {
                    if repos == crate::tray::REPOS_FLYOUT {
                        crate::tray::position_window_flyout_on_owner_thread(
                            hwnd,
                            crate::tray::FLYOUT_WIDTH,
                            crate::tray::FLYOUT_HEIGHT,
                        );
                    } else if repos == crate::tray::REPOS_STUDIO {
                        crate::tray::position_window_studio_on_owner_thread(
                            hwnd,
                            crate::tray::STUDIO_WIDTH,
                            crate::tray::STUDIO_HEIGHT,
                        );
                    }
                }
            }
        }

        // Let frame 0 complete OpenGL swapchain init cleanly, then hide to System Tray on frame 1
        if self.startup_frames < 2 {
            self.startup_frames += 1;
            if self.startup_frames == 2 && self.tray.last_shown_ms.load(Ordering::SeqCst) == 0 {
                if let Some(hwnd) = self.main_hwnd {
                    unsafe {
                        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                            hwnd,
                            windows::Win32::UI::WindowsAndMessaging::SW_HIDE,
                        );
                    }
                }
            }
        }

        // Handle window close (X) button -> minimize to tray if enabled
        if ui.ctx().input(|i| i.viewport().close_requested()) {
            if self.config.close_to_tray && !self.exit_requested && !self.tray.exit_requested.load(Ordering::Relaxed) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
                let _ = save_config(&self.config);
                let target = self.tray.get_main_hwnd().or_else(|| unsafe { crate::tray::find_main_window() });
                if let Some(hwnd) = target {
                    self.tray.set_main_hwnd(hwnd);
                    self.main_hwnd = Some(hwnd);
                    self.tray.last_hidden_ms.store(crate::tray::now_ms(), Ordering::SeqCst);
                    unsafe {
                        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                            hwnd,
                            windows::Win32::UI::WindowsAndMessaging::SW_HIDE,
                        );
                    }
                }
            }
        }

        self.check_autosave();

        let engine_state = { self.engine.state.read().clone() };

        // ==================== TOP PANEL: RESPONSIVE HEADER ====================
        egui::Panel::top("top_header_panel")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(17, 19, 25))
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(34, 38, 50))),
            )
            .show(ui, |ui| {
                if self.flyout_mode {
                    // ==========================================
                    // COMPACT FLYOUT HEADER (EarTrumpet Style)
                    // ==========================================
                    // Row 1: Brand + Flyout Controls
                    ui.horizontal(|ui| {
                        let (rect, _resp) = ui.allocate_exact_size(Vec2::new(3.0, 18.0), Sense::hover());
                        ui.painter().rect_filled(rect, CornerRadius::same(1), Color32::from_rgb(14, 165, 233));
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("AUDIO MONITOR")
                                .strong()
                                .size(13.5)
                                .color(Color32::from_rgb(243, 244, 246)),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Close / Hide button
                            if ui
                                .button(RichText::new("— Ocultar").size(11.0))
                                .on_hover_text("Ocultar el mezclador rápido en la bandeja del sistema")
                                .clicked()
                            {
                                self.tray.has_gained_focus.store(false, Ordering::SeqCst);
                                self.tray.last_hidden_ms.store(crate::tray::now_ms(), Ordering::SeqCst);
                                if let Some(hwnd) = self.main_hwnd {
                                    unsafe {
                                        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                                            hwnd,
                                            windows::Win32::UI::WindowsAndMessaging::SW_HIDE,
                                        );
                                    }
                                }
                            }

                            // Switch to full normal Studio window
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Ventana Normal")
                                            .strong()
                                            .size(11.0)
                                            .color(Color32::from_rgb(240, 249, 255)),
                                    )
                                    .fill(Color32::from_rgb(2, 132, 199)),
                                )
                                .on_hover_text("Abrir la ventana normal completa (Modo Estudio 1140x740)")
                                .clicked()
                            {
                                self.flyout_mode = false;
                                self.tray.flyout_mode.store(false, Ordering::SeqCst);
                                self.auto_hide_on_blur = false;
                                self.tray.auto_hide_enabled.store(false, Ordering::SeqCst);
                                self.tray.has_gained_focus.store(false, Ordering::SeqCst);
                                self.tray.last_shown_ms.store(crate::tray::now_ms(), Ordering::SeqCst);
                                if let Some(hwnd) = self.main_hwnd {
                                    unsafe {
                                        crate::tray::position_window_studio_on_owner_thread(
                                            hwnd,
                                            crate::tray::STUDIO_WIDTH,
                                            crate::tray::STUDIO_HEIGHT,
                                        );
                                    }
                                }
                            }

                            // Pin / Auto-hide toggle
                            let (pin_text, pin_bg, pin_fg) = if self.auto_hide_on_blur {
                                (
                                    "Auto-ocultar",
                                    Color32::from_rgb(28, 31, 41),
                                    Color32::from_rgb(156, 163, 175),
                                )
                            } else {
                                (
                                    "Fijado",
                                    Color32::from_rgb(14, 116, 144),
                                    Color32::from_rgb(240, 249, 255),
                                )
                            };
                            let pin_tip = if self.auto_hide_on_blur {
                                "Se oculta al hacer clic fuera (como EarTrumpet). Haz clic aquí para fijar el popup."
                            } else {
                                "Popup fijado: no se oculta al hacer clic fuera. Haz clic para activar auto-ocultado."
                            };
                            if ui
                                .add(egui::Button::new(RichText::new(pin_text).size(10.5).color(pin_fg)).fill(pin_bg))
                                .on_hover_text(pin_tip)
                                .clicked()
                            {
                                self.auto_hide_on_blur = !self.auto_hide_on_blur;
                                self.tray.auto_hide_enabled.store(self.auto_hide_on_blur, Ordering::SeqCst);
                            }
                        });
                    });

                    ui.add_space(6.0);

                    // Row 2: Master Volume Slider & Output
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Maestro:")
                                .strong()
                                .size(11.5)
                                .color(Color32::from_rgb(156, 163, 175)),
                        );

                        let mut master_vol = self.config.master_volume;
                        let master_pct = format!("{:3.0}%", master_vol * 100.0);

                        ui.spacing_mut().slider_width = 140.0;
                        if ui
                            .add(
                                egui::Slider::new(&mut master_vol, 0.0..=1.5)
                                    .show_value(false),
                            )
                            .changed()
                        {
                            self.config.master_volume = master_vol;
                            self.mark_dirty();
                        }

                        ui.label(
                            RichText::new(master_pct)
                                .monospace()
                                .size(10.5)
                                .color(Color32::from_rgb(209, 213, 219)),
                        );

                        studio_segmented_vu_meter(ui, engine_state.master_peak_db, 0.0, 50.0, 9.0);

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let red_db = engine_state.sidechain_reduction_db;
                            if red_db < -0.2 {
                                gain_reduction_meter(ui, red_db, 50.0, 8.0);
                                ui.colored_label(
                                    Color32::from_rgb(249, 115, 22),
                                    RichText::new(format!("{:.0}dB", red_db)).monospace().size(10.0).strong(),
                                );
                            }
                        });
                    });

                    ui.add_space(4.0);

                    // Row 3: Compact Quick Toggles (Voice, Ducking, Focus) & Filter
                    ui.horizontal(|ui| {
                        // Quick Voice Clarity Toggle
                        let vc_on = self.config.voice_clarity_enabled;
                        let vc_color = if vc_on { Color32::from_rgb(5, 150, 105) } else { Color32::from_rgb(28, 31, 41) };
                        if ui
                            .add(egui::Button::new(RichText::new("Voz Clara").size(10.5)).fill(vc_color))
                            .on_hover_text("Claridad Vocal Discord DSP")
                            .clicked()
                        {
                            self.config.voice_clarity_enabled = !self.config.voice_clarity_enabled;
                            self.mark_dirty();
                        }

                        // Quick Spectral Ducking Toggle
                        let sd_on = self.config.compressor.spectral_ducking;
                        let sd_color = if sd_on { Color32::from_rgb(2, 132, 199) } else { Color32::from_rgb(28, 31, 41) };
                        if ui
                            .add(egui::Button::new(RichText::new("Espectral").size(10.5)).fill(sd_color))
                            .on_hover_text("Ducking Espectral Inteligente (Trackspacer)")
                            .clicked()
                        {
                            self.config.compressor.spectral_ducking = !self.config.compressor.spectral_ducking;
                            self.mark_dirty();
                        }

                        // Quick Focus Mode Toggle
                        let fm_on = self.config.focus_mode_enabled;
                        let fm_color = if fm_on { Color32::from_rgb(180, 83, 9) } else { Color32::from_rgb(28, 31, 41) };
                        if ui
                            .add(egui::Button::new(RichText::new("Enfoque P2").size(10.5)).fill(fm_color))
                            .on_hover_text("Modo Enfoque Prioridad 2")
                            .clicked()
                        {
                            self.config.focus_mode_enabled = !self.config.focus_mode_enabled;
                            self.mark_dirty();
                        }

                        ui.add_space(4.0);

                        // Mini search input
                        ui.add(
                            egui::TextEdit::singleline(&mut self.search_filter_channels)
                                .hint_text("Filtrar...")
                                .desired_width(110.0),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let only_active = self.config.show_only_active;
                            let act_label = if only_active { "Activas" } else { "Todas" };
                            if ui
                                .button(RichText::new(act_label).size(10.5))
                                .on_hover_text("Alternar entre ver todas las apps o solo las que están en ejecución")
                                .clicked()
                            {
                                self.config.show_only_active = !self.config.show_only_active;
                                self.mark_dirty();
                            }
                        });
                    });
                } else {
                    // ==========================================
                    // STUDIO CONSOLE HEADER (Non-Overlapping Rows)
                    // ==========================================
                    // ROW 1: Master Routing, Global Output & Window Management (Strict Left/Right Rects)
                    let (row1_rect, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), 26.0),
                        Sense::hover(),
                    );
                    let right_btns_w = 290.0;
                    let row1_right_rect = Rect::from_min_max(
                        pos2((row1_rect.max.x - right_btns_w).max(row1_rect.min.x), row1_rect.min.y),
                        row1_rect.max,
                    );
                    let row1_left_rect = Rect::from_min_max(
                        row1_rect.min,
                        pos2((row1_right_rect.min.x - 10.0).max(row1_rect.min.x), row1_rect.max.y),
                    );

                    ui.scope_builder(egui::UiBuilder::new().max_rect(row1_left_rect), |ui| {
                        ui.set_clip_rect(row1_left_rect);
                        ui.horizontal(|ui| {
                            // Brand Badge
                            let (rect, _resp) = ui.allocate_exact_size(Vec2::new(3.0, 20.0), Sense::hover());
                            ui.painter().rect_filled(rect, CornerRadius::same(1), Color32::from_rgb(14, 165, 233));
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new("AUDIO MONITOR")
                                    .strong()
                                    .size(14.5)
                                    .color(Color32::from_rgb(243, 244, 246)),
                            );
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new("WASAPI")
                                    .monospace()
                                    .size(9.5)
                                    .color(Color32::from_rgb(14, 165, 233)),
                            );

                            ui.add_space(12.0);

                            // Output Device Selector (truncated display so long headset names never push Maestro!)
                            ui.label(
                                RichText::new("Salida:")
                                    .strong()
                                    .size(11.5)
                                    .color(Color32::from_rgb(156, 163, 175)),
                            );
                            let raw_dev_name = &engine_state.current_output_device_name;
                            let short_dev_name: String = if raw_dev_name.chars().count() > 26 {
                                let prefix: String = raw_dev_name.chars().take(25).collect();
                                format!("{}…", prefix)
                            } else {
                                raw_dev_name.clone()
                            };

                            egui::ComboBox::from_id_salt("output_device_combo")
                                .selected_text(
                                    RichText::new(short_dev_name)
                                        .size(11.5)
                                        .color(Color32::from_rgb(56, 189, 248)),
                                )
                                .width(190.0)
                                .show_ui(ui, |ui| {
                                    for dev in &engine_state.output_devices {
                                        let is_selected = self.config.output_device_id == dev.id;
                                        if ui.selectable_label(is_selected, &dev.name).clicked() {
                                            self.config.output_device_id = dev.id.clone();
                                            self.mark_dirty();
                                        }
                                    }
                                })
                                .response
                                .on_hover_text(raw_dev_name);

                            ui.add_space(12.0);

                            // Master Volume Fader
                            ui.label(
                                RichText::new("Maestro:")
                                    .strong()
                                    .size(11.5)
                                    .color(Color32::from_rgb(156, 163, 175)),
                            );

                            let mut master_vol = self.config.master_volume;
                            let master_pct = format!("{:3.0}%", master_vol * 100.0);

                            ui.spacing_mut().slider_width = 110.0;
                            if ui
                                .add(
                                    egui::Slider::new(&mut master_vol, 0.0..=1.5)
                                        .show_value(false),
                                )
                                .changed()
                            {
                                self.config.master_volume = master_vol;
                                self.mark_dirty();
                            }

                            // Monospace readout for master
                            ui.label(
                                RichText::new(master_pct)
                                    .monospace()
                                    .size(10.5)
                                    .color(Color32::from_rgb(209, 213, 219)),
                            );

                            // Master Stereo Peak VU meter
                            studio_segmented_vu_meter(ui, engine_state.master_peak_db, 0.0, 50.0, 9.0);
                        });
                    });

                    // Right-aligned Window Management Buttons (Isolated in row1_right_rect)
                    ui.scope_builder(egui::UiBuilder::new().max_rect(row1_right_rect), |ui| {
                        ui.set_clip_rect(row1_right_rect);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Hide window to system tray
                            if ui
                                .button(RichText::new("— Ocultar a Bandeja").size(11.0))
                                .on_hover_text("Oculta la ventana en el icono del reloj sin detener el monitoreo.")
                                .clicked()
                            {
                                self.tray.has_gained_focus.store(false, Ordering::SeqCst);
                                self.tray.last_hidden_ms.store(crate::tray::now_ms(), Ordering::SeqCst);
                                if let Some(hwnd) = self.main_hwnd {
                                    unsafe {
                                        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                                            hwnd,
                                            windows::Win32::UI::WindowsAndMessaging::SW_HIDE,
                                        );
                                    }
                                }
                            }

                            // Switch to Flyout mode
                            if ui
                                .button(RichText::new("Modo Bandeja (Popup)").size(11.0))
                                .on_hover_text("Cambia al mezclador rápido compacto junto al reloj (estilo EarTrumpet).")
                                .clicked()
                            {
                                self.flyout_mode = true;
                                self.tray.flyout_mode.store(true, Ordering::SeqCst);
                                self.auto_hide_on_blur = true;
                                self.tray.auto_hide_enabled.store(true, Ordering::SeqCst);
                                self.tray.has_gained_focus.store(false, Ordering::SeqCst);
                                self.show_add_drawer = false;
                                self.show_comp_drawer = false;
                                self.tray.last_shown_ms.store(crate::tray::now_ms(), Ordering::SeqCst);
                                if let Some(hwnd) = self.main_hwnd {
                                    unsafe {
                                        crate::tray::position_window_flyout_on_owner_thread(
                                            hwnd,
                                            crate::tray::FLYOUT_WIDTH,
                                            crate::tray::FLYOUT_HEIGHT,
                                        );
                                    }
                                }
                            }
                        });
                    });

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);

                    // ROW 2: Filters, DSP Quick Controls & Studio Drawers
                    let active_count = self
                        .config
                        .channels
                        .iter()
                        .filter(|c| {
                            engine_state
                                .channel_status
                                .get(&c.id)
                                .map(|s| s.active)
                                .unwrap_or(false)
                        })
                        .count();
                    let total_count = self.config.channels.len();

                    ui.horizontal(|ui| {
                        // Mixer Channel Search Filter
                        ui.add(
                            egui::TextEdit::singleline(&mut self.search_filter_channels)
                                .hint_text("Filtrar canales...")
                                .desired_width(140.0),
                        );

                        ui.add_space(4.0);

                        // Segmented Filter Toggle: Todas vs Solo Activas
                        let btn_all_active = !self.config.show_only_active;
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("Todas ({})", total_count))
                                        .size(11.0)
                                        .color(if btn_all_active {
                                            Color32::from_rgb(240, 249, 255)
                                        } else {
                                            Color32::from_rgb(156, 163, 175)
                                        }),
                                )
                                .fill(if btn_all_active {
                                    Color32::from_rgb(14, 116, 144)
                                } else {
                                    Color32::from_rgb(28, 31, 41)
                                }),
                            )
                            .clicked()
                        {
                            self.config.show_only_active = false;
                            self.mark_dirty();
                        }

                        let btn_only_active = self.config.show_only_active;
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("Activas ({})", active_count))
                                        .size(11.0)
                                        .color(if btn_only_active {
                                            Color32::from_rgb(236, 253, 245)
                                        } else {
                                            Color32::from_rgb(156, 163, 175)
                                        }),
                                )
                                .fill(if btn_only_active {
                                    Color32::from_rgb(5, 150, 105)
                                } else {
                                    Color32::from_rgb(28, 31, 41)
                                }),
                            )
                            .clicked()
                        {
                            self.config.show_only_active = true;
                            self.mark_dirty();
                        }

                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(4.0);

                        // DSP: Discord Voice Clarity Toggle
                        let vc_active = self.config.voice_clarity_enabled;
                        let (vc_label, vc_bg, vc_fg) = if vc_active {
                            ("VOZ CLARA", Color32::from_rgb(5, 150, 105), Color32::from_rgb(236, 253, 245))
                        } else {
                            ("VOZ CLARA", Color32::from_rgb(38, 42, 54), Color32::from_rgb(156, 163, 175))
                        };

                        let vc_resp = ui.add(
                            egui::Button::new(RichText::new(vc_label).strong().size(11.0).color(vc_fg))
                                .fill(vc_bg),
                        );
                        if vc_resp.on_hover_text("Claridad de Voz Discord: HPF + EQ de presencia vocal + Nivelador dinámico.").clicked() {
                            self.config.voice_clarity_enabled = !self.config.voice_clarity_enabled;
                            if self.config.voice_clarity_enabled {
                                for ch in &mut self.config.channels {
                                    if ch.id == "discord" || ch.exe_name.to_lowercase() == "discord.exe" || ch.is_trigger {
                                        ch.voice_clarity = true;
                                    }
                                }
                            }
                            self.mark_dirty();
                        }

                        if self.config.voice_clarity_enabled {
                            let current_mode = self.config.voice_clarity_mode;
                            let mode_text = match current_mode {
                                1 => "Extra Claridad",
                                2 => "Broadcast",
                                _ => "Nítido",
                            };

                            egui::ComboBox::from_id_salt("voice_clarity_mode_dropdown")
                                .selected_text(
                                    RichText::new(mode_text)
                                        .size(11.0)
                                        .color(Color32::from_rgb(52, 211, 153)),
                                )
                                .width(105.0)
                                .show_ui(ui, |ui| {
                                    if ui.selectable_label(current_mode == 0, "Nítido / Presencia").clicked() {
                                        self.config.voice_clarity_mode = 0;
                                        self.mark_dirty();
                                    }
                                    if ui.selectable_label(current_mode == 1, "Extra Claridad").clicked() {
                                        self.config.voice_clarity_mode = 1;
                                        self.mark_dirty();
                                    }
                                    if ui.selectable_label(current_mode == 2, "Broadcast (Cálido)").clicked() {
                                        self.config.voice_clarity_mode = 2;
                                        self.mark_dirty();
                                    }
                                });
                        }

                        // DSP: Ducking Espectral Toggle
                        let sd_active = self.config.compressor.spectral_ducking;
                        let (sd_label, sd_bg, sd_fg) = if sd_active {
                            ("DUCKING ESPECTRAL", Color32::from_rgb(2, 132, 199), Color32::from_rgb(240, 249, 255))
                        } else {
                            ("DUCKING ESPECTRAL", Color32::from_rgb(38, 42, 54), Color32::from_rgb(156, 163, 175))
                        };
                        if ui
                            .add(egui::Button::new(RichText::new(sd_label).strong().size(11.0).color(sd_fg)).fill(sd_bg))
                            .on_hover_text("Ducking Espectral (Trackspacer): Abre un hueco en 300Hz-4kHz (125%) y baja el resto (75%).")
                            .clicked()
                        {
                            self.config.compressor.spectral_ducking = !self.config.compressor.spectral_ducking;
                            self.mark_dirty();
                        }

                        // DSP: De-Esser + Gate Toggle
                        let de_active = self.config.deesser_enabled && self.config.noise_gate_enabled;
                        let (de_label, de_bg, de_fg) = if de_active {
                            ("DE-ESSER + GATE", Color32::from_rgb(5, 150, 105), Color32::from_rgb(236, 253, 245))
                        } else {
                            ("DE-ESSER + GATE", Color32::from_rgb(38, 42, 54), Color32::from_rgb(156, 163, 175))
                        };
                        if ui
                            .add(egui::Button::new(RichText::new(de_label).strong().size(11.0).color(de_fg)).fill(de_bg))
                            .on_hover_text("De-Esser (6.2kHz) + Compuerta Anti-Siseo (-14dB).")
                            .clicked()
                        {
                            let target = !de_active;
                            self.config.deesser_enabled = target;
                            self.config.noise_gate_enabled = target;
                            self.mark_dirty();
                        }

                        // DSP: Modo Enfoque Toggle
                        let fm_active = self.config.focus_mode_enabled;
                        let (fm_label, fm_bg, fm_fg) = if fm_active {
                            ("ENFOQUE PRIO 2", Color32::from_rgb(180, 83, 9), Color32::from_rgb(254, 252, 232))
                        } else {
                            ("ENFOQUE PRIO 2", Color32::from_rgb(38, 42, 54), Color32::from_rgb(156, 163, 175))
                        };
                        if ui
                            .add(egui::Button::new(RichText::new(fm_label).strong().size(11.0).color(fm_fg)).fill(fm_bg))
                            .on_hover_text("Modo Enfoque / Prioridad 2: Apps PRIO 2 bajan las apps normales.")
                            .clicked()
                        {
                            self.config.focus_mode_enabled = !self.config.focus_mode_enabled;
                            self.mark_dirty();
                        }

                        // Right action buttons (Studio Drawers)
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Toggle Compressor Drawer
                            let comp_active = self.show_comp_drawer;
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Ajustes Compresor")
                                            .size(11.0)
                                            .color(if comp_active {
                                                Color32::from_rgb(255, 251, 235)
                                            } else {
                                                Color32::from_rgb(209, 213, 219)
                                            }),
                                    )
                                    .fill(if comp_active {
                                        Color32::from_rgb(180, 83, 9)
                                    } else {
                                        Color32::from_rgb(32, 36, 48)
                                    }),
                                )
                                .clicked()
                            {
                                self.show_comp_drawer = !self.show_comp_drawer;
                                if self.show_comp_drawer {
                                    self.show_add_drawer = false;
                                }
                            }

                            // Toggle Add Application Drawer
                            let add_active = self.show_add_drawer;
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("+ Agregar")
                                            .strong()
                                            .size(11.0)
                                            .color(if add_active {
                                                Color32::from_rgb(240, 249, 255)
                                            } else {
                                                Color32::from_rgb(56, 189, 248)
                                            }),
                                    )
                                    .fill(if add_active {
                                        Color32::from_rgb(2, 132, 199)
                                    } else {
                                        Color32::from_rgb(24, 34, 52)
                                    }),
                                )
                                .clicked()
                            {
                                self.show_add_drawer = !self.show_add_drawer;
                                if self.show_add_drawer {
                                    self.show_comp_drawer = false;
                                }
                            }
                        });
                    });

                    ui.add_space(5.0);

                    // ROW 3: Dedicated Studio Telemetry Bar (No collision with sliders or buttons)
                    ui.horizontal(|ui| {
                        // Sidechain Live Gain Reduction
                        let red_db = engine_state.sidechain_reduction_db;
                        let text = if red_db < -0.1 {
                            format!("{:.1} dB", red_db)
                        } else {
                            "0.0 dB".to_string()
                        };

                        let text_color = if red_db < -6.0 {
                            Color32::from_rgb(239, 68, 68)
                        } else if red_db < -0.5 {
                            Color32::from_rgb(249, 115, 22)
                        } else {
                            Color32::from_rgb(156, 163, 175)
                        };

                        ui.label(
                            RichText::new("Atenuación Sidechain:")
                                .size(10.5)
                                .color(Color32::from_rgb(156, 163, 175)),
                        );
                        gain_reduction_meter(ui, red_db, 80.0, 8.0);
                        ui.colored_label(
                            text_color,
                            RichText::new(text).monospace().size(10.5).strong(),
                        );

                        // Focus Mode Live Reduction Display
                        let f_red = engine_state.focus_reduction_db;
                        if self.config.focus_mode_enabled && f_red < -0.2 {
                            ui.add_space(12.0);
                            ui.separator();
                            ui.add_space(12.0);

                            ui.label(
                                RichText::new("Enfoque Prio 2:")
                                    .size(10.5)
                                    .color(Color32::from_rgb(250, 204, 21)),
                            );
                            gain_reduction_meter(ui, f_red, 65.0, 8.0);
                            ui.colored_label(
                                Color32::from_rgb(250, 204, 21),
                                RichText::new(format!("{:.1} dB", f_red)).monospace().size(10.5).strong(),
                            );
                        }

                        // Right Telemetry summary
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let status_text = format!("Canales activos: {} / {}", active_count, total_count);
                            ui.label(RichText::new(status_text).size(10.5).color(Color32::from_rgb(110, 118, 138)));
                        });
                    });
                }
            });

        // ==================== BOTTOM DRAWER: ADD APPLICATION ====================
        if self.show_add_drawer && !self.flyout_mode {
            egui::Panel::bottom("add_drawer_panel")
                .frame(
                    egui::Frame::new()
                        .fill(Color32::from_rgb(18, 20, 27))
                        .inner_margin(egui::Margin::symmetric(16, 12))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(45, 52, 70))),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("AGREGAR APLICACIÓN (BÚSQUEDA POR NOMBRE TIPO OBS)")
                                .strong()
                                .size(13.0)
                                .color(Color32::from_rgb(56, 189, 248)),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(RichText::new("✕ Cerrar").size(11.0)).clicked() {
                                self.show_add_drawer = false;
                            }
                        });
                    });

                    ui.label(
                        RichText::new(
                            "Escribe para buscar aplicaciones en ejecución por su título de ventana o nombre de ejecutable. También puedes agregar cualquier ejecutable aunque esté cerrado.",
                        )
                        .size(11.0)
                        .color(Color32::from_rgb(140, 148, 170)),
                    );

                    ui.add_space(6.0);

                    // Search input & system proc toggle
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Buscar:").strong().size(12.0));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.add_search_query)
                                .hint_text("Buscar por ventana o ejecutable (ej. Overwatch, Spotify, Chrome)...")
                                .desired_width(360.0),
                        );

                        ui.add_space(10.0);
                        ui.checkbox(&mut self.show_system_procs, "Mostrar procesos del sistema");
                    });

                    ui.add_space(8.0);

                    // Running processes matching query
                    let query = self.add_search_query.trim().to_lowercase();
                    let existing_exes: std::collections::HashSet<String> = self
                        .config
                        .channels
                        .iter()
                        .map(|c| c.exe_name.to_lowercase())
                        .collect();

                    let matching_procs: Vec<_> = engine_state
                        .available_processes
                        .iter()
                        .filter(|p| {
                            if existing_exes.contains(&p.exe_name.to_lowercase()) {
                                return false;
                            }
                            if !self.show_system_procs && p.is_system {
                                return false;
                            }
                            if query.is_empty() {
                                return true;
                            }

                            let matches_exe = p.exe_name.to_lowercase().contains(&query);
                            let matches_title = p
                                .window_title
                                .as_ref()
                                .map(|t| t.to_lowercase().contains(&query))
                                .unwrap_or(false);

                            matches_exe || matches_title
                        })
                        .collect();

                    egui::ScrollArea::vertical()
                        .max_height(140.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if matching_procs.is_empty() {
                                ui.add_space(8.0);
                                ui.colored_label(
                                    Color32::from_rgb(156, 163, 175),
                                    "No se encontraron procesos en ejecución con ese nombre.",
                                );
                            } else {
                                for p in matching_procs {
                                    ui.horizontal(|ui| {
                                        let display_title = p
                                            .window_title
                                            .as_deref()
                                            .unwrap_or_else(|| p.exe_name.trim_end_matches(".exe"));

                                        ui.allocate_ui_with_layout(
                                            Vec2::new(260.0, 22.0),
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(display_title)
                                                        .strong()
                                                        .size(12.0)
                                                        .color(Color32::from_rgb(243, 244, 246)),
                                                );
                                            },
                                        );

                                        ui.allocate_ui_with_layout(
                                            Vec2::new(160.0, 22.0),
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(&p.exe_name)
                                                        .monospace()
                                                        .size(11.0)
                                                        .color(Color32::from_rgb(156, 163, 175)),
                                                );
                                            },
                                        );

                                        ui.allocate_ui_with_layout(
                                            Vec2::new(110.0, 22.0),
                                            egui::Layout::left_to_right(egui::Align::Center),
                                            |ui| {
                                                channel_status_badge(ui, true);
                                            },
                                        );

                                        if ui
                                            .button(RichText::new("+ Agregar").strong().size(11.0))
                                            .clicked()
                                        {
                                            let friendly = p
                                                .window_title
                                                .as_deref()
                                                .unwrap_or_else(|| p.exe_name.trim_end_matches(".exe"));
                                            self.add_channel_by_exe(&p.exe_name, Some(friendly));
                                        }
                                    });
                                    ui.separator();
                                }
                            }
                        });

                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        if !query.is_empty() {
                            let quick_add_label = format!("+ Agregar \"{}\" como ejecutable", self.add_search_query.trim());
                            if ui
                                .button(
                                    RichText::new(quick_add_label)
                                        .strong()
                                        .color(Color32::from_rgb(56, 189, 248)),
                                )
                                .clicked()
                            {
                                let query_str = self.add_search_query.trim().to_string();
                                let target_exe = if query_str.to_lowercase().ends_with(".exe") {
                                    query_str.clone()
                                } else {
                                    format!("{}.exe", query_str)
                                };
                                self.add_channel_by_exe(&target_exe, Some(&query_str));
                                self.add_search_query.clear();
                            }

                            ui.separator();
                        }

                        ui.label(RichText::new("O agregar manualmente:").size(11.0));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.manual_exe_input)
                                .hint_text("Ejecutable (ej. Game.exe)")
                                .desired_width(140.0),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut self.manual_name_input)
                                .hint_text("Nombre visible")
                                .desired_width(120.0),
                        );

                        if ui.button(RichText::new("+ Guardar manual").strong()).clicked() {
                            let exe = self.manual_exe_input.trim().to_string();
                            let name = self.manual_name_input.trim().to_string();
                            if !exe.is_empty() {
                                let friendly = if name.is_empty() { None } else { Some(name.as_str()) };
                                self.add_channel_by_exe(&exe, friendly);
                                self.manual_exe_input.clear();
                                self.manual_name_input.clear();
                            }
                        }
                    });
                });
        }

        // ==================== BOTTOM DRAWER: COMPRESSOR SETTINGS ====================
        if self.show_comp_drawer && !self.flyout_mode {
            egui::Panel::bottom("comp_drawer_panel")
                .frame(
                    egui::Frame::new()
                        .fill(Color32::from_rgb(18, 20, 27))
                        .inner_margin(egui::Margin::symmetric(16, 12))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(55, 45, 30))),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("AJUSTES DEL COMPRESOR SIDECHAIN (STUDIO DUCKING & ANTI-PUMPING)")
                                .strong()
                                .size(13.0)
                                .color(Color32::from_rgb(251, 191, 36)),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(RichText::new("✕ Cerrar").size(11.0)).clicked() {
                                self.show_comp_drawer = false;
                            }
                        });
                    });

                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        if ui
                            .checkbox(
                                &mut self.config.compressor.enabled,
                                RichText::new("Activar Compresor Sidechain").strong(),
                            )
                            .changed()
                        {
                            self.mark_dirty();
                        }

                        ui.add_space(16.0);
                        ui.separator();
                        ui.add_space(16.0);

                        // Presets
                        ui.label(RichText::new("Presets de Estudio:").strong().size(11.5));

                        let cur_preset = self.config.compressor.preset_name.clone();

                        // 1. Gaming Pro
                        let is_gp = cur_preset == "Gaming Pro";
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Gaming Pro")
                                        .size(11.0)
                                        .color(if is_gp {
                                            Color32::from_rgb(255, 251, 235)
                                        } else {
                                            Color32::from_rgb(209, 213, 219)
                                        }),
                                )
                                .fill(if is_gp {
                                    Color32::from_rgb(180, 83, 9)
                                } else {
                                    Color32::from_rgb(32, 36, 48)
                                }),
                            )
                            .on_hover_text("Discord + Juegos: umbral -38dB, hold 85ms (cero bombeo), tope -15dB.")
                            .clicked()
                        {
                            self.config.compressor = CompressorConfig::preset_gaming_pro();
                            self.mark_dirty();
                        }

                        // 2. Competitivo
                        let is_comp = cur_preset == "Competitivo";
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Competitivo")
                                        .size(11.0)
                                        .color(if is_comp {
                                            Color32::from_rgb(255, 251, 235)
                                        } else {
                                            Color32::from_rgb(209, 213, 219)
                                        }),
                                )
                                .fill(if is_comp {
                                    Color32::from_rgb(180, 83, 9)
                                } else {
                                    Color32::from_rgb(32, 36, 48)
                                }),
                            )
                            .on_hover_text("Prioridad total a la voz: ratio 6:1, ataque 1.5ms, hold 110ms, baja hasta -22dB.")
                            .clicked()
                        {
                            self.config.compressor = CompressorConfig::preset_competitive();
                            self.mark_dirty();
                        }

                        // 3. Suave / Música
                        let is_music = cur_preset == "Suave / Música";
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Suave / Música")
                                        .size(11.0)
                                        .color(if is_music {
                                            Color32::from_rgb(255, 251, 235)
                                        } else {
                                            Color32::from_rgb(209, 213, 219)
                                        }),
                                )
                                .fill(if is_music {
                                    Color32::from_rgb(180, 83, 9)
                                } else {
                                    Color32::from_rgb(32, 36, 48)
                                }),
                            )
                            .on_hover_text("Atenuación musical sutil: ratio 2.5:1, ataque 10ms, piso -9dB.")
                            .clicked()
                        {
                            self.config.compressor = CompressorConfig::preset_music_subtle();
                            self.mark_dirty();
                        }

                        // 4. OBS Clásico
                        let is_obs = cur_preset == "OBS Clásico";
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("OBS Clásico")
                                        .size(11.0)
                                        .color(if is_obs {
                                            Color32::from_rgb(255, 251, 235)
                                        } else {
                                            Color32::from_rgb(209, 213, 219)
                                        }),
                                )
                                .fill(if is_obs {
                                    Color32::from_rgb(180, 83, 9)
                                } else {
                                    Color32::from_rgb(32, 36, 48)
                                }),
                            )
                            .on_hover_text("Valores originales de tu escena de OBS: 5:1, -50dB, 1ms ataque, 300ms release.")
                            .clicked()
                        {
                            self.config.compressor = CompressorConfig::preset_obs_classic();
                            self.mark_dirty();
                        }
                    });

                    ui.add_space(4.0);

                    let preset_desc = match self.config.compressor.preset_name.as_str() {
                        "Gaming Pro" => "Modo Gaming Pro activo: Umbral -38 dB, Retención 85 ms (cero bombeo), Piso en -15 dB.",
                        "Competitivo" => "Modo Competitivo activo: Umbral -42 dB, Ratio 6:1, Retención 110 ms, Piso en -22 dB.",
                        "Suave / Música" => "Modo Suave activo: Umbral -34 dB, Ratio 2.5:1, Piso en -9 dB.",
                        _ => "Modo OBS Clásico: 5:1, -50 dB, 1 ms ataque, 300 ms release, sin límite de piso.",
                    };
                    ui.label(
                        RichText::new(preset_desc)
                            .size(10.5)
                            .color(Color32::from_rgb(251, 191, 36)),
                    );

                    ui.add_space(6.0);

                    egui::Grid::new("comp_studio_grid")
                        .num_columns(4)
                        .spacing([18.0, 6.0])
                        .show(ui, |ui| {
                            ui.label("Umbral (Threshold):");
                            ui.spacing_mut().slider_width = 180.0;
                            if ui
                                .add(
                                    egui::Slider::new(
                                        &mut self.config.compressor.threshold_db,
                                        -60.0..=0.0,
                                    )
                                    .suffix(" dB")
                                    .step_by(0.5),
                                )
                                .changed()
                            {
                                self.config.compressor.preset_name = "Personalizado".into();
                                self.mark_dirty();
                            }

                            ui.label("Relación (Ratio):");
                            if ui
                                .add(
                                    egui::Slider::new(&mut self.config.compressor.ratio, 1.0..=20.0)
                                        .suffix(" : 1")
                                        .step_by(0.5),
                                )
                                .changed()
                            {
                                self.config.compressor.preset_name = "Personalizado".into();
                                self.mark_dirty();
                            }
                            ui.end_row();

                            ui.label("Retención (Hold):");
                            if ui
                                .add(
                                    egui::Slider::new(
                                        &mut self.config.compressor.hold_ms,
                                        0.0..=500.0,
                                    )
                                    .suffix(" ms")
                                    .step_by(5.0),
                                )
                                .changed()
                            {
                                self.config.compressor.preset_name = "Personalizado".into();
                                self.mark_dirty();
                            }

                            ui.label("Atenuación Máx. (Piso):");
                            if ui
                                .add(
                                    egui::Slider::new(
                                        &mut self.config.compressor.max_reduction_db,
                                        -40.0..=-3.0,
                                    )
                                    .suffix(" dB")
                                    .step_by(0.5),
                                )
                                .changed()
                            {
                                self.config.compressor.preset_name = "Personalizado".into();
                                self.mark_dirty();
                            }
                            ui.end_row();

                            ui.label("Ataque (Attack):");
                            if ui
                                .add(
                                    egui::Slider::new(
                                        &mut self.config.compressor.attack_ms,
                                        0.5..=100.0,
                                    )
                                    .suffix(" ms")
                                    .step_by(0.5),
                                )
                                .changed()
                            {
                                self.config.compressor.preset_name = "Personalizado".into();
                                self.mark_dirty();
                            }

                            ui.label("Liberación (Release):");
                            if ui
                                .add(
                                    egui::Slider::new(
                                        &mut self.config.compressor.release_ms,
                                        10.0..=1000.0,
                                    )
                                    .suffix(" ms")
                                    .step_by(10.0),
                                )
                                .changed()
                            {
                                self.config.compressor.preset_name = "Personalizado".into();
                                self.mark_dirty();
                            }
                            ui.end_row();

                            ui.label("Ganancia de salida:");
                            if ui
                                .add(
                                    egui::Slider::new(
                                        &mut self.config.compressor.output_gain_db,
                                        -20.0..=20.0,
                                    )
                                    .suffix(" dB")
                                    .step_by(0.5),
                                )
                                .changed()
                            {
                                self.config.compressor.preset_name = "Personalizado".into();
                                self.mark_dirty();
                            }
                            ui.end_row();
                        });

                    ui.add_space(6.0);
                    ui.separator();
                    ui.add_space(6.0);

                    // DSP Avanzado: Ducking Espectral y De-Esser / Gate
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("PROCESAMIENTO INTELIGENTE AVANZADO:")
                                .monospace()
                                .size(10.5)
                                .color(Color32::from_rgb(148, 163, 184)),
                        );
                    });

                    ui.horizontal(|ui| {
                        if ui
                            .checkbox(
                                &mut self.config.compressor.spectral_ducking,
                                RichText::new("Ducking Espectral Inteligente (300 Hz - 4 kHz)").size(11.0),
                            )
                            .on_hover_text("Abre un hueco transparente en medios donde reside la voz humana sin apagar el impacto de graves ni los agudos del juego.")
                            .changed()
                        {
                            self.mark_dirty();
                        }

                        ui.add_space(12.0);

                        if ui
                            .checkbox(
                                &mut self.config.compressor.voice_sc_filter,
                                RichText::new("Detector Sidechain Anti-Golpes (200 Hz - 3.8 kHz)").size(11.0),
                            )
                            .on_hover_text("Aísla la voz antes de medir volumen para que golpes a la mesa o clics no bajen el juego por error.")
                            .changed()
                        {
                            self.mark_dirty();
                        }
                    });

                    ui.horizontal(|ui| {
                        if ui
                            .checkbox(
                                &mut self.config.deesser_enabled,
                                RichText::new("De-Esser Dinámico en Voz Clara (6.2 kHz)").size(11.0),
                            )
                            .on_hover_text("Comprime selectivamente las frecuencias agudas molestas ('S'/'CH').")
                            .changed()
                        {
                            self.mark_dirty();
                        }

                        ui.add_space(12.0);

                        if ui
                            .checkbox(
                                &mut self.config.noise_gate_enabled,
                                RichText::new("Compuerta Anti-Siseo (-14 dB)").size(11.0),
                            )
                            .on_hover_text("Atenúa el ruido de fondo, ventiladores o siseo en pausas.")
                            .changed()
                        {
                            self.mark_dirty();
                        }
                    });

                    ui.add_space(4.0);

                    // Fuerza de Bajada General y Atenuación Modo Enfoque
                    ui.horizontal(|ui| {
                        ui.label("Fuerza de bajada general (Espectral):");
                        let mut pct = self.config.compressor.spectral_fullband_ratio * 100.0;
                        if ui
                            .add(
                                egui::Slider::new(&mut pct, 40.0..=100.0)
                                    .suffix(" %")
                                    .step_by(5.0),
                            )
                            .on_hover_text("Determina cuánto baja el resto del espectro (graves y agudos) cuando hablan en Discord. Por defecto 75%.")
                            .changed()
                        {
                            self.config.compressor.spectral_fullband_ratio = pct / 100.0;
                            self.mark_dirty();
                        }

                        ui.add_space(16.0);

                        ui.label("Atenuación Modo Enfoque (Prio 2):");
                        if ui
                            .add(
                                egui::Slider::new(&mut self.config.focus_duck_db, -30.0..=-4.0)
                                    .suffix(" dB")
                                    .step_by(1.0),
                            )
                            .on_hover_text("Nivel de reducción aplicado a canales normales cuando suena una app de Prioridad 2.")
                            .changed()
                        {
                            self.mark_dirty();
                        }
                    });

                    ui.add_space(4.0);

                    // Minimizar a la bandeja
                    ui.horizontal(|ui| {
                        if ui
                            .checkbox(
                                &mut self.config.close_to_tray,
                                RichText::new("Minimizar a la bandeja del sistema al cerrar (X)").size(11.0),
                            )
                            .on_hover_text("Al hacer clic en la X de la ventana, la aplicación se oculta en la bandeja junto al reloj.")
                            .changed()
                        {
                            self.mark_dirty();
                        }
                    });
                });
        }

        // ==================== CENTRAL PANEL: CHANNEL MIXER ====================
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(14, 15, 20))
                    .inner_margin(egui::Margin::symmetric(14, 10)),
            )
            .show(ui, |ui| {
                let avail_w = ui.available_width();

                // Define exact column geometry for pixel-perfect mathematical alignment
                let (col1_w, col3_w, gap, row_h) = if self.flyout_mode {
                    (130.0, 128.0, 8.0, 44.0)
                } else {
                    (220.0, 396.0, 12.0, 56.0)
                };

                let col1_x = 0.0;
                let col3_x = avail_w - col3_w;
                let col2_x = col1_x + col1_w + gap;
                let col2_w = (col3_x - gap - col2_x).max(40.0);

                // Table header column labels (aligned with 0.0 px deviation to cards below)
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(col1_w, 16.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.label(
                                RichText::new(if self.flyout_mode { "APLICACIÓN" } else { "FUENTE / APLICACIÓN" })
                                    .monospace()
                                    .size(9.5)
                                    .color(Color32::from_rgb(110, 118, 138)),
                            );
                        },
                    );

                    ui.add_space(gap);

                    ui.allocate_ui_with_layout(
                        Vec2::new(col2_w, 16.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.label(
                                RichText::new(if self.flyout_mode { "VOLUMEN / FADER" } else { "NIVEL DE ENTRADA & FADER CÚBICO (OBS)" })
                                    .monospace()
                                    .size(9.5)
                                    .color(Color32::from_rgb(110, 118, 138)),
                            );
                        },
                    );

                    ui.add_space(gap);

                    ui.allocate_ui_with_layout(
                        Vec2::new(col3_w, 16.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.label(
                                RichText::new(if self.flyout_mode { "ESTADO / MUTE" } else { "CONTROLES DE CANAL" })
                                    .monospace()
                                    .size(9.5)
                                    .color(Color32::from_rgb(110, 118, 138)),
                            );
                        },
                    );
                });

                ui.add_space(4.0);

                // Channel ScrollArea filling 100% of remaining vertical room
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut channel_to_remove: Option<usize> = None;
                        let mut channels_changed = false;

                        let filter_query = self.search_filter_channels.trim().to_lowercase();
                        let show_only_active = self.config.show_only_active;

                        for (idx, ch) in self.config.channels.iter_mut().enumerate() {
                            let status = engine_state
                                .channel_status
                                .get(&ch.id)
                                .cloned()
                                .unwrap_or_default();

                            if show_only_active && !status.active {
                                continue;
                            }

                            if !filter_query.is_empty() {
                                let match_name = ch.name.to_lowercase().contains(&filter_query);
                                let match_exe = ch.exe_name.to_lowercase().contains(&filter_query);
                                if !match_name && !match_exe {
                                    continue;
                                }
                            }

                            // Pre-allocate exact bounding box of channel row for zero-deviation alignment
                            let (row_rect, _) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), row_h),
                                Sense::hover(),
                            );

                            let card_bg = if status.active {
                                Color32::from_rgb(23, 25, 33)
                            } else {
                                Color32::from_rgb(18, 19, 25)
                            };

                            let card_stroke = if status.active {
                                Color32::from_rgb(42, 48, 65)
                            } else {
                                Color32::from_rgb(30, 32, 42)
                            };

                            // Draw rounded container card
                            ui.painter().rect_filled(row_rect, CornerRadius::same(6), card_bg);
                            ui.painter().rect_stroke(
                                row_rect,
                                CornerRadius::same(6),
                                Stroke::new(1.0, card_stroke),
                                StrokeKind::Inside,
                            );

                            // Inner padded area
                            let pad_h = if self.flyout_mode { 8.0 } else { 10.0 };
                            let pad_v = if self.flyout_mode { 4.0 } else { 5.0 };
                            let inner = row_rect.shrink2(Vec2::new(pad_h, pad_v));

                            // Mathematical Sub-Rectangles (0.0 px variance)
                            let col1_rect = Rect::from_min_max(
                                inner.min,
                                pos2(inner.min.x + col1_w, inner.max.y),
                            );
                            let col3_rect = Rect::from_min_max(
                                pos2(inner.max.x - col3_w, inner.min.y),
                                inner.max,
                            );
                            let col2_rect = Rect::from_min_max(
                                pos2(col1_rect.max.x + gap, inner.min.y),
                                pos2(col3_rect.min.x - gap, inner.max.y),
                            );

                            // ==========================================
                            // CELL 1: IDENTITY & STATUS
                            // ==========================================
                            ui.scope_builder(egui::UiBuilder::new().max_rect(col1_rect), |ui| {
                                ui.set_clip_rect(col1_rect);
                                ui.vertical(|ui| {
                                    if self.flyout_mode {
                                        // Compact Flyout Identity
                                        let display_title = if ch.exe_name == "__hardware_input__" {
                                            format!("{} [Hardware]", ch.name)
                                        } else {
                                            ch.name.clone()
                                        };
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(display_title)
                                                    .strong()
                                                    .size(11.5)
                                                    .color(if status.active {
                                                        Color32::from_rgb(243, 244, 246)
                                                    } else {
                                                        Color32::from_rgb(156, 163, 175)
                                                    }),
                                            )
                                            .truncate(),
                                        );

                                        ui.horizontal(|ui| {
                                            draw_led_dot(
                                                ui.painter(),
                                                pos2(col1_rect.min.x + 5.0, ui.cursor().center().y),
                                                status.active,
                                            );
                                            ui.add_space(10.0);
                                            ui.label(
                                                RichText::new(if status.active { "ACTIVO" } else { "INACTIVO" })
                                                    .monospace()
                                                    .size(9.0)
                                                    .color(if status.active {
                                                        Color32::from_rgb(16, 185, 129)
                                                    } else {
                                                        Color32::from_rgb(107, 114, 128)
                                                    }),
                                            );
                                        });
                                    } else {
                                        // Studio Identity (2 Clean Lines: [LED + Title] on top, [Status + Exe/Device] on bottom)
                                        ui.spacing_mut().item_spacing.y = 4.0;
                                        ui.add_space(2.0);

                                        ui.horizontal(|ui| {
                                            draw_led_dot(
                                                ui.painter(),
                                                pos2(col1_rect.min.x + 6.0, ui.cursor().min.y + 8.0),
                                                status.active,
                                            );
                                            ui.add_space(14.0);

                                            let display_title = if ch.exe_name == "__hardware_input__" {
                                                format!("{} [Hardware]", ch.name)
                                            } else {
                                                ch.name.clone()
                                            };
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(display_title)
                                                        .strong()
                                                        .size(13.0)
                                                        .color(if status.active {
                                                            Color32::from_rgb(243, 244, 246)
                                                        } else {
                                                            Color32::from_rgb(175, 182, 195)
                                                        }),
                                                )
                                                .truncate(),
                                            );
                                        });

                                        ui.horizontal(|ui| {
                                            let (st_text, st_color) = if status.active {
                                                ("EN EJECUCIÓN", Color32::from_rgb(16, 185, 129))
                                            } else {
                                                ("INACTIVO", Color32::from_rgb(107, 114, 128))
                                            };
                                            ui.label(
                                                RichText::new(st_text)
                                                    .monospace()
                                                    .strong()
                                                    .size(9.5)
                                                    .color(st_color),
                                            );

                                            let sub_text = if ch.exe_name == "__hardware_input__" {
                                                "· Entrada física USB".to_string()
                                            } else {
                                                format!("· {}", ch.exe_name)
                                            };
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(sub_text)
                                                        .monospace()
                                                        .size(9.5)
                                                        .color(Color32::from_rgb(130, 138, 158)),
                                                )
                                                .truncate(),
                                            );
                                        });
                                    }
                                });
                            });

                            // ==========================================
                            // CELL 2: VU METER & CUBIC FADER
                            // ==========================================
                            ui.scope_builder(egui::UiBuilder::new().max_rect(col2_rect), |ui| {
                                ui.set_clip_rect(col2_rect);
                                ui.vertical(|ui| {
                                    let c2_w = col2_rect.width();
                                    let pill_w = if self.flyout_mode { 46.0 } else { 104.0 };
                                    let slider_w = (c2_w - pill_w - 8.0).max(40.0);

                                    // Studio Segmented VU Meter with Ducking overlay
                                    let duck_amount = if ch.is_trigger {
                                        0.0
                                    } else if ch.is_priority2 {
                                        if ch.use_compressor {
                                            engine_state.sidechain_reduction_db
                                        } else {
                                            0.0
                                        }
                                    } else if ch.use_compressor {
                                        (engine_state.sidechain_reduction_db + engine_state.focus_reduction_db).max(-45.0)
                                    } else {
                                        0.0
                                    };

                                    let meter_h = if self.flyout_mode { 6.0 } else { 8.0 };
                                    studio_segmented_vu_meter(
                                        ui,
                                        status.vu_peak_db,
                                        duck_amount,
                                        slider_w,
                                        meter_h,
                                    );

                                    if !self.flyout_mode {
                                        vu_scale_labels(ui, slider_w);
                                        ui.add_space(1.0);
                                    }

                                    // Slider & Monospace Readout Pill
                                    ui.horizontal(|ui| {
                                        let mut slider_val = volume_to_slider(ch.volume);
                                        ui.spacing_mut().slider_width = slider_w - 6.0;
                                        let slider_resp = ui.add(
                                            egui::Slider::new(&mut slider_val, 0.0..=1.0)
                                                .show_value(false),
                                        );

                                        if slider_resp.changed() {
                                            ch.volume = slider_to_volume(slider_val);
                                            channels_changed = true;
                                        }
                                        if slider_resp.double_clicked() {
                                            ch.volume = 1.0;
                                            channels_changed = true;
                                        }

                                        let vol_pct = ch.volume * 100.0;
                                        let vol_db = volume_to_db(ch.volume);
                                        let vol_pill_text = if self.flyout_mode {
                                            format!("{:3.0}%", vol_pct)
                                        } else {
                                            format!("{:4.0}% | {:4.1}dB", vol_pct, vol_db)
                                        };

                                        let (pill_rect, _) = ui.allocate_exact_size(
                                            Vec2::new(pill_w, if self.flyout_mode { 16.0 } else { 18.0 }),
                                            Sense::hover(),
                                        );
                                        ui.painter().rect_filled(
                                            pill_rect,
                                            CornerRadius::same(3),
                                            Color32::from_rgb(16, 18, 24),
                                        );
                                        ui.painter().rect_stroke(
                                            pill_rect,
                                            CornerRadius::same(3),
                                            Stroke::new(1.0, Color32::from_rgb(38, 42, 56)),
                                            StrokeKind::Inside,
                                        );
                                        ui.painter().text(
                                            pill_rect.center(),
                                            egui::Align2::CENTER_CENTER,
                                            vol_pill_text,
                                            egui::FontId::monospace(if self.flyout_mode { 9.0 } else { 9.5 }),
                                            Color32::from_rgb(209, 213, 219),
                                        );
                                    });
                                });
                            });

                            // ==========================================
                            // CELL 3: CHANNEL CONTROLS
                            // ==========================================
                            ui.scope_builder(egui::UiBuilder::new().max_rect(col3_rect), |ui| {
                                ui.set_clip_rect(col3_rect);
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if self.flyout_mode {
                                        // Compact Flyout: Remove + Quick Role + Mute
                                        if remove_channel_button(ui).clicked() {
                                            channel_to_remove = Some(idx);
                                        }

                                        let (role_text, role_color) = if ch.is_trigger {
                                            ("SC", PillColor::Cyan)
                                        } else if ch.is_priority2 {
                                            ("P2", PillColor::Gold)
                                        } else if ch.use_compressor {
                                            ("DUCK", PillColor::Amber)
                                        } else if ch.exempt_master {
                                            ("FIJO", PillColor::Purple)
                                        } else {
                                            ("NORM", PillColor::Emerald)
                                        };

                                        if pill_toggle_button(
                                            ui,
                                            ch.use_compressor || ch.is_trigger || ch.is_priority2 || ch.exempt_master,
                                            role_text,
                                            46.0,
                                            role_color,
                                            "Alternar atenuación sidechain",
                                        ) {
                                            if ch.is_trigger {
                                                ch.is_trigger = false;
                                                ch.use_compressor = true;
                                            } else if ch.use_compressor {
                                                ch.use_compressor = false;
                                            } else {
                                                ch.use_compressor = true;
                                            }
                                            channels_changed = true;
                                        }

                                        if vector_mute_button(ui, ch.muted).clicked() {
                                            ch.muted = !ch.muted;
                                            channels_changed = true;
                                        }
                                    } else {
                                        // Full Studio Mode: All 7 Controls on Single Aligned Row (376px total inside 396px cell)
                                        if remove_channel_button(ui).clicked() {
                                            channel_to_remove = Some(idx);
                                        }

                                        if pill_toggle_button(
                                            ui,
                                            ch.exempt_master,
                                            "FIJO",
                                            46.0,
                                            PillColor::Purple,
                                            "Volumen Fijo: No es alterado cuando mueves el Volumen Maestro general.",
                                        ) {
                                            ch.exempt_master = !ch.exempt_master;
                                            channels_changed = true;
                                        }

                                        if pill_toggle_button(
                                            ui,
                                            ch.use_compressor,
                                            "COMPRESOR",
                                            72.0,
                                            PillColor::Amber,
                                            "Compresor Sidechain: Baja su volumen automáticamente cuando suena Discord o apps de Prioridad 2.",
                                        ) {
                                            ch.use_compressor = !ch.use_compressor;
                                            channels_changed = true;
                                        }

                                        if pill_toggle_button(
                                            ui,
                                            ch.is_priority2,
                                            "PRIO 2",
                                            52.0,
                                            PillColor::Gold,
                                            "Prioridad 2 (Modo Enfoque): Cuando esta app emite sonido, baja automáticamente las demás apps normales.",
                                        ) {
                                            ch.is_priority2 = !ch.is_priority2;
                                            channels_changed = true;
                                        }

                                        if pill_toggle_button(
                                            ui,
                                            ch.is_trigger,
                                            "SIDECHAIN",
                                            70.0,
                                            PillColor::Cyan,
                                            "Disparador Sidechain (Voz de mando): Cuando este canal produce sonido, atenúa tanto canales normales como canales de Prioridad 2.",
                                        ) {
                                            ch.is_trigger = !ch.is_trigger;
                                            channels_changed = true;
                                        }

                                        if pill_toggle_button(
                                            ui,
                                            ch.voice_clarity,
                                            "VOZ",
                                            44.0,
                                            PillColor::Emerald,
                                            "Claridad Vocal DSP: Activa el ecualizador y nivelador vocal DSP para este canal.",
                                        ) {
                                            ch.voice_clarity = !ch.voice_clarity;
                                            channels_changed = true;
                                        }

                                        if vector_mute_button(ui, ch.muted).clicked() {
                                            ch.muted = !ch.muted;
                                            channels_changed = true;
                                        }
                                    }
                                });
                            });

                            ui.add_space(4.0);
                        }

                        if channels_changed {
                            self.mark_dirty();
                        }

                        if let Some(idx) = channel_to_remove {
                            self.config.channels.remove(idx);
                            self.mark_dirty();
                        }
                    });
            });
    }
}
