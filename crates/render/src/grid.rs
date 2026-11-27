//! 床の格子と軸の線。高さ0の水平面を画面全体の三角形1枚で描き、フラグメントシェーダーが視線と
//! 平面の交点から線の濃さと深度を決める。形状の後に同じパスで描き、深度は比べるが書かない。
//!
//! 視線は `view_proj` の逆行列で画素を近い面（深度1）と深度0.5へ戻して作るので、透視でも平行投影でも
//! 同じシェーダーで働く。シェーダーは `shaders/grid.wgsl`、パイプラインは `pipelines.rs`。
//! `frag_depth` は4サンプルの描画先でも前後を正しく付ける（`tests/frag_depth.rs` で確かめた）。

/// 格子の入力。間隔は `core::camera::grid_spacing` が決める（状態欄と同じ値）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridParams {
    pub spacing: f32,
    pub visible: bool,
}

/// 既定は「描かない」。第1段階と同じ絵になる。
impl Default for GridParams {
    fn default() -> Self {
        Self {
            spacing: 1.0,
            visible: false,
        }
    }
}
