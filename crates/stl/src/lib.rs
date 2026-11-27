//! STL の読み込み。バイト列から `core::scene::Model`（部品1つ・描画1つ・形状1つ）を作る。
//! ファイル形式のクレートには依存しない。`gltf` とも互いに依存しない（つなぐのは `app`）。
//!
//! 約束:
//! - 判定（`detect`）は、ASCII の印（`solid` の後に `facet` か `endsolid`）を先に見て、無ければ
//!   バイナリの大きさ（`84 + 50 × 三角形の数`）を見る。`solid` で始まるバイナリもある。
//! - 座標の値は形状にそのまま入れ、部品の `local` に単位の倍率（mm なら 0.001）を入れる。
//! - 申告された三角形の数で確保しない。座標が有限でないか `COORD_LIMIT` を超えたら断る。

mod error;

use model_reviewer_core::scene::Model;
use model_reviewer_core::units::UnitLabel;

pub use error::StlError;

/// STL の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StlKind {
    Binary,
    Ascii,
}

/// 読み込みの指定。`name` は部品の名前、`unit` は座標の単位の仮定。
/// `allow_trailing` が真なら、バイナリの三角形の後ろの余りを警告にして読む。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StlOptions<'a> {
    pub name: &'a str,
    pub unit: UnitLabel,
    pub allow_trailing: bool,
}

/// 部品の色（明るい灰色1色）。
pub const STL_COLOR: [f32; 4] = [0.72, 0.72, 0.72, 1.0];
/// 座標の絶対値の上限。長さの2乗が `f32` で無限大にならない範囲。
pub const COORD_LIMIT: f32 = 1.0e18;

/// STL かどうかと、その種類。どちらでもなければ `None`。
pub fn detect(bytes: &[u8], allow_trailing: bool) -> Option<StlKind> {
    // 契約の先置き。中身は後の作業で書く。
    let _ = (bytes, allow_trailing);
    None
}

/// STL を読んで `Model` を作る。
pub fn load_stl(bytes: &[u8], options: &StlOptions) -> Result<Model, StlError> {
    // 契約の先置き。中身は後の作業で書く。
    let _ = (bytes, options);
    Err(StlError::NotStl)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_do_not_panic() {
        let options = StlOptions {
            name: "部品",
            unit: UnitLabel::Mm,
            allow_trailing: true,
        };
        assert_eq!(detect(&[], true), None);
        assert!(matches!(load_stl(&[], &options), Err(StlError::NotStl)));
    }
}
