//! 道具の列と図形のボタン。入力用の構造体だけで動き、押すと `SetTool`・`AddShape` を積む。

use egui::accesskit::Toggled;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use model_reviewer::automation::WidgetFrame;
use model_reviewer::command::{Command, ShapeKind, Tool};
use model_reviewer::ui::ids;
use model_reviewer::ui::ids::TOOL_SHAPE;
use model_reviewer::ui::toolrail::{self, NO_MODEL_REASON, ToolRailInput, snap_label};

struct Probe {
    input: ToolRailInput,
    /// 吸着の状態（`None` なら `show` を呼ぶ）。
    snap: Option<bool>,
    out: Vec<Command>,
}

fn input(tool: Tool) -> ToolRailInput {
    ToolRailInput {
        tool,
        has_model: true,
        edit_blocked: None,
        keys: model_reviewer::keymap::KeyLabels::default(),
    }
}

fn harness(input: ToolRailInput) -> Harness<'static, Probe> {
    Harness::builder()
        .with_size(egui::vec2(420.0, 460.0))
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
                match probe.snap {
                    None => toolrail::show(ui, &probe.input, &mut out),
                    Some(_) => toolrail::show_with(ui, &probe.input, probe.snap, &mut out),
                };
                probe.out.extend(out);
            },
            Probe {
                input,
                snap: None,
                out: Vec::new(),
            },
        )
}

fn names(harness: &Harness<'static, Probe>) -> Vec<(String, egui::Rect)> {
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
    names(harness)
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("{name} が登録されていない"))
        .1
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

const TOOLS: [(&str, Tool); 7] = [
    (ids::TOOL_SELECT, Tool::Select),
    (ids::TOOL_MOVE, Tool::Move),
    (ids::TOOL_ROTATE, Tool::Rotate),
    (ids::TOOL_SCALE, Tool::Scale),
    (TOOL_SHAPE, Tool::Shape),
    (ids::TOOL_MEASURE, Tool::Measure),
    (ids::TOOL_ANNOTATE, Tool::Annotate),
];

#[test]
fn tools_stand_in_one_column_with_large_targets() {
    let mut harness = harness(input(Tool::Select));
    harness.step();
    let rects: Vec<egui::Rect> = TOOLS
        .iter()
        .map(|(name, _)| widget(&harness, name))
        .collect();
    for pair in rects.windows(2) {
        assert!(pair[0].bottom() <= pair[1].top(), "{pair:?}");
        assert!((pair[0].center().x - pair[1].center().x).abs() < 0.5);
    }
    for rect in &rects {
        assert!(rect.width() >= 44.0 && rect.height() >= 44.0, "{rect:?}");
    }
    // 図形のボタンは図形の道具のときだけ。
    assert!(
        names(&harness)
            .iter()
            .all(|(name, _)| !name.starts_with("shape."))
    );
    for label in ["選択", "移動", "回転", "拡大", "図形", "計測", "注釈"] {
        harness.get_by_label(label);
    }
}

#[test]
fn pressing_a_tool_selects_it() {
    let mut harness = harness(input(Tool::Select));
    harness.step();
    for (name, tool) in TOOLS {
        let out = click(&mut harness, name);
        if tool == Tool::Select {
            assert!(out.is_empty(), "選ばれている道具を押しても何も積まない");
        } else {
            assert_eq!(out, vec![Command::SetTool(tool)], "{name}");
        }
    }
    let selected = harness.get_by_label("選択");
    assert_eq!(selected.accesskit_node().toggled(), Some(Toggled::True));
    assert_eq!(
        harness.get_by_label("移動").accesskit_node().toggled(),
        Some(Toggled::False)
    );
}

#[test]
fn shape_buttons_appear_beside_the_shape_tool_and_add_shapes() {
    let mut harness = harness(input(Tool::Shape));
    harness.step();
    harness.step();
    let shape_tool = widget(&harness, TOOL_SHAPE);
    let shapes = [
        (ids::SHAPE_CUBE, ShapeKind::Cube, "立方体"),
        (ids::SHAPE_SPHERE, ShapeKind::Sphere, "球"),
        (ids::SHAPE_CYLINDER, ShapeKind::Cylinder, "円柱"),
        (ids::SHAPE_PLANE, ShapeKind::Plane, "平面"),
    ];
    let mut last_right = shape_tool.right();
    for (name, kind, label) in shapes {
        let rect = widget(&harness, name);
        assert!(rect.left() >= last_right, "{name}: {rect:?}");
        assert!(
            (rect.center().y - shape_tool.center().y).abs() < 6.0,
            "図形の道具の横に並ぶ: {rect:?} {shape_tool:?}"
        );
        last_right = rect.right();
        harness.get_by_label(label);
        assert_eq!(click(&mut harness, name), vec![Command::AddShape(kind)]);
    }
}

