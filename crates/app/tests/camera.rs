//! 視点の操作。GPUは使わない。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use model_reviewer::automation::WidgetFrame;
use model_reviewer::command::{Command, DialogButton, DialogRequest};
use model_reviewer::loader::LoadPhase;
use model_reviewer::session::{Draft, Session};
use model_reviewer::ui::ids;
use model_reviewer_core::camera::OrbitCamera;
use model_reviewer_core::review::Annotation;
use model_reviewer_core::scene::NodeId;

fn path_for(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("view-input")
        .join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("triangle.glb");
    std::fs::write(&path, model_reviewer_testdata::inputs::triangle()).unwrap();
    path
}

fn open_harness() -> (Harness<'static, Session>, Arc<Mutex<Vec<Command>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let slot = Arc::clone(&seen);
    let harness = Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_max_steps(40)
        .with_size(egui::vec2(1100.0, 720.0))
        .with_context_setup(model_reviewer::fonts::install)
        .build_ui_state(
            move |ui, session| {
                let mut commands = Vec::new();
                let pixels = model_reviewer::ui::show(ui, session, &mut commands);
                commands.extend(model_reviewer::shortcut_commands(ui.ctx(), session));
                slot.lock().unwrap().extend(commands.iter().cloned());
                for command in commands {
                    session.apply(command);
                }
                session.set_viewport(pixels);
            },
            Session::default(),
        );
    (harness, seen)
}

fn load_triangle(harness: &mut Harness<Session>, name: &str) {
    harness
        .state_mut()
        .apply(Command::OpenModel(path_for(name)));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        harness.state_mut().poll_loader();
        harness.step();
        let session = harness.state();
        let ready = matches!(session.load, LoadPhase::Idle | LoadPhase::Failed(_))
            && session.model.is_some()
            && session.pick_index.is_some();
        if ready {
            break;
        }
        if Instant::now() > deadline {
            panic!("読み込みが終わりません: {}", session.status_text);
        }
        thread::sleep(Duration::from_millis(2));
    }
    let _ = harness.try_run();
    let _ = harness.state_mut().take_scene_dirty();
    harness.step();
    let dirty = harness.state_mut().take_scene_dirty();
    assert!(
        !dirty.any(),
        "落ち着いたフレームで描き直しが残った: {dirty:?}"
    );
}

fn widget_rect(harness: &Harness<Session>, name: &str) -> egui::Rect {
    let frame = harness
        .ctx
        .data(|data| data.get_temp::<WidgetFrame>(egui::Id::NULL))
        .unwrap_or_default();
    let entry = frame
        .entries()
        .iter()
        .find(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("部品が無い: {name}"));
    egui::Rect::from_min_max(
        egui::pos2(entry.min[0], entry.min[1]),
        egui::pos2(entry.max[0], entry.max[1]),
    )
}

fn pointer(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn drag(
    harness: &mut Harness<Session>,
    from: egui::Pos2,
    to: egui::Pos2,
    button: egui::PointerButton,
    shift: bool,
) {
    if shift {
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::SHIFT));
    }
    harness.event(egui::Event::PointerMoved(from));
    harness.event(pointer(from, button, true));
    harness.step();
    harness.event(egui::Event::PointerMoved(to));
    harness.step();
    harness.event(pointer(to, button, false));
    harness.step();
    if shift {
        harness.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        harness.step();
    }
}

fn scroll_at(harness: &mut Harness<Session>, pos: egui::Pos2, delta_y: f32) {
    harness.event(egui::Event::PointerMoved(pos));
    harness.step();
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, delta_y),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Trackpad,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
}

#[test]
fn left_drag_orbits_and_shift_or_right_pans() {
    let (mut harness, _) = open_harness();
    load_triangle(&mut harness, "drag");
    let view = widget_rect(&harness, ids::VIEWPORT);
    let origin = view.center();
    let yaw = harness.state().camera.yaw;
    let target = harness.state().camera.target;
    drag(
        &mut harness,
        origin,
        origin + egui::vec2(80.0, 0.0),
        egui::PointerButton::Primary,
        false,
    );
    assert!(
        (harness.state().camera.yaw - yaw).abs() > 1.0e-3,
        "左ドラッグで向きが変わらない"
    );
    assert_eq!(harness.state().camera.target, target);

    let yaw = harness.state().camera.yaw;
    let target = harness.state().camera.target;
    drag(
        &mut harness,
        origin,
        origin + egui::vec2(0.0, 70.0),
        egui::PointerButton::Primary,
        true,
    );
    assert_eq!(harness.state().camera.yaw, yaw);
    assert_ne!(harness.state().camera.target, target);

    let yaw = harness.state().camera.yaw;
    let target = harness.state().camera.target;
    drag(
        &mut harness,
        origin,
        origin + egui::vec2(-60.0, 0.0),
        egui::PointerButton::Secondary,
        false,
    );
    assert_eq!(harness.state().camera.yaw, yaw);
    assert_ne!(harness.state().camera.target, target);
    assert!(!harness.state().unsaved());
}

