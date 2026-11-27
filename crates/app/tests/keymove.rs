//! キーで動かす操作を、変形のコマンドと画面へ届くキーの両方から確かめる。GPUは使わない。

mod support;

use egui::{Event, Key, Modifiers};
use glam::{Affine3A, Vec3};
use model_reviewer::command::{Command, Handle, KeyInput, Tool};
use model_reviewer::session::{Session, TransformSource};
use model_reviewer_core::camera::grid_spacing;
use model_reviewer_core::scene::{NodeId, SceneRead};
use model_reviewer_core::section::Axis;
use model_reviewer_core::units::UnitLabel;

use support::{NEAR_CENTER, close3, near};

const PART: NodeId = NodeId(0);

/// 手前の立方体を選び、道具を `tool` にした状態。
fn selected(tool: Tool) -> Session {
    let mut session = support::session();
    session.apply(Command::Select(Some(PART)));
    session.apply(Command::SetTool(tool));
    assert_eq!(session.tool, tool);
    assert_eq!(session.selection, Some(PART));
    session
}

fn type_text(session: &mut Session, text: &str) {
    for c in text.chars() {
        session.apply(Command::KeyTransformInput(KeyInput::Char(c)));
    }
}

fn local(session: &Session) -> Affine3A {
    session.doc.as_ref().unwrap().node(PART).unwrap().local
}

fn position(session: &Session) -> Vec3 {
    local(session).translation.into()
}

fn entry_text(session: &Session) -> Option<(Handle, String)> {
    match session.transform.as_ref().map(|t| &t.source) {
        Some(TransformSource::Keys(entry)) => Some((entry.handle, entry.text.clone())),
        _ => None,
    }
}

fn delta_translation(session: &Session) -> Vec3 {
    session.transform.as_ref().unwrap().delta.translation.into()
}

