//! 断面の接合: C、控え、消えた側の当たり、消えた側の注釈と計測、開き直し、変形中の切り替え。
//! GPUは使わない。

mod support;

use glam::Vec3;
use model_reviewer::command::{Command, DialogButton, DialogRequest, Tool};
use model_reviewer::session::Session;
use model_reviewer::ui::{ids, overlay, viewport};
use model_reviewer_core::gizmo::Handle;
use model_reviewer_core::review::Annotation;
use model_reviewer_core::scene::NodeId;
use model_reviewer_core::section::{Axis, SectionPlane};
use model_reviewer_core::snap::SnapKind;
use support::{FAR_CENTER, NEAR_CENTER, ndc_of};

/// 画面の中央（手前の立方体の前の面。断面で消えると、奥の立方体の前の面）。
const MIDDLE: glam::Vec2 = glam::Vec2::new(0.02, 0.03);

fn z_cut() -> SectionPlane {
    SectionPlane {
        axis: Axis::Z,
        offset: 0.0,
        flipped: false,
    }
}

#[test]
fn c_toggles_section_and_remembers_plane() {
    let (mut harness, seen) = support::harness(support::session(), egui::vec2(1100.0, 720.0));
    for _ in 0..3 {
        harness.step();
    }
    support::hover_view(&mut harness);
    let _ = harness.state_mut().take_scene_dirty();
    harness.key_press(egui::Key::C);
    harness.step();
    // 最初は Z 軸・見えている全体の中央・正の側を消す。
    assert_eq!(harness.state().section, Some(z_cut()));
    assert!(!harness.state().has_unsaved(), "未保存の印に入らない");
    assert!(harness.state().history.is_empty(), "履歴に入らない");
    // 断面の帯が出る。
    harness.step();
    for name in [
        ids::SECTION_AXIS_X,
        ids::SECTION_AXIS_Y,
        ids::SECTION_AXIS_Z,
        ids::SECTION_OFFSET,
        ids::SECTION_FLIP,
    ] {
        assert!(support::widget_rect(&harness, name).is_some(), "{name}");
    }
    // 軸 Y・位置・反転を選んでから切る。もう一度入れると同じ平面。
    let y_plane = SectionPlane {
        axis: Axis::Y,
        offset: 0.5,
        flipped: true,
    };
    harness
        .state_mut()
        .apply(Command::SetSection(Some(y_plane)));
    harness.step();
    harness.key_press(egui::Key::C);
    harness.step();
    assert_eq!(harness.state().section, None);
    assert!(
        support::widget_rect(&harness, ids::SECTION_FLIP).is_none(),
        "帯が消える"
    );
    harness.key_press(egui::Key::C);
    harness.step();
    assert_eq!(
        harness.state().section,
        Some(y_plane),
        "前の軸と位置を覚えている"
    );

    // Cmd+C はコピーなので受けない。ダイアログ中・入力中は働かない。
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::C);
    harness.step();
    assert_eq!(harness.state().section, Some(y_plane));
    harness.state_mut().dialog = Some(DialogRequest {
        title: "確認".to_owned(),
        body: "続けますか".to_owned(),
        buttons: vec![DialogButton {
            id: ids::DIALOG_CANCEL.to_owned(),
            label: "閉じる".to_owned(),
            command: Command::DismissDialog,
        }],
    });
    harness.step();
    seen.lock().unwrap().clear();
    harness.key_press(egui::Key::C);
    harness.step();
    assert_eq!(harness.state().section, Some(y_plane));
    harness.state_mut().dialog = None;
    harness.step();
    let field = support::widget_rect(&harness, ids::VIEWS_NAME).expect("視点の名前の欄");
    support::click(&mut harness, field.center());
    assert!(harness.ctx.text_edit_focused());
    harness.key_press(egui::Key::C);
    harness.step();
    assert_eq!(harness.state().section, Some(y_plane));
    assert!(
        !seen
            .lock()
            .unwrap()
            .iter()
            .any(|command| matches!(command, Command::ToggleSection)),
        "ダイアログ中と入力中に C が積まれた"
    );
}

