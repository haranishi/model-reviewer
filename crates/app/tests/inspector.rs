//! 「変形」の数値の欄。ドラッグ中は `PreviewLocal`、離すと `CommitLocal`、打って欄を離れると
//! `CommitLocal`、Esc で `CancelTransform`。入力用の構造体だけで動く。

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use glam::{Quat, Vec3};
use model_reviewer::automation::WidgetFrame;
use model_reviewer::command::{Command, Trs};
use model_reviewer::ui::ids;
use model_reviewer::ui::inspector::{
    self, InspectorInput, InspectorValue, NO_SELECTION, from_fields, to_fields,
};
use model_reviewer_core::doc::NodeKey;
use model_reviewer_core::gizmo::from_euler_deg;

struct Probe {
    input: InspectorInput,
    out: Vec<Command>,
}

fn sample_trs() -> Trs {
    Trs {
        translation: Vec3::new(1.0, 2.0, 3.0),
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    }
}

fn editable(trs: Trs, rev: u64) -> InspectorInput {
    InspectorInput {
        node: Some(NodeKey(7)),
        value: InspectorValue::Editable(trs),
        doc_rev: rev,
    }
}

fn harness(input: InspectorInput) -> Harness<'static, Probe> {
    Harness::builder()
        .with_size(egui::vec2(320.0, 220.0))
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
                inspector::show(ui, &probe.input, &mut out);
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

fn click(harness: &mut Harness<'static, Probe>, name: &str) {
    let pos = widget(harness, name).center();
    pointer(harness, pos, None);
    pointer(harness, pos, Some(true));
    pointer(harness, pos, Some(false));
    harness.step();
}

fn type_into(harness: &mut Harness<'static, Probe>, name: &str, text: &str) {
    click(harness, name);
    harness.event(egui::Event::Text(text.to_owned()));
    harness.step();
    harness.step();
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).abs().max_element() < 1.0e-4
}

/// 2つの回転が同じ向きか。軸を回した結果で比べる。`angle_between` は差がほぼ0のとき f32 の丸め1つで
/// 約0.0007になり、CPUや数学ライブラリによって落ちる。
fn same_rotation(a: Quat, b: Quat, tolerance: f32) -> bool {
    [Vec3::X, Vec3::Y, Vec3::Z]
        .iter()
        .all(|&axis| (a * axis - b * axis).length() < tolerance)
}

#[test]
fn fields_round_trip_through_degrees() {
    let trs = Trs {
        translation: Vec3::new(-1.5, 0.0, 2.25),
        rotation: from_euler_deg(Vec3::new(10.0, 20.0, 30.0)),
        scale: Vec3::new(1.0, 2.0, 0.5),
    };
    let fields = to_fields(&trs);
    assert!((fields[3] - 10.0).abs() < 1.0e-3);
    assert!((fields[5] - 30.0).abs() < 1.0e-3);
    let back = from_fields(&fields);
    assert!(close(back.translation, trs.translation));
    assert!(close(back.scale, trs.scale));
    assert!(same_rotation(back.rotation, trs.rotation, 1.0e-4));
}

#[test]
fn without_selection_a_single_line_explains() {
    let mut harness = harness(InspectorInput {
        node: None,
        value: InspectorValue::Disabled(NO_SELECTION.to_owned()),
        doc_rev: 0,
    });
    harness.step();
    harness.get_by_label(NO_SELECTION);
    assert!(registered(&harness).is_empty());

    let reason = "傾いた拡大を含むため数値で表せません";
    harness.state_mut().input = InspectorInput {
        node: Some(NodeKey(3)),
        value: InspectorValue::Disabled(reason.to_owned()),
        doc_rev: 1,
    };
    harness.step();
    harness.get_by_label(reason);
    assert!(registered(&harness).is_empty());
    assert!(harness.state().out.is_empty());
}

