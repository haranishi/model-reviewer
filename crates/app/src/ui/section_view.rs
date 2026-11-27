//! 断面の枠と矢印（3Dビューの上に egui で描く）。矢印のドラッグで `SetSection(Some(plane))` を
//! 毎フレーム積む。当たりは `gizmo_view` と同じく、線から12ポイントに入っている間だけ小さな
//! 操作域を重ねる。名前は `ids::SECTION_HANDLE`。`Session` を読まず、入力用の構造体だけを受け取る。
//!
//! 枠は `frame_corners` を投影した破線で、視点の後ろに回った角がある辺は描かない。矢印は、境界箱の
//! 中央を平面へ移した点から、法線の両方向へ画面上で約60ポイント。視線が軸とほぼ平行で矢印が
//! 潰れるときは矢印を出さない（位置の欄で動かす）。ドラッグ中の位置は `drag_offset` が求め、
//! `None` の間は前の値を保つ（何も積まない）。Esc は掴んだときの平面へ戻す。

use glam::{Vec2, Vec3};
use model_reviewer_core::camera::OrbitCamera;
use model_reviewer_core::math::{Aabb, Ray};
use model_reviewer_core::section::{SectionPlane, drag_offset};

use crate::automation::{WidgetEntry, WidgetFrame};
use crate::command::Command;
use crate::theme;

use super::ids;

/// 矢印の当たりの幅（ポイント。`gizmo_view` と同じ）。
pub const HIT_TOLERANCE: f32 = 12.0;
/// 矢印の片側の長さ（ポイント）。
pub const ARROW_HALF: f32 = 60.0;
/// 画面上でこれより短くしか見えない軸は、視線とほぼ平行とみなす（矢印を出さない）。
const MIN_SCREEN_STEP: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionViewInput {
    pub plane: SectionPlane,
    /// 枠を作る境界箱（見えている全体）。
    pub bounds: Aabb,
    pub camera: OrbitCamera,
    /// 3Dビューの矩形（画面の座標）。
    pub rect: egui::Rect,
    /// 矢印を掴んでいるか。
    pub grabbing: bool,
}

/// フレームをまたいで持つ値。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct GrabState {
    /// 掴んだときの平面と視線。
    start: Option<(SectionPlane, Ray)>,
    cancelled: bool,
}

fn aspect(rect: egui::Rect) -> f32 {
    if rect.height() > 0.0 {
        rect.width() / rect.height()
    } else {
        1.0
    }
}

