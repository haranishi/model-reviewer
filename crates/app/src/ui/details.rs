//! 性能値。詳細パネルと `--log-metrics` は同じ値を出す。
//! パネルの高さは固定（`panel_height`）で、中を縦にスクロールする。高さを中身に合わせて
//! 変えると、最後の行が状態欄と重なる（1280×800 の実機で起きた）。

use crate::command::Command;
use crate::metrics::{self, Counts};
use crate::session::Session;
use crate::theme;

/// パネルの高さの上限。
const PANEL_CAP: f32 = 280.0;
/// 画面の高さに対するパネルの高さの上限。3Dの表示を狭くしすぎない。
const PANEL_SHARE: f32 = 0.35;

/// 詳細パネルの高さ。`min(280, 画面の35%)`。
pub fn panel_height(screen_height: f32) -> f32 {
    PANEL_CAP.min(screen_height * PANEL_SHARE).max(40.0)
}

pub fn show(ui: &mut egui::Ui, session: &Session, out: &mut Vec<Command>) {
    let _ = out;
    ui.label("詳細");
    let counts = session
        .model
        .as_ref()
        .map(|model| Counts::from_stats(model.stats));
    let lines = metrics::report_lines(&session.metrics, counts);
    egui::ScrollArea::vertical()
        .id_salt("details.lines")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let p = &session.textures;
            if session.model.is_some() {
                ui.label(format!(
                    "テクスチャ　表示 {} 枚／対象 {} 枚、縮小 {} 枚、転送 {} バイト",
                    p.displayed(),
                    session.texture_counts().0,
                    p.shrunk,
                    p.bytes
                ));
            }
            for line in &lines {
                // 値の行は短く保ち、説明は次の行に小さく薄く出す。
                ui.add(egui::Label::new(format!("{}: {}", line.label, line.value)).wrap());
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(format!("（{}）", line.about))
                            .size(theme::SMALL_SIZE)
                            .color(theme::TEXT_WEAK),
                    )
                    .wrap(),
                );
                ui.add_space(2.0);
            }
        });
}
