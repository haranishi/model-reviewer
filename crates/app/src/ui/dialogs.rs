//! 確認ダイアログ。中身は `Session` が渡す。

use crate::command::Command;
use crate::session::Session;

use super::reg;

pub fn show(ui: &mut egui::Ui, session: &Session, out: &mut Vec<Command>) {
    let Some(dialog) = &session.dialog else {
        return;
    };
    let ctx = ui.ctx().clone();
    let max_w = (ctx.content_rect().width() - 64.0).clamp(260.0, 480.0);
    egui::Modal::new(egui::Id::unique("app.dialog")).show(&ctx, |ui| {
        ui.set_max_width(max_w);
        ui.heading(dialog.title.as_str());
        ui.add_space(4.0);
        ui.add(egui::Label::new(dialog.body.as_str()).wrap());
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            for button in &dialog.buttons {
                let response = ui.button(button.label.as_str());
                reg(&button.id, &response);
                if response.clicked() {
                    out.push(button.command.clone());
                }
            }
        });
    });
}
