//! 3Dビューの上端のボタン：表示（面・面と線・線・透かし）、投影（透視・平行）、格子、断面。
//! 状態が見えるように並べたボタンにする（開くメニューにしない）。線が使えないGPUでは「面と線」
//! 「線」を無効にし、理由を出す。押すと `SetDisplay`・`SetProjection`・`SetGrid`・
//! `ToggleSection` を積む。[断面] は断面の状態を `show_with` で渡したときだけ出す。
//! 名前は `ids::VIEW_*`。
//!
//! 選ばれているボタンは、色だけでなく琥珀の帯と太字でも示す（`chip`。道具の列も使う）。
//! 押せないボタンは薄く描き、触れると理由が出る。いま選ばれているボタンを押しても何も積まない。

use model_reviewer_core::camera::Projection;
use model_reviewer_render::DisplayMode;

use crate::command::Command;
use crate::theme;

use super::ids;
use super::reg;

/// 線の表示が使えないときの理由。
pub const NO_LINES_REASON: &str = "このGPUは線の表示に対応していないため使えません";

/// 上端のボタンの高さ。トラックパッドで押しやすいよう、本文の行より高くする。
const BAR_HEIGHT: f32 = 28.0;
/// 選ばれている印の帯の太さ。
const BAND: f32 = 3.0;
/// 組と組の間。
const GROUP_GAP: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewBarInput {
    pub display: DisplayMode,
    pub projection: Projection,
    pub grid_visible: bool,
    /// `SceneRenderer::supports_lines`。GPUが無いとき（試験）は偽。
    pub lines_supported: bool,
}

/// [断面] を出さずに置く。
pub fn show(ui: &mut egui::Ui, input: &ViewBarInput, out: &mut Vec<Command>) {
    show_with(ui, input, None, out);
}

/// `section` は断面が入っているか（`None` なら [断面] を出さない）。
pub fn show_with(
    ui: &mut egui::Ui,
    input: &ViewBarInput,
    section: Option<bool>,
    out: &mut Vec<Command>,
) {
    egui::Frame::new()
        .fill(theme::PANEL_BG.gamma_multiply(0.92))
        .stroke(egui::Stroke::new(1.0, theme::SEPARATOR))
        .corner_radius(6)
        .inner_margin(4)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 4.0);
            // 狭い3Dビュー（幅960の画面で両側のパネルを開いたとき）では、組の途中で次の行へ送る。
            ui.horizontal_wrapped(|ui| {
                let displays = [
                    (
                        DisplayMode::Faces,
                        "面",
                        ids::VIEW_DISPLAY_FACES,
                        "面だけを描きます（Z で面と線を切り替え）",
                    ),
                    (
                        DisplayMode::FacesAndLines,
                        "面と線",
                        ids::VIEW_DISPLAY_FACES_LINES,
                        "面に三角形の辺を重ねます（Z）",
                    ),
                    (
                        DisplayMode::Lines,
                        "線",
                        ids::VIEW_DISPLAY_LINES,
                        "三角形の辺だけを描きます",
                    ),
                    (
                        DisplayMode::XRay,
                        "透かし",
                        ids::VIEW_DISPLAY_XRAY,
                        "部品を半透明にして奥を見せます（Shift+Z）",
                    ),
                ];
                for (mode, label, id, hint) in displays {
                    let needs_lines =
                        matches!(mode, DisplayMode::FacesAndLines | DisplayMode::Lines);
                    let enabled = !needs_lines || input.lines_supported;
                    let response = chip(
                        ui,
                        &Chip {
                            label,
                            selected: input.display == mode,
                            enabled,
                            reason: (!enabled).then_some(NO_LINES_REASON),
                            hint,
                            size: text_size(ui, label),
                            band: BandSide::Bottom,
                        },
                        None,
                    );
                    reg(id, &response);
                    if response.clicked() && input.display != mode {
                        out.push(Command::SetDisplay(mode));
                    }
                }
                ui.add_space(GROUP_GAP);
                let projections = [
                    (
                        Projection::Perspective,
                        "透視",
                        ids::VIEW_PROJECTION_PERSPECTIVE,
                        "遠くの物を小さく描きます（5 で切り替え）",
                    ),
                    (
                        Projection::Orthographic,
                        "平行",
                        ids::VIEW_PROJECTION_ORTHOGRAPHIC,
                        "遠近のゆがみ無しで描きます（5 で切り替え）",
                    ),
                ];
                for (projection, label, id, hint) in projections {
                    let response = chip(
                        ui,
                        &Chip {
                            label,
                            selected: input.projection == projection,
                            enabled: true,
                            reason: None,
                            hint,
                            size: text_size(ui, label),
                            band: BandSide::Bottom,
                        },
                        None,
                    );
                    reg(id, &response);
                    if response.clicked() && input.projection != projection {
                        out.push(Command::SetProjection(projection));
                    }
                }
                ui.add_space(GROUP_GAP);
                let label = "格子";
                let response = chip(
                    ui,
                    &Chip {
                        label,
                        selected: input.grid_visible,
                        enabled: true,
                        reason: None,
                        hint: if input.grid_visible {
                            "床の格子と軸を隠します"
                        } else {
                            "床の格子と軸を出します"
                        },
                        size: text_size(ui, label),
                        band: BandSide::Bottom,
                    },
                    None,
                );
                reg(ids::VIEW_GRID, &response);
                if response.clicked() {
                    out.push(Command::SetGrid(!input.grid_visible));
                }
                if let Some(on) = section {
                    ui.add_space(GROUP_GAP);
                    let label = "断面";
                    let response = chip(
                        ui,
                        &Chip {
                            label,
                            selected: on,
                            enabled: true,
                            reason: None,
                            hint: if on {
                                "断面をやめて全体を表示します（C）"
                            } else {
                                "軸に垂直な面で切って中を見ます（C）"
                            },
                            size: text_size(ui, label),
                            band: BandSide::Bottom,
                        },
                        None,
                    );
                    reg(ids::VIEW_SECTION, &response);
                    if response.clicked() {
                        out.push(Command::ToggleSection);
                    }
                }
            });
        });
}

