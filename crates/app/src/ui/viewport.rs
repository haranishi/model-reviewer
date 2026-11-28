//! 中央の3D。ドラッグは回転、右か Shift＋左は移動。クリックだけが選択になる。
//! 左端に道具の列（`toolrail`）、上端に表示・投影・格子のボタン（`viewbar`）、右上に向きの
//! 立方体（`viewcube`）を重ねる。断面が入っている間は、上端のボタンの下に断面の帯
//! （`section_bar`）を、3Dの上に平面の枠と矢印（`section_view`）を重ねる。
//! 入力の振り分けは、置いた順の逆（後に置いたものが勝つ）:
//! 3Dビューの操作域 → 注釈の印 → 断面の矢印 → ハンドル（`gizmo_view`）→ ボタン・道具の列・立方体。
//! 断面の矢印と変形のハンドルが重なったら、変形のハンドルが先。
//!
//! 計測の道具では、ポインタが3Dビューの上で動いたフレームだけ `ViewHover` を積む（吸着の候補を
//! 探す）。止まっている間とドラッグ中は積まない。3Dビューを出たら `ViewHover { ndc: None }` を1回。

use crate::command::Command;
use crate::session::Session;
use model_reviewer_core::camera::ZOOM_EXP_PER_PX;
use model_reviewer_core::gizmo::Tool;
use model_reviewer_core::snap::SnapKind;
use model_reviewer_core::units::UnitLabel;

use super::empty;
use super::gizmo_view::{self, GizmoViewInput};
use super::ids;
use super::measure_view::{self, MeasureViewInput, SnapCandidate, SnapMarks};
use super::overlay;
use super::reg;
use super::section_bar::{self, SectionBarInput};
use super::section_view::{self, SectionViewInput};
use super::toolrail::{self, ToolRailInput};
use super::viewbar::{self, ViewBarInput};
use super::viewcube::{self, ViewCubeInput};

/// 3Dビューの縁から部品までの余白。
const EDGE: f32 = 8.0;
/// 向きの立方体の大きさ（ux.md の72ポイント）。
const CUBE_SIZE: f32 = 72.0;

const POINTER_ON_VIEWPORT: &str = "viewport.pointer";
/// 前のフレームで `ViewHover` を積んだポインタの位置（`None` は3Dビューの外）。
const LAST_HOVER: &str = "viewport.last_hover";

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
    collect_hover(ui, session, &response, rect, out);
    collect_pointer(ui, &response, rect, out);
    overlay::show(ui, session, rect, out);
    if session.model.is_some() {
        let (input, marks) = measure_input(session, rect);
        measure_view::show_with(ui, &input, &marks);
        let section = session.section.and_then(|plane| {
            Some(SectionBarInput {
                plane,
                bounds: crate::session::section_bounds(session)?,
                units: session.units,
            })
        });
        let on_arrow = section
            .as_ref()
            .is_some_and(|section| section_handle(ui, session, rect, section, out));
        let on_handle = gizmo(ui, session, rect, out);
        if (on_arrow || on_handle) && !on_view {
            // 断面の矢印と変形のハンドルは3Dビューの一部。上にポインタがあってもキーを働かせる。
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::unique(POINTER_ON_VIEWPORT), true);
            });
        }
        view_controls(ui, session, rect, section.as_ref(), out);
    }
}

/// 計測の点のワールドの位置（1点目・2点目の順）。変形中は、動かしている部品の点に差分の行列を
/// 掛ける（線と距離が部品と一緒に動く）。部品が無いか有限でなければ `None`。
pub fn measure_worlds(session: &Session) -> [Option<glam::Vec3>; 2] {
    let Some(doc) = session.doc.as_ref() else {
        return [None, None];
    };
    [session.measurement.first, session.measurement.second].map(|point| {
        let point = point?;
        let id = doc.id_of(point.node)?;
        let node = doc.node(id)?;
        let world = node.world.transform_point3(point.local);
        let world = match session.transform.as_ref() {
            Some(transform) if transform.nodes.contains(&id.0) => {
                transform.delta.transform_point3(world)
            }
            _ => world,
        };
        world.is_finite().then_some(world)
    })
}

fn view_aspect(rect: egui::Rect) -> f32 {
    if rect.height() > 0.0 {
        rect.width() / rect.height()
    } else {
        1.0
    }
}

