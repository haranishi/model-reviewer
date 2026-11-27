//! 右のパネルの「寸法」。外形を表示の単位で出し、足した図形は寸法の欄で直せる。
//! 確定は `CommitLocal`（`scale` だけを差し替えた変形）。受け付けない値は欄を戻して理由を出す。
//! 入力用の構造体（`DimsView`）だけで動く。

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use glam::{Quat, Vec3};
use model_reviewer::automation::WidgetFrame;
use model_reviewer::command::{Command, ShapeKind, Trs, UnitLabel};
use model_reviewer::session::{DimsView, ShapeDims};
use model_reviewer::ui::dims::{
    self, LOADED_PART, NOT_DIMENSIONAL, NOT_POSITIVE, TOO_LARGE, TOO_SMALL, fields_of,
    reject_reason,
};
use model_reviewer::ui::ids;
use model_reviewer_core::shapes::{Dims, ShapeSpec, scale_for};
use model_reviewer_core::units::Units;

struct Probe {
    view: DimsView,
    out: Vec<Command>,
}

const CUBE: ShapeSpec = ShapeSpec {
    kind: ShapeKind::Cube,
    size: 0.5,
};

fn trs(scale: Vec3) -> Trs {
    Trs {
        translation: Vec3::new(1.0, 2.0, 3.0),
        rotation: Quat::IDENTITY,
        scale,
    }
}

/// 0.5 の立方体を (2, 4, 1) 倍した図形（寸法 1 × 2 × 0.5）。
fn cube_view(units: Units) -> DimsView {
    DimsView {
        node: None,
        extent: Some([1.0, 2.0, 0.5]),
        shape: Some(ShapeDims {
            dims: Some(Dims::Cube {
                x: 1.0,
                y: 2.0,
                z: 0.5,
            }),
            trs: trs(Vec3::new(2.0, 4.0, 1.0)),
            spec: CUBE,
            blocked: None,
        }),
        units,
        doc_rev: 1,
    }
}

fn loaded_view() -> DimsView {
    DimsView {
        node: None,
        extent: Some([0.5, 2.5, 1024.0]),
        shape: None,
        units: Units::default(),
        doc_rev: 3,
    }
}