fn text_size(ui: &egui::Ui, label: &str) -> egui::Vec2 {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(theme::BODY_SIZE),
        theme::TEXT,
    );
    egui::vec2((galley.size().x + 20.0).max(40.0), BAR_HEIGHT)
}

/// 選ばれている印の帯をどの辺に付けるか。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BandSide {
    Bottom,
    Left,
}

/// 状態を持つボタン。
pub(crate) struct Chip<'a> {
    pub label: &'a str,
    pub selected: bool,
    pub enabled: bool,
    /// 押せない理由（触れると出る）。
    pub reason: Option<&'a str>,
    /// 押せるときに触れると出る説明。
    pub hint: &'a str,
    pub size: egui::Vec2,
    pub band: BandSide,
}

/// 絵を描く関数。描く矩形と線の色を受け取る。
pub(crate) type Icon<'a> = &'a dyn Fn(&egui::Painter, egui::Rect, egui::Color32);

/// 状態を持つボタンを描く。絵があれば上に絵、下に文字。押せないときは触れるだけを受ける。
pub(crate) fn chip(ui: &mut egui::Ui, chip: &Chip<'_>, icon: Option<Icon<'_>>) -> egui::Response {
    let sense = if chip.enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(chip.size, sense);
    let enabled = chip.enabled;
    let selected = chip.selected;
    let label = chip.label.to_owned();
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, enabled, selected, label.clone())
    });
    if ui.is_rect_visible(rect) {
        paint_chip(ui, rect, &response, chip, icon);
    }
    let tip = if chip.enabled {
        chip.hint
    } else {
        chip.reason.unwrap_or(chip.hint)
    };
    if tip.is_empty() {
        response
    } else {
        response.on_hover_text(tip)
    }
}

fn paint_chip(
    ui: &egui::Ui,
    rect: egui::Rect,
    response: &egui::Response,
    chip: &Chip<'_>,
    icon: Option<Icon<'_>>,
) {
    let visuals = &ui.style().visuals.widgets;
    let pressed = chip.enabled && response.is_pointer_button_down_on();
    let hovered = chip.enabled && response.hovered();
    let mut fill = if chip.selected {
        theme::MODE_ACTIVE
    } else if pressed {
        visuals.active.weak_bg_fill
    } else if hovered {
        visuals.hovered.weak_bg_fill
    } else {
        visuals.inactive.weak_bg_fill
    };
    // 押せるボタンは、選ばれていなくても本文と同じ明るさの文字と細い枠で描く（灰色の文字だけだと
    // 押せないボタンと見分けにくい）。押せないボタンは枠を付けず、塗りも文字も薄くする。
    let text = if chip.enabled {
        theme::TEXT
    } else {
        theme::TEXT_WEAK.gamma_multiply(0.45)
    };
    if !chip.enabled {
        fill = fill.gamma_multiply(0.35);
    }
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, fill);
    if hovered || pressed {
        painter.rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(1.0, theme::TEXT_WEAK),
            egui::StrokeKind::Inside,
        );
    } else if chip.enabled && !chip.selected {
        painter.rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(1.0, theme::TEXT_WEAK.gamma_multiply(0.5)),
            egui::StrokeKind::Inside,
        );
    }
    if chip.selected {
        let band = match chip.band {
            BandSide::Bottom => egui::Rect::from_min_max(
                egui::pos2(rect.left() + 3.0, rect.bottom() - BAND),
                egui::pos2(rect.right() - 3.0, rect.bottom()),
            ),
            BandSide::Left => egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.top() + 3.0),
                egui::pos2(rect.left() + BAND, rect.bottom() - 3.0),
            ),
        };
        painter.rect_filled(band, 1.0, theme::SELECTION);
    }
    let font = if icon.is_some() {
        egui::FontId::proportional(theme::SMALL_SIZE)
    } else {
        egui::FontId::proportional(theme::BODY_SIZE)
    };
    let galley = painter.layout_no_wrap(chip.label.to_owned(), font, text);
    let text_pos = if let Some(icon) = icon {
        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.top() + 16.0),
            egui::vec2(18.0, 18.0),
        );
        icon(painter, icon_rect, text);
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.bottom() - galley.size().y - 4.0,
        )
    } else {
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5 - if chip.selected { 1.0 } else { 0.0 },
        )
    };
    painter.galley(text_pos, galley.clone(), text);
    if chip.selected {
        // 太字の代わりに、少しずらして重ねる（日本語の太字の書体を登録していない）。
        painter.galley(text_pos + egui::vec2(0.6, 0.0), galley, text);
    }
}
