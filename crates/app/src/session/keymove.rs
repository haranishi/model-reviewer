//! キーで動かす: 向きの固定と数値の入力、矢印で少しずつ（要件11・12）。変形の仕組みは増やさず、
//! いまの変形（差分の行列 → 確定）の3つ目の出どころ（`TransformSource::Keys`）になる。
//!
//! いまは形だけを置いている（WP-K1 が中身を書く）。どの関数も何もしないか、空の値を返す。

use model_reviewer_core::gizmo::Handle;
use model_reviewer_core::section::Axis;

use crate::command::KeyInput;

use super::Session;

/// 打った文字を量に読む。空と "-" は `Some(0.0)`。数字・小数点・負号以外は `None`。
pub fn parse_amount(text: &str) -> Option<f64> {
    let _ = text;
    None
}

/// 有効6桁。末尾の 0 と小数点を落とす。
pub fn format_amount(value: f64) -> String {
    let _ = value;
    String::new()
}

/// 矢印の1歩。移動は格子（シーンの単位）、回転は 15・1（度）、拡大は 0.1・0.01。
pub fn step_of(handle: Handle, grid: f32, fine: bool) -> f64 {
    let _ = (handle, grid, fine);
    0.0
}

/// `Command::BeginKeyTransform`。
pub(super) fn begin(session: &mut Session, axis: Axis) {
    let _ = (session, axis);
}

/// `Command::KeyTransformInput`。
pub(super) fn input(session: &mut Session, key: KeyInput) {
    let _ = (session, key);
}

/// `Command::Nudge`。
pub(super) fn nudge(session: &mut Session, axis: Axis, sign: i8, fine: bool) {
    let _ = (session, axis, sign, fine);
}
