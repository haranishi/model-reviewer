//! 道具の列（3Dビューの左端）と、図形のボタン。押すと `SetTool`・`AddShape` を積む。
//! 図形はボタンを押した時点で置かれる。名前は `ids::TOOL_*`・`ids::SHAPE_*`。
//!
//! 道具の絵は図形として描く（絵文字や記号の書体に頼らない）。選ばれている道具は、地の色に加えて
//! 左の琥珀の帯と太字で示す。押せない道具は薄く描き、触れると理由が出る。
//! 説明の文と、ボタンの右下の小さい1文字は、入力の `keys`（今の割り当て）から作る。
//! 図形の4つは「図形」の道具を選んでいる間だけ、その道具の横に並べる。
//! [吸着] も同じく「計測」の道具を選んでいる間だけ、その道具の横に出し、押すと `SetSnap` を積む
//! （名前は `ids::MEASURE_SNAP`）。吸着の状態は `show_with` で渡す（`show` は出さない）。

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use model_reviewer_core::gizmo::Tool;
use model_reviewer_core::shapes::{ShapeKind, name_ja};

use crate::command::Command;
use crate::keymap::{Action, KeyLabels};
use crate::theme;

use super::ids;
use super::reg;
use super::viewbar::{BandSide, Chip, chip};

/// モデルが無いときの理由。
pub const NO_MODEL_REASON: &str = "モデルを開くと使えます";

/// 道具のボタンの大きさ（トラックパッドで押しやすい大きさ）。
const BUTTON: egui::Vec2 = egui::vec2(52.0, 48.0);
/// 道具のボタンの右下に出すキーの文字の大きさ。
const BADGE_SIZE: f32 = 10.0;

#[derive(Clone, Debug, PartialEq)]
pub struct ToolRailInput {
    pub tool: Tool,
    pub has_model: bool,
    /// 編集できない理由（読み込み中・書き出し中など）。`Some` の間、編集の道具と図形を無効にする。
    pub edit_blocked: Option<String>,
    /// 今のキーの文字（説明の文と道具の列の1文字に使う）。
    pub keys: KeyLabels,
}

struct ToolDef {
    tool: Tool,
    action: Action,
    label: &'static str,
    id: &'static str,
    /// 説明の文の「:」の後ろ。前には「移動（G）」のように今のキーを付ける。
    hint: &'static str,
    /// 形を編集する道具か（`edit_blocked` で止める）。
    edits: bool,
}

const TOOLS: [ToolDef; 7] = [
    ToolDef {
        tool: Tool::Select,
        action: Action::ToolSelect,
        label: "選択",
        id: ids::TOOL_SELECT,
        hint: "クリックで部品を選びます",
        edits: false,
    },
    ToolDef {
        tool: Tool::Move,
        action: Action::ToolMove,
        label: "移動",
        id: ids::TOOL_MOVE,
        hint: "矢印をドラッグして動かします",
        edits: true,
    },
    ToolDef {
        tool: Tool::Rotate,
        action: Action::ToolRotate,
        label: "回転",
        id: ids::TOOL_ROTATE,
        hint: "輪をドラッグして回します",
        edits: true,
    },
    ToolDef {
        tool: Tool::Scale,
        action: Action::ToolScale,
        label: "拡大",
        id: ids::TOOL_SCALE,
        hint: "箱をドラッグして大きさを変えます",
        edits: true,
    },
    ToolDef {
        tool: Tool::Shape,
        action: Action::ToolShape,
        label: "図形",
        id: ids::TOOL_SHAPE,
        hint: "立方体・球・円柱・平面を置きます",
        edits: true,
    },
    ToolDef {
        tool: Tool::Measure,
        action: Action::ToolMeasure,
        label: "計測",
        id: ids::TOOL_MEASURE,
        hint: "角や辺に近づけて2回クリックし、距離を測ります",
        edits: false,
    },
    ToolDef {
        tool: Tool::Annotate,
        action: Action::ToolAnnotate,
        label: "注釈",
        id: ids::TOOL_ANNOTATE,
        hint: "表面をクリックして注釈を書きます",
        edits: true,
    },
];

const SHAPES: [(ShapeKind, &str, Action); 4] = [
    (ShapeKind::Cube, ids::SHAPE_CUBE, Action::ShapeCube),
    (ShapeKind::Sphere, ids::SHAPE_SPHERE, Action::ShapeSphere),
    (
        ShapeKind::Cylinder,
        ids::SHAPE_CYLINDER,
        Action::ShapeCylinder,
    ),
    (ShapeKind::Plane, ids::SHAPE_PLANE, Action::ShapePlane),
];

