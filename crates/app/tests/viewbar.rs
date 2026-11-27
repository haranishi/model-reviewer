//! 3Dビューの上端のボタン。押すと期待した `Command` が積まれ、線が使えないときは理由が出る。

use egui::accesskit::Toggled;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use model_reviewer::automation::WidgetFrame;
use model_reviewer::command::{Command, DisplayMode, Projection};
use model_reviewer::ui::ids;
use model_reviewer::ui::viewbar::{self, NO_LINES_REASON, ViewBarInput};

struct Probe {
    input: ViewBarInput,
    /// 断面の状態（`None` なら `show` を呼ぶ）。
    section: Option<bool>,
    out: Vec<Command>,
}

fn input(lines_supported: bool) -> ViewBarInput {
    ViewBarInput {
        display: DisplayMode::Faces,
        projection: Projection::Perspective,
        grid_visible: true,
        lines_supported,
    }
}

fn harness(input: ViewBarInput) -> Harness<'static, Probe> {
    Harness::builder()
        .with_size(egui::vec2(520.0, 160.0))
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
                match probe.section {
                    None => viewbar::show(ui, &probe.input, &mut out),
                    Some(_) => viewbar::show_with(ui, &probe.input, probe.section, &mut out),
                }
                probe.out.extend(out);
            },
            Probe {
                input,
                section: None,
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
fn every_button_is_registered_in_one_row() {
    let mut harness = harness(input(true));
    harness.step();
    let names = [
        ids::VIEW_DISPLAY_FACES,
        ids::VIEW_DISPLAY_FACES_LINES,
        ids::VIEW_DISPLAY_LINES,
        ids::VIEW_DISPLAY_XRAY,
        ids::VIEW_PROJECTION_PERSPECTIVE,
        ids::VIEW_PROJECTION_ORTHOGRAPHIC,
        ids::VIEW_GRID,
    ];
    let rects: Vec<egui::Rect> = names.iter().map(|name| widget(&harness, name)).collect();
    for pair in rects.windows(2) {
        assert!(pair[0].right() <= pair[1].left(), "{pair:?}");
        assert!((pair[0].center().y - pair[1].center().y).abs() < 0.5);
    }
    for rect in &rects {
        assert!(rect.height() >= 28.0, "押しやすい高さ: {rect:?}");
        assert!(rect.width() >= 40.0, "押しやすい幅: {rect:?}");
    }
    for label in ["面", "面と線", "線", "透かし", "透視", "平行", "格子"] {
        harness.get_by_label(label);
    }
}

#[test]
fn pressing_buttons_pushes_commands() {
    let mut harness = harness(input(true));
    harness.step();
    assert_eq!(
        click(&mut harness, ids::VIEW_DISPLAY_FACES_LINES),
        vec![Command::SetDisplay(DisplayMode::FacesAndLines)]
    );
    assert_eq!(
        click(&mut harness, ids::VIEW_DISPLAY_LINES),
        vec![Command::SetDisplay(DisplayMode::Lines)]
    );
    assert_eq!(
        click(&mut harness, ids::VIEW_DISPLAY_XRAY),
        vec![Command::SetDisplay(DisplayMode::XRay)]
    );
    assert_eq!(
        click(&mut harness, ids::VIEW_PROJECTION_ORTHOGRAPHIC),
        vec![Command::SetProjection(Projection::Orthographic)]
    );
    assert_eq!(
        click(&mut harness, ids::VIEW_GRID),
        vec![Command::SetGrid(false)]
    );
    // いま選ばれているものを押しても何も積まない。
    assert!(click(&mut harness, ids::VIEW_DISPLAY_FACES).is_empty());
    assert!(click(&mut harness, ids::VIEW_PROJECTION_PERSPECTIVE).is_empty());

    harness.state_mut().input = ViewBarInput {
        display: DisplayMode::XRay,
        projection: Projection::Orthographic,
        grid_visible: false,
        lines_supported: true,
    };
    harness.step();
    assert_eq!(
        click(&mut harness, ids::VIEW_DISPLAY_FACES),
        vec![Command::SetDisplay(DisplayMode::Faces)]
    );
    assert_eq!(
        click(&mut harness, ids::VIEW_PROJECTION_PERSPECTIVE),
        vec![Command::SetProjection(Projection::Perspective)]
    );
    assert_eq!(
        click(&mut harness, ids::VIEW_GRID),
        vec![Command::SetGrid(true)]
    );
}

#[test]
fn lines_buttons_are_disabled_with_a_reason_without_line_support() {
    let mut harness = harness(input(false));
    harness.step();
    assert!(click(&mut harness, ids::VIEW_DISPLAY_FACES_LINES).is_empty());
    assert!(click(&mut harness, ids::VIEW_DISPLAY_LINES).is_empty());
    // 透かしは線に依らない。
    assert_eq!(
        click(&mut harness, ids::VIEW_DISPLAY_XRAY),
        vec![Command::SetDisplay(DisplayMode::XRay)]
    );
    // 触れると理由が出る。
    let pos = widget(&harness, ids::VIEW_DISPLAY_LINES).center();
    harness.event(egui::Event::PointerMoved(pos));
    for _ in 0..6 {
        harness.step();
    }
    harness.get_by_label(NO_LINES_REASON);
    assert!(
        harness.get_by_label("線").accesskit_node().is_disabled(),
        "線のボタンは無効として伝わる"
    );
    assert!(
        !harness
            .get_by_label("透かし")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn section_button_toggles_and_shows_its_state() {
    // 状態を渡さなければ出さない（接合の前の呼び方）。
    let mut harness = harness(input(true));
    harness.step();
    assert!(
        harness
            .ctx
            .data(|data| data.get_temp::<WidgetFrame>(egui::Id::NULL))
            .unwrap_or_default()
            .entries()
            .iter()
            .all(|entry| entry.name != ids::VIEW_SECTION)
    );
    for on in [false, true] {
        harness.state_mut().section = Some(on);
        harness.step();
        let grid = widget(&harness, ids::VIEW_GRID);
        let section = widget(&harness, ids::VIEW_SECTION);
        assert!(section.left() >= grid.right(), "格子の右: {section:?}");
        assert!(section.height() >= 28.0 && section.width() >= 40.0);
        let node = harness.get_by_label("断面");
        let toggled = node.accesskit_node().toggled();
        assert_eq!(
            toggled,
            Some(if on { Toggled::True } else { Toggled::False })
        );
        assert_eq!(
            click(&mut harness, ids::VIEW_SECTION),
            vec![Command::ToggleSection]
        );
    }
}

#[test]
#[ignore = "wgpuで画面をPNGへ書き出す"]
fn render_viewbar_for_visual_check() {
    let cases = [
        ("viewbar-default", input(true), None),
        (
            "viewbar-ortho-xray-nogrid",
            ViewBarInput {
                display: DisplayMode::XRay,
                projection: Projection::Orthographic,
                grid_visible: false,
                lines_supported: true,
            },
            None,
        ),
        (
            "viewbar-no-lines-hover",
            input(false),
            Some(ids::VIEW_DISPLAY_LINES),
        ),
    ];
    for (name, input, hover) in cases {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(520.0, 110.0))
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
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(egui::Id::NULL, WidgetFrame::default()));
                    viewbar::show(ui, &probe.input, &mut probe.out);
                },
                Probe {
                    input,
                    section: None,
                    out: Vec::new(),
                },
            );
        harness.step();
        if let Some(hover) = hover {
            let pos = widget(&harness, hover).center();
            harness.event(egui::Event::PointerMoved(pos));
        }
        for _ in 0..6 {
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
