//! 右のパネルの「計測」：距離・ΔX・ΔY・ΔZ（有効数字6桁）と、2点が何に吸着したか。
//! 「消す」で `ClearMeasure` を積む。名前は `ids::MEASURE_CLEAR`。
//! `Session` を読まず、入力用の構造体だけを受け取る。

use glam::Vec3;
use model_reviewer_core::snap::SnapKind;
use model_reviewer_core::units::Units;

use crate::command::Command;

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

pub fn show(ui: &mut egui::Ui, input: &MeasurePanelInput, out: &mut Vec<Command>) {
    // 契約の先置き。中身は後の作業で書く（いまは何も描かない）。
    let _ = (ui, input, out);
}
