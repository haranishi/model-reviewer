//! ショートカットの設定の画面の状態と、割り当てを変える `Command`。どれも履歴と未保存の印に入らない。
//!
//! いまは形だけを置いている（WP-J1 が中身を書く）。`apply` から呼ぶ関数は、どれも何もしない。

use crate::keymap::{Action, Chord, Refusal};

use super::Session;

/// 設定の画面の状態（`Session.keys`）。検索の欄の文字は画面の部品の持ち物で、ここには置かない。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeysState {
    pub open: bool,
    /// 取り込み中の行。同時に1つ。
    pub capture: Option<Action>,
    /// 重なりの確認を待っている。
    pub conflict: Option<KeyConflict>,
    /// 直前に断った理由（その行の下に出す）。
    pub refusal: Option<(Action, Refusal)>,
    /// 「すべて初期値に戻す」の1回目を押した。
    pub reset_all_armed: bool,
}

/// 取り込んだ組が、ほかの操作（`holder`）で使われている。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyConflict {
    pub action: Action,
    pub chord: Chord,
    pub holder: Action,
}

pub(super) fn set_open(session: &mut Session, open: bool) {
    let _ = (session, open);
}

pub(super) fn begin_capture(session: &mut Session, action: Action) {
    let _ = (session, action);
}

pub(super) fn cancel_capture(session: &mut Session) {
    let _ = session;
}

pub(super) fn capture(session: &mut Session, key: Result<Chord, Refusal>) {
    let _ = (session, key);
}

pub(super) fn resolve_conflict(session: &mut Session, replace: bool) {
    let _ = (session, replace);
}

pub(super) fn clear(session: &mut Session, action: Action) {
    let _ = (session, action);
}

pub(super) fn reset(session: &mut Session, action: Action) {
    let _ = (session, action);
}

pub(super) fn arm_reset_all(session: &mut Session, armed: bool) {
    let _ = (session, armed);
}

pub(super) fn reset_all(session: &mut Session) {
    let _ = session;
}
