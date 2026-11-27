//! 計測の点の吸着（`core::snap` をつなぐ）。
//!
//! 約束: 探索は、計測の道具でポインタが動いたフレーム（`ViewHover`）とクリックだけで行う。
//! 結果は `Session::snap_preview` に置く。クリックは控えを使わず、その場で探す。吸着の入・切は
//! 履歴にも未保存の印にも入らない。

use glam::Vec2;

use super::Session;

/// ポインタが3Dビューの上を動いた（`Some`）か、出た（`None`）。
pub(super) fn hover(session: &mut Session, ndc: Option<Vec2>) {
    // 契約の先置き。中身は接合の作業で書く。
    let _ = (session, ndc);
}

/// 吸着を入れるか。
pub(super) fn set_snap(session: &mut Session, on: bool) {
    // 契約の先置き。中身は接合の作業で書く。
    let _ = (session, on);
}

/// 吸着の入・切。
pub(super) fn toggle_snap(session: &mut Session) {
    // 契約の先置き。中身は接合の作業で書く。
    let _ = session;
}
