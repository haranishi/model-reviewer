//! ショートカットの設定の画面。入力用の構造体だけで動き、押すと割り当てを変える `Command` を積む。
//! 画面の文字を比べる所は `Platform` を明示する（試験を動かすOSに頼らない）。

use egui::Key;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use model_reviewer::automation::WidgetFrame;
use model_reviewer::command::Command;
use model_reviewer::keymap::{Action, Chord, Keymap, Platform, Refusal};
use model_reviewer::session::KeyConflict;
use model_reviewer::ui::ids;
use model_reviewer::ui::keys::{
    self, AXIS_NOTE, CAPTURE_TEXT, CHANGED_MARK, KeyRow, KeysInput, RESET_ALL_ARMED, SAVED_NOTE,
    UNASSIGNED, build_rows, matches,
};

struct Probe {
    input: KeysInput,
    out: Vec<Command>,
}

fn chord(key: Key, shift: bool, command: bool) -> Chord {
    Chord {
        key,
        shift,
        command,
    }
}

fn input_for(keymap: &Keymap, platform: Platform) -> KeysInput {
    KeysInput {
        rows: build_rows(keymap, platform),
        capture: None,
        conflict: None,
        refusal: None,
        reset_all_armed: false,
        store_note: None,
        platform,
    }
}

fn default_input() -> KeysInput {
    input_for(&Keymap::default(), Platform::Mac)
}

/// 高さに余裕のある画面（全部の行が見えて登録される）。
fn tall(input: KeysInput) -> Harness<'static, Probe> {
    harness_sized(input, egui::vec2(960.0, 2400.0))
}

fn harness_sized(input: KeysInput, size: egui::Vec2) -> Harness<'static, Probe> {
    Harness::builder()
        .with_size(size)
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_context_setup(|ctx| {
            model_reviewer::fonts::install(ctx);
            model_reviewer::theme::install(ctx);
        })
        .build_ui_state(
            draw,
            Probe {
                input,
                out: Vec::new(),
            },
        )
}

fn draw(ui: &mut egui::Ui, probe: &mut Probe) {
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::NULL, WidgetFrame::default()));
    let mut out = Vec::new();
    keys::show(ui, &probe.input, &mut out);
    probe.out.extend(out);
}

fn names(harness: &Harness<'static, Probe>) -> Vec<(String, egui::Rect)> {
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

fn has(harness: &Harness<'static, Probe>, name: &str) -> bool {
    names(harness).iter().any(|(n, _)| n == name)
}

fn widget(harness: &Harness<'static, Probe>, name: &str) -> egui::Rect {
    names(harness)
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("{name} が登録されていない"))
        .1
}

fn row_name(prefix: &str, action: Action) -> String {
    ids::keys_row_id(prefix, action)
}

/// 部品の場所が動かなくなるまで描く。Modal は出した次のフレームで大きさが決まり、中の縦の
/// スクロールは数十フレームかけて中身の高さまで伸びる（ヘルプと同じ）。
fn settle(harness: &mut Harness<'static, Probe>) {
    let mut last = Vec::new();
    for _ in 0..300 {
        harness.step();
        let now = names(harness);
        if now == last {
            return;
        }
        last = now;
    }
    panic!("部品の場所が落ち着かない");
}

fn click(harness: &mut Harness<'static, Probe>, name: &str) -> Vec<Command> {
    harness.state_mut().out.clear();
    let pos = widget(harness, name).center();
    harness.event(egui::Event::PointerMoved(pos));
    harness.step();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.step();
    }
    harness.step();
    std::mem::take(&mut harness.state_mut().out)
}

fn set_filter(harness: &mut Harness<'static, Probe>, text: &str) {
    harness
        .ctx
        .data_mut(|data| data.insert_temp(egui::Id::unique(ids::KEYS_FILTER), text.to_owned()));
    settle(harness);
}

