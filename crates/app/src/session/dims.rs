//! 寸法と外形（右のパネルの入力を組み立てる）。
//!
//! 約束: 寸法は部品の大きさ（`scale`）として持つ。確定は既存の `CommitLocal` で、新しい
//! `Command` は足さない。`shape` が `Some` になるのは、選択した部品が子を持たず、描画が1つで、
//! その形状に `ShapeSpec` があるときだけ。

use model_reviewer_core::doc::NodeKey;
use model_reviewer_core::gizmo::{Trs, decompose};
use model_reviewer_core::scene::{NodeId, SceneRead};
use model_reviewer_core::shapes::{Dims, ShapeSpec, dims_of};
use model_reviewer_core::units::Units;

use super::Session;
use crate::ui::dims::NOT_DIMENSIONAL;

/// 右のパネルの「寸法」の入力。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DimsView {
    /// 選んだ部品の鍵。欄の一時メモリと入力位置を部品ごとに分ける（選択を替えたとき、前の部品に
    /// 打ちかけた文字が次の部品に当てはまらないように）。
    pub node: Option<NodeKey>,
    /// 選択と子孫のワールドの境界箱の大きさ（シーンの単位）。空なら `None`。
    pub extent: Option<[f32; 3]>,
    pub shape: Option<ShapeDims>,
    pub units: Units,
    pub doc_rev: u64,
}

/// 寸法を直せる図形の情報。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeDims {
    /// 寸法（シーンの単位）。拡大や回転で形が変わっていれば `None`。
    pub dims: Option<Dims>,
    /// いまの部品の変形（寸法の確定は `scale` だけを差し替えた値を積む）。
    pub trs: Trs,
    pub spec: ShapeSpec,
    /// 寸法で直せない理由。
    pub blocked: Option<&'static str>,
}

/// 選択した部品の寸法と外形。選択が無ければ `None`。
pub fn selection_dims(session: &Session) -> Option<DimsView> {
    let doc = session.doc.as_ref()?;
    let id = session
        .selection
        .filter(|id| (id.0 as usize) < doc.node_count())?;
    let node = doc.node(id)?;
    let bounds = doc.subtree_bounds(id);
    let size = bounds.max - bounds.min;
    let extent = (!bounds.is_empty() && size.is_finite()).then(|| size.to_array());
    Some(DimsView {
        node: Some(node.key),
        extent,
        shape: shape_dims(session, id),
        units: session.units,
        doc_rev: session.history.doc_rev(),
    })
}

/// 寸法を直せる図形か。子を持たず、描画が1つで、その形状に作り方の記録があるときだけ `Some`。
fn shape_dims(session: &Session, id: NodeId) -> Option<ShapeDims> {
    let doc = session.doc.as_ref()?;
    let node = doc.node(id)?;
    if node.subtree_end != id.0 + 1 {
        return None;
    }
    let [draw] = doc.draws(id) else {
        return None;
    };
    let spec = doc.shape_spec(draw.prim)?;
    let locked = if session.is_exporting() {
        Some(crate::ui::LOCKED_EXPORT)
    } else if session.edits_locked() {
        Some(crate::ui::LOCKED)
    } else {
        None
    };
    let Some(trs) = decompose(&node.local) else {
        return Some(ShapeDims {
            dims: None,
            trs: Trs::default(),
            spec,
            blocked: Some(locked.unwrap_or(NOT_DIMENSIONAL)),
        });
    };
    let dims = dims_of(spec, trs.scale);
    Some(ShapeDims {
        dims,
        trs,
        spec,
        blocked: locked.or(dims.is_none().then_some(NOT_DIMENSIONAL)),
    })
}
