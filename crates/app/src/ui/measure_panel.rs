//! 右のパネルの「計測」：距離・ΔX・ΔY・ΔZ（有効数字6桁）と、2点が何に吸着したか。
//! 「消す」で `ClearMeasure` を積む。名前は `ids::MEASURE_CLEAR`。
//! `Session` を読まず、入力用の構造体だけを受け取る。

use glam::Vec3;
use model_reviewer_core::snap::SnapKind;
use model_reviewer_core::units::Units;

use crate::command::Command;
use crate::theme;

use super::ids;
use super::reg;

/// 右のパネルの有効数字（要件2.3）。
pub const PANEL_DIGITS: u8 = 6;
/// 点が無いときの1行。
pub const NO_POINTS: &str = "表面を2回クリックすると距離が出ます";
/// 1点だけのときの1行。
pub const ONE_POINT: &str = "もう1点をクリックしてください";
/// 「消す」を押せないときの理由。
pub const NOTHING_TO_CLEAR: &str = "消す点がありません";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasurePanelInput {
    /// 2点がそろっていれば距離（シーンの単位）。
    pub distance: Option<f32>,
    /// 2点目 − 1点目（ワールド、シーンの単位）。
    pub delta: Option<Vec3>,
    /// 置いた点の数（0〜2）。
    pub points: u8,
    /// 1点目と2点目が何に吸着したか。
    pub kinds: [SnapKind; 2],
    pub units: Units,
}

/// 吸着の種類の言葉（「頂点」「辺」「面」）。
pub fn kind_name(kind: SnapKind) -> &'static str {
    match kind {
        SnapKind::Vertex => "頂点",
        SnapKind::Edge => "辺",
        SnapKind::Face => "面",
    }
}

/// 表に出す行（見出しと値）。2点がそろっていなければ距離と差の行は出さない。
/// 値が無いか有限でなければ「—」（0や架空の数を出さない。要件2.5）。
pub fn rows(input: &MeasurePanelInput) -> Vec<(&'static str, String)> {
    let mut rows = Vec::new();
    let points = input.points.min(2);
    if points == 2 {
        let length = |value: Option<f32>| match value {
            Some(value) => input.units.format(value, PANEL_DIGITS),
            None => input.units.format(f32::NAN, PANEL_DIGITS),
        };
        rows.push(("距離", length(input.distance)));
        let delta = input.delta;
        rows.push(("ΔX", length(delta.map(|d| d.x))));
        rows.push(("ΔY", length(delta.map(|d| d.y))));
        rows.push(("ΔZ", length(delta.map(|d| d.z))));
    }
    if points >= 1 {
        rows.push(("1点目", kind_name(input.kinds[0]).to_owned()));
    }
    if points == 2 {
        rows.push(("2点目", kind_name(input.kinds[1]).to_owned()));
    }
    rows
}

pub fn show(ui: &mut egui::Ui, input: &MeasurePanelInput, out: &mut Vec<Command>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("計測").strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let enabled = input.points > 0;
            let button = ui.add_enabled(
                enabled,
                egui::Button::new("消す").min_size(egui::vec2(48.0, 24.0)),
            );
            let button = if enabled {
                button.on_hover_text("置いた計測の点を消します")
            } else {
                button.on_disabled_hover_text(NOTHING_TO_CLEAR)
            };
            reg(ids::MEASURE_CLEAR, &button);
            if button.clicked() {
                out.push(Command::ClearMeasure);
            }
        });
    });
    match input.points {
        0 => {
            ui.label(egui::RichText::new(NO_POINTS).color(theme::TEXT_WEAK));
            return;
        }
        1 => {
            ui.label(egui::RichText::new(ONE_POINT).color(theme::TEXT_WEAK));
        }
        _ => {}
    }
    egui::Grid::new(ui.scope_id().with("measure_panel"))
        .num_columns(2)
        .spacing(egui::vec2(10.0, 4.0))
        .show(ui, |ui| {
            for (name, value) in rows(input) {
                ui.label(name);
                ui.label(value);
                ui.end_row();
            }
        });
}
