//! 道具・ハンドル・数値の欄・取り消しのボタンを、画面の部品を通して確かめる。GPUは使わない。

use std::sync::{Arc, Mutex};

use egui_kittest::Harness;
use glam::{Vec2, Vec3};
use model_reviewer::automation::WidgetFrame;
use model_reviewer::command::{Command, Tool, ViewDir};
use model_reviewer::session::Session;
use model_reviewer::ui::{ids, overlay};
use model_reviewer_core::camera::{OrbitCamera, Projection};
use model_reviewer_core::gizmo::decompose;
use model_reviewer_core::identity::{ModelIdentity, ModelRef};
use model_reviewer_core::math::Ray;
use model_reviewer_core::pick::{PickIndex, pick};
use model_reviewer_core::review::ReviewState;
use model_reviewer_core::scene::{NodeId, SceneRead};

const ARM: NodeId = NodeId(9);
const BASE: NodeId = NodeId(1);

fn sample_session() -> Session {
    let bytes = model_reviewer_testdata::sample_glb();
    let model = Arc::new(model_reviewer_gltf::load_glb(&bytes).expect("見本のGLB"));
    let mut session = Session::default();
    session.review = Some(ReviewState::new(&model));
    session.model_ref = Some(ModelRef {
        file_name: "sample.glb".to_owned(),
        path: "sample.glb".to_owned(),
        relative_path: None,
        identity: ModelIdentity::from_bytes(&bytes),
        stl_unit: None,
    });
    session.pick_index = Some(PickIndex::build(&model));
    session.set_model(model);
    session
}

type Seen = Arc<Mutex<Vec<Command>>>;

fn harness(session: Session) -> (Harness<'static, Session>, Seen) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let slot = Arc::clone(&seen);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_max_steps(40)
        .with_size(egui::vec2(1100.0, 720.0))
        .with_context_setup(model_reviewer::fonts::install)
        .build_ui_state(
            move |ui, session| {
                session.pixels_per_point = ui.pixels_per_point();
                let mut commands = Vec::new();
                let pixels = model_reviewer::ui::show(ui, session, &mut commands);
                commands.extend(model_reviewer::shortcut_commands(ui.ctx(), session));
                slot.lock().unwrap().extend(commands.iter().cloned());
                for command in commands {
                    session.apply(command);
                }
                session.set_viewport(pixels);
            },
            session,
        );
    for _ in 0..4 {
        harness.step();
    }
    // 開いた直後の視点を、3Dビューの大きさに合わせて置き直す。
    let session = harness.state_mut();
    let [w, h] = session.viewport_px;
    let bounds = session.doc.as_ref().unwrap().bounds();
    session.camera = OrbitCamera::home(&bounds, w as f32 / h as f32);
    harness.step();
    harness.step();
    (harness, seen)
}

fn widget_rect(harness: &Harness<Session>, name: &str) -> Option<egui::Rect> {
    let frame = harness
        .ctx
        .data(|data| data.get_temp::<WidgetFrame>(egui::Id::NULL))
        .unwrap_or_default();
    frame
        .entries()
        .iter()
        .find(|entry| entry.name == name)
        .map(|entry| {
            egui::Rect::from_min_max(
                egui::pos2(entry.min[0], entry.min[1]),
                egui::pos2(entry.max[0], entry.max[1]),
            )
        })
}

fn rect_of(harness: &Harness<Session>, name: &str) -> egui::Rect {
    widget_rect(harness, name).unwrap_or_else(|| panic!("部品が無い: {name}"))
}

fn button(harness: &mut Harness<Session>, pos: egui::Pos2, pressed: bool) {
    harness.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    });
}

fn click(harness: &mut Harness<Session>, pos: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(pos));
    harness.step();
    button(harness, pos, true);
    button(harness, pos, false);
    harness.step();
    harness.step();
}

