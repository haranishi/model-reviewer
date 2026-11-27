//! 断面の枠と矢印。ポインタが矢印の当たりに入ったときだけ受け、掴んだら3Dビュー全体で動きを
//! 受ける。入力用の構造体だけで動く。

use egui_kittest::Harness;
use glam::{Vec2, Vec3};
use model_reviewer::automation::WidgetFrame;
use model_reviewer::command::{Axis, Command, SectionPlane};
use model_reviewer::ui::ids;
use model_reviewer::ui::section_view::{
    self, ARROW_HALF, HIT_TOLERANCE, SectionViewInput, arrow_center, arrow_segment, grab_point,
    hits_arrow,
};
use model_reviewer_core::camera::{OrbitCamera, Projection};
use model_reviewer_core::math::Aabb;
use model_reviewer_core::section::drag_offset;

fn view_rect() -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(40.0, 30.0), egui::vec2(640.0, 480.0))
}

fn bounds() -> Aabb {
    Aabb {
        min: Vec3::splat(-1.0),
        max: Vec3::splat(1.0),
    }
}

fn camera(projection: Projection) -> OrbitCamera {
    OrbitCamera {
        target: Vec3::ZERO,
        yaw: 35_f32.to_radians(),
        pitch: 20_f32.to_radians(),
        distance: 6.0,
        projection,
        ..OrbitCamera::default()
    }
}

fn input(axis: Axis, projection: Projection) -> SectionViewInput {
    SectionViewInput {
        plane: SectionPlane::centered(axis, &bounds()),
        bounds: bounds(),
        camera: camera(projection),
        rect: view_rect(),
        grabbing: false,
    }
}

struct Probe {
    input: SectionViewInput,
    out: Vec<Command>,
    view_drags: usize,
}

fn harness(input: SectionViewInput) -> Harness<'static, Probe> {
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
                section_view::show(ui, &probe.input, &mut out);
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

fn to_ndc(pos: egui::Pos2) -> Vec2 {
    let rect = view_rect();
    Vec2::new(
        (pos.x - rect.center().x) / (rect.width() * 0.5),
        (rect.center().y - pos.y) / (rect.height() * 0.5),
    )
}

#[test]
fn arrow_sits_on_the_plane_and_has_a_fixed_screen_length() {
    for projection in [Projection::Perspective, Projection::Orthographic] {
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            let input = input(axis, projection);
            let center = arrow_center(&input.plane, &input.bounds);
            assert!(center.abs().max_element() < 1.0e-6, "{center:?}");
            let [tip, tail] = arrow_segment(&input).expect("斜めから見れば矢印が出る");
            assert!(
                (tip.distance(tail) - 2.0 * ARROW_HALF).abs() < 1.0e-2,
                "{axis:?} {projection:?}"
            );
            assert!(view_rect().contains(tip) && view_rect().contains(tail));
            let grab = grab_point(&input).expect("掴める点");
            assert!(hits_arrow(&input, grab));
            assert!(!hits_arrow(
                &input,
                grab + (tip - tail).normalized().rot90() * (HIT_TOLERANCE * 2.0)
            ));
        }
    }
    // 中心は平面の位置へ移る。
    let moved = SectionPlane {
        offset: 0.25,
        ..SectionPlane::centered(Axis::Y, &bounds())
    };
    let center = arrow_center(&moved, &bounds());
    assert!((center.y - 0.25).abs() < 1.0e-6);
}

#[test]
fn looking_along_the_axis_hides_the_arrow() {
    let mut input = input(Axis::Z, Projection::Orthographic);
    // 正面（-Z 向きの視線）から見ると Z 軸は点に潰れる。
    input.camera.yaw = 0.0;
    input.camera.pitch = 0.0;
    assert_eq!(arrow_segment(&input), None);
    let mut harness = harness(input);
    harness.step();
    assert!(
        registered(&harness)
            .iter()
            .all(|(name, _)| name != ids::SECTION_HANDLE)
    );
}

#[test]
fn handle_is_registered_even_when_the_pointer_is_away() {
    let mut harness = harness(input(Axis::Y, Projection::Perspective));
    harness.step();
    let handle = registered(&harness)
        .into_iter()
        .find(|(name, _)| name == ids::SECTION_HANDLE)
        .expect("矢印の名前が登録される")
        .1;
    assert!(hits_arrow(&harness.state().input, handle.center()));
}

