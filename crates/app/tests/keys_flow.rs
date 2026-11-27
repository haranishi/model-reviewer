//! キーの振り分け（`shortcut_commands`）を、操作とキーの表を通して確かめる。GPUは使わない。
//! 利用者の設定ファイルには触れない（`Session::default` は場所の無い持ち手を使う）。

mod support;

use egui::{Event, Key, Modifiers};
use model_reviewer::Session;
use model_reviewer::command::{Command, DisplayMode, Tool, ViewDir};
use model_reviewer::keymap::{Action, Chord};
use model_reviewer::shortcut_commands;
use model_reviewer::ui::ids;
use model_reviewer_core::scene::NodeId;

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
