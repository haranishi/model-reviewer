//! ショートカットの設定の画面。`Session` を読まず、入力用の構造体だけを受け取って `Command` を積む。
//!
//! ヘルプと同じ出し方（動かせない `egui::Modal`）。開くかどうかは呼ぶ側が決める（開いている間だけ
//! 呼ぶ）。キーの取り込み（次に押されたキーを読む）は `app::shortcut_commands` が行い、ここは
//! 「キーを押してください」を出すだけにする（部品の入力の位置は前面でないウィンドウで働かない）。
//! 検索の欄の文字は egui の一時の記憶に置く。名前は `ids::KEYS_*`。見えている行だけ登録する。

use crate::command::Command;
use crate::keymap::{Action, Group, Keymap, Platform, Refusal};
use crate::session::KeyConflict;
use crate::theme;

use super::ids;
use super::reg;

/// 設定の画面の入力。
#[derive(Clone, Debug, PartialEq)]
pub struct KeysInput {
    pub rows: Vec<KeyRow>,
    pub capture: Option<Action>,
    pub conflict: Option<KeyConflict>,
    pub refusal: Option<(Action, Refusal)>,
    pub reset_all_armed: bool,
    /// 保存されない事情があるときだけの文。
    pub store_note: Option<String>,
    pub platform: Platform,
}

/// 1つの操作の行。
#[derive(Clone, Debug, PartialEq)]
pub struct KeyRow {
    pub action: Action,
    /// 今のキーの文字（未割り当ては `None`）。
    pub key: Option<String>,
    /// 既定と違うか。
    pub changed: bool,
    pub default_key: Option<String>,
    /// 修飾なしの X・Y・Z を持つ行の注意（要件13.2）。
    pub note: Option<&'static str>,
}

/// 小窓の見出し。
pub const TITLE: &str = "ショートカットの設定";
/// 取り込み中の行のキーの部品の文字。
pub const CAPTURE_TEXT: &str = "キーを押してください（Esc でやめる）";
/// キーの無い行のキーの部品の文字。
pub const UNASSIGNED: &str = "未割り当て";
/// 保存されるときに、検索の欄の横に出す文。
pub const SAVED_NOTE: &str = "この設定は保存されます";
/// 既定と違う行の印の文字。
pub const CHANGED_MARK: &str = "変更";
/// [すべて初期値に戻す] の1回目と、押した後（もう1回で実行）の文字。
pub const RESET_ALL: &str = "すべて初期値に戻す";
pub const RESET_ALL_ARMED: &str = "もう一度押すと、すべて初期値に戻します";

/// 操作の名前の欄の幅。
const NAME_WIDTH: f32 = 170.0;
/// キーの部品の最小の幅。
const KEY_WIDTH: f32 = 130.0;

/// 修飾なしの X・Y・Z を持つ行の注意（要件13.2）。割り当ては断らない。
pub const AXIS_NOTE: &str =
    "移動・回転・拡大の道具で部品を選んでいる間は、向きの固定が先に働きます";

/// 表から行を作る（並びは `Action::ALL`）。接合が `KeysInput.rows` を組み立てるときに使う。
pub fn build_rows(keymap: &Keymap, platform: Platform) -> Vec<KeyRow> {
    Action::ALL
        .iter()
        .map(|&action| {
            let chord = keymap.chord(action);
            let axis = chord.is_some_and(|c| {
                c.is_plain() && matches!(c.key, egui::Key::X | egui::Key::Y | egui::Key::Z)
            });
            KeyRow {
                action,
                key: chord.map(|c| c.label(platform)),
                changed: keymap.is_changed(action),
                default_key: action.default_chord().map(|c| c.label(platform)),
                note: axis.then_some(AXIS_NOTE),
            }
        })
        .collect()
}

