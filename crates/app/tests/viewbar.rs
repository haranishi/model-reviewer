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
        textures_shown: Some(true),
        lines_supported,
        keys: model_reviewer::keymap::KeyLabels::default(),
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
        ids::VIEW_TEXTURES,
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
    for label in [
        "面",
        "面と線",
        "線",
        "透かし",
        "画像",
        "透視",
        "平行",
        "格子",
    ] {
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
        textures_shown: Some(false),
        lines_supported: true,
        keys: model_reviewer::keymap::KeyLabels::default(),
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
                textures_shown: Some(false),
                lines_supported: true,
                keys: model_reviewer::keymap::KeyLabels::default(),
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

// ---- キーの文字（第4段階の WP-V1）----

use model_reviewer::keymap::{Action, Keymap, Platform};
use model_reviewer::ui::{empty, help, toolbar, toolrail};

#[test]
fn default_labels_keep_existing_text() {
    // 直す前の説明の文（第3段階の文を、そのまま写した）。
    let keys = Keymap::default().labels(Platform::Mac);
    let displays = [
        (
            DisplayMode::Faces,
            "面だけを描きます（Z で面と線を切り替え）",
        ),
        (DisplayMode::FacesAndLines, "面に三角形の辺を重ねます（Z）"),
        (DisplayMode::Lines, "三角形の辺だけを描きます"),
        (
            DisplayMode::XRay,
            "部品を半透明にして奥を見せます（Shift+Z）",
        ),
    ];
    for (mode, old) in displays {
        assert_eq!(viewbar::display_hint(mode, &keys), old, "{mode:?}");
    }
    assert_eq!(
        viewbar::projection_hint(Projection::Perspective, &keys),
        "遠くの物を小さく描きます（5 で切り替え）"
    );
    assert_eq!(
        viewbar::projection_hint(Projection::Orthographic, &keys),
        "遠近のゆがみ無しで描きます（5 で切り替え）"
    );
    assert_eq!(viewbar::grid_hint(true, &keys), "床の格子と軸を隠します");
    assert_eq!(viewbar::grid_hint(false, &keys), "床の格子と軸を出します");
    assert_eq!(
        viewbar::section_hint(true, &keys),
        "断面をやめて全体を表示します（C）"
    );
    assert_eq!(
        viewbar::section_hint(false, &keys),
        "軸に垂直な面で切って中を見ます（C）"
    );
    // 画面の説明（読み上げの説明）も同じ文。
    let mut harness = harness(input(true));
    harness.step();
    assert_eq!(
        harness
            .get_by_label("透かし")
            .accesskit_node()
            .description()
            .as_deref(),
        Some("部品を半透明にして奥を見せます（Shift+Z）")
    );
}

#[test]
fn unassigned_shows_no_parentheses() {
    let mut keymap = Keymap::default();
    for &action in Action::ALL {
        keymap.clear(action);
    }
    let keys = keymap.labels(Platform::Mac);
    let mut texts = Vec::new();
    for mode in [
        DisplayMode::Faces,
        DisplayMode::FacesAndLines,
        DisplayMode::Lines,
        DisplayMode::XRay,
    ] {
        texts.push(viewbar::display_hint(mode, &keys));
    }
    for projection in [Projection::Perspective, Projection::Orthographic] {
        texts.push(viewbar::projection_hint(projection, &keys));
    }
    for on in [true, false] {
        texts.push(viewbar::grid_hint(on, &keys));
        texts.push(viewbar::section_hint(on, &keys));
        texts.push(toolrail::snap_hint(on, &keys));
    }
    for tool in [
        model_reviewer::command::Tool::Select,
        model_reviewer::command::Tool::Move,
        model_reviewer::command::Tool::Measure,
    ] {
        texts.push(toolrail::tool_hint(tool, &keys));
        assert_eq!(toolrail::tool_badge(tool, &keys), None);
    }
    texts.push(empty::new_hint(&keys));
    for text in &texts {
        assert!(!text.contains('（'), "{text}");
    }
    assert_eq!(
        viewbar::display_hint(DisplayMode::Faces, &keys),
        "面だけを描きます"
    );
    assert_eq!(
        toolrail::tool_hint(model_reviewer::command::Tool::Move, &keys),
        "移動: 矢印をドラッグして動かします"
    );
    assert_eq!(toolbar::button_hint(&keys, Action::FileOpen, "開く"), None);
    // ヘルプ: キーの列はキーの無い所を「なし」にし、全部無い行は空にする。固定のキーの文は残る。
    let rows = help::operation_rows(&keys);
    let key_of = |what: &str| {
        rows.iter()
            .find(|row| row[0] == what)
            .map(|row| row[3].clone())
            .unwrap()
    };
    assert_eq!(key_of("全体表示／選択を表示／初期視点"), "");
    assert_eq!(key_of("道具を選ぶ"), "");
    assert_eq!(key_of("注釈を追加"), "Esc で終了");
    assert_eq!(key_of("複製／削除"), "なし ／ Delete（Backspace）");
    assert_eq!(key_of("選択"), "Esc で解除");
    assert_eq!(
        help::key_note(&keys, Platform::Mac),
        "中ボタンは使わない。数字は文字キーの段で、テンキーは使わない。1文字のキーは、入力欄に文字を打っている間は働かない。Windows と Linux では Cmd の代わりに Ctrl。"
    );
    // 一部だけ外したときは、位置が分かるよう「なし」で埋める。
    let mut keymap = Keymap::default();
    keymap.clear(Action::ViewFitSelection);
    let rows = help::operation_rows(&keymap.labels(Platform::Mac));
    assert!(rows.iter().any(|row| row[3] == "A ／ なし ／ Home"));
}
