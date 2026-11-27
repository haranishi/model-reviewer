//! 断面の操作の帯：軸（X・Y・Z）、位置の欄、反転。押すと `SetSection(Some(plane))` を積む。
//! 軸を替えたら、位置は新しい軸の中央にする。位置の欄は表示の単位で出し、
//! `Units::from_display` でシーンの値へ戻す。名前は `ids::SECTION_*`。
//! `Session` を読まず、入力用の構造体だけを受け取る。

use model_reviewer_core::math::Aabb;
use model_reviewer_core::section::SectionPlane;
use model_reviewer_core::units::Units;

use crate::command::Command;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionBarInput {
    /// いまの平面。
    pub plane: SectionPlane,
    /// 位置の範囲（見えている全体の境界箱）。
    pub bounds: Aabb,
    pub units: Units,
}

pub fn show(ui: &mut egui::Ui, input: &SectionBarInput, out: &mut Vec<Command>) {
    // 契約の先置き。中身は後の作業で書く（いまは何も描かない）。
    let _ = (ui, input, out);
}
