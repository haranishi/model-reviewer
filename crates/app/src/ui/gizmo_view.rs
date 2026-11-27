//! ハンドルの画面。形と当たりは `core::gizmo::{layout, hit}` を呼ぶ。
//!
//! 約束: ポインタが当たりの幅（12ポイント）に入っている間だけ、ポインタの位置に小さな操作域を
//! 重ねて押下とドラッグを受ける。掴んでいる間は操作域を3Dビュー全体へ広げる。押すと
//! `BeginTransform`、動かすと `DragTransform`、離すと `CommitTransform`、Esc で `CancelTransform`。
//! 触れたときの強調は egui だけで描き、3Dを描き直さない。名前は `ids::GIZMO_*`。
//!
//! 操作域の当たり判定は、egui の決まりで前のフレームの矩形を使う。触れた時点のフレームで操作域を
//! 置くので、次のフレームの押下はその操作域が受ける。操作域の `Id` は小さいときも広げたときも
//! 同じにして、掴んでいる途中で外れないようにする。
//!
//! 自動操作のために、見えているハンドルごとに「掴める点」のまわりの矩形を `gizmo.<道具>.<軸>`
//! の名前で登録する（ポインタが離れていても登録する。押せる場所を先に知る必要があるため）。

use glam::Vec2;
use model_reviewer_core::gizmo::{GizmoFrame, Handle, HandleShape, Readout, Tool, hit, layout};

use crate::automation::{WidgetEntry, WidgetFrame};
use crate::command::Command;
use crate::theme;

use super::ids;

/// ハンドルの当たりの幅（ポイント）。
pub const HIT_TOLERANCE: f32 = 12.0;

/// 線の太さ（ふだん・触れている・掴んでいる）。
const WIDTH: f32 = 2.5;
const WIDTH_HOT: f32 = 4.0;
/// 軸の文字を形の先からさらに離す距離。
const LABEL_GAP: f32 = 11.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GizmoViewInput {
    pub tool: Tool,
    /// 中心・軸の向き・カメラ・大きさ。
    pub frame: GizmoFrame,
    /// 3Dビューの矩形（画面の座標）。
    pub rect: egui::Rect,
    /// 掴んでいるハンドル。
    pub active: Option<Handle>,
    /// 掴んでいる間の変化量。
    pub readout: Option<Readout>,
}

/// フレームをまたいで持つ値。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct GrabState {
    /// このドラッグで `BeginTransform` を積んだハンドル。
    grabbing: Option<Handle>,
    /// Esc で取り消した（離すまで何も積まない）。
    cancelled: bool,
}

/// ハンドルの自動操作の名前。
pub fn widget_name(handle: Handle) -> &'static str {
    match handle {
        Handle::MoveX => ids::GIZMO_MOVE_X,
        Handle::MoveY => ids::GIZMO_MOVE_Y,
        Handle::MoveZ => ids::GIZMO_MOVE_Z,
        Handle::RotX => ids::GIZMO_ROTATE_X,
        Handle::RotY => ids::GIZMO_ROTATE_Y,
        Handle::RotZ => ids::GIZMO_ROTATE_Z,
        Handle::ScaleX => ids::GIZMO_SCALE_X,
        Handle::ScaleY => ids::GIZMO_SCALE_Y,
        Handle::ScaleZ => ids::GIZMO_SCALE_Z,
        Handle::ScaleAll => ids::GIZMO_SCALE_ALL,
    }
}

/// 軸の番号（0=X・1=Y・2=Z）。全体の拡大は `None`。
fn axis_of(handle: Handle) -> Option<usize> {
    match handle {
        Handle::MoveX | Handle::RotX | Handle::ScaleX => Some(0),
        Handle::MoveY | Handle::RotY | Handle::ScaleY => Some(1),
        Handle::MoveZ | Handle::RotZ | Handle::ScaleZ => Some(2),
        Handle::ScaleAll => None,
    }
}

fn is_ring(handle: Handle) -> bool {
    matches!(handle, Handle::RotX | Handle::RotY | Handle::RotZ)
}

