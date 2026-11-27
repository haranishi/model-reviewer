//! 断面の枠と矢印（3Dビューの上に egui で描く）。矢印のドラッグで `SetSection(Some(plane))` を
//! 毎フレーム積む。当たりは `gizmo_view` と同じく、線から12ポイントに入っている間だけ小さな
//! 操作域を重ねる。名前は `ids::SECTION_HANDLE`。`Session` を読まず、入力用の構造体だけを受け取る。

use model_reviewer_core::camera::OrbitCamera;
use model_reviewer_core::math::Aabb;
use model_reviewer_core::section::SectionPlane;

use crate::command::Command;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionViewInput {
    pub plane: SectionPlane,
    /// 枠を作る境界箱（見えている全体）。
    pub bounds: Aabb,
    pub camera: OrbitCamera,
    /// 3Dビューの矩形（画面の座標）。
    pub rect: egui::Rect,
    /// 矢印を掴んでいるか。
    pub grabbing: bool,
}

pub fn show(ui: &mut egui::Ui, input: &SectionViewInput, out: &mut Vec<Command>) {
    // 契約の先置き。中身は後の作業で書く（いまは何も描かない）。
    let _ = (ui, input, out);
}