#[test]
fn section_commands_clamp_and_mark_only_the_section_dirty() {
    let mut session = support::session();
    // モデルが無ければ何もしない。
    let mut empty = Session::default();
    empty.apply(Command::ToggleSection);
    assert_eq!(empty.section, None);
    // 範囲の外の位置は、境界箱（z は −3〜3）へ収める。
    session.apply(Command::SetSection(Some(SectionPlane {
        offset: 40.0,
        ..z_cut()
    })));
    assert_eq!(session.section.map(|p| p.offset), Some(3.0));
    let dirty = session.take_scene_dirty();
    assert!(dirty.section);
    assert!(
        !dirty.instances && !dirty.model && !dirty.selection,
        "{dirty:?}"
    );
    // 同じ平面をもう一度置いても、描き直さない。
    session.apply(Command::SetSection(session.section));
    assert!(!session.take_scene_dirty().any());
    session.apply(Command::SetSection(None));
    assert_eq!(session.section, None);
    assert!(session.take_scene_dirty().section);
    assert!(!session.has_unsaved());
}

#[test]
fn cut_side_is_not_selectable() {
    // 選択: 断面なしでは手前、断面ありでは消えた手前を通り抜けて奥。
    let mut session = support::session();
    session.apply(Command::ViewClick { ndc: MIDDLE });
    assert_eq!(session.selection, Some(NodeId(0)));
    session.apply(Command::SetSection(Some(z_cut())));
    session.apply(Command::ViewClick { ndc: MIDDLE });
    assert_eq!(session.selection, Some(NodeId(1)), "消えた側は選べない");
    // 反転すると、奥が消えて手前が選べる。
    session.apply(Command::SetSection(Some(SectionPlane {
        flipped: true,
        ..z_cut()
    })));
    session.apply(Command::ViewClick { ndc: MIDDLE });
    assert_eq!(session.selection, Some(NodeId(0)));

    // 注釈の下書き: 奥の立方体に付く。
    let mut session = support::session();
    session.apply(Command::SetSection(Some(z_cut())));
    session.apply(Command::SetAnnotateMode(true));
    session.apply(Command::ViewClick { ndc: MIDDLE });
    let draft = session.draft.as_ref().expect("下書き");
    assert_eq!(draft.node, NodeId(1), "見えていない面に注釈を付けない");
    assert!(
        (draft.local_pos.z - 1.0).abs() < 1.0e-4,
        "奥の前の面: {:?}",
        draft.local_pos
    );

    // 計測（吸着を入れても切っても）: 奥の立方体の点になる。
    for snap in [true, false] {
        let mut session = support::session();
        session.apply(Command::SetTool(Tool::Measure));
        session.apply(Command::SetSnap(snap));
        session.apply(Command::SetSection(Some(z_cut())));
        session.apply(Command::ViewClick { ndc: MIDDLE });
        let doc = session.doc.as_ref().unwrap();
        let first = session.measurement.first.expect("点");
        assert_eq!(doc.id_of(first.node), Some(NodeId(1)), "吸着 {snap}");
        // 奥の角は候補になり、消えた手前の角は候補にならない。
        let far_corner = FAR_CENTER + Vec3::new(1.0, 1.0, 1.0);
        session.apply(Command::ClearMeasure);
        session.apply(Command::ViewClick {
            ndc: ndc_of(&session, far_corner, -3.0, -3.0),
        });
        let first = session.measurement.first.expect("点");
        assert_eq!(doc_id(&session, first.node), Some(NodeId(1)));
        if snap {
            assert_eq!(session.measurement.kinds[0], SnapKind::Vertex);
            assert_eq!(first.local, Vec3::new(1.0, 1.0, 1.0));
        }
    }
}

fn doc_id(session: &Session, key: model_reviewer_core::doc::NodeKey) -> Option<NodeId> {
    session.doc.as_ref().unwrap().id_of(key)
}

