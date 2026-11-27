//! STL の読み込み。バイト列から `core::scene::Model`（部品1つ・描画1つ・形状1つ）を作る。
//! ファイル形式のクレートには依存しない。`gltf` とも互いに依存しない（つなぐのは `app`）。
//!
//! 約束:
//! - 判定（`detect`）は、ASCII の印（`solid` の後に `facet` か `endsolid`）を先に見て、無ければ
//!   バイナリの大きさ（`84 + 50 × 三角形の数`）を見る。`solid` で始まるバイナリもある。
//! - 座標の値は形状にそのまま入れ、部品の `local` に単位の倍率と上の向きの回転を入れる。
//! - 申告された三角形の数で確保しない。座標が有限でないか `COORD_LIMIT` を超えたら断る。

mod ascii;
mod binary;
mod detect;
mod error;
mod weld;

use glam::Affine3A;
use model_reviewer_core::scene::{Draw, Model, ModelStats, Node, PrimId};
use model_reviewer_core::units::{UnitLabel, UpAxis};

pub use detect::detect;
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
    pub up: UpAxis,
    pub allow_trailing: bool,
}

/// 部品の色（明るい灰色1色）。
pub const STL_COLOR: [f32; 4] = [0.72, 0.72, 0.72, 1.0];
/// 座標の絶対値の上限。長さの2乗が `f32` で無限大にならない範囲。
pub const COORD_LIMIT: f32 = 1.0e18;

/// STL を読んで `Model` を作る。
pub fn load_stl(bytes: &[u8], options: &StlOptions) -> Result<Model, StlError> {
    let parsed = match detect(bytes, options.allow_trailing) {
        Some(StlKind::Ascii) => ascii::parse(bytes)?,
        Some(StlKind::Binary) => binary::parse(bytes, options)?,
        None if detect::looks_like_broken_ascii(bytes) => ascii::parse(bytes)?,
        None => binary::parse(bytes, options)?,
    };
    Ok(assemble_model(parsed, options))
}

fn assemble_model(parsed: binary::Parsed, options: &StlOptions<'_>) -> Model {
    let scale = options.unit.meters() as f32;
    let local = Affine3A::from_mat3(options.up.rotation() * scale);
    let world = local;
    let world_aabb = parsed.primitive.local_aabb.transformed(&world);
    let triangles = (parsed.primitive.indices.len() / 3) as u64;
    let node = Node {
        name: options.name.to_owned(),
        gltf_index: 0,
        parent: None,
        subtree_end: 1,
        depth: 0,
        local,
        world,
        draws: 0..1,
        world_aabb,
    };
    let draw = Draw {
        prim: PrimId(0),
        base_color: STL_COLOR,
    };
    Model {
        nodes: vec![node],
        draws: vec![draw],
        primitives: vec![parsed.primitive],
        bounds: world_aabb,
        stats: ModelStats {
            nodes: 1,
            meshes: 1,
            primitives: 1,
            unique_triangles: triangles,
            placed_triangles: triangles,
        },
        warnings: parsed.warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_is_rejected_without_panic() {
        let options = StlOptions {
            name: "部品",
            unit: UnitLabel::Mm,
            up: UpAxis::default(),
            allow_trailing: true,
        };
        assert_eq!(detect(&[], true), None);
        assert_eq!(load_stl(&[], &options), Err(StlError::TooShort));
    }
}
