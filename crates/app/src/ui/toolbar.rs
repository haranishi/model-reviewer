//! 上部の操作。使えないボタンは押せない見た目にする。

use crate::command::Command;
use crate::session::Session;
use crate::theme;

use super::help;
use super::ids;
use super::reg;

pub fn show(ui: &mut egui::Ui, session: &Session, out: &mut Vec<Command>) {
    let has_model = session.model.is_some();
    let locked = session.edits_locked();
    let exporting = session.is_exporting();
    ui.horizontal(|ui| {
        press_with_reason(
            ui,
            ids::TOOLBAR_OPEN,
            "開く",
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
            has_model && !exporting,
            if exporting {
                Some("書き出しが終わってからレビューを保存してください")
            } else if !has_model {
                Some("モデルを開くとレビューを保存できます")
            } else {
                None
            },
            Command::SaveReviewDialog,
            out,
        );
        press_with_reason(
            ui,
            ids::TOOLBAR_LOAD_REVIEW,
            "レビュー読込",
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
            "戻す",
            session.history.undo_label(),
            !locked,
            Command::Undo,
            out,
        );
        history_button(
            ui,
            ids::TOOLBAR_REDO,
            "やり直す",
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
            has_model,
            Command::FitAll,
            out,
        );
        press(
            ui,
            ids::TOOLBAR_FIT_SELECTION,
            "選択を表示",
            session.selection.is_some(),
            Command::FitSelection,
            out,
        );
        press(
            ui,
            ids::TOOLBAR_HOME,
            "初期視点",
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
        let response = ui.add_enabled(has_model && !locked, button);
        reg(ids::TOOLBAR_ANNOTATE, &response);
        if response.clicked() {
            out.push(Command::SetAnnotateMode(!session.annotate_mode));
        }
        ui.add_space(8.0);
        let help_button = ui.button("?");
        reg(ids::TOOLBAR_HELP, &help_button);
        if help_button.clicked() {
            help::toggle(ui.ctx());
        }
    });
}

/// 戻す・やり直す。触れると対象の操作の名前（無ければ理由）を出す。
fn history_button(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    target: Option<&str>,
    unlocked: bool,
    command: Command,
    out: &mut Vec<Command>,
) {
    let enabled = unlocked && target.is_some();
    let response = ui.add_enabled(enabled, egui::Button::new(label));
    let response = match target {
        Some(name) => response.on_hover_text(format!("{label}: {name}")),
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

fn press(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    enabled: bool,
    command: Command,
    out: &mut Vec<Command>,
) {
    press_with_reason(ui, id, label, enabled, None, command, out);
}

fn press_with_reason(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    enabled: bool,
    disabled_reason: Option<&str>,
    command: Command,
    out: &mut Vec<Command>,
) {
    let mut response = ui.add_enabled(enabled, egui::Button::new(label));
    if let Some(reason) = disabled_reason {
        response = response.on_disabled_hover_text(reason);
    }
    reg(id, &response);
    if response.clicked() {
        out.push(command);
    }
}
