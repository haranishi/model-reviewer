//! 保存した視点。名前の入力途中は egui の一時メモリに置き、ボタンで確定する。

use crate::command::Command;
use crate::session::Session;
use crate::theme;

use super::ids;
use super::reg;

pub fn show(ui: &mut egui::Ui, session: &Session, out: &mut Vec<Command>) {
    ui.label("視点");
    let locked = session.edits_locked();
    let modal_open = session.dialog.is_some() || crate::app::help_open(ui.ctx());
    let editable = !locked && !modal_open;
    let escape = !modal_open && ui.input(|input| input.key_pressed(egui::Key::Escape));
    let buf_id = ui.scope_id().with("views.name.buf");
    let mut name = ui
        .data(|data| data.get_temp::<String>(buf_id))
        .unwrap_or_default();
    ui.horizontal(|ui| {
        let edit = ui.add_enabled(
            editable,
            egui::TextEdit::singleline(&mut name)
                .hint_text("名前")
                .id_salt(ids::VIEWS_NAME)
                .desired_width(140.0),
        );
        reg(ids::VIEWS_NAME, &edit);
        if escape && (edit.lost_focus() || edit.has_focus()) {
            edit.surrender_focus();
            crate::app::mark_text_escape(ui.ctx());
        }
        if edit.changed() {
            ui.data_mut(|data| data.insert_temp(buf_id, name.clone()));
        }
        let trimmed = name.trim();
        let save = ui.add_enabled(
            editable && !trimmed.is_empty(),
            egui::Button::new("視点を保存"),
        );
        reg(ids::VIEWS_SAVE, &save);
        if save.clicked() {
            out.push(Command::SaveView {
                name: trimmed.to_owned(),
            });
            ui.data_mut(|data| data.remove_temp::<String>(buf_id));
        }
    });

    let Some(review) = session.review.as_ref() else {
        return;
    };
    for view in &review.views {
        ui.horizontal(|ui| {
            let apply = ui.button(&view.name);
            let apply_name = format!("{}.{}", ids::VIEWS_APPLY, view.id);
            reg(&apply_name, &apply);
            if apply.clicked() {
                out.push(Command::ApplyView(view.id));
            }
            let delete = ui.add_enabled(
                editable,
                egui::Button::new(egui::RichText::new("削除").color(theme::ERROR)),
            );
            let delete_name = format!("{}.{}", ids::VIEWS_DELETE, view.id);
            reg(&delete_name, &delete);
            if delete.clicked() {
                out.push(Command::DeleteView(view.id));
            }
        });
    }
}