fn to_screen(input: &SectionViewInput, world: Vec3) -> Option<egui::Pos2> {
    let ndc = input.camera.project(world, aspect(input.rect))?;
    let rect = input.rect;
    let pos = egui::pos2(
        rect.center().x + ndc.x * rect.width() * 0.5,
        rect.center().y - ndc.y * rect.height() * 0.5,
    );
    pos.is_finite().then_some(pos)
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

/// 矢印の中心（境界箱の中央を平面へ移した点。ワールド）。
pub fn arrow_center(plane: &SectionPlane, bounds: &Aabb) -> Vec3 {
    let mut center = if bounds.is_empty() {
        Vec3::ZERO
    } else {
        bounds.center()
    };
    if !center.is_finite() {
        center = Vec3::ZERO;
    }
    center[plane.axis.index()] = plane.offset;
    center
}

/// 矢印の両端（画面の座標）。1つ目が法線の側（消す側）。視線が軸とほぼ平行か、中心が視点の後ろ
/// なら `None`。
pub fn arrow_segment(input: &SectionViewInput) -> Option<[egui::Pos2; 2]> {
    if !input.rect.is_finite() || input.rect.width() <= 0.0 || input.rect.height() <= 0.0 {
        return None;
    }
    let center_world = arrow_center(&input.plane, &input.bounds);
    let center = to_screen(input, center_world)?;
    // 画面上の向きは、中心から法線の向きへ少し進めた点を投影して求める。
    let reach = if input.bounds.is_empty() {
        1.0
    } else {
        input.bounds.radius().max(1.0e-6)
    };
    let step = reach * 0.25;
    let ahead = to_screen(input, center_world + input.plane.normal() * step);
    let behind = to_screen(input, center_world - input.plane.normal() * step);
    let dir = match (ahead, behind) {
        (Some(a), _) => a - center,
        (None, Some(b)) => center - b,
        (None, None) => return None,
    };
    if !dir.is_finite() || dir.length() < MIN_SCREEN_STEP {
        return None;
    }
    let dir = dir.normalized();
    Some([center + dir * ARROW_HALF, center - dir * ARROW_HALF])
}

/// 自動操作が掴む点（矢印の消す側の半分の中ほど）。
pub fn grab_point(input: &SectionViewInput) -> Option<egui::Pos2> {
    let [tip, tail] = arrow_segment(input)?;
    let center = tip + (tail - tip) * 0.5;
    Some(center + (tip - center) * 0.6)
}

fn segment_distance(point: egui::Pos2, a: egui::Pos2, b: egui::Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 <= f32::EPSILON {
        return point.distance(a);
    }
    let t = ((point - a).dot(ab) / len2).clamp(0.0, 1.0);
    point.distance(a + ab * t)
}

/// 画面の点が矢印の当たりに入っているか。
pub fn hits_arrow(input: &SectionViewInput, pos: egui::Pos2) -> bool {
    arrow_segment(input).is_some_and(|[a, b]| {
        segment_distance(pos, a, b) <= HIT_TOLERANCE && input.rect.contains(pos)
    })
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

pub fn show(ui: &mut egui::Ui, input: &SectionViewInput, out: &mut Vec<Command>) {
    let rect = input.rect;
    if !rect.is_finite() || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let id = ui.scope_id().with("section.handle");
    let mut state: GrabState = ui.data(|data| data.get_temp(id)).unwrap_or_default();
    let dragging = ui.ctx().is_being_dragged(id);
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let hovered = pointer.is_some_and(|p| hits_arrow(input, p));
    let zone = if input.grabbing || dragging || state.start.is_some() {
        Some(rect)
    } else {
        match (hovered, pointer) {
            (true, Some(p)) => Some(
                egui::Rect::from_center_size(p, egui::Vec2::splat(HIT_TOLERANCE * 2.0))
                    .intersect(rect),
            ),
            _ => None,
        }
    };
    let mut hot = hovered;
    if let Some(zone) = zone {
        let response = ui.interact(zone, id, egui::Sense::drag());
        handle_input(ui, &response, input, &mut state, out);
        if response.hovered() || response.dragged() {
            hot = true;
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
    paint(ui, input, hot || state.start.is_some() || input.grabbing);
    if let Some(point) = grab_point(input) {
        reg_rect(
            ui.ctx(),
            ids::SECTION_HANDLE,
            egui::Rect::from_center_size(point, egui::Vec2::splat(HIT_TOLERANCE)),
        );
    }
}

fn handle_input(
    ui: &egui::Ui,
    response: &egui::Response,
    input: &SectionViewInput,
    state: &mut GrabState,
    out: &mut Vec<Command>,
) {
    let ray_at = |pos: egui::Pos2| {
        pos_to_ndc(pos, input.rect).map(|ndc| {
            input
                .camera
                .screen_ray_in(ndc, aspect(input.rect), &input.bounds)
        })
    };
    if response.drag_started() {
        let origin = ui
            .input(|i| i.pointer.press_origin())
            .or_else(|| response.interact_pointer_pos());
        *state = GrabState::default();
        if let Some(ray) = origin
            .filter(|p| hits_arrow(input, *p) || input.grabbing)
            .and_then(ray_at)
        {
            state.start = Some((input.plane, ray));
        }
        return;
    }
    if response.dragged()
        && !state.cancelled
        && let Some((start, start_ray)) = state.start
    {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            out.push(Command::SetSection(Some(start)));
            state.cancelled = true;
        } else if response.drag_delta() != egui::Vec2::ZERO
            && let Some(now_ray) = response.interact_pointer_pos().and_then(ray_at)
            && let Some(offset) = drag_offset(&start, &input.bounds, &start_ray, &now_ray)
            && offset.is_finite()
        {
            let plane = SectionPlane { offset, ..start }.clamped(&input.bounds);
            out.push(Command::SetSection(Some(plane)));
        }
    }
    if response.drag_stopped() {
        if let (Some((start, _)), false) = (state.start, state.cancelled)
            && ui.input(|i| i.key_pressed(egui::Key::Escape))
        {
            // egui は Esc でドラッグを打ち切る（離したのと同じに見える）。そのときは戻す。
            out.push(Command::SetSection(Some(start)));
        }
        *state = GrabState::default();
    }
}

fn dashed(painter: &egui::Painter, a: egui::Pos2, b: egui::Pos2, stroke: egui::Stroke) {
    painter.extend(egui::Shape::dashed_line(&[a, b], stroke, 6.0, 4.0));
}

fn paint(ui: &egui::Ui, input: &SectionViewInput, hot: bool) {
    let painter = ui.painter_at(input.rect);
    let halo = egui::Stroke::new(3.0, egui::Color32::from_black_alpha(150));
    let line = egui::Stroke::new(1.2, theme::SELECTION);
    let corners = input.plane.frame_corners(&input.bounds);
    let screen: Vec<Option<egui::Pos2>> = corners.iter().map(|c| to_screen(input, *c)).collect();
    for i in 0..4 {
        if let (Some(a), Some(b)) = (screen[i], screen[(i + 1) % 4])
            && a.distance(b) > 0.5
        {
            dashed(&painter, a, b, halo);
            dashed(&painter, a, b, line);
        }
    }
    let Some([tip, tail]) = arrow_segment(input) else {
        return;
    };
    let width = if hot { 4.0 } else { 2.5 };
    painter.line_segment([tip, tail], egui::Stroke::new(width + 2.0, halo.color));
    painter.line_segment([tip, tail], egui::Stroke::new(width, theme::SELECTION));
    for (end, other) in [(tip, tail), (tail, tip)] {
        let dir = (end - other).normalized();
        let side = dir.rot90() * 6.0;
        let base = end - dir * 10.0;
        painter.add(egui::Shape::convex_polygon(
            vec![end + dir * 2.0, base + side, base - side],
            theme::SELECTION,
            egui::Stroke::new(1.0, halo.color),
        ));
    }
    let center = tip + (tail - tip) * 0.5;
    painter.circle(
        center,
        3.0,
        theme::SELECTION,
        egui::Stroke::new(1.0, halo.color),
    );
}