#[test]
fn middle_drag_and_panel_pointer_do_not_move_the_camera() {
    let (mut harness, _) = open_harness();
    load_triangle(&mut harness, "panel");
    let view = widget_rect(&harness, ids::VIEWPORT);
    let before = harness.state().camera;
    drag(
        &mut harness,
        view.center(),
        view.center() + egui::vec2(50.0, 20.0),
        egui::PointerButton::Middle,
        false,
    );
    assert_eq!(harness.state().camera, before);

    let panel = egui::pos2(view.min.x * 0.4, view.center().y);
    assert!(panel.x < view.min.x);
    drag(
        &mut harness,
        panel,
        panel + egui::vec2(40.0, 0.0),
        egui::PointerButton::Primary,
        false,
    );
    scroll_at(&mut harness, panel, -3.0);
    assert_eq!(harness.state().camera, before);

    harness.event(egui::Event::PointerMoved(panel));
    harness.step();
    harness.key_press(egui::Key::A);
    harness.step();
    assert_eq!(harness.state().camera, before);
}

#[test]
fn trackpad_scroll_and_pinch_zoom_toward_the_cursor() {
    let (mut harness, _) = open_harness();
    load_triangle(&mut harness, "zoom");
    let view = widget_rect(&harness, ids::VIEWPORT);
    let distance = harness.state().camera.distance;
    let target = harness.state().camera.target;
    scroll_at(&mut harness, view.center(), 3.0); // 上へ回すと寄る
    assert!(harness.state().camera.distance < distance);
    assert!((harness.state().camera.target - target).length() < 1.0e-2);

    let distance = harness.state().camera.distance;
    harness.event(egui::Event::PointerMoved(view.center()));
    harness.step();
    harness.event(egui::Event::Zoom(2.0));
    harness.step();
    assert!(harness.state().camera.distance < distance * 0.6);

    let edge = view.center() + egui::vec2(80.0, -50.0);
    let target = harness.state().camera.target;
    let distance = harness.state().camera.distance;
    harness.event(egui::Event::PointerMoved(edge));
    harness.step();
    harness.event(egui::Event::Zoom(1.8));
    harness.step();
    assert!(harness.state().camera.distance < distance);
    assert!((harness.state().camera.target - target).length() > 1.0e-3);
}

#[test]
fn two_finger_drag_pans() {
    let (mut harness, _) = open_harness();
    load_triangle(&mut harness, "fingers");
    let view = widget_rect(&harness, ids::VIEWPORT);
    let center = view.center();
    let yaw = harness.state().camera.yaw;
    let target = harness.state().camera.target;
    harness.event(egui::Event::PointerMoved(center));
    harness.step();
    let touch = |id, phase, pos| egui::Event::Touch {
        device_id: egui::TouchDeviceId(1),
        id: egui::TouchId(id),
        phase,
        pos,
        force: None,
    };
    harness.event(touch(
        1,
        egui::TouchPhase::Start,
        center + egui::vec2(-30.0, 0.0),
    ));
    harness.event(touch(
        2,
        egui::TouchPhase::Start,
        center + egui::vec2(30.0, 0.0),
    ));
    harness.step();
    harness.event(touch(
        1,
        egui::TouchPhase::Move,
        center + egui::vec2(-10.0, 0.0),
    ));
    harness.event(touch(
        2,
        egui::TouchPhase::Move,
        center + egui::vec2(50.0, 0.0),
    ));
    harness.step();
    assert_eq!(harness.state().camera.yaw, yaw);
    assert_ne!(harness.state().camera.target, target);
}