fn tool_def(tool: Tool) -> &'static ToolDef {
    TOOLS
        .iter()
        .find(|def| def.tool == tool)
        .expect("道具の列はすべての道具を持つ")
}

/// 道具のボタンの説明（「移動（G）: 矢印をドラッグして動かします」）。未割り当てなら括弧を出さない。
pub fn tool_hint(tool: Tool, keys: &KeyLabels) -> String {
    let def = tool_def(tool);
    format!("{}: {}", keys.with_key(def.action, def.label), def.hint)
}

/// 道具のボタンの右下に出す1文字。修飾の無い1文字のキーのときだけ（「Shift+G」や「F2」は出さない）。
pub fn tool_badge(tool: Tool, keys: &KeyLabels) -> Option<&str> {
    keys.get(tool_def(tool).action)
        .filter(|key| key.chars().count() == 1)
}

/// [吸着] の説明。
pub fn snap_hint(enabled: bool, keys: &KeyLabels) -> String {
    let name = keys.with_key(Action::MeasureSnap, "吸着");
    if enabled {
        format!("{name}: 角や辺に近づけると、そこへ点を置きます。押すと切ります")
    } else {
        format!("{name}: 切っている間は、押した面の上に点を置きます。押すと入れます")
    }
}

/// 図形のボタンの説明（「立方体を視点の中心に置きます」。キーがあれば括弧で足す）。
pub fn shape_hint(kind: ShapeKind, keys: &KeyLabels) -> String {
    let action = SHAPES
        .iter()
        .find(|(k, _, _)| *k == kind)
        .map(|(_, _, action)| *action)
        .expect("図形のボタンはすべての形を持つ");
    let text = format!("{}を視点の中心に置きます", name_ja(kind));
    match keys.get(action) {
        Some(key) => format!("{text}（{key}）"),
        None => text,
    }
}

/// 道具を押せない理由。押せれば `None`。
pub fn tool_disabled_reason(input: &ToolRailInput, tool: Tool) -> Option<String> {
    if tool == Tool::Select {
        return None;
    }
    if !input.has_model {
        return Some(NO_MODEL_REASON.to_owned());
    }
    let edits = TOOLS.iter().any(|def| def.tool == tool && def.edits);
    if edits {
        input.edit_blocked.clone()
    } else {
        None
    }
}

/// 図形を置けない理由。置ければ `None`。
pub fn shape_disabled_reason(input: &ToolRailInput) -> Option<String> {
    tool_disabled_reason(input, Tool::Shape)
}

/// 道具の列を置き、列そのものの矩形（図形の4つを除く）を返す。[吸着] は出さない。
pub fn show(ui: &mut egui::Ui, input: &ToolRailInput, out: &mut Vec<Command>) -> egui::Rect {
    show_with(ui, input, None, out)
}

/// [吸着] の言葉（「吸着 入」「吸着 切」）。
pub fn snap_label(enabled: bool) -> &'static str {
    if enabled { "吸着 入" } else { "吸着 切" }
}

/// 道具の列を置き、列そのものの矩形（図形の4つと [吸着] を除く）を返す。`snap` は吸着が
/// 入っているか（`None` なら [吸着] を出さない）。[吸着] は計測の道具のときだけ出す。
/// 1列が使える高さに収まらないとき（幅960・高さ600で詳細を開いたときなど）は2列に並べ、
/// 押しやすい大きさを保つ。
pub fn show_with(
    ui: &mut egui::Ui,
    input: &ToolRailInput,
    snap: Option<bool>,
    out: &mut Vec<Command>,
) -> egui::Rect {
    let one_column = TOOLS.len() as f32 * (BUTTON.y + 2.0) + 8.0;
    let columns = if ui.available_height() >= one_column {
        1
    } else {
        2
    };
    let rows = TOOLS.len().div_ceil(columns);
    let mut rail_rect = egui::Rect::NOTHING;
    ui.horizontal_top(|ui| {
        let mut shape_row_top = None;
        let mut measure_row_top = None;
        rail_rect = rail_frame()
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
                ui.horizontal_top(|ui| {
                    for column in TOOLS.chunks(rows) {
                        ui.vertical(|ui| {
                            for def in column {
                                let top = tool_button(ui, input, def, out);
                                match def.tool {
                                    Tool::Shape => shape_row_top = Some(top),
                                    Tool::Measure => measure_row_top = Some(top),
                                    _ => {}
                                }
                            }
                        });
                    }
                });
            })
            .response
            .rect;
        if input.tool == Tool::Shape
            && let Some(top) = shape_row_top
        {
            ui.vertical(|ui| {
                // 図形の道具のボタンと上端をそろえる（枠の内側の余白の分だけ上へ）。
                let offset = top - ui.cursor().top() - 4.0;
                if offset > 0.0 {
                    ui.add_space(offset);
                }
                show_shapes(ui, input, out);
            });
        }
        if input.tool == Tool::Measure
            && let (Some(top), Some(enabled)) = (measure_row_top, snap)
        {
            ui.vertical(|ui| {
                let offset = top - ui.cursor().top() - 4.0;
                if offset > 0.0 {
                    ui.add_space(offset);
                }
                show_snap(ui, input, enabled, out);
            });
        }
    });
    rail_rect
}

