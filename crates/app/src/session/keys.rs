//! ショートカットの設定の画面の状態と、割り当てを変える `Command`。どれも履歴と未保存の印に入らない。

use crate::command::LaunchArgs;
use crate::keymap::store::Store;
use crate::keymap::{Action, Chord, Platform, Refusal};
use crate::ui::ids;

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

impl KeysState {
    pub(super) fn clear_pending(&mut self) {
        *self = Self {
            open: self.open,
            ..Self::default()
        };
    }
}

impl Session {
    /// 実行ファイルの入口が選んだ場所だけを読む。
    pub(crate) fn from_launch(launch: &LaunchArgs) -> Self {
        let mut session = Self {
            scripted: launch.script.is_some(),
            ..Self::default()
        };
        if let Some(path) = &launch.keymap {
            let documents = launch
                .positional
                .iter()
                .chain(&launch.script)
                .cloned()
                .collect();
            let opened = Store::open_for(path.clone(), documents);
            session.key_labels = opened.keymap.labels(Platform::current());
            session.keymap = opened.keymap;
            session.store = opened.store;
            session.notice = opened.notice.unwrap_or_default();
        }
        session
    }

    pub(super) fn protect_document(&mut self, path: &std::path::Path) {
        if let Some(reason) = self.store.protect_document(path) {
            self.notice = reason.to_owned();
        }
    }
}

pub(super) fn set_open(session: &mut Session, open: bool) {
    if open && session.dialog.is_some() {
        return;
    }
    session.keys = KeysState {
        open,
        ..KeysState::default()
    };
    if open && let Some(ctx) = &session.egui_ctx {
        ctx.data_mut(|data| data.insert_temp(egui::Id::unique(ids::HELP_OPEN), false));
    }
}

pub(super) fn begin_capture(session: &mut Session, action: Action) {
    if session.keys.open && session.dialog.is_none() {
        session.keys.clear_pending();
        session.keys.capture = Some(action);
        if let Some(ctx) = &session.egui_ctx {
            crate::ui::surrender_focus(ctx);
        }
    }
}

pub(super) fn cancel_capture(session: &mut Session) {
    session.keys.clear_pending();
}

pub(super) fn capture(session: &mut Session, key: Result<Chord, Refusal>) {
    let Some(action) = session.keys.capture else {
        return;
    };
    match key {
        Ok(chord) => assign(session, action, chord, false),
        Err(reason) => session.keys.refusal = Some((action, reason)),
    }
}

pub(super) fn resolve_conflict(session: &mut Session, replace: bool) {
    if let Some(conflict) = session.keys.conflict {
        if replace {
            assign(session, conflict.action, conflict.chord, true);
        } else {
            session.keys.clear_pending();
        }
    }
}

fn assign(session: &mut Session, action: Action, chord: Chord, replace: bool) {
    session.keys.clear_pending();
    if let Err(holder) = session.keymap.assign(action, chord, replace) {
        session.keys.conflict = Some(KeyConflict {
            action,
            chord,
            holder,
        });
    }
}

pub(super) fn clear(session: &mut Session, action: Action) {
    session.keys.clear_pending();
    session.keymap.clear(action);
}

pub(super) fn reset(session: &mut Session, action: Action) {
    match action.default_chord() {
        Some(chord) => assign(session, action, chord, false),
        None => clear(session, action),
    }
}

pub(super) fn arm_reset_all(session: &mut Session, armed: bool) {
    session.keys.clear_pending();
    session.keys.reset_all_armed = armed;
}

pub(super) fn reset_all(session: &mut Session) {
    if session.keys.reset_all_armed {
        session.keys.clear_pending();
        session.keymap.reset_all();
    }
}

pub(super) fn save_changes(session: &mut Session) {
    session.key_labels = session.keymap.labels(Platform::current());
    if session.store.saves() {
        session.notice = session
            .store
            .save(&session.keymap)
            .err()
            .unwrap_or_default();
    }
}
