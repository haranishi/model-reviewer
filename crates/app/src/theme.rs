//! 配色（ux.md の「表層」）。暗めの1配色に絞る。文字は本文14、補助12ポイント。
//! 色の値はここにだけ置き、画面の部品はこの定数を使う。

use egui::{Color32, FontFamily, FontId, Stroke, TextStyle, Theme};

/// 3Dビューの背景 #22252B。
pub const VIEW_BG: Color32 = Color32::from_rgb(0x22, 0x25, 0x2B);
pub const PANEL_BG: Color32 = Color32::from_rgb(0x2A, 0x2D, 0x34);
pub const SEPARATOR: Color32 = Color32::from_rgb(0x3A, 0x3E, 0x47);
pub const TEXT: Color32 = Color32::from_rgb(0xE6, 0xE8, 0xEB);
pub const TEXT_WEAK: Color32 = Color32::from_rgb(0xA9, 0xAF, 0xB8);
/// 選択（輪郭線・行の左の帯）。3Dの輪郭線は render 側が同じ値を持つ。
pub const SELECTION: Color32 = Color32::from_rgb(0xF2, 0xA3, 0x3A);
pub const ANNOTATION: Color32 = Color32::from_rgb(0x4F, 0xB3, 0xFF);
pub const WARNING: Color32 = Color32::from_rgb(0xF2, 0xC1, 0x4E);
pub const ERROR: Color32 = Color32::from_rgb(0xFF, 0x6B, 0x6B);
/// 注釈追加モードの間の「注釈を追加中」の地の色。
pub const MODE_ACTIVE: Color32 = Color32::from_rgb(0x94, 0x6A, 0x33);

/// 軸の色（X・Y・Z）。色だけで見分けさせず、必ず文字を添える。
pub const AXIS_X: Color32 = Color32::from_rgb(0xE0, 0x65, 0x5F);
pub const AXIS_Y: Color32 = Color32::from_rgb(0x7C, 0xC3, 0x6A);
pub const AXIS_Z: Color32 = Color32::from_rgb(0x5B, 0x8D, 0xEF);
/// 軸の色を X・Y・Z の順に。
pub const AXES: [Color32; 3] = [AXIS_X, AXIS_Y, AXIS_Z];

pub const BODY_SIZE: f32 = 14.0;
pub const SMALL_SIZE: f32 = 12.0;

/// 3Dビューの背景をリニアRGBAで返す（描画先のクリア値）。
pub fn view_clear_linear() -> [f32; 4] {
    let lin = |v: u8| {
        let c = f32::from(v) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    [lin(VIEW_BG.r()), lin(VIEW_BG.g()), lin(VIEW_BG.b()), 1.0]
}

pub fn install(ctx: &egui::Context) {
    ctx.set_theme(Theme::Dark);
    ctx.style_mut_of(Theme::Dark, |style| {
        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(17.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Body,
                FontId::new(BODY_SIZE, FontFamily::Proportional),
            ),
            (
                TextStyle::Button,
                FontId::new(BODY_SIZE, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(SMALL_SIZE, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(13.0, FontFamily::Monospace),
            ),
        ]
        .into();
        let visuals = &mut style.visuals;
        visuals.dark_mode = true;
        visuals.override_text_color = Some(TEXT);
        visuals.weak_text_color = Some(TEXT_WEAK);
        visuals.panel_fill = PANEL_BG;
        visuals.window_fill = PANEL_BG;
        visuals.window_stroke = Stroke::new(1.0, SEPARATOR);
        visuals.extreme_bg_color = VIEW_BG;
        visuals.faint_bg_color = Color32::from_rgb(0x30, 0x33, 0x3B);
        visuals.warn_fg_color = WARNING;
        visuals.error_fg_color = ERROR;
        visuals.hyperlink_color = ANNOTATION;
        visuals.selection.bg_fill = SELECTION.gamma_multiply(0.35);
        visuals.selection.stroke = Stroke::new(1.0, SELECTION);
        visuals.button_frame = true;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, SEPARATOR);
        visuals.widgets.noninteractive.bg_fill = PANEL_BG;
        visuals.widgets.noninteractive.weak_bg_fill = PANEL_BG;
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        // ボタンは weak_bg_fill で塗る。パネル #2A2D34 との差が小さいと文字だけに見える。
        visuals.widgets.inactive = button_visual(mix(SEPARATOR, TEXT, 0.22), TEXT_WEAK);
        visuals.widgets.hovered = button_visual(mix(SEPARATOR, TEXT, 0.55), TEXT);
        // 押したときは選択色を混ぜ、重ねたときの灰色と色相で分ける。
        visuals.widgets.active = button_visual(mix(VIEW_BG, SELECTION, 0.55), SELECTION);
        visuals.widgets.open = button_visual(mix(SEPARATOR, TEXT, 0.55), TEXT);
    });
}

fn mix(from: Color32, to: Color32, t: f32) -> Color32 {
    let chan = |a: u8, b: u8| {
        let mixed = f32::from(a) + (f32::from(b) - f32::from(a)) * t;
        mixed.round().clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(
        chan(from.r(), to.r()),
        chan(from.g(), to.g()),
        chan(from.b(), to.b()),
    )
}

fn button_visual(fill: Color32, stroke: Color32) -> egui::style::WidgetVisuals {
    egui::style::WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: Stroke::new(1.0, stroke),
        fg_stroke: Stroke::new(1.0, TEXT),
        corner_radius: egui::CornerRadius::same(4),
        expansion: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_color_is_linear_view_background() {
        let c = view_clear_linear();
        assert!((c[0] - 0.016).abs() < 0.001);
        assert!((c[2] - 0.024).abs() < 0.001);
        assert_eq!(c[3], 1.0);
    }

    #[test]
    fn install_sets_dark_panels_and_sizes() {
        let ctx = egui::Context::default();
        install(&ctx);
        let style = ctx.style_of(Theme::Dark);
        assert_eq!(style.visuals.panel_fill, PANEL_BG);
        assert_eq!(style.text_styles[&TextStyle::Body].size, BODY_SIZE);
        assert_eq!(style.text_styles[&TextStyle::Small].size, SMALL_SIZE);
        let widgets = &style.visuals.widgets;
        assert_ne!(widgets.inactive.weak_bg_fill, PANEL_BG);
        assert_ne!(widgets.hovered.weak_bg_fill, widgets.inactive.weak_bg_fill);
        assert_ne!(widgets.active.weak_bg_fill, widgets.hovered.weak_bg_fill);
        assert_ne!(
            widgets.active.bg_stroke.color,
            widgets.inactive.bg_stroke.color
        );
        assert_eq!(widgets.inactive.fg_stroke.color, TEXT);
        assert!(widgets.inactive.bg_stroke.width > 0.0);
    }
}
