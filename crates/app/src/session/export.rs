//! GLBの書き出し。文書から `ExportScene` を作って `crate::exporter` へ渡し、終わったら作業の対象を
//! 新しいファイルへ移す。
//!
//! 約束:
//! - 「開く」で開いたファイルへは書き出さない。
//! - 書き出し中は編集を受け付けず、視点の操作は受け付ける。
//! - 失敗しても文書・履歴・レビューは残る。
//! - 書き出した後は `Document::set_file_indices`・`ModelRef`・`saved_doc_rev` を更新し、
//!   レビューに内容があれば未保存にする。履歴は残す。

use std::path::PathBuf;

use model_reviewer_core::doc::Document;
use model_reviewer_core::identity::ModelRef;
use model_reviewer_core::scene::{NodeId, SceneRead};
use model_reviewer_gltf::export::{ExportNode, ExportNote, ExportScene};

use crate::command::{Command, DialogButton, DialogRequest};
use crate::exporter::{self, ExportDone, ExportEvent, ExportFailure, ExportRequest, ExportStage};
use crate::metrics;
use crate::ui::ids;

use super::{ExportJob, Session, SourceKind, failure_dialog};
use model_reviewer_core::units::UnitLabel;

/// 文書の現在の状態から、書き出しスレッドへ渡す部品の表の写しを作る。
pub fn export_scene(doc: &Document) -> ExportScene {
    let nodes = doc
        .nodes()
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let id = NodeId(index as u32);
            ExportNode {
                parent: node.parent.map(|parent| parent.0),
                name: node.name.as_ref().to_owned(),
                local: node.local,
                file_index: doc.file_index(id),
                template: node.template,
                draws: doc.draws(id).to_vec(),
            }
        })
        .collect();
    ExportScene {
        nodes,
        added: doc.added_primitives().to_vec(),
        base_primitive_count: doc.base().primitives.len() as u32,
    }
}

/// 書き出しの土台の種類。STL なら、開いたときの単位と部品の名前で読み直して合成する。
fn export_base(session: &Session) -> exporter::ExportBase {
    match session.source_kind {
        SourceKind::Glb => exporter::ExportBase::File,
        SourceKind::Stl { unit } => exporter::ExportBase::Stl {
            unit,
            name: session
                .model
                .as_ref()
                .and_then(|model| model.nodes.first())
                .map(|node| node.name.clone())
                .unwrap_or_default(),
        },
    }
}

/// STL から書き出したときに結果の一覧へ足す1行（要件8.2）。
pub fn stl_export_note(unit: UnitLabel) -> String {
    format!(
        "STL から GLB を作りました。座標の値は元のまま（{}）で、部品の大きさ {} でメートルに合わせています",
        unit.suffix(),
        unit.meters()
    )
}

pub(super) fn export_glb(session: &mut Session, path: PathBuf) {
    if session.is_exporting() {
        session.status_text = "すでに書き出し中です".to_owned();
        return;
    }
    if session.is_loading_replacement() {
        session.block_edit_if_locked();
        return;
    }
    let (Some(doc), Some(model_ref)) = (session.doc.as_ref(), session.model_ref.as_ref()) else {
        let message = "モデルが開いていません。".to_owned();
        session.export_error = Some(message.clone());
        session.export_failed = true;
        session.status_text = "書き出せません".to_owned();
        session.dialog = Some(failure_dialog("書き出せません", message));
        return;
    };
    let request = ExportRequest {
        base_path: PathBuf::from(&model_ref.path),
        base_identity: model_ref.identity.clone(),
        target: path,
        scene: export_scene(doc),
        forbidden: session.opened_files.clone(),
        base_kind: export_base(session),
    };
    session.export_error = None;
    session.export_failed = false;
    session.dialog = None;
    session.status_text.clear();
    let handle = exporter::start_with_hooks(
        request,
        session.export_hooks.clone(),
        session.egui_ctx.clone(),
    );
    session.export_job = Some(ExportJob {
        generation: session.shown_generation,
        doc_rev: session.history.doc_rev(),
        handle,
        stage: ExportStage::Reading,
        peak_mem: metrics::sample_memory(),
    });
}

/// 書き出しスレッドからの知らせを取り込む。進みがあれば `true`。
pub(super) fn poll(session: &mut Session) -> bool {
    let events = match session.export_job.as_ref() {
        Some(job) => job.handle.drain(),
        None => return false,
    };
    if events.is_empty() {
        return false;
    }
    for event in events {
        apply_event(session, event);
    }
    true
}

fn apply_event(session: &mut Session, event: ExportEvent) {
    match event {
        ExportEvent::Stage(stage) => {
            if let Some(job) = session.export_job.as_mut() {
                job.stage = stage;
            }
        }
        ExportEvent::Done(done) => finish_export(session, *done),
        ExportEvent::Failed(failure) => fail_export(session, failure),
    }
}

