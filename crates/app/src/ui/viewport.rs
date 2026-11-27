//! 中央の3D。ドラッグは回転、右か Shift＋左は移動。クリックだけが選択になる。
//! 左端に道具の列（`toolrail`）、上端に表示・投影・格子のボタン（`viewbar`）、右上に向きの
//! 立方体（`viewcube`）を重ねる。入力の振り分けは、置いた順の逆（後に置いたものが勝つ）:
//! 3Dビューの操作域 → 注釈の印 → ハンドル（`gizmo_view`）→ ボタン・道具の列・立方体。

use crate::command::Command;
use crate::session::Session;
use model_reviewer_core::camera::ZOOM_EXP_PER_PX;

use super::empty;
use super::gizmo_view::{self, GizmoViewInput};
use super::ids;
use super::measure_view::{self, MeasureViewInput};
use super::overlay;
use super::reg;
use super::toolrail::{self, ToolRailInput};
use super::viewbar::{self, ViewBarInput};
use super::viewcube::{self, ViewCubeInput};

/// 3Dビューの縁から部品までの余白。
const EDGE: f32 = 8.0;
/// 向きの立方体の大きさ（ux.md の72ポイント）。
const CUBE_SIZE: f32 = 72.0;

const POINTER_ON_VIEWPORT: &str = "viewport.pointer";

pub fn show(ui: &mut egui::Ui, session: &Session, out: &mut Vec<Command>) {
    let rect = ui.available_rect_before_wrap();
    let response = ui.interact(
        rect,
        ui.scope_id().with(ids::VIEWPORT),
        egui::Sense::CLICK | egui::Sense::DRAG,
    );
    reg(ids::VIEWPORT, &response);
    let on_view = response.hovered() || response.dragged() || response.is_pointer_button_down_on();
    ui.ctx().data_mut(|data| {
        data.insert_temp(egui::Id::unique(POINTER_ON_VIEWPORT), on_view);
    });
    crate::gpu_view::paint_scene(ui, rect);
    if session.model.is_none() {
        empty::show(ui, session, out);
    }
    collect_pointer(ui, &response, rect, out);
    overlay::show(ui, session, rect, out);
    if session.model.is_some() {
        measure_view::show(ui, &measure_input(session, rect));
        gizmo(ui, session, rect, out);
        view_controls(ui, session, rect, out);
    }
}

/// 計測の点を画面へ移す。変形中は、動かしている部品の点に差分の行列を掛ける（線と距離が
/// 部品と一緒に動く）。視点の後ろの点は描かない。
pub fn measure_input(session: &Session, rect: egui::Rect) -> MeasureViewInput {
    let mut input = MeasureViewInput {
        points: Vec::new(),
        distance: None,
        unit: session.unit,
    };
    let Some(doc) = session.doc.as_ref() else {
        return input;
    };
    let worlds: Vec<glam::Vec3> = [session.measurement.first, session.measurement.second]
        .into_iter()
        .flatten()
        .filter_map(|point| {
            let id = doc.id_of(point.node)?;
            let node = doc.node(id)?;
            let world = node.world.transform_point3(point.local);
            Some(match session.transform.as_ref() {
                Some(transform) if transform.nodes.contains(&id.0) => {
                    transform.delta.transform_point3(world)
                }
                _ => world,
            })
        })
        .filter(|world| world.is_finite())
        .collect();
    if let [a, b] = worlds[..] {
        input.distance = Some(a.distance(b)).filter(|d| d.is_finite());
    }
    let aspect = if rect.height() > 0.0 {
        rect.width() / rect.height()
    } else {
        1.0
    };
    input.points = worlds
        .iter()
        .filter_map(|world| session.camera.project(*world, aspect))
        .map(|ndc| {
            egui::pos2(
                rect.center().x + ndc.x * rect.width() * 0.5,
                rect.center().y - ndc.y * rect.height() * 0.5,
            )
        })
        .collect();
    if input.points.len() < 2 {
        input.distance = None;
    }
    input
}

/// 移動・回転・拡大の道具で部品が選ばれているときのハンドル。読み込み中は出さない。
fn gizmo(ui: &mut egui::Ui, session: &Session, rect: egui::Rect, out: &mut Vec<Command>) {
    if session.edits_locked() {
        return;
    }
    let Some(frame) = crate::session::gizmo_frame(
        session,
        [rect.width(), rect.height()],
        crate::session::GIZMO_SIZE,
    ) else {
        return;
    };
    let transform = session.transform.as_ref();
    let input = GizmoViewInput {
        tool: session.tool,
        frame: crate::session::moved_frame(frame, transform),
        rect,
        active: transform.and_then(|t| t.handle()),
        readout: transform.and_then(|t| t.readout),
    };
    gizmo_view::show(ui, &input, out);
}

