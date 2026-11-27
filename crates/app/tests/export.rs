//! GLB書き出しと未保存の2分割・失敗と中断の扱い。GPUは使わない。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use glam::{Affine3A, Vec2, Vec3};
use model_reviewer::automation::{Automation, WidgetFrame};
use model_reviewer::command::{Command, Handle, Projection, Tool, Trs, ViewDir};
use model_reviewer::exporter::{ExportHooks, ExportStage, same_file};
use model_reviewer::loader::LoadPhase;
use model_reviewer::metrics;
use model_reviewer::session::{Draft, Session};
use model_reviewer::ui::{self, ids};
use model_reviewer_core::gizmo::decompose;
use model_reviewer_core::identity::ModelIdentity;
use model_reviewer_core::scene::{NodeId, SceneRead};
use model_reviewer_testdata::{inputs, rich, rich_glb, sample_glb};

const ARM: NodeId = NodeId(9);
const BASE: NodeId = NodeId(1);

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("export-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_bytes(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, bytes).unwrap();
    path
}

fn pump_load(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let _ = session.poll_loader();
        let settled = matches!(session.load, LoadPhase::Idle | LoadPhase::Failed(_));
        let pick_ready = session.model.is_none() || session.pick_index.is_some();
        if settled && pick_ready && session.loading_file_name.is_none() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "読み込みが終わりません status={} phase={:?}",
            session.status_text,
            session.load.label()
        );
        thread::sleep(Duration::from_millis(2));
    }
}

fn pump_export(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.is_exporting() {
        let _ = session.poll_exporter();
        assert!(
            Instant::now() < deadline,
            "書き出しが終わりません stage={:?}",
            session.export_stage()
        );
        thread::sleep(Duration::from_millis(2));
    }
}

fn open_glb(dir: &Path, name: &str, bytes: &[u8]) -> (Session, PathBuf) {
    let path = write_bytes(dir, name, bytes);
    let mut session = Session::default();
    session.apply(Command::OpenModel(path.clone()));
    pump_load(&mut session);
    assert_eq!(session.status_text, "読み込み完了");
    (session, path)
}

fn move_part(session: &mut Session, id: NodeId, delta: Vec3) -> Affine3A {
    session.apply(Command::Select(Some(id)));
    let before = session.doc.as_ref().unwrap().node(id).unwrap().local;
    let mut trs = decompose(&before).expect("分けられる");
    trs.translation += delta;
    session.apply(Command::CommitLocal(trs));
    let after = session.doc.as_ref().unwrap().node(id).unwrap().local;
    assert_ne!(after, before);
    after
}

fn local_close(a: Affine3A, b: Affine3A) -> bool {
    a.to_cols_array()
        .iter()
        .zip(b.to_cols_array())
        .all(|(x, y)| (x - y).abs() < 1.0e-4)
}

fn has_tmp_files(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
}

fn ui_harness(session: Session) -> Harness<'static, Session> {
    Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_max_steps(40)
        .with_size(egui::vec2(1100.0, 720.0))
        .with_context_setup(|ctx| {
            model_reviewer::fonts::install(ctx);
            model_reviewer::theme::install(ctx);
        })
        .build_ui_state(
            |ui, session| {
                let mut commands = Vec::new();
                let pixels = ui::show(ui, session, &mut commands);
                for command in commands {
                    session.apply(command);
                }
                session.set_viewport(pixels);
            },
            session,
        )
}

fn widget_rect(harness: &Harness<Session>, name: &str) -> egui::Rect {
    let frame = harness
        .ctx
        .data(|data| data.get_temp::<WidgetFrame>(egui::Id::NULL))
        .unwrap_or_default();
    let entry = frame
        .entries()
        .iter()
        .find(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("部品が無い: {name}"));
    egui::Rect::from_min_max(
        egui::pos2(entry.min[0], entry.min[1]),
        egui::pos2(entry.max[0], entry.max[1]),
    )
}

struct Hold {
    release: Arc<AtomicBool>,
}

impl Drop for Hold {
    fn drop(&mut self) {
        self.release.store(true, Ordering::Release);
    }
}

#[test]
fn export_switches_target_saves_review_and_reopens_matching() {
    let dir = scratch("roundtrip");
    let original = sample_glb();
    let original_id = ModelIdentity::from_bytes(&original);
    let (mut session, opened_path) = open_glb(&dir, "sample.glb", &original);

    // 元のファイル用のレビューを1つ保存しておく（後で別モデル不一致を確かめる）。
    session.apply(Command::SetVisible {
        node: BASE,
        visible: false,
    });
    let old_review = dir.join("old-review.json");
    session.apply(Command::SaveReview(old_review.clone()));
    assert_eq!(session.status_text, "レビューを保存しました");
    let old_review_bytes = std::fs::read(&old_review).unwrap();
    assert!(!session.has_unsaved());

    // 部品を動かし、注釈と平行投影の視点を足す。
    let moved_local = move_part(&mut session, ARM, Vec3::new(0.4, 0.25, -0.15));
    session.draft = Some(Draft {
        node: ARM,
        local_pos: Vec3::new(0.0, 0.05, 0.0),
        local_normal: Vec3::Y,
        text: String::new(),
    });
    session.apply(Command::CommitDraft {
        text: "動かした腕".to_owned(),
    });
    session.apply(Command::SetProjection(Projection::Orthographic));
    session.apply(Command::SaveView {
        name: "平行".to_owned(),
    });
    assert!(session.doc_unsaved());
    assert!(session.review_unsaved());

    // 書き出し。
    let exported = dir.join("edited.glb");
    session.apply(Command::ExportGlb(exported.clone()));
    pump_export(&mut session);

    assert!(exported.is_file());
    assert!(!has_tmp_files(&dir));
    assert_eq!(std::fs::read(&opened_path).unwrap(), original);
    assert_eq!(std::fs::read(&old_review).unwrap(), old_review_bytes);
    assert!(session.dialog.is_none(), "注意が無いのでダイアログなし");
    assert!(
        session.status_text.contains("edited.glb"),
        "{}",
        session.status_text
    );
    assert!(!session.doc_unsaved());
    assert!(session.review_unsaved(), "作業中のレビューは未保存");
    assert!(session.history.can_undo(), "履歴は残る");

    let new_ref = session.model_ref.as_ref().unwrap().clone();
    assert_eq!(new_ref.file_name, "edited.glb");
    assert_ne!(new_ref.identity, original_id);
    assert_eq!(
        new_ref.identity,
        ModelIdentity::from_bytes(&std::fs::read(&exported).unwrap())
    );

    // 工程ごとの時間が性能値に出る。
    assert!(session.metrics.export_read.is_some());
    assert!(session.metrics.export_build.is_some());
    assert!(session.metrics.export_write.is_some());
    assert!(session.metrics.export_verify.is_some());
    let lines = metrics::report_lines(&session.metrics, None);
    let export_line = lines.iter().find(|line| line.label == "書き出し").unwrap();
    assert!(
        export_line.value.contains("読み取り"),
        "{}",
        export_line.value
    );
    assert!(
        !export_line.value.contains("未計測"),
        "{}",
        export_line.value
    );
    let value = metrics::report_value(&session.metrics);
    assert!(value["export_read_ms"]["value"].is_number());
    assert!(value["export_build_ms"]["value"].is_number());
    assert!(value["export_write_ms"]["value"].is_number());
    assert!(value["export_verify_ms"]["value"].is_number());

    // 新しいファイル向けにレビューを保存。
    let new_review = dir.join("edited-review.json");
    session.apply(Command::SaveReview(new_review.clone()));
    assert_eq!(session.status_text, "レビューを保存しました");
    assert!(!session.doc_unsaved());
    assert!(!session.review_unsaved());
    assert!(!session.has_unsaved());

    // 新しい Session でレビューを開き直し、書き出したGLBと一致することを確かめる。
    let mut reopened = Session::default();
    reopened.apply(Command::LoadReview(new_review));
    pump_load(&mut reopened);
    assert_eq!(reopened.status_text, "レビューを復元しました");
    assert_eq!(
        reopened.model_ref.as_ref().unwrap().identity,
        new_ref.identity
    );
    let reopened_local = reopened.doc.as_ref().unwrap().node(ARM).unwrap().local;
    assert!(
        local_close(reopened_local, moved_local),
        "reopened={reopened_local:?} expected={moved_local:?}"
    );
    let review = reopened.review.as_ref().unwrap();
    assert!(!review.visibility.own(BASE));
    assert_eq!(review.annotations.len(), 1);
    assert_eq!(review.annotations[0].text, "動かした腕");
    assert_eq!(review.views.len(), 1);
    assert_eq!(review.views[0].camera.projection, Projection::Orthographic);

    // 元のファイル用のレビューは、書き出したGLBへ当てはめない（要件12.5）。
    // 元のファイルが別物になっていれば不一致になる。
    std::fs::write(&opened_path, std::fs::read(&exported).unwrap()).unwrap();
    let mut mismatch = Session::default();
    mismatch.apply(Command::OpenModel(exported));
    pump_load(&mut mismatch);
    mismatch.apply(Command::LoadReview(old_review));
    pump_load(&mut mismatch);
    assert_eq!(mismatch.status_text, "モデルが一致しません");
    assert!(mismatch.review.as_ref().unwrap().visibility.own(BASE));
}