/// ハンドルの上に置いてから押し、`delta` だけ数フレームで動かす。`escape` なら離す前に Esc。
fn drag_handle(harness: &mut Harness<Session>, name: &str, delta: egui::Vec2, escape: bool) {
    let start = rect_of(harness, name).center();
    harness.event(egui::Event::PointerMoved(start));
    harness.step();
    harness.step();
    button(harness, start, true);
    harness.step();
    let steps = 5;
    for i in 1..=steps {
        let pos = start + delta * (i as f32 / steps as f32);
        harness.event(egui::Event::PointerMoved(pos));
        harness.step();
    }
    if escape {
        harness.key_press(egui::Key::Escape);
        harness.step();
    }
    button(harness, start + delta, false);
    harness.step();
    harness.step();
}

fn select_arm_with_move(harness: &mut Harness<Session>) {
    harness.state_mut().apply(Command::Select(Some(ARM)));
    // 1文字のキーは、ポインタがパネルの上にあると働かない（第1段階の約束）。
    let view = rect_of(harness, ids::VIEWPORT).center();
    harness.event(egui::Event::PointerMoved(view));
    harness.step();
    harness.key_press(egui::Key::G);
    harness.step();
    harness.step();
    harness.step();
    assert_eq!(harness.state().tool, Tool::Move);
}

fn local_of(session: &Session, id: NodeId) -> glam::Affine3A {
    session.doc.as_ref().unwrap().node(id).unwrap().local
}

#[test]
fn dragging_a_handle_commits_once_and_the_undo_button_reverts() {
    let (mut harness, seen) = harness(sample_session());
    select_arm_with_move(&mut harness);
    let before = local_of(harness.state(), ARM);
    seen.lock().unwrap().clear();
    drag_handle(
        &mut harness,
        ids::GIZMO_MOVE_X,
        egui::vec2(60.0, 0.0),
        false,
    );
    let session = harness.state();
    assert!(session.transform.is_none());
    assert_eq!(session.history.len(), 1, "離したときに1回分");
    assert_ne!(local_of(session, ARM), before);
    let commands = seen.lock().unwrap().clone();
    assert!(
        commands
            .iter()
            .any(|c| matches!(c, Command::BeginTransform { .. })),
        "{commands:?}"
    );
    assert!(commands.contains(&Command::CommitTransform));
    // 視点は回っていない（ハンドルが押下を受けた）。
    assert!(
        !commands.iter().any(|c| matches!(c, Command::Orbit(_))),
        "{commands:?}"
    );

    let undo = rect_of(&harness, ids::TOOLBAR_UNDO).center();
    click(&mut harness, undo);
    assert_eq!(local_of(harness.state(), ARM), before);
    assert!(!harness.state().history.can_undo());
    let redo = rect_of(&harness, ids::TOOLBAR_REDO).center();
    click(&mut harness, redo);
    assert_ne!(local_of(harness.state(), ARM), before);
}

#[test]
fn escape_during_a_handle_drag_cancels_without_history() {
    let (mut harness, seen) = harness(sample_session());
    select_arm_with_move(&mut harness);
    let before = local_of(harness.state(), ARM);
    seen.lock().unwrap().clear();
    drag_handle(&mut harness, ids::GIZMO_MOVE_X, egui::vec2(60.0, 0.0), true);
    let session = harness.state();
    assert!(session.transform.is_none());
    assert!(session.history.is_empty(), "Esc の取り消しは履歴に残らない");
    assert_eq!(local_of(session, ARM), before);
    assert_eq!(session.tool, Tool::Move, "Esc は1回に1つだけ（道具は残る）");
    assert!(!seen.lock().unwrap().contains(&Command::CommitTransform));
}

