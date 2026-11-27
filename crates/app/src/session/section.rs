//! 断面（`core::section` をつなぐ）。
//!
//! 約束: 断面は `Session::section` と、切っている間の控え `Session::section_memory` で持つ。
//! 履歴・未保存の印・レビューJSONに入れない。変えたら `snap_preview` を消し、
//! `scene_dirty.section` を立てる。部品の表もインスタンスも作り直さない。
//! 位置は置くたびに、いまの文書の境界箱へ収める（編集で箱が変わっても、勝手には動かさない）。

use model_reviewer_core::math::Aabb;
use model_reviewer_core::scene::{NodeId, SceneRead};
use model_reviewer_core::section::{Axis, SectionPlane};

use super::Session;

/// 断面の位置を収める範囲（見えている全体の境界箱。要件3.1・3.3）。隠した部品は含めない。
/// 全部を隠しているときは、全体の境界箱にする（断面を入れられなくならないように）。
/// モデルが無ければ `None`。
pub fn section_bounds(session: &Session) -> Option<Aabb> {
    let doc = session.doc.as_ref()?;
    let Some(review) = session.review.as_ref() else {
        return Some(doc.bounds());
    };
    let visible = doc
        .nodes()
        .iter()
        .enumerate()
        .filter(|(index, _)| review.visibility.effective(NodeId(*index as u32)))
        .fold(Aabb::empty(), |bounds, (_, node)| {
            bounds.union(node.world_aabb)
        });
    Some(if visible.is_empty() {
        doc.bounds()
    } else {
        visible
    })
}

/// 断面を置く（`None` で外す）。置く前に境界箱へ収める。
pub(super) fn set_section(session: &mut Session, plane: Option<SectionPlane>) {
    let Some(bounds) = section_bounds(session) else {
        return;
    };
    let next = plane.map(|plane| plane.clamped(&bounds));
    if let Some(plane) = next {
        session.section_memory = plane;
        session.section_remembered = true;
    }
    replace(session, next);
}

/// 断面の入・切。最初は Z 軸・中央・正の側を消す。切ってからもう一度入れると、前の軸と位置。
pub(super) fn toggle_section(session: &mut Session) {
    let Some(bounds) = section_bounds(session) else {
        return;
    };
    if let Some(plane) = session.section {
        session.section_memory = plane;
        session.section_remembered = true;
        replace(session, None);
        session.status_text = "断面: 切".to_owned();
        return;
    }
    let plane = if session.section_remembered {
        session.section_memory.clamped(&bounds)
    } else {
        SectionPlane::centered(Axis::Z, &bounds)
    };
    session.section_memory = plane;
    session.section_remembered = true;
    replace(session, Some(plane));
    session.status_text = "断面: 入".to_owned();
}

/// モデルを開く・閉じるとき。断面も控えも消す。
pub(super) fn reset(session: &mut Session) {
    replace(session, None);
    session.section_memory = SectionPlane {
        axis: Axis::Z,
        offset: 0.0,
        flipped: false,
    };
    session.section_remembered = false;
}

fn replace(session: &mut Session, next: Option<SectionPlane>) {
    if session.section != next {
        session.section = next;
        session.scene_dirty.section = true;
        super::snap::set_preview(session, None);
    }
}
