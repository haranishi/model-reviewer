//! レビューの保存と復元。GLBそのものは読まない・書かない。

use std::path::{Path, PathBuf};

use model_reviewer_core::doc::Document;
use model_reviewer_core::error::ReviewError;
use model_reviewer_core::identity::{ModelIdentity, ModelRef};
use model_reviewer_core::review::ReviewState;
use model_reviewer_core::review_file::{self, ReviewFile};

use crate::command::{Command, DialogButton, DialogRequest};
use crate::loader;
use crate::ui::ids;

use super::{PendingRestore, Session};

pub(super) enum Restored {
    Absent,
    State(ReviewState),
    Blocked,
}

pub fn save_review(session: &mut Session, path: PathBuf) {
    if session.is_exporting() {
        session.status_text = "書き出しが終わってからレビューを保存してください".to_owned();
        return;
    }
    if session.doc.is_none() {
        fail(
            session,
            "レビューを保存できません",
            "モデルが開いていません。",
        );
        return;
    }
    if session.doc_unsaved() {
        session.dialog = Some(super::export::export_first_dialog());
        session.status_text = "先にGLBを書き出してください".to_owned();
        return;
    }
    if session.is_new_document() {
        refuse_new_review(session);
        return;
    }
    let Some(review) = session.review.clone() else {
        fail(
            session,
            "レビューを保存できません",
            "レビューがありません。",
        );
        return;
    };
    let Some(model_ref) = session.model_ref.clone() else {
        fail(
            session,
            "レビューを保存できません",
            "モデルの場所が分かりません。",
        );
        return;
    };
    let parent = match canonical_parent(&path) {
        Ok(parent) => parent,
        Err(message) => {
            fail(session, "レビューを保存できません", &message);
            return;
        }
    };
    let Some(file_name) = path.file_name() else {
        fail(
            session,
            "レビューを保存できません",
            "保存先のファイル名がありません。",
        );
        return;
    };
    let destination = parent.join(file_name);
    let model_path = PathBuf::from(&model_ref.path);
    // レビューの保存先に、開いたGLBやいまの作業対象のGLBを選ぶと、モデルが消える。
    // 別名のリンク経由も含めて断る（書き出しの `forbidden` と同じ判定）。
    let model_file = |candidate: &Path| {
        crate::exporter::same_file(candidate, &model_path)
            || session
                .opened_files
                .iter()
                .any(|opened| crate::exporter::same_file(candidate, opened))
    };
    if model_file(&path) || model_file(&destination) {
        fail(
            session,
            "レビューを保存できません",
            "モデルのファイルへはレビューを保存できません。別の名前を選んでください。",
        );
        return;
    }
    let model_abs = std::fs::canonicalize(&model_path).unwrap_or(model_path);
    let mut stamped = model_ref;
    stamped.path = model_abs.display().to_string();
    stamped.relative_path = Some(relative_from(&parent, &model_abs));
    let written = match session.doc.as_ref() {
        Some(doc) => review_file::to_json(&review, doc, &stamped),
        None => return,
    };
    let json = match written {
        Ok(json) => json,
        Err(err) => {
            fail(session, "レビューを保存できません", &err.message_ja());
            return;
        }
    };
    if let Err(message) = write_atomic(&destination, json.as_bytes()) {
        fail(session, "レビューを保存できません", &message);
        return;
    }
    session.model_ref = Some(stamped);
    session.mark_review_saved();
    session.export_failed = false;
    session.dialog = None;
    session.status_text = "レビューを保存しました".to_owned();
}

/// 新規の文書（まだ書き出していない）のレビュー保存を断る（要件2.6）。レビューはモデルの
/// ファイルの識別を持つので、書き出すまで保存できない。注釈と視点は残る。
pub(super) fn refuse_new_review(session: &mut Session) {
    fail(
        session,
        "レビューを保存できません",
        super::REVIEW_NEEDS_EXPORT,
    );
}