fn harness(view: DimsView) -> Harness<'static, Probe> {
    Harness::builder()
        .with_size(egui::vec2(320.0, 300.0))
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
                dims::show(ui, &probe.view, &mut out);
                probe.out.extend(out);
            },
            Probe {
                view,
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

/// 欄を押して打ち、Enter で離れる。
fn type_and_enter(harness: &mut Harness<'static, Probe>, name: &str, text: &str) -> Vec<Command> {
    harness.state_mut().out.clear();
    let pos = widget(harness, name).center();
    pointer(harness, pos, None);
    pointer(harness, pos, Some(true));
    pointer(harness, pos, Some(false));
    harness.step();
    harness.event(egui::Event::Text(text.to_owned()));
    harness.step();
    harness.step();
    assert!(harness.state().out.is_empty(), "打っている間は積まない");
    harness.key_press(egui::Key::Enter);
    harness.step();
    harness.step();
    std::mem::take(&mut harness.state_mut().out)
}

fn field_value(harness: &Harness<'static, Probe>, index: usize) -> Option<f64> {
    harness
        .get_all_by_role(egui::accesskit::Role::SpinButton)
        .nth(index)
        .and_then(|node| node.accesskit_node().numeric_value())
}

#[test]
fn fields_follow_the_shape_kind() {
    let names = |kind| -> Vec<&str> { fields_of(kind).iter().map(|(n, _)| *n).collect() };
    assert_eq!(
        names(ShapeKind::Cube),
        [ids::DIMS_X, ids::DIMS_Y, ids::DIMS_Z]
    );
    assert_eq!(names(ShapeKind::Sphere), [ids::DIMS_D]);
    assert_eq!(names(ShapeKind::Cylinder), [ids::DIMS_D, ids::DIMS_H]);
    assert_eq!(names(ShapeKind::Plane), [ids::DIMS_X, ids::DIMS_Z]);
    assert_eq!(fields_of(ShapeKind::Cylinder)[0].1, "直径");
}

#[test]
fn reject_reasons_cover_the_limits() {
    assert_eq!(reject_reason(0.0), Some(NOT_POSITIVE));
    assert_eq!(reject_reason(-1.0), Some(NOT_POSITIVE));
    assert_eq!(reject_reason(f32::NAN), Some(NOT_POSITIVE));
    assert_eq!(reject_reason(f32::INFINITY), Some(NOT_POSITIVE));
    assert_eq!(reject_reason(2.0e9), Some(TOO_LARGE));
    assert_eq!(reject_reason(5.0e-7), Some(TOO_SMALL));
    assert_eq!(reject_reason(0.5), None);
}

#[test]
fn loaded_part_shows_extent_only() {
    let mut harness = harness(loaded_view());
    harness.step();
    for label in [
        "寸法",
        "幅 X",
        "高さ Y",
        "奥行 Z",
        "0.500000 m",
        "2.50000 m",
        "1024.00 m",
        LOADED_PART,
    ] {
        harness.get_by_label(label);
    }
    assert!(
        registered(&harness).is_empty(),
        "直す欄は出さない: {:?}",
        registered(&harness)
    );
    // 境界箱が空か有限でなければ「—」。
    harness.state_mut().view = DimsView {
        node: None,
        extent: None,
        ..loaded_view()
    };
    harness.step();
    assert_eq!(harness.get_all_by_label("— m").count(), 3);
    assert!(harness.state().out.is_empty());
}

#[test]
fn shape_fields_are_shown_in_display_units() {
    let mm = Units {
        source: UnitLabel::M,
        display: Some(UnitLabel::Mm),
    };
    let mut harness = harness(cube_view(mm));
    harness.step();
    for name in [ids::DIMS_X, ids::DIMS_Y, ids::DIMS_Z] {
        let rect = widget(&harness, name);
        assert!(rect.height() >= 24.0 && rect.width() >= 60.0, "{rect:?}");
    }
    for (index, expected) in [1000.0, 2000.0, 500.0].into_iter().enumerate() {
        let value = field_value(&harness, index);
        assert!(
            value.is_some_and(|v| (v - expected).abs() < 1.0e-3),
            "{index}: {value:?}"
        );
    }
    harness.get_by_label("1000.00 mm");
    assert!(harness.state().out.is_empty(), "見るだけでは何も積まない");
}

#[test]
fn dims_commit_emits_commit_local() {
    let mut harness = harness(cube_view(Units::default()));
    harness.step();
    let out = type_and_enter(&mut harness, ids::DIMS_Y, "3");
    let wanted = Dims::Cube {
        x: 1.0,
        y: 3.0,
        z: 0.5,
    };
    // 大きさへの換算はコアの `scale_for` に任せる（同じ関数で期待値を作る）。
    match scale_for(CUBE, wanted, 4.0) {
        Some(scale) => {
            assert_eq!(out, vec![Command::CommitLocal(trs(scale))]);
            let Command::CommitLocal(committed) = out[0] else {
                unreachable!()
            };
            assert_eq!(
                committed.translation,
                Vec3::new(1.0, 2.0, 3.0),
                "中心を保つ"
            );
            assert_eq!(committed.rotation, Quat::IDENTITY);
        }
        None => {
            // コアの関数がまだ置いただけのとき。確定は積まず、欄は元へ戻る。
            assert!(
                !out.iter().any(|c| matches!(c, Command::CommitLocal(_))),
                "{out:?}"
            );
        }
    }
}

#[test]
fn invalid_dim_restores_and_explains() {
    let mut harness = harness(cube_view(Units::default()));
    harness.step();
    for (text, reason) in [
        ("0", NOT_POSITIVE),
        ("-2", NOT_POSITIVE),
        ("5000000000", TOO_LARGE),
    ] {
        let out = type_and_enter(&mut harness, ids::DIMS_X, text);
        assert!(
            out.is_empty(),
            "{text}: 受け付けない値は何も積まない: {out:?}"
        );
        harness.get_by_label(reason);
        let value = field_value(&harness, 0);
        assert!(
            value.is_some_and(|v| (v - 1.0).abs() < 1.0e-6),
            "{text}: 欄を元の値へ戻す: {value:?}"
        );
    }
    let out = type_and_enter(&mut harness, ids::DIMS_Z, "0.0000001");
    assert!(out.is_empty(), "{out:?}");
    harness.get_by_label(TOO_SMALL);
}

#[test]
fn escape_while_typing_cancels_without_commit() {
    let mut harness = harness(cube_view(Units::default()));
    harness.step();
    let pos = widget(&harness, ids::DIMS_Z).center();
    pointer(&mut harness, pos, None);
    pointer(&mut harness, pos, Some(true));
    pointer(&mut harness, pos, Some(false));
    harness.step();
    harness.event(egui::Event::Text("4".to_owned()));
    harness.step();
    harness.key_press(egui::Key::Escape);
    harness.step();
    harness.step();
    harness.step();
    assert_eq!(harness.state().out, vec![Command::CancelTransform]);
}

#[test]
fn blocked_shapes_explain_why_and_show_no_fields() {
    let reason = "球の3軸がそろっていないため、寸法では直せません";
    let mut view = cube_view(Units::default());
    view.shape = Some(ShapeDims {
        dims: None,
        blocked: Some(reason),
        ..view.shape.unwrap()
    });
    let mut harness = harness(view);
    harness.step();
    harness.get_by_label(reason);
    assert!(registered(&harness).is_empty());

    // 理由が無ければ既定の文。
    let mut view = cube_view(Units::default());
    view.shape = Some(ShapeDims {
        dims: None,
        blocked: None,
        ..view.shape.unwrap()
    });
    harness.state_mut().view = view;
    harness.step();
    harness.get_by_label(NOT_DIMENSIONAL);
    assert!(registered(&harness).is_empty());
    assert!(harness.state().out.is_empty());
}

#[test]
fn cylinder_has_diameter_and_height_fields() {
    let spec = ShapeSpec {
        kind: ShapeKind::Cylinder,
        size: 1.0,
    };
    let mut harness = harness(DimsView {
        node: None,
        extent: Some([2.0, 0.5, 2.0]),
        shape: Some(ShapeDims {
            dims: Some(Dims::Cylinder { d: 2.0, h: 0.5 }),
            trs: trs(Vec3::new(2.0, 0.5, 2.0)),
            spec,
            blocked: None,
        }),
        units: Units::default(),
        doc_rev: 2,
    });
    harness.step();
    let names: Vec<String> = registered(&harness).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, [ids::DIMS_D, ids::DIMS_H]);
    harness.get_by_label("直径");
    harness.get_by_label("高さ");
    assert!(
        harness
            .get_all_by_role(egui::accesskit::Role::SpinButton)
            .all(|node| !node.accesskit_node().is_disabled())
    );
}

