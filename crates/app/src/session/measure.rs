//! 計測と単位。
//!
//! 約束: 計測は保存せず、履歴にも未保存の印にも入らない。点は `(NodeKey, 部品の中の座標)` で
//! 持つので、部品を動かすと距離が変わり、部品を削除すると計測が消える（`edit::after_applied`）。
//! 単位は「ファイルの単位（前提）」と「表示の単位」に分ける。シーンの数値は換算しない。
//! 表示が「ファイルと同じ」の間は、画面の数値も文字も第2段階と同じになる。

use glam::Vec3;
use model_reviewer_core::measure::MeasurePoint;
use model_reviewer_core::snap::SnapTarget;
use model_reviewer_core::units::UnitLabel;

use super::{Session, SourceKind};

/// 計測の道具でのクリック。表面に当たれば点を足し、3回目は新しい計測の1点目になる。
pub(super) fn click(session: &mut Session, target: Option<SnapTarget>) {
    let Some(target) = target else {
        session.status_text = "計測: 表面をクリックしてください".to_owned();
        return;
    };
    let Some(key) = session.doc.as_ref().and_then(|doc| doc.key(target.node)) else {
        return;
    };
    session.measurement.push_with(
        MeasurePoint {
            node: key,
            local: target.local,
        },
        target.kind,
    );
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

/// いまの差（2点目 − 1点目、ワールド）。
pub fn delta(session: &Session) -> Option<Vec3> {
    let doc = session.doc.as_ref()?;
    session.measurement.delta(doc)
}

pub(super) fn clear_measure(session: &mut Session) {
    if session.measurement.first.is_some() {
        session.measurement.clear();
        session.status_text = "計測を消しました".to_owned();
    }
}

/// ファイルの単位（前提）を替える。GLB だけ。数値は換算しない。STL の仮定は `SetStlUnit`。
pub(super) fn set_unit(session: &mut Session, unit: UnitLabel) {
    if matches!(session.source_kind, SourceKind::Stl { .. }) {
        return;
    }
    session.units.source = unit;
}

/// 表示の単位を替える（`None` は「ファイルと同じ」）。数値を換算するのは画面の出入り口だけ。
pub(super) fn set_display_unit(session: &mut Session, unit: Option<UnitLabel>) {
    session.units.display = unit;
}
