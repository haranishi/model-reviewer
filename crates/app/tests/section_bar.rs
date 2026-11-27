//! 断面の操作の帯。入力用の構造体だけで動き、押すと `SetSection(Some(plane))` を積む。

use egui::accesskit::Toggled;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use glam::Vec3;
use model_reviewer::automation::WidgetFrame;
use model_reviewer::command::{Axis, Command, SectionPlane, UnitLabel};
use model_reviewer::ui::ids;
use model_reviewer::ui::section_bar::{self, SectionBarInput, with_axis};
use model_reviewer_core::math::Aabb;
use model_reviewer_core::units::Units;

struct Probe {
    input: SectionBarInput,
    out: Vec<Command>,
}

fn bounds() -> Aabb {
    Aabb {
        min: Vec3::new(-1.0, 2.0, -4.0),
        max: Vec3::new(3.0, 6.0, 0.0),
    }
}

fn input(units: Units) -> SectionBarInput {
    SectionBarInput {
        plane: SectionPlane::centered(Axis::Z, &bounds()),
        bounds: bounds(),
        units,
    }
}

fn harness(input: SectionBarInput) -> Harness<'static, Probe> {
    Harness::builder()
        .with_size(egui::vec2(560.0, 120.0))
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
                section_bar::show(ui, &probe.input, &mut out);
                probe.out.extend(out);
            },
            Probe {
                input,
                out: Vec::new(),
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

fn click(harness: &mut Harness<'static, Probe>, name: &str) -> Vec<Command> {
    harness.state_mut().out.clear();
    let pos = widget(harness, name).center();
    pointer(harness, pos, None);
    pointer(harness, pos, Some(true));
    pointer(harness, pos, Some(false));
    harness.step();
    std::mem::take(&mut harness.state_mut().out)
}

fn near(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() <= 1.0e-6 + expected.abs() * 1.0e-5
}

fn only_plane(out: &[Command]) -> SectionPlane {
    match out {
        [Command::SetSection(Some(plane))] => *plane,
        _ => panic!("SetSection が1つだけのはず: {out:?}"),
    }
}

#[test]
fn every_part_is_registered_and_labelled() {
    let mut harness = harness(input(Units::default()));
    harness.step();
    for name in [
        ids::SECTION_AXIS_X,
        ids::SECTION_AXIS_Y,
        ids::SECTION_AXIS_Z,
        ids::SECTION_OFFSET,
        ids::SECTION_FLIP,
    ] {
        let rect = widget(&harness, name);
        assert!(rect.height() >= 24.0, "{name}: {rect:?}");
    }
    for label in ["断面", "位置", "X", "Y", "Z", "反転"] {
        harness.get_by_label(label);
    }
    // 選ばれている軸が示される。
    assert_eq!(
        harness.get_by_label("Z").accesskit_node().toggled(),
        Some(Toggled::True)
    );
    assert_eq!(
        harness.get_by_label("X").accesskit_node().toggled(),
        Some(Toggled::False)
    );
    assert!(harness.state().out.is_empty(), "見るだけでは何も積まない");
}

#[test]
fn section_bar_emits_set_section() {
    let mut harness = harness(input(Units::default()));
    harness.step();
    // 反転。
    let flipped = only_plane(&click(&mut harness, ids::SECTION_FLIP));
    assert!(flipped.flipped);
    assert_eq!(flipped.axis, Axis::Z);
    assert_eq!(flipped.offset.to_bits(), (-2.0_f32).to_bits());
    // いま選ばれている軸を押しても何も積まない。
    assert!(click(&mut harness, ids::SECTION_AXIS_Z).is_empty());
    // 位置を打って Enter。表示とファイルの単位が同じなら値はそのまま。
    harness.state_mut().out.clear();
    let pos = widget(&harness, ids::SECTION_OFFSET).center();
    pointer(&mut harness, pos, None);
    pointer(&mut harness, pos, Some(true));
    pointer(&mut harness, pos, Some(false));
    harness.step();
    harness.event(egui::Event::Text("-1.5".to_owned()));
    harness.step();
    assert!(harness.state().out.is_empty(), "打っている間は積まない");
    harness.key_press(egui::Key::Enter);
    harness.step();
    harness.step();
    let typed = only_plane(&std::mem::take(&mut harness.state_mut().out));
    assert_eq!(typed.offset.to_bits(), (-1.5_f32).to_bits());
    assert_eq!(typed.axis, Axis::Z);
    assert!(!typed.flipped);
}

#[test]
fn offset_in_display_units_is_converted_and_clamped() {
    let units = Units {
        source: UnitLabel::M,
        display: Some(UnitLabel::Mm),
    };
    let mut harness = harness(input(units));
    harness.step();
    // 欄は表示の単位の値を持つ（−2 m → −2000 mm）。
    let shown = harness
        .get_by_role(egui::accesskit::Role::SpinButton)
        .accesskit_node()
        .numeric_value();
    assert!(
        shown.is_some_and(|v| (v + 2000.0).abs() < 1.0e-3),
        "{shown:?}"
    );
    for (text, expected) in [("-2500", -2.5_f32), ("12345", 0.0)] {
        harness.state_mut().out.clear();
        let pos = widget(&harness, ids::SECTION_OFFSET).center();
        pointer(&mut harness, pos, None);
        pointer(&mut harness, pos, Some(true));
        pointer(&mut harness, pos, Some(false));
        harness.step();
        harness.event(egui::Event::Text(text.to_owned()));
        harness.step();
        harness.key_press(egui::Key::Enter);
        harness.step();
        harness.step();
        let plane = only_plane(&std::mem::take(&mut harness.state_mut().out));
        assert!(near(plane.offset, expected), "{text}: {plane:?}");
    }
}

#[test]
fn axis_change_recenters() {
    let mut harness = harness(SectionBarInput {
        plane: SectionPlane {
            axis: Axis::Z,
            offset: -3.5,
            flipped: true,
        },
        ..input(Units::default())
    });
    harness.step();
    let x = only_plane(&click(&mut harness, ids::SECTION_AXIS_X));
    assert_eq!(x.axis, Axis::X);
    assert!(near(x.offset, 1.0), "X の中央: {x:?}");
    assert!(x.flipped, "向きは保つ");
    let y = only_plane(&click(&mut harness, ids::SECTION_AXIS_Y));
    assert_eq!(y.axis, Axis::Y);
    assert!(near(y.offset, 4.0), "Y の中央: {y:?}");
    assert_eq!(
        y,
        with_axis(
            &SectionBarInput {
                plane: SectionPlane {
                    axis: Axis::Z,
                    offset: -3.5,
                    flipped: true,
                },
                ..input(Units::default())
            },
            Axis::Y
        )
    );
}

#[test]
fn dragging_the_offset_sends_planes_and_escape_restores() {
    let mut harness = harness(input(Units::default()));
    harness.step();
    let start = widget(&harness, ids::SECTION_OFFSET).center();
    pointer(&mut harness, start, None);
    pointer(&mut harness, start, Some(true));
    pointer(&mut harness, start + egui::vec2(20.0, 0.0), None);
    pointer(&mut harness, start + egui::vec2(40.0, 0.0), None);
    let out = std::mem::take(&mut harness.state_mut().out);
    assert!(!out.is_empty());
    let mut last = -2.0_f32;
    for command in &out {
        let Command::SetSection(Some(plane)) = command else {
            panic!("{out:?}");
        };
        assert!(plane.offset >= last, "右へドラッグで増える: {out:?}");
        assert!(plane.offset <= 0.0, "境界箱に収まる: {plane:?}");
        last = plane.offset;
    }
    assert!(last > -2.0);
    harness.key_press(egui::Key::Escape);
    pointer(&mut harness, start + egui::vec2(60.0, 0.0), None);
    pointer(&mut harness, start + egui::vec2(60.0, 0.0), Some(false));
    harness.step();
    let out = std::mem::take(&mut harness.state_mut().out);
    assert_eq!(
        out.first(),
        Some(&Command::SetSection(Some(SectionPlane::centered(
            Axis::Z,
            &bounds()
        )))),
        "Esc で掴んだときの位置へ戻す: {out:?}"
    );
    assert_eq!(out.len(), 1, "{out:?}");
}

#[test]
#[ignore = "wgpuで画面をPNGへ書き出す"]
fn render_section_bar_for_visual_check() {
    let mm = Units {
        source: UnitLabel::M,
        display: Some(UnitLabel::Mm),
    };
    let mut harness = Harness::builder()
        .with_size(egui::vec2(560.0, 60.0))
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
                section_bar::show(ui, &probe.input, &mut probe.out);
            },
            Probe {
                input: input(mm),
                out: Vec::new(),
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
        .save(dir.join("section-bar.png"))
        .expect("PNGの保存");
}