/// 上端のボタンと右上の立方体。ボタンが立方体まで届く狭い幅では、立方体をボタンの下へ下げる。
fn view_controls(ui: &mut egui::Ui, session: &Session, rect: egui::Rect, out: &mut Vec<Command>) {
    let input = ViewBarInput {
        display: session.display,
        projection: session.camera.projection,
        grid_visible: session.grid_visible,
        lines_supported: session.lines_supported,
    };
    let area = rect.shrink(EDGE);
    let rail_input = ToolRailInput {
        tool: session.tool,
        has_model: session.model.is_some(),
        edit_blocked: session.edit_block_reason().map(str::to_owned),
    };
    let rail = ui
        .scope_builder(
            egui::UiBuilder::new()
                .max_rect(area)
                .layout(egui::Layout::left_to_right(egui::Align::Min)),
            |ui| toolrail::show(ui, &rail_input, out),
        )
        .inner;
    // 道具の列の右から始める（図形の4つは列の右に出るが、上端のボタンとは高さが違う）。
    let rail_right = if rail.is_finite() {
        rail.right()
    } else {
        area.left()
    };
    let bar_area = egui::Rect::from_min_max(egui::pos2(rail_right + EDGE, area.top()), area.max);
    let bar = ui
        .scope_builder(
            egui::UiBuilder::new()
                .max_rect(bar_area)
                .layout(egui::Layout::left_to_right(egui::Align::Min)),
            |ui| viewbar::show(ui, &input, out),
        )
        .response
        .rect;
    let mut cube = egui::Rect::from_min_size(
        egui::pos2(area.right() - CUBE_SIZE, area.top()),
        egui::vec2(CUBE_SIZE, CUBE_SIZE),
    );
    if cube.left() < bar.right() + EDGE {
        cube = cube.translate(egui::vec2(0.0, bar.bottom() + EDGE - cube.top()));
    }
    viewcube::show(
        ui,
        &ViewCubeInput {
            yaw: session.camera.yaw,
            pitch: session.camera.pitch,
            rect: cube,
        },
        out,
    );
}

pub(crate) fn pointer_on_viewport(ctx: &egui::Context) -> bool {
    ctx.data(|data| {
        data.get_temp(egui::Id::unique(POINTER_ON_VIEWPORT))
            .unwrap_or(false)
    })
}

fn collect_pointer(
    ui: &egui::Ui,
    response: &egui::Response,
    rect: egui::Rect,
    out: &mut Vec<Command>,
) {
    let typing = ui.ctx().text_edit_focused();
    if !typing {
        let delta = response.drag_delta();
        if delta != egui::Vec2::ZERO {
            let y_up = glam::Vec2::new(delta.x, -delta.y);
            let shift = ui.input(|input| input.modifiers.shift);
            let secondary = response.dragged_by(egui::PointerButton::Secondary);
            let primary = response.dragged_by(egui::PointerButton::Primary);
            if secondary || (shift && primary) {
                let scale = ui.pixels_per_point();
                out.push(Command::Pan(y_up * scale));
            } else if primary && !shift {
                out.push(Command::Orbit(y_up));
            }
        }
        if let Some(touch) = ui.input(|input| input.multi_touch())
            && rect.contains(touch.center_pos)
        {
            let delta = touch.translation_delta;
            if delta != egui::Vec2::ZERO {
                let scale = ui.pixels_per_point();
                out.push(Command::Pan(glam::Vec2::new(delta.x, -delta.y) * scale));
            }
        }
        if response.hovered() {
            push_zoom(ui, rect, out);
        }
    }
    if response.clicked_by(egui::PointerButton::Primary)
        && let Some(pos) = response.interact_pointer_pos()
        && let Some(ndc) = pos_to_ndc(pos, rect)
    {
        out.push(Command::ViewClick { ndc });
    }
}

fn push_zoom(ui: &egui::Ui, rect: egui::Rect, out: &mut Vec<Command>) {
    let (scroll_y, zoom_delta, pos) = ui.input(|input| {
        (
            input.smooth_scroll_delta.y,
            input.zoom_delta(),
            input.pointer.hover_pos(),
        )
    });
    let mut factor = 1.0_f32;
    if scroll_y.is_finite() && scroll_y != 0.0 {
        // 上へ回す（egui では正の値）と寄る。egui の Ctrl＋ホイールの拡大と同じ向き。
        factor *= (-scroll_y * ZOOM_EXP_PER_PX).exp();
    }
    if zoom_delta.is_finite() && zoom_delta > 0.0 && (zoom_delta - 1.0).abs() > 1.0e-4 {
        factor /= zoom_delta;
    }
    if !factor.is_finite() || (factor - 1.0).abs() <= 1.0e-6 {
        return;
    }
    out.push(Command::Zoom {
        factor,
        cursor_ndc: pos.and_then(|pos| pos_to_ndc(pos, rect)),
    });
}

fn pos_to_ndc(pos: egui::Pos2, rect: egui::Rect) -> Option<glam::Vec2> {
    let width = rect.width();
    let height = rect.height();
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let ndc = glam::Vec2::new(
        (pos.x - rect.center().x) / (width * 0.5),
        (rect.center().y - pos.y) / (height * 0.5),
    );
    ndc.is_finite().then_some(ndc)
}