#[test]
fn without_a_model_only_select_works_and_tells_why() {
    let mut harness = harness(ToolRailInput {
        tool: Tool::Select,
        has_model: false,
        edit_blocked: None,
        keys: model_reviewer::keymap::KeyLabels::default(),
    });
    harness.step();
    for (name, tool) in TOOLS {
        if tool == Tool::Select {
            continue;
        }
        assert!(click(&mut harness, name).is_empty(), "{name}");
    }
    assert!(harness.get_by_label("移動").accesskit_node().is_disabled());
    assert!(!harness.get_by_label("選択").accesskit_node().is_disabled());
    let pos = widget(&harness, ids::TOOL_MEASURE).center();
    harness.event(egui::Event::PointerMoved(pos));
    for _ in 0..6 {
        harness.step();
    }
    harness.get_by_label(NO_MODEL_REASON);
}

#[test]
fn blocked_editing_disables_edit_tools_and_shapes_with_the_reason() {
    let reason = "書き出し中は編集できません";
    let mut harness = harness(ToolRailInput {
        tool: Tool::Shape,
        has_model: true,
        edit_blocked: Some(reason.to_owned()),
        keys: model_reviewer::keymap::KeyLabels::default(),
    });
    harness.step();
    harness.step();
    for name in [
        ids::TOOL_MOVE,
        ids::TOOL_ROTATE,
        ids::TOOL_SCALE,
        ids::TOOL_ANNOTATE,
        ids::SHAPE_CUBE,
        ids::SHAPE_PLANE,
    ] {
        assert!(click(&mut harness, name).is_empty(), "{name}");
    }
    // 選択と計測は形を変えないので使える。
    assert_eq!(
        click(&mut harness, ids::TOOL_MEASURE),
        vec![Command::SetTool(Tool::Measure)]
    );
    assert_eq!(
        click(&mut harness, ids::TOOL_SELECT),
        vec![Command::SetTool(Tool::Select)]
    );
    let pos = widget(&harness, ids::SHAPE_SPHERE).center();
    harness.event(egui::Event::PointerMoved(pos));
    for _ in 0..6 {
        harness.step();
    }
    harness.get_by_label(reason);
}

#[test]
fn snap_button_only_in_measure_tool() {
    // 吸着の状態を渡しても、計測の道具でなければ出さない。
    for tool in [Tool::Select, Tool::Move, Tool::Shape, Tool::Annotate] {
        let mut harness = harness(input(tool));
        harness.state_mut().snap = Some(true);
        harness.step();
        harness.step();
        assert!(
            names(&harness)
                .iter()
                .all(|(name, _)| name != ids::MEASURE_SNAP),
            "{tool:?}"
        );
    }
    // 計測の道具でも、状態を渡さなければ出さない（接合の前の呼び方）。
    let mut harness = harness(input(Tool::Measure));
    harness.step();
    harness.step();
    assert!(
        names(&harness)
            .iter()
            .all(|(name, _)| name != ids::MEASURE_SNAP)
    );

    // 計測の道具の横に並び、押すと入・切が替わる。
    harness.state_mut().snap = Some(true);
    harness.step();
    harness.step();
    let measure = widget(&harness, ids::TOOL_MEASURE);
    let snap = widget(&harness, ids::MEASURE_SNAP);
    assert!(snap.left() >= measure.right(), "{snap:?} {measure:?}");
    assert!(
        (snap.center().y - measure.center().y).abs() < 6.0,
        "計測の道具の横: {snap:?} {measure:?}"
    );
    assert!(snap.width() >= 44.0 && snap.height() >= 44.0, "{snap:?}");
    let on = harness.get_by_label(snap_label(true));
    assert_eq!(on.accesskit_node().toggled(), Some(Toggled::True));
    assert_eq!(
        click(&mut harness, ids::MEASURE_SNAP),
        vec![Command::SetSnap(false)]
    );
    harness.state_mut().snap = Some(false);
    harness.step();
    let off = harness.get_by_label(snap_label(false));
    assert_eq!(off.accesskit_node().toggled(), Some(Toggled::False));
    assert_eq!(
        click(&mut harness, ids::MEASURE_SNAP),
        vec![Command::SetSnap(true)]
    );
    assert_eq!(snap_label(true), "吸着 入");
    assert_eq!(snap_label(false), "吸着 切");
}