#[test]
fn cut_side_notes_are_dimmed_and_inert() {
    let mut session = support::session();
    // 手前の立方体の前の面と、奥の立方体の前の面に注釈。
    let review = session.review.as_mut().unwrap();
    for (id, node, z) in [(1, 0, 1.0), (2, 1, 1.0)] {
        review.annotations.push(Annotation {
            id,
            node: NodeId(node),
            local_pos: Vec3::new(0.5, 0.5, z),
            local_normal: Vec3::Z,
            text: format!("注釈{id}"),
        });
    }
    session.mark_saved();
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 800.0));
    let places = overlay::placed_markers(&session, rect);
    assert_eq!(places.len(), 2);
    assert!(places.iter().all(|p| !p.faded));
    let far = places.iter().find(|p| p.id == 2).unwrap();
    assert!(far.hidden, "断面なしでは、奥の注釈は手前の立方体に隠れる");

    session.apply(Command::SetSection(Some(z_cut())));
    let places = overlay::placed_markers(&session, rect);
    let near_note = places.iter().find(|p| p.id == 1).unwrap();
    let far = places.iter().find(|p| p.id == 2).unwrap();
    assert!(near_note.faded, "消えた側の注釈は薄く");
    assert!(!far.faded);
    assert!(!far.hidden, "消えた手前は隠さない（当たり判定と同じ平面）");
    // 注釈の一覧は変わらない。
    assert_eq!(session.review.as_ref().unwrap().annotations.len(), 2);

    // 計測の点も、消えた側なら薄い（点の順と種類はずれない）。
    session.apply(Command::SetTool(Tool::Measure));
    session.apply(Command::SetSection(None));
    let near_corner = NEAR_CENTER + Vec3::new(1.0, 1.0, 1.0);
    session.apply(Command::ViewClick {
        ndc: ndc_of(&session, near_corner, -3.0, -3.0),
    });
    session.apply(Command::ViewClick { ndc: MIDDLE });
    assert_eq!(session.measurement.kinds[0], SnapKind::Vertex);
    session.apply(Command::SetSection(Some(z_cut())));
    let (input, marks) = viewport::measure_input(&session, rect);
    assert_eq!(input.points.len(), 2);
    assert_eq!(marks.faded, [true, true]);
    assert_eq!(marks.point_kinds[0], SnapKind::Vertex);
    session.apply(Command::SetSection(Some(SectionPlane {
        flipped: true,
        ..z_cut()
    })));
    let (_, marks) = viewport::measure_input(&session, rect);
    assert_eq!(marks.faded, [false, false]);
    assert!(!session.has_unsaved());
}

/// 消えた側の注釈の印は押せない（3Dビューの上に押せる場所を置かない）。
#[test]
fn cut_side_pin_cannot_be_clicked() {
    let mut session = support::session();
    session
        .review
        .as_mut()
        .unwrap()
        .annotations
        .push(Annotation {
            id: 7,
            node: NodeId(0),
            local_pos: Vec3::new(0.5, 0.5, 1.0),
            local_normal: Vec3::Z,
            text: "手前".to_owned(),
        });
    session.mark_saved();
    let (mut harness, _) = support::harness(session, egui::vec2(1100.0, 720.0));
    for _ in 0..3 {
        harness.step();
    }
    assert!(support::widget_rect(&harness, "annotation.pin.7").is_some());
    harness
        .state_mut()
        .apply(Command::SetSection(Some(z_cut())));
    harness.step();
    harness.step();
    assert!(
        support::widget_rect(&harness, "annotation.pin.7").is_none(),
        "消えた側の印に押せる場所を置かない"
    );
    // 印のあった所を押しても、注釈は選ばれない（奥の部品が選ばれる）。
    let view = support::widget_rect(&harness, ids::VIEWPORT).unwrap();
    let world = NEAR_CENTER + Vec3::new(0.5, 0.5, 1.0);
    let ndc = harness
        .state()
        .camera
        .project(world, view.width() / view.height())
        .unwrap();
    let pos = egui::pos2(
        view.center().x + ndc.x * view.width() * 0.5,
        view.center().y - ndc.y * view.height() * 0.5,
    );
    support::click(&mut harness, pos);
    assert_eq!(harness.state().selected_annotation, None);
    assert_eq!(harness.state().selection, Some(NodeId(1)));
}

/// モデルを開く（ここでは文書を作り直す）と、断面と控えと候補が消える。
#[test]
fn opening_a_model_clears_the_section() {
    let mut session = support::session();
    session.apply(Command::SetSection(Some(SectionPlane {
        axis: Axis::X,
        offset: 0.3,
        flipped: true,
    })));
    let model = session.model.clone().unwrap();
    session.set_model(model);
    assert_eq!(session.section, None);
    session.apply(Command::ToggleSection);
    assert_eq!(
        session.section,
        Some(z_cut()),
        "控えも消え、Z 軸の中央から始まる"
    );
}