#[test]
fn row_click_begins_capture() {
    let mut harness = tall(default_input());
    settle(&mut harness);
    // 検索の欄に入力の位置を置いてから行を押すと、入力の位置を手放す（落とし穴2）。
    let filter = widget(&harness, ids::KEYS_FILTER).center();
    harness.event(egui::Event::PointerMoved(filter));
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: filter,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.step();
    }
    let filter_id = egui::Id::unique(ids::KEYS_FILTER);
    assert!(harness.ctx.memory(|memory| memory.has_focus(filter_id)));
    let row = row_name(ids::KEYS_ROW, Action::ToolMove);
    assert_eq!(
        click(&mut harness, &row),
        vec![Command::BeginKeyCapture(Action::ToolMove)]
    );
    assert!(!harness.ctx.memory(|memory| memory.has_focus(filter_id)));

    // 取り込み中の行は「キーを押してください」になり、押し直しても積まない。ほかの行は押せる。
    harness.state_mut().input.capture = Some(Action::ToolMove);
    settle(&mut harness);
    harness.get_by_label(CAPTURE_TEXT);
    assert!(click(&mut harness, &row).is_empty());
    assert_eq!(
        click(&mut harness, &row_name(ids::KEYS_ROW, Action::ToolRotate)),
        vec![Command::BeginKeyCapture(Action::ToolRotate)]
    );
    // 未割り当ての行の部品は「未割り当て」。
    let unassigned = Action::ALL
        .iter()
        .filter(|a| a.default_chord().is_none())
        .count();
    assert_eq!(harness.get_all_by_label(UNASSIGNED).count(), unassigned);
}

#[test]
fn conflict_offers_replace_and_cancel() {
    let mut input = default_input();
    input.conflict = Some(KeyConflict {
        action: Action::ToolRotate,
        chord: chord(Key::G, false, false),
        holder: Action::ToolMove,
    });
    let mut harness = tall(input);
    settle(&mut harness);
    harness.get_by_label_contains("「G」は「移動」で使っています");
    // 確認はその行の下にだけ出る。
    let rotate = widget(&harness, &row_name(ids::KEYS_ROW, Action::ToolRotate));
    let replace = widget(&harness, ids::KEYS_CONFLICT_REPLACE);
    assert!(replace.top() >= rotate.bottom(), "{replace:?} {rotate:?}");
    let next = widget(&harness, &row_name(ids::KEYS_ROW, Action::ToolScale));
    assert!(replace.bottom() <= next.top(), "{replace:?} {next:?}");
    assert_eq!(
        click(&mut harness, ids::KEYS_CONFLICT_REPLACE),
        vec![Command::ResolveKeyConflict { replace: true }]
    );
    assert_eq!(
        click(&mut harness, ids::KEYS_CONFLICT_CANCEL),
        vec![Command::ResolveKeyConflict { replace: false }]
    );
    // Ctrl の書き方でも出る。
    let mut other = input_for(&Keymap::default(), Platform::Other);
    other.conflict = Some(KeyConflict {
        action: Action::EditDuplicate,
        chord: chord(Key::Z, false, true),
        holder: Action::EditUndo,
    });
    let mut harness = tall(other);
    settle(&mut harness);
    harness.get_by_label_contains("「Ctrl+Z」は「戻す」で使っています");
}

