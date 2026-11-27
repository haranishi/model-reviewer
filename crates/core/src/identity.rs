//! モデルの識別。ファイル全体の blake3 とバイト数。

use std::path::PathBuf;

use crate::units::{UnitLabel, UpAxis};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelIdentity {
    pub size_bytes: u64,
    pub blake3_hex: String,
}

impl ModelIdentity {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            size_bytes: bytes.len() as u64,
            blake3_hex: blake3::hash(bytes).to_hex().to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelRef {
    pub file_name: String,
    /// 読み直しと同一ファイルの照合に使う実パス。
    pub path: PathBuf,
    pub relative_path: Option<String>,
    pub identity: ModelIdentity,
    /// STL を開いたときの単位の仮定。GLB では `None`（レビューJSONに書かない）。
    pub stl_unit: Option<UnitLabel>,
    /// STL を開いたときに上と仮定した軸。
    pub stl_up: Option<UpAxis>,
}