/// 道具のボタン1つ。ボタンの上端を返す（横に並べる図形の4つと [吸着] をそろえるため）。
fn tool_button(
    ui: &mut egui::Ui,
    input: &ToolRailInput,
    def: &ToolDef,
    out: &mut Vec<Command>,
) -> f32 {
    let reason = tool_disabled_reason(input, def.tool);
    let draw = |painter: &egui::Painter, rect: egui::Rect, color: egui::Color32| {
        paint_tool_icon(painter, def.tool, rect, color);
    };
    let hint = tool_hint(def.tool, &input.keys);
    let response = chip(
        ui,
        &Chip {
            label: def.label,
            selected: input.tool == def.tool,
            enabled: reason.is_none(),
            reason: reason.as_deref(),
            hint: &hint,
            size: BUTTON,
            band: BandSide::Left,
        },
        Some(&draw),
    );
    if let Some(key) = tool_badge(def.tool, &input.keys)
        && ui.is_rect_visible(response.rect)
    {
        // 右下の隅に、本文より薄い小さい文字で。絵（上の中央）と名前（下の中央）には重ならない。
        let color = if reason.is_none() {
            theme::TEXT_WEAK
        } else {
            theme::TEXT_WEAK.gamma_multiply(0.45)
        };
        ui.painter().text(
            response.rect.right_bottom() + egui::vec2(-4.0, -3.0),
            egui::Align2::RIGHT_BOTTOM,
            key,
            egui::FontId::proportional(BADGE_SIZE),
            color,
        );
    }
    reg(def.id, &response);
    if response.clicked() && input.tool != def.tool {
        out.push(Command::SetTool(def.tool));
    }
    response.rect.top()
}

fn show_snap(ui: &mut egui::Ui, input: &ToolRailInput, enabled: bool, out: &mut Vec<Command>) {
    let reason = tool_disabled_reason(input, Tool::Measure);
    rail_frame().show(ui, |ui| {
        let draw = |painter: &egui::Painter, rect: egui::Rect, color: egui::Color32| {
            paint_snap_icon(painter, rect, color);
        };
        let hint = snap_hint(enabled, &input.keys);
        let response = chip(
            ui,
            &Chip {
                label: snap_label(enabled),
                selected: enabled,
                enabled: reason.is_none(),
                reason: reason.as_deref(),
                hint: &hint,
                size: egui::vec2(64.0, BUTTON.y),
                band: BandSide::Bottom,
            },
            Some(&draw),
        );
        reg(ids::MEASURE_SNAP, &response);
        if response.clicked() {
            out.push(Command::SetSnap(!enabled));
        }
    });
}

/// 吸着の絵（四角い頂点の印と、そこへ向かう4本の線）。
fn paint_snap_icon(painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.5;
    let p = |x: f32, y: f32| c + egui::vec2(x * r, y * r);
    painter.rect_stroke(
        egui::Rect::from_center_size(c, egui::vec2(r * 0.7, r * 0.7)),
        0.0,
        egui::Stroke::new(1.6, color),
        egui::StrokeKind::Middle,
    );
    for (from, to) in [
        ((-0.95, 0.0), (-0.5, 0.0)),
        ((0.5, 0.0), (0.95, 0.0)),
        ((0.0, -0.95), (0.0, -0.5)),
        ((0.0, 0.5), (0.0, 0.95)),
    ] {
        line(painter, vec![p(from.0, from.1), p(to.0, to.1)], color);
    }
}