#[test]
fn typing_a_number_and_enter_commits_it() {
    let (mut harness, _) = harness(sample_session());
    harness.state_mut().apply(Command::Select(Some(ARM)));
    harness.step();
    harness.step();
    let field = rect_of(&harness, ids::INSPECTOR_POS_X).center();
    click(&mut harness, field);
    assert!(harness.ctx.text_edit_focused(), "欄に入力位置");
    // 打っている間、Cmd+Z は欄の中の文字の取り消しで、アプリの取り消しには働かない。
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.step();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.event(egui::Event::Text("1.25".to_owned()));
    harness.step();
    harness.key_press(egui::Key::G);
    harness.step();
    assert_eq!(harness.state().tool, Tool::Select);
    assert!(harness.state().history.is_empty());
    harness.key_press(egui::Key::Enter);
    harness.step();
    harness.step();
    let session = harness.state();
    assert_eq!(session.history.len(), 1, "Enter で1回分");
    let trs = decompose(&local_of(session, ARM)).unwrap();
    assert!((trs.translation.x - 1.25).abs() < 1.0e-5, "{trs:?}");
}

#[test]
fn tool_keys_and_undo_keys_are_ignored_while_typing() {
    let (mut harness, seen) = harness(sample_session());
    harness.state_mut().apply(Command::Select(Some(ARM)));
    harness.state_mut().apply(Command::HideSelected);
    harness.step();
    let field = rect_of(&harness, ids::VIEWS_NAME).center();
    click(&mut harness, field);
    assert!(harness.ctx.text_edit_focused());
    seen.lock().unwrap().clear();
    for key in [
        egui::Key::V,
        egui::Key::G,
        egui::Key::R,
        egui::Key::S,
        egui::Key::M,
    ] {
        harness.key_press(key);
        harness.step();
    }
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.step();
    harness.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::Z,
    );
    harness.step();
    let seen = seen.lock().unwrap();
    assert!(
        !seen
            .iter()
            .any(|c| matches!(c, Command::SetTool(_) | Command::Undo | Command::Redo)),
        "入力中に道具と取り消しが積まれた: {seen:?}"
    );
    assert_eq!(harness.state().history.len(), 1);
}

#[test]
fn keys_pick_tools_and_cmd_z_undoes() {
    let (mut harness, _) = harness(sample_session());
    harness.state_mut().apply(Command::Select(Some(ARM)));
    harness.state_mut().apply(Command::HideSelected);
    harness.step();
    for (key, tool) in [
        (egui::Key::R, Tool::Rotate),
        (egui::Key::S, Tool::Scale),
        (egui::Key::M, Tool::Measure),
        (egui::Key::G, Tool::Move),
        (egui::Key::V, Tool::Select),
    ] {
        harness.key_press(key);
        harness.step();
        assert_eq!(harness.state().tool, tool);
    }
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.step();
    assert!(!harness.state().history.can_undo());
    harness.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::Z,
    );
    harness.step();
    assert!(harness.state().history.can_undo());
    // 道具の列のボタンでも選べる。
    let rotate = rect_of(&harness, ids::TOOL_ROTATE).center();
    click(&mut harness, rotate);
    assert_eq!(harness.state().tool, Tool::Rotate);
}

#[test]
fn undo_buttons_are_disabled_without_history_and_idle_frames_do_not_redraw() {
    let (mut harness, seen) = harness(sample_session());
    let undo = rect_of(&harness, ids::TOOLBAR_UNDO).center();
    seen.lock().unwrap().clear();
    click(&mut harness, undo);
    assert!(!seen.lock().unwrap().contains(&Command::Undo), "押せない");
    // ハンドルを出した状態で、何もしないフレームとハンドルに触れるだけでは描き直さない。
    select_arm_with_move(&mut harness);
    harness.step();
    let _ = harness.state_mut().take_scene_dirty();
    for _ in 0..3 {
        harness.step();
        let dirty = harness.state_mut().take_scene_dirty();
        assert!(!dirty.any(), "何もしないフレーム: {dirty:?}");
    }
    let handle = rect_of(&harness, ids::GIZMO_MOVE_Y).center();
    harness.event(egui::Event::PointerMoved(handle));
    harness.step();
    harness.step();
    let dirty = harness.state_mut().take_scene_dirty();
    assert!(!dirty.any(), "触れただけ: {dirty:?}");
}

