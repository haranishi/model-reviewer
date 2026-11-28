//! キーの振り分け（`shortcut_commands`）を、操作とキーの表を通して確かめる。GPUは使わない。
//! 利用者の設定ファイルには触れない（`Session::default` は場所の無い持ち手を使う）。

mod support;

use egui::{Event, Key, Modifiers};
use model_reviewer::command::{Command, DisplayMode, ShapeKind, Tool, ViewDir};
use model_reviewer::keymap::store::{Os, StoreSource, resolve_path};
use model_reviewer::keymap::{Action, Chord, Refusal};
use model_reviewer::shortcut_commands;
use model_reviewer::ui::ids;
use model_reviewer::{App, LaunchArgs, Session};
use model_reviewer_core::scene::NodeId;
use std::{fs, path::PathBuf};

fn press(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// 画面を描かずに、1フレームに届いたキーから積まれる `Command` を返す。
fn dispatch_events(ctx: &egui::Context, session: &Session, events: Vec<Event>) -> Vec<Command> {
    let raw = egui::RawInput {
        events,
        ..Default::default()
    };
    let mut out = Vec::new();
    let mut output = ctx.run_ui(raw, |ui| out = shortcut_commands(ui.ctx(), session));
    // 描かないので、文字の画像の差分は捨てる。
    output.textures_delta.clear();
    out
}

fn dispatch(session: &Session, key: Key, modifiers: Modifiers) -> Vec<Command> {
    dispatch_events(
        &egui::Context::default(),
        session,
        vec![press(key, modifiers)],
    )
}

const NONE: Modifiers = Modifiers::NONE;
const SHIFT: Modifiers = Modifiers::SHIFT;
const CMD: Modifiers = Modifiers::COMMAND;

fn shift_cmd() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::COMMAND
    }
}

fn selected() -> Session {
    let mut session = support::session();
    session.apply(Command::Select(Some(NodeId(0))));
    session
}

fn with_tool(tool: Tool) -> Session {
    let mut session = support::session();
    session.tool = tool;
    session
}

fn with_display(display: DisplayMode) -> Session {
    let mut session = support::session();
    session.display = display;
    session
}

fn begin(session: &mut Session, action: Action) {
    session.apply(Command::SetKeysOpen(true));
    session.apply(Command::BeginKeyCapture(action));
}

fn send(session: &mut Session, events: Vec<Event>) {
    for command in dispatch_events(&egui::Context::default(), session, events) {
        session.apply(command);
    }
}

fn bind(session: &mut Session, action: Action, key: Key) {
    begin(session, action);
    send(session, vec![press(key, NONE)]);
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "model-reviewer-keys-flow-{name}-{}",
        std::process::id()
    ));
    fs::create_dir(&dir).unwrap();
    dir
}

fn headless(launch: LaunchArgs) -> App {
    App::new(
        &eframe::CreationContext::_new_kittest(egui::Context::default()),
        launch,
    )
}

#[test]
fn capture_takes_the_next_key() {
    let mut s = selected();
    begin(&mut s, Action::ToolMove);
    send(
        &mut s,
        vec![
            Event::Text("B".into()),
            press(Key::B, SHIFT),
            press(Key::S, NONE),
        ],
    );
    assert_eq!(
        s.keymap.chord(Action::ToolMove),
        Chord::parse("Shift+B").ok()
    );
    assert_eq!(s.key_labels.get(Action::ToolMove), Some("Shift+B"));
    assert!(s.keys.capture.is_none());
    assert_eq!(s.tool, Tool::Select);
    assert!(s.history.is_empty());
    assert!(!s.has_unsaved());
}

#[test]
fn escape_cancels_capture() {
    let mut s = Session::default();
    begin(&mut s, Action::ToolMove);
    send(&mut s, vec![press(Key::Escape, NONE)]);
    assert!(s.keys.open && s.keys.capture.is_none());
    assert_eq!(s.keymap.rev(), 0);
    send(&mut s, vec![press(Key::Escape, NONE)]);
    assert!(!s.keys.open);
}