fn show_shapes(ui: &mut egui::Ui, input: &ToolRailInput, out: &mut Vec<Command>) {
    let reason = shape_disabled_reason(input);
    rail_frame().show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.horizontal(|ui| {
            for (kind, id, _) in SHAPES {
                let draw = |painter: &egui::Painter, rect: egui::Rect, color: egui::Color32| {
                    paint_shape_icon(painter, kind, rect, color);
                };
                let label = name_ja(kind);
                let hint = shape_hint(kind, &input.keys);
                let response = chip(
                    ui,
                    &Chip {
                        label,
                        selected: false,
                        enabled: reason.is_none(),
                        reason: reason.as_deref(),
                        hint: &hint,
                        size: BUTTON,
                        band: BandSide::Bottom,
                    },
                    Some(&draw),
                );
                reg(id, &response);
                if response.clicked() {
                    out.push(Command::AddShape(kind));
                }
            }
        });
    });
}

fn rail_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::PANEL_BG.gamma_multiply(0.92))
        .stroke(egui::Stroke::new(1.0, theme::SEPARATOR))
        .corner_radius(6)
        .inner_margin(4)
}

fn line(painter: &egui::Painter, points: Vec<egui::Pos2>, color: egui::Color32) {
    painter.add(egui::Shape::line(points, egui::Stroke::new(1.6, color)));
}

fn arc(center: egui::Pos2, radius: f32, from: f32, to: f32, steps: usize) -> Vec<egui::Pos2> {
    (0..=steps)
        .map(|i| {
            let t = from + (to - from) * i as f32 / steps as f32;
            center + egui::vec2(t.cos(), -t.sin()) * radius
        })
        .collect()
}

fn ellipse(center: egui::Pos2, rx: f32, ry: f32, steps: usize) -> Vec<egui::Pos2> {
    (0..steps)
        .map(|i| {
            let t = TAU * i as f32 / steps as f32;
            center + egui::vec2(t.cos() * rx, t.sin() * ry)
        })
        .collect()
}

fn arrow_head(painter: &egui::Painter, tip: egui::Pos2, dir: egui::Vec2, color: egui::Color32) {
    let dir = dir.normalized();
    let side = dir.rot90() * 3.2;
    let base = tip - dir * 4.5;
    painter.add(egui::Shape::convex_polygon(
        vec![tip, base + side, base - side],
        color,
        egui::Stroke::NONE,
    ));
}

/// 道具の絵（20ポイントの矩形の中）。
fn paint_tool_icon(painter: &egui::Painter, tool: Tool, rect: egui::Rect, color: egui::Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.5;
    let p = |x: f32, y: f32| c + egui::vec2(x * r, y * r);
    match tool {
        Tool::Select => {
            // 矢印のカーソル。
            painter.add(egui::Shape::convex_polygon(
                vec![p(-0.45, -0.9), p(0.55, 0.15), p(0.0, 0.2), p(-0.45, 0.75)],
                color,
                egui::Stroke::NONE,
            ));
            line(painter, vec![p(0.0, 0.2), p(0.35, 0.9)], color);
        }
        Tool::Move => {
            line(painter, vec![p(-0.8, 0.0), p(0.8, 0.0)], color);
            line(painter, vec![p(0.0, -0.8), p(0.0, 0.8)], color);
            for dir in [
                egui::vec2(1.0, 0.0),
                egui::vec2(-1.0, 0.0),
                egui::vec2(0.0, 1.0),
                egui::vec2(0.0, -1.0),
            ] {
                arrow_head(painter, c + dir * r, dir, color);
            }
        }
        Tool::Rotate => {
            let points = arc(c, r * 0.72, 0.35 * PI, 1.85 * PI, 24);
            let last = points[points.len() - 1];
            let before = points[points.len() - 2];
            line(painter, points, color);
            arrow_head(
                painter,
                last + (last - before).normalized() * 3.0,
                last - before,
                color,
            );
        }
        Tool::Scale => {
            painter.rect_stroke(
                egui::Rect::from_min_max(p(-0.85, -0.85), p(0.85, 0.85)),
                1.0,
                egui::Stroke::new(1.4, color),
                egui::StrokeKind::Middle,
            );
            painter.rect_filled(
                egui::Rect::from_min_max(p(-0.85, 0.15), p(-0.15, 0.85)),
                1.0,
                color,
            );
            line(painter, vec![p(-0.05, 0.05), p(0.45, -0.45)], color);
            arrow_head(painter, p(0.62, -0.62), egui::vec2(1.0, -1.0), color);
        }
        Tool::Shape => paint_shape_icon(painter, ShapeKind::Cube, rect, color),
        Tool::Measure => {
            // 定規: 両端に止めのある線と目盛り。
            line(painter, vec![p(-0.85, 0.35), p(0.85, 0.35)], color);
            line(painter, vec![p(-0.85, -0.25), p(-0.85, 0.75)], color);
            line(painter, vec![p(0.85, -0.25), p(0.85, 0.75)], color);
            for x in [-0.42, 0.0, 0.42] {
                line(painter, vec![p(x, 0.35), p(x, 0.0)], color);
            }
            line(painter, vec![p(-0.6, -0.55), p(0.6, -0.55)], color);
            arrow_head(painter, p(-0.85, -0.55), egui::vec2(-1.0, 0.0), color);
            arrow_head(painter, p(0.85, -0.55), egui::vec2(1.0, 0.0), color);
        }
        Tool::Annotate => {
            // 注釈の印（番号つきの円と同じ形）に「＋」。
            painter.circle_stroke(p(0.0, -0.05), r * 0.7, egui::Stroke::new(1.6, color));
            line(painter, vec![p(-0.32, -0.05), p(0.32, -0.05)], color);
            line(painter, vec![p(0.0, -0.37), p(0.0, 0.27)], color);
            line(painter, vec![p(-0.2, 0.6), p(-0.45, 0.95)], color);
        }
    }
}

