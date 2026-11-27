//! GLBの書き出し。元のファイルのバイト列を土台にし、JSONは値（`gltf::json::Value`。
//! `serde_json::Value` の再公開）として変えた所だけを直し、足す物は配列の末尾に追記する。
//! 既存の要素の番号は1つも変えない。BINは元のバイト列をそのまま置き、足した形状だけを末尾に足す。
//!
//! 約束（呼び出し側が頼ること）:
//! - `build` は `base` を変えない。`assigned[i]` は `scene.nodes[i]` のファイル内のノード番号で、
//!   既存の部品は元の番号のまま。削除した部品はシーンから外すだけで、ノードは配列に残す。
//! - `write_container` は `base` のBINの範囲と `built.appended_bin` を順に書き、書いたバイト数を返す。
//!   全体が4GiBを超えるときは書く前に `TooLarge`。
//! - `verify` は、書いたバイト列が「元のBINと同じ位置に同じバイト」「触らない項目が値として同じ」
//!   「`load_glb` で開け、部品の名前・ワールド行列・三角形数が `scene` と一致」を満たすか確かめる。
//! - 変えていない部品の判定は、ファイルの値を読み込みと同じ関数（`crate::build::local_transform`）で
//!   行列にし、`local` とビット単位で一致すること。
//!
//! 断るとき（`ExportError`）は何も書かない。`build` と `verify` は純粋な関数で、ファイルは扱わない。

mod append;
mod base;
mod container;
mod json;
mod patch;
mod verify;

use std::io;
use std::sync::Arc;

use glam::Affine3A;
use model_reviewer_core::scene::{Draw, Primitive};

/// 書き出しの入力。`core::doc::Document` から接合（WP-J4）が作る、部品の表の写し。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportScene {
    /// 行きがけ順。
    pub nodes: Vec<ExportNode>,
    /// 足した図形の形状。`Draw::prim` が `base_primitive_count` 以上なら、ここの
    /// `prim - base_primitive_count` 番を指す。
    pub added: Vec<Arc<Primitive>>,
    pub base_primitive_count: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportNode {
    /// `nodes` の添字。
    pub parent: Option<u32>,
    pub name: String,
    pub local: Affine3A,
    /// ファイルにある部品の、GLBの中のノード番号。
    pub file_index: Option<u32>,
    /// まだファイルに無い複製が、どのノード（ファイル内の番号）のJSONを写すか。
    pub template: Option<u32>,
    pub draws: Vec<Draw>,
}

/// 組み立ての結果。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Built {
    /// 新しいJSONチャンク（詰め物の前）。
    pub json: Vec<u8>,
    /// 元のBINの後ろに足すバイト列（4バイト境界にそろえた後の分）。
    pub appended_bin: Vec<u8>,
    /// `scene.nodes[i]` のファイル内のノード番号。
    pub assigned: Vec<u32>,
    pub notes: Vec<ExportNote>,
}

/// 書き出しの結果の一覧に出すこと。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportNote {
    /// シーンから外した部品（データはファイルに残る）。
    DetachedNodes {
        count: u32,
    },
    AnimatedNodesChanged {
        count: u32,
    },
    SkinnedNodesChanged {
        count: u32,
    },
    /// 位置・回転・大きさに分けられず `matrix` で書いた部品。
    MatrixWritten {
        count: u32,
    },
    /// シーンが無いファイルに、根を並べたシーンを足した。
    SceneAdded,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExportError {
    /// 元のGLBを読めない（容器・JSON・BIN）。
    #[error("base: {0}")]
    InvalidBase(String),
    /// `ExportScene` が元のファイルと合わない（範囲外の番号など）。
    #[error("scene: {0}")]
    InvalidScene(String),
    /// GLBの長さ（32ビット）を超える。
    #[error("too large: {bytes}")]
    TooLarge { bytes: u64 },
    #[error("io: {0}")]
    Io(String),
    /// 書いた結果の照合に失敗した。
    #[error("verify: {0}")]
    Verify(String),
    /// 中身がまだ無い（E0 の置き場所。いまは使わない）。
    #[error("not implemented")]
    NotImplemented,
}

impl ExportError {
    /// 利用者に出す文。
    pub fn message_ja(&self) -> String {
        match self {
            Self::InvalidBase(detail) => {
                format!("元のファイルを書き出しの土台にできません: {detail}")
            }
            Self::InvalidScene(detail) => {
                format!("書き出す内容が元のファイルと合いません: {detail}")
            }
            Self::TooLarge { bytes } => format!(
                "書き出すと {bytes} バイトになり、GLBの上限（4GiB）を超えるため書き出せません"
            ),
            Self::Io(detail) => format!("書き込めません: {detail}"),
            Self::Verify(detail) => {
                format!(
                    "書いた結果を読み直して確かめたところ、元の中身と合いませんでした: {detail}"
                )
            }
            Self::NotImplemented => "書き出しはまだ使えません".to_owned(),
        }
    }
}