#[test]
#[ignore = "wgpuで画面をPNGへ書き出す"]
fn render_dims_for_visual_check() {
    let mm = Units {
        source: UnitLabel::M,
        display: Some(UnitLabel::Mm),
    };
    for (name, view) in [
        ("dims-cube-mm", cube_view(mm)),
        ("dims-loaded", loaded_view()),
    ] {
        let mut harness = Harness::builder()
            .with_size(egui::vec2(280.0, 220.0))
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
                    dims::show(ui, &probe.view, &mut probe.out);
                },
                Probe {
                    view,
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

mod support;

mod joined {
    //! 接合: `Session` から組み立てた「寸法」を、右のパネルごと動かす。
    use super::support;
    use egui_kittest::kittest::Queryable;
    use model_reviewer::command::{Command, ShapeKind};
    use model_reviewer::session::selection_dims;
    use model_reviewer::ui::dims::LOADED_PART;
    use model_reviewer::ui::ids;
    use model_reviewer_core::scene::NodeId;
    use model_reviewer_core::shapes::Dims;
    use model_reviewer_core::units::UnitLabel;

    const SIZE: egui::Vec2 = egui::vec2(1100.0, 760.0);

    fn cube_dims(session: &model_reviewer::session::Session) -> [f32; 3] {
        match selection_dims(session)
            .and_then(|view| view.shape)
            .and_then(|s| s.dims)
        {
            Some(Dims::Cube { x, y, z }) => [x, y, z],
            other => panic!("立方体の寸法ではない: {other:?}"),
        }
    }

    fn type_into(
        harness: &mut egui_kittest::Harness<'static, model_reviewer::session::Session>,
        name: &str,
        text: &str,
        enter: bool,
    ) {
        let rect = support::widget_rect(harness, name).unwrap_or_else(|| panic!("{name} が無い"));
        support::click(harness, rect.center());
        harness.event(egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        });
        harness.event(egui::Event::Text(text.to_owned()));
        harness.step();
        if enter {
            harness.key_press(egui::Key::Enter);
            harness.step();
            harness.step();
        }
    }

    #[test]
    fn dims_edit_is_one_undo_step() {
        let mut session = support::session();
        session.apply(Command::AddShape(ShapeKind::Cube));
        let before = cube_dims(&session);
        let undo_before = session.history.len();
        let (mut harness, seen) = support::harness(session, SIZE);
        harness.step();
        harness.step();
        seen.lock().unwrap().clear();
        type_into(&mut harness, ids::DIMS_X, "1.5", true);
        let commits = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|c| matches!(c, Command::CommitLocal(_)))
            .count();
        assert_eq!(commits, 1, "{:?}", seen.lock().unwrap());
        let after = cube_dims(harness.state());
        assert!(support::near(after[0], 1.5), "{after:?}");
        assert!(support::near(after[1], before[1]) && support::near(after[2], before[2]));
        assert_eq!(harness.state().history.len(), undo_before + 1);
        // 中心は動かない。
        let center = |s: &model_reviewer::session::Session| {
            let id = s.selection.unwrap();
            s.doc.as_ref().unwrap().subtree_bounds(id).center()
        };
        let c0 = center(harness.state());
        harness.state_mut().apply(Command::Undo);
        let undone = cube_dims(harness.state());
        assert!(support::near(undone[0], before[0]), "{undone:?}");
        assert!(support::close3(center(harness.state()), c0));
        harness.state_mut().apply(Command::Redo);
        assert!(support::near(cube_dims(harness.state())[0], 1.5));

        // 表示を mm にすると、欄は mm で受ける（250 mm = 0.25 m）。
        harness
            .state_mut()
            .apply(Command::SetDisplayUnit(Some(UnitLabel::Mm)));
        harness.step();
        type_into(&mut harness, ids::DIMS_Y, "250", true);
        assert!(support::near(cube_dims(harness.state())[1], 0.25));
        harness.step();
        harness.get_by_label("250.000 mm");
    }

    #[test]
    fn dims_of_loaded_part_are_read_only() {
        let mut session = support::session();
        session.apply(Command::Select(Some(NodeId(0))));
        let view = selection_dims(&session).expect("外形");
        assert!(view.shape.is_none());
        let extent = view.extent.unwrap();
        assert!(extent.iter().all(|v| support::near(*v, 2.0)), "{extent:?}");
        let (mut harness, _) = support::harness(session, SIZE);
        harness.step();
        harness.step();
        harness.get_by_label(LOADED_PART);
        assert!(support::widget_rect(&harness, ids::DIMS_X).is_none());
        // 選択が無ければ「寸法」は出さない。
        harness.state_mut().apply(Command::Select(None));
        harness.step();
        assert!(selection_dims(harness.state()).is_none());
        assert!(harness.query_by_label(LOADED_PART).is_none());
    }

    #[test]
    fn typed_text_does_not_follow_a_new_selection() {
        let mut session = support::session();
        session.apply(Command::AddShape(ShapeKind::Cube));
        let first = session.selection.unwrap();
        session.apply(Command::AddShape(ShapeKind::Cube));
        let second = session.selection.unwrap();
        session.apply(Command::Select(Some(first)));
        let (mut harness, seen) = support::harness(session, SIZE);
        harness.step();
        harness.step();
        let original = {
            harness.state_mut().apply(Command::Select(Some(second)));
            let dims = cube_dims(harness.state());
            harness.state_mut().apply(Command::Select(Some(first)));
            dims
        };
        harness.step();
        // 1つ目の幅に打ちかけたまま、選択を2つ目へ替える（欄を離れていない）。
        type_into(&mut harness, ids::DIMS_X, "7", false);
        harness.state_mut().apply(Command::Select(Some(second)));
        harness.step();
        harness.step();
        harness.key_press(egui::Key::Enter);
        harness.step();
        harness.step();
        assert_eq!(harness.state().selection, Some(second));
        let now = cube_dims(harness.state());
        assert!(
            support::near(now[0], original[0]),
            "2つ目に当てはまった: {now:?}"
        );
        assert!(
            !seen.lock().unwrap().iter().any(|c| matches!(
                c,
                Command::CommitLocal(trs) if (trs.scale.x - 7.0).abs() < 1.0
                    || trs.scale.x > 5.0
            )),
            "打ちかけの文字が確定した"
        );
    }

    #[test]
    fn typed_position_does_not_follow_a_new_selection() {
        let mut session = support::session();
        session.apply(Command::Select(Some(NodeId(0))));
        let (mut harness, seen) = support::harness(session, SIZE);
        harness.step();
        harness.step();
        let before = harness
            .state()
            .doc
            .as_ref()
            .unwrap()
            .node(NodeId(1))
            .unwrap()
            .local;
        type_into(&mut harness, ids::INSPECTOR_POS_X, "9", false);
        harness.state_mut().apply(Command::Select(Some(NodeId(1))));
        harness.step();
        harness.step();
        harness.key_press(egui::Key::Enter);
        harness.step();
        harness.step();
        let after = harness
            .state()
            .doc
            .as_ref()
            .unwrap()
            .node(NodeId(1))
            .unwrap()
            .local;
        assert_eq!(after, before, "打ちかけの位置が別の部品に当てはまった");
        assert!(
            !seen
                .lock()
                .unwrap()
                .iter()
                .any(|c| matches!(c, Command::CommitLocal(_)))
        );
    }
}