/// 変えられないキーと、その役目（要件3.9・13.1）。
pub fn fixed_keys() -> [(&'static str, &'static str); 6] {
    [
        (
            "Esc",
            "取り消し・解除・小窓を閉じる。場面ごとに働く順が決まっているため変えられません",
        ),
        ("Enter", "入力欄と数値の確定。変えられません"),
        (
            "Delete・Backspace",
            "選んだ部品を削除。誤って別のキーへ移らないよう固定しています",
        ),
        (
            "X・Y・Z",
            "移動・回転・拡大の道具で部品を選んでいる間は、向きの固定（同じキーの操作より先に働きます）",
        ),
        (
            "矢印",
            "移動の道具で部品を選んでいる間は、格子1つぶん動かす（Shift で10分の1）",
        ),
        (
            "0〜9・小数点・負号",
            "向きを固定して数値を打っている間は、数値の入力に使います（Backspace で1字消し、Enter で確定、Esc でやめる）",
        ),
    ]
}

/// 割り当てに使えないキーと、その理由（要件4）。修飾の名前は `platform` に合わせる。
pub fn refused_keys(platform: Platform) -> Vec<(String, &'static str)> {
    let c = platform.command_name();
    let modifiers = match platform {
        Platform::Mac => "Option・Control",
        Platform::Other => "Alt",
    };
    vec![
        ("Tab・Space・矢印".to_owned(), Refusal::Navigation.message()),
        (
            format!("{c}+C・{c}+X・{c}+V・{c}+A"),
            Refusal::TextEditing.message(),
        ),
        (
            format!("{c}+Q・{c}+W・{c}+H・{c}+M"),
            Refusal::System.message(),
        ),
        ("Shift+数字".to_owned(), Refusal::ShiftDigit.message()),
        (modifiers.to_owned(), Refusal::Modifier.message()),
        (
            "記号・テンキー".to_owned(),
            Refusal::KeyNotAllowed.message(),
        ),
    ]
}

/// 重なりの確認の文。
pub fn conflict_text(conflict: &KeyConflict, platform: Platform) -> String {
    let holder = conflict.holder.label();
    format!(
        "「{}」は「{holder}」で使っています。置き換えると「{holder}」は未割り当てになります",
        conflict.chord.label(platform)
    )
}

/// [初期値に戻す] の文字（「初期値に戻す（R）」。初期値がキーなしなら「初期値に戻す（なし）」）。
pub fn reset_text(row: &KeyRow) -> String {
    format!(
        "初期値に戻す（{}）",
        row.default_key.as_deref().unwrap_or("なし")
    )
}

pub fn show(ui: &mut egui::Ui, input: &KeysInput, out: &mut Vec<Command>) {
    let ctx = ui.ctx().clone();
    let screen = ctx.content_rect();
    let inner_w = (screen.width() - 48.0).clamp(280.0, 760.0);
    // 見出し・検索の欄・下のボタンの分を残す。
    let scroll_h = (screen.height() - 170.0).max(80.0);
    let mut pushed = Vec::new();
    let mut modal = egui::Modal::new(egui::Id::unique("keys.modal"));
    modal.area = modal
        .area
        .fade_in(false)
        .movable(false)
        .constrain(true)
        .default_width(inner_w);
    modal.show(&ctx, |ui| {
        let width = inner_w.min(ui.available_width().max(1.0));
        ui.set_min_width(width);
        ui.set_max_width(width);
        ui.label(egui::RichText::new(TITLE).strong());
        let filter = filter_field(ui, input);
        ui.add_space(4.0);
        super::show_scroll_handle(ui);
        egui::ScrollArea::vertical()
            .id_salt("keys.scroll")
            .max_height(scroll_h)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                rows(ui, input, &filter, &mut pushed);
                ui.add_space(8.0);
                fixed_section(ui, input.platform);
            });
        ui.add_space(6.0);
        let (left, right) = egui::Sides::new().show(
            ui,
            |ui| {
                let (text, command) = if input.reset_all_armed {
                    (RESET_ALL_ARMED, Command::ResetAllKeys)
                } else {
                    (RESET_ALL, Command::ArmResetAllKeys(true))
                };
                let mut button = egui::Button::new(text);
                if input.reset_all_armed {
                    button = button.stroke(egui::Stroke::new(1.5, theme::WARNING));
                }
                let response = ui.add(button);
                reg_visible(ui, ids::KEYS_RESET_ALL, &response);
                response.clicked().then_some(command)
            },
            |ui| {
                let close = ui.button("閉じる");
                reg_visible(ui, ids::KEYS_CLOSE, &close);
                close.clicked().then_some(Command::SetKeysOpen(false))
            },
        );
        pushed.extend(left.into_iter().chain(right));
    });
    // [すべて初期値に戻す] の1回目の後に、ほかの所を押したら取り消す。
    if input.reset_all_armed
        && pushed
            .iter()
            .any(|command| !matches!(command, Command::ResetAllKeys))
    {
        out.push(Command::ArmResetAllKeys(false));
    }
    out.extend(pushed);
}

