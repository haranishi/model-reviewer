//! 上部の操作。使えないボタンは押せない見た目にする。押せるボタンに触れると、
//! 割り当てたキーを「開く（Cmd+O）」の形で出す（`Session.key_labels` から作る）。

use crate::command::Command;
use crate::keymap::{Action, KeyLabels};
use crate::session::Session;
use crate::theme;

use super::help;
use super::ids;
use super::reg;

pub fn show(ui: &mut egui::Ui, session: &Session, out: &mut Vec<Command>) {
    let has_model = session.model.is_some();
    let locked = session.edits_locked();
    let exporting = session.is_exporting();
    let keys = &session.key_labels;
    ui.horizontal(|ui| {
        press_with_reason(
            ui,
            ids::TOOLBAR_OPEN,
            "開く",
            button_hint(keys, Action::FileOpen, "開く"),
            !exporting,
            exporting.then_some("書き出し中です"),
            Command::OpenModelDialog,
            out,
        );
        ui.add_space(8.0);
        press_with_reason(
            ui,
            ids::TOOLBAR_SAVE_REVIEW,
            "レビュー保存",
            button_hint(keys, Action::FileSaveReview, "レビュー保存"),
            has_model && !locked,
            session
                .review_save_block_reason()
                .or((!has_model).then_some("モデルを開くとレビューを保存できます")),
            Command::SaveReviewDialog,
            out,
        );
        press_with_reason(
            ui,
            ids::TOOLBAR_LOAD_REVIEW,
            "レビュー読込",
            button_hint(keys, Action::FileLoadReview, "レビュー読込"),
            !exporting,
            exporting.then_some("書き出し中です"),
            Command::LoadReviewDialog,
            out,
        );
        ui.add_space(8.0);
        press_with_reason(
            ui,
            ids::TOOLBAR_EXPORT,
            "書き出し",
            button_hint(keys, Action::FileExport, "書き出し"),
            has_model && !locked,
            if exporting {
                Some("書き出し中です")
            } else if locked {
                Some("読み込みが終わると書き出せます")
            } else if !has_model {
                Some("モデルを開くと書き出せます")
            } else {
                None
            },
            Command::ExportGlbDialog,
            out,
        );
        ui.add_space(8.0);
        history_button(
            ui,
            ids::TOOLBAR_UNDO,
            &keys.with_key(Action::EditUndo, "戻す"),
            session.history.undo_label(),
            !locked,
            Command::Undo,
            out,
        );
        history_button(
            ui,
            ids::TOOLBAR_REDO,
            &keys.with_key(Action::EditRedo, "やり直す"),
            session.history.redo_label(),
            !locked,
            Command::Redo,
            out,
        );
        ui.add_space(8.0);
        press(
            ui,
            ids::TOOLBAR_FIT_ALL,
            "全体表示",
            button_hint(keys, Action::ViewFitAll, "全体表示"),
            has_model,
            Command::FitAll,
            out,
        );
        press(
            ui,
            ids::TOOLBAR_FIT_SELECTION,
            "選択を表示",
            button_hint(keys, Action::ViewFitSelection, "選択を表示"),
            session.selection.is_some(),
            Command::FitSelection,
            out,
        );
        press(
            ui,
            ids::TOOLBAR_HOME,
            "初期視点",
            button_hint(keys, Action::ViewHome, "初期視点"),
            has_model,
            Command::Home,
            out,
        );
        ui.add_space(8.0);
        let annotate = if session.annotate_mode {
            "注釈を追加中"
        } else {
            "注釈を追加"
        };
        let mut button = egui::Button::new(annotate);
        if session.annotate_mode {
            button = button
                .fill(theme::MODE_ACTIVE)
                .stroke(egui::Stroke::new(1.5, theme::SELECTION));
        }
        let mut response = ui.add_enabled(has_model && !locked, button);
        if let Some(hint) = button_hint(keys, Action::ToolAnnotate, "注釈の切り替え") {
            response = response.on_hover_text(hint);
        }
        reg(ids::TOOLBAR_ANNOTATE, &response);
        if response.clicked() {
            out.push(Command::SetAnnotateMode(!session.annotate_mode));
        }
        ui.add_space(8.0);
        let mut help_button = ui.button("?");
        if let Some(hint) = button_hint(keys, Action::AppHelp, "ヘルプ") {
            help_button = help_button.on_hover_text(hint);
        }
        reg(ids::TOOLBAR_HELP, &help_button);
        if help_button.clicked() {
            help::toggle(ui.ctx());
        }
    });
}

/// 戻す・やり直す。触れると対象の操作の名前（無ければ理由）を出す。`described` は説明の文の
/// 頭（「戻す（Cmd+Z）」）。
fn history_button(
    ui: &mut egui::Ui,
    id: &str,
    described: &str,
    target: Option<&str>,
    unlocked: bool,
    command: Command,
    out: &mut Vec<Command>,
) {
    let enabled = unlocked && target.is_some();
    let label = if id == ids::TOOLBAR_UNDO {
        "戻す"
    } else {
        "やり直す"
    };
    let response = ui.add_enabled(enabled, egui::Button::new(label));
    let response = match target {
        Some(name) => response.on_hover_text(format!("{described}: {name}")),
        None if id == ids::TOOLBAR_UNDO => {
            response.on_disabled_hover_text("戻せる操作はありません")
        }
        None => response.on_disabled_hover_text("やり直せる操作はありません"),
    };
    reg(id, &response);
    if response.clicked() {
        out.push(command);
    }
}

/// 押せるボタンに触れたときの説明（「開く（Cmd+O）」）。キーが無ければ説明を出さない
/// （ボタンの文字と同じ文になるため）。
pub fn button_hint(keys: &KeyLabels, action: Action, label: &str) -> Option<String> {
    keys.get(action).map(|_| keys.with_key(action, label))
}

fn press(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    hint: Option<String>,
    enabled: bool,
    command: Command,
    out: &mut Vec<Command>,
) {
    press_with_reason(ui, id, label, hint, enabled, None, command, out);
}

#[allow(clippy::too_many_arguments)]
fn press_with_reason(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    hint: Option<String>,
    enabled: bool,
    disabled_reason: Option<&str>,
    command: Command,
    out: &mut Vec<Command>,
) {
    let mut response = ui.add_enabled(enabled, egui::Button::new(label));
    if let Some(hint) = hint {
        response = response.on_hover_text(hint);
    }
    if let Some(reason) = disabled_reason {
        response = response.on_disabled_hover_text(reason);
    }
    reg(id, &response);
    if response.clicked() {
        out.push(command);
    }
}