fn press(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn dispatch(ctx: &egui::Context, s: &mut Session, events: Vec<Event>) -> Vec<Command> {
    let raw = egui::RawInput {
        events,
        ..Default::default()
    };
    let mut cmds = Vec::new();
    ctx.run_ui(raw, |ui| {
        cmds = model_reviewer::shortcut_commands(ui.ctx(), s)
    })
    .textures_delta
    .clear();
    for command in cmds.iter().cloned() {
        s.apply(command);
    }
    cmds
}

fn key(s: &mut Session, key: Key, mods: Modifiers) -> Vec<Command> {
    dispatch(&egui::Context::default(), s, vec![press(key, mods)])
}

#[test]
fn xyz_lock_axis_only_with_edit_tool_and_selection() {
    for tool in [Tool::Move, Tool::Rotate, Tool::Scale] {
        let mut s = selected(tool);
        for (letter, axis) in [(Key::X, Axis::X), (Key::Y, Axis::Y), (Key::Z, Axis::Z)] {
            assert_eq!(
                key(&mut s, letter, Modifiers::NONE),
                [Command::BeginKeyTransform(axis)]
            );
        }
        key(&mut s, Key::Escape, Modifiers::NONE);
        assert!(s.transform.is_none());
        assert_eq!(
            key(&mut s, Key::Z, Modifiers::SHIFT),
            [Command::SetDisplay(
                model_reviewer::command::DisplayMode::XRay
            )]
        );
        s.apply(Command::Select(None));
        assert_eq!(key(&mut s, Key::X, Modifiers::NONE), [Command::ToggleSnap]);
        assert!(key(&mut s, Key::Y, Modifiers::NONE).is_empty());
        assert!(key(&mut s, Key::ArrowRight, Modifiers::NONE).is_empty());
    }
    let mut s = selected(Tool::Move);
    s.load = model_reviewer::LoadPhase::Reading;
    assert!(key(&mut s, Key::ArrowRight, Modifiers::NONE).is_empty());
    assert!(key(&mut s, Key::Z, Modifiers::NONE).is_empty());
    assert!(s.history.is_empty());
}

#[test]
fn x_still_toggles_snap_with_select_tool() {
    for tool in [Tool::Select, Tool::Measure] {
        let mut s = selected(tool);
        let snap = s.snap.enabled;
        assert_eq!(key(&mut s, Key::X, Modifiers::NONE), [Command::ToggleSnap]);
        assert_ne!(s.snap.enabled, snap);
        assert_eq!(
            key(&mut s, Key::Z, Modifiers::NONE),
            [Command::SetDisplay(
                model_reviewer::command::DisplayMode::FacesAndLines
            )]
        );
        assert!(s.transform.is_none());
    }
}

#[test]
fn backspace_edits_amount_not_part() {
    let mut s = selected(Tool::Move);
    let n = s.scene().unwrap().node_count();
    for k in [
        Key::X,
        Key::Minus,
        Key::Num1,
        Key::Period,
        Key::Num2,
        Key::Num5,
        Key::Backspace,
    ] {
        key(&mut s, k, Modifiers::NONE);
    }
    assert_eq!(entry_text(&s), Some((Handle::MoveX, "-1.2".into())));
    assert_eq!(s.scene().unwrap().node_count(), n);
    assert!(s.history.is_empty());
    key(&mut s, Key::Enter, Modifiers::NONE);
    assert!(near(position(&s).x, NEAR_CENTER.x - 1.2));
    assert_eq!(s.history.len(), 1);
}

#[test]
fn other_keys_are_ignored_while_typing_amount() {
    let mut s = selected(Tool::Rotate);
    key(&mut s, Key::Y, Modifiers::NONE);
    for (k, mods) in [
        (Key::G, Modifiers::NONE),
        (Key::Z, Modifiers::COMMAND),
        (Key::Delete, Modifiers::NONE),
        (Key::F1, Modifiers::NONE),
        (Key::N, Modifiers::COMMAND),
        (Key::Num5, Modifiers::SHIFT),
    ] {
        assert!(key(&mut s, k, mods).is_empty());
    }
    let camera = s.camera;
    key(&mut s, Key::Num1, Modifiers::NONE);
    key(&mut s, Key::ArrowUp, Modifiers::SHIFT);
    assert_eq!(entry_text(&s), Some((Handle::RotY, "2".into())));
    assert_eq!(s.camera, camera);
    assert!(s.history.is_empty());
    key(&mut s, Key::Escape, Modifiers::NONE);
    assert!(s.transform.is_none() && !s.status_text.contains("Enter"));
}

#[test]
fn key_repeat_does_not_nudge() {
    let mut s = selected(Tool::Move);
    let grid = grid_spacing(s.camera.distance);
    let ctx = egui::Context::default();
    let down = press(Key::ArrowDown, Modifiers::SHIFT);
    assert_eq!(
        dispatch(&ctx, &mut s, vec![down.clone()]),
        [Command::Nudge {
            axis: Axis::Y,
            sign: -1,
            fine: true
        }]
    );
    assert!(dispatch(&ctx, &mut s, vec![down]).is_empty());
    assert!(near(position(&s).y, NEAR_CENTER.y - grid * 0.1));
    assert_eq!(s.history.len(), 1);
}

#[test]
fn view_click_cancels_amount() {
    let mut s = selected(Tool::Move);
    let before = local(&s);
    key(&mut s, Key::X, Modifiers::NONE);
    key(&mut s, Key::Num2, Modifiers::NONE);
    s.apply(Command::ViewClick {
        ndc: glam::Vec2::ZERO,
    });
    assert!(s.transform.is_none());
    assert_eq!(local(&s), before);
    assert!(s.history.is_empty() && !s.has_unsaved());
}

#[test]
fn axis_entry_releases_widget_focus_for_arrows_and_enter() {
    let (mut h, seen) = support::harness(selected(Tool::Move), egui::vec2(1100.0, 720.0));
    h.key_press(Key::Tab);
    h.run();
    assert!(h.ctx.memory(|m| m.focused().is_some()) && !h.ctx.text_edit_focused());
    h.key_press(Key::ArrowRight);
    h.run();
    assert!(h.state().history.is_empty());
    assert!(
        !seen
            .lock()
            .unwrap()
            .iter()
            .any(|cmd| matches!(cmd, Command::Nudge { .. }))
    );
    h.key_press(Key::X);
    h.run();
    assert_eq!(entry_text(h.state()), Some((Handle::MoveX, String::new())));
    assert!(h.ctx.memory(|m| m.focused().is_none()));
    h.key_press(Key::ArrowUp);
    h.run();
    h.key_press(Key::Enter);
    h.run();
    assert!(h.state().transform.is_none());
    let grid = grid_spacing(h.state().camera.distance);
    assert!(near(position(h.state()).x, NEAR_CENTER.x + grid));
}

#[test]
fn fast_keys_use_the_result_of_the_preceding_key() {
    use eframe::App as _;
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let mut app =
        model_reviewer::App::new(&cc, model_reviewer::parse_args(["model-reviewer"]).unwrap());
    *app.session_mut() = selected(Tool::Select);
    let events = [
        Key::G,
        Key::X,
        Key::Num1,
        Key::Num2,
        Key::Backspace,
        Key::Enter,
    ]
    .into_iter()
    .map(|k| press(k, Modifiers::NONE))
    .collect();
    let raw = egui::RawInput {
        events,
        ..Default::default()
    };
    let mut frame = eframe::Frame::_new_kittest();
    ctx.run_ui(raw, |ui| app.ui(ui, &mut frame))
        .textures_delta
        .clear();
    let s = app.session();
    assert_eq!(s.tool, Tool::Move);
    assert!(s.transform.is_none());
    assert!(near(position(s).x, NEAR_CENTER.x + 1.0));
    assert_eq!(s.history.len(), 1);
}

#[test]
fn typed_amount_previews_without_touching_document() {
    let mut session = selected(Tool::Move);
    let before = local(&session);
    let history = session.history.len();
    session.apply(Command::BeginKeyTransform(Axis::X));
    assert_eq!(entry_text(&session), Some((Handle::MoveX, String::new())));
    assert_eq!(
        session.transform.as_ref().unwrap().handle(),
        Some(Handle::MoveX),
        "固定した軸のハンドルを掴んだ見た目にする"
    );
    type_text(&mut session, "12.5");
    assert_eq!(
        entry_text(&session),
        Some((Handle::MoveX, "12.5".to_owned()))
    );
    assert!(close3(
        delta_translation(&session),
        Vec3::new(12.5, 0.0, 0.0)
    ));
    // 打っている間は、文書・履歴・未保存の印が変わらない。
    assert_eq!(local(&session), before);
    assert_eq!(session.history.len(), history);
    assert!(!session.unsaved());
    assert!(
        session
            .status_text
            .starts_with("移動 X: 12.5 ｜ Enter で確定・Esc でやめる"),
        "{}",
        session.status_text
    );
    // 数字以外と、2つ目の小数点・途中の負号は受けない。Backspace は1文字消す。
    type_text(&mut session, ".-a");
    assert_eq!(entry_text(&session).unwrap().1, "12.5");
    session.apply(Command::KeyTransformInput(KeyInput::Backspace));
    assert_eq!(entry_text(&session).unwrap().1, "12.");
    assert!(close3(
        delta_translation(&session),
        Vec3::new(12.0, 0.0, 0.0)
    ));
    // Esc（取り消し）で何も変わらない。
    session.apply(Command::CancelTransform);
    assert!(session.transform.is_none());
    assert_eq!(local(&session), before);
    assert_eq!(session.history.len(), history);
    assert!(!session.unsaved());
}

#[test]
fn scale_that_cannot_form_a_local_matrix_stays_editable() {
    let mut s = selected(Tool::Scale);
    s.apply(Command::CommitLocal(model_reviewer::command::Trs {
        scale: Vec3::splat(1.0e-6),
        ..Default::default()
    }));
    let before = local(&s);
    s.apply(Command::BeginKeyTransform(Axis::X));
    type_text(&mut s, "0.000000000000001");
    s.apply(Command::CommitTransform);
    assert_eq!(local(&s), before);
    assert!(entry_text(&s).is_some());
}

#[test]
fn overflowing_scale_keeps_the_entry_until_corrected() {
    let mut s = selected(Tool::Scale);
    s.apply(Command::CommitLocal(model_reviewer::command::Trs {
        translation: Vec3::X * 1.0e30,
        ..Default::default()
    }));
    let before = local(&s);
    let rev = s.history.doc_rev();
    s.apply(Command::BeginKeyTransform(Axis::X));
    type_text(&mut s, "2000000000");
    assert!(s.status_text.contains("大きすぎ"));
    s.apply(Command::CommitTransform);
    assert_eq!(local(&s), before);
    assert_eq!(s.history.doc_rev(), rev);
    assert_eq!(entry_text(&s), Some((Handle::ScaleX, "2000000000".into())));
    assert!(s.status_text.contains("大きすぎ"));
    s.apply(Command::KeyTransformInput(KeyInput::Backspace));
    s.apply(Command::CommitTransform);
    assert!(entry_text(&s).is_none());
    assert_eq!(local(&s).matrix3.x_axis.x, 200000000.0);
}

#[test]
fn enter_commits_one_history_step() {
    let mut session = selected(Tool::Move);
    let history = session.history.len();
    session.apply(Command::BeginKeyTransform(Axis::X));
    type_text(&mut session, "-2.5");
    session.apply(Command::CommitTransform);
    assert!(session.transform.is_none());
    assert!(close3(
        position(&session),
        NEAR_CENTER + Vec3::new(-2.5, 0.0, 0.0)
    ));
    assert_eq!(session.history.len(), history + 1);
    assert_eq!(session.history.undo_label(), Some("移動"));
    assert!(session.unsaved());
    session.apply(Command::Undo);
    assert!(close3(position(&session), NEAR_CENTER));

    // 回転は度、拡大は倍率。どちらも1回分で、名前は「回転」「拡大」。
    let mut session = selected(Tool::Rotate);
    session.apply(Command::BeginKeyTransform(Axis::Z));
    type_text(&mut session, "90");
    session.apply(Command::CommitTransform);
    assert_eq!(session.history.undo_label(), Some("回転"));
    let x = local(&session).transform_vector3(Vec3::X);
    assert!(close3(x, Vec3::Y), "{x:?}");
    assert!(
        close3(position(&session), NEAR_CENTER),
        "中心のまわりに回る"
    );

    let mut session = selected(Tool::Scale);
    session.apply(Command::BeginKeyTransform(Axis::Y));
    type_text(&mut session, "1.5");
    assert!(
        session.status_text.starts_with("拡大 Y: ×1.5"),
        "{}",
        session.status_text
    );
    session.apply(Command::CommitTransform);
    assert_eq!(session.history.undo_label(), Some("拡大"));
    let y = local(&session).transform_vector3(Vec3::Y);
    assert!(close3(y, Vec3::new(0.0, 1.5, 0.0)), "{y:?}");
}

#[test]
fn axis_switch_keeps_amount() {
    let mut session = selected(Tool::Move);
    session.apply(Command::BeginKeyTransform(Axis::X));
    type_text(&mut session, "3");
    session.apply(Command::BeginKeyTransform(Axis::Y));
    assert_eq!(entry_text(&session), Some((Handle::MoveY, "3".to_owned())));
    assert!(close3(
        delta_translation(&session),
        Vec3::new(0.0, 3.0, 0.0)
    ));
    assert!(
        session.status_text.starts_with("移動 Y: 3"),
        "{}",
        session.status_text
    );
    session.apply(Command::BeginKeyTransform(Axis::Z));
    session.apply(Command::CommitTransform);
    assert!(close3(
        position(&session),
        NEAR_CENTER + Vec3::new(0.0, 0.0, 3.0)
    ));

    // 矢印（Step）は今の量に1歩を足し引きし、文字に書き戻す。回転は 15 度、Shift で 1 度。
    let mut session = selected(Tool::Rotate);
    session.apply(Command::BeginKeyTransform(Axis::X));
    let up = |fine| Command::KeyTransformInput(KeyInput::Step { up: true, fine });
    session.apply(up(false));
    session.apply(up(false));
    session.apply(up(true));
    assert_eq!(entry_text(&session).unwrap().1, "31");
    session.apply(Command::KeyTransformInput(KeyInput::Step {
        up: false,
        fine: false,
    }));
    assert_eq!(entry_text(&session).unwrap().1, "16");
    // 拡大は空なら 1 から始め、0.1 ずつ。
    let mut session = selected(Tool::Scale);
    session.apply(Command::BeginKeyTransform(Axis::X));
    session.apply(up(false));
    assert_eq!(entry_text(&session).unwrap().1, "1.1");
}

#[test]
fn move_amount_is_read_in_display_unit() {
    let mut session = selected(Tool::Move);
    assert_eq!(session.units.source, UnitLabel::M);
    session.apply(Command::SetDisplayUnit(Some(UnitLabel::Mm)));
    session.apply(Command::BeginKeyTransform(Axis::X));
    type_text(&mut session, "12.5");
    let grid_mm = session
        .units
        .to_display(grid_spacing(session.camera.distance));
    let expected = format!(
        "移動 X: 12.5 mm ｜ Enter で確定・Esc でやめる・矢印で {} mm ずつ",
        model_reviewer::session::keymove::format_amount(f64::from(grid_mm))
    );
    assert_eq!(session.status_text, expected);
    // 矢印の1歩も表示の単位で足す（格子 1 m・表示 mm なら 1000）。
    session.apply(Command::KeyTransformInput(KeyInput::Step {
        up: true,
        fine: false,
    }));
    assert_eq!(
        entry_text(&session).unwrap().1,
        model_reviewer::session::keymove::format_amount(12.5 + f64::from(grid_mm))
    );
    session.apply(Command::KeyTransformInput(KeyInput::Step {
        up: false,
        fine: false,
    }));
    assert_eq!(entry_text(&session).unwrap().1, "12.5");
    session.apply(Command::CommitTransform);
    let moved = position(&session) - NEAR_CENTER;
    assert!(near(moved.x, 0.0125), "{moved:?}");
    assert!(near(moved.y, 0.0) && near(moved.z, 0.0));
}

#[test]
fn zero_or_empty_amount_changes_nothing() {
    for (tool, text) in [
        (Tool::Move, ""),
        (Tool::Move, "-"),
        (Tool::Move, "0"),
        (Tool::Move, "-0.0"),
        (Tool::Rotate, ""),
        (Tool::Rotate, "0"),
        (Tool::Scale, ""),
        (Tool::Scale, "1"),
        (Tool::Scale, "1.00"),
    ] {
        let mut session = selected(tool);
        let before = local(&session);
        let history = session.history.len();
        session.apply(Command::BeginKeyTransform(Axis::X));
        type_text(&mut session, text);
        assert_eq!(
            session.transform.as_ref().unwrap().delta,
            Affine3A::IDENTITY,
            "{tool:?} {text:?}"
        );
        session.apply(Command::CommitTransform);
        assert!(session.transform.is_none(), "{tool:?} {text:?}");
        assert_eq!(local(&session), before, "{tool:?} {text:?}");
        assert_eq!(session.history.len(), history, "{tool:?} {text:?}");
        assert!(!session.unsaved(), "{tool:?} {text:?}");
    }

    // 拡大の倍率が 0 以下なら、差分を変えずに理由を出し、確定を断って入力を続ける（要件11.10）。
    for text in ["0", "-2"] {
        let mut session = selected(Tool::Scale);
        let history = session.history.len();
        session.apply(Command::BeginKeyTransform(Axis::Z));
        type_text(&mut session, "2");
        session.apply(Command::KeyTransformInput(KeyInput::Backspace));
        type_text(&mut session, text);
        assert!(
            session
                .status_text
                .contains("倍率は 0 より大きい数にしてください"),
            "{}",
            session.status_text
        );
        session.apply(Command::CommitTransform);
        assert!(session.transform.is_some(), "{text}: 入力を続ける");
        assert_eq!(session.history.len(), history, "{text}");
        assert!(
            session.status_text.contains("倍率は 0 より大きい"),
            "{text}"
        );
    }
}

#[test]
fn nudge_moves_one_grid_step() {
    let mut session = selected(Tool::Move);
    session.apply(Command::SetDisplayUnit(Some(UnitLabel::Mm)));
    let grid = grid_spacing(session.camera.distance);
    let history = session.history.len();
    session.apply(Command::Nudge {
        axis: Axis::X,
        sign: 1,
        fine: false,
    });
    assert!(close3(
        position(&session),
        NEAR_CENTER + Vec3::new(grid, 0.0, 0.0)
    ));
    assert_eq!(session.history.len(), history + 1);
    assert_eq!(session.history.undo_label(), Some("移動"));
    let mm =
        model_reviewer::session::keymove::format_amount(f64::from(session.units.to_display(grid)));
    assert_eq!(session.status_text, format!("移動 X: +{mm} mm"));
    // Shift で10分の1。↓ は Y の負。
    session.apply(Command::Nudge {
        axis: Axis::Y,
        sign: -1,
        fine: true,
    });
    assert!(close3(
        position(&session),
        NEAR_CENTER + Vec3::new(grid, -grid * 0.1, 0.0)
    ));
    assert_eq!(session.history.len(), history + 2);
    assert!(
        session.status_text.starts_with("移動 Y: −"),
        "{}",
        session.status_text
    );

    // 移動の道具でない、選択が無い、数値の入力の途中は、何もしない。
    let nudge = Command::Nudge {
        axis: Axis::X,
        sign: 1,
        fine: false,
    };
    for tool in [Tool::Select, Tool::Rotate, Tool::Scale, Tool::Measure] {
        let mut other = selected(tool);
        let before = local(&other);
        other.apply(nudge.clone());
        assert_eq!(local(&other), before, "{tool:?}");
        assert_eq!(other.history.len(), 0, "{tool:?}");
    }
    let mut none = selected(Tool::Move);
    none.apply(Command::Select(None));
    none.apply(nudge.clone());
    assert_eq!(none.history.len(), 0);
    let mut typing = selected(Tool::Move);
    typing.apply(Command::BeginKeyTransform(Axis::X));
    typing.apply(nudge);
    assert_eq!(typing.history.len(), 0);
    assert!(typing.transform.is_some());
}

#[test]
fn nudge_does_not_snap_to_grid() {
    let mut session = selected(Tool::Move);
    session.apply(Command::BeginKeyTransform(Axis::X));
    type_text(&mut session, "0.37");
    session.apply(Command::CommitTransform);
    let start = position(&session);
    assert!(near(start.x, 0.37));
    let grid = grid_spacing(session.camera.distance);
    session.apply(Command::Nudge {
        axis: Axis::X,
        sign: 1,
        fine: false,
    });
    session.apply(Command::Nudge {
        axis: Axis::X,
        sign: -1,
        fine: true,
    });
    let now = position(&session);
    assert!(near(now.x, 0.37 + grid - grid * 0.1), "{now:?}");
    assert!(near(now.z, start.z));
}

/// 選択の道具・選択が無い・ハンドルを掴んでいる途中では、向きの固定を始めない。
#[test]
fn begin_needs_edit_tool_selection_and_no_drag() {
    for tool in [Tool::Select, Tool::Measure, Tool::Shape] {
        let mut session = selected(tool);
        session.apply(Command::BeginKeyTransform(Axis::X));
        assert!(session.transform.is_none(), "{tool:?}");
    }
    let mut session = selected(Tool::Move);
    session.apply(Command::Select(None));
    session.apply(Command::BeginKeyTransform(Axis::X));
    assert!(session.transform.is_none());

    let mut session = selected(Tool::Move);
    let ndc = support::ndc_of(&session, NEAR_CENTER, 0.0, 0.0);
    let grab = model_reviewer::session::gizmo_frame(&session, [800.0, 800.0], 90.0)
        .map(|frame| {
            model_reviewer::ui::gizmo_view::grab_points(Tool::Move, &frame)
                .into_iter()
                .find(|(handle, _)| *handle == Handle::MoveX)
                .map(|(_, p)| glam::Vec2::new(p.x / 400.0 - 1.0, 1.0 - p.y / 400.0))
                .unwrap()
        })
        .unwrap_or(ndc);
    session.apply(Command::BeginTransform {
        handle: Handle::MoveX,
        ndc: grab,
    });
    assert!(matches!(
        session.transform.as_ref().map(|t| &t.source),
        Some(TransformSource::Gizmo(_))
    ));
    session.apply(Command::BeginKeyTransform(Axis::Y));
    assert!(matches!(
        session.transform.as_ref().map(|t| &t.source),
        Some(TransformSource::Gizmo(_))
    ));
}
