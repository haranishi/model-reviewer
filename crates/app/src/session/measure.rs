//! 計測と単位。
//!
//! 約束: 計測は保存せず、履歴にも未保存の印にも入らない。点は `(NodeKey, 部品の中の座標)` で
//! 持つので、部品を動かすと距離が変わり、部品を削除すると計測が消える（`edit::after_applied`）。
//! 単位は添える文字だけを替える（数値は換算しない）。

use model_reviewer_core::measure::MeasurePoint;
use model_reviewer_core::pick::Hit;
use model_reviewer_core::units::UnitLabel;

use super::Session;

/// 計測の道具でのクリック。表面に当たれば点を足し、3回目は新しい計測の1点目になる。
pub(super) fn click(session: &mut Session, hit: Option<Hit>) {
    let Some(hit) = hit else {
        session.status_text = "計測: 表面をクリックしてください".to_owned();
        return;
    };
    let Some(key) = session.doc.as_ref().and_then(|doc| doc.key(hit.node)) else {
        return;
    };
    session.measurement.push(MeasurePoint {
        node: key,
        local: hit.local_pos,
    });
    // 2点そろえば、距離は状態欄の右（「距離 …」）と線の脇に出るので、知らせは使い方に戻す。
    if distance(session).is_some() {
        session.status_text.clear();
    } else {
        session.status_text = "計測: もう1点をクリック".to_owned();
    }
}

/// いまの距離（2点がそろっていて、部品が文書にあるとき）。
pub fn distance(session: &Session) -> Option<f32> {
    let doc = session.doc.as_ref()?;
    session.measurement.distance(doc)
}

pub(super) fn clear_measure(session: &mut Session) {
    if session.measurement.first.is_some() {
        session.measurement.clear();
        session.status_text = "計測を消しました".to_owned();
    }
}

pub(super) fn set_unit(session: &mut Session, unit: UnitLabel) {
    session.unit = unit;
}
