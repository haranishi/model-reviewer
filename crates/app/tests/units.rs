//! 表示の単位の接合。換算する場所（design-stl-units.md の表）と、この試験の項目を1対1にする:
//! ①計測の距離と差、②断面の位置、③床の格子の間隔、④移動のハンドルの変化量。
//! 寸法と外形は、右のパネルの「寸法」を置く次の接合で足す。換算しないもの: 「変形」の欄、文書の
//! 数値、自動操作の既存の項目、履歴と未保存の印。GPUは使わない。

mod support;

use glam::Vec3;
use model_reviewer::command::{Command, Tool};
use model_reviewer::session::{Session, measure_distance};
use model_reviewer::ui::gizmo_view::{readout_text, readout_text_with};
use model_reviewer::ui::status::{display_unit_text, grid_label};
use model_reviewer::ui::{ids, inspector_input, measure_panel, measure_panel_input, viewport};
use model_reviewer_core::gizmo::Readout;
use model_reviewer_core::scene::{NodeId, SceneRead};
use model_reviewer_core::section::{Axis, SectionPlane};
use model_reviewer_core::units::{UnitLabel, Units, format_distance, format_length};
use support::{NEAR_CENTER, ndc_of, near};

const RECT: egui::Rect = egui::Rect {
    min: egui::pos2(0.0, 0.0),
    max: egui::pos2(800.0, 800.0),
};

/// 角から角を測った状態（距離 2、差 (−2, 0, 0)）。
fn measured() -> Session {
    let mut session = support::session();
    session.apply(Command::SetTool(Tool::Measure));
    for (corner, dx) in [
        (Vec3::new(1.0, 1.0, 3.0), -4.0),
        (Vec3::new(-1.0, 1.0, 3.0), 4.0),
    ] {
        session.apply(Command::ViewClick {
            ndc: ndc_of(&session, corner, dx, -4.0),
        });
    }
    assert!(measure_distance(&session).is_some());
    session
}

#[test]
fn display_same_as_file_changes_nothing() {
    let session = measured();
    assert_eq!(
        session.units,
        Units {
            source: UnitLabel::M,
            display: None
        }
    );
    let distance = measure_distance(&session).unwrap();
    // ①計測: 3Dビューの線の脇・状態欄・右のパネル。数値は1ビットも変わらず、文字は第2段階と同じ。
    let (input, _) = viewport::measure_input(&session, RECT);
    assert_eq!(input.distance.unwrap().to_bits(), distance.to_bits());
    assert_eq!(input.unit, UnitLabel::M);
    assert_eq!(
        model_reviewer::ui::measure_view::label_text(&input).unwrap(),
        format_distance(distance, UnitLabel::M)
    );
    assert_eq!(
        session.units.format(distance, 4),
        format_distance(distance, UnitLabel::M)
    );
    let panel = measure_panel_input(&session);
    let rows = measure_panel::rows(&panel);
    assert_eq!(rows[0].1, format_length(distance, UnitLabel::M, 6));
    // ②断面の位置の欄は、シーンの値のまま（換算しない）。
    assert_eq!(session.units.to_display(0.25).to_bits(), 0.25_f32.to_bits());
    // ③格子は今までどおり「格子 0.1」。
    assert_eq!(grid_label(0.1, session.units), "格子 0.1");
    assert_eq!(display_unit_text(session.units), "表示 ファイルと同じ");
    // ④移動のハンドルの変化量は、単位を添えない今までの文字。
    let readout = Readout::Distance(0.25);
    assert_eq!(readout_text_with(readout, None), readout_text(readout));
    assert_eq!(readout_text(readout), "+0.2500");
}