#[test]
fn refused_key_keeps_capturing() {
    let mut s = Session::default();
    begin(&mut s, Action::ToolMove);
    send(&mut s, vec![press(Key::Enter, NONE)]);
    assert_eq!(s.keys.refusal, Some((Action::ToolMove, Refusal::Fixed)));
    assert_eq!(s.keys.capture, Some(Action::ToolMove));
    assert_eq!(s.keymap.rev(), 0);
    send(&mut s, vec![press(Key::T, NONE)]);
    assert!(s.keys.refusal.is_none() && s.keys.capture.is_none());
    assert_eq!(s.key_labels.get(Action::ToolMove), Some("T"));
}

#[test]
fn clipboard_event_explains_itself() {
    let mut s = Session::default();
    begin(&mut s, Action::ToolMove);
    for event in [Event::Copy, Event::Cut, Event::Paste("文字".into())] {
        let cmds = dispatch_events(
            &egui::Context::default(),
            &s,
            vec![event, press(Key::T, NONE)],
        );
        assert_eq!(cmds, [Command::CaptureKey(Err(Refusal::TextEditing))]);
        for cmd in cmds {
            s.apply(cmd);
        }
        assert_eq!(
            s.keys.refusal,
            Some((Action::ToolMove, Refusal::TextEditing))
        );
        assert_eq!(s.keys.capture, Some(Action::ToolMove));
        assert_eq!(s.keymap.rev(), 0);
    }
}

#[test]
fn conflict_replace_unbinds_the_holder() {
    let mut s = selected();
    bind(&mut s, Action::ToolRotate, Key::G);
    assert_eq!(s.keys.conflict.unwrap().holder, Action::ToolMove);
    assert!(s.keys.capture.is_none());
    assert_eq!(s.keymap.rev(), 0);
    send(&mut s, vec![press(Key::Escape, NONE)]);
    assert!(s.keys.open && s.keys.conflict.is_none());
    assert_eq!(s.keymap.rev(), 0);
    bind(&mut s, Action::ToolRotate, Key::G);
    s.apply(Command::ResolveKeyConflict { replace: true });
    assert!(s.keys.conflict.is_none());
    assert_eq!(s.key_labels.get(Action::ToolRotate), Some("G"));
    assert_eq!(s.key_labels.get(Action::ToolMove), None);
    // 行を初期値に戻すときも、重なる相手から黙って取り上げない。
    s.apply(Command::ResetKey(Action::ToolMove));
    assert_eq!(s.keys.conflict.unwrap().holder, Action::ToolRotate);
    s.apply(Command::ResolveKeyConflict { replace: false });
    assert_eq!(s.key_labels.get(Action::ToolRotate), Some("G"));
    s.apply(Command::ResetAllKeys);
    assert_eq!(s.key_labels.get(Action::ToolMove), None);
    s.apply(Command::ArmResetAllKeys(true));
    s.apply(Command::ResetAllKeys);
    assert!(!s.keys.reset_all_armed);
    assert!(Action::ALL.iter().all(|&a| !s.keymap.is_changed(a)));
}