fn world_to_pos(session: &Session, world: glam::Vec3, rect: egui::Rect) -> Option<egui::Pos2> {
    let ndc = session.camera.project(world, view_aspect(rect))?;
    let pos = egui::pos2(
        rect.center().x + ndc.x * rect.width() * 0.5,
        rect.center().y - ndc.y * rect.height() * 0.5,
    );
    pos.is_finite().then_some(pos)
}

/// 計測の線・点・候補の印の入力。視点の後ろの点は描かない。点を除いても、種類と薄さは点と
/// 同じ順に詰める（`SnapMarks` の約束）。距離は表示の単位へ換算して渡す。
pub fn measure_input(session: &Session, rect: egui::Rect) -> (MeasureViewInput, SnapMarks) {
    let units = session.units;
    let mut input = MeasureViewInput {
        points: Vec::new(),
        distance: None,
        unit: units.shown(),
    };
    let mut marks = SnapMarks::default();
    let worlds = measure_worlds(session);
    if let [Some(a), Some(b)] = worlds {
        input.distance = Some(a.distance(b))
            .filter(|d| d.is_finite())
            .map(|d| units.to_display(d));
    }
    let kinds = session.measurement.kinds;
    for (index, world) in worlds.iter().enumerate() {
        let Some(world) = world else {
            continue;
        };
        let Some(pos) = world_to_pos(session, *world, rect) else {
            continue;
        };
        let slot = input.points.len();
        input.points.push(pos);
        marks.point_kinds[slot] = kinds[index];
        marks.faded[slot] = session.section.is_some_and(|plane| plane.cuts(*world));
    }
    if input.points.len() < 2 {
        input.distance = None;
    }
    if session.tool == Tool::Measure
        && let Some(target) = session.snap_preview
        && let Some(pos) = world_to_pos(session, target.world, rect)
    {
        let edge = match (target.kind, target.edge) {
            (SnapKind::Edge, Some([a, b])) => world_to_pos(session, a, rect)
                .zip(world_to_pos(session, b, rect))
                .map(|(a, b)| [a, b]),
            _ => None,
        };
        marks.candidate = Some(SnapCandidate {
            pos,
            kind: target.kind,
            edge,
        });
    }
    (input, marks)
}

/// 計測の道具で、ポインタが3Dビューの上を動いたフレームだけ `ViewHover` を積む。
fn collect_hover(
    ui: &egui::Ui,
    session: &Session,
    response: &egui::Response,
    rect: egui::Rect,
    out: &mut Vec<Command>,
) {
    let id = egui::Id::unique(LAST_HOVER);
    let last: Option<egui::Pos2> = ui.data(|data| data.get_temp(id)).flatten();
    let measuring = session.tool == Tool::Measure && session.model.is_some();
    let dragging = response.dragged() || response.is_pointer_button_down_on();
    let pos = response
        .hover_pos()
        .filter(|_| measuring && !dragging)
        .filter(|p| rect.contains(*p));
    match pos {
        Some(pos) if last != Some(pos) => {
            if let Some(ndc) = pos_to_ndc(pos, rect) {
                out.push(Command::ViewHover { ndc: Some(ndc) });
            }
            ui.data_mut(|data| data.insert_temp(id, Some(pos)));
        }
        Some(_) => {}
        None if dragging && measuring => {
            // ドラッグ中（回転・移動）は候補を探さない。離した後に動けば探し直す。
        }
        None => {
            if last.is_some() || session.snap_preview.is_some() {
                out.push(Command::ViewHover { ndc: None });
            }
            ui.data_mut(|data| data.insert_temp(id, None::<egui::Pos2>));
        }
    }
}

/// 断面の枠と矢印。断面が入っていて、読み込みで置き換わる途中でないときだけ。
/// ポインタが矢印の当たりに入っていれば真。
fn section_handle(
    ui: &mut egui::Ui,
    session: &Session,
    rect: egui::Rect,
    section: &SectionBarInput,
    out: &mut Vec<Command>,
) -> bool {
    if session.is_loading_replacement() {
        return false;
    }
    let input = SectionViewInput {
        plane: section.plane,
        bounds: section.bounds,
        camera: session.camera,
        rect,
        grabbing: false,
    };
    section_view::show(ui, &input, out);
    ui.input(|i| i.pointer.hover_pos())
        .is_some_and(|p| section_view::hits_arrow(&input, p))
}