/// 変形の途中で断面・吸着を切り替えても、変形は続き、確定すれば履歴の1回分になる。
/// 取り消しても断面はそのまま（履歴に入っていない）。
#[test]
fn toggling_section_during_a_transform_keeps_the_edit() {
    let mut session = support::session();
    session.apply(Command::Select(Some(NodeId(0))));
    session.apply(Command::SetTool(Tool::Move));
    let before = session.doc.as_ref().unwrap().node(NodeId(0)).unwrap().local;
    session.apply(Command::BeginTransform {
        handle: Handle::MoveX,
        ndc: glam::Vec2::new(0.05, 0.0),
    });
    assert!(session.transform.is_some(), "{}", session.status_text);
    session.apply(Command::DragTransform {
        ndc: glam::Vec2::new(0.25, 0.0),
    });
    session.apply(Command::ToggleSection);
    session.apply(Command::ToggleSnap);
    session.apply(Command::SetDisplayUnit(Some(
        model_reviewer_core::units::UnitLabel::Mm,
    )));
    assert!(session.transform.is_some(), "切り替えは変形をやめない");
    session.apply(Command::CommitTransform);
    assert_eq!(session.history.len(), 1);
    let after = session.doc.as_ref().unwrap().node(NodeId(0)).unwrap().local;
    assert!(after.translation.x > before.translation.x + 1.0e-3);
    let plane = session.section;
    assert!(plane.is_some());
    session.apply(Command::Undo);
    assert_eq!(session.section, plane, "断面は取り消しの対象ではない");
    assert!(!session.snap.enabled, "吸着も取り消しの対象ではない");
    let undone = session.doc.as_ref().unwrap().node(NodeId(0)).unwrap().local;
    assert_eq!(undone, before);
}

/// 断面の矢印のドラッグで位置が動く（境界箱の中）。
#[test]
fn dragging_the_arrow_moves_the_plane() {
    let mut session = support::session();
    // 斜めから見る（Z 軸の矢印が画面で潰れない）。
    session.camera.yaw = 0.8;
    session.camera.pitch = 0.4;
    session.apply(Command::ToggleSection);
    let (mut harness, seen) = support::harness(session, egui::vec2(1100.0, 720.0));
    for _ in 0..3 {
        harness.step();
    }
    let handle = support::widget_rect(&harness, ids::SECTION_HANDLE).expect("矢印");
    let start = handle.center();
    harness.event(egui::Event::PointerMoved(start));
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos: start,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    for step in 1..=6 {
        harness.event(egui::Event::PointerMoved(
            start + egui::vec2(0.0, -8.0 * step as f32),
        ));
        harness.step();
    }
    harness.event(egui::Event::PointerButton {
        pos: start + egui::vec2(0.0, -48.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    let plane = harness.state().section.expect("断面");
    assert_eq!(plane.axis, Axis::Z);
    assert!(plane.offset.abs() > 1.0e-3, "動いた: {}", plane.offset);
    assert!((-3.0..=3.0).contains(&plane.offset));
    assert!(
        !seen
            .lock()
            .unwrap()
            .iter()
            .any(|command| matches!(command, Command::Orbit(_))),
        "矢印のドラッグで視点が回らない"
    );
    assert!(harness.state().history.is_empty());
}

#[test]
fn section_range_is_the_visible_whole() {
    let mut session = support::session();
    // 手前（z = 1〜3）を隠すと、見えている全体は奥（z = −3〜−1）だけ。
    session.apply(Command::SetVisible {
        node: NodeId(0),
        visible: false,
    });
    let bounds = model_reviewer::session::section_bounds(&session).expect("範囲");
    assert!(support::close3(bounds.min, Vec3::new(-1.0, -1.0, -3.0)));
    assert!(support::close3(bounds.max, Vec3::new(1.0, 1.0, -1.0)));
    // 入れると、見えている全体の中央（z = −2）から始まる。
    session.apply(Command::ToggleSection);
    let plane = session.section.expect("断面");
    assert_eq!(plane.axis, Axis::Z);
    assert!(
        support::near(plane.offset, FAR_CENTER.z),
        "{}",
        plane.offset
    );
    // 位置は見えている全体に収める（隠した手前の中は指せない）。
    session.apply(Command::SetSection(Some(SectionPlane {
        offset: NEAR_CENTER.z,
        ..plane
    })));
    assert!(support::near(session.section.unwrap().offset, -1.0));
    // 全部を隠したときは、全体の箱を使う（断面を入れられなくはならない）。
    session.apply(Command::SetVisible {
        node: NodeId(1),
        visible: false,
    });
    let all = model_reviewer::session::section_bounds(&session).unwrap();
    assert!(support::close3(all.min, Vec3::new(-1.0, -1.0, -3.0)));
    assert!(support::close3(all.max, Vec3::new(1.0, 1.0, 3.0)));
}
