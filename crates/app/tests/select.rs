//! クリックの選択と注釈の印。GPUは使わない。

use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use glam::Vec3;
use model_reviewer::automation::WidgetFrame;
use model_reviewer::command::Command;
use model_reviewer::loader::LoadPhase;
use model_reviewer::session::Session;
use model_reviewer::ui::ids;
use model_reviewer::ui::overlay::{self, MarkerPlace};
use model_reviewer_core::review::Annotation;
use model_reviewer_core::scene::NodeId;

fn path_for(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("view-select")
        .join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("triangle.glb");
    std::fs::write(&path, model_reviewer_testdata::inputs::triangle()).unwrap();
    path
}

fn load_into(session: &mut Session, name: &str) {
    session.apply(Command::OpenModel(path_for(name)));
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let _ = session.poll_loader();
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
    session.set_viewport([800, 600]);
}

fn point_ndc(session: &Session, point: Vec3) -> glam::Vec2 {
    let aspect = session.viewport_px[0] as f32 / session.viewport_px[1] as f32;
    session
        .camera
        .project(point, aspect)
        .unwrap_or_else(|| panic!("点が画面に乗らない"))
}

fn open_harness() -> Harness<'static, Session> {
    Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_max_steps(40)
        .with_size(egui::vec2(1100.0, 720.0))
        .with_context_setup(model_reviewer::fonts::install)
        .build_ui_state(
            |ui, session| {
                let mut commands = Vec::new();
                let pixels = model_reviewer::ui::show(ui, session, &mut commands);
                commands.extend(model_reviewer::shortcut_commands(ui.ctx(), session));
                for command in commands {
                    session.apply(command);
                }
                session.set_viewport(pixels);
            },
            Session::default(),
        )
}

fn load_harness(harness: &mut Harness<Session>, name: &str) {
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

fn click_same(harness: &mut Harness<Session>, pos: egui::Pos2) {
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(pos));
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
}

fn click_at(harness: &mut Harness<Session>, pos: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(pos));
    harness.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
}

fn screen_of(ndc: glam::Vec2, rect: egui::Rect) -> egui::Pos2 {
    egui::pos2(
        rect.center().x + ndc.x * rect.width() * 0.5,
        rect.center().y - ndc.y * rect.height() * 0.5,
    )
}

#[test]
fn click_selects_the_triangle_and_a_hidden_part_does_not() {
    let mut session = Session::default();
    load_into(&mut session, "hit");
    let ndc = point_ndc(&session, Vec3::new(0.2, 0.2, 0.0));
    let _ = session.take_scene_dirty();
    session.apply(Command::ViewClick { ndc });
    assert_eq!(session.selection, Some(NodeId(0)));
    assert!(session.status_text.is_empty());
    assert!(!session.unsaved());
    assert!(!session.take_scene_dirty().camera);
    assert!(session.review.as_ref().unwrap().annotations.is_empty());

    let model = session.model.clone().unwrap();
    session
        .review
        .as_mut()
        .unwrap()
        .visibility
        .set(&model, NodeId(0), false);
    session.apply(Command::ViewClick { ndc });
    assert!(session.selection.is_none());
    assert_eq!(session.status_text, "表面をクリックしてください");
    assert!(session.review.as_ref().unwrap().annotations.is_empty());
}

#[test]
fn miss_and_unready_index_do_not_create_an_annotation() {
    let mut session = Session::default();
    load_into(&mut session, "miss");
    session.apply(Command::SetAnnotateMode(true));
    session.apply(Command::ViewClick {
        ndc: glam::Vec2::new(0.98, 0.98),
    });
    assert!(session.draft.is_none());
    assert!(session.review.as_ref().unwrap().annotations.is_empty());
    assert_eq!(session.status_text, "表面をクリックしてください");
    assert!(session.selection.is_none());

    session.pick_index = None;
    session.selection = Some(NodeId(0));
    session.apply(Command::ViewClick {
        ndc: glam::Vec2::new(0.0, 0.0),
    });
    assert_eq!(session.status_text, "選択の準備中です");
    assert_eq!(session.selection, Some(NodeId(0)));
    assert!(session.draft.is_none());
}