#[test]
fn keys_fit_hide_and_do_nothing_while_typing_or_modified() {
    let (mut harness, seen) = open_harness();
    load_triangle(&mut harness, "keys");
    let before = harness.state().camera;
    harness.key_press(egui::Key::H);
    harness.step();
    assert!(seen.lock().unwrap().contains(&Command::HideSelected));
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::H);
    harness.step();
    assert!(seen.lock().unwrap().contains(&Command::ShowAll));

    harness.key_press(egui::Key::A);
    harness.step();
    let fitted = harness.state().camera;
    assert_eq!(fitted.target, before.target);

    harness.state_mut().camera.yaw += 0.4;
    let turned = harness.state().camera;
    harness.key_press(egui::Key::Home);
    harness.step();
    assert_ne!(harness.state().camera.yaw, turned.yaw);

    let home = harness.state().camera;
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.step();
    harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::N);
    harness.step();
    assert_eq!(harness.state().camera, home);
    assert!(!harness.state().annotate_mode);

    harness.state_mut().draft = Some(Draft {
        node: NodeId(0),
        local_pos: glam::Vec3::new(0.2, 0.2, 0.0),
        local_normal: glam::Vec3::Z,
        text: String::new(),
    });
    harness.step();
    harness.step();
    let field = widget_rect(&harness, ids::DRAFT_TEXT).center();
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(field));
    harness
        .input_mut()
        .events
        .push(pointer(field, egui::PointerButton::Primary, true));
    harness
        .input_mut()
        .events
        .push(pointer(field, egui::PointerButton::Primary, false));
    harness.step();
    assert!(
        harness.ctx.text_edit_focused(),
        "入力欄 rect={:?}",
        widget_rect(&harness, ids::DRAFT_TEXT)
    );
    let camera = harness.state().camera;
    let mode = harness.state().annotate_mode;
    harness.key_press(egui::Key::A);
    harness.step();
    harness.key_press(egui::Key::N);
    harness.step();
    harness.key_press(egui::Key::Home);
    harness.step();
    let view = widget_rect(&harness, ids::VIEWPORT);
    drag(
        &mut harness,
        view.center(),
        view.center() + egui::vec2(40.0, 0.0),
        egui::PointerButton::Primary,
        false,
    );
    assert_eq!(harness.state().camera, camera);
    assert_eq!(harness.state().annotate_mode, mode);
    assert!(harness.state().draft.is_some());
}

#[test]
fn escape_cancels_one_level_at_a_time() {
    let (mut harness, _) = open_harness();
    load_triangle(&mut harness, "escape");
    {
        let session = harness.state_mut();
        session.draft = Some(Draft {
            node: NodeId(0),
            local_pos: glam::Vec3::ZERO,
            local_normal: glam::Vec3::Y,
            text: String::new(),
        });
        session.annotate_mode = true;
        session.selection = Some(NodeId(0));
        session.selected_annotation = Some(4);
    }
    let press_escape = |harness: &mut Harness<Session>| {
        harness.key_press(egui::Key::Escape);
        harness.step();
    };
    press_escape(&mut harness);
    assert!(harness.state().draft.is_none());
    assert!(harness.state().annotate_mode);
    assert_eq!(harness.state().selection, Some(NodeId(0)));
    assert_eq!(harness.state().selected_annotation, Some(4));
    press_escape(&mut harness);
    assert!(!harness.state().annotate_mode);
    assert_eq!(harness.state().selection, Some(NodeId(0)));
    press_escape(&mut harness);
    assert!(harness.state().selection.is_none());
    assert!(harness.state().selected_annotation.is_none());
    let camera = harness.state().camera;
    press_escape(&mut harness);
    assert_eq!(harness.state().camera, camera);
}

#[test]
fn idle_frame_takes_no_scene_dirty_and_disabled_buttons_do_nothing() {
    let (mut harness, seen) = open_harness();
    harness.step();
    let camera = harness.state().camera;
    let fit = widget_rect(&harness, ids::TOOLBAR_FIT_ALL);
    harness.event(egui::Event::PointerMoved(fit.center()));
    harness.event(pointer(fit.center(), egui::PointerButton::Primary, true));
    harness.step();
    harness.event(pointer(fit.center(), egui::PointerButton::Primary, false));
    harness.step();
    assert!(!seen.lock().unwrap().contains(&Command::FitAll));
    assert_eq!(harness.state().camera, camera);
    assert!(!seen.lock().unwrap().contains(&Command::Home));

    load_triangle(&mut harness, "idle");
    harness.step();
    assert!(!harness.state_mut().take_scene_dirty().any());
}