pub use container::glb_length;

/// 元のGLBと文書の写しから、新しいJSONと末尾に足すBINを作る。`base` は変えない。
pub fn build(base: &[u8], scene: &ExportScene) -> Result<Built, ExportError> {
    patch::build(base, scene)
}

/// 12バイトのヘッダと2つのチャンク（JSONは空白、BINは0で4バイト境界まで詰める）を書き、
/// 書いたバイト数を返す。BINは元の範囲をそのまま書き、続けて `built.appended_bin` を書く。
/// 全体が4GiBを超えるときは、1バイトも書かずに `TooLarge`。
pub fn write_container(
    out: &mut dyn io::Write,
    base: &[u8],
    built: &Built,
) -> Result<u64, ExportError> {
    let base = container::parse(base).map_err(ExportError::InvalidBase)?;
    if !built.appended_bin.len().is_multiple_of(4) {
        return Err(ExportError::InvalidScene(
            "足すBINの長さが4の倍数ではありません".to_owned(),
        ));
    }
    let bin_len = container::bin_chunk_len(base.bin, &built.appended_bin);
    let total = glb_length(built.json.len() as u64, bin_len)?;
    let json_len = container::align4(built.json.len() as u64);
    let io_error = |error: io::Error| ExportError::Io(error.to_string());
    out.write_all(b"glTF").map_err(io_error)?;
    out.write_all(&2u32.to_le_bytes()).map_err(io_error)?;
    out.write_all(&(total as u32).to_le_bytes())
        .map_err(io_error)?;
    out.write_all(&(json_len as u32).to_le_bytes())
        .map_err(io_error)?;
    out.write_all(b"JSON").map_err(io_error)?;
    out.write_all(&built.json).map_err(io_error)?;
    write_fill(out, b' ', json_len - built.json.len() as u64).map_err(io_error)?;
    if let Some(bin_len) = bin_len {
        out.write_all(&(bin_len as u32).to_le_bytes())
            .map_err(io_error)?;
        out.write_all(b"BIN\0").map_err(io_error)?;
        let original = base.bin.unwrap_or(&[]);
        out.write_all(original).map_err(io_error)?;
        let padded = container::align4(original.len() as u64);
        write_fill(out, 0, padded - original.len() as u64).map_err(io_error)?;
        out.write_all(&built.appended_bin).map_err(io_error)?;
    }
    out.flush().map_err(io_error)?;
    Ok(total)
}

fn write_fill(out: &mut dyn io::Write, byte: u8, count: u64) -> io::Result<()> {
    for _ in 0..count {
        out.write_all(&[byte])?;
    }
    Ok(())
}

/// 書いたバイト列を読み直して照合する。1つでも外れたら `Verify`（呼び出し側は書いたファイルを消す）。
/// 確かめること:
/// 1. 容器の長さ、JSONチャンクが `built.json`、元のBINが同じ位置に同じバイト、足したBINが末尾。
/// 2. 最上位の項目のうち8つ（`nodes`・`scenes`・`scene`・`meshes`・`materials`・`accessors`・
///    `bufferViews`・`buffers`）以外が値として元と等しい。8つも、元からあった要素は、`scene` から
///    決まる書き換えてよい所（変形・名前・子を変えたノード、外したノードの親、使うシーンの根、
///    `buffers[0].byteLength`）を除いて等しい。足した要素は末尾にあり、足したBINの範囲を指す。
/// 3. `load_glb` で開け、部品の数・ファイル内の番号・親・名前・ワールド行列（許容差つき）・
///    描画ごとの形状と色・配置後の三角形数が `scene` と一致する。
pub fn verify(
    written: &[u8],
    base: &[u8],
    scene: &ExportScene,
    built: &Built,
) -> Result<(), ExportError> {
    verify::verify(written, base, scene, built).map(|_| ())
}

/// `verify` と同じ照合をし、通れば、読み直した各部品の親に対する行列を `scene.nodes` の順に返す。
/// 書き出しは行列を位置・回転・大きさに分けて書くので、読み直した値は文書の値と最後の桁で
/// 違うことがある。呼び出し側はこれを文書へ入れ、開き直したときと同じ値にそろえる。
pub fn verify_locals(
    written: &[u8],
    base: &[u8],
    scene: &ExportScene,
    built: &Built,
) -> Result<Vec<Affine3A>, ExportError> {
    let model = verify::verify(written, base, scene, built)?;
    Ok(model.nodes.iter().map(|node| node.local).collect())
}
