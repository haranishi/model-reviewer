//! 計測の線と距離の文字。入力用の構造体だけで動き、単位を替えても数値は変わらない。

use egui_kittest::Harness;
use model_reviewer::command::UnitLabel;
use model_reviewer::ui::measure_view::{
    self, FADED_OPACITY, MarkShape, MeasureViewInput, SnapCandidate, SnapMarks, candidate_mark,
    label_anchor, label_text, mark_shape, point_opacity,
};
use model_reviewer_core::snap::SnapKind;

fn input(points: Vec<egui::Pos2>, distance: Option<f32>, unit: UnitLabel) -> MeasureViewInput {
    MeasureViewInput {
        points,
        distance,
        unit,
    }
}

#[test]
fn label_needs_two_points_and_a_distance() {
    let a = egui::pos2(10.0, 10.0);
    let b = egui::pos2(110.0, 10.0);
    assert_eq!(label_text(&input(vec![], None, UnitLabel::M)), None);
    assert_eq!(label_text(&input(vec![a], Some(1.0), UnitLabel::M)), None);
    assert_eq!(label_text(&input(vec![a, b], None, UnitLabel::M)), None);
    assert_eq!(
        label_text(&input(vec![a, b], Some(2.5), UnitLabel::M)).as_deref(),
        Some("2.500 m")
    );
}

#[test]
fn changing_the_unit_changes_only_the_suffix() {
    let points = vec![egui::pos2(0.0, 0.0), egui::pos2(50.0, 40.0)];
    let texts: Vec<String> = [UnitLabel::M, UnitLabel::Cm, UnitLabel::Mm]
        .into_iter()
        .map(|unit| label_text(&input(points.clone(), Some(12.3456), unit)).unwrap())
        .collect();
    assert_eq!(texts, ["12.35 m", "12.35 cm", "12.35 mm"]);
}

#[test]
fn label_sits_beside_the_line_and_above_it() {
    let size = egui::vec2(80.0, 20.0);
    let a = egui::pos2(0.0, 100.0);
    let b = egui::pos2(200.0, 100.0);
    let anchor = label_anchor(a, b, size);
    assert!((anchor.x - 100.0).abs() < 1.0e-3);
    assert!(
        anchor.y + size.y * 0.5 < 100.0,
        "水平な線では上へ、線に重ならない: {anchor:?}"
    );
    // 線の向きを逆にしても同じ側。
    assert_eq!(label_anchor(b, a, size), anchor);
    // 縦の線では右へ、箱の幅の分だけずれる。
    let top = egui::pos2(50.0, 0.0);
    let bottom = egui::pos2(50.0, 100.0);
    let side = label_anchor(top, bottom, size);
    assert!((side.y - 50.0).abs() < 1.0e-3);
    assert!(side.x - size.x * 0.5 > 50.0, "{side:?}");
    assert_eq!(label_anchor(bottom, top, size), side);
    // 斜めの線でも、箱の角が線に触れない。
    let c = egui::pos2(0.0, 100.0);
    let d = egui::pos2(100.0, 0.0);
    let center = label_anchor(c, d, size);
    let rect = egui::Rect::from_center_size(center, size);
    for corner in [
        rect.left_top(),
        rect.right_top(),
        rect.left_bottom(),
        rect.right_bottom(),
    ] {
        // 線 x + y = 100 より上（x + y < 100）にある。
        assert!(corner.x + corner.y < 100.0, "{corner:?}");
    }
    // 同じ点（長さ0）でも有限。
    assert!(label_anchor(a, a, size).is_finite());
}

struct Probe {
    input: MeasureViewInput,
}

fn harness(input: MeasureViewInput) -> Harness<'static, Probe> {
    Harness::builder()
        .with_size(egui::vec2(320.0, 200.0))
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_context_setup(|ctx| {
            model_reviewer::fonts::install(ctx);
            model_reviewer::theme::install(ctx);
        })
        .build_ui_state(
            |ui, probe: &mut Probe| measure_view::show(ui, &probe.input),
            Probe { input },
        )
}

#[test]
fn draws_with_zero_one_and_two_points_and_odd_values() {
    let a = egui::pos2(40.0, 150.0);
    let b = egui::pos2(260.0, 60.0);
    let mut harness = harness(input(vec![], None, UnitLabel::M));
    harness.step();
    for next in [
        input(vec![a], None, UnitLabel::M),
        input(vec![a, b], Some(2.5), UnitLabel::Cm),
        input(vec![a, b], Some(f32::NAN), UnitLabel::Mm),
        input(vec![a, egui::pos2(f32::NAN, 0.0)], Some(1.0), UnitLabel::M),
        input(vec![a, a], Some(0.0), UnitLabel::M),
    ] {
        harness.state_mut().input = next;
        harness.step();
    }
}

#[test]
fn snap_marker_names_the_kind() {
    let at = egui::pos2(100.0, 80.0);
    let candidate = |kind| SnapMarks {
        candidate: Some(SnapCandidate {
            pos: at,
            kind,
            edge: None,
        }),
        ..SnapMarks::default()
    };
    assert_eq!(
        candidate_mark(&candidate(SnapKind::Vertex)),
        Some((MarkShape::Square, "頂点"))
    );
    assert_eq!(
        candidate_mark(&candidate(SnapKind::Edge)),
        Some((MarkShape::Diamond, "辺"))
    );
    assert_eq!(
        candidate_mark(&candidate(SnapKind::Face)),
        Some((MarkShape::Circle, "面"))
    );
    // 形は3つとも違う（色だけで見分けさせない）。
    let shapes = [SnapKind::Vertex, SnapKind::Edge, SnapKind::Face].map(mark_shape);
    assert!(shapes[0] != shapes[1] && shapes[1] != shapes[2] && shapes[0] != shapes[2]);
    assert_eq!(candidate_mark(&SnapMarks::default()), None);
    let mut off_screen = candidate(SnapKind::Vertex);
    off_screen.candidate.as_mut().unwrap().pos = egui::pos2(f32::NAN, 0.0);
    assert_eq!(candidate_mark(&off_screen), None);
}

