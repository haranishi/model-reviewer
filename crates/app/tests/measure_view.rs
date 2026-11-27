//! 計測の線と距離の文字。入力用の構造体だけで動き、単位を替えても数値は変わらない。

use egui_kittest::Harness;
use model_reviewer::command::UnitLabel;
use model_reviewer::ui::measure_view::{self, MeasureViewInput, label_anchor, label_text};

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
