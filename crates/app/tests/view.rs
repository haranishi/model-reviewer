//! 見る操作の接合：決まった向き・投影・表示・格子。GPUは使わない。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use model_reviewer::automation::WidgetFrame;
use model_reviewer::command::{Command, DisplayMode, Projection, ViewDir};
use model_reviewer::loader::LoadPhase;
use model_reviewer::session::Session;
use model_reviewer::ui::ids;
use model_reviewer::ui::viewbar::NO_LINES_REASON;

fn path_for(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("view-modes")
        .join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("triangle.glb");
    std::fs::write(&path, model_reviewer_testdata::inputs::triangle()).unwrap();
    path
}

fn open_harness(size: egui::Vec2) -> (Harness<'static, Session>, Arc<Mutex<Vec<Command>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let slot = Arc::clone(&seen);
    let harness = Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_max_steps(40)
        .with_size(size)
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
        if matches!(session.load, LoadPhase::Idle | LoadPhase::Failed(_))
            && session.model.is_some()
            && session.pick_index.is_some()
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "読み込みが終わりません: {}",
            session.status_text
        );
        thread::sleep(Duration::from_millis(2));
    }
    let _ = harness.try_run();
    let _ = harness.state_mut().take_scene_dirty();
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

fn click(harness: &mut Harness<Session>, pos: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(pos));
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    harness.step();
    harness.step();
}

#[test]
fn commands_change_view_projection_display_and_grid_without_marking_unsaved() {
    let (mut harness, _) = open_harness(egui::vec2(1100.0, 720.0));
    load_triangle(&mut harness, "commands");
    let session = harness.state_mut();
    session.set_lines_supported(true);
    let _ = session.take_scene_dirty();
    assert!(session.grid_visible, "格子は既定で出す");
    assert_eq!(session.display, DisplayMode::Faces);

    let target = session.camera.target;
    let distance = session.camera.distance;
    session.apply(Command::SnapView(ViewDir::Top));
    assert!((session.camera.pitch.to_degrees() - 90.0).abs() < 1.0e-4);
    assert!(session.camera.yaw.abs() < 1.0e-6);
    assert_eq!(session.camera.target, target, "注視点は変えない");
    assert_eq!(session.camera.distance, distance, "距離は変えない");
    assert!(session.take_scene_dirty().camera);
    // 同じ向きをもう一度選んでも描き直さない。
    session.apply(Command::SnapView(ViewDir::Top));
    assert!(!session.take_scene_dirty().any());

    session.apply(Command::SnapView(ViewDir::Right));
    assert!((session.camera.yaw.to_degrees() - 90.0).abs() < 1.0e-4);
    assert!(session.camera.pitch.abs() < 1.0e-6);
    let _ = session.take_scene_dirty();

    session.apply(Command::ToggleProjection);
    assert_eq!(session.camera.projection, Projection::Orthographic);
    assert!(session.take_scene_dirty().camera);
    session.apply(Command::SetProjection(Projection::Orthographic));
    assert!(!session.take_scene_dirty().any());
    session.apply(Command::ToggleProjection);
    assert_eq!(session.camera.projection, Projection::Perspective);
    let _ = session.take_scene_dirty();

    for mode in [
        DisplayMode::FacesAndLines,
        DisplayMode::Lines,
        DisplayMode::XRay,
        DisplayMode::Faces,
    ] {
        session.apply(Command::SetDisplay(mode));
        assert_eq!(session.display, mode);
        let dirty = session.take_scene_dirty();
        assert!(dirty.display && !dirty.instances, "{mode:?}: {dirty:?}");
    }
    session.apply(Command::SetGrid(false));
    assert!(!session.grid_visible);
    assert!(session.take_scene_dirty().display);
    session.apply(Command::SetGrid(false));
    assert!(!session.take_scene_dirty().any());

    assert!(!session.has_unsaved(), "見る操作は未保存にしない");

    // 投影は初期視点と開き直しで保つ。
    session.apply(Command::SetProjection(Projection::Orthographic));
    session.apply(Command::Home);
    assert_eq!(session.camera.projection, Projection::Orthographic);
}

#[test]
fn lines_are_refused_with_a_reason_when_the_gpu_lacks_them() {
    let (mut harness, _) = open_harness(egui::vec2(1100.0, 720.0));
    load_triangle(&mut harness, "no-lines");
    let session = harness.state_mut();
    assert!(!session.lines_supported, "GPUが無いときは線を使えない");
    session.apply(Command::SetDisplay(DisplayMode::Lines));
    assert_eq!(session.display, DisplayMode::Faces);
    assert_eq!(session.status_text, NO_LINES_REASON);
    assert!(!session.take_scene_dirty().any());
    // 透かしは線が要らない。
    session.apply(Command::SetDisplay(DisplayMode::XRay));
    assert_eq!(session.display, DisplayMode::XRay);
}

