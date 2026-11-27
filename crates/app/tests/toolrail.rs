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