/// 変化量の文字に、距離の単位を添える（`None` なら添えない。表示の単位が「ファイルと同じ」の
/// 間は第2段階と同じ文字にするため）。距離の値は呼び出し側が表示の単位へ換算しておく。
pub fn readout_text_with(readout: Readout, distance_suffix: Option<&str>) -> String {
    match (readout, distance_suffix) {
        (Readout::Distance(_), Some(suffix)) => format!("{} {suffix}", readout_text(readout)),
        _ => readout_text(readout),
    }
}

/// 変化量の文字。距離は符号つき有効数字4桁、角度は度、倍率は「×」。
pub fn readout_text(readout: Readout) -> String {
    match readout {
        Readout::Distance(value) => signed(value, ""),
        Readout::AngleDeg(value) => signed(value, "°"),
        Readout::Factor(value) => {
            if value.is_finite() {
                format!("×{}", significant(value))
            } else {
                "×—".to_owned()
            }
        }
    }
}

fn signed(value: f32, suffix: &str) -> String {
    if !value.is_finite() {
        return format!("—{suffix}");
    }
    let sign = if value < 0.0 { "−" } else { "+" };
    format!("{sign}{}{suffix}", significant(value.abs()))
}

/// 有効数字4桁を目安にした小数点以下の桁数で書く（1000 以上は整数）。
fn significant(value: f32) -> String {
    let magnitude = value.abs();
    let decimals = if magnitude == 0.0 {
        2
    } else {
        (3 - magnitude.log10().floor() as i32).clamp(0, 6)
    };
    format!("{value:.prec$}", prec = decimals as usize)
}

/// 見えているハンドルごとの掴める点（3Dビューの左上が原点）。その点で `hit` がそのハンドルを返し、
/// ほかのハンドルからできるだけ離れた点を選ぶ。
pub fn grab_points(tool: Tool, frame: &GizmoFrame) -> Vec<(Handle, Vec2)> {
    let shapes = layout(tool, frame);
    let mut handles: Vec<Handle> = Vec::new();
    for shape in &shapes {
        if !handles.contains(&shape.handle) {
            handles.push(shape.handle);
        }
    }
    handles
        .into_iter()
        .filter_map(|handle| grab_point(&shapes, handle).map(|p| (handle, p)))
        .collect()
}

fn grab_point(shapes: &[HandleShape], handle: Handle) -> Option<Vec2> {
    let mut candidates = Vec::new();
    for shape in shapes.iter().filter(|shape| shape.handle == handle) {
        let points = &shape.points;
        if shape.closed && !is_ring(handle) && points.len() >= 3 {
            let sum = points.iter().fold(Vec2::ZERO, |acc, p| acc + *p);
            candidates.push(sum / points.len() as f32);
        }
        for (i, p) in points.iter().enumerate() {
            candidates.push(*p);
            let next = if i + 1 < points.len() {
                Some(points[i + 1])
            } else if shape.closed {
                Some(points[0])
            } else {
                None
            };
            if let Some(next) = next {
                candidates.push((*p + next) * 0.5);
            }
        }
    }
    let others: Vec<&HandleShape> = shapes.iter().filter(|s| s.handle != handle).collect();
    candidates
        .into_iter()
        .filter(|p| p.is_finite() && hit(shapes, *p, HIT_TOLERANCE) == Some(handle))
        .map(|p| {
            let clearance = others
                .iter()
                .map(|shape| polyline_distance(shape, p))
                .fold(f32::INFINITY, f32::min);
            (p, clearance)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(p, _)| p)
}

fn polyline_distance(shape: &HandleShape, point: Vec2) -> f32 {
    let points = &shape.points;
    match points.len() {
        0 => f32::INFINITY,
        1 => point.distance(points[0]),
        _ => {
            let mut best = f32::INFINITY;
            for pair in points.windows(2) {
                best = best.min(segment_distance(point, pair[0], pair[1]));
            }
            if shape.closed {
                best = best.min(segment_distance(point, points[points.len() - 1], points[0]));
            }
            best
        }
    }
}

fn segment_distance(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 <= f32::EPSILON {
        return point.distance(a);
    }
    let t = ((point - a).dot(ab) / len2).clamp(0.0, 1.0);
    point.distance(a + ab * t)
}

fn to_screen(rect: egui::Rect, p: Vec2) -> egui::Pos2 {
    egui::pos2(rect.min.x + p.x, rect.min.y + p.y)
}

fn to_local(rect: egui::Rect, p: egui::Pos2) -> Vec2 {
    Vec2::new(p.x - rect.min.x, p.y - rect.min.y)
}

fn pos_to_ndc(pos: egui::Pos2, rect: egui::Rect) -> Option<Vec2> {
    let (width, height) = (rect.width(), rect.height());
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let ndc = Vec2::new(
        (pos.x - rect.center().x) / (width * 0.5),
        (rect.center().y - pos.y) / (height * 0.5),
    );
    ndc.is_finite().then_some(ndc)
}

fn reg_rect(ctx: &egui::Context, name: &str, rect: egui::Rect) {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<WidgetFrame>(egui::Id::NULL)
            .push(WidgetEntry {
                name: name.to_owned(),
                min: [rect.min.x, rect.min.y],
                max: [rect.max.x, rect.max.y],
            });
    });
}