#[test]
fn points_on_the_cut_side_are_faded() {
    let marks = SnapMarks {
        faded: [false, true],
        ..SnapMarks::default()
    };
    assert_eq!(point_opacity(&marks, 0), 1.0);
    assert_eq!(point_opacity(&marks, 1), FADED_OPACITY);
    assert_eq!(point_opacity(&marks, 5), 1.0);
    assert_eq!(FADED_OPACITY, 0.4);
}

#[test]
fn draws_candidates_and_faded_points_without_panicking() {
    struct Marked {
        input: MeasureViewInput,
        marks: SnapMarks,
    }
    let a = egui::pos2(40.0, 150.0);
    let b = egui::pos2(260.0, 60.0);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(320.0, 200.0))
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_context_setup(|ctx| {
            model_reviewer::fonts::install(ctx);
            model_reviewer::theme::install(ctx);
        })
        .build_ui_state(
            |ui, probe: &mut Marked| measure_view::show_with(ui, &probe.input, &probe.marks),
            Marked {
                input: input(vec![], None, UnitLabel::M),
                marks: SnapMarks::default(),
            },
        );
    harness.step();
    for kind in [SnapKind::Vertex, SnapKind::Edge, SnapKind::Face] {
        for edge in [None, Some([a, b]), Some([a, egui::pos2(f32::NAN, 1.0)])] {
            harness.state_mut().marks = SnapMarks {
                candidate: Some(SnapCandidate {
                    pos: egui::pos2(150.0, 100.0),
                    kind,
                    edge,
                }),
                point_kinds: [kind, SnapKind::Face],
                faded: [true, false],
            };
            harness.state_mut().input = input(vec![a, b], Some(2.5), UnitLabel::Mm);
            harness.step();
            harness.state_mut().input = input(vec![a], None, UnitLabel::Mm);
            harness.step();
        }
    }
}

#[test]
#[ignore = "wgpuで画面をPNGへ書き出す"]
fn render_measure_for_visual_check() {
    let cases = [
        (
            "measure-two",
            input(
                vec![egui::pos2(40.0, 150.0), egui::pos2(260.0, 60.0)],
                Some(2.5),
                UnitLabel::M,
            ),
        ),
        (
            "measure-one",
            input(vec![egui::pos2(160.0, 100.0)], None, UnitLabel::Cm),
        ),
        (
            "measure-vertical-mm",
            input(
                vec![egui::pos2(160.0, 20.0), egui::pos2(160.0, 180.0)],
                Some(0.012345),
                UnitLabel::Mm,
            ),
        ),
    ];
    for (name, input) in cases {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(320.0, 200.0))
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
                    // 明るい面の上でも見えるかを確かめるため、右半分を明るくする。
                    let rect = ui.max_rect();
                    ui.painter()
                        .rect_filled(rect, 0.0, model_reviewer::theme::VIEW_BG);
                    let right = egui::Rect::from_min_max(rect.center_top(), rect.max);
                    ui.painter()
                        .rect_filled(right, 0.0, egui::Color32::from_gray(200));
                    measure_view::show(ui, &probe.input);
                },
                Probe { input },
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

#[test]
#[ignore = "wgpuで画面をPNGへ書き出す"]
fn render_snap_marks_for_visual_check() {
    struct Marked {
        input: MeasureViewInput,
        marks: SnapMarks,
    }
    let a = egui::pos2(60.0, 150.0);
    let b = egui::pos2(260.0, 60.0);
    for (name, kind) in [
        ("snap-vertex", SnapKind::Vertex),
        ("snap-edge", SnapKind::Edge),
        ("snap-face", SnapKind::Face),
    ] {
        let marks = SnapMarks {
            candidate: Some(SnapCandidate {
                pos: egui::pos2(160.0, 105.0),
                kind,
                edge: (kind == SnapKind::Edge)
                    .then_some([egui::pos2(110.0, 130.0), egui::pos2(210.0, 80.0)]),
            }),
            point_kinds: [SnapKind::Vertex, SnapKind::Edge],
            faded: [false, true],
        };
        let mut harness = Harness::builder()
            .with_size(egui::vec2(320.0, 200.0))
            .with_pixels_per_point(2.0)
            .wgpu()
            .with_accessibility_check(false)
            .allow_missing_glyphs()
            .with_context_setup(|ctx| {
                model_reviewer::fonts::install(ctx);
                model_reviewer::theme::install(ctx);
            })
            .build_ui_state(
                |ui, probe: &mut Marked| {
                    let rect = ui.max_rect();
                    ui.painter()
                        .rect_filled(rect, 0.0, model_reviewer::theme::VIEW_BG);
                    let right = egui::Rect::from_min_max(rect.center_top(), rect.max);
                    ui.painter()
                        .rect_filled(right, 0.0, egui::Color32::from_gray(200));
                    measure_view::show_with(ui, &probe.input, &probe.marks);
                },
                Marked {
                    input: input(vec![a, b], Some(0.0254), UnitLabel::M),
                    marks,
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