#[test]
fn markers_skip_hidden_parts_the_back_side_and_keep_list_numbers() {
    let mut session = Session::default();
    load_into(&mut session, "pins");
    let review = session.review.as_mut().unwrap();
    review.annotations.push(Annotation {
        id: 3,
        node: NodeId(9),
        local_pos: Vec3::ZERO,
        local_normal: Vec3::Y,
        text: "無い".to_owned(),
    });
    review.annotations.push(Annotation {
        id: 8,
        node: NodeId(0),
        local_pos: Vec3::new(0.2, 0.2, 0.0),
        local_normal: Vec3::Z,
        text: "角".to_owned(),
    });
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    let places = overlay::placed_markers(&session, rect);
    assert_eq!(places.len(), 1);
    assert_eq!(
        places[0],
        MarkerPlace {
            id: 8,
            number: 2,
            node: NodeId(0),
            center: places[0].center,
            selected: false,
            hidden: false,
        }
    );

    session.selected_annotation = Some(8);
    let selected = overlay::placed_markers(&session, rect);
    assert!(selected[0].selected);

    let model = session.model.clone().unwrap();
    session
        .review
        .as_mut()
        .unwrap()
        .visibility
        .set(&model, NodeId(0), false);
    assert!(overlay::placed_markers(&session, rect).is_empty());

    session
        .review
        .as_mut()
        .unwrap()
        .visibility
        .set(&model, NodeId(0), true);
    session.camera.yaw = 0.0;
    session.camera.pitch = 0.0;
    session.camera.distance = 3.0;
    session.camera.target = Vec3::ZERO;
    session.review.as_mut().unwrap().annotations[1].local_pos = Vec3::new(0.0, 0.0, 10.0);
    assert!(overlay::placed_markers(&session, rect).is_empty());
}

/// 目と点の間に面があれば「隠れている印」。点の面そのものは隠れとみなさない。
#[test]
fn markers_behind_a_surface_are_marked_hidden() {
    let mut session = Session::default();
    load_into(&mut session, "hidden");
    let model = session.model.clone().unwrap();
    let front = Vec3::new(0.2, 0.2, 0.0);
    let eye = session.camera.eye();
    let behind = front + (front - eye).normalize() * 0.5;
    let inverse = model.nodes[0].world.inverse();
    let review = session.review.as_mut().unwrap();
    for (id, world) in [(1, front), (2, behind)] {
        review.annotations.push(Annotation {
            id,
            node: NodeId(0),
            local_pos: inverse.transform_point3(world),
            local_normal: Vec3::Z,
            text: format!("{id}"),
        });
    }
    let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
    let places = overlay::placed_markers(&session, rect);
    assert_eq!(places.len(), 2);
    assert!(!places[0].hidden, "面の上の点は隠れない");
    assert!(places[1].hidden, "面の奥の点は隠れている");

    // 索引ができるまでは通常の印。
    session.pick_index = None;
    assert!(
        overlay::placed_markers(&session, rect)
            .iter()
            .all(|place| !place.hidden)
    );
}

#[test]
fn screen_click_selects_and_a_drag_does_not() {
    let mut harness = open_harness();
    load_harness(&mut harness, "screen");
    let view = widget_rect(&harness, ids::VIEWPORT);
    let ndc = {
        let session = harness.state();
        let aspect = session.viewport_px[0] as f32 / session.viewport_px[1] as f32;
        session
            .camera
            .project(Vec3::new(0.2, 0.2, 0.0), aspect)
            .unwrap()
    };
    let pos = screen_of(ndc, view);
    assert!(view.contains(pos));
    click_at(&mut harness, pos);
    assert_eq!(harness.state().selection, Some(NodeId(0)));

    harness.state_mut().selection = None;
    let yaw = harness.state().camera.yaw;
    let from = pos;
    let to = pos + egui::vec2(36.0, 8.0);
    harness.event(egui::Event::PointerMoved(from));
    harness.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    harness.event(egui::Event::PointerMoved(to));
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    assert!(harness.state().selection.is_none());
    assert!((harness.state().camera.yaw - yaw).abs() > 1.0e-3);
}