#[test]
fn only_changed_rows_show_reset() {
    let mut keymap = Keymap::default();
    keymap
        .assign(Action::ToolMove, chord(Key::T, false, false), false)
        .unwrap();
    keymap.clear(Action::ViewHome);
    let mut harness = tall(input_for(&keymap, Platform::Mac));
    settle(&mut harness);
    for &action in Action::ALL {
        let changed = matches!(action, Action::ToolMove | Action::ViewHome);
        assert_eq!(
            has(&harness, &row_name(ids::KEYS_RESET, action)),
            changed,
            "{action:?}"
        );
        // [外す] はキーのある行だけ。
        assert_eq!(
            has(&harness, &row_name(ids::KEYS_CLEAR, action)),
            keymap.chord(action).is_some(),
            "{action:?}"
        );
        assert!(
            has(&harness, &row_name(ids::KEYS_ROW, action)),
            "{action:?}"
        );
    }
    assert_eq!(harness.get_all_by_label(CHANGED_MARK).count(), 2);
    harness.get_by_label("初期値に戻す（G）");
    harness.get_by_label("初期値に戻す（Home）");
    assert_eq!(
        click(&mut harness, &row_name(ids::KEYS_RESET, Action::ToolMove)),
        vec![Command::ResetKey(Action::ToolMove)]
    );
    assert_eq!(
        click(&mut harness, &row_name(ids::KEYS_CLEAR, Action::ToolMove)),
        vec![Command::ClearKey(Action::ToolMove)]
    );
    // 既定がキーなしの操作を変えた行は「初期値に戻す（なし）」。
    let mut keymap = Keymap::default();
    keymap
        .assign(Action::ToolShape, chord(Key::B, false, false), false)
        .unwrap();
    let mut harness = tall(input_for(&keymap, Platform::Mac));
    settle(&mut harness);
    harness.get_by_label("初期値に戻す（なし）");
}

#[test]
fn filter_matches_name_and_key() {
    let rows = build_rows(&Keymap::default(), Platform::Mac);
    let row = |action: Action| rows.iter().find(|r| r.action == action).unwrap();
    assert!(matches(row(Action::ToolMove), "移動"));
    assert!(matches(row(Action::ToolMove), "g"));
    assert!(matches(row(Action::ToolMove), " G "));
    assert!(matches(row(Action::EditUndo), "cmd+z"));
    assert!(matches(row(Action::EditRedo), "Cmd+Z"));
    assert!(!matches(row(Action::ToolMove), "回転"));
    assert!(!matches(row(Action::ToolShape), "Cmd"));
    assert!(rows.iter().all(|r| matches(r, "")));

    // 画面でも、合う行だけが登録される（名前の「表示」と、キーの「Shift」）。
    let mut harness = tall(input_for(&Keymap::default(), Platform::Mac));
    settle(&mut harness);
    for filter in ["表示", "shift", "F1"] {
        set_filter(&mut harness, filter);
        let shown: Vec<Action> = Action::ALL
            .iter()
            .copied()
            .filter(|&a| has(&harness, &row_name(ids::KEYS_ROW, a)))
            .collect();
        let expected: Vec<Action> = rows
            .iter()
            .filter(|r| matches(r, filter))
            .map(|r| r.action)
            .collect();
        assert!(!expected.is_empty(), "{filter}");
        assert_eq!(shown, expected, "{filter}");
    }
    set_filter(&mut harness, "存在しない操作");
    harness.get_by_label("合う操作はありません");
    // 変えられないキーの一覧は、検索に関わらず出る。
    harness.get_by_label("X・Y・Z");
}

#[test]
fn reset_all_needs_two_clicks() {
    let mut harness = tall(default_input());
    settle(&mut harness);
    assert_eq!(
        click(&mut harness, ids::KEYS_RESET_ALL),
        vec![Command::ArmResetAllKeys(true)]
    );
    harness.state_mut().input.reset_all_armed = true;
    settle(&mut harness);
    harness.get_by_label(RESET_ALL_ARMED);
    assert_eq!(
        click(&mut harness, ids::KEYS_RESET_ALL),
        vec![Command::ResetAllKeys]
    );
    // 1回目の後にほかの所を押すと、取り消してから、その操作を積む。
    let out = click(&mut harness, &row_name(ids::KEYS_ROW, Action::ToolMove));
    assert_eq!(
        out,
        vec![
            Command::ArmResetAllKeys(false),
            Command::BeginKeyCapture(Action::ToolMove)
        ]
    );
    let out = click(&mut harness, ids::KEYS_CLOSE);
    assert_eq!(
        out,
        vec![Command::ArmResetAllKeys(false), Command::SetKeysOpen(false)]
    );
}

