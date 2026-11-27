//! 表示の切り替え（面・面と線・線・透かし）と、変形中の差分の行列。
//!
//! 約束: 「面と線」「線」は `PolygonMode::Line` のパイプラインで描く。使えないGPUでは
//! `SceneRenderer::supports_lines` が偽になり、画面がその表示を無効にする。変形中は、範囲の部品に
//! 差分の行列を1つ掛けて描き、インスタンスは書き直さない（送るのは定数バッファ1つ）。
//!
//! 描き方は `SceneRenderer::render` にある。「面と線」は面の後に線のパイプラインで同じ形状をもう一度
//! 描き（深度は「同じか手前」で比べ、偏りで手前に寄せる）、「線」は深度を比べずに線だけ、「透かし」は
//! 深度を比べず書かずに半透明で描く。差分の行列は本体と輪郭のマスクの両方のシェーダーが掛ける。

use std::ops::Range;

use glam::Mat4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DisplayMode {
    /// 第1段階のまま。
    #[default]
    Faces,
    FacesAndLines,
    Lines,
    XRay,
}

/// 変形中だけ `Some`。`nodes` は `NodeId` の連続範囲（選択した部品と子孫）。
#[derive(Clone, Debug, PartialEq)]
pub struct DragPreview {
    pub nodes: Range<u32>,
    pub world_delta: Mat4,
}
