use eframe::egui::{self, Color32, RichText, Stroke, TextureHandle, Vec2};
use std::{fs, sync::Arc};

#[cfg(target_os = "windows")]
pub fn apply_native_window_theme(cc: &eframe::CreationContext<'_>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::ffi::c_void;

    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: *mut c_void,
            attribute: u32,
            value: *const c_void,
            size: u32,
        ) -> i32;
    }

    if let Ok(handle) = cc.window_handle() {
        if let RawWindowHandle::Win32(window) = handle.as_raw() {
            let dark: u32 = 1;
            // DWMWA_USE_IMMERSIVE_DARK_MODE; older Windows versions simply ignore it.
            unsafe {
                let _ = DwmSetWindowAttribute(
                    window.hwnd.get() as *mut c_void,
                    20,
                    (&dark as *const u32).cast(),
                    std::mem::size_of::<u32>() as u32,
                );
            }
        }
    }
}

pub const BG: Color32 = Color32::from_rgb(14, 17, 30);
pub const SIDEBAR: Color32 = Color32::from_rgb(19, 23, 39);
pub const SURFACE: Color32 = Color32::from_rgb(27, 32, 52);
pub const SURFACE_RAISED: Color32 = Color32::from_rgb(34, 40, 63);
pub const OUTLINE: Color32 = Color32::from_rgb(56, 64, 91);
pub const TEXT: Color32 = Color32::from_rgb(244, 241, 247);
pub const MUTED: Color32 = Color32::from_rgb(160, 169, 190);
pub const CORAL: Color32 = Color32::from_rgb(255, 150, 123);
pub const LILAC: Color32 = Color32::from_rgb(171, 156, 255);
pub const MINT: Color32 = Color32::from_rgb(110, 218, 190);
pub const RED: Color32 = Color32::from_rgb(255, 133, 146);

const HERO_BYTES: &[u8] = include_bytes!("../assets/summanus-glow.png");

#[derive(Clone, Copy)]
pub enum Tone {
    Primary,
    Secondary,
    Quiet,
    Danger,
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = SIDEBAR;
    visuals.window_rounding = egui::Rounding::same(16.0);
    visuals.window_stroke = Stroke::new(1.0_f32, OUTLINE);
    visuals.menu_rounding = egui::Rounding::same(12.0);
    visuals.override_text_color = Some(TEXT);
    visuals.hyperlink_color = LILAC;
    visuals.extreme_bg_color = Color32::from_rgb(17, 21, 36);
    visuals.code_bg_color = Color32::from_rgb(20, 24, 39);
    visuals.faint_bg_color = SURFACE;
    visuals.selection.bg_fill = Color32::from_rgb(92, 75, 130);
    visuals.selection.stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.noninteractive.bg_fill = SURFACE;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.inactive.bg_fill = SURFACE_RAISED;
    visuals.widgets.inactive.weak_bg_fill = SURFACE;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, OUTLINE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(49, 55, 82);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(49, 55, 82);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, LILAC);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.active.bg_fill = Color32::from_rgb(67, 56, 92);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(67, 56, 92);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, TEXT);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.rounding = egui::Rounding::same(10.0);
    }
    visuals.slider_trailing_fill = true;
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(9.0, 9.0);
    style.spacing.button_padding = Vec2::new(12.0, 8.0);
    style.spacing.interact_size.y = 34.0;
    ctx.set_style(style);

    if let Ok(bytes) = fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
        let mut fonts = egui::FontDefinitions::default();
        fonts
            .font_data
            .insert("segoe".into(), egui::FontData::from_owned(bytes));
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "segoe".into());
        if let Ok(symbols) = fs::read(r"C:\Windows\Fonts\seguisym.ttf") {
            fonts
                .font_data
                .insert("segoe_symbols".into(), egui::FontData::from_owned(symbols));
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push("segoe_symbols".into());
        }
        ctx.set_fonts(fonts);
    }
}

pub fn button(ui: &mut egui::Ui, label: &str, tone: Tone, size: Vec2) -> egui::Response {
    let (fill, stroke, color) = match tone {
        Tone::Primary => (CORAL, Stroke::NONE, BG),
        Tone::Secondary => (
            Color32::from_rgb(62, 52, 92),
            Stroke::new(1.0_f32, LILAC),
            TEXT,
        ),
        Tone::Quiet => (SURFACE, Stroke::new(1.0_f32, OUTLINE), TEXT),
        Tone::Danger => (
            Color32::from_rgb(74, 40, 60),
            Stroke::new(1.0_f32, RED),
            RED,
        ),
    };
    ui.add_sized(
        size,
        egui::Button::new(RichText::new(label).color(color).size(14.0).strong())
            .fill(fill)
            .stroke(stroke)
            .rounding(egui::Rounding::same(11.0)),
    )
}

pub fn brand_mark(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, size * 0.27, Color32::from_rgb(58, 48, 91));
    let center = rect.center();
    painter.circle_stroke(center, size * 0.29, Stroke::new(size * 0.065, LILAC));
    painter.circle_filled(center, size * 0.18, CORAL);
    painter.circle_filled(
        center + Vec2::new(size * 0.04, -size * 0.04),
        size * 0.075,
        Color32::from_rgb(255, 217, 159),
    );
    painter.circle_filled(
        center + Vec2::new(size * 0.28, -size * 0.24),
        size * 0.045,
        Color32::from_rgb(255, 218, 180),
    );
}

pub fn hero_texture(ctx: &egui::Context) -> Option<TextureHandle> {
    let rgba = image::load_from_memory(HERO_BYTES).ok()?.into_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Some(ctx.load_texture("summanus-glow", image, egui::TextureOptions::LINEAR))
}

pub fn window_icon() -> Option<Arc<egui::IconData>> {
    let rgba = image::load_from_memory(HERO_BYTES).ok()?.into_rgba8();
    let icon = image::imageops::resize(&rgba, 64, 64, image::imageops::FilterType::Lanczos3);
    Some(Arc::new(egui::IconData {
        rgba: icon.into_raw(),
        width: 64,
        height: 64,
    }))
}
