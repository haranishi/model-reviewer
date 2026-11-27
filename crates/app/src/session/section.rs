//! 断面（`core::section` をつなぐ）。
//!
//! 約束: 断面は `Session::section` と、切っている間の控え `Session::section_memory` で持つ。
//! 履歴・未保存の印・レビューJSONに入れない。変えたら `snap_preview` を消し、
//! `scene_dirty.section` を立てる。部品の表もインスタンスも作り直さない。

use model_reviewer_core::section::SectionPlane;

use super::Session;

/// 断面を置く（`None` で外す）。置く前に境界箱へ収める。
pub(super) fn set_section(session: &mut Session, plane: Option<SectionPlane>) {
    // 契約の先置き。中身は接合の作業で書く。
    let _ = (session, plane);
}

/// 断面の入・切。
pub(super) fn toggle_section(session: &mut Session) {
    // 契約の先置き。中身は接合の作業で書く。
    let _ = session;
}