pub fn relink(session: &mut Session, path: PathBuf) {
    if session.is_exporting() {
        session.status_text = "書き出し中は別のファイルを開けません".to_owned();
        return;
    }
    let Some(pending) = session.restore.as_mut() else {
        fail(
            session,
            "モデルを指定できません",
            "先にレビューを開いてください。",
        );
        return;
    };
    let expected = identity_of(&pending.file);
    pending.remaining.clear();
    pending.first_mismatch = None;
    session.start_model_load_for_restore(path, expected);
}

pub fn load_review(session: &mut Session, path: PathBuf) {
    if session.is_exporting() {
        session.end_discard_bypass();
        session.status_text = "書き出し中は別のファイルを開けません".to_owned();
        return;
    }
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            log::error!("failed to read review: {err}");
            session.restore = None;
            session.fail_load(
                "レビューを読めません",
                format!(
                    "レビューを読めませんでした（{}）",
                    loader::io_error_ja(&err)
                ),
            );
            return;
        }
    };
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(_) => {
            session.restore = None;
            session.fail_load(
                "レビューを読めません",
                "レビューを読めませんでした（文字コードがUTF-8ではありません）".to_owned(),
            );
            return;
        }
    };
    let file = match review_file::parse(&text) {
        Ok(file) => file,
        Err(err) => {
            session.restore = None;
            session.fail_load("レビューを読めません", err.message_ja());
            return;
        }
    };
    let expected = identity_of(&file);
    // STL は同じファイルでも単位の仮定が違えば部品の大きさが違う。レビューの単位（無ければ mm）と
    // 開いている単位が違うときは、その場で当てはめず、レビューの単位で開き直す（要件8.5）。
    let same_unit = match session.source_kind {
        super::SourceKind::Glb => true,
        // 新規の文書には識別が無く、レビューが指すモデルではない（開き直す）。
        super::SourceKind::New => false,
        super::SourceKind::Stl { unit } => {
            file.model
                .stl_unit
                .unwrap_or(model_reviewer_core::units::UnitLabel::Mm)
                == unit
        }
    };
    let same_loaded = session.model.is_some()
        && same_unit
        && session
            .model_ref
            .as_ref()
            .is_some_and(|model_ref| model_ref.identity == expected);
    if same_loaded {
        if !session.exported_since_load || !session.doc_unsaved() {
            apply_in_place(session, &file);
            return;
        }
        // 書き出し後にさらに形を編集した状態で、書き出したファイル向けのレビューを読み込む場合。
        // `session.model` は書き出し前の古い土台なので、書き出したファイルを開き直して復元する。
        let mut existing = Vec::new();
        if let Some(current) = session
            .model_ref
            .as_ref()
            .map(|model_ref| PathBuf::from(&model_ref.path))
            .filter(|candidate| candidate.is_file())
        {
            existing.push(current);
        }
        for candidate in candidate_paths(&path, &file) {
            if candidate.is_file()
                && !existing
                    .iter()
                    .any(|seen| crate::exporter::same_file(seen, &candidate))
            {
                existing.push(candidate);
            }
        }
        if !existing.is_empty() {
            let first = existing.remove(0);
            session.restore = Some(PendingRestore {
                file,
                remaining: existing,
                first_mismatch: None,
            });
            session.start_model_load_for_exported_restore(first, expected);
            return;
        }
    }
    let searched = candidate_paths(&path, &file);
    let mut existing: Vec<PathBuf> = searched
        .iter()
        .filter(|candidate| candidate.is_file())
        .cloned()
        .collect();
    if existing.is_empty() {
        session.cancel_pending_load();
        session.restore = Some(PendingRestore {
            file,
            remaining: Vec::new(),
            first_mismatch: None,
        });
        session.end_discard_bypass();
        session.load = crate::loader::LoadPhase::Idle;
        session.dialog = Some(missing_dialog(&searched));
        session.status_text = "モデルが見つかりません".to_owned();
        return;
    }
    let first = existing.remove(0);
    session.restore = Some(PendingRestore {
        file,
        remaining: existing,
        first_mismatch: None,
    });
    session.start_model_load_for_restore(first, expected);
}