/// 検索の欄と、保存の事情の文。打った文字を返す。
fn filter_field(ui: &mut egui::Ui, input: &KeysInput) -> String {
    let id = filter_id();
    let mut text = ui.data(|data| data.get_temp::<String>(id).unwrap_or_default());
    ui.horizontal(|ui| {
        ui.label("探す");
        let response = ui.add(
            egui::TextEdit::singleline(&mut text)
                .id(id)
                .hint_text("名前かキー")
                .desired_width(220.0),
        );
        reg_visible(ui, ids::KEYS_FILTER, &response);
        ui.add_space(12.0);
        match &input.store_note {
            Some(note) => {
                ui.add(egui::Label::new(egui::RichText::new(note).color(theme::WARNING)).wrap());
            }
            None => {
                ui.label(egui::RichText::new(SAVED_NOTE).color(theme::TEXT_WEAK));
            }
        }
    });
    ui.data_mut(|data| data.insert_temp(id, text.clone()));
    text
}

fn filter_id() -> egui::Id {
    egui::Id::unique(ids::KEYS_FILTER)
}

/// まとまりごとの行。検索に合う行が無いまとまりは見出しも出さない。
fn rows(ui: &mut egui::Ui, input: &KeysInput, filter: &str, out: &mut Vec<Command>) {
    let mut shown = 0;
    for &group in Group::ALL {
        let mut rows = input
            .rows
            .iter()
            .filter(|row| row.action.group() == group && matches(row, filter))
            .peekable();
        if rows.peek().is_none() {
            continue;
        }
        heading(ui, group.label());
        for row in rows {
            row_ui(ui, input, row, out);
            shown += 1;
        }
    }
    if shown == 0 {
        ui.label(egui::RichText::new("合う操作はありません").color(theme::TEXT_WEAK));
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).color(theme::TEXT_WEAK));
    ui.separator();
}