#[test]
fn pressing_off_the_arrow_goes_to_the_view() {
    let mut harness = harness(input(Axis::Y, Projection::Perspective));
    harness.step();
    let far = view_rect().left_top() + egui::vec2(20.0, 20.0);
    assert!(!hits_arrow(&harness.state().input, far));
    pointer(&mut harness, far, None);
    pointer(&mut harness, far, Some(true));
    pointer(&mut harness, far + egui::vec2(30.0, 10.0), None);
    pointer(&mut harness, far + egui::vec2(60.0, 20.0), Some(false));
    assert!(harness.state().out.is_empty(), "{:?}", harness.state().out);
    assert!(harness.state().view_drags > 0, "3Dビューの操作域が受ける");
}

#[test]
fn dragging_the_arrow_sets_the_section_and_escape_restores() {
    let input = input(Axis::Y, Projection::Perspective);
    let mut harness = harness(input);
    harness.step();
    let grab = grab_point(&input).expect("掴める点");
    let [tip, tail] = arrow_segment(&input).expect("矢印");
    let along = (tip - tail).normalized();
    pointer(&mut harness, grab, None);
    pointer(&mut harness, grab, Some(true));
    let moved = grab + along * 30.0;
    pointer(&mut harness, grab + along * 15.0, None);
    pointer(&mut harness, moved, None);
    assert_eq!(
        harness.state().view_drags,
        0,
        "矢印を掴んだら3Dビューは動かない"
    );
    let out = std::mem::take(&mut harness.state_mut().out);
    // いまの位置はコアの `drag_offset` が求める（同じ関数で期待値を作る）。
    let aspect = view_rect().width() / view_rect().height();
    let start_ray = input
        .camera
        .screen_ray_in(to_ndc(grab), aspect, &input.bounds);
    let now_ray = input
        .camera
        .screen_ray_in(to_ndc(moved), aspect, &input.bounds);
    match drag_offset(&input.plane, &input.bounds, &start_ray, &now_ray) {
        Some(offset) => {
            let Some(Command::SetSection(Some(plane))) = out.last() else {
                panic!("ドラッグで位置が送られる: {out:?}");
            };
            assert_eq!(plane.axis, Axis::Y);
            assert!(!plane.flipped);
            let expected = offset.clamp(-1.0, 1.0);
            assert!((plane.offset - expected).abs() < 1.0e-4, "{plane:?}");
        }
        None => assert!(out.is_empty(), "位置が求まらない間は何も積まない: {out:?}"),
    }
    assert!(
        out.iter()
            .all(|c| matches!(c, Command::SetSection(Some(p)) if p.axis == Axis::Y)),
        "{out:?}"
    );
    harness.key_press(egui::Key::Escape);
    pointer(&mut harness, moved + along * 10.0, None);
    pointer(&mut harness, moved + along * 10.0, Some(false));
    harness.step();
    let out = std::mem::take(&mut harness.state_mut().out);
    assert_eq!(
        out,
        vec![Command::SetSection(Some(input.plane))],
        "Esc で掴んだときの平面へ戻し、そのあとは何も積まない"
    );
}

#[test]
fn odd_inputs_do_not_panic() {
    let mut harness = harness(input(Axis::X, Projection::Perspective));
    harness.step();
    let cases = [
        SectionViewInput {
            bounds: Aabb::empty(),
            ..input(Axis::X, Projection::Perspective)
        },
        SectionViewInput {
            rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(0.0, 0.0)),
            ..input(Axis::Z, Projection::Perspective)
        },
        SectionViewInput {
            plane: SectionPlane {
                axis: Axis::Z,
                offset: f32::NAN,
                flipped: true,
            },
            ..input(Axis::Z, Projection::Orthographic)
        },
        SectionViewInput {
            grabbing: true,
            ..input(Axis::Y, Projection::Orthographic)
        },
    ];
    for case in cases {
        harness.state_mut().input = case;
        harness.step();
    }
}

#[test]
#[ignore = "wgpuで画面をPNGへ書き出す"]
fn render_section_view_for_visual_check() {
    let cases = [
        ("section-view-y", input(Axis::Y, Projection::Perspective)),
        (
            "section-view-x-ortho",
            input(Axis::X, Projection::Orthographic),
        ),
    ];
    for (name, input) in cases {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(720.0, 540.0))
            .with_pixels_per_point(2.0)
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
                        .rect_filled(ui.max_rect(), 0.0, model_reviewer::theme::VIEW_BG);
                    section_view::show(ui, &probe.input, &mut probe.out);
                },
                Probe {
                    input,
                    out: Vec::new(),
                    view_drags: 0,
                },
            );
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