#[test]
fn rebound_key_works_immediately() {
    let mut s = selected();
    bind(&mut s, Action::ToolMove, Key::T);
    s.apply(Command::SetKeysOpen(false));
    send(&mut s, vec![press(Key::G, NONE)]);
    assert_eq!(s.tool, Tool::Select);
    send(&mut s, vec![press(Key::T, NONE)]);
    assert_eq!(s.tool, Tool::Move);
    s.apply(Command::ClearKey(Action::ToolMove));
    assert!(dispatch(&s, Key::T, NONE).is_empty());
    assert_eq!(s.key_labels.get(Action::ToolMove), None);
    s.apply(Command::ResetKey(Action::ToolMove));
    assert_eq!(s.key_labels.get(Action::ToolMove), Some("G"));

    bind(&mut s, Action::ToolShape, Key::B);
    s.apply(Command::SetKeysOpen(false));
    let commands = dispatch(&s, Key::B, NONE);
    assert_eq!(commands, [Command::SetTool(Tool::Shape)]);
    for command in commands {
        s.apply(command);
    }
    assert_eq!(s.tool, Tool::Shape);

    let mut empty = Session::default();
    bind(&mut empty, Action::FileNew, Key::T);
    empty.apply(Command::AddShape(ShapeKind::Cube));
    assert_eq!(
        empty.status_text,
        "［新規作成］（T）か、モデルを開くと図形を置けます"
    );
    empty.apply(Command::ClearKey(Action::FileNew));
    empty.apply(Command::AddShape(ShapeKind::Cube));
    assert_eq!(
        empty.status_text,
        "［新規作成］か、モデルを開くと図形を置けます"
    );
}

