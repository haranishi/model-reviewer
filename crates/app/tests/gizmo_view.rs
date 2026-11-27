//! ハンドルの画面。ポインタが当たりの幅に入ったときだけ受け、掴んだら3Dビュー全体で動きを受ける。

use egui_kittest::Harness;
use glam::{Vec2, Vec3};
use model_reviewer::automation::WidgetFrame;
use model_reviewer::command::{Command, Handle, Tool};
use model_reviewer::ui::gizmo_view::{
    self, GizmoViewInput, grab_points, readout_text, widget_name,
};
use model_reviewer::ui::ids;
use model_reviewer_core::camera::{OrbitCamera, Projection};
use model_reviewer_core::gizmo::{GizmoFrame, Readout};

fn view_rect() -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(40.0, 30.0), egui::vec2(640.0, 480.0))
}

fn frame(projection: Projection) -> GizmoFrame {
    let rect = view_rect();
    GizmoFrame {
        pivot: Vec3::ZERO,
        axes: [Vec3::X, Vec3::Y, Vec3::Z],
        camera: OrbitCamera {
            target: Vec3::ZERO,
            yaw: 35_f32.to_radians(),
            pitch: 20_f32.to_radians(),
            distance: 10.0,
            projection,
            ..OrbitCamera::default()
        },
        viewport_px: [rect.width(), rect.height()],
        size_px: 90.0,
    }
}

fn input(tool: Tool) -> GizmoViewInput {
    GizmoViewInput {
        tool,
        frame: frame(Projection::Perspective),
        rect: view_rect(),
        active: None,
        readout: None,
    }
}

struct Probe {
    input: GizmoViewInput,
    out: Vec<Command>,
    /// 下に3Dビューの操作域を置いて、ハンドルの外のドラッグがそちらへ行くことを確かめる。
    view_drags: usize,
}

fn harness(input: GizmoViewInput) -> Harness<'static, Probe> {
    Harness::builder()
        .with_size(egui::vec2(720.0, 540.0))
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_context_setup(|ctx| {
            model_reviewer::fonts::install(ctx);
            model_reviewer::theme::install(ctx);
        })
        .build_ui_state(
            |ui, probe: &mut Probe| {
                ui.ctx()
                    .data_mut(|data| data.insert_temp(egui::Id::NULL, WidgetFrame::default()));
                let view = ui.interact(
                    probe.input.rect,
                    egui::Id::unique("test.viewport"),
                    egui::Sense::click_and_drag(),
                );
                if view.dragged() && view.drag_delta() != egui::Vec2::ZERO {
                    probe.view_drags += 1;
                }
                let mut out = Vec::new();
                gizmo_view::show(ui, &probe.input, &mut out);
                probe.out.extend(out);
            },
            Probe {
                input,
                out: Vec::new(),
                view_drags: 0,
            },
        )
}

fn registered(harness: &Harness<'static, Probe>) -> Vec<(String, egui::Rect)> {
    harness
        .ctx
        .data(|data| data.get_temp::<WidgetFrame>(egui::Id::NULL))
        .unwrap_or_default()
        .entries()
        .iter()
        .map(|entry| {
            (
                entry.name.clone(),
                egui::Rect::from_min_max(entry.min.into(), entry.max.into()),
            )
        })
        .collect()
}

fn widget(harness: &Harness<'static, Probe>, name: &str) -> egui::Rect {
    registered(harness)
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("{name} が登録されていない"))
        .1
}

