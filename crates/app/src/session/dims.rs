//! 寸法と外形（右のパネルの入力を組み立てる）。
//!
//! 約束: 寸法は部品の大きさ（`scale`）として持つ。確定は既存の `CommitLocal` で、新しい
//! `Command` は足さない。`shape` が `Some` になるのは、選択した部品が子を持たず、描画が1つで、
//! その形状に `ShapeSpec` があるときだけ。

use model_reviewer_core::gizmo::Trs;
use model_reviewer_core::shapes::{Dims, ShapeSpec};
use model_reviewer_core::units::Units;

use super::Session;

/// 右のパネルの「寸法」の入力。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DimsView {
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
    // 契約の先置き。中身は接合の作業で書く。
    let _ = session;
    None
}