#[test]
fn protect_moved_export() {
    for (tag, original, copy) in [
        ("renamed-glb", sample_glb(), false),
        ("moved-stl", model_reviewer_testdata::stl_sample(), true),
    ] {
        let dir = scratch(tag);
        let (mut session, source) = open_glb(&dir, "source", &original);
        session.apply(Command::ExportGlb(dir.join("working.glb")));
        pump_export(&mut session);
        assert!(session.export_error().is_none());
        let moved = dir.join("moved.glb");
        if copy {
            std::fs::copy(&source, &moved).unwrap();
            std::fs::remove_file(source).unwrap();
        } else {
            std::fs::rename(source, &moved).unwrap();
        }
        session.apply(Command::ExportGlb(moved.clone()));
        pump_export(&mut session);
        assert!(
            session
                .export_error()
                .is_some_and(|s| s.contains("別の名前"))
        );
        assert_eq!(std::fs::read(moved).unwrap(), original);
    }
}

#[test]
fn protect_swapped_destination() {
    let dir = scratch("swap-before-replace");
    let original = sample_glb();
    let (mut session, source) = open_glb(&dir, "sample.glb", &original);
    let target = dir.join("target.glb");
    let moved = target.clone();
    session.set_export_hooks(ExportHooks {
        before_verify: Some(Arc::new(move || std::fs::rename(&source, &moved).unwrap())),
        ..Default::default()
    });
    session.apply(Command::ExportGlb(target.clone()));
    pump_export(&mut session);
    assert!(
        session
            .export_error()
            .is_some_and(|s| s.contains("別の名前"))
    );
    assert_eq!(std::fs::read(target).unwrap(), original);
    assert!(!has_tmp_files(&dir));
}

#[test]
fn protect_replaced_output() {
    let dir = scratch("replaced-output");
    let (mut session, _) = open_glb(&dir, "sample.glb", &sample_glb());
    let out = dir.join("out.glb");
    session.apply(Command::ExportGlb(out.clone()));
    pump_export(&mut session);
    let bytes = std::fs::read(&out).unwrap();
    let twin = write_bytes(&dir, "opened-twin.glb", &bytes);
    session.apply(Command::OpenModel(twin.clone()));
    pump_load(&mut session);

    // 出力と同じバイト列を開いても、本物の出力には引き続き書ける。
    session.apply(Command::ExportGlb(out.clone()));
    pump_export(&mut session);
    assert!(session.export_error().is_none());
    assert_eq!(std::fs::read(&out).unwrap(), bytes);

    // 同じ場所・同じ内容でも、開いたファイルと入れ替われば例外を取り消す。
    std::fs::remove_file(&out).unwrap();
    std::fs::rename(twin, &out).unwrap();
    move_part(&mut session, ARM, Vec3::X);
    session.apply(Command::ExportGlb(out.clone()));
    pump_export(&mut session);
    assert!(
        session
            .export_error()
            .is_some_and(|s| s.contains("別の名前"))
    );
    assert_eq!(std::fs::read(out).unwrap(), bytes);
    assert!(session.doc_unsaved());
}

#[test]
fn protect_restore_candidate() {
    let dir = scratch("restore-other-copy");
    let (mut session, _) = open_glb(&dir, "sample.glb", &sample_glb());
    move_part(&mut session, ARM, Vec3::X);
    let out = dir.join("out.glb");
    session.apply(Command::ExportGlb(out.clone()));
    pump_export(&mut session);
    let review = dir.join("review.json");
    session.apply(Command::SaveReview(review.clone()));
    let mut json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&review).unwrap()).unwrap();
    json["model"]["relative_path"] = "candidate.glb".into();
    std::fs::write(&review, json.to_string()).unwrap();
    let candidate = dir.join("candidate.glb");
    std::fs::copy(&out, &candidate).unwrap();
    std::fs::remove_file(out).unwrap();
    let original = std::fs::read(&candidate).unwrap();
    move_part(&mut session, ARM, Vec3::Y);
    session.apply(Command::LoadReview(review));
    session.apply(Command::ConfirmDiscard);
    pump_load(&mut session);
    assert_eq!(session.status_text, "レビューを復元しました");
    move_part(&mut session, ARM, Vec3::Z);
    session.apply(Command::ExportGlb(candidate.clone()));
    pump_export(&mut session);
    assert!(
        session
            .export_error()
            .is_some_and(|s| s.contains("別の名前"))
    );
    assert_eq!(std::fs::read(candidate).unwrap(), original);
    assert!(session.doc_unsaved());
}

