//! 土台の合成。部品1つ・描画1つ・形状1つの `Model`（STL から読んだもの）から、最小の GLB を作る。
//! 書き出しは、この GLB を土台にして第2段階の `build`・`write_container`・照合へ渡す。

use model_reviewer_core::scene::Model;

use super::ExportError;

/// 部品1つ・描画1つ・形状1つの `Model` から、最小の GLB を作る。同じ入力からは同じバイト列になる。
/// 形が違えば `InvalidScene`、4GiB を超えれば `TooLarge`。
pub fn synthesize_base(model: &Model) -> Result<Vec<u8>, ExportError> {
    // 契約の先置き。中身は後の作業で書く。
    let _ = model;
    Err(ExportError::NotImplemented)
}