/// 図形の絵（20ポイントの矩形の中）。
fn paint_shape_icon(
    painter: &egui::Painter,
    kind: ShapeKind,
    rect: egui::Rect,
    color: egui::Color32,
) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.5;
    let p = |x: f32, y: f32| c + egui::vec2(x * r, y * r);
    match kind {
        ShapeKind::Cube => {
            // 斜めから見た立方体（六角形と内側の3本）。
            let top = p(0.0, -0.9);
            let upper_right = p(0.8, -0.45);
            let lower_right = p(0.8, 0.45);
            let bottom = p(0.0, 0.9);
            let lower_left = p(-0.8, 0.45);
            let upper_left = p(-0.8, -0.45);
            let middle = p(0.0, 0.0);
            painter.add(egui::Shape::closed_line(
                vec![
                    top,
                    upper_right,
                    lower_right,
                    bottom,
                    lower_left,
                    upper_left,
                ],
                egui::Stroke::new(1.6, color),
            ));
            line(painter, vec![upper_left, middle, upper_right], color);
            line(painter, vec![middle, bottom], color);
        }
        ShapeKind::Sphere => {
            painter.circle_stroke(c, r * 0.85, egui::Stroke::new(1.6, color));
            painter.add(egui::Shape::closed_line(
                ellipse(c, r * 0.85, r * 0.3, 32),
                egui::Stroke::new(1.0, color),
            ));
        }
        ShapeKind::Cylinder => {
            let rx = r * 0.7;
            let ry = r * 0.25;
            let top = p(0.0, -0.62);
            let bottom = p(0.0, 0.62);
            painter.add(egui::Shape::closed_line(
                ellipse(top, rx, ry, 32),
                egui::Stroke::new(1.6, color),
            ));
            line(painter, arc_lower(bottom, rx, ry), color);
            line(
                painter,
                vec![top - egui::vec2(rx, 0.0), bottom - egui::vec2(rx, 0.0)],
                color,
            );
            line(
                painter,
                vec![top + egui::vec2(rx, 0.0), bottom + egui::vec2(rx, 0.0)],
                color,
            );
        }
        ShapeKind::Plane => {
            painter.add(egui::Shape::closed_line(
                vec![
                    p(-0.45, -0.45),
                    p(0.95, -0.45),
                    p(0.45, 0.45),
                    p(-0.95, 0.45),
                ],
                egui::Stroke::new(1.6, color),
            ));
        }
    }
}

/// 楕円の下半分（円柱の底の見える側）。
fn arc_lower(center: egui::Pos2, rx: f32, ry: f32) -> Vec<egui::Pos2> {
    (0..=16)
        .map(|i| {
            let t = FRAC_PI_2 * 2.0 * i as f32 / 16.0;
            center + egui::vec2(t.cos() * rx, t.sin() * ry)
        })
        .collect()
}