#[test]
fn moving_a_part_moves_its_annotation_marker_during_the_drag() {
    let mut session = sample_session();
    // arm の中に注釈を1つ置く。
    let hit = model_reviewer_core::pick::Hit {
        node: ARM,
        t: 0.0,
        world_pos: Vec3::ZERO,
        local_pos: Vec3::new(0.0, 0.1, 0.0),
        local_normal: Vec3::Y,
        prim: model_reviewer_core::scene::PrimId(0),
        triangle: 0,
    };
    session
        .review
        .as_mut()
        .unwrap()
        .add_annotation(&hit, "先".to_owned());
    let (mut harness, _) = harness(session);
    select_arm_with_move(&mut harness);
    let rect = rect_of(&harness, ids::VIEWPORT);
    let before = overlay::placed_markers(harness.state(), rect)[0].center;
    let start = rect_of(&harness, ids::GIZMO_MOVE_X).center();
    harness.event(egui::Event::PointerMoved(start));
    harness.step();
    harness.step();
    button(&mut harness, start, true);
    harness.step();
    for i in 1..=4 {
        harness.event(egui::Event::PointerMoved(
            start + egui::vec2(15.0 * i as f32, 0.0),
        ));
        harness.step();
    }
    assert!(harness.state().transform.is_some(), "掴んでいる");
    let during = overlay::placed_markers(harness.state(), rect)[0].center;
    assert!(
        during.x > before.x + 20.0,
        "印が部品と一緒に動く: {before:?} → {during:?}"
    );
    assert_eq!(harness.state().history.len(), 0);
    button(&mut harness, start + egui::vec2(60.0, 0.0), false);
    harness.step();
    let after = overlay::placed_markers(harness.state(), rect)[0].center;
    assert!((after - during).length() < 2.0, "確定の後も同じ場所");
}

/// 平行投影では、印の隠れ判定も平行な視線で行う。目が部品の内側にあるほど近づいても、
/// 目の後ろの面（平行投影では描かれる）で隠れる。
#[test]
fn hidden_markers_use_parallel_rays_in_orthographic_view() {
    let mut session = sample_session();
    session.set_viewport([800, 600]);
    let aspect = 800.0 / 600.0;
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    // 台の上面の点を、真上からの視線で求める。
    let top = {
        let doc = session.doc.as_ref().unwrap();
        let review = session.review.as_ref().unwrap();
        let ray = Ray {
            origin: Vec3::new(0.3, 10.0, -0.3),
            dir: Vec3::NEG_Y,
        };
        pick(
            doc,
            session.pick_index.as_ref().unwrap(),
            &review.visibility,
            &ray,
        )
        .expect("上面")
    };
    assert_eq!(top.node, BASE);
    let review = session.review.as_mut().unwrap();
    review.add_annotation(&top, "上面".to_owned());
    let mut camera = OrbitCamera::home(&session.doc.as_ref().unwrap().bounds(), aspect);
    camera.projection = Projection::Orthographic;
    camera.target = top.world_pos;
    camera.snap(ViewDir::Top);
    session.camera = camera;
    let seen_from_above = overlay::placed_markers(&session, rect);
    assert!(!seen_from_above[0].hidden, "上からは見える");
    // 下から、注視点にごく近づく。目は台の中にあり、目から点への線は台の下面を通らない。
    camera.snap(ViewDir::Bottom);
    camera.distance = 0.02;
    session.camera = camera;
    let eye = session.camera.eye();
    assert!(
        eye.y < top.world_pos.y && eye.y > top.world_pos.y - 0.2,
        "目は台の中"
    );
    let from_below = overlay::placed_markers(&session, rect);
    assert!(from_below[0].hidden, "平行な視線では台の下面に隠れる");
    // 同じ点と視線を透視にすると、目から点の間に面は無い。
    session.camera.projection = Projection::Perspective;
    let ndc = session.camera.project(top.world_pos, aspect).unwrap();
    assert!(!overlay::is_hidden(&session, top.world_pos, ndc, aspect));
    let _ = Vec2::ZERO;
}

