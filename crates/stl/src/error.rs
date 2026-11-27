//! STL を読めないときの理由。

/// STL を読めない理由。`message_ja` が利用者に出す文。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StlError {
    #[error("not stl")]
    NotStl,
    #[error("too short")]
    TooShort,
    #[error("count mismatch: claimed {claimed}, fits {fits}")]
    CountMismatch { claimed: u64, fits: u64 },
    #[error("bad number in triangle {triangle}")]
    BadNumber { triangle: u64 },
    #[error("syntax error at line {line}")]
    Syntax { line: u64 },
    #[error("truncated")]
    Truncated,
    #[error("no triangles")]
    NoTriangles,
    #[error("too large")]
    TooLarge,
}

impl StlError {
    /// 利用者に出す文。
    pub fn message_ja(&self) -> String {
        match self {
            Self::NotStl => "STL ではありません".to_owned(),
            Self::TooShort => "STL として短すぎます".to_owned(),
            Self::CountMismatch { claimed, fits } => format!(
                "三角形の数が {claimed} 個と書かれていますが、ファイルには {fits} 個分しかありません"
            ),
            Self::BadNumber { triangle } => {
                format!("{triangle} 番目の三角形に、読めないか大きすぎる座標があります")
            }
            Self::Syntax { line } => format!("{line} 行目の書き方が STL の決まりと合いません"),
            Self::Truncated => "STL が途中で終わっています".to_owned(),
            Self::NoTriangles => "STL に三角形がありません".to_owned(),
            Self::TooLarge => "STL が大きすぎて読み込めません".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_error_has_a_japanese_message() {
        let all = [
            StlError::NotStl,
            StlError::TooShort,
            StlError::CountMismatch {
                claimed: 10,
                fits: 2,
            },
            StlError::BadNumber { triangle: 3 },
            StlError::Syntax { line: 7 },
            StlError::Truncated,
            StlError::NoTriangles,
            StlError::TooLarge,
        ];
        for error in all {
            assert!(error.message_ja().contains("STL") || error.message_ja().contains("三角形"));
        }
    }
}