#[test]
fn opened_document_paths_are_protected_from_keymap_saves() {
    let dir = scratch("document-path");
    let path = dir.join("keymap.json");
    let alias = dir.join("document.json");
    let bytes = br#"{"format":"model-reviewer-keymap","version":1,"bindings":{}}"#;
    fs::write(&path, bytes).unwrap();
    fs::hard_link(&path, &alias).unwrap();
    for when in ["launch", "review", "model"] {
        let mut launch = model_reviewer::parse_args(["model-reviewer"]).unwrap();
        launch.keymap = Some(path.clone());
        if when == "launch" {
            launch.positional = Some(alias.clone());
        }
        let mut app = headless(launch);
        let s = app.session_mut();
        match when {
            "review" => s.apply(Command::LoadReview(alias.clone())),
            "model" => s.apply(Command::OpenModel(alias.clone())),
            _ => {}
        }
        s.apply(Command::ClearKey(Action::ToolMove));
        assert_eq!(fs::read(&path).unwrap(), bytes, "{when}");
        assert!(!s.store.saves(), "{when}");
        assert!(s.store.note(false).is_some());
        assert!(!dir.join("keymap.json.broken").exists());
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn launch_script_is_protected_at_the_settings_and_backup_paths() {
    let dir = scratch("script-path");
    let settings = dir.join("keymap.json");
    let backup = dir.join("keymap.json.broken");
    let steps = br#"{"steps":[]}"#;
    for script in [&settings, &backup] {
        fs::write(&settings, b"{}").unwrap();
        fs::write(script, steps).unwrap();
        let mut launch = model_reviewer::parse_args(["model-reviewer"]).unwrap();
        launch.keymap = Some(settings.clone());
        launch.script = Some(script.clone());
        launch.out = Some(dir.join("out"));
        let mut app = headless(launch);
        let session = app.session_mut();
        session.apply(Command::ClearKey(Action::ToolMove));
        assert_eq!(fs::read(script).unwrap(), steps);
        assert!(!session.store.saves());
        assert!(session.store.note(true).is_some());
        if script == &settings {
            assert!(!backup.exists());
        } else {
            assert_eq!(fs::read(&settings).unwrap(), b"{}");
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn change_is_saved_and_restored() {
    let dir = scratch("restore");
    let path = dir.join("keymap.json");
    let mut launch = model_reviewer::parse_args(["model-reviewer"]).unwrap();
    launch.keymap = Some(path.clone());
    let mut app = headless(launch.clone());
    bind(app.session_mut(), Action::ToolMove, Key::G);
    assert!(!path.exists(), "同じ割り当てでファイルを作らない");
    bind(app.session_mut(), Action::ToolMove, Key::T);
    app.session_mut()
        .apply(Command::ClearKey(Action::ToolRotate));
    let json: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        json["bindings"],
        serde_json::json!({"tool.move":"T","tool.rotate":null})
    );
    let restored = headless(launch.clone());
    let s = restored.session();
    assert_eq!(s.store.source(), StoreSource::File);
    assert_eq!(s.key_labels.get(Action::ToolMove), Some("T"));
    assert_eq!(s.key_labels.get(Action::ToolRotate), None);
    assert_eq!(s.keymap.chord(Action::ToolMove), Chord::parse("T").ok());
    fs::write(&path, b"{").unwrap();
    let broken = headless(launch);
    assert!(broken.session().notice.contains("初期値"));
    assert_eq!(fs::read(path).unwrap(), b"{");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn no_keys_fire_while_window_is_open() {
    let mut s = selected();
    s.apply(Command::SetKeysOpen(true));
    for (key, mods) in [
        (Key::G, NONE),
        (Key::Z, CMD),
        (Key::Delete, NONE),
        (Key::F1, NONE),
    ] {
        assert!(dispatch(&s, key, mods).is_empty());
    }
    assert!(s.keys.open && s.selection.is_some());
    s.apply(Command::BeginKeyCapture(Action::FileNew));
    s.apply(Command::NewDocument);
    assert!(s.keys.capture.is_none());
}

#[test]
fn help_opens_settings_and_escape_first_leaves_the_filter() {
    fn click(h: &mut egui_kittest::Harness<Session>, name: &str) {
        // 新しい小窓や絞り込み後の行は、寸法が決まった次のフレームで中央に移る。
        for _ in 0..4 {
            h.step();
        }
        let button = support::widget_rect(h, name).unwrap().center();
        support::click(h, button);
    }
    let (mut h, _) = support::harness(selected(), egui::vec2(960.0, 600.0));
    h.key_press(Key::F1);
    h.run();
    click(&mut h, ids::HELP_KEYS);
    h.run();
    assert!(h.state().keys.open);
    assert!(support::widget_rect(&h, ids::HELP_CLOSE).is_none());
    click(&mut h, ids::KEYS_FILTER);
    h.event(Event::Text("移動".into()));
    h.run();
    assert!(h.ctx.text_edit_focused());
    h.key_press(Key::Escape);
    h.run();
    assert!(h.state().keys.open && !h.ctx.text_edit_focused());
    let row = ids::keys_row_id(ids::KEYS_ROW, Action::ToolMove);
    click(&mut h, &row);
    assert_eq!(h.state().keys.capture, Some(Action::ToolMove));
    h.key_press(Key::Tab);
    h.run();
    assert_eq!(
        h.state().keys.refusal,
        Some((Action::ToolMove, Refusal::Navigation))
    );
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(
        h.state().keys.refusal,
        Some((Action::ToolMove, Refusal::Fixed))
    );
    click(&mut h, ids::KEYS_FILTER);
    h.event(Event::Paste("貼り付け".into()));
    h.run();
    assert_eq!(
        h.state().keys.refusal,
        Some((Action::ToolMove, Refusal::TextEditing))
    );
    assert!(support::widget_rect(&h, &row).is_some());
    h.key_press(Key::T);
    h.run();
    assert_eq!(h.state().key_labels.get(Action::ToolMove), Some("T"));
    click(&mut h, ids::KEYS_RESET_ALL);
    assert!(h.state().keys.reset_all_armed);
    click(&mut h, ids::KEYS_FILTER);
    assert!(!h.state().keys.reset_all_armed);
    assert_eq!(h.state().key_labels.get(Action::ToolMove), Some("T"));
}

#[test]
fn dialog_takes_priority_over_window() {
    let mut s = selected();
    s.apply(Command::DuplicateSelected);
    begin(&mut s, Action::ToolMove);
    s.apply(Command::NewDocument);
    assert!(s.dialog.is_some() && s.keys.open && s.keys.capture.is_none());
    assert!(dispatch(&s, Key::T, NONE).is_empty());
    assert_eq!(dispatch(&s, Key::Escape, NONE), [Command::CancelPending]);
    s.apply(Command::CancelPending);
    assert!(s.has_unsaved() && s.keys.open);
    s.apply(Command::BeginKeyCapture(Action::FileNew));
    assert!(s.on_close_requested());
    assert!(s.keys.capture.is_none());
}

#[test]
fn script_without_keymap_uses_defaults() {
    let dir = scratch("script");
    let script = dir.join("script.json");
    fs::write(&script, br#"{"steps":[]}"#).unwrap();
    let mut launch = model_reviewer::parse_args(["model-reviewer"]).unwrap();
    launch.script = Some(script);
    launch.out = Some(dir.clone());
    launch.keymap = resolve_path(None, launch.script.is_some(), Os::current(), &|_| {
        panic!("環境を読んだ")
    });
    let mut app = headless(launch.clone());
    assert_eq!(app.session().store.path(), None);
    assert_eq!(
        app.session().store.note(true),
        Some("自動操作のため、変更は保存されません")
    );
    bind(app.session_mut(), Action::ToolMove, Key::T);
    assert_eq!(app.session().key_labels.get(Action::ToolMove), Some("T"));
    assert_eq!(
        headless(launch).session().key_labels.get(Action::ToolMove),
        Some("G")
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn failed_save_keeps_binding_and_notifies() {
    let dir = scratch("failure");
    let path = dir.join("keymap.json");
    let mut launch = model_reviewer::parse_args(["model-reviewer"]).unwrap();
    launch.keymap = Some(path.clone());
    let mut app = headless(launch);
    fs::create_dir(&path).unwrap();
    bind(app.session_mut(), Action::ToolMove, Key::T);
    let s = app.session_mut();
    assert!(s.notice.contains("設定ファイルを置き換えられませんでした"));
    assert_eq!(s.key_labels.get(Action::ToolMove), Some("T"));
    s.apply(Command::SetKeysOpen(false));
    s.apply(Command::NewDocument);
    send(s, vec![press(Key::T, NONE)]);
    assert_eq!(s.tool, Tool::Move);
    assert!(s.keymap.is_changed(Action::ToolMove) && !s.has_unsaved());
    assert!(path.is_dir());
    fs::remove_dir(&path).unwrap();
    bind(s, Action::ToolMove, Key::F7);
    assert!(s.notice.is_empty());
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["bindings"]["tool.move"], "F7");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn two_sessions_keep_the_original_backup_and_each_others_temp() {
    let dir = scratch("two-sessions");
    let path = dir.join("keymap.json");
    fs::write(&path, b"{}").unwrap();
    let mut launch = model_reviewer::parse_args(["model-reviewer"]).unwrap();
    launch.keymap = Some(path.clone());
    let mut first = headless(launch.clone());
    let mut second = headless(launch);
    // 別の保存が使っている一時ファイルを残して再現する。
    let in_use = dir.join(".keymap.json.tmp");
    fs::write(&in_use, b"writing").unwrap();
    bind(first.session_mut(), Action::ToolMove, Key::T);
    bind(second.session_mut(), Action::ToolMove, Key::F7);
    assert_eq!(fs::read(&in_use).unwrap(), b"writing");
    assert_eq!(fs::read(dir.join("keymap.json.broken")).unwrap(), b"{}");
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["bindings"]["tool.move"], "F7");
    fs::remove_dir_all(dir).unwrap();
}

/// 既定のままなら、第3段階のキーがすべて同じ `Command` になる（足した4つと変わった所を含む）。
#[test]
fn default_keymap_dispatches_like_before() {
    let open = support::session;
    let empty = Session::default;
    let annotating = || {
        let mut session = support::session();
        session.annotate_mode = true;
        session
    };
    type Setup = fn() -> Session;
    let cases: Vec<(Key, Modifiers, Setup, Vec<Command>)> = vec![
        // 道具（いまの道具と同じなら何もしない）。
        (
            Key::V,
            NONE,
            || with_tool(Tool::Move),
            vec![Command::SetTool(Tool::Select)],
        ),
        (Key::V, NONE, open, vec![]),
        (Key::G, NONE, open, vec![Command::SetTool(Tool::Move)]),
        (Key::G, NONE, || with_tool(Tool::Move), vec![]),
        (Key::R, NONE, open, vec![Command::SetTool(Tool::Rotate)]),
        (Key::S, NONE, open, vec![Command::SetTool(Tool::Scale)]),
        (Key::M, NONE, open, vec![Command::SetTool(Tool::Measure)]),
        (Key::G, SHIFT, open, vec![]),
        (Key::G, CMD, open, vec![]),
        (Key::N, NONE, open, vec![Command::SetAnnotateMode(true)]),
        (
            Key::N,
            NONE,
            annotating,
            vec![Command::SetAnnotateMode(false)],
        ),
        // 見る。
        (Key::A, NONE, open, vec![Command::FitAll]),
        (Key::A, NONE, empty, vec![Command::FitAll]),
        (Key::F, NONE, open, vec![Command::FitSelection]),
        (Key::Home, NONE, open, vec![Command::Home]),
        (
            Key::Num1,
            NONE,
            open,
            vec![Command::SnapView(ViewDir::Front)],
        ),
        (
            Key::Num3,
            NONE,
            open,
            vec![Command::SnapView(ViewDir::Right)],
        ),
        (Key::Num7, NONE, open, vec![Command::SnapView(ViewDir::Top)]),
        (Key::Num5, NONE, open, vec![Command::ToggleProjection]),
        (Key::Num1, SHIFT, open, vec![]),
        // 表示と部品。
        (Key::H, NONE, open, vec![Command::HideSelected]),
        (Key::H, SHIFT, open, vec![Command::ShowAll]),
        (
            Key::Z,
            NONE,
            open,
            vec![Command::SetDisplay(DisplayMode::FacesAndLines)],
        ),
        (
            Key::Z,
            NONE,
            || with_display(DisplayMode::FacesAndLines),
            vec![Command::SetDisplay(DisplayMode::Faces)],
        ),
        (
            Key::Z,
            SHIFT,
            open,
            vec![Command::SetDisplay(DisplayMode::XRay)],
        ),
        (
            Key::Z,
            SHIFT,
            || with_display(DisplayMode::XRay),
            vec![Command::SetDisplay(DisplayMode::Faces)],
        ),
        (Key::X, NONE, open, vec![Command::ToggleSnap]),
        (Key::C, NONE, open, vec![Command::ToggleSection]),
        (Key::C, NONE, empty, vec![]),
        (Key::X, SHIFT, open, vec![]),
        // 編集（Cmd は macOS、ほかは Ctrl）。
        (Key::Z, CMD, open, vec![Command::Undo]),
        (Key::Z, shift_cmd(), open, vec![Command::Redo]),
        (Key::D, CMD, selected, vec![Command::DuplicateSelected]),
        (Key::D, CMD, open, vec![]),
        (Key::D, shift_cmd(), selected, vec![]),
        (Key::Delete, NONE, selected, vec![Command::DeleteSelected]),
        (
            Key::Backspace,
            NONE,
            selected,
            vec![Command::DeleteSelected],
        ),
        (Key::Delete, NONE, open, vec![]),
        (Key::Delete, SHIFT, selected, vec![]),
        (Key::Delete, CMD, selected, vec![]),
        // 切り取りとコピーは割り当てない。
        (Key::X, CMD, open, vec![]),
        (Key::C, CMD, open, vec![]),
        // 新規作成と、足した既定の3つ。
        (Key::N, CMD, open, vec![Command::NewDocument]),
        (Key::N, CMD, empty, vec![Command::NewDocument]),
        (Key::N, shift_cmd(), open, vec![]),
        (Key::O, CMD, open, vec![Command::OpenModelDialog]),
        (Key::S, CMD, open, vec![Command::SaveReviewDialog]),
        (Key::S, CMD, empty, vec![]),
        (Key::E, CMD, open, vec![Command::ExportGlbDialog]),
        (Key::E, CMD, empty, vec![]),
        // 表に無いキー。
        (Key::T, NONE, open, vec![]),
        (Key::Space, NONE, open, vec![]),
        (Key::ArrowLeft, NONE, selected, vec![]),
        (Key::Enter, NONE, selected, vec![]),
    ];
    for (key, modifiers, setup, expected) in cases {
        let session = setup();
        assert_eq!(
            dispatch(&session, key, modifiers),
            expected,
            "{key:?} {modifiers:?}"
        );
    }

    // Alt（Option）と macOS の Control は受けない。
    let alt = Modifiers { alt: true, ..NONE };
    assert!(dispatch(&open(), Key::G, alt).is_empty());

    // F1 はヘルプを開く（`Command` ではなく egui の一時の記憶）。開いている間は表のキーが働かない。
    let ctx = egui::Context::default();
    let session = open();
    assert!(dispatch_events(&ctx, &session, vec![press(Key::F1, NONE)]).is_empty());
    let help_open = |ctx: &egui::Context| {
        ctx.data(|data| data.get_temp::<bool>(egui::Id::unique(ids::HELP_OPEN)))
            .unwrap_or(false)
    };
    assert!(help_open(&ctx));
    assert!(dispatch_events(&ctx, &session, vec![press(Key::G, NONE)]).is_empty());
    assert!(dispatch_events(&ctx, &session, vec![press(Key::Escape, NONE)]).is_empty());
    assert!(!help_open(&ctx));

    // 同じフレームの複数のキーは、届いた順に積む。
    let both = dispatch_events(
        &egui::Context::default(),
        &session,
        vec![press(Key::A, NONE), press(Key::G, NONE)],
    );
    assert_eq!(both, vec![Command::FitAll, Command::SetTool(Tool::Move)]);
    let both = dispatch_events(
        &egui::Context::default(),
        &session,
        vec![press(Key::G, NONE), press(Key::A, NONE)],
    );
    assert_eq!(both, vec![Command::SetTool(Tool::Move), Command::FitAll]);
    // 押し続け（repeat）では働かない。egui は前のフレームから押されているキーを repeat にする。
    let ctx = egui::Context::default();
    assert_eq!(
        dispatch_events(&ctx, &session, vec![press(Key::A, NONE)]),
        vec![Command::FitAll]
    );
    let repeat = Event::Key {
        key: Key::A,
        physical_key: None,
        pressed: true,
        repeat: true,
        modifiers: NONE,
    };
    assert!(dispatch_events(&ctx, &session, vec![repeat]).is_empty());
    // 削除は1フレームに1回。
    let deleted = dispatch_events(
        &egui::Context::default(),
        &selected(),
        vec![press(Key::Delete, NONE), press(Key::Backspace, NONE)],
    );
    assert_eq!(deleted, vec![Command::DeleteSelected]);
}

/// 修飾は完全一致。初期視点は Home だけで、Shift+Home では働かない（変えた所）。
#[test]
fn shift_home_no_longer_fits() {
    let session = support::session();
    assert_eq!(dispatch(&session, Key::Home, NONE), vec![Command::Home]);
    assert!(dispatch(&session, Key::Home, SHIFT).is_empty());
    assert!(dispatch(&session, Key::Home, CMD).is_empty());
}

/// 表を変えると、振り分けも変わる。G を T に移すと、T で移動になり G は何もしない。
#[test]
fn rebinding_changes_the_dispatch() {
    let mut session = support::session();
    let t = Chord::from_event(Key::T, NONE).unwrap();
    session
        .keymap
        .assign(Action::ToolMove, t, false)
        .expect("T は空いている");
    assert_eq!(
        dispatch(&session, Key::T, NONE),
        vec![Command::SetTool(Tool::Move)]
    );
    assert!(dispatch(&session, Key::G, NONE).is_empty());
    // 既定の無かった操作に割り当てると、ボタンと同じ `Command` になる。
    let b = Chord::from_event(Key::B, NONE).unwrap();
    session.keymap.assign(Action::ViewBack, b, false).unwrap();
    assert_eq!(
        dispatch(&session, Key::B, NONE),
        vec![Command::SnapView(ViewDir::Back)]
    );
    let k = Chord::from_event(Key::K, NONE).unwrap();
    session.keymap.assign(Action::AppKeys, k, false).unwrap();
    assert_eq!(
        dispatch(&session, Key::K, NONE),
        vec![Command::SetKeysOpen(true)]
    );
    // 変形の途中は、道具を替えない（数値の欄をドラッグしている最中の G など）。
    let mut session = selected();
    session.tool = Tool::Move;
    session.apply(Command::PreviewLocal(
        model_reviewer::command::Trs::default(),
    ));
    assert!(session.transform.is_some(), "数値の欄の変形が始まっている");
    assert!(dispatch(&session, Key::R, NONE).is_empty());
}

/// ポインタがパネルやボタンの上にあっても、G・Cmd+Z・Delete が働く（要件2.5。変えた所）。
#[test]
fn keys_work_over_panels() {
    let (mut harness, seen) = support::harness(support::session(), egui::vec2(1100.0, 720.0));
    harness.step();
    harness.state_mut().apply(Command::Select(Some(NodeId(0))));
    harness.step();
    let row = format!("{}.0", ids::PARTS_ROW);
    let panel = support::widget_rect(&harness, &row).expect("部品の一覧の行");
    harness.event(Event::PointerMoved(panel.center()));
    harness.step();
    assert!(harness.ctx.is_pointer_over_egui());
    assert!(harness.ctx.memory(|memory| memory.focused()).is_none());
    seen.lock().unwrap().clear();
    harness.event(press(Key::G, NONE));
    harness.step();
    assert!(seen.lock().unwrap().contains(&Command::SetTool(Tool::Move)));
    harness.event(press(Key::Z, CMD));
    harness.step();
    assert!(seen.lock().unwrap().contains(&Command::Undo));
    harness.event(press(Key::Delete, NONE));
    harness.step();
    assert!(seen.lock().unwrap().contains(&Command::DeleteSelected));

    // 上部のボタンの上でも働く。
    let button = support::widget_rect(&harness, ids::TOOLBAR_HOME).expect("［初期視点］");
    harness.event(Event::PointerMoved(button.center()));
    harness.step();
    seen.lock().unwrap().clear();
    harness.event(press(Key::A, NONE));
    harness.step();
    assert!(seen.lock().unwrap().contains(&Command::FitAll));
}

/// 入力欄でない部品が入力の位置を持っている間は、Delete・矢印・Enter を捨て、G は働かせる。
#[test]
fn focused_widget_keeps_delete_and_arrows() {
    let (mut harness, seen) = support::harness(selected(), egui::vec2(1100.0, 720.0));
    support::hover_view(&mut harness);
    // Tab で入力の位置を、最初の部品（上部のボタン）へ移す。
    harness.event(press(Key::Tab, NONE));
    harness.step();
    harness.step();
    assert!(
        harness.ctx.memory(|memory| memory.focused()).is_some(),
        "Tab で入力の位置が部品へ移る"
    );
    assert!(!harness.ctx.text_edit_focused(), "入力欄ではない");
    assert!(harness.state().selection.is_some());
    seen.lock().unwrap().clear();
    for key in [
        Key::Delete,
        Key::Backspace,
        Key::ArrowLeft,
        Key::ArrowDown,
        Key::Enter,
    ] {
        harness.event(press(key, NONE));
        harness.step();
    }
    assert!(
        !seen.lock().unwrap().contains(&Command::DeleteSelected),
        "{:?}",
        seen.lock().unwrap()
    );
    assert!(harness.state().selection.is_some(), "部品は消えていない");
    // 英字は働く。
    assert!(
        harness.ctx.memory(|memory| memory.focused()).is_some(),
        "入力の位置は部品にある"
    );
    harness.event(press(Key::G, NONE));
    harness.step();
    assert!(seen.lock().unwrap().contains(&Command::SetTool(Tool::Move)));
}
