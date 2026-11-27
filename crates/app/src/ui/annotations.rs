//! 注釈の一覧。本文の入力途中は egui の一時メモリに置き、入力欄を離れたときに確定する。

use model_reviewer_core::scene::NodeId;

use crate::command::Command;
use crate::session::Session;
use crate::theme;

use super::ids;
use super::reg;

pub(crate) fn body_temp_id(generation: u64, id: u64) -> egui::Id {
    egui::Id::unique(("annotation-body", generation, id))
}

pub fn show(ui: &mut egui::Ui, session: &Session, out: &mut Vec<Command>) {
    ui.label("注釈");
    let notes = {
        let Some(review) = session.review.as_ref() else {
            if session.body_pending() {
                out.push(Command::SetBodyPending(false));
            }
            return;
        };
        review.annotations.clone()
    };
    let locked = session.edits_locked();
    let modal_open = session.dialog.is_some() || crate::app::help_open(ui.ctx());
    let editable = !locked && !modal_open;
    let max_height = (ui.available_height() * 0.55).clamp(80.0, 420.0);
    let escape = !modal_open && ui.input(|input| input.key_pressed(egui::Key::Escape));
    let mut any_pending = false;
    egui::ScrollArea::vertical()
        .id_salt("notes.list")
        .max_height(max_height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (index, note) in notes.iter().enumerate() {
                if note_row(ui, session, index, note, escape, editable, out) {
                    any_pending = true;
                }
            }
        });
    if any_pending != session.body_pending() {
        out.push(Command::SetBodyPending(any_pending));
    }
}

fn note_row(
    ui: &mut egui::Ui,
    session: &Session,
    index: usize,
    note: &model_reviewer_core::review::Annotation,
    escape: bool,
    editable: bool,
    out: &mut Vec<Command>,
) -> bool {
    let part = part_name(session, note.node);
    let hidden = session
        .review
        .as_ref()
        .is_some_and(|review| !review.visibility.effective(note.node));
    let selected = session.selected_annotation == Some(note.id);
    ui.horizontal(|ui| {
        let (band, _) = ui.allocate_exact_size(egui::vec2(4.0, 16.0), egui::Sense::hover());
        if selected {
            ui.painter().rect_filled(band, 0.0, theme::SELECTION);
        }
        ui.label(mark(index));
        // 削除は右端に固定し、長い部品名は残りの幅で省略する。
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let delete = ui.add_enabled(
                editable,
                egui::Button::new(egui::RichText::new("削除").color(theme::ERROR)),
            );
            let delete_name = format!("{}.{}", ids::NOTES_DELETE, note.id);
            reg(&delete_name, &delete);
            if delete.clicked() {
                out.push(Command::DeleteAnnotation(note.id));
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let select = ui.add(egui::Button::new(&part).truncate());
                let select_name = format!("{}.{}", ids::NOTES_SELECT, note.id);
                reg(&select_name, &select);
                if select.clicked() {
                    out.push(Command::SelectAnnotation(Some(note.id)));
                }
            });
        });
    });
    if hidden {
        ui.weak("非表示の部品");
    }

    let buf_id = body_temp_id(session.review_generation(), note.id);
    let mut text = ui
        .data(|data| data.get_temp::<String>(buf_id))
        .unwrap_or_else(|| note.text.clone());
    // 本文は折り返して全部見せる。改行も入れられ、入力欄を離れたときに確定する。
    let edit = ui.add_enabled(
        editable,
        egui::TextEdit::multiline(&mut text)
            .id(buf_id)
            .desired_rows(1)
            .desired_width(f32::INFINITY),
    );
    let text_name = format!("{}.{}", ids::NOTES_TEXT, note.id);
    reg(&text_name, &edit);
    if escape && (edit.lost_focus() || edit.has_focus()) {
        ui.data_mut(|data| data.remove_temp::<String>(buf_id));
        edit.surrender_focus();
        crate::app::mark_text_escape(ui.ctx());
        return false;
    }
    if edit.changed() {
        ui.data_mut(|data| data.insert_temp(buf_id, text.clone()));
    }
    if editable && edit.lost_focus() {
        ui.data_mut(|data| data.remove_temp::<String>(buf_id));
        let next = text.trim().to_owned();
        if next != note.text {
            out.push(Command::EditAnnotation {
                id: note.id,
                text: next,
            });
        }
        return false;
    }
    ui.data(|data| data.get_temp::<String>(buf_id))
        .is_some_and(|pending| pending.trim() != note.text)
}

fn part_name(session: &Session, node: NodeId) -> String {
    session
        .doc
        .as_ref()
        .and_then(|doc| doc.node(node))
        .map(|node| node.name.to_string())
        .unwrap_or_else(|| "部品".to_owned())
}

fn mark(index: usize) -> String {
    let number = index + 1;
    if (1..=20).contains(&number) {
        char::from_u32(0x2460 + number as u32 - 1)
            .map(|mark| mark.to_string())
            .unwrap_or_else(|| number.to_string())
    } else {
        number.to_string()
    }
}