pub fn show(ui: &mut egui::Ui, input: &GizmoViewInput, out: &mut Vec<Command>) {
    show_with(ui, input, None, out);
}

/// `distance_suffix` は移動の変化量に添える単位（`readout_text_with`）。
pub fn show_with(
    ui: &mut egui::Ui,
    input: &GizmoViewInput,
    distance_suffix: Option<&str>,
    out: &mut Vec<Command>,
) {
    let rect = input.rect;
    if !rect.is_finite() || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let id = ui.scope_id().with("gizmo");
    let mut state: GrabState = ui.data(|data| data.get_temp(id)).unwrap_or_default();
    let shapes = layout(input.tool, &input.frame);
    let dragging = ui.ctx().is_being_dragged(id);
    if shapes.is_empty() && !dragging && state.grabbing.is_none() {
        ui.data_mut(|data| data.remove::<GrabState>(id));
        return;
    }

    let pointer = ui.input(|i| i.pointer.hover_pos());
    let hovered = pointer
        .filter(|p| rect.contains(*p))
        .and_then(|p| hit(&shapes, to_local(rect, p), HIT_TOLERANCE));
    let zone = if input.active.is_some() || dragging || state.grabbing.is_some() {
        Some(rect)
    } else {
        match (hovered, pointer) {
            (Some(_), Some(p)) => Some(
                egui::Rect::from_center_size(p, egui::Vec2::splat(HIT_TOLERANCE * 2.0))
                    .intersect(rect),
            ),
            _ => None,
        }
    };

    if let Some(zone) = zone {
        let response = ui.interact(zone, id, egui::Sense::drag());
        handle_input(ui, &response, rect, &shapes, hovered, &mut state, out);
        if response.hovered() || response.dragged() {
            let icon = if response.dragged() {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            };
            ui.ctx().set_cursor_icon(icon);
        }
    } else if !dragging {
        state = GrabState::default();
    }
    ui.data_mut(|data| data.insert_temp(id, state));

    let hot = input.active.or(state.grabbing).or(hovered);
    paint(
        ui,
        input,
        rect,
        &shapes,
        hot,
        state.grabbing.is_some() || input.active.is_some(),
        distance_suffix,
    );
    for (handle, point) in grab_points(input.tool, &input.frame) {
        let center = to_screen(rect, point);
        let marker = egui::Rect::from_center_size(center, egui::Vec2::splat(HIT_TOLERANCE));
        reg_rect(ui.ctx(), widget_name(handle), marker);
    }
}

fn handle_input(
    ui: &egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
    shapes: &[HandleShape],
    hovered: Option<Handle>,
    state: &mut GrabState,
    out: &mut Vec<Command>,
) {
    if response.drag_started() {
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .or_else(|| response.interact_pointer_pos());
        let handle = origin
            .and_then(|p| hit(shapes, to_local(rect, p), HIT_TOLERANCE))
            .or(hovered);
        *state = GrabState::default();
        if let (Some(handle), Some(ndc)) = (handle, origin.and_then(|p| pos_to_ndc(p, rect))) {
            out.push(Command::BeginTransform { handle, ndc });
            state.grabbing = Some(handle);
        }
        return;
    }
    if response.dragged() && state.grabbing.is_some() && !state.cancelled {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            out.push(Command::CancelTransform);
            state.cancelled = true;
        } else if response.drag_delta() != egui::Vec2::ZERO
            && let Some(ndc) = response
                .interact_pointer_pos()
                .and_then(|p| pos_to_ndc(p, rect))
        {
            out.push(Command::DragTransform { ndc });
        }
    }
    if response.drag_stopped() {
        if state.grabbing.is_some() && !state.cancelled {
            // egui は Esc でドラッグを打ち切る（離したのと同じに見える）。そのときは確定しない。
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                out.push(Command::CancelTransform);
            } else {
                out.push(Command::CommitTransform);
            }
        }
        *state = GrabState::default();
    }
}