#[test]
fn annotate_click_drafts_and_commit_registers_without_an_empty_body() {
    let mut harness = open_harness();
    load_harness(&mut harness, "draft");
    harness.key_press(egui::Key::N);
    harness.step();
    assert!(harness.state().annotate_mode);
    let view = widget_rect(&harness, ids::VIEWPORT);
    let yaw = harness.state().camera.yaw;
    let from = view.center();
    harness.event(egui::Event::PointerMoved(from));
    harness.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    harness.event(egui::Event::PointerMoved(from + egui::vec2(48.0, 0.0)));
    harness.step();
    harness.event(egui::Event::PointerButton {
        pos: from + egui::vec2(48.0, 0.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    assert!(harness.state().draft.is_none());
    assert!((harness.state().camera.yaw - yaw).abs() > 1.0e-3);
    assert!(harness.state().annotate_mode);

    let ndc = {
        let session = harness.state();
        let aspect = session.viewport_px[0] as f32 / session.viewport_px[1] as f32;
        session
            .camera
            .project(Vec3::new(0.2, 0.2, 0.0), aspect)
            .unwrap()
    };
    let pos = screen_of(ndc, widget_rect(&harness, ids::VIEWPORT));
    click_at(&mut harness, pos);
    assert!(harness.state().draft.is_some());
    assert!(
        harness
            .state()
            .review
            .as_ref()
            .unwrap()
            .annotations
            .is_empty()
    );
    harness.step();
    harness.step();
    let register = widget_rect(&harness, ids::DRAFT_COMMIT);
    let yaw = harness.state().camera.yaw;
    click_at(&mut harness, register.center());
    assert!(
        harness.state().draft.is_some(),
        "mode={} yaw_delta={} register={register:?}",
        harness.state().annotate_mode,
        harness.state().camera.yaw - yaw
    );
    assert!(
        harness
            .state()
            .review
            .as_ref()
            .unwrap()
            .annotations
            .is_empty()
    );

    let field = widget_rect(&harness, ids::DRAFT_TEXT).center();
    click_same(&mut harness, field);
    harness.event(egui::Event::Text("角を直す".to_owned()));
    harness.step();
    let register = widget_rect(&harness, ids::DRAFT_COMMIT).center();
    click_same(&mut harness, register);
    let notes = &harness.state().review.as_ref().unwrap().annotations;
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].text, "角を直す");
    assert!(harness.state().draft.is_none());
    assert!(harness.state().unsaved());
    assert!(harness.state().annotate_mode);

    let ndc = {
        let session = harness.state();
        let aspect = session.viewport_px[0] as f32 / session.viewport_px[1] as f32;
        session
            .camera
            .project(Vec3::new(0.7, 0.05, 0.0), aspect)
            .unwrap()
    };
    let view = widget_rect(&harness, ids::VIEWPORT);
    let pos = screen_of(ndc, view);
    let marker = overlay::placed_markers(harness.state(), view)[0].center;
    assert!(
        marker.distance(pos) > 24.0,
        "印と次のクリックが近すぎる: {}",
        marker.distance(pos)
    );
    click_at(&mut harness, pos);
    assert!(
        harness.state().draft.is_some(),
        "status={} selection={:?} note={}",
        harness.state().status_text,
        harness.state().selected_annotation,
        harness.state().selection.is_some()
    );
    harness.step();
    harness.step();
    let field = widget_rect(&harness, ids::DRAFT_TEXT).center();
    click_same(&mut harness, field);
    harness.event(egui::Event::Text("もう一つ".to_owned()));
    harness.step();
    harness.key_press(egui::Key::Enter);
    harness.step();
    let notes = &harness.state().review.as_ref().unwrap().annotations;
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[1].text, "もう一つ");
    assert!(harness.state().draft.is_none());
}

