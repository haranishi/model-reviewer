//! 読み込みエラー。表示文はここだけが作る。

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoadError {
    #[error("not a glb")]
    NotGlb,
    #[error("container: {0}")]
    Container(String),
    #[error("json: {0}")]
    Json(String),
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("unsupported required extension {name}")]
    UnsupportedRequiredExtension { name: String, kind: ExtensionKind },
    #[error("external buffer")]
    ExternalBuffer,
    #[error("missing bin")]
    MissingBin,
    #[error("bad data: {what}")]
    BadData { what: String },
    #[error("unsupported sparse accessor")]
    UnsupportedSparse,
    #[error("node graph: {0}")]
    NodeGraph(String),
    #[error("no mesh")]
    NoMesh,
    #[error("too large: {what}")]
    TooLarge { what: String },
    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionKind {
    GeometryCompression,
    Quantization,
    Instancing,
    Unknown,
}

impl LoadError {
    pub fn message_ja(&self) -> String {
        match self {
            Self::NotGlb => "GLBではありません".to_owned(),
            Self::Container(detail) => format!("容器を読めません: {detail}"),
            Self::Json(detail) => format!("JSONを読めません: {detail}"),
            Self::Invalid(detail) => format!("データが不正です: {detail}"),
            Self::UnsupportedRequiredExtension { name, kind } => {
                let why = match kind {
                    ExtensionKind::GeometryCompression => "形状が圧縮されているため未対応",
                    ExtensionKind::Quantization => "量子化された頂点形式は未対応",
                    ExtensionKind::Instancing => "拡張によるインスタンス配置は未対応",
                    ExtensionKind::Unknown => "未知の必須拡張",
                };
                format!("必須拡張 {name} は未対応です（{why}）")
            }
            Self::ExternalBuffer => "外部バッファは未対応です".to_owned(),
            Self::MissingBin => "BINチャンクがありません".to_owned(),
            Self::BadData { what } => format!("データが破損しています（{what}）"),
            Self::UnsupportedSparse => "未対応の形式です（sparse アクセサ）".to_owned(),
            Self::NodeGraph(detail) => format!("ノード階層が不正です: {detail}"),
            Self::NoMesh => "表示できる形状がありません".to_owned(),
            Self::TooLarge { what } => format!("大きすぎます（{what}）"),
            Self::Internal(detail) => format!("内部エラー: {detail}"),
        }
    }
}
