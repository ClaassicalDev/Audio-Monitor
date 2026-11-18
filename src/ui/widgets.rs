use eframe::egui::{
    self, Color32, CornerRadius, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2,
};

#[derive(Debug, Clone, Copy)]
pub enum PillColor {
    Cyan,
    Amber,
    Purple,
    Emerald,
    Gold,
}

/// Draws an 8px circular glowing LED indicator
pub fn draw_led_dot(painter: &egui::Painter, center: Pos2, active: bool) {
    if active {
        // Soft outer glow
        painter.circle_filled(
            center,
            5.5,
            Color32::from_rgba_unmultiplied(16, 185, 129, 65),
        );
        // Primary LED
        painter.circle_filled(center, 3.5, Color32::from_rgb(16, 185, 129));
        // Bright specular core
        painter.circle_filled(center, 1.2, Color32::from_rgb(209, 250, 229));
    } else {
        // Inactive dark LED
        painter.circle_filled(center, 3.5, Color32::from_rgb(45, 52, 65));
        painter.circle_stroke(
            center,
            3.5,
            Stroke::new(1.0, Color32::from_rgb(60, 68, 85)),
        );
    }
}

/// Channel Status Badge: LED + "EN EJECUCIÓN" / "INACTIVO" (No PIDs!)
pub fn channel_status_badge(ui: &mut Ui, active: bool) {
    let (rect, _resp) = ui.allocate_exact_size(Vec2::new(104.0, 18.0), Sense::hover());
    let painter = ui.painter();

    let led_pos = Pos2::new(rect.min.x + 6.0, rect.center().y);
    draw_led_dot(painter, led_pos, active);

    let (text, color) = if active {
        ("EN EJECUCIÓN", Color32::from_rgb(16, 185, 129))
    } else {
        ("INACTIVO", Color32::from_rgb(107, 114, 128))
    };

    painter.text(
        Pos2::new(rect.min.x + 16.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::monospace(10.5),
        color,
    );
}

/// Draws a crisp vector speaker / mute icon inside a given rect
pub fn draw_speaker_icon(painter: &egui::Painter, rect: Rect, muted: bool, color: Color32) {
    let w = rect.width();
    let h = rect.height();
    let cx = rect.center().x;
    let cy = rect.center().y;

    // Speaker body (back rectangle)
    let body_w = w * 0.18;
    let body_h = h * 0.32;
    let body_x = cx - w * 0.35;
    let body_y = cy - body_h * 0.5;

    let body_rect = Rect::from_min_size(Pos2::new(body_x, body_y), Vec2::new(body_w, body_h));
    painter.rect_filled(body_rect, CornerRadius::same(1), color);

    // Flared cone (trapezoid polygon)
    let cone_left = body_x + body_w;
    let cone_right = cx - w * 0.05;
    let cone_h = h * 0.65;

    let p1 = Pos2::new(cone_left, cy - body_h * 0.5);
    let p2 = Pos2::new(cone_right, cy - cone_h * 0.5);
    let p3 = Pos2::new(cone_right, cy + cone_h * 0.5);
    let p4 = Pos2::new(cone_left, cy + body_h * 0.5);

    painter.add(egui::Shape::convex_polygon(
        vec![p1, p2, p3, p4],
        color,
        Stroke::NONE,
    ));

    if muted {
        // Red 'X' mark
        let x_stroke = Stroke::new(2.0, Color32::from_rgb(239, 68, 68));
        let x_size = w * 0.22;
        let x_center = cx + w * 0.22;

        painter.line_segment(
            [
                Pos2::new(x_center - x_size, cy - x_size),
                Pos2::new(x_center + x_size, cy + x_size),
            ],
            x_stroke,
        );
        painter.line_segment(
            [
                Pos2::new(x_center - x_size, cy + x_size),
                Pos2::new(x_center + x_size, cy - x_size),
            ],
            x_stroke,
        );
    } else {
        // Sound waves (2 concentric arc segments)
        let wave_stroke = Stroke::new(1.8, color);

        // Small wave
        let r1 = w * 0.18;
        let arc1_x = cx + w * 0.08;
        painter.line_segment(
            [
                Pos2::new(arc1_x, cy - r1),
                Pos2::new(arc1_x + r1 * 0.6, cy),
            ],
            wave_stroke,
        );
        painter.line_segment(
            [
                Pos2::new(arc1_x + r1 * 0.6, cy),
                Pos2::new(arc1_x, cy + r1),
            ],
            wave_stroke,
        );

        // Larger wave
        let r2 = w * 0.32;
        let arc2_x = cx + w * 0.16;
        painter.line_segment(
            [
                Pos2::new(arc2_x, cy - r2),
                Pos2::new(arc2_x + r2 * 0.6, cy),
            ],
            wave_stroke,
        );
        painter.line_segment(
            [
                Pos2::new(arc2_x + r2 * 0.6, cy),
                Pos2::new(arc2_x, cy + r2),
            ],
            wave_stroke,
        );
    }
}

/// Mute button with custom vector speaker icon
pub fn vector_mute_button(ui: &mut Ui, muted: bool) -> Response {
    let size = Vec2::new(34.0, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    let painter = ui.painter();

    let (bg_color, stroke_color, icon_color) = if muted {
        (
            Color32::from_rgb(127, 29, 29), // Crimson red
            Color32::from_rgb(239, 68, 68),
            Color32::from_rgb(254, 202, 202),
        )
    } else if response.hovered() {
        (
            Color32::from_rgb(45, 50, 65),
            Color32::from_rgb(75, 85, 110),
            Color32::from_rgb(243, 244, 246),
        )
    } else {
        (
            Color32::from_rgb(28, 31, 41),
            Color32::from_rgb(48, 54, 70),
            Color32::from_rgb(180, 187, 204),
        )
    };

    painter.rect_filled(rect, CornerRadius::same(5), bg_color);
    painter.rect_stroke(
        rect,
        CornerRadius::same(5),
        Stroke::new(1.0, stroke_color),
        StrokeKind::Inside,
    );

    let icon_rect = Rect::from_center_size(rect.center(), Vec2::new(18.0, 18.0));
    draw_speaker_icon(painter, icon_rect, muted, icon_color);

    response.on_hover_text(if muted {
        "Reactivar audio (Desmutear)"
    } else {
        "Silenciar canal (Mute)"
    })
}

/// Studio Segmented LED VU Meter (OBS / Wave Link Dark Console Style)
/// - Segmented LED bars
/// - Green: -60 dB to -12 dB
/// - Amber: -12 dB to -3 dB
/// - Red: -3 dB to 0 dB
/// - Optional sidechain ducking overlay bar in orange
pub fn studio_segmented_vu_meter(
    ui: &mut Ui,
    peak_db: f32,
    ducking_db: f32,
    width: f32,
    height: f32,
) {
    let (rect, _response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();

    // Dark meter background track
    painter.rect_filled(rect, CornerRadius::same(3), Color32::from_rgb(14, 16, 22));
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.0, Color32::from_rgb(36, 40, 54)),
        StrokeKind::Inside,
    );

    let num_segments = 28;
    let seg_spacing = 1.5;
    let total_spacing = seg_spacing * (num_segments - 1) as f32;
    let inner_width = (rect.width() - 4.0 - total_spacing).max(10.0);
    let seg_width = inner_width / num_segments as f32;
    let seg_height = rect.height() - 4.0;
    let seg_y = rect.min.y + 2.0;

    // Convert peak dB [-60.0, 0.0] -> fraction [0.0, 1.0]
    let peak_fraction = ((peak_db + 60.0) / 60.0).clamp(0.0, 1.0);
    let active_segments = (peak_fraction * num_segments as f32).round() as usize;

    for i in 0..num_segments {
        let seg_x = rect.min.x + 2.0 + i as f32 * (seg_width + seg_spacing);
        let seg_rect = Rect::from_min_size(Pos2::new(seg_x, seg_y), Vec2::new(seg_width, seg_height));

        // Thresholds in fraction:
        // -12 dB is fraction (-12 + 60) / 60 = 48/60 = 0.80
        // -3 dB is fraction (-3 + 60) / 60 = 57/60 = 0.95
        let seg_frac = (i as f32 + 0.5) / num_segments as f32;

        let is_lit = i < active_segments;

        let seg_color = if is_lit {
            if seg_frac >= 0.95 {
                Color32::from_rgb(239, 68, 68) // Red (clipping/hot)
            } else if seg_frac >= 0.80 {
                Color32::from_rgb(245, 158, 11) // Amber
            } else {
                Color32::from_rgb(16, 185, 129) // Green (optimal)
            }
        } else {
            // Unlit dark segment
            if seg_frac >= 0.95 {
                Color32::from_rgb(45, 20, 20)
            } else if seg_frac >= 0.80 {
                Color32::from_rgb(40, 32, 18)
            } else {
                Color32::from_rgb(20, 32, 26)
            }
        };

        painter.rect_filled(seg_rect, CornerRadius::same(1), seg_color);
    }

    // Peak tick indicator
    if peak_fraction > 0.02 {
        let tick_x = (rect.min.x + 2.0 + (rect.width() - 4.0) * peak_fraction).min(rect.max.x - 2.0);
        let tick_color = if peak_db > -3.0 {
            Color32::from_rgb(255, 200, 200)
        } else if peak_db > -12.0 {
            Color32::from_rgb(255, 240, 180)
        } else {
            Color32::from_rgb(200, 255, 220)
        };
        painter.line_segment(
            [Pos2::new(tick_x, seg_y), Pos2::new(tick_x, seg_y + seg_height)],
            Stroke::new(1.8, tick_color),
        );
    }

    // Sidechain Ducking Overlay:
    // If ducking_db is below -0.2 dB, show ducking reduction bar from the right
    if ducking_db < -0.2 {
        let duck_fraction = (ducking_db.abs() / 30.0).clamp(0.0, 0.75);
        let duck_width = (rect.width() - 4.0) * duck_fraction;
        let duck_rect = Rect::from_min_size(
            Pos2::new(rect.max.x - 2.0 - duck_width, seg_y),
            Vec2::new(duck_width, seg_height),
        );
        painter.rect_filled(
            duck_rect,
            CornerRadius::same(1),
            Color32::from_rgba_unmultiplied(249, 115, 22, 170), // Translucent Orange
        );
    }
}

/// Scale markings for VU meters (-60, -30, -18, -12, -6, 0 dB)
pub fn vu_scale_labels(ui: &mut Ui, width: f32) {
    let (rect, _resp) = ui.allocate_exact_size(Vec2::new(width, 10.0), Sense::hover());
    let painter = ui.painter();

    let points: &[(f32, &str)] = &[
        (-60.0, "-60"),
        (-36.0, "-36"),
        (-24.0, "-24"),
        (-12.0, "-12"),
        (-6.0, "-6"),
        (0.0, "0 dB"),
    ];

    let font = egui::FontId::monospace(8.5);
    let color = Color32::from_rgb(110, 118, 138);

    for &(db, label) in points {
        let fraction = ((db + 60.0) / 60.0).clamp(0.0f32, 1.0f32);
        let x = rect.min.x + fraction * rect.width();
        let align = if fraction < 0.05 {
            egui::Align2::LEFT_TOP
        } else if fraction > 0.95 {
            egui::Align2::RIGHT_TOP
        } else {
            egui::Align2::CENTER_TOP
        };
        painter.text(Pos2::new(x, rect.min.y), align, label, font.clone(), color);
    }
}

/// Sidechain Live Gain Reduction Meter (-0.0 to -30.0 dB)
pub fn gain_reduction_meter(ui: &mut Ui, reduction_db: f32, width: f32, height: f32) {
    let (rect, _response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let painter = ui.painter();

    painter.rect_filled(rect, CornerRadius::same(3), Color32::from_rgb(14, 16, 22));
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.0, Color32::from_rgb(36, 40, 54)),
        StrokeKind::Inside,
    );

    let abs_reduction = reduction_db.abs();
    let fraction = (abs_reduction / 30.0).clamp(0.0, 1.0);

    if fraction > 0.01 {
        let active_width = (rect.width() - 4.0) * fraction;
        let active_rect = Rect::from_min_size(
            Pos2::new(rect.min.x + 2.0, rect.min.y + 2.0),
            Vec2::new(active_width, rect.height() - 4.0),
        );

        let color = if abs_reduction > 15.0 {
            Color32::from_rgb(239, 68, 68) // Red
        } else if abs_reduction > 6.0 {
            Color32::from_rgb(249, 115, 22) // Orange
        } else {
            Color32::from_rgb(245, 158, 11) // Amber
        };

        painter.rect_filled(active_rect, CornerRadius::same(2), color);
    }
}