fn pointer(harness: &mut Harness<'static, Probe>, pos: egui::Pos2, pressed: Option<bool>) {
    harness.event(egui::Event::PointerMoved(pos));
    if let Some(pressed) = pressed {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    harness.step();
}

fn ndc(pos: egui::Pos2) -> Vec2 {
    let rect = view_rect();
    Vec2::new(
        (pos.x - rect.center().x) / (rect.width() * 0.5),
        (rect.center().y - pos.y) / (rect.height() * 0.5),
    )
}

#[test]
fn every_visible_handle_is_registered_by_name() {
    for (tool, want) in [
        (
            Tool::Move,
            vec![ids::GIZMO_MOVE_X, ids::GIZMO_MOVE_Y, ids::GIZMO_MOVE_Z],
        ),
        (
            Tool::Rotate,
            vec![
                ids::GIZMO_ROTATE_X,
                ids::GIZMO_ROTATE_Y,
                ids::GIZMO_ROTATE_Z,
            ],
        ),
        (
            Tool::Scale,
            vec![
                ids::GIZMO_SCALE_X,
                ids::GIZMO_SCALE_Y,
                ids::GIZMO_SCALE_Z,
                ids::GIZMO_SCALE_ALL,
            ],
        ),
        (Tool::Select, vec![]),
        (Tool::Measure, vec![]),
    ] {
        let mut harness = harness(input(tool));
        harness.step();
        let mut names: Vec<String> = registered(&harness).into_iter().map(|(n, _)| n).collect();
        names.sort();
        let mut want: Vec<String> = want.into_iter().map(str::to_owned).collect();
        want.sort();
        assert_eq!(names, want, "{tool:?}");
        for (_, rect) in registered(&harness) {
            assert!(view_rect().contains_rect(rect), "{rect:?}");
        }
    }
}

#[test]
fn grab_points_hit_their_own_handle() {
    for tool in [Tool::Move, Tool::Rotate, Tool::Scale] {
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let frame = frame(projection);
            let shapes = model_reviewer_core::gizmo::layout(tool, &frame);
            let points = grab_points(tool, &frame);
            assert!(!points.is_empty());
            for (handle, point) in points {
                assert_eq!(
                    model_reviewer_core::gizmo::hit(&shapes, point, gizmo_view::HIT_TOLERANCE),
                    Some(handle),
                    "{tool:?} {projection:?}"
                );
            }
        }
    }
}

#[test]
fn dragging_a_handle_begins_drags_and_commits() {
    let mut harness = harness(input(Tool::Move));
    harness.step();
    let start = widget(&harness, ids::GIZMO_MOVE_X).center();
    pointer(&mut harness, start, None);
    pointer(&mut harness, start, Some(true));
    // 押した時点で掴む。
    assert_eq!(
        harness.state().out,
        vec![Command::BeginTransform {
            handle: Handle::MoveX,
            ndc: ndc(start),
        }]
    );
    harness.state_mut().input.active = Some(Handle::MoveX);
    // 当たりの幅の外まで動かしても受け続ける（操作域が3Dビュー全体へ広がる）。
    let far = start + egui::vec2(150.0, 90.0);
    pointer(&mut harness, start + egui::vec2(30.0, 10.0), None);
    pointer(&mut harness, far, None);
    harness.state_mut().input.readout = Some(Readout::Distance(1.25));
    pointer(&mut harness, far, Some(false));
    harness.step();
    let out = harness.state().out.clone();
    assert_eq!(out.len(), 4, "{out:?}");
    assert_eq!(
        out[2],
        Command::DragTransform { ndc: ndc(far) },
        "最後の位置の ndc"
    );
    assert!(matches!(out[1], Command::DragTransform { .. }));
    assert_eq!(out[3], Command::CommitTransform);
    assert_eq!(harness.state().view_drags, 0, "視点は回らない");
}

#[test]
fn escape_cancels_and_release_does_not_commit() {
    let mut harness = harness(input(Tool::Rotate));
    harness.step();
    let start = widget(&harness, ids::GIZMO_ROTATE_Y).center();
    pointer(&mut harness, start, None);
    pointer(&mut harness, start, Some(true));
    harness.state_mut().input.active = Some(Handle::RotY);
    pointer(&mut harness, start + egui::vec2(20.0, 20.0), None);
    harness.key_press(egui::Key::Escape);
    pointer(&mut harness, start + egui::vec2(25.0, 25.0), None);
    harness.state_mut().input.active = None;
    pointer(&mut harness, start + egui::vec2(40.0, 40.0), None);
    pointer(&mut harness, start + egui::vec2(40.0, 40.0), Some(false));
    harness.step();
    let out = &harness.state().out;
    assert!(matches!(
        out[0],
        Command::BeginTransform {
            handle: Handle::RotY,
            ..
        }
    ));
    assert_eq!(
        out.iter()
            .filter(|c| matches!(c, Command::CancelTransform))
            .count(),
        1,
        "{out:?}"
    );
    let cancel_at = out
        .iter()
        .position(|c| matches!(c, Command::CancelTransform))
        .unwrap();
    assert!(
        out[cancel_at + 1..].is_empty(),
        "取り消した後は何も積まない: {out:?}"
    );
}

