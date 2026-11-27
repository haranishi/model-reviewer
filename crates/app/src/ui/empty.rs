//! モデルが無いときの中央。

use crate::command::Command;
use crate::loader::LoadPhase;
use crate::session::Session;

/// モデルが無いときの案内（要件5.1）。
pub const EMPTY_HINT: &str = "GLB か STL をここへドロップ、または［開く］";

pub fn show(ui: &mut egui::Ui, session: &Session, out: &mut Vec<Command>) {
    let _ = out;
    ui.vertical_centered(|ui| {
        ui.add_space(24.0);
        match &session.load {
            LoadPhase::Reading | LoadPhase::Parsing | LoadPhase::Uploading { .. } => {
                if let Some(name) = &session.loading_file_name {
                    ui.label(name);
                }
                if let Some(label) = session.load.label() {
                    ui.label(label);
                }
            }
            LoadPhase::Idle | LoadPhase::Failed(_) => {
                ui.label(EMPTY_HINT);
                ui.add_space(12.0);
                // ヘルプと同じ、幅に合わせて折り返す表。右端で切れないよう、使える幅に収める。
                // 低い画面では下が切れないよう、縦にスクロールする。
                egui::ScrollArea::vertical()
                    .id_salt("empty.scroll")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_max_width(ui.available_width());
                            super::help::operations(ui, "empty.operations");
                        });
                    });
            }
        }
    });
}