/// Custom Pill Toggle Button (VOZ, SIDECHAIN, COMPRESOR, FIJO)
pub fn pill_toggle_button(
    ui: &mut Ui,
    active: bool,
    text: &str,
    width: f32,
    color_type: PillColor,
    tooltip: &str,
) -> bool {
    let size = Vec2::new(width, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();

    let (bg, stroke, text_color) = if active {
        match color_type {
            PillColor::Cyan => (
                Color32::from_rgb(14, 116, 144),  // Deep Cyan
                Color32::from_rgb(56, 189, 248),  // Bright Cyan border
                Color32::from_rgb(240, 249, 255), // Crisp White
            ),
            PillColor::Amber => (
                Color32::from_rgb(180, 83, 9),    // Amber
                Color32::from_rgb(251, 191, 36),  // Bright Amber border
                Color32::from_rgb(255, 251, 235), // Crisp White
            ),
            PillColor::Purple => (
                Color32::from_rgb(109, 40, 217),  // Purple
                Color32::from_rgb(167, 139, 250), // Bright Violet border
                Color32::from_rgb(250, 245, 255), // Crisp White
            ),
            PillColor::Emerald => (
                Color32::from_rgb(5, 150, 105),   // Rich Emerald
                Color32::from_rgb(52, 211, 153),  // Bright Emerald border
                Color32::from_rgb(236, 253, 245), // Crisp Mint/White
            ),
            PillColor::Gold => (
                Color32::from_rgb(180, 110, 10),  // Deep Gold
                Color32::from_rgb(250, 204, 21),  // Bright Yellow/Gold border
                Color32::from_rgb(254, 252, 232), // Warm Golden White
            ),
        }
    } else if response.hovered() {
        (
            Color32::from_rgb(38, 42, 54),
            Color32::from_rgb(60, 68, 88),
            Color32::from_rgb(209, 213, 219),
        )
    } else {
        (
            Color32::from_rgb(24, 27, 36),
            Color32::from_rgb(42, 47, 62),
            Color32::from_rgb(130, 138, 158),
        )
    };

    painter.rect_filled(rect, CornerRadius::same(4), bg);
    painter.rect_stroke(
        rect,
        CornerRadius::same(4),
        Stroke::new(1.0, stroke),
        StrokeKind::Inside,
    );

    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(11.0),
        text_color,
    );

    response.on_hover_text(tooltip).clicked()
}

/// Subtle channel remove button (cross '✕')
pub fn remove_channel_button(ui: &mut Ui) -> Response {
    let size = Vec2::new(22.0, 22.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();

    let (bg, text_color) = if response.hovered() {
        (Color32::from_rgb(185, 28, 28), Color32::WHITE)
    } else {
        (Color32::from_rgb(28, 31, 41), Color32::from_rgb(130, 138, 155))
    };

    painter.rect_filled(rect, CornerRadius::same(3), bg);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "✕",
        egui::FontId::proportional(12.0),
        text_color,
    );

    response.on_hover_text("Quitar este canal de la lista")
}