#[test]
fn dragging_away_from_handles_turns_the_view() {
    let mut harness = harness(input(Tool::Move));
    harness.step();
    let start = view_rect().min + egui::vec2(30.0, 30.0);
    pointer(&mut harness, start, None);
    pointer(&mut harness, start, Some(true));
    pointer(&mut harness, start + egui::vec2(20.0, 0.0), None);
    pointer(&mut harness, start + egui::vec2(40.0, 0.0), Some(false));
    assert!(harness.state().out.is_empty(), "{:?}", harness.state().out);
    assert!(harness.state().view_drags > 0);
}

#[test]
fn readout_text_has_sign_and_unit_marks() {
    assert_eq!(readout_text(Readout::Distance(12.5)), "+12.50");
    assert_eq!(readout_text(Readout::Distance(-0.25)), "−0.2500");
    assert_eq!(readout_text(Readout::AngleDeg(30.0)), "+30.00°");
    assert_eq!(readout_text(Readout::Factor(1.5)), "×1.500");
    assert_eq!(readout_text(Readout::Distance(f32::NAN)), "—");
    assert_eq!(widget_name(Handle::ScaleAll), ids::GIZMO_SCALE_ALL);
}

/// 名前・道具・掴んでいるハンドル・変化量・触れるハンドル。
type ShotCase = (
    &'static str,
    Tool,
    Option<Handle>,
    Option<Readout>,
    Option<Handle>,
);

#[test]
#[ignore = "wgpuで画面をPNGへ書き出す"]
fn render_gizmo_for_visual_check() {
    let cases: [ShotCase; 5] = [
        ("gizmo-move", Tool::Move, None, None, None),
        (
            "gizmo-move-hover-y",
            Tool::Move,
            None,
            None,
            Some(Handle::MoveY),
        ),
        (
            "gizmo-move-active-x",
            Tool::Move,
            Some(Handle::MoveX),
            Some(Readout::Distance(12.5)),
            Some(Handle::MoveX),
        ),
        ("gizmo-rotate", Tool::Rotate, None, None, None),
        (
            "gizmo-scale-active",
            Tool::Scale,
            Some(Handle::ScaleAll),
            Some(Readout::Factor(1.25)),
            Some(Handle::ScaleAll),
        ),
    ];
    for (name, tool, active, readout, hover) in cases {
        let mut probe_input = input(tool);
        probe_input.active = active;
        probe_input.readout = readout;
        let mut harness = Harness::builder()
            .with_size(egui::vec2(720.0, 540.0))
            .with_pixels_per_point(1.0)
            .wgpu()
            .with_accessibility_check(false)
            .allow_missing_glyphs()
            .with_context_setup(|ctx| {
                model_reviewer::fonts::install(ctx);
                model_reviewer::theme::install(ctx);
            })
            .build_ui_state(
                |ui, probe: &mut Probe| {
                    ui.painter()
                        .rect_filled(probe.input.rect, 0.0, model_reviewer::theme::VIEW_BG);
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(egui::Id::NULL, WidgetFrame::default()));
                    gizmo_view::show(ui, &probe.input, &mut probe.out);
                },
                Probe {
                    input: probe_input,
                    out: Vec::new(),
                    view_drags: 0,
                },
            );
        harness.step();
        if let Some(handle) = hover {
            let pos = widget(&harness, widget_name(handle)).center();
            harness.event(egui::Event::PointerMoved(pos));
        }
        for _ in 0..3 {
            harness.step();
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-check");
        std::fs::create_dir_all(&dir).expect("画像の出力先");
        harness
            .render()
            .expect("描画")
            .save(dir.join(format!("{name}.png")))
            .expect("PNGの保存");
    }
}