/// 候補の識別が一致しなかったとき、次の候補があれば別スレッドで確かめる。
pub(super) fn on_candidate_mismatch(session: &mut Session, actual: ModelIdentity) {
    let Some(pending) = session.restore.as_mut() else {
        session.loading_file_name = None;
        session.open_started = None;
        session.awaiting_present = false;
        session.end_discard_bypass();
        session.load = crate::loader::LoadPhase::Idle;
        return;
    };
    if pending.first_mismatch.is_none() {
        pending.first_mismatch = Some(actual);
    }
    let expected = identity_of(&pending.file);
    while !pending.remaining.is_empty() {
        let next = pending.remaining.remove(0);
        if next.is_file() {
            session.start_model_load_for_restore(next, expected);
            return;
        }
    }
    let mismatch = pending
        .first_mismatch
        .clone()
        .unwrap_or_else(|| ModelIdentity::from_bytes(&[]));
    let dialog = mismatch_dialog(&pending.file, &mismatch);
    session.loading_file_name = None;
    session.open_started = None;
    session.awaiting_present = false;
    session.end_discard_bypass();
    session.load = crate::loader::LoadPhase::Idle;
    session.dialog = Some(dialog);
    session.status_text = "モデルが一致しません".to_owned();
}

/// 候補の読み取りに失敗したとき、残りの候補や先に見た不一致へ進む。
pub(super) fn try_next_candidate_after_error(session: &mut Session) -> bool {
    let Some(pending) = session.restore.as_mut() else {
        return false;
    };
    let expected = identity_of(&pending.file);
    while !pending.remaining.is_empty() {
        let next = pending.remaining.remove(0);
        if next.is_file() {
            session.start_model_load_for_restore(next, expected);
            return true;
        }
    }
    if let Some(mismatch) = pending.first_mismatch.clone() {
        let dialog = mismatch_dialog(&pending.file, &mismatch);
        session.loading_file_name = None;
        session.open_started = None;
        session.awaiting_present = false;
        session.end_discard_bypass();
        session.load = crate::loader::LoadPhase::Idle;
        session.dialog = Some(dialog);
        session.status_text = "モデルが一致しません".to_owned();
        return true;
    }
    false
}

/// 識別が違うファイルは転送しない。`true` ならこの読み込みは捨てる。
pub(super) fn reject_mismatch(session: &mut Session, model_ref: &ModelRef) -> bool {
    let Some(pending) = session.restore.as_ref() else {
        return false;
    };
    if identity_of(&pending.file) == model_ref.identity {
        return false;
    }
    on_candidate_mismatch(session, model_ref.identity.clone());
    true
}

pub(super) fn take_restored(
    session: &mut Session,
    doc: &Document,
    identity: &ModelIdentity,
) -> Restored {
    let Some(pending) = session.restore.clone() else {
        return Restored::Absent;
    };
    match review_file::restore(&pending.file, doc, identity) {
        Ok(state) => {
            session.restore = None;
            session.dialog = None;
            Restored::State(state)
        }
        Err(ReviewError::ModelMismatch { .. }) => {
            session.dialog = Some(mismatch_dialog(&pending.file, identity));
            session.status_text = "モデルが一致しません".to_owned();
            Restored::Blocked
        }
        Err(err) => {
            session.restore = None;
            session.dialog = Some(stopped_dialog(&err.message_ja()));
            session.status_text = err.message_ja();
            Restored::Blocked
        }
    }
}

