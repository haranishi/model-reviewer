//! 右のパネルの「寸法」：外形（選択と子孫の境界箱の大きさ）と、寸法を指定できる図形の欄。
//! 欄の値は表示の単位で出し、`Units::from_display` と `scale_for` で大きさに戻して、ドラッグ中は
//! `PreviewLocal`、確定で `CommitLocal`、Esc で `CancelTransform` を積む。新しい `Command` は
//! 足さない。名前は `ids::DIMS_*`。`Session` を読まず、`DimsView` だけを受け取る。

use crate::command::Command;
use crate::session::DimsView;

pub fn show(ui: &mut egui::Ui, view: &DimsView, out: &mut Vec<Command>) {
    // 契約の先置き。中身は後の作業で書く（いまは何も描かない）。
    let _ = (ui, view, out);
}