#[test]
fn nine_fields_are_registered_with_axis_letters() {
    let mut harness = harness(editable(sample_trs(), 1));
    harness.step();
    let names: Vec<String> = registered(&harness).into_iter().map(|(n, _)| n).collect();
    assert_eq!(
        names,
        [
            ids::INSPECTOR_POS_X,
            ids::INSPECTOR_POS_Y,
            ids::INSPECTOR_POS_Z,
            ids::INSPECTOR_ROT_X,
            ids::INSPECTOR_ROT_Y,
            ids::INSPECTOR_ROT_Z,
            ids::INSPECTOR_SCALE_X,
            ids::INSPECTOR_SCALE_Y,
            ids::INSPECTOR_SCALE_Z,
        ]
    );
    for label in ["変形", "位置", "回転", "大きさ", "X", "Y", "Z"] {
        harness.get_by_label(label);
    }
    for (_, rect) in registered(&harness) {
        assert!(rect.height() >= 24.0 && rect.width() >= 60.0, "{rect:?}");
    }
    assert!(harness.state().out.is_empty(), "見るだけでは何も積まない");
}

#[test]
fn dragging_previews_and_release_commits() {
    let mut harness = harness(editable(sample_trs(), 1));
    harness.step();
    let start = widget(&harness, ids::INSPECTOR_POS_X).center();
    pointer(&mut harness, start, None);
    pointer(&mut harness, start, Some(true));
    pointer(&mut harness, start + egui::vec2(20.0, 0.0), None);
    pointer(&mut harness, start + egui::vec2(50.0, 0.0), None);
    let previews = harness.state().out.clone();
    assert!(!previews.is_empty());
    assert!(
        previews
            .iter()
            .all(|c| matches!(c, Command::PreviewLocal(_))),
        "{previews:?}"
    );
    pointer(&mut harness, start + egui::vec2(50.0, 0.0), Some(false));
    harness.step();
    let out = harness.state().out.clone();
    let Some(Command::CommitLocal(trs)) = out.last() else {
        panic!("離したら確定: {out:?}");
    };
    assert!(trs.translation.x > 1.3, "右へドラッグで増える: {trs:?}");
    assert!(close(
        Vec3::new(trs.translation.y, trs.translation.z, 0.0),
        Vec3::new(2.0, 3.0, 0.0)
    ));
    let Command::PreviewLocal(last_preview) = out[out.len() - 2] else {
        panic!("{out:?}");
    };
    assert_eq!(last_preview, *trs, "確定の値は最後に見せた値");
    assert_eq!(
        out.iter()
            .filter(|c| matches!(c, Command::CommitLocal(_)))
            .count(),
        1
    );
}

#[test]
fn typing_commits_when_the_field_is_left() {
    let mut harness = harness(editable(sample_trs(), 1));
    harness.step();
    type_into(&mut harness, ids::INSPECTOR_POS_X, "12.5");
    assert!(
        harness.state().out.is_empty(),
        "打っている間は何も積まない: {:?}",
        harness.state().out
    );
    harness.key_press(egui::Key::Enter);
    harness.step();
    harness.step();
    let out = harness.state().out.clone();
    assert_eq!(out.len(), 1, "{out:?}");
    let Command::CommitLocal(trs) = out[0] else {
        panic!("{out:?}");
    };
    assert!(close(trs.translation, Vec3::new(12.5, 2.0, 3.0)), "{trs:?}");
    assert!(close(trs.scale, Vec3::ONE));

    // 回転は度で打つ。Tab で離れても確定する。
    harness.state_mut().out.clear();
    harness.state_mut().input = editable(trs, 2);
    harness.step();
    type_into(&mut harness, ids::INSPECTOR_ROT_Y, "90");
    harness.key_press(egui::Key::Tab);
    harness.step();
    harness.step();
    let out = harness.state().out.clone();
    assert_eq!(out.len(), 1, "{out:?}");
    let Command::CommitLocal(rotated) = out[0] else {
        panic!("{out:?}");
    };
    let want = from_euler_deg(Vec3::new(0.0, 90.0, 0.0));
    assert!(same_rotation(rotated.rotation, want, 1.0e-3), "{rotated:?}");
    assert!(close(rotated.translation, Vec3::new(12.5, 2.0, 3.0)));
}

