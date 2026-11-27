//! レビューJSONのエラー。表示文はここだけが作る。

use crate::identity::ModelIdentity;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReviewError {
    #[error("json: {0}")]
    Json(String),
    #[error("wrong format")]
    WrongFormat,
    #[error("newer version {found}, supported {supported}")]
    NewerVersion { found: u32, supported: u32 },
    #[error("invalid value: {0}")]
    InvalidValue(String),
    #[error("model mismatch")]
    ModelMismatch {
        expected: ModelIdentity,
        actual: ModelIdentity,
    },
    #[error("unknown node {gltf_index}")]
    UnknownNode { gltf_index: u32 },
}

impl ReviewError {
    pub fn message_ja(&self) -> String {
        match self {
            Self::Json(detail) => format!("JSONを読めません: {detail}"),
            Self::WrongFormat => "レビューJSONではありません".to_owned(),
            Self::NewerVersion { found, supported } => {
                format!("レビューの版 {found} は未対応です（対応は {supported} まで）")
            }
            Self::InvalidValue(detail) => format!("値が不正です: {detail}"),
            Self::ModelMismatch { expected, actual } => format!(
                "モデルが一致しません。保存時 {} バイト、今 {} バイト",
                expected.size_bytes, actual.size_bytes
            ),
            Self::UnknownNode { gltf_index } => {
                format!("モデルに無い部品です（gltf_index {gltf_index}）")
            }
        }
    }
}