fn axis_color(handle: Handle) -> egui::Color32 {
    axis_of(handle).map_or(theme::TEXT, |axis| theme::AXES[axis])
}

fn brighten(color: egui::Color32) -> egui::Color32 {
    let lift = |c: u8| (f32::from(c) + (255.0 - f32::from(c)) * 0.35).round() as u8;
    egui::Color32::from_rgb(lift(color.r()), lift(color.g()), lift(color.b()))
}

fn paint(
    ui: &egui::Ui,
    input: &GizmoViewInput,
    rect: egui::Rect,
    shapes: &[HandleShape],
    hot: Option<Handle>,
    grabbing: bool,
    distance_suffix: Option<&str>,
) {
    let painter = ui.painter_at(rect);
    let pivot = pivot_px(&input.frame).map(|p| to_screen(rect, p));
    // 掴んでいるハンドルを最後に描いて、重なりの上に出す。
    let order = shapes
        .iter()
        .filter(|s| Some(s.handle) != hot)
        .chain(shapes.iter().filter(|s| Some(s.handle) == hot));
    for shape in order {
        let is_hot = Some(shape.handle) == hot;
        let mut color = axis_color(shape.handle);
        if is_hot {
            color = brighten(color);
        } else if grabbing {
            color = color.gamma_multiply(0.35);
        }
        let width = if is_hot { WIDTH_HOT } else { WIDTH };
        let points: Vec<egui::Pos2> = shape.points.iter().map(|p| to_screen(rect, *p)).collect();
        if points.iter().any(|p| !p.is_finite()) {
            continue;
        }
        let halo = egui::Stroke::new(width + 2.0, egui::Color32::from_black_alpha(110));
        let stroke = egui::Stroke::new(width, color);
        if !shape.closed {
            painter.add(egui::Shape::line(points.clone(), halo));
            painter.add(egui::Shape::line(points, stroke));
        } else if is_ring(shape.handle) {
            painter.add(egui::Shape::closed_line(points.clone(), halo));
            painter.add(egui::Shape::closed_line(points, stroke));
        } else if shape.handle == Handle::ScaleAll {
            let fill = if is_hot {
                theme::TEXT.gamma_multiply(0.45)
            } else {
                theme::TEXT.gamma_multiply(0.18)
            };
            painter.add(egui::Shape::convex_polygon(
                points,
                fill,
                egui::Stroke::new(if is_hot { 2.0 } else { 1.5 }, theme::TEXT),
            ));
        } else {
            painter.add(egui::Shape::convex_polygon(
                points,
                color,
                egui::Stroke::new(1.0, egui::Color32::from_black_alpha(110)),
            ));
        }
    }
    if let Some(pivot) = pivot {
        painter.circle(
            pivot,
            3.0,
            theme::TEXT,
            egui::Stroke::new(1.0, egui::Color32::from_black_alpha(140)),
        );
    }
    paint_axis_labels(&painter, rect, shapes, pivot, hot, grabbing);
    if let (Some(readout), true) = (input.readout, input.active.is_some() || grabbing) {
        let anchor = ui
            .input(|i| i.pointer.latest_pos())
            .filter(|p| rect.contains(*p))
            .or(pivot)
            .unwrap_or(rect.center());
        paint_readout(
            &painter,
            rect,
            anchor,
            &readout_text_with(readout, distance_suffix),
        );
    }
}