#[test]
fn re_exporting_to_own_exported_file_succeeds_while_opened_file_stays_forbidden() {
    let dir = scratch("reexport");
    let original = sample_glb();
    let (mut session, opened_path) = open_glb(&dir, "sample.glb", &original);

    move_part(&mut session, ARM, Vec3::new(0.2, 0.0, 0.0));
    let out = dir.join("out.glb");
    session.apply(Command::ExportGlb(out.clone()));
    pump_export(&mut session);
    assert!(!session.doc_unsaved());
    let first_bytes = std::fs::read(&out).unwrap();

    // 同じ作業の中で書き出したファイルへ、もう一度書き出せる。
    let second_local = move_part(&mut session, ARM, Vec3::new(0.0, 0.3, 0.0));
    session.apply(Command::ExportGlb(out.clone()));
    pump_export(&mut session);
    assert!(!session.doc_unsaved());
    let second_bytes = std::fs::read(&out).unwrap();
    assert_ne!(second_bytes, first_bytes);
    let reloaded = model_reviewer_gltf::load_glb(&second_bytes).unwrap();
    assert!(local_close(
        reloaded.nodes[ARM.0 as usize].local,
        second_local
    ));

    // 作業の対象が移った後も、最初に「開く」で開いたファイルへは書き出せない。
    move_part(&mut session, ARM, Vec3::new(0.0, 0.0, 0.2));
    session.apply(Command::ExportGlb(opened_path.clone()));
    pump_export(&mut session);
    assert_eq!(session.status_text, "書き出しに失敗しました");
    let body = session.dialog.as_ref().unwrap().body.clone();
    assert!(body.contains("別の名前"), "{body}");
    assert_eq!(std::fs::read(&opened_path).unwrap(), original);
    assert!(session.doc_unsaved());
}

#[test]
fn refuses_same_file_via_relative_symlink_and_hardlink_without_leaving_files() {
    let dir = scratch("same-file");
    let original = sample_glb();
    let (mut session, opened_path) = open_glb(&dir, "sample.glb", &original);
    let moved = move_part(&mut session, ARM, Vec3::new(0.1, 0.0, 0.0));
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let relative_same = sub.join("..").join("sample.glb");

    let mut targets = vec![("exact", opened_path.clone()), ("dotdot", relative_same)];

    let hard = dir.join("hard.glb");
    std::fs::hard_link(&opened_path, &hard).unwrap();
    targets.push(("hardlink", hard));

    #[cfg(unix)]
    {
        let link = dir.join("link.glb");
        std::os::unix::fs::symlink(&opened_path, &link).unwrap();
        targets.push(("symlink", link));

        let dir_link = dir.join("dir_link");
        std::os::unix::fs::symlink(&dir, &dir_link).unwrap();
        targets.push(("symlink_dir", dir_link.join("sample.glb")));
    }

    for (label, target) in &targets {
        assert!(same_file(&opened_path, target), "same_file failed: {label}");
        session.apply(Command::ExportGlb(target.clone()));
        pump_export(&mut session);
        assert_eq!(session.status_text, "書き出しに失敗しました", "{label}");
        let body = session.dialog.as_ref().expect(label).body.clone();
        assert!(body.contains("別の名前"), "{label}: {body}");
        assert_eq!(std::fs::read(&opened_path).unwrap(), original, "{label}");
        assert!(!has_tmp_files(&dir), "{label}");
        assert!(session.doc_unsaved(), "{label}");
        assert_eq!(
            session.doc.as_ref().unwrap().node(ARM).unwrap().local,
            moved,
            "{label}"
        );
    }

    // シンボリックリンクから開いた場合も、元の実体へ書き出せない。
    #[cfg(unix)]
    {
        let link = dir.join("link.glb");
        let mut via_link = Session::default();
        via_link.apply(Command::OpenModel(link));
        pump_load(&mut via_link);
        move_part(&mut via_link, ARM, Vec3::new(0.2, 0.0, 0.0));
        via_link.apply(Command::ExportGlb(opened_path.clone()));
        pump_export(&mut via_link);
        assert_eq!(via_link.status_text, "書き出しに失敗しました");
        assert!(via_link.dialog.as_ref().unwrap().body.contains("別の名前"));
        assert_eq!(std::fs::read(&opened_path).unwrap(), original);
    }
}

#[test]
#[cfg(unix)]
fn removed_opening_symlink_keeps_its_target_protected_for_both_writes() {
    let dir = scratch("removed-opening-link");
    let original = sample_glb();
    let target = write_bytes(&dir, "original.glb", &original);
    let link = dir.join("link.glb");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let mut session = Session::default();
    session.apply(Command::OpenModel(link.clone()));
    pump_load(&mut session);
    assert_eq!(session.status_text, "読み込み完了");
    std::fs::remove_file(&link).unwrap();

    // 作業対象を切り替え、開いたファイルの記録だけで原本を守れるかを確かめる。
    let other = write_bytes(&dir, "other.glb", &inputs::triangle());
    session.apply(Command::OpenModel(other));
    pump_load(&mut session);
    assert_eq!(session.status_text, "読み込み完了");
    session.apply(Command::SaveReview(target.clone()));
    let save_status = session.status_text.clone();
    session.apply(Command::ExportGlb(target.clone()));
    pump_export(&mut session);
    assert_eq!(
        (save_status.as_str(), session.status_text.as_str()),
        ("レビューを保存できません", "書き出しに失敗しました")
    );
    assert!(session.dialog.as_ref().unwrap().body.contains("別の名前"));
    assert!(std::fs::read(target).unwrap() == original);
}

#[test]
fn refuses_when_base_file_changed_or_missing_and_keeps_edits() {
    let dir = scratch("base-changed");
    let original = sample_glb();
    let (mut session, opened_path) = open_glb(&dir, "sample.glb", &original);
    let moved = move_part(&mut session, ARM, Vec3::new(0.3, 0.0, 0.0));
    session.apply(Command::SetVisible {
        node: BASE,
        visible: false,
    });
    let review_before = session.review.clone();
    let history_before = session.history.len();

    // 元のファイルを外から書き換える。
    std::fs::write(&opened_path, inputs::triangle()).unwrap();
    let out = dir.join("out.glb");
    session.apply(Command::ExportGlb(out.clone()));
    pump_export(&mut session);
    assert_eq!(session.status_text, "書き出しに失敗しました");
    let body = session.dialog.as_ref().unwrap().body.clone();
    assert!(body.contains("変わっている"), "{body}");
    assert!(!out.exists(), "保存先に何も残さない");
    assert!(!has_tmp_files(&dir));
    assert_eq!(
        session.doc.as_ref().unwrap().node(ARM).unwrap().local,
        moved
    );
    assert_eq!(session.review, review_before);
    assert_eq!(session.history.len(), history_before);
    assert!(session.doc_unsaved());
    assert!(session.review_unsaved());

    // 元のファイルが消えていても断り、編集を残す。
    std::fs::remove_file(&opened_path).unwrap();
    let out2 = dir.join("out2.glb");
    session.apply(Command::ExportGlb(out2.clone()));
    pump_export(&mut session);
    assert_eq!(session.status_text, "書き出しに失敗しました");
    let body = session.dialog.as_ref().unwrap().body.clone();
    assert!(
        body.contains("読めません") || body.contains("見つかりません"),
        "{body}"
    );
    assert!(!out2.exists());
    assert!(!has_tmp_files(&dir));
    assert_eq!(
        session.doc.as_ref().unwrap().node(ARM).unwrap().local,
        moved
    );
    assert_eq!(session.review, review_before);
    assert_eq!(session.history.len(), history_before);
}