/// 移動・回転・拡大の道具で部品が選ばれているときのハンドル。読み込み中は出さない。
/// ポインタがハンドルの当たりに入っていれば真。
fn gizmo(ui: &mut egui::Ui, session: &Session, rect: egui::Rect, out: &mut Vec<Command>) -> bool {
    if session.edits_locked() {
        return false;
    }
    let Some(frame) = crate::session::gizmo_frame(
        session,
        [rect.width(), rect.height()],
        crate::session::GIZMO_SIZE,
    ) else {
        return false;
    };
    let transform = session.transform.as_ref();
    let units = session.units;
    let readout = transform
        .and_then(|t| t.readout)
        .map(|readout| gizmo_view::shown_readout(readout, units));
    let input = GizmoViewInput {
        tool: session.tool,
        frame: crate::session::moved_frame(frame, transform),
        rect,
        active: transform.and_then(|t| t.handle()),
        readout,
    };
    let suffix = units.display.map(UnitLabel::suffix);
    gizmo_view::show_with(ui, &input, suffix, out);
    let shapes = model_reviewer_core::gizmo::layout(input.tool, &input.frame);
    ui.input(|i| i.pointer.hover_pos())
        .filter(|p| rect.contains(*p))
        .is_some_and(|p| {
            let local = glam::Vec2::new(p.x - rect.min.x, p.y - rect.min.y);
            model_reviewer_core::gizmo::hit(&shapes, local, gizmo_view::HIT_TOLERANCE).is_some()
        })
}

/// 上端のボタンと右上の立方体。ボタンが立方体まで届く狭い幅では、立方体をボタンの下へ下げる。
fn view_controls(
    ui: &mut egui::Ui,
    session: &Session,
    rect: egui::Rect,
    section: Option<&SectionBarInput>,
    out: &mut Vec<Command>,
) {
    let input = ViewBarInput {
        display: session.display,
        projection: session.camera.projection,
        grid_visible: session.grid_visible,
        textures_shown: session
            .texture_options
            .enabled
            .then_some(session.textures_shown),
        lines_supported: session.lines_supported,
        keys: session.key_labels.clone(),
    };
    let area = rect.shrink(EDGE);
    let rail_input = ToolRailInput {
        tool: session.tool,
        has_model: session.model.is_some(),
        edit_blocked: session.edit_block_reason().map(str::to_owned),
        keys: session.key_labels.clone(),
    };
    let rail = ui
        .scope_builder(
            egui::UiBuilder::new()
                .max_rect(area)
                .layout(egui::Layout::left_to_right(egui::Align::Min)),
            |ui| toolrail::show_with(ui, &rail_input, Some(session.snap.enabled), out),
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
            |ui| viewbar::show_with(ui, &input, Some(session.section.is_some()), out),
        )
        .response
        .rect;
    let mut cube = egui::Rect::from_min_size(
        egui::pos2(area.right() - CUBE_SIZE, area.top()),
        egui::vec2(CUBE_SIZE, CUBE_SIZE),
    );
    // 断面の帯は上端のボタンのすぐ下。右は立方体の分を空ける（立方体と重ならない）。
    let mut below = bar.bottom();
    if let Some(section) = section {
        let band_area = egui::Rect::from_min_max(
            egui::pos2(bar_area.left(), bar.bottom() + 4.0),
            egui::pos2(
                (area.right() - CUBE_SIZE - EDGE).max(bar_area.left() + 1.0),
                area.bottom(),
            ),
        );
        let band = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .max_rect(band_area)
                    .layout(egui::Layout::left_to_right(egui::Align::Min)),
                |ui| section_bar::show(ui, section, out),
            )
            .response
            .rect;
        below = below.max(band.bottom());
    }
    if cube.left() < bar.right() + EDGE {
        cube = cube.translate(egui::vec2(0.0, below + EDGE - cube.top()));
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

/// ポインタが3Dビューの上にあるか（前のフレームの結果）。
pub fn pointer_on_viewport(ctx: &egui::Context) -> bool {
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