/// 輪の文字を置く点。ほかの輪から離れていて、中心から遠い点（輪の外側の縁）を選ぶ。
fn ring_label_point(shapes: &[HandleShape], handle: Handle, pivot: Vec2) -> Option<Vec2> {
    let others: Vec<&HandleShape> = shapes.iter().filter(|s| s.handle != handle).collect();
    shapes
        .iter()
        .filter(|s| s.handle == handle)
        .flat_map(|s| s.points.iter().copied())
        .filter(|p| p.is_finite())
        .map(|p| {
            let clearance = others
                .iter()
                .map(|shape| polyline_distance(shape, p))
                .fold(f32::INFINITY, f32::min)
                .min(40.0);
            (p, clearance + 0.5 * p.distance(pivot))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(p, _)| p)
}

fn pivot_px(frame: &GizmoFrame) -> Option<Vec2> {
    let viewport = Vec2::from(frame.viewport_px);
    if viewport.x <= 0.0 || viewport.y <= 0.0 {
        return None;
    }
    let ndc = frame.camera.project(frame.pivot, viewport.x / viewport.y)?;
    let px = Vec2::new(
        (ndc.x + 1.0) * 0.5 * viewport.x,
        (1.0 - ndc.y) * 0.5 * viewport.y,
    );
    px.is_finite().then_some(px)
}

/// 軸の文字（X・Y・Z）。色だけで見分けさせない。移動と拡大は形の先、回転は輪の掴める点の外側。
fn paint_axis_labels(
    painter: &egui::Painter,
    rect: egui::Rect,
    shapes: &[HandleShape],
    pivot: Option<egui::Pos2>,
    hot: Option<Handle>,
    grabbing: bool,
) {
    let Some(pivot) = pivot else {
        return;
    };
    let mut done: Vec<Handle> = Vec::new();
    for shape in shapes {
        let handle = shape.handle;
        let Some(axis) = axis_of(handle) else {
            continue;
        };
        if done.contains(&handle) {
            continue;
        }
        done.push(handle);
        let anchor = if is_ring(handle) {
            ring_label_point(shapes, handle, to_local(rect, pivot))
        } else {
            shapes
                .iter()
                .filter(|s| s.handle == handle)
                .flat_map(|s| s.points.iter().copied())
                .max_by(|a, b| {
                    let pa = to_local(rect, pivot);
                    a.distance(pa).total_cmp(&b.distance(pa))
                })
        };
        let Some(anchor) = anchor.map(|p| to_screen(rect, p)) else {
            continue;
        };
        let out = anchor - pivot;
        let dir = if out.length() > 1.0e-3 {
            out.normalized()
        } else {
            egui::vec2(1.0, 0.0)
        };
        let pos = anchor + dir * LABEL_GAP;
        let mut color = theme::AXES[axis];
        if grabbing && Some(handle) != hot {
            color = color.gamma_multiply(0.35);
        }
        let text = ["X", "Y", "Z"][axis];
        let font = egui::FontId::proportional(theme::SMALL_SIZE);
        let galley = painter.layout_no_wrap(text.to_owned(), font, color);
        let rect = egui::Rect::from_center_size(pos, galley.size() + egui::vec2(6.0, 2.0));
        painter.rect_filled(rect, 3.0, egui::Color32::from_black_alpha(120));
        painter.galley(rect.center() - galley.size() * 0.5, galley, color);
    }
}

fn paint_readout(painter: &egui::Painter, view: egui::Rect, anchor: egui::Pos2, text: &str) {
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        egui::FontId::proportional(theme::BODY_SIZE),
        theme::TEXT,
    );
    let size = galley.size() + egui::vec2(12.0, 6.0);
    let mut rect = egui::Rect::from_min_size(anchor + egui::vec2(16.0, -size.y - 10.0), size);
    // 3Dビューからはみ出さないように押し戻す。
    let shift_x =
        (view.right() - 4.0 - rect.right()).min(0.0) + (view.left() + 4.0 - rect.left()).max(0.0);
    let shift_y =
        (view.bottom() - 4.0 - rect.bottom()).min(0.0) + (view.top() + 4.0 - rect.top()).max(0.0);
    rect = rect.translate(egui::vec2(shift_x, shift_y));
    painter.rect(
        rect,
        4.0,
        theme::PANEL_BG.gamma_multiply(0.94),
        egui::Stroke::new(1.0, theme::SELECTION),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.center() - galley.size() * 0.5, galley, theme::TEXT);
}