fn apply_in_place(session: &mut Session, file: &ReviewFile) {
    session.end_discard_bypass();
    if session.doc.is_none() {
        return;
    }
    let Some(identity) = session
        .model_ref
        .as_ref()
        .map(|model_ref| model_ref.identity.clone())
    else {
        return;
    };
    // レビューを開くと履歴は空になる。書き出し前で未書き出しの編集があれば、元のファイルの形へ
    // 戻した文書に当てはめる（書き出し後の未書き出し編集は `load_review` で開き直しへ回している）。
    let fresh = (!session.exported_since_load && session.doc_unsaved())
        .then_some(session.model.as_ref())
        .flatten()
        .map(|model| {
            let mut doc = model_reviewer_core::doc::Document::new(std::sync::Arc::clone(model));
            // 足した図形の形状はGPUと当たり判定の索引に送ってある。番号をそろえるため、同じ順に
            // 引き継ぐ（部品は戻すが、形状の番号は作業の間ずっと同じ物を指す）。
            if let Some(old) = session.doc.as_ref() {
                let base = old.base().primitives.len() as u32;
                for (index, prim) in old.added_primitives().iter().enumerate() {
                    let id = model_reviewer_core::scene::PrimId(base + index as u32);
                    let prim = std::sync::Arc::clone(prim);
                    // 図形の作り方の記録も引き継ぐ（同じ形状を指す部品の寸法を直せるように）。
                    match old.shape_spec(id) {
                        Some(spec) => doc.add_shape_primitive(prim, spec),
                        None => doc.add_primitive(prim),
                    };
                }
            }
            doc
        });
    let restored = match fresh.as_ref().or(session.doc.as_ref()) {
        Some(doc) => review_file::restore(file, doc, &identity),
        None => return,
    };
    match restored {
        Ok(state) => {
            session.cancel_pending_load();
            super::notes::clear_body_edits(session);
            super::notes::cancel(session);
            session.body_pending = false;
            session.advance_review_generation();
            session.review = Some(state);
            session.selection = None;
            session.selected_annotation = None;
            session.annotate_mode = false;
            if session.tool == model_reviewer_core::gizmo::Tool::Annotate {
                session.tool = model_reviewer_core::gizmo::Tool::Select;
            }
            if let Some(fresh) = fresh {
                session.doc = Some(fresh);
                session.recount_triangles();
            }
            session.measurement.clear();
            session.reset_history();
            session.export_error = None;
            session.export_failed = false;
            session.discard_bypass = false;
            session.restore = None;
            session.dialog = None;
            session.scene_dirty.instances = true;
            session.scene_dirty.selection = true;
            session.status_text = "レビューを復元しました".to_owned();
        }
        Err(err) => {
            session.restore = None;
            session.dialog = Some(stopped_dialog(&err.message_ja()));
            session.status_text = err.message_ja();
        }
    }
}

fn identity_of(file: &ReviewFile) -> ModelIdentity {
    ModelIdentity {
        size_bytes: file.model.size_bytes,
        blake3_hex: file.model.hash.hex.to_ascii_lowercase(),
    }
}

fn candidate_paths(json_path: &Path, file: &ReviewFile) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(relative) = file
        .model
        .relative_path
        .as_deref()
        .filter(|relative| !relative.is_empty())
        && let Some(parent) = json_path.parent()
    {
        paths.push(parent.join(relative));
    }
    if !file.model.path.is_empty() {
        let absolute = PathBuf::from(&file.model.path);
        let duplicate = paths.first().is_some_and(|first| {
            *first == absolute
                || matches!(
                    (std::fs::canonicalize(first), std::fs::canonicalize(&absolute)),
                    (Ok(left), Ok(right)) if left == right
                )
        });
        if !duplicate {
            paths.push(absolute);
        }
    }
    paths
}

fn canonical_parent(path: &Path) -> Result<PathBuf, String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let parent = parent.unwrap_or_else(|| Path::new("."));
    std::fs::canonicalize(parent).map_err(|err| {
        format!(
            "保存先のフォルダを確認できません（{}）",
            loader::io_error_ja(&err)
        )
    })
}