#[test]
fn store_note_is_shown() {
    let mut harness = tall(default_input());
    settle(&mut harness);
    harness.get_by_label(SAVED_NOTE);
    for note in [
        "設定の場所が決められないため、変更は保存されません",
        "新しい版の設定のため、変更は保存されません",
        "自動操作のため、変更は保存されません",
    ] {
        harness.state_mut().input.store_note = Some(note.to_owned());
        settle(&mut harness);
        harness.get_by_label(note);
        assert!(harness.query_by_label(SAVED_NOTE).is_none());
    }
}

#[test]
fn refusal_and_axis_note_are_shown_under_the_row() {
    let mut keymap = Keymap::default();
    keymap
        .assign(Action::ToolMove, chord(Key::Y, false, false), false)
        .unwrap();
    let mut input = input_for(&keymap, Platform::Mac);
    input.refusal = Some((Action::ToolRotate, Refusal::ShiftDigit));
    let mut harness = tall(input);
    settle(&mut harness);
    // 行の下の理由と、使えないキーの一覧の同じ理由の2か所。
    assert_eq!(
        harness
            .get_all_by_label(Refusal::ShiftDigit.message())
            .count(),
        2
    );
    let rotate = widget(&harness, &row_name(ids::KEYS_ROW, Action::ToolRotate));
    let scale = widget(&harness, &row_name(ids::KEYS_ROW, Action::ToolScale));
    let reason = harness
        .get_all_by_label(Refusal::ShiftDigit.message())
        .map(|node| node.rect())
        .find(|rect| rect.top() >= rotate.bottom() && rect.bottom() <= scale.top());
    assert!(reason.is_some(), "理由が回転の行の下にない");
    // 既定の X（吸着）・Z（面と線）と、Y にした移動の3行に注意が出る。
    assert_eq!(harness.get_all_by_label(AXIS_NOTE).count(), 3);
    // 変えられないキーと、使えないキーの一覧（要件3.9・13.1）。
    for label in [
        "Esc",
        "Enter",
        "Delete・Backspace",
        "X・Y・Z",
        "矢印",
        "0〜9・小数点・負号",
        "Cmd+C・Cmd+X・Cmd+V・Cmd+A",
        "Cmd+Q・Cmd+W・Cmd+H・Cmd+M",
        "Shift+数字",
    ] {
        harness.get_by_label(label);
    }
    let rows = build_rows(&keymap, Platform::Mac);
    let noted: Vec<Action> = rows
        .iter()
        .filter(|r: &&KeyRow| r.note.is_some())
        .map(|r| r.action)
        .collect();
    assert_eq!(
        noted,
        vec![
            Action::ToolMove,
            Action::MeasureSnap,
            Action::DisplayFacesLines
        ]
    );
    // Shift+X は修飾つきなので注意を出さない。
    let mut keymap = Keymap::default();
    keymap
        .assign(Action::ToolShape, chord(Key::X, true, false), false)
        .unwrap();
    let rows = build_rows(&keymap, Platform::Other);
    assert!(
        rows.iter()
            .find(|r| r.action == Action::ToolShape)
            .unwrap()
            .note
            .is_none()
    );
}

