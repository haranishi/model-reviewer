//! モデルとレビューを開く。

use std::path::PathBuf;

use crate::command::is_review_path;
use crate::loader::{self, LoadOptions};

use super::{Pending, Session};

pub(super) fn open_guarded(session: &mut Session, path: PathBuf, options: LoadOptions) {
    if session.is_exporting() {
        session.status_text = "書き出し中は別のファイルを開けません".to_owned();
        return;
    }
    if session.hold(Pending::OpenModel(path.clone(), options)) {
        return;
    }
    open_model(session, path, options);
}

pub(super) fn review_guarded(session: &mut Session, path: PathBuf) {
    if session.is_exporting() {
        session.status_text = "書き出し中は別のファイルを開けません".to_owned();
        return;
    }
    if session.hold(Pending::LoadReview(path.clone())) {
        return;
    }
    load_review(session, path);
}

pub fn open_model(session: &mut Session, path: PathBuf, options: LoadOptions) {
    if session.is_exporting() {
        session.end_discard_bypass();
        session.status_text = "書き出し中は別のファイルを開けません".to_owned();
        return;
    }
    session.restore = None;
    session.start_model_load(path, options);
}

/// 新規作成（要件1.1・1.3・1.4）。書き出し中・読み込み中は断る。未保存があれば確認を出し、
/// 選ばれるまで今の文書を残す（確認の後は `confirm_discard` が `new_document` へ続ける）。
pub(super) fn new_guarded(session: &mut Session) {
    if refuse_new(session) {
        return;
    }
    if session.hold(Pending::New) {
        return;
    }
    new_document(session);
}

/// 未保存の確認を済ませた後の新規作成。復元の途中と進行中の読み込みを消し、空のモデルを置く。
pub fn new_document(session: &mut Session) {
    if refuse_new(session) {
        session.end_discard_bypass();
        return;
    }
    session.restore = None;
    session.start_new_document();
}

fn refuse_new(session: &mut Session) -> bool {
    let reason = if session.is_exporting() {
        "書き出し中は新規作成できません"
    } else if session.is_loading() {
        "読み込み中は新規作成できません"
    } else {
        return false;
    };
    session.status_text = reason.to_owned();
    true
}

pub fn load_review(session: &mut Session, path: PathBuf) {
    super::persist::load_review(session, path);
}

pub fn dropped(session: &mut Session, paths: Vec<PathBuf>) {
    if session.is_exporting() {
        session.status_text = "書き出し中は別のファイルを開けません".to_owned();
        return;
    }
    let Some(first) = paths.first().cloned() else {
        return;
    };
    if paths.len() > 1 {
        session.notice = "複数のファイルのうち、最初の1つだけを開きます".to_owned();
    } else {
        session.notice.clear();
    }
    if loader::file_starts_with_glb(&first) || !is_review_path(&first) {
        open_guarded(session, first, LoadOptions::default());
    } else {
        review_guarded(session, first);
    }
}

/// 起動引数。拡張子は目安で、先頭4バイトが glTF ならモデルとして開く。
pub(super) fn open_launch(session: &mut Session, path: PathBuf) {
    if is_review_path(&path) && !loader::file_starts_with_glb(&path) {
        review_guarded(session, path);
    } else {
        open_guarded(session, path, LoadOptions::default());
    }
}