#[test]
fn escape_while_typing_cancels_without_commit() {
    let mut harness = harness(editable(sample_trs(), 1));
    harness.step();
    type_into(&mut harness, ids::INSPECTOR_SCALE_Z, "4");
    harness.key_press(egui::Key::Escape);
    harness.step();
    harness.step();
    harness.step();
    let out = harness.state().out.clone();
    assert_eq!(out, vec![Command::CancelTransform]);
}

#[test]
fn escape_while_dragging_cancels_and_release_does_not_commit() {
    let mut harness = harness(editable(sample_trs(), 1));
    harness.step();
    let start = widget(&harness, ids::INSPECTOR_POS_Y).center();
    pointer(&mut harness, start, None);
    pointer(&mut harness, start, Some(true));
    pointer(&mut harness, start + egui::vec2(30.0, 0.0), None);
    harness.key_press(egui::Key::Escape);
    pointer(&mut harness, start + egui::vec2(40.0, 0.0), None);
    pointer(&mut harness, start + egui::vec2(60.0, 0.0), None);
    pointer(&mut harness, start + egui::vec2(60.0, 0.0), Some(false));
    harness.step();
    let out = harness.state().out.clone();
    assert!(matches!(out[0], Command::PreviewLocal(_)), "{out:?}");
    let cancel = out
        .iter()
        .position(|c| matches!(c, Command::CancelTransform))
        .unwrap_or_else(|| panic!("取り消しが無い: {out:?}"));
    assert!(out[cancel + 1..].is_empty(), "{out:?}");
    assert!(!out.iter().any(|c| matches!(c, Command::CommitLocal(_))));
}

#[test]
fn a_new_revision_reloads_the_fields() {
    let mut harness = harness(editable(sample_trs(), 1));
    harness.step();
    // 外で変わった（ハンドルの確定など）。版が上がって値が替わる。
    let moved = Trs {
        translation: Vec3::new(5.0, 2.0, 3.0),
        ..sample_trs()
    };
    harness.state_mut().input = editable(moved, 2);
    harness.step();
    // 別の欄を打って確定すると、ほかの欄は新しい値のまま。
    type_into(&mut harness, ids::INSPECTOR_POS_Z, "7");
    harness.key_press(egui::Key::Enter);
    harness.step();
    harness.step();
    let out = harness.state().out.clone();
    let Some(Command::CommitLocal(trs)) = out.last() else {
        panic!("{out:?}");
    };
    assert!(close(trs.translation, Vec3::new(5.0, 2.0, 7.0)), "{trs:?}");
}

#[test]
#[ignore = "wgpuで画面をPNGへ書き出す"]
fn render_inspector_for_visual_check() {
    let rotated = Trs {
        translation: Vec3::new(12.5, -0.25, 1234.5678),
        rotation: from_euler_deg(Vec3::new(0.0, 45.0, -90.0)),
        scale: Vec3::new(1.0, 2.5, 0.125),
    };
    let cases = [
        ("inspector-values", editable(rotated, 1)),
        (
            "inspector-none",
            InspectorInput {
                node: None,
                value: InspectorValue::Disabled(NO_SELECTION.to_owned()),
                doc_rev: 0,
            },
        ),
    ];
    for (name, input) in cases {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(300.0, 180.0))
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
                        .rect_filled(ui.max_rect(), 0.0, model_reviewer::theme::PANEL_BG);
                    inspector::show(ui, &probe.input, &mut probe.out);
                },
                Probe {
                    input,
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
            .save(dir.join(format!("{name}.png")))
            .expect("PNGの保存");
    }
}