fn relative_from(from_dir: &Path, target: &Path) -> String {
    let from: Vec<_> = from_dir.components().collect();
    let to: Vec<_> = target.components().collect();
    let mut shared = 0;
    while shared < from.len() && shared < to.len() && from[shared] == to[shared] {
        shared += 1;
    }
    let mut parts = Vec::new();
    for _ in shared..from.len() {
        parts.push("..".to_owned());
    }
    for component in &to[shared..] {
        parts.push(component.as_os_str().to_string_lossy().replace('\\', "/"));
    }
    if parts.is_empty() {
        ".".to_owned()
    } else {
        parts.join("/")
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let Some(name) = path.file_name() else {
        return Err("保存先のファイル名がありません".to_owned());
    };
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(".tmp");
    let tmp = parent.join(tmp_name);
    // 名前を付け替える前に装置へ書き切る。書き切らずに付け替えると、電源が落ちたときに
    // 中身の無いファイルが前のレビューを置き換えることがある。
    if let Err(err) = write_synced(&tmp, bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "レビューを書けませんでした（{}）",
            loader::io_error_ja(&err)
        ));
    }
    if let Err(err) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!(
            "レビューを置き換えられませんでした（{}）",
            loader::io_error_ja(&err)
        ));
    }
    Ok(())
}

fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn prefix(hex: &str) -> &str {
    let end = hex.len().min(16);
    &hex[..end]
}

fn mismatch_dialog(file: &ReviewFile, actual: &ModelIdentity) -> DialogRequest {
    let expected = identity_of(file);
    DialogRequest {
        title: "モデルが一致しません".to_owned(),
        body: format!(
            "別のGLBです。保存時は {} バイト、ハッシュ先頭 {}。今のファイルは {} バイト、ハッシュ先頭 {}。別のGLBを指定するか、やめてください。",
            expected.size_bytes,
            prefix(&expected.blake3_hex),
            actual.size_bytes,
            prefix(&actual.blake3_hex)
        ),
        buttons: vec![
            DialogButton {
                id: ids::DIALOG_CONFIRM.to_owned(),
                label: "別のGLBを指定".to_owned(),
                command: Command::RelinkModelDialog,
            },
            DialogButton {
                id: ids::DIALOG_CANCEL.to_owned(),
                label: "やめる".to_owned(),
                command: Command::CancelRestore,
            },
        ],
    }
}

fn missing_dialog(searched: &[PathBuf]) -> DialogRequest {
    let places = if searched.is_empty() {
        "（レビューにパスがありません）".to_owned()
    } else {
        searched
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    };
    DialogRequest {
        title: "モデルが見つかりません".to_owned(),
        body: format!("探した場所:\n{places}\nモデルを指定してください。"),
        buttons: vec![
            DialogButton {
                id: ids::DIALOG_CONFIRM.to_owned(),
                label: "モデルを指定…".to_owned(),
                command: Command::RelinkModelDialog,
            },
            DialogButton {
                id: ids::DIALOG_CANCEL.to_owned(),
                label: "やめる".to_owned(),
                command: Command::CancelRestore,
            },
        ],
    }
}

fn stopped_dialog(body: &str) -> DialogRequest {
    DialogRequest {
        title: "レビューを復元できません".to_owned(),
        body: format!("{body}\n何も復元していません。"),
        buttons: vec![DialogButton {
            id: ids::DIALOG_CANCEL.to_owned(),
            label: "閉じる".to_owned(),
            command: Command::DismissDialog,
        }],
    }
}

fn fail(session: &mut Session, title: &str, body: &str) {
    session.dialog = Some(DialogRequest {
        title: title.to_owned(),
        body: body.to_owned(),
        buttons: vec![DialogButton {
            id: ids::DIALOG_CANCEL.to_owned(),
            label: "閉じる".to_owned(),
            command: Command::DismissDialog,
        }],
    });
    session.status_text = title.to_owned();
}
