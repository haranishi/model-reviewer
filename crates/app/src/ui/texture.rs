//! 選んだ部品の画像と、表示単位で入力する箱形の大きさ。

use crate::command::Command;
use crate::session::TextureView;

use super::{ids, reg};

pub fn show(ui: &mut egui::Ui, input: &TextureView, out: &mut Vec<Command>) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("画像").strong());
        reg(ids::TEXTURE_STATE, &ui.label(&input.state));
    });
    ui.horizontal_wrapped(|ui| {
        let paste = ui.add_enabled(
            input.paste_block.is_none(),
            egui::Button::new("画像を貼る…"),
        );
        reg(ids::TEXTURE_PASTE, &paste);
        if paste.clicked() {
            out.push(Command::PasteTextureDialog);
        }
        if let Some(reason) = input.paste_block {
            paste.on_disabled_hover_text(reason);
        }
        let remove = ui.add_enabled(
            input.has_image && !input.locked,
            egui::Button::new("画像を外す"),
        );
        reg(ids::TEXTURE_REMOVE, &remove);
        if remove.clicked() {
            out.push(Command::RemoveTexture);
        }
        if !input.has_image {
            remove.on_disabled_hover_text("この部品には、外す画像がありません");
        } else if input.locked {
            remove.on_disabled_hover_text("読み込み・書き出しが終わると画像を外せます");
        }
    });
    if input.has_box {
        ui.add_enabled_ui(!input.locked, |ui| size_field(ui, input, out));
    }
    if let Some(reason) = input.paste_block {
        reg(ids::TEXTURE_NOTE, &ui.label(reason));
    }
}

fn size_field(ui: &mut egui::Ui, input: &TextureView, out: &mut Vec<Command>) {
    let unit = input.units.shown();
    let id = ui
        .scope_id()
        .with((ids::TEXTURE_SIZE, input.key.0, input.doc_rev, unit));
    let value = input
        .world_size
        .map(|w| (f64::from(w) / unit.meters()) as f32);
    let mut text = ui
        .data_mut(|d| d.get_temp::<String>(id))
        .unwrap_or_else(|| value.map(|v| v.to_string()).unwrap_or_default());
    let field = ui
        .horizontal(|ui| {
            let label = ui.label("1枚の大きさ");
            let field = ui
                .add(
                    egui::TextEdit::singleline(&mut text)
                        .id(id)
                        .desired_width(72.0)
                        .hint_text("複数の値"),
                )
                .labelled_by(label.id);
            ui.label(unit.suffix());
            field
        })
        .inner;
    reg(ids::TEXTURE_SIZE, &field);
    if field.lost_focus() {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            crate::app::mark_text_escape(ui.ctx());
        } else if value.map(|v| v.to_string()).unwrap_or_default() != text {
            let size = text.trim().parse::<f64>().unwrap_or(f64::NAN) * unit.meters();
            out.insert(0, Command::SetTextureSize(size as f32));
        }
        ui.data_mut(|d| d.remove::<String>(id));
    } else if field.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, text));
    }
}