#[test]
fn keys_and_buttons_drive_the_view() {
    let (mut harness, seen) = open_harness(egui::vec2(1100.0, 720.0));
    load_triangle(&mut harness, "keys");
    harness.state_mut().set_lines_supported(true);
    harness.step();

    harness.key_press(egui::Key::Num7);
    harness.step();
    assert!((harness.state().camera.pitch.to_degrees() - 90.0).abs() < 1.0e-4);
    harness.key_press(egui::Key::Num3);
    harness.step();
    assert!((harness.state().camera.yaw.to_degrees() - 90.0).abs() < 1.0e-4);
    harness.key_press(egui::Key::Num1);
    harness.step();
    assert!(harness.state().camera.yaw.abs() < 1.0e-6);
    harness.key_press(egui::Key::Num5);
    harness.step();
    assert_eq!(harness.state().camera.projection, Projection::Orthographic);
    harness.key_press(egui::Key::Z);
    harness.step();
    assert_eq!(harness.state().display, DisplayMode::FacesAndLines);
    harness.key_press(egui::Key::Z);
    harness.step();
    assert_eq!(harness.state().display, DisplayMode::Faces);
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Z);
    harness.step();
    assert_eq!(harness.state().display, DisplayMode::XRay);
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Z);
    harness.step();
    assert_eq!(harness.state().display, DisplayMode::Faces);
    // Cmd+Z は取り消し（接合3）なので、表示を変えない。
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    harness.step();
    assert_eq!(harness.state().display, DisplayMode::Faces);

    // 上端のボタンと立方体。
    let pos = rect_of(&harness, ids::VIEW_DISPLAY_XRAY).center();
    click(&mut harness, pos);
    assert_eq!(harness.state().display, DisplayMode::XRay);
    let pos = rect_of(&harness, ids::VIEW_PROJECTION_PERSPECTIVE).center();
    click(&mut harness, pos);
    assert_eq!(harness.state().camera.projection, Projection::Perspective);
    let pos = rect_of(&harness, ids::VIEW_GRID).center();
    click(&mut harness, pos);
    assert!(!harness.state().grid_visible);
    // 前の面が見える向きへ回してから、立方体の前の面を押す。
    harness.state_mut().camera.yaw = 0.5;
    harness.state_mut().camera.pitch = 0.3;
    harness.step();
    harness.step();
    let front = rect_of(&harness, ids::VIEWCUBE_FRONT).center();
    click(&mut harness, front);
    assert!(
        seen.lock()
            .unwrap()
            .contains(&Command::SnapView(ViewDir::Front)),
        "立方体の前の面で前から見る"
    );
    assert!(harness.state().camera.yaw.abs() < 1.0e-6);
}

#[test]
fn view_keys_do_nothing_while_typing() {
    let (mut harness, seen) = open_harness(egui::vec2(1100.0, 720.0));
    load_triangle(&mut harness, "typing");
    harness.state_mut().set_lines_supported(true);
    harness.step();
    let field = rect_of(&harness, ids::VIEWS_NAME).center();
    click(&mut harness, field);
    assert!(harness.ctx.text_edit_focused(), "視点の名前の欄に入力位置");
    let camera = harness.state().camera;
    let display = harness.state().display;
    seen.lock().unwrap().clear();
    for key in [
        egui::Key::Num1,
        egui::Key::Num3,
        egui::Key::Num7,
        egui::Key::Num5,
        egui::Key::Z,
    ] {
        harness.key_press(key);
        harness.step();
    }
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Z);
    harness.step();
    assert_eq!(harness.state().camera, camera);
    assert_eq!(harness.state().display, display);
    let seen = seen.lock().unwrap();
    assert!(
        !seen.iter().any(|command| matches!(
            command,
            Command::SnapView(_) | Command::ToggleProjection | Command::SetDisplay(_)
        )),
        "入力中に見る操作が積まれた: {seen:?}"
    );
}

#[test]
fn idle_frames_and_hovering_the_view_controls_do_not_redraw() {
    let (mut harness, _) = open_harness(egui::vec2(1100.0, 720.0));
    load_triangle(&mut harness, "idle");
    harness.step();
    let _ = harness.state_mut().take_scene_dirty();
    for _ in 0..3 {
        harness.step();
        let dirty = harness.state_mut().take_scene_dirty();
        assert!(!dirty.any(), "何もしないフレームで描き直しの印: {dirty:?}");
    }
    // 立方体と上端のボタンに触れても、強調は egui の描画なので3Dを描き直さない。
    for name in [ids::VIEWCUBE_FRONT, ids::VIEW_DISPLAY_FACES, ids::VIEW_GRID] {
        let pos = rect_of(&harness, name).center();
        harness.event(egui::Event::PointerMoved(pos));
        harness.step();
        harness.step();
        let dirty = harness.state_mut().take_scene_dirty();
        assert!(!dirty.any(), "{name} に触れて描き直しの印: {dirty:?}");
    }
}