#[test]
fn verify_failure_removes_temp_file_and_leaves_destination_untouched() {
    let dir = scratch("verify-fail");
    let original = sample_glb();
    let (mut session, _) = open_glb(&dir, "sample.glb", &original);
    let moved = move_part(&mut session, ARM, Vec3::new(0.25, 0.0, 0.0));
    session.apply(Command::SetVisible {
        node: BASE,
        visible: false,
    });
    let review_before = session.review.clone();

    let corrupt: Arc<dyn Fn(&Path) + Send + Sync> = Arc::new(|tmp: &Path| {
        let mut bytes = std::fs::read(tmp).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        std::fs::write(tmp, bytes).unwrap();
    });
    session.set_export_hooks(ExportHooks {
        after_write: Some(Arc::clone(&corrupt)),
        ..ExportHooks::default()
    });

    // 新規の保存先：照合に失敗したら保存先にも一時ファイルにも何も残さない。
    let out = dir.join("broken-target.glb");
    session.apply(Command::ExportGlb(out.clone()));
    pump_export(&mut session);
    assert_eq!(session.status_text, "書き出しに失敗しました");
    let body = session.dialog.as_ref().unwrap().body.clone();
    assert!(body.contains("合いませんでした"), "{body}");
    assert!(!out.exists(), "保存先に何も残さない");
    assert!(!has_tmp_files(&dir), "一時ファイルを消す");
    assert_eq!(
        session.doc.as_ref().unwrap().node(ARM).unwrap().local,
        moved
    );
    assert_eq!(session.review, review_before);
    assert!(session.doc_unsaved());

    // 先に正常に書き出したファイルへ再書き出しし、照合に失敗しても前のファイルが壊れない。
    session.set_export_hooks(ExportHooks::default());
    let existing = dir.join("existing.glb");
    session.apply(Command::ExportGlb(existing.clone()));
    pump_export(&mut session);
    let good_bytes = std::fs::read(&existing).unwrap();

    move_part(&mut session, ARM, Vec3::new(0.0, 0.5, 0.0));
    session.set_export_hooks(ExportHooks {
        after_write: Some(corrupt),
        ..ExportHooks::default()
    });
    session.apply(Command::ExportGlb(existing.clone()));
    pump_export(&mut session);
    assert_eq!(session.status_text, "書き出しに失敗しました");
    assert_eq!(std::fs::read(&existing).unwrap(), good_bytes);
    assert!(!has_tmp_files(&dir));
    assert!(session.doc_unsaved());
}

#[test]
fn undo_to_exported_point_clears_doc_unsaved_and_further_undo_sets_it_again() {
    let dir = scratch("undo-export");
    let (mut session, _) = open_glb(&dir, "sample.glb", &sample_glb());
    assert!(!session.doc_unsaved());
    assert!(!session.review_unsaved());

    move_part(&mut session, ARM, Vec3::new(0.1, 0.0, 0.0));
    assert!(session.doc_unsaved());
    assert!(!session.review_unsaved());

    let out = dir.join("out.glb");
    session.apply(Command::ExportGlb(out));
    pump_export(&mut session);
    assert!(!session.doc_unsaved());
    assert!(
        !session.review_unsaved(),
        "レビューに内容が無ければ未保存にならない"
    );
    assert!(!session.has_unsaved());

    move_part(&mut session, ARM, Vec3::new(0.0, 0.2, 0.0));
    assert!(session.doc_unsaved());
    assert!(!session.review_unsaved());

    // 書き出した時点へ戻ると印が消える。
    session.apply(Command::Undo);
    assert!(!session.doc_unsaved());
    assert!(!session.has_unsaved());

    // 書き出した時点より前へ戻ると、また未書き出しになる。
    session.apply(Command::Undo);
    assert!(session.doc_unsaved());
    assert!(session.has_unsaved());

    // やり直して書き出した時点へ戻ると、また消える。
    session.apply(Command::Redo);
    assert!(!session.doc_unsaved());
    assert!(!session.has_unsaved());
}