/// 割り当てられる組の画面の文字が、どのOSでも描ける文字だけでできている（落とし穴5）。日本語の
/// フォントの無い機械でも確かめられるよう、ここは付属のフォントだけで、描けない文字で落とす。
/// 日本語のフォントがあれば、設定の画面そのもの（取り込み中・重なり・断った理由）も落とさずに描く。
#[test]
fn key_names_render_without_missing_glyphs() {
    let mut labels = Vec::new();
    for &key in Key::ALL {
        for (shift, command) in [(false, false), (true, false), (false, true), (true, true)] {
            let modifiers = egui::Modifiers {
                shift,
                command,
                ..Default::default()
            };
            if let Ok(chord) = Chord::from_event(key, modifiers) {
                for platform in [Platform::Mac, Platform::Other] {
                    labels.push(chord.label(platform));
                }
            }
        }
    }
    for must in [
        "F1",
        "PageUp",
        "Home",
        "1",
        "Shift+Cmd+F12",
        "Ctrl+PageDown",
    ] {
        assert!(labels.iter().any(|l| l == must), "{must}");
    }
    let mut harness = Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .with_accessibility_check(false)
        .build_ui(move |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for label in &labels {
                    ui.label(label);
                }
            });
        });
    harness.run();

    let japanese = {
        use egui::FontProvider as _;
        let family = egui::FontFamily::Proportional;
        egui_system_font_provider::SystemFontProvider::new()
            .with_locale(Some("ja"))
            .font_for(&egui::FallbackRequest {
                cluster: "部",
                family: &family,
            })
            .is_some()
    };
    if !japanese {
        eprintln!("日本語のフォントが無いため、設定の画面の字形は確かめない");
        return;
    }
    let mut keymap = Keymap::default();
    keymap
        .assign(Action::ViewBack, chord(Key::PageUp, false, false), false)
        .unwrap();
    keymap
        .assign(Action::ViewLeft, chord(Key::F12, true, true), false)
        .unwrap();
    for platform in [Platform::Mac, Platform::Other] {
        let mut input = input_for(&keymap, platform);
        input.capture = Some(Action::ToolMove);
        input.conflict = Some(KeyConflict {
            action: Action::ToolRotate,
            chord: chord(Key::G, false, false),
            holder: Action::ToolMove,
        });
        input.refusal = Some((Action::ToolScale, Refusal::TextEditing));
        input.reset_all_armed = true;
        input.store_note = Some("自動操作のため、変更は保存されません".to_owned());
        let mut harness = Harness::builder()
            .with_size(egui::vec2(960.0, 2400.0))
            .with_accessibility_check(false)
            .with_context_setup(|ctx| {
                model_reviewer::fonts::install(ctx);
                model_reviewer::theme::install(ctx);
            })
            .build_ui_state(
                draw,
                Probe {
                    input,
                    out: Vec::new(),
                },
            );
        settle(&mut harness);
        harness.get_by_label("PageUp");
    }
}

#[test]
fn rows_do_not_overlap_at_960() {
    let mut input = default_input();
    input.conflict = Some(KeyConflict {
        action: Action::FileOpen,
        chord: chord(Key::S, false, true),
        holder: Action::FileSaveReview,
    });
    input.refusal = Some((Action::FileExport, Refusal::Navigation));
    input.capture = Some(Action::FileNew);
    let mut harness = harness_sized(input, egui::vec2(960.0, 600.0));
    settle(&mut harness);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 600.0));
    let entries: Vec<(String, egui::Rect)> = names(&harness)
        .into_iter()
        .filter(|(name, _)| name.starts_with("keys."))
        .collect();
    // 行は少なくともファイルのまとまりの分は見えていて、下のボタンも見える。
    for name in [
        row_name(ids::KEYS_ROW, Action::FileNew),
        ids::KEYS_CONFLICT_REPLACE.to_owned(),
        ids::KEYS_RESET_ALL.to_owned(),
        ids::KEYS_CLOSE.to_owned(),
        ids::KEYS_FILTER.to_owned(),
    ] {
        assert!(entries.iter().any(|(n, _)| *n == name), "{name}");
    }
    for (name, rect) in &entries {
        assert!(screen.contains_rect(*rect), "{name}: {rect:?}");
    }
    for (i, (a, ra)) in entries.iter().enumerate() {
        for (b, rb) in &entries[i + 1..] {
            let overlap = ra.intersect(*rb);
            assert!(
                overlap.width() <= 0.5 || overlap.height() <= 0.5,
                "{a} {ra:?} と {b} {rb:?} が重なる"
            );
        }
    }
    // 取り込み中の長い文でも、行の部品が隣と重ならない（上で確かめた）うえに、名前の欄より右にある。
    let capture = widget(&harness, &row_name(ids::KEYS_ROW, Action::FileNew));
    assert!(capture.width() > 130.0, "{capture:?}");
}