#[test]
fn display_mm_converts_readouts_only() {
    let mut session = measured();
    let distance = measure_distance(&session).unwrap();
    let doc_before: Vec<_> = session
        .doc
        .as_ref()
        .unwrap()
        .nodes()
        .iter()
        .map(|node| node.local)
        .collect();
    session.apply(Command::Select(Some(NodeId(0))));
    let inspector_before = inspector_input(&session);
    session.apply(Command::SetDisplayUnit(Some(UnitLabel::Mm)));
    let units = session.units;
    assert_eq!(units.shown(), UnitLabel::Mm);

    // ①計測の距離と差: ×1000 で、単位は mm。
    let (input, _) = viewport::measure_input(&session, RECT);
    assert!(near(input.distance.unwrap(), distance * 1000.0));
    assert_eq!(input.unit, UnitLabel::Mm);
    assert!(
        model_reviewer::ui::measure_view::label_text(&input)
            .unwrap()
            .ends_with(" mm")
    );
    let rows = measure_panel::rows(&measure_panel_input(&session));
    let row = |name: &str| rows.iter().find(|(n, _)| *n == name).unwrap().1.clone();
    assert_eq!(row("距離"), "2000.00 mm");
    assert_eq!(row("ΔX"), "-2000.00 mm");
    assert_eq!(row("ΔY"), "0.00000 mm");
    // ②断面の位置: 欄は表示の単位で出し、受け取った値はシーンの値へ戻す。
    let bar = model_reviewer::ui::section_bar::SectionBarInput {
        plane: SectionPlane {
            axis: Axis::Z,
            offset: 0.0,
            flipped: false,
        },
        bounds: session.doc.as_ref().unwrap().bounds(),
        units,
    };
    let plane = model_reviewer::ui::section_bar::with_offset(&bar, 1500.0);
    assert!(near(plane.offset, 1.5), "{}", plane.offset);
    // ③格子の間隔: 0.1 m は「格子 100 mm」。
    assert_eq!(grid_label(0.1, units), "格子 100 mm");
    assert_eq!(display_unit_text(units), "表示 mm");
    // ④移動のハンドルの変化量: 換算した値に単位を添える。
    let shown = Readout::Distance(units.to_display(0.25));
    assert_eq!(readout_text_with(shown, Some("mm")), "+250.0 mm");
    // 角度と倍率には単位を添えない。
    assert_eq!(
        readout_text_with(Readout::AngleDeg(15.0), Some("mm")),
        readout_text(Readout::AngleDeg(15.0))
    );

    // 換算しないもの: 計測の数値（シーンの値）、「変形」の欄、文書、履歴と未保存の印。
    assert_eq!(
        measure_distance(&session).unwrap().to_bits(),
        distance.to_bits()
    );
    assert_eq!(inspector_input(&session), inspector_before);
    let doc_after: Vec<_> = session
        .doc
        .as_ref()
        .unwrap()
        .nodes()
        .iter()
        .map(|node| node.local)
        .collect();
    assert_eq!(doc_after, doc_before);
    assert!(session.history.is_empty());
    assert!(!session.has_unsaved());

    // 表示は ファイルと同じ → mm → cm → m → ファイルと同じ。
    assert_eq!(units.next_display(), Some(UnitLabel::Cm));
    session.apply(Command::SetDisplayUnit(None));
    assert_eq!(session.units.display, None);
}

/// 表示の単位を選んでいても、前提の単位を替えると換算の元が変わる（GLB の数値は変えない）。
#[test]
fn source_unit_is_the_premise_of_the_conversion() {
    let mut session = measured();
    let distance = measure_distance(&session).unwrap();
    session.apply(Command::SetUnit(UnitLabel::Cm));
    session.apply(Command::SetDisplayUnit(Some(UnitLabel::Mm)));
    let (input, _) = viewport::measure_input(&session, RECT);
    assert!(near(input.distance.unwrap(), distance * 10.0));
    assert_eq!(
        measure_distance(&session).unwrap().to_bits(),
        distance.to_bits()
    );
}

/// モデルを開くと、前提は m・表示は「ファイルと同じ」に戻る（要件7.5）。
#[test]
fn opening_a_glb_resets_the_units() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("units-reopen");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("triangle.glb");
    std::fs::write(&path, model_reviewer_testdata::inputs::triangle()).unwrap();
    let mut session = support::session();
    session.apply(Command::SetUnit(UnitLabel::Mm));
    session.apply(Command::SetDisplayUnit(Some(UnitLabel::Cm)));
    session.apply(Command::ToggleSection);
    session.apply(Command::OpenModel(path));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while session.model_ref.as_ref().map(|m| m.file_name.as_str()) != Some("triangle.glb") {
        session.poll_loader();
        assert!(
            std::time::Instant::now() < deadline,
            "{}",
            session.status_text
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(session.units, Units::default());
    assert_eq!(session.section, None);
}

/// 状態欄の「表示 …」を押すと表示の単位が替わり、距離の文字と格子が換算される。
#[test]
fn status_display_unit_button_cycles() {
    let session = measured();
    let (mut harness, _) = support::harness(session, egui::vec2(1280.0, 800.0));
    for _ in 0..4 {
        harness.step();
    }
    let distance = measure_distance(harness.state()).unwrap();
    for expected in [
        Some(UnitLabel::Mm),
        Some(UnitLabel::Cm),
        Some(UnitLabel::M),
        None,
    ] {
        let button = support::widget_rect(&harness, ids::STATUS_DISPLAY_UNIT).expect("表示の単位");
        support::click(&mut harness, button.center());
        assert_eq!(harness.state().units.display, expected);
        let units = harness.state().units;
        let text = format!("距離 {}", units.format(distance, 4));
        use egui_kittest::kittest::Queryable;
        assert!(harness.query_by_label(&text).is_some(), "{text}");
        assert!(harness.query_by_label(&display_unit_text(units)).is_some());
        assert!(!harness.state().has_unsaved());
    }
    let _ = NEAR_CENTER;
}