#[test]
fn split_unsaved_indicators_save_review_gate_and_named_discard_dialog() {
    let dir = scratch("split-unsaved");
    let (mut session, _) = open_glb(&dir, "sample.glb", &sample_glb());
    let other = write_bytes(&dir, "other.glb", &inputs::triangle());
    let broken = write_bytes(&dir, "broken.glb", &inputs::bad_json());

    // 1. 形の編集だけ未書き出し。
    move_part(&mut session, ARM, Vec3::new(0.2, 0.0, 0.0));
    assert!(session.doc_unsaved());
    assert!(!session.review_unsaved());

    // 書き出していない編集がある間のレビュー保存は断り、書き出しを選ばせる（要件12.2）。
    let blocked_review = dir.join("blocked.json");
    session.apply(Command::SaveReview(blocked_review.clone()));
    assert!(!blocked_review.exists());
    let dialog = session.dialog.as_ref().expect("書き出しを促すダイアログ");
    assert!(dialog.title.contains("書き出"), "{}", dialog.title);
    assert!(
        dialog
            .buttons
            .iter()
            .any(|button| button.command == Command::ExportGlbDialog)
    );
    session.apply(Command::DismissDialog);
    session.apply(Command::SaveReviewDialog);
    assert!(session.dialog.is_some());
    session.apply(Command::DismissDialog);

    // 確認ダイアログは「書き出していない形の編集」だけを挙げる。
    session.apply(Command::OpenModel(other.clone()));
    let dialog = session.dialog.as_ref().unwrap();
    assert!(
        dialog.body.contains("書き出していない形の編集"),
        "{}",
        dialog.body
    );
    assert!(
        !dialog.body.contains("保存していないレビュー"),
        "{}",
        dialog.body
    );
    assert!(!dialog.body.contains("入力途中の注釈"), "{}", dialog.body);
    assert!(dialog.buttons.iter().any(|b| b.label == "破棄して続ける"));
    assert!(dialog.buttons.iter().any(|b| b.label == "やめる"));
    session.apply(Command::CancelPending);

    // 状態欄に「●形の編集 未書き出し」だけが出る。
    let mut harness = ui_harness(session);
    harness.step();
    harness.get_by_label("●形の編集 未書き出し");
    assert!(harness.query_by_label("●レビュー未保存").is_none());

    // 2. レビューの変更も足すと、2つの印が別々に出て、確認ダイアログに両方が出る。
    harness.state_mut().apply(Command::SetVisible {
        node: BASE,
        visible: false,
    });
    harness.step();
    harness.get_by_label("●形の編集 未書き出し");
    harness.get_by_label("●レビュー未保存");

    let session = harness.state_mut();
    session.apply(Command::OpenModel(other.clone()));
    let body = session.dialog.as_ref().unwrap().body.clone();
    assert!(body.contains("書き出していない形の編集"), "{body}");
    assert!(body.contains("保存していないレビュー"), "{body}");
    assert!(!body.contains("入力途中の注釈"), "{body}");
    session.apply(Command::CancelPending);

    // 3. 入力途中の注釈も足すと、3つすべてを挙げる。
    session.apply(Command::SetDraftPending(true));
    session.apply(Command::OpenModel(broken));
    let body = session.dialog.as_ref().unwrap().body.clone();
    assert!(body.contains("書き出していない形の編集"), "{body}");
    assert!(body.contains("保存していないレビュー"), "{body}");
    assert!(body.contains("入力途中の注釈"), "{body}");

    // 4. 確認の後の読み込みが失敗したら、文書・履歴・レビュー・未保存のまま残る（要件13.4）。
    let local_before = session.doc.as_ref().unwrap().node(ARM).unwrap().local;
    let history_before = session.history.len();
    session.apply(Command::ConfirmDiscard);
    pump_load(session);
    assert!(session.doc_unsaved());
    assert!(session.review_unsaved());
    assert!(session.draft_pending());
    assert_eq!(session.history.len(), history_before);
    assert_eq!(
        session.doc.as_ref().unwrap().node(ARM).unwrap().local,
        local_before
    );
    session.apply(Command::DismissDialog);
    session.apply(Command::OpenModel(other));
    assert!(
        session.dialog.is_some(),
        "失敗した後は再び未保存の確認が出る"
    );
}

#[test]
fn apply_in_place_after_export_uses_exported_file_not_pre_export_model() {
    let dir = scratch("inplace-after-export");
    let (mut session, _) = open_glb(&dir, "sample.glb", &sample_glb());
    let exported_local = move_part(&mut session, ARM, Vec3::new(0.5, 0.0, 0.0));
    let out = dir.join("exported.glb");
    session.apply(Command::ExportGlb(out.clone()));
    pump_export(&mut session);

    session.apply(Command::SetVisible {
        node: BASE,
        visible: false,
    });
    let review_path = dir.join("review.json");
    session.apply(Command::SaveReview(review_path.clone()));
    assert!(!session.has_unsaved());

    // 書き出し後に形の編集が無い状態での読み込み：その場で復元し、書き出した位置を保つ。
    session.apply(Command::SetVisible {
        node: BASE,
        visible: true,
    });
    session.apply(Command::LoadReview(review_path.clone()));
    session.apply(Command::ConfirmDiscard);
    assert_eq!(session.status_text, "レビューを復元しました");
    assert!(!session.review.as_ref().unwrap().visibility.own(BASE));
    assert!(local_close(
        session.doc.as_ref().unwrap().node(ARM).unwrap().local,
        exported_local
    ));

    // 書き出し後にさらに形を動かした状態での読み込み：書き出し前のモデルへ戻らず、
    // 書き出したファイルを開き直して復元する。
    move_part(&mut session, ARM, Vec3::new(0.0, 0.8, 0.0));
    assert!(session.doc_unsaved());
    session.apply(Command::LoadReview(review_path));
    assert!(
        session
            .dialog
            .as_ref()
            .unwrap()
            .body
            .contains("書き出していない形の編集")
    );
    session.apply(Command::ConfirmDiscard);
    pump_load(&mut session);
    assert_eq!(session.status_text, "レビューを復元しました");
    assert!(!session.doc_unsaved());
    assert!(!session.review_unsaved());
    assert!(!session.review.as_ref().unwrap().visibility.own(BASE));
    assert!(
        local_close(
            session.doc.as_ref().unwrap().node(ARM).unwrap().local,
            exported_local
        ),
        "書き出したファイルの位置へ戻る（書き出し前へ戻らない）"
    );

    // 内部の再読み込みで exported.glb が禁止一覧に入らない（再書き出しできる）。
    move_part(&mut session, ARM, Vec3::new(0.0, 0.0, 0.1));
    session.apply(Command::ExportGlb(out));
    pump_export(&mut session);
    assert!(!session.doc_unsaved());
}

#[test]
fn export_notes_dialog_lists_detached_animated_skinned_and_matrix_nodes() {
    let dir = scratch("notes-dialog");
    let (mut session, _) = open_glb(&dir, "rich.glb", &rich_glb());

    let node_id = |session: &Session, file: u32| -> NodeId {
        session
            .doc
            .as_ref()
            .unwrap()
            .find_file_index(file)
            .expect("node")
    };
    // アニメーション対象・スキン対象・傾いた拡大のノードをそれぞれ動かす。
    let arm = node_id(&session, rich::ARM);
    let skinned = node_id(&session, rich::SKINNED);
    let skewed = node_id(&session, rich::SKEWED);
    let joint = node_id(&session, rich::JOINT);

    move_part(&mut session, arm, Vec3::new(0.0, 0.0, 0.5));
    move_part(&mut session, skinned, Vec3::new(0.5, 0.0, 0.0));

    // 傾いた拡大（SKEWED）は行列で移動し、関節（JOINT）を削除する。
    let skewed_key = session.doc.as_ref().unwrap().key(skewed).unwrap();
    let skewed_local = session.doc.as_ref().unwrap().node(skewed).unwrap().local;
    session
        .doc
        .as_mut()
        .unwrap()
        .set_local(
            skewed_key,
            Affine3A::from_translation(Vec3::Y) * skewed_local,
        )
        .unwrap();
    let joint_key = session.doc.as_ref().unwrap().key(joint).unwrap();
    session
        .doc
        .as_mut()
        .unwrap()
        .remove_subtree(joint_key)
        .unwrap();

    let out = dir.join("rich-out.glb");
    session.apply(Command::ExportGlb(out));
    pump_export(&mut session);

    let dialog = session.dialog.as_ref().expect("注意の一覧ダイアログ");
    assert_eq!(dialog.title, "書き出しました");
    assert!(dialog.body.contains("削除した部品 1 個"), "{}", dialog.body);
    assert!(
        dialog.body.contains("ファイルに残っています"),
        "{}",
        dialog.body
    );
    assert!(dialog.body.contains("アニメーション"), "{}", dialog.body);
    assert!(dialog.body.contains("スキン"), "{}", dialog.body);
    assert!(dialog.body.contains("matrix"), "{}", dialog.body);
}