/// 面を押した直後に、入力欄を押さずに打ち始められること。打った文字は
/// ショートカット（H で非表示、N でモード切り替え）として働かない。
#[test]
fn typing_right_after_placing_a_draft_goes_into_the_note() {
    let mut harness = open_harness();
    load_harness(&mut harness, "autofocus");
    harness.key_press(egui::Key::N);
    harness.step();
    let ndc = {
        let session = harness.state();
        let aspect = session.viewport_px[0] as f32 / session.viewport_px[1] as f32;
        session
            .camera
            .project(Vec3::new(0.2, 0.2, 0.0), aspect)
            .unwrap()
    };
    let pos = screen_of(ndc, widget_rect(&harness, ids::VIEWPORT));
    for round in 1..=2 {
        click_at(&mut harness, pos);
        assert!(harness.state().draft.is_some(), "round {round}");
        harness.step();
        harness.step();
        for key in [egui::Key::H, egui::Key::N] {
            harness.key_press(key);
            harness.step();
        }
        harness.event(egui::Event::Text("hn 直す".to_owned()));
        harness.step();
        assert!(harness.state().annotate_mode, "round {round}");
        let review = harness.state().review.as_ref().unwrap();
        assert_eq!(review.visibility.hidden_own().count(), 0, "round {round}");
        if round == 1 {
            // 取り消して同じ場所に置き直しても、もう一度入力位置が移る。
            harness.key_press(egui::Key::Escape);
            harness.step();
            harness.step();
            assert!(harness.state().draft.is_none());
        }
    }
    // Shift＋Enter は改行で、登録しない。
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Enter);
    harness.step();
    harness.event(egui::Event::Text("次".to_owned()));
    harness.step();
    assert!(harness.state().draft.is_some());
    harness.key_press(egui::Key::Enter);
    harness.step();
    let notes = &harness.state().review.as_ref().unwrap().annotations;
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].text, "hn 直す\n次");
}

/// ウィンドウが前面に無くても（自動操作など）、Enter で登録できる。
#[test]
fn enter_commits_a_draft_when_the_window_is_not_focused() {
    let mut harness = open_harness();
    load_harness(&mut harness, "unfocused");
    harness.key_press(egui::Key::N);
    harness.step();
    let ndc = {
        let session = harness.state();
        let aspect = session.viewport_px[0] as f32 / session.viewport_px[1] as f32;
        session
            .camera
            .project(Vec3::new(0.2, 0.2, 0.0), aspect)
            .unwrap()
    };
    let pos = screen_of(ndc, widget_rect(&harness, ids::VIEWPORT));
    click_at(&mut harness, pos);
    harness.step();
    harness.step();
    harness.input_mut().focused = false;
    harness.event(egui::Event::Text("前面でない".to_owned()));
    harness.step();
    harness.input_mut().focused = false;
    harness.key_press(egui::Key::Enter);
    harness.step();
    let notes = &harness.state().review.as_ref().unwrap().annotations;
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].text, "前面でない");
}

#[test]
fn marker_click_selects_the_note_and_does_not_start_a_draft() {
    let mut harness = open_harness();
    load_harness(&mut harness, "marker");
    harness.state_mut().apply(Command::SetAnnotateMode(true));
    harness
        .state_mut()
        .review
        .as_mut()
        .unwrap()
        .annotations
        .push(Annotation {
            id: 5,
            node: NodeId(0),
            local_pos: Vec3::new(0.2, 0.2, 0.0),
            local_normal: Vec3::Z,
            text: "既存".to_owned(),
        });
    harness.step();
    let view = widget_rect(&harness, ids::VIEWPORT);
    let center = overlay::placed_markers(harness.state(), view)[0].center;
    click_at(&mut harness, center);
    assert!(harness.state().draft.is_none());
    assert_eq!(harness.state().selected_annotation, Some(5));
    assert_eq!(harness.state().selection, Some(NodeId(0)));
    assert_eq!(
        harness.state().review.as_ref().unwrap().annotations.len(),
        1
    );
}