#[test]
fn help_lists_the_same_operations() {
    let (mut harness, _) = open_harness();
    load_triangle(&mut harness, "help");
    let widget = widget_rect(&harness, ids::TOOLBAR_HELP);
    let button = harness.get_by_label("?");
    let access = button.rect();
    button.click();
    harness.step();
    // Area は出した次のフレームで大きさが決まる。
    harness.step();
    let open = harness.ctx.data(|data| {
        data.get_temp::<bool>(egui::Id::unique(ids::HELP_OPEN))
            .unwrap_or(false)
    });
    assert!(open, "ヘルプが開かない widget={widget:?} access={access:?}");
    harness.get_by_label("左ドラッグ");
    harness.get_by_label("右ドラッグ、または Shift＋左ドラッグ");
    harness.get_by_label("2本指スクロール、ピンチ");
    harness.get_by_label_contains("中ボタンは使わない");
    harness.get_by_label("A ／ F ／ Home");
    let close = widget_rect(&harness, ids::HELP_CLOSE).center();
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(close));
    harness
        .input_mut()
        .events
        .push(pointer(close, egui::PointerButton::Primary, true));
    harness
        .input_mut()
        .events
        .push(pointer(close, egui::PointerButton::Primary, false));
    harness.step();
    harness.step();
    assert!(harness.query_by_label("左ドラッグ").is_none());
}

#[test]
fn home_matches_orbit_camera_home() {
    let (mut harness, _) = open_harness();
    load_triangle(&mut harness, "home-value");
    harness.state_mut().camera.distance += 2.0;
    harness.key_press(egui::Key::Home);
    harness.step();
    let session = harness.state();
    let expected = OrbitCamera::home(
        &session.model.as_ref().unwrap().bounds,
        session.viewport_px[0] as f32 / session.viewport_px[1] as f32,
    );
    assert_eq!(session.camera, expected);
}

fn click_point(harness: &mut Harness<Session>, pos: egui::Pos2) {
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(pos));
    harness
        .input_mut()
        .events
        .push(pointer(pos, egui::PointerButton::Primary, true));
    harness
        .input_mut()
        .events
        .push(pointer(pos, egui::PointerButton::Primary, false));
    harness.step();
}

#[test]
fn dialog_blocks_shortcuts_and_escape_only_closes_cancelable_dialogs() {
    let (mut harness, _) = open_harness();
    load_triangle(&mut harness, "dialog-shortcuts");
    let view = widget_rect(&harness, ids::VIEWPORT);
    harness.event(egui::Event::PointerMoved(view.center()));
    harness.step();

    {
        let session = harness.state_mut();
        session.selection = Some(NodeId(0));
        session.annotate_mode = true;
        session.camera.yaw += 0.5;
        session.camera.distance += 1.5;
        session.dialog = Some(DialogRequest {
            title: "確認".to_owned(),
            body: "続けますか".to_owned(),
            buttons: vec![
                DialogButton {
                    id: ids::DIALOG_CONFIRM.to_owned(),
                    label: "続ける".to_owned(),
                    command: Command::ConfirmDiscard,
                },
                DialogButton {
                    id: ids::DIALOG_CANCEL.to_owned(),
                    label: "キャンセル".to_owned(),
                    command: Command::CancelPending,
                },
            ],
        });
    }
    harness.step();
    let camera_before = harness.state().camera;

    for key in [
        egui::Key::A,
        egui::Key::F,
        egui::Key::H,
        egui::Key::N,
        egui::Key::Home,
    ] {
        harness.key_press(key);
        harness.step();
    }
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::H);
    harness.step();

    assert_eq!(harness.state().camera, camera_before);
    assert!(harness.state().annotate_mode);
    assert_eq!(harness.state().selection, Some(NodeId(0)));
    assert!(
        harness
            .state()
            .review
            .as_ref()
            .unwrap()
            .visibility
            .own(NodeId(0))
    );
    assert!(harness.state().dialog.is_some());

    // キャンセル可能なダイアログでの Esc はダイアログだけ閉じる。
    harness.key_press(egui::Key::Escape);
    harness.step();
    assert!(harness.state().dialog.is_none());
    assert!(harness.state().annotate_mode);
    assert_eq!(harness.state().selection, Some(NodeId(0)));

    // 閉じる・やめるに当たるボタンが無いダイアログでは Esc が何もしない。
    harness.state_mut().dialog = Some(DialogRequest {
        title: "続行のみ".to_owned(),
        body: "確認のみ".to_owned(),
        buttons: vec![DialogButton {
            id: ids::DIALOG_CONFIRM.to_owned(),
            label: "破棄して続ける".to_owned(),
            command: Command::ConfirmDiscard,
        }],
    });
    harness.step();
    harness.key_press(egui::Key::Escape);
    harness.step();
    assert!(harness.state().dialog.is_some());
    assert!(harness.state().annotate_mode);
    assert_eq!(harness.state().selection, Some(NodeId(0)));

    // ヘルプダイアログ表示中もショートカットを止め、Esc はヘルプだけ閉じる。
    harness.state_mut().dialog = None;
    harness.ctx.data_mut(|data| {
        data.insert_temp(egui::Id::unique(ids::HELP_OPEN), true);
    });
    harness.step();
    harness.key_press(egui::Key::Home);
    harness.step();
    harness.key_press(egui::Key::N);
    harness.step();
    assert_eq!(harness.state().camera, camera_before);
    assert!(harness.state().annotate_mode);
    harness.key_press(egui::Key::Escape);
    harness.step();
    let help_open = harness.ctx.data(|data| {
        data.get_temp::<bool>(egui::Id::unique(ids::HELP_OPEN))
            .unwrap_or(false)
    });
    assert!(!help_open);
    assert!(harness.state().annotate_mode);
    assert_eq!(harness.state().selection, Some(NodeId(0)));
}