#[test]
#[ignore = "wgpuで画面をPNGへ書き出す"]
fn render_toolrail_for_visual_check() {
    let cases = [
        ("toolrail-move", input(Tool::Move), None),
        (
            "toolrail-shape",
            input(Tool::Shape),
            Some(ids::SHAPE_SPHERE),
        ),
        (
            "toolrail-no-model",
            ToolRailInput {
                tool: Tool::Select,
                has_model: false,
                edit_blocked: None,
                keys: model_reviewer::keymap::KeyLabels::default(),
            },
            Some(ids::TOOL_ROTATE),
        ),
    ];
    for (name, input, hover) in cases {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(420.0, 420.0))
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
                    toolrail::show(ui, &probe.input, &mut probe.out);
                },
                Probe {
                    input,
                    snap: None,
                    out: Vec::new(),
                },
            );
        harness.step();
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

use model_reviewer::keymap::{Action, Chord, KeyLabels, Keymap, Platform};
use model_reviewer::ui::{empty, help, toolbar};

/// 直す前の説明の文（第3段階と新規作成の時点の文を、そのまま写した）。
const OLD_TOOL_HINTS: [(Tool, &str); 7] = [
    (Tool::Select, "選択（V）: クリックで部品を選びます"),
    (Tool::Move, "移動（G）: 矢印をドラッグして動かします"),
    (Tool::Rotate, "回転（R）: 輪をドラッグして回します"),
    (Tool::Scale, "拡大（S）: 箱をドラッグして大きさを変えます"),
    (Tool::Shape, "図形: 立方体・球・円柱・平面を置きます"),
    (
        Tool::Measure,
        "計測（M）: 角や辺に近づけて2回クリックし、距離を測ります",
    ),
    (
        Tool::Annotate,
        "注釈（N）: 表面をクリックして注釈を書きます",
    ),
];

/// 操作表の既定の文。既存の行は変更前の全角の「＋」で記録している。
const DEFAULT_HELP_ROWS: [[&str; 4]; 32] = [
    ["したいこと", "マウス", "トラックパッド", "キー"],
    [
        "新規作成（空の状態から作り始める）",
        "何も開いていない画面の［新規作成］",
        "同じ",
        "Cmd＋N",
    ],
    ["回転", "左ドラッグ", "1本指で押してドラッグ", ""],
    [
        "平行移動",
        "右ドラッグ、または Shift＋左ドラッグ",
        "Shift＋ドラッグ、または2本指で押してドラッグ",
        "",
    ],
    ["ズーム", "ホイール", "2本指スクロール、ピンチ", ""],
    ["選択", "左クリック", "クリック（タップ）", "Esc で解除"],
    [
        "全体表示／選択を表示／初期視点",
        "上部のボタン",
        "同じ",
        "A ／ F ／ Home",
    ],
    [
        "選択を非表示／すべて表示",
        "目の印、ボタン",
        "同じ",
        "H ／ Shift＋H",
    ],
    [
        "注釈を追加",
        "「注釈を追加」→ 表面をクリック",
        "同じ",
        "N で切り替え、Esc で終了",
    ],
    [
        "前・右・上から見る",
        "右上の立方体の面を押す（後・左・下も）",
        "同じ",
        "1 ／ 3 ／ 7",
    ],
    [
        "透視と平行を切り替える",
        "3Dビューの上端のボタン",
        "同じ",
        "5",
    ],
    [
        "面と線／透かし",
        "3Dビューの上端のボタン",
        "同じ",
        "Z ／ Shift＋Z",
    ],
    ["床の格子と軸", "上端の「格子」", "同じ", ""],
    [
        "道具を選ぶ",
        "左端の列を押す",
        "同じ",
        "V 選択／G 移動／R 回転／S 拡大／M 計測／N 注釈",
    ],
    [
        "動かす・回す・拡大する",
        "ハンドルを押してドラッグ、離すと確定",
        "1本指で押してドラッグ",
        "ドラッグ中の Esc で取り消し",
    ],
    [
        "数値で指定する",
        "右の「変形」の欄を押して入力、または欄を左右にドラッグ",
        "同じ",
        "Enter で確定、Esc で戻す",
    ],
    [
        "向きを固定して数値を打つ",
        "移動・回転・拡大の道具で部品を選ぶ",
        "同じ",
        "X・Y・Z で軸を選び、数値を入力",
    ],
    [
        "格子1つぶん動かす",
        "移動の道具で部品を選ぶ",
        "同じ",
        "矢印で移動、Shift で10分の1",
    ],
    [
        "打った量を確定する／やめる",
        "数値の入力中",
        "同じ",
        "Enter で確定、Esc でやめる",
    ],
    [
        "取り消し／やり直し",
        "上部の「戻す」「やり直す」",
        "同じ",
        "Cmd＋Z ／ Shift＋Cmd＋Z",
    ],
    ["図形を足す", "左端の「図形」→ 形を押す", "同じ", ""],
    [
        "複製／削除",
        "部品を選んでからキー",
        "同じ",
        "Cmd＋D ／ Delete（Backspace）",
    ],
    [
        "角や辺に合わせて測る",
        "左端の「計測」→ 印が付いた所でクリック",
        "同じ",
        "M、Esc で消す・終了",
    ],
    ["吸着を入れる・切る", "道具の列の横の「吸着」", "同じ", "X"],
    ["切って中を見る", "上端の「断面」", "同じ", "C"],
    [
        "切る軸を選ぶ・向きを反転する",
        "断面の行の X・Y・Z、「反転」",
        "同じ",
        "",
    ],
    [
        "切る位置を動かす",
        "3Dビューの矢印をドラッグ、または位置の欄を左右にドラッグ・数値を入力",
        "同じ",
        "Enter で確定、Esc で戻す",
    ],
    [
        "表示の単位を替える",
        "状態欄の「表示 …」を押す（ファイルと同じ → mm → cm → m）",
        "同じ",
        "",
    ],
    [
        "ファイルの単位の前提を替える",
        "状態欄の「1単位 = …」「STL = …」を押す（STL は開き直す）",
        "同じ",
        "",
    ],
    [
        "STL の上の向きを替える",
        "状態欄の「上 = …」を押す（開き直す）",
        "同じ",
        "",
    ],
    [
        "図形の寸法を決める",
        "「図形」で置く → 右の「寸法」に入力",
        "同じ",
        "Enter で確定、Esc で戻す",
    ],
    [
        "STL を開く",
        "「開く」、またはファイルをドロップ（バイナリと ASCII）",
        "同じ",
        "",
    ],
];