fn row_ui(ui: &mut egui::Ui, input: &KeysInput, row: &KeyRow, out: &mut Vec<Command>) {
    let action = row.action;
    let capturing = input.capture == Some(action);
    ui.horizontal(|ui| {
        fixed_width(ui, |ui| {
            ui.label(action.label());
        });
        let text = if capturing {
            CAPTURE_TEXT
        } else {
            row.key.as_deref().unwrap_or(UNASSIGNED)
        };
        let mut button = egui::Button::new(text).min_size(egui::vec2(KEY_WIDTH, 0.0));
        if capturing {
            button = button
                .fill(theme::MODE_ACTIVE)
                .stroke(egui::Stroke::new(1.5, theme::SELECTION));
        }
        let key = ui.add(button).on_hover_text(format!(
            "{}: 押してから、割り当てるキーを押します",
            action.label()
        ));
        reg_visible(ui, &ids::keys_row_id(ids::KEYS_ROW, action), &key);
        if key.clicked() && !capturing {
            // 検索の欄に入力の位置が残っていると、振り分けが「入力中」で止まりキーが届かない。
            ui.memory_mut(|memory| memory.surrender_focus(filter_id()));
            out.push(Command::BeginKeyCapture(action));
        }
        if row.changed {
            changed_mark(ui);
        }
        if row.key.is_some() {
            let clear = ui.button("外す");
            reg_visible(ui, &ids::keys_row_id(ids::KEYS_CLEAR, action), &clear);
            if clear.clicked() {
                out.push(Command::ClearKey(action));
            }
        }
        if row.changed {
            let reset = ui.button(reset_text(row));
            reg_visible(ui, &ids::keys_row_id(ids::KEYS_RESET, action), &reset);
            if reset.clicked() {
                out.push(Command::ResetKey(action));
            }
        }
    });
    if let Some(conflict) = input.conflict.filter(|c| c.action == action) {
        indented(ui, |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(conflict_text(&conflict, input.platform))
                        .color(theme::WARNING),
                )
                .wrap(),
            );
            ui.horizontal(|ui| {
                let replace = ui.button("置き換える");
                reg_visible(ui, ids::KEYS_CONFLICT_REPLACE, &replace);
                if replace.clicked() {
                    out.push(Command::ResolveKeyConflict { replace: true });
                }
                let cancel = ui.button("やめる");
                reg_visible(ui, ids::KEYS_CONFLICT_CANCEL, &cancel);
                if cancel.clicked() {
                    out.push(Command::ResolveKeyConflict { replace: false });
                }
            });
        });
    }
    if let Some((_, refusal)) = input.refusal.filter(|(a, _)| *a == action) {
        indented(ui, |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(refusal.message()).color(theme::WARNING))
                    .wrap(),
            );
        });
    }
    if let Some(note) = row.note {
        indented(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new(note).color(theme::TEXT_WEAK)).wrap());
        });
    }
}

/// 行の下に字下げして出す（理由・確認・注意）。
fn indented(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.add_space(NAME_WIDTH * 0.25);
        ui.vertical(add);
    });
}

/// 名前の欄（決まった幅。長い名前は折り返さずに収まる幅にしてある）。
fn fixed_width(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(NAME_WIDTH, ui.spacing().interact_size.y),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(NAME_WIDTH);
            add(ui);
        },
    );
}

/// 既定と違う行の印（琥珀の小さい丸と「変更」）。丸は図形で描く（記号の字形に頼らない）。
fn changed_mark(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), 3.5, theme::SELECTION);
    ui.label(egui::RichText::new(CHANGED_MARK).color(theme::SELECTION));
}

/// 変えられないキーと、使えないキーの一覧。
fn fixed_section(ui: &mut egui::Ui, platform: Platform) {
    heading(ui, "変えられないキー");
    for (key, what) in fixed_keys() {
        key_line(ui, key, what);
    }
    heading(ui, "割り当てに使えないキー");
    for (key, why) in refused_keys(platform) {
        key_line(ui, &key, why);
    }
}

fn key_line(ui: &mut egui::Ui, key: &str, text: &str) {
    ui.horizontal_top(|ui| {
        fixed_width(ui, |ui| {
            ui.label(key);
        });
        ui.add(egui::Label::new(egui::RichText::new(text).color(theme::TEXT_WEAK)).wrap());
    });
}

/// 見えている（スクロールの外へ切れていない）部品だけ登録する。
fn reg_visible(ui: &egui::Ui, name: &str, response: &egui::Response) {
    if ui.clip_rect().contains_rect(response.rect) {
        reg(name, response);
    }
}

/// 名前かキーに `filter` を含むか。英字は大文字と小文字を区別しない。空の `filter` はすべて残す。
pub fn matches(row: &KeyRow, filter: &str) -> bool {
    let filter = filter.trim();
    if filter.is_empty() {
        return true;
    }
    let wanted = filter.to_lowercase();
    row.action.label().to_lowercase().contains(&wanted)
        || row
            .key
            .as_deref()
            .is_some_and(|key| key.to_lowercase().contains(&wanted))
}
