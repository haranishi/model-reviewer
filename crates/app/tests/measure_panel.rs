//! 右のパネルの「計測」。距離・軸ごとの差・2点の種類を表示の単位で出し、「消す」で
//! `ClearMeasure` を積む。入力用の構造体だけで動く。

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use glam::Vec3;
use model_reviewer::automation::WidgetFrame;
use model_reviewer::command::{Command, UnitLabel};
use model_reviewer::ui::ids;
use model_reviewer::ui::measure_panel::{
    self, MeasurePanelInput, NO_POINTS, ONE_POINT, kind_name, rows,
};
use model_reviewer_core::snap::SnapKind;
use model_reviewer_core::units::Units;

struct Probe {
    input: MeasurePanelInput,
    out: Vec<Command>,
}

fn two_points(units: Units) -> MeasurePanelInput {
    MeasurePanelInput {
        distance: Some(2.5),
        delta: Some(Vec3::new(0.5, -2.0, 1024.0)),
        points: 2,
        kinds: [SnapKind::Vertex, SnapKind::Edge],
        units,
    }
}

fn harness(input: MeasurePanelInput) -> Harness<'static, Probe> {
    Harness::builder()
        .with_size(egui::vec2(280.0, 240.0))
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
                let mut out = Vec::new();
                measure_panel::show(ui, &probe.input, &mut out);
                probe.out.extend(out);
            },
            Probe {
                input,
                out: Vec::new(),
            },
        )
}

fn widget(harness: &Harness<'static, Probe>, name: &str) -> egui::Rect {
    let frame = harness
        .ctx
        .data(|data| data.get_temp::<WidgetFrame>(egui::Id::NULL))
        .unwrap_or_default();
    let entry = frame
        .entries()
        .iter()
        .find(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("{name} が登録されていない"));
    egui::Rect::from_min_max(entry.min.into(), entry.max.into())
}

fn click(harness: &mut Harness<'static, Probe>, name: &str) -> Vec<Command> {
    harness.state_mut().out.clear();
    let pos = widget(harness, name).center();
    harness.event(egui::Event::PointerMoved(pos));
    harness.step();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.step();
    }
    harness.step();
    std::mem::take(&mut harness.state_mut().out)
}

#[test]
fn kinds_have_japanese_names() {
    assert_eq!(kind_name(SnapKind::Vertex), "頂点");
    assert_eq!(kind_name(SnapKind::Edge), "辺");
    assert_eq!(kind_name(SnapKind::Face), "面");
}

#[test]
fn rows_show_distance_signed_deltas_and_kinds_with_six_digits() {
    let rows = rows(&two_points(Units::default()));
    assert_eq!(
        rows,
        vec![
            ("距離", "2.50000 m".to_owned()),
            ("ΔX", "0.500000 m".to_owned()),
            ("ΔY", "-2.00000 m".to_owned()),
            ("ΔZ", "1024.00 m".to_owned()),
            ("1点目", "頂点".to_owned()),
            ("2点目", "辺".to_owned()),
        ]
    );
    // 表示の単位を mm にすると換算される（数値は2進で正確に表せる値）。
    let mm = Units {
        source: UnitLabel::M,
        display: Some(UnitLabel::Mm),
    };
    let rows_mm = measure_panel::rows(&two_points(mm));
    assert_eq!(rows_mm[0], ("距離", "2500.00 mm".to_owned()));
    assert_eq!(rows_mm[1], ("ΔX", "500.000 mm".to_owned()));
}

#[test]
fn missing_or_non_finite_values_show_a_dash() {
    let input = MeasurePanelInput {
        distance: Some(f32::NAN),
        delta: None,
        ..two_points(Units::default())
    };
    let rows = rows(&input);
    for (name, value) in &rows[..4] {
        assert_eq!(value, "— m", "{name}");
    }
    let one = MeasurePanelInput {
        distance: None,
        delta: None,
        points: 1,
        kinds: [SnapKind::Face, SnapKind::Face],
        units: Units::default(),
    };
    assert_eq!(measure_panel::rows(&one), vec![("1点目", "面".to_owned())]);
}

#[test]
fn the_panel_shows_values_and_clear_emits_clear_measure() {
    let mut harness = harness(two_points(Units::default()));
    harness.step();
    for label in [
        "計測",
        "距離",
        "2.50000 m",
        "ΔX",
        "ΔY",
        "-2.00000 m",
        "ΔZ",
        "1点目",
        "頂点",
        "2点目",
        "辺",
    ] {
        harness.get_by_label(label);
    }
    assert!(harness.state().out.is_empty(), "見るだけでは何も積まない");
    assert!(widget(&harness, ids::MEASURE_CLEAR).height() >= 24.0);
    assert_eq!(
        click(&mut harness, ids::MEASURE_CLEAR),
        vec![Command::ClearMeasure]
    );
}

#[test]
fn without_points_the_panel_explains_and_clear_is_disabled() {
    let mut harness = harness(MeasurePanelInput {
        distance: None,
        delta: None,
        points: 0,
        kinds: [SnapKind::Face; 2],
        units: Units::default(),
    });
    harness.step();
    harness.get_by_label(NO_POINTS);
    assert!(harness.get_by_label("消す").accesskit_node().is_disabled());
    assert!(click(&mut harness, ids::MEASURE_CLEAR).is_empty());

    harness.state_mut().input = MeasurePanelInput {
        points: 1,
        ..harness.state().input
    };
    harness.step();
    harness.get_by_label(ONE_POINT);
    assert_eq!(
        click(&mut harness, ids::MEASURE_CLEAR),
        vec![Command::ClearMeasure]
    );
}