#[test]
fn shape_buttons_add_and_cmd_d_and_delete_keys_work_but_not_while_typing() {
    let (mut harness, seen) = harness(sample_session());
    // 図形の道具 → 形のボタン。置いた図形が選ばれ、移動の道具になる。
    let tool = rect_of(&harness, ids::TOOL_SHAPE).center();
    click(&mut harness, tool);
    assert_eq!(harness.state().tool, Tool::Shape);
    let sphere = rect_of(&harness, ids::SHAPE_SPHERE).center();
    click(&mut harness, sphere);
    let count = harness.state().doc.as_ref().unwrap().node_count();
    assert_eq!(count, 13);
    let shape = harness.state().selection.unwrap();
    assert_eq!(harness.state().doc.as_ref().unwrap().name(shape), "球");
    assert_eq!(harness.state().tool, Tool::Move);
    // 部品の一覧にも出る。
    harness.step();
    assert!(widget_rect(&harness, &format!("parts.row.{}", shape.0)).is_some());

    // 入力欄に文字を打っている間は、Cmd+D・Delete・Backspace が働かない。
    let field = rect_of(&harness, ids::VIEWS_NAME).center();
    click(&mut harness, field);
    assert!(harness.ctx.text_edit_focused());
    seen.lock().unwrap().clear();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::D);
    harness.step();
    harness.key_press(egui::Key::Delete);
    harness.step();
    harness.key_press(egui::Key::Backspace);
    harness.step();
    assert!(
        !seen
            .lock()
            .unwrap()
            .iter()
            .any(|c| matches!(c, Command::DuplicateSelected | Command::DeleteSelected)),
        "入力中に複製・削除が積まれた"
    );
    assert_eq!(harness.state().doc.as_ref().unwrap().node_count(), 13);

    // 3Dビューの上では効く。
    harness
        .ctx
        .memory_mut(|memory| memory.surrender_focus(memory.focused().unwrap()));
    let view = rect_of(&harness, ids::VIEWPORT).center();
    harness.event(egui::Event::PointerMoved(view));
    harness.step();
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::D);
    harness.step();
    harness.step();
    assert_eq!(harness.state().doc.as_ref().unwrap().node_count(), 14);
    let copy = harness.state().selection.unwrap();
    assert_eq!(
        harness.state().doc.as_ref().unwrap().name(copy),
        "球 コピー"
    );
    harness.key_press(egui::Key::Backspace);
    harness.step();
    harness.step();
    assert_eq!(harness.state().doc.as_ref().unwrap().node_count(), 13);
    assert_eq!(harness.state().status_text, "部品 1 個を削除しました");
    assert_eq!(harness.state().selection, None);
    harness.state_mut().apply(Command::Select(Some(shape)));
    harness.key_press(egui::Key::Delete);
    harness.step();
    harness.step();
    assert_eq!(harness.state().doc.as_ref().unwrap().node_count(), 12);
    // 取り消しのボタンで戻る。
    let undo = rect_of(&harness, ids::TOOLBAR_UNDO).center();
    click(&mut harness, undo);
    assert_eq!(harness.state().doc.as_ref().unwrap().node_count(), 13);
    assert_eq!(harness.state().selection, Some(shape));
}

#[test]
fn shape_buttons_are_disabled_without_a_model() {
    let mut session = Session::default();
    session.apply(Command::SetTool(Tool::Shape));
    assert_eq!(
        session.tool,
        Tool::Select,
        "モデルが無いと図形の道具を選べない"
    );
    let input = model_reviewer::ui::toolrail::ToolRailInput {
        tool: Tool::Shape,
        has_model: false,
        edit_blocked: None,
    };
    assert_eq!(
        model_reviewer::ui::toolrail::shape_disabled_reason(&input).as_deref(),
        Some(model_reviewer::ui::toolrail::NO_MODEL_REASON)
    );
}