#[test]
fn exporting_blocks_edits_opens_and_close_while_camera_works() {
    let dir = scratch("during-export");
    let (mut session, _) = open_glb(&dir, "sample.glb", &sample_glb());
    let other = write_bytes(&dir, "other.glb", &inputs::triangle());
    let moved = move_part(&mut session, ARM, Vec3::new(0.2, 0.0, 0.0));
    session.apply(Command::SetTool(Tool::Move));
    let review_before = session.review.clone();
    let history_before = session.history.len();

    let release = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(AtomicUsize::new(0));
    let _hold = Hold {
        release: Arc::clone(&release),
    };
    let rel = Arc::clone(&release);
    let ent = Arc::clone(&entered);
    session.set_export_hooks(ExportHooks {
        before_build: Some(Arc::new(move || {
            ent.fetch_add(1, Ordering::AcqRel);
            while !rel.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(1));
            }
        })),
        ..ExportHooks::default()
    });

    let out = dir.join("out.glb");
    session.apply(Command::ExportGlb(out.clone()));
    let deadline = Instant::now() + Duration::from_secs(5);
    while entered.load(Ordering::Acquire) == 0 {
        let _ = session.poll_exporter();
        assert!(Instant::now() < deadline, "組み立て工程に入らない");
        thread::sleep(Duration::from_millis(2));
    }
    let _ = session.poll_exporter();
    assert!(session.is_exporting());
    assert!(session.edits_locked());
    assert_eq!(session.export_stage(), Some(ExportStage::Building));
    let reason = session.edit_block_reason().expect("理由");
    assert!(reason.contains("書き出し中"), "{reason}");

    // 編集・取り消し・保存・別ファイルを開く・二重書き出しはすべて断られ、状態を壊さない。
    let trs = decompose(&moved).unwrap();
    for command in [
        Command::SetVisible {
            node: BASE,
            visible: false,
        },
        Command::HideSelected,
        Command::ShowAll,
        Command::SetTool(Tool::Rotate),
        Command::BeginTransform {
            handle: Handle::MoveX,
            ndc: Vec2::ZERO,
        },
        Command::PreviewLocal(trs),
        Command::CommitLocal(Trs {
            translation: trs.translation + Vec3::X,
            ..trs
        }),
        Command::Undo,
        Command::Redo,
        Command::SaveView {
            name: "横".to_owned(),
        },
        Command::AddShape(model_reviewer_core::shapes::ShapeKind::Cube),
        Command::DuplicateSelected,
        Command::DeleteSelected,
    ] {
        session.status_text.clear();
        session.apply(command.clone());
        assert_eq!(session.status_text, reason, "{command:?}");
    }
    assert_eq!(
        session.doc.as_ref().unwrap().node_count(),
        12,
        "構造も変わらない"
    );
    session.apply(Command::ExportGlb(dir.join("out2.glb")));
    assert!(session.status_text.contains("書き出し中"));
    session.apply(Command::SaveReview(dir.join("r.json")));
    assert!(session.status_text.contains("書き出し"));
    session.apply(Command::OpenModel(other.clone()));
    assert!(session.status_text.contains("書き出し中"));
    session.apply(Command::LoadReview(dir.join("r.json")));
    assert!(session.status_text.contains("書き出し中"));
    session.apply(Command::DroppedFiles(vec![other]));
    assert!(session.status_text.contains("書き出し中"));

    // 閉じる要求も書き出しが終わるまで止める。
    assert!(session.on_close_requested());
    assert_eq!(session.dialog.as_ref().unwrap().title, "書き出し中です");
    assert!(!session.take_quit());
    session.apply(Command::DismissDialog);

    // 視点の操作は書き出し中でも受け付ける（要件11.9）。
    let yaw_before = session.camera.yaw;
    session.apply(Command::Orbit(Vec2::new(24.0, 0.0)));
    assert_ne!(session.camera.yaw, yaw_before);
    session.apply(Command::SnapView(ViewDir::Top));
    assert!((session.camera.pitch.to_degrees() - 90.0).abs() < 1.0e-4);

    assert_eq!(
        session.doc.as_ref().unwrap().node(ARM).unwrap().local,
        moved
    );
    assert_eq!(session.review, review_before);
    assert_eq!(session.history.len(), history_before);

    // 画面でも工程名・理由・ボタン無効化が出る。
    let mut harness = ui_harness(session);
    harness.step();
    harness.get_by_label_contains(ExportStage::Building.label_ja());
    harness.get_by_label_contains(reason);
    harness.get_by_label(ui::LOCKED_EXPORT);
    // 書き出しボタンの位置が登録されている。
    let _ = widget_rect(&harness, ids::TOOLBAR_EXPORT);

    // 書き出しを解放すると正常終了し、再び編集できる。
    release.store(true, Ordering::Release);
    pump_export(harness.state_mut());
    assert!(!harness.state().is_exporting());
    assert!(!harness.state().edits_locked());
    assert!(!harness.state().doc_unsaved());
    assert!(out.is_file());
    harness.state_mut().apply(Command::Undo);
    assert!(harness.state().doc_unsaved());
}

#[test]
fn export_during_active_transform_cancels_preview_and_loading_blocks_export() {
    let dir = scratch("interrupt");
    let (mut session, _) = open_glb(&dir, "sample.glb", &sample_glb());
    let committed = move_part(&mut session, ARM, Vec3::new(0.2, 0.0, 0.0));
    let mut preview = decompose(&committed).unwrap();
    preview.translation.x += 5.0;
    session.apply(Command::PreviewLocal(preview));
    assert!(session.transform.is_some());

    // 変形プレビュー中の書き出しはプレビューを取り消してから確定済みの文書を書く。
    let out = dir.join("committed.glb");
    session.apply(Command::ExportGlb(out.clone()));
    assert!(session.transform.is_none());
    pump_export(&mut session);
    let reloaded = model_reviewer_gltf::load_glb(&std::fs::read(&out).unwrap()).unwrap();
    assert!(local_close(reloaded.nodes[ARM.0 as usize].local, committed));

    // 読み込み中は書き出しを断る。
    session.load = LoadPhase::Reading;
    let blocked = dir.join("blocked.glb");
    session.apply(Command::ExportGlb(blocked.clone()));
    assert!(!session.is_exporting());
    assert!(!blocked.exists());
    assert!(session.status_text.contains("読み込み中"));
}