#[test]
fn escape_while_typing_only_leaves_field_or_cancels_draft() {
    let (mut harness, _) = open_harness();
    load_triangle(&mut harness, "typing-escape");
    {
        let session = harness.state_mut();
        session
            .review
            .as_mut()
            .unwrap()
            .annotations
            .push(Annotation {
                id: 1,
                node: NodeId(0),
                local_pos: glam::Vec3::new(0.2, 0.2, 0.0),
                local_normal: glam::Vec3::Z,
                text: "元の本文".to_owned(),
            });
        session.selection = Some(NodeId(0));
        session.annotate_mode = true;
    }
    harness.step();
    harness.step();

    // 注釈本文の入力中の Esc は入力欄を離れて編集を取り消すだけ。モードや選択は変えない。
    let note_field = widget_rect(&harness, &format!("{}.1", ids::NOTES_TEXT)).center();
    click_point(&mut harness, note_field);
    assert!(harness.ctx.text_edit_focused());
    harness.event(egui::Event::Text("追記".to_owned()));
    harness.step();
    harness.key_press(egui::Key::Escape);
    harness.step();
    assert!(!harness.ctx.text_edit_focused());
    assert!(harness.state().annotate_mode);
    assert_eq!(harness.state().selection, Some(NodeId(0)));
    assert_eq!(
        harness.state().review.as_ref().unwrap().annotations[0].text,
        "元の本文"
    );

    // 視点名の入力中の Esc は入力欄を離れるだけ。モードや選択は変えない。
    let view_field = widget_rect(&harness, ids::VIEWS_NAME).center();
    click_point(&mut harness, view_field);
    assert!(harness.ctx.text_edit_focused());
    harness.event(egui::Event::Text("正面".to_owned()));
    harness.step();
    harness.key_press(egui::Key::Escape);
    harness.step();
    assert!(!harness.ctx.text_edit_focused());
    assert!(harness.state().annotate_mode);
    assert_eq!(harness.state().selection, Some(NodeId(0)));

    // ダイアログ表示中の Esc は、背後の注釈本文の入力途中を消さずにダイアログだけ閉じる。
    let note_field = widget_rect(&harness, &format!("{}.1", ids::NOTES_TEXT)).center();
    click_point(&mut harness, note_field);
    harness.event(egui::Event::Text("保持".to_owned()));
    harness.step();
    assert!(harness.state().body_pending());
    harness.state_mut().dialog = Some(DialogRequest {
        title: "知らせ".to_owned(),
        body: "確認".to_owned(),
        buttons: vec![DialogButton {
            id: ids::DIALOG_CANCEL.to_owned(),
            label: "閉じる".to_owned(),
            command: Command::DismissDialog,
        }],
    });
    harness.step();
    harness.key_press(egui::Key::Escape);
    harness.step();
    assert!(harness.state().dialog.is_none());
    assert!(harness.state().body_pending());

    // 下書き欄の入力中の Esc は下書きの取消だけ。モードや選択は変えない。
    harness.state_mut().draft = Some(Draft {
        node: NodeId(0),
        local_pos: glam::Vec3::new(0.2, 0.2, 0.0),
        local_normal: glam::Vec3::Z,
        text: String::new(),
    });
    harness.step();
    harness.step();
    let draft_field = widget_rect(&harness, ids::DRAFT_TEXT).center();
    click_point(&mut harness, draft_field);
    harness.event(egui::Event::Text("途中".to_owned()));
    harness.step();
    harness.key_press(egui::Key::Escape);
    harness.step();
    assert!(harness.state().draft.is_none());
    assert!(!harness.state().draft_pending());
    assert!(harness.state().annotate_mode);
    assert_eq!(harness.state().selection, Some(NodeId(0)));
}