fn finish_export(session: &mut Session, done: ExportDone) {
    let Some(job) = session.export_job.take() else {
        return;
    };
    if job.generation != session.shown_generation || job.doc_rev != session.history.doc_rev() {
        // 書き出し中は編集と「開く」を断っているので、ここへは来ない見込み。来たときは、
        // 書いたファイルと文書が食い違うので作業の対象は切り替えず、未書き出しのまま残す。
        log::warn!("export finished for a different document state; target not switched");
        session.status_text = format!(
            "{} へ書き出しましたが、その間に文書が変わったため作業の対象は切り替えていません",
            done.path.display()
        );
        return;
    }
    let Some(doc) = session.doc.as_mut() else {
        return;
    };
    doc.set_file_indices(&done.assigned);
    // 書いたファイルを読み直した行列へそろえる（開き直したときと同じ範囲・初期視点になる）。
    // 最後の桁だけの違いなので、版は進めず、インスタンスだけ作り直す。
    if doc.adopt_file_locals(&done.locals) {
        session.scene_dirty.instances = true;
        session.pending_patches.clear();
    }

    let canonical = std::fs::canonicalize(&done.path).unwrap_or_else(|_| done.path.clone());
    let file_name = canonical
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "モデル".to_owned());
    session.model_ref = Some(ModelRef {
        file_name: file_name.clone(),
        path: canonical.display().to_string(),
        relative_path: None,
        identity: done.identity,
        stl_unit: None,
    });
    session.exported_since_load = true;
    session.export_error = None;
    session.export_failed = false;

    session.metrics.export_read = Some(done.times.read);
    session.metrics.export_build = Some(done.times.build);
    session.metrics.export_write = Some(done.times.write);
    session.metrics.export_verify = Some(done.times.verify);
    session.metrics.export_peak_mem = match (done.peak_mem, job.peak_mem) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };

    // STL から書き出すと、次からは新しい GLB が土台になる。シーンは最初からメートルなので、
    // 単位（前提と表示）は変えない（数値が動かない）。
    let mut lines: Vec<String> = done.notes.iter().map(note_ja).collect();
    if let SourceKind::Stl { unit } = session.source_kind {
        lines.push(stl_export_note(unit));
        session.source_kind = SourceKind::Glb;
    }
    session.mark_doc_exported();
    session.status_text = format!("書き出しました（作業の対象を {file_name} へ切り替えました）");
    if lines.is_empty() {
        session.dialog = None;
    } else {
        session.dialog = Some(result_dialog(&file_name, &lines));
    }
}

fn fail_export(session: &mut Session, failure: ExportFailure) {
    session.export_job = None;
    let message = failure.message_ja();
    log::error!("export failed: {failure:?}");
    session.export_error = Some(message.clone());
    session.export_failed = true;
    session.status_text = "書き出しに失敗しました".to_owned();
    session.dialog = Some(failure_dialog("書き出せません", message));
}

/// 書き出しの結果の注意1件を利用者向けの文にする。
pub fn note_ja(note: &ExportNote) -> String {
    match *note {
        ExportNote::DetachedNodes { count } => {
            format!(
                "削除した部品 {count} 個はシーンから外しました。形状のデータはファイルに残っています。"
            )
        }
        ExportNote::AnimatedNodesChanged { count } => {
            format!(
                "アニメーションの対象になっている部品を {count} 個変更しました。再生するソフトではアニメーションの値が優先されます。"
            )
        }
        ExportNote::SkinnedNodesChanged { count } => {
            format!("スキンの付いた部品を {count} 個変更しました。仕様上、表示には使われません。")
        }
        ExportNote::MatrixWritten { count } => {
            format!("傾いた拡大を含む部品 {count} 個は行列（matrix）で書き出しました。")
        }
        ExportNote::SceneAdded => {
            "シーンが無いファイルだったため、根の部品を並べたシーンを1つ追加しました。".to_owned()
        }
    }
}

fn result_dialog(file_name: &str, notes: &[String]) -> DialogRequest {
    let lines: Vec<String> = notes.iter().map(|note| format!("・{note}")).collect();
    DialogRequest {
        title: "書き出しました".to_owned(),
        body: format!(
            "{file_name} へ書き出しました。以後の作業の対象をこのファイルへ切り替えました。\n\n注意:\n{}",
            lines.join("\n")
        ),
        buttons: vec![DialogButton {
            id: ids::DIALOG_CANCEL.to_owned(),
            label: "閉じる".to_owned(),
            command: Command::DismissDialog,
        }],
    }
}

/// 書き出していない形の編集がある状態で「レビュー保存」が押されたときのダイアログ（要件12.2）。
pub(super) fn export_first_dialog() -> DialogRequest {
    DialogRequest {
        title: "先にGLBを書き出してください".to_owned(),
        body: "書き出していない形の編集があるため、このままではレビューを保存できません。\n先にGLBを書き出してから、レビューを保存してください。"
            .to_owned(),
        buttons: vec![
            DialogButton {
                id: ids::DIALOG_CONFIRM.to_owned(),
                label: "GLBを書き出す…".to_owned(),
                command: Command::ExportGlbDialog,
            },
            DialogButton {
                id: ids::DIALOG_CANCEL.to_owned(),
                label: "やめる".to_owned(),
                command: Command::DismissDialog,
            },
        ],
    }
}