#[test]
fn edit_scripts_parse() {
    let out = scratch("scripts");
    for name in [
        "edit-1.json",
        "edit-2.json",
        "edit-3.json",
        "measure-1.json",
        "heavy-edit.json",
    ] {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("e2e")
            .join(name);
        let launch = model_reviewer::parse_args([
            "model-reviewer",
            "--script",
            script.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .unwrap();
        let automation = Automation::from_launch(&launch);
        assert!(
            automation.script_error().is_none(),
            "{name}: {}",
            automation.script_error().unwrap_or_default()
        );
    }
}

/// レビューの保存先に、開いたGLBや書き出したGLBを選んでも、モデルのファイルを上書きしない。
#[test]
fn saving_review_onto_opened_or_exported_glb_is_refused_and_keeps_model_bytes() {
    let dir = scratch("review-onto-glb");
    let original = sample_glb();
    let (mut session, opened_path) = open_glb(&dir, "sample.glb", &original);
    session.apply(Command::SetVisible {
        node: BASE,
        visible: false,
    });

    // 開いたGLBそのもの、別名のリンク経由の両方を断る。
    session.apply(Command::SaveReview(opened_path.clone()));
    assert_eq!(std::fs::read(&opened_path).unwrap(), original);
    assert!(session.review_unsaved(), "保存していないので未保存のまま");
    let body = session
        .dialog
        .as_ref()
        .expect("理由のダイアログ")
        .body
        .clone();
    assert!(body.contains("別の名前"), "{body}");
    session.apply(Command::DismissDialog);
    #[cfg(unix)]
    {
        let link = dir.join("link.json");
        std::os::unix::fs::symlink("sample.glb", &link).unwrap();
        session.apply(Command::SaveReview(link.clone()));
        assert_eq!(std::fs::read(&opened_path).unwrap(), original);
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        session.apply(Command::DismissDialog);
    }

    // 書き出したGLB（いまの作業対象）へも保存しない。
    move_part(&mut session, ARM, Vec3::new(0.2, 0.0, 0.0));
    let exported = dir.join("edited.glb");
    session.apply(Command::ExportGlb(exported.clone()));
    pump_export(&mut session);
    let exported_bytes = std::fs::read(&exported).unwrap();
    session.apply(Command::SaveReview(exported.clone()));
    assert_eq!(std::fs::read(&exported).unwrap(), exported_bytes);
    assert!(session.review_unsaved());
    assert!(!has_tmp_files(&dir));

    // 別の名前なら保存できる。
    session.apply(Command::DismissDialog);
    session.apply(Command::SaveReview(dir.join("review.json")));
    assert_eq!(session.status_text, "レビューを保存しました");
}

#[test]
fn exported_document_matches_reopened_file_bit_for_bit() {
    // 書き出しは行列を位置・回転・大きさに分けて書くので、読み直すと最後の桁が変わる。
    // 書き出した後の文書は、開き直したときと同じ行列・範囲・初期視点になること。
    let dir = scratch("reopen-bits");
    let (mut session, _) = open_glb(&dir, "sample.glb", &sample_glb());
    move_part(&mut session, ARM, Vec3::new(0.4, 0.25, -0.15));
    for id in [ARM, BASE, NodeId(3)] {
        session.apply(Command::Select(Some(id)));
        let before = session.doc.as_ref().unwrap().node(id).unwrap().local;
        let mut trs = decompose(&before).expect("分けられる");
        trs.rotation = glam::Quat::from_euler(glam::EulerRot::XYZ, 0.3, 0.7, -0.2) * trs.rotation;
        trs.scale *= 1.37;
        session.apply(Command::CommitLocal(trs));
    }
    let exported = dir.join("e.glb");
    session.apply(Command::ExportGlb(exported.clone()));
    pump_export(&mut session);
    assert!(!session.doc_unsaved(), "{}", session.status_text);
    assert!(
        session.take_scene_dirty().instances,
        "そろえたらインスタンスを作り直す"
    );

    let mut again = Session::default();
    again.apply(Command::OpenModel(exported.clone()));
    pump_load(&mut again);
    let (edited, reopened) = (session.doc.as_ref().unwrap(), again.doc.as_ref().unwrap());
    assert_eq!(edited.node_count(), reopened.node_count());
    for (a, b) in edited.nodes().iter().zip(reopened.nodes()) {
        assert_eq!(
            a.local.to_cols_array(),
            b.local.to_cols_array(),
            "{}",
            a.name
        );
        assert_eq!(
            a.world.to_cols_array(),
            b.world.to_cols_array(),
            "{}",
            a.name
        );
        assert_eq!(a.world_aabb, b.world_aabb, "{}", a.name);
    }
    assert_eq!(edited.bounds(), reopened.bounds());
    session.apply(Command::Home);
    again.apply(Command::Home);
    assert_eq!(
        session.camera.distance.to_bits(),
        again.camera.distance.to_bits()
    );
    assert_eq!(session.camera.target, again.camera.target);

    // そろえた後は、何も変えずにもう一度書き出しても、変形を書き換えない（行列がファイルと同じ）。
    let second = dir.join("e2.glb");
    session.apply(Command::ExportGlb(second.clone()));
    pump_export(&mut session);
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );
    // JSONの数の書き方は読み書きで最後の桁が揺れることがあるので、読み直した行列で比べる。
    let mut third = Session::default();
    third.apply(Command::OpenModel(second));
    pump_load(&mut third);
    for (a, b) in again
        .doc
        .as_ref()
        .unwrap()
        .nodes()
        .iter()
        .zip(third.doc.as_ref().unwrap().nodes())
    {
        assert_eq!(
            a.local.to_cols_array(),
            b.local.to_cols_array(),
            "{}",
            a.name
        );
    }
}

fn reopen(path: &Path) -> Session {
    let mut session = Session::default();
    session.apply(Command::OpenModel(path.to_path_buf()));
    pump_load(&mut session);
    assert_eq!(session.status_text, "読み込み完了", "{}", path.display());
    session
}

fn names(session: &Session) -> Vec<String> {
    let doc = session.doc.as_ref().unwrap();
    (0..doc.node_count() as u32)
        .map(|i| doc.name(NodeId(i)).to_owned())
        .collect()
}

fn world_of(session: &Session, name: &str) -> Affine3A {
    let doc = session.doc.as_ref().unwrap();
    let id = (0..doc.node_count() as u32)
        .map(NodeId)
        .find(|id| doc.name(*id) == name)
        .unwrap_or_else(|| panic!("部品が無い: {name}"));
    *doc.world(id)
}

/// 足した部品を書き出し、動かしてもう一度書き出す。2回目も照合を通って開き直せる。
fn export_move_export(dir: &Path, add: impl FnOnce(&mut Session) -> String) {
    let (mut session, _) = open_glb(dir, "sample.glb", &sample_glb());
    let name = add(&mut session);
    let added = session.selection.expect("足した部品が選ばれている");
    let first = dir.join("first.glb");
    session.apply(Command::ExportGlb(first.clone()));
    pump_export(&mut session);
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );
    assert!(!session.doc_unsaved());
    let reopened = reopen(&first);
    assert_eq!(names(&reopened), names(&session));
    assert!(local_close(
        world_of(&reopened, &name),
        world_of(&session, &name)
    ));
    assert_eq!(reopened.placed_triangles(), session.placed_triangles());

    // 書き出したファイルが土台になった後で、足した部品を動かしてもう一度書き出す。
    let moved = move_part(&mut session, added, Vec3::new(0.0, 0.3, 0.2));
    let second = dir.join("second.glb");
    session.apply(Command::ExportGlb(second.clone()));
    pump_export(&mut session);
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );
    let again = reopen(&second);
    assert_eq!(names(&again), names(&session));
    let doc = again.doc.as_ref().unwrap();
    let id = (0..doc.node_count() as u32)
        .map(NodeId)
        .find(|id| doc.name(*id) == name)
        .unwrap();
    assert!(local_close(doc.node(id).unwrap().local, moved));
    assert_eq!(again.placed_triangles(), session.placed_triangles());
    // 足した形状は1回だけ足される（2回目は1回目に足したメッシュを使う）。
    assert_eq!(
        again.model.as_ref().unwrap().stats.meshes,
        reopened.model.as_ref().unwrap().stats.meshes
    );
}