const OLD_KEY_NOTE: &str = "中ボタンは使わない。数字は文字キーの段で、テンキーは使わない。1文字のキーと Cmd＋Z は、入力欄に文字を打っている間は働かない。Windows と Linux では Cmd の代わりに Ctrl。";

fn mac_defaults() -> KeyLabels {
    Keymap::default().labels(Platform::Mac)
}

fn plain(key: egui::Key) -> Chord {
    Chord {
        key,
        shift: false,
        command: false,
    }
}

/// 描いた文字と、その位置（道具の列の右下の1文字を確かめる）。
fn painted_texts(harness: &Harness<'static, Probe>) -> Vec<(String, egui::Rect)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(text) => {
                out.push((text.galley.text().to_owned(), shape.visual_bounding_rect()));
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &harness.output().shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

fn badge_of(harness: &Harness<'static, Probe>, name: &str) -> Vec<String> {
    let button = widget(harness, name);
    painted_texts(harness)
        .into_iter()
        .filter(|(text, rect)| {
            text.chars().count() == 1
                && button.contains_rect(*rect)
                && rect.center().x > button.center().x
                && rect.center().y > button.center().y
        })
        .map(|(text, _)| text)
        .collect()
}

#[test]
fn default_labels_keep_existing_text() {
    let keys = mac_defaults();
    for (tool, old) in OLD_TOOL_HINTS {
        assert_eq!(toolrail::tool_hint(tool, &keys), old, "{tool:?}");
    }
    assert_eq!(
        toolrail::snap_hint(true, &keys),
        "吸着（X）: 角や辺に近づけると、そこへ点を置きます。押すと切ります"
    );
    assert_eq!(
        toolrail::snap_hint(false, &keys),
        "吸着（X）: 切っている間は、押した面の上に点を置きます。押すと入れます"
    );
    assert_eq!(
        toolrail::shape_hint(ShapeKind::Cube, &keys),
        "立方体を視点の中心に置きます"
    );
    // ヘルプの表: キーの列はつなぎの「＋」→「+」だけが変わり、ほかの列は変わらない。
    let rows = help::operation_rows(&keys);
    assert_eq!(rows.len(), DEFAULT_HELP_ROWS.len());
    for (row, old) in rows.iter().zip(DEFAULT_HELP_ROWS) {
        assert_eq!(row[..3], old[..3], "{old:?}");
        assert_eq!(row[3], old[3].replace('＋', "+"), "{old:?}");
    }
    assert_eq!(
        help::key_note(&keys, Platform::Mac),
        OLD_KEY_NOTE.replace('＋', "+")
    );
    assert_eq!(empty::new_hint(&keys), "空の状態から作ります（Cmd+N）");
    // 道具の列の1文字は、修飾の無い1文字のキーだけ。
    let badges: Vec<Option<&str>> = OLD_TOOL_HINTS
        .iter()
        .map(|(tool, _)| toolrail::tool_badge(*tool, &keys))
        .collect();
    assert_eq!(
        badges,
        [
            Some("V"),
            Some("G"),
            Some("R"),
            Some("S"),
            None,
            Some("M"),
            Some("N")
        ]
    );
    // 上部のボタンの説明（今までは無かった）。キーの無いボタンは説明を出さない。
    assert_eq!(
        toolbar::button_hint(&keys, Action::FileOpen, "開く").as_deref(),
        Some("開く（Cmd+O）")
    );
    assert_eq!(
        toolbar::button_hint(&keys, Action::AppHelp, "ヘルプ").as_deref(),
        Some("ヘルプ（F1）")
    );
    assert_eq!(
        toolbar::button_hint(&keys, Action::FileLoadReview, "レビュー読込"),
        None
    );
    // Windows と Linux の書き方。
    let other = Keymap::default().labels(Platform::Other);
    let rows = help::operation_rows(&other);
    let undo = rows
        .iter()
        .find(|row| row[0] == "取り消し／やり直し")
        .unwrap();
    assert_eq!(undo[3], "Ctrl+Z ／ Shift+Ctrl+Z");
    assert_eq!(
        help::key_note(&other, Platform::Other),
        "中ボタンは使わない。数字は文字キーの段で、テンキーは使わない。1文字のキーと Ctrl+Z は、入力欄に文字を打っている間は働かない。macOS では Ctrl の代わりに Cmd。"
    );
}

#[test]
fn rebinding_changes_hint_badge_and_help() {
    let mut keymap = Keymap::default();
    keymap
        .assign(Action::ToolMove, plain(egui::Key::T), false)
        .unwrap();
    keymap
        .assign(Action::ToolShape, plain(egui::Key::B), false)
        .unwrap();
    keymap
        .assign(
            Action::ToolRotate,
            Chord {
                key: egui::Key::R,
                shift: true,
                command: false,
            },
            false,
        )
        .unwrap();
    let keys = keymap.labels(Platform::Mac);
    assert_eq!(
        toolrail::tool_hint(Tool::Move, &keys),
        "移動（T）: 矢印をドラッグして動かします"
    );
    assert_eq!(
        toolrail::tool_hint(Tool::Shape, &keys),
        "図形（B）: 立方体・球・円柱・平面を置きます"
    );
    assert_eq!(toolrail::tool_badge(Tool::Rotate, &keys), None);

    // 画面: 説明の文（読み上げの説明）と、右下の1文字が今のキーになる。
    let mut harness = harness(ToolRailInput {
        keys: keys.clone(),
        ..input(Tool::Select)
    });
    harness.step();
    harness.step();
    assert_eq!(badge_of(&harness, ids::TOOL_MOVE), ["T"]);
    assert_eq!(badge_of(&harness, ids::TOOL_SHAPE), ["B"]);
    assert!(badge_of(&harness, ids::TOOL_ROTATE).is_empty());
    assert_eq!(badge_of(&harness, ids::TOOL_SELECT), ["V"]);
    assert_eq!(
        harness
            .get_by_label("移動")
            .accesskit_node()
            .description()
            .as_deref(),
        Some("移動（T）: 矢印をドラッグして動かします")
    );
    // 次のフレームから既定の文字に戻せる（割り当てが変わったときの描き直し）。
    harness.state_mut().input.keys = mac_defaults();
    harness.step();
    assert_eq!(badge_of(&harness, ids::TOOL_MOVE), ["G"]);
    assert_eq!(badge_of(&harness, ids::TOOL_SHAPE), Vec::<String>::new());

    // ヘルプの表。
    let rows = help::operation_rows(&keys);
    let tools = rows.iter().find(|row| row[0] == "道具を選ぶ").unwrap();
    assert_eq!(
        tools[3],
        "V 選択／T 移動／Shift+R 回転／S 拡大／B 図形／M 計測／N 注釈"
    );
    let shapes = rows.iter().find(|row| row[0] == "図形を足す").unwrap();
    assert_eq!(shapes[3], "B");
}