#[test]
fn added_shape_survives_export_move_and_second_export() {
    let dir = scratch("shape-twice");
    export_move_export(&dir, |session| {
        session.camera.target = Vec3::new(2.0, 0.5, 0.0);
        session.apply(Command::AddShape(
            model_reviewer_core::shapes::ShapeKind::Cylinder,
        ));
        "円柱".to_owned()
    });
}

#[test]
fn duplicate_survives_export_move_and_second_export() {
    let dir = scratch("copy-twice");
    export_move_export(&dir, |session| {
        session.apply(Command::Select(Some(ARM)));
        session.apply(Command::DuplicateSelected);
        "arm コピー".to_owned()
    });
}

#[test]
fn deleted_part_comes_back_after_undo_and_second_export() {
    // 削除して書き出し → 取り消して書き出し。外したノードがシーンに戻る（データは残っていた）。
    let dir = scratch("delete-undo");
    let (mut session, _) = open_glb(&dir, "sample.glb", &sample_glb());
    session.apply(Command::Select(Some(ARM)));
    session.apply(Command::DeleteSelected);
    let first = dir.join("first.glb");
    session.apply(Command::ExportGlb(first.clone()));
    pump_export(&mut session);
    let dialog = session.dialog.as_ref().expect("削除の注意");
    assert!(dialog.body.contains("削除した部品 2 個"), "{}", dialog.body);
    assert_eq!(reopen(&first).doc.as_ref().unwrap().node_count(), 10);
    session.apply(Command::DismissDialog);
    session.apply(Command::Undo);
    assert!(session.doc_unsaved());
    let second = dir.join("second.glb");
    session.apply(Command::ExportGlb(second.clone()));
    pump_export(&mut session);
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );
    let again = reopen(&second);
    assert_eq!(names(&again), names(&reopen(&dir.join("sample.glb"))));
}

#[test]
fn duplicated_textured_part_exports_and_reopens() {
    let dir = scratch("textured-copy");
    let (mut session, _) = open_glb(&dir, "rich.glb", &rich_glb());
    let body = session
        .doc
        .as_ref()
        .unwrap()
        .find_file_index(rich::BODY)
        .unwrap();
    session.apply(Command::Select(Some(body)));
    session.apply(Command::DuplicateSelected);
    let copy = session.selection.unwrap();
    move_part(&mut session, copy, Vec3::new(1.0, 0.0, 0.0));
    let out = dir.join("out.glb");
    session.apply(Command::ExportGlb(out.clone()));
    pump_export(&mut session);
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );
    let reopened = reopen(&out);
    assert_eq!(names(&reopened), names(&session));
    // 複製は元と同じメッシュ（テクスチャ付きの材質ごと）を指す。
    let bytes = std::fs::read(&out).unwrap();
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let json: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
    let nodes = json["nodes"].as_array().unwrap();
    let copies: Vec<_> = nodes
        .iter()
        .filter(|node| {
            node["name"]
                .as_str()
                .is_some_and(|n| n.ends_with(" コピー"))
        })
        .collect();
    assert_eq!(copies.len(), 1);
    assert_eq!(copies[0]["mesh"], nodes[rich::BODY as usize]["mesh"]);
    assert!(json["textures"].as_array().is_some_and(|t| !t.is_empty()));
}

#[test]
fn undoing_an_exported_shape_then_adding_another_exports_and_reopens() {
    // 足して書き出す → 取り消す → 別の図形を足して書き出す → やり直しは消えている。
    // さらに書き出し後の取り消しとやり直しを挟んでも、毎回照合を通って開き直せる。
    let dir = scratch("undo-shape-across-export");
    let (mut session, _) = open_glb(&dir, "sample.glb", &sample_glb());
    let base_names = names(&session);
    session.camera.target = Vec3::new(2.0, 0.0, 0.0);
    session.apply(Command::AddShape(
        model_reviewer_core::shapes::ShapeKind::Cube,
    ));
    let first = dir.join("first.glb");
    session.apply(Command::ExportGlb(first.clone()));
    pump_export(&mut session);
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );

    // 取り消して書き出す（足した部品はシーンから外れる）。
    session.apply(Command::Undo);
    let second = dir.join("second.glb");
    session.apply(Command::ExportGlb(second.clone()));
    pump_export(&mut session);
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );
    session.apply(Command::DismissDialog);
    assert_eq!(names(&reopen(&second)), base_names);

    // やり直して書き出す（外れたノードが戻る）。
    session.apply(Command::Redo);
    let third = dir.join("third.glb");
    session.apply(Command::ExportGlb(third.clone()));
    pump_export(&mut session);
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );
    let reopened = reopen(&third);
    assert_eq!(names(&reopened), names(&session));
    assert!(local_close(
        world_of(&reopened, "立方体"),
        world_of(&session, "立方体")
    ));

    // 取り消して別の図形を足し、書き出す。
    session.apply(Command::Undo);
    session.camera.target = Vec3::new(-2.0, 0.0, 0.0);
    session.apply(Command::AddShape(
        model_reviewer_core::shapes::ShapeKind::Sphere,
    ));
    let fourth = dir.join("fourth.glb");
    session.apply(Command::ExportGlb(fourth.clone()));
    pump_export(&mut session);
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );
    let reopened = reopen(&fourth);
    assert_eq!(names(&reopened), names(&session));
    assert_eq!(reopened.placed_triangles(), session.placed_triangles());
    assert!(local_close(
        world_of(&reopened, "球"),
        world_of(&session, "球")
    ));
}
