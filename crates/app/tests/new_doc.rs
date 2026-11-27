//! 新規作成（第5段階の要件1・2）。空の文書・未保存の確認・断る場面・書き出しで作業の対象が移ること・
//! レビューの保存と復元。GPUは使わない。

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use glam::Vec3;
use model_reviewer::command::{Command, ShapeKind};
use model_reviewer::exporter::ExportHooks;
use model_reviewer::loader::LoadPhase;
use model_reviewer::session::{
    Draft, NEW_DISTANCE, NEW_EXPORT_NOTE, NOTHING_TO_EXPORT, REVIEW_NEEDS_EXPORT, Session,
    SourceKind,
};
use model_reviewer::ui::ids;
use model_reviewer_core::camera::grid_spacing;
use model_reviewer_core::gizmo::decompose;
use model_reviewer_core::scene::{NodeId, SceneRead};
use model_reviewer_core::units::Units;
use model_reviewer_gltf::load_glb;
use model_reviewer_testdata::sample_glb;

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("new-doc")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
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
            "読み込みが終わりません status={}",
            session.status_text
        );
        thread::sleep(Duration::from_millis(2));
    }
}

fn pump_export(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.is_exporting() {
        let _ = session.poll_exporter();
        assert!(Instant::now() < deadline, "書き出しが終わりません");
        thread::sleep(Duration::from_millis(2));
    }
}

fn open_sample(dir: &Path) -> Session {
    let path = dir.join("sample.glb");
    std::fs::write(&path, sample_glb()).unwrap();
    let mut session = Session::default();
    session.apply(Command::OpenModel(path));
    pump_load(&mut session);
    assert_eq!(session.status_text, "読み込み完了");
    session
}

fn new_session() -> Session {
    let mut session = Session::default();
    session.apply(Command::NewDocument);
    session
}

fn node_count(session: &Session) -> usize {
    session.doc.as_ref().map_or(0, |doc| doc.node_count())
}

fn translation(session: &Session, id: NodeId) -> Vec3 {
    let local = session.doc.as_ref().unwrap().node(id).unwrap().local;
    Vec3::from(local.translation)
}

fn is_empty_new(session: &Session) -> bool {
    session.is_new_document() && node_count(session) == 0 && session.model_ref.is_none()
}

#[test]
fn new_document_starts_empty() {
    let session = new_session();
    assert!(is_empty_new(&session));
    assert_eq!(session.source_kind, SourceKind::New);
    assert_eq!(session.status_text, "新規の文書を作りました");
    assert_eq!(session.units, Units::default());
    assert!(session.review.is_some());
    assert!(session.pick_index.is_some(), "当たり判定の表はすぐ作る");
    assert!(session.history.is_empty());
    assert!(!session.has_unsaved());
    assert!(session.dialog.is_none());
    assert!(matches!(session.load, LoadPhase::Idle));
    // 初期視点: 原点を見て、格子の間隔が 0.1 になる距離（要件1.6）。
    assert_eq!(session.camera.target, Vec3::ZERO);
    assert_eq!(session.camera.distance, NEW_DISTANCE);
    assert_eq!(grid_spacing(session.camera.distance), 0.1);
    // ファイルを読んでいないので、ファイルの大きさと読み取り・識別・解析は測らない。
    assert!(session.metrics.file_bytes.is_none());
    assert!(session.metrics.read.is_none());
    assert!(session.metrics.identity.is_none());
    assert!(session.metrics.parse.is_none());
    // 書き出し先を禁じるファイルも無い（何も開いていない）。
    assert!(session.opened_files().is_empty());
    assert_eq!(
        model_reviewer::export_file_name(&session).as_deref(),
        Some("無題.glb")
    );
}

#[test]
fn new_document_keeps_projection_and_home_returns_to_the_new_view() {
    let mut session = Session::default();
    session.apply(Command::ToggleProjection);
    let projection = session.camera.projection;
    session.apply(Command::NewDocument);
    assert_eq!(session.camera.projection, projection);
    session.set_viewport([800, 600]);
    session.apply(Command::Orbit(glam::Vec2::new(40.0, 10.0)));
    session.apply(Command::Zoom {
        factor: 0.5,
        cursor_ndc: None,
    });
    session.apply(Command::FitAll);
    session.apply(Command::Home);
    assert_eq!(session.camera.target, Vec3::ZERO);
    assert_eq!(session.camera.distance, NEW_DISTANCE);
    assert_eq!(session.camera.projection, projection);
}

#[test]
fn new_document_asks_before_discarding() {
    let dir = scratch("discard");
    let mut session = open_sample(&dir);
    let before = node_count(&session);
    // 形の編集（未書き出し）。
    session.apply(Command::Select(Some(NodeId(1))));
    let mut trs = decompose(&session.doc.as_ref().unwrap().node(NodeId(1)).unwrap().local)
        .expect("分けられる");
    trs.translation += Vec3::X;
    session.apply(Command::CommitLocal(trs));
    assert!(session.doc_unsaved());

    session.apply(Command::NewDocument);
    let dialog = session.dialog.clone().expect("確認が出る");
    assert!(dialog.body.contains("書き出していない形の編集"));
    // ［やめる］で今の文書が残る。
    session.apply(Command::CancelPending);
    assert_eq!(node_count(&session), before);
    assert_eq!(session.source_kind, SourceKind::Glb);
    assert!(session.model_ref.is_some());
    assert!(session.doc_unsaved());

    // 入力途中の注釈も確認の対象。
    session.apply(Command::SetDraftPending(true));
    session.apply(Command::NewDocument);
    assert!(
        session
            .dialog
            .as_ref()
            .unwrap()
            .body
            .contains("入力途中の注釈")
    );
    session.apply(Command::ConfirmDiscard);
    assert!(is_empty_new(&session));
    assert!(!session.has_unsaved());

    // 新規の文書で置いた図形も、別の新規作成の前に確かめる。
    session.apply(Command::AddShape(ShapeKind::Cube));
    assert!(session.doc_unsaved());
    session.apply(Command::NewDocument);
    assert!(session.dialog.is_some());
    session.apply(Command::CancelPending);
    assert_eq!(node_count(&session), 1);
    session.apply(Command::NewDocument);
    session.apply(Command::ConfirmDiscard);
    assert!(is_empty_new(&session));
}

#[test]
fn new_document_is_refused_while_busy() {
    let dir = scratch("busy");
    // 読み込み中（何も表示していない状態から開き始めた直後）。
    let path = dir.join("sample.glb");
    std::fs::write(&path, sample_glb()).unwrap();
    let mut session = Session::default();
    session.apply(Command::OpenModel(path));
    assert!(matches!(session.load, LoadPhase::Reading));
    session.apply(Command::NewDocument);
    assert_eq!(session.status_text, "読み込み中は新規作成できません");
    pump_load(&mut session);
    assert_eq!(session.source_kind, SourceKind::Glb);
    assert!(node_count(&session) > 0);

    // 書き出し中。
    let mut session = new_session();
    session.apply(Command::AddShape(ShapeKind::Cube));
    let release = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(AtomicUsize::new(0));
    let hold: Arc<dyn Fn() + Send + Sync> = {
        let release = Arc::clone(&release);
        let entered = Arc::clone(&entered);
        Arc::new(move || {
            entered.fetch_add(1, Ordering::AcqRel);
            let deadline = Instant::now() + Duration::from_secs(10);
            while !release.load(Ordering::Acquire) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(1));
            }
        })
    };
    session.set_export_hooks(ExportHooks {
        before_build: Some(hold),
        ..ExportHooks::default()
    });
    session.apply(Command::ExportGlb(dir.join("busy.glb")));
    assert!(session.is_exporting());
    session.apply(Command::NewDocument);
    assert_eq!(session.status_text, "書き出し中は新規作成できません");
    assert!(session.dialog.is_none());
    release.store(true, Ordering::Release);
    pump_export(&mut session);
    assert_eq!(node_count(&session), 1, "書き出した文書が残る");
    assert_eq!(session.source_kind, SourceKind::Glb);
    assert!(dir.join("busy.glb").is_file());
}

#[test]
fn first_shape_is_one_meter_at_origin() {
    let mut session = new_session();
    session.apply(Command::AddShape(ShapeKind::Cube));
    assert_eq!(node_count(&session), 1);
    assert_eq!(translation(&session, NodeId(0)), Vec3::ZERO);
    let bounds = session.doc.as_ref().unwrap().bounds();
    assert_eq!(bounds.min, Vec3::splat(-0.5));
    assert_eq!(bounds.max, Vec3::splat(0.5));
    assert!(
        session.status_text.contains("1辺 1"),
        "{}",
        session.status_text
    );
    assert_eq!(session.selection, Some(NodeId(0)));
    // 2個目からは今の決まり（全体の大きさから決める）。置けて、当たり判定の表に足される。
    session.apply(Command::AddShape(ShapeKind::Sphere));
    assert_eq!(node_count(&session), 2);
    assert!(session.placed_triangles() > 12);
    assert!(session.pick_index.is_some());
}

#[test]
fn deleting_everything_keeps_the_document() {
    let mut session = new_session();
    session.set_viewport([800, 600]);
    session.apply(Command::AddShape(ShapeKind::Cube));
    session.apply(Command::DeleteSelected);
    assert!(is_empty_new(&session));
    assert!(session.doc.as_ref().unwrap().bounds().is_empty());
    // 全体表示は空の範囲で視点を変えない。初期視点は新規の視点。
    let camera = session.camera;
    session.apply(Command::FitAll);
    assert_eq!(session.camera, camera);
    session.apply(Command::Home);
    assert_eq!(session.camera.distance, NEW_DISTANCE);
    // 空のまま次の図形を置くと、また大きさ 1 で原点に。
    session.apply(Command::AddShape(ShapeKind::Cylinder));
    assert_eq!(translation(&session, NodeId(0)), Vec3::ZERO);
    // 全部取り消しても空の新規の文書が残り、やり直せる。
    while !session.history.is_empty() {
        session.apply(Command::Undo);
    }
    assert!(is_empty_new(&session));
    session.apply(Command::Redo);
    assert_eq!(node_count(&session), 1);
    assert!(session.placed_triangles() > 0);
}

#[test]
fn review_save_is_refused_until_export() {
    let dir = scratch("review");
    let mut session = new_session();
    let review = dir.join("review.json");
    // 編集が無いとき: 失敗のダイアログ（要件2.6）。保存先には何も書かない。
    session.apply(Command::SaveReviewDialog);
    assert_eq!(session.dialog.as_ref().unwrap().body, REVIEW_NEEDS_EXPORT);
    session.apply(Command::DismissDialog);
    session.apply(Command::SaveReview(review.clone()));
    assert_eq!(session.dialog.as_ref().unwrap().body, REVIEW_NEEDS_EXPORT);
    assert!(!review.exists());
    session.apply(Command::DismissDialog);

    // 形の編集があるとき: 今の「先に GLB を書き出してください」の確認。
    session.apply(Command::AddShape(ShapeKind::Cube));
    session.apply(Command::SaveReviewDialog);
    assert_eq!(
        session.dialog.as_ref().unwrap().title,
        "先にGLBを書き出してください"
    );
    session.apply(Command::DismissDialog);

    // 注釈は置ける。書き出した後に保存でき、開き直すと同じ注釈が戻る。
    session.draft = Some(Draft {
        node: NodeId(0),
        local_pos: Vec3::new(0.0, 0.5, 0.0),
        local_normal: Vec3::Y,
        text: String::new(),
    });
    session.apply(Command::CommitDraft {
        text: "天板の高さ".to_owned(),
    });
    assert_eq!(session.review.as_ref().unwrap().annotations.len(), 1);
    let glb = dir.join("first.glb");
    session.apply(Command::ExportGlb(glb.clone()));
    pump_export(&mut session);
    session.apply(Command::DismissDialog);
    assert!(session.review_unsaved(), "書き出した後はレビューが未保存");
    session.apply(Command::SaveReview(review.clone()));
    assert_eq!(session.status_text, "レビューを保存しました");
    assert!(!session.has_unsaved());

    let mut reopened = Session::default();
    reopened.apply(Command::LoadReview(review));
    pump_load(&mut reopened);
    assert_eq!(reopened.status_text, "レビューを復元しました");
    assert_eq!(reopened.source_kind, SourceKind::Glb);
    assert_eq!(node_count(&reopened), 1);
    let notes = &reopened.review.as_ref().unwrap().annotations;
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].text, "天板の高さ");
}

#[test]
fn new_document_exports_and_moves_target() {
    let dir = scratch("export");
    let mut session = new_session();
    session.apply(Command::AddShape(ShapeKind::Cube));
    let first = dir.join("無題.glb");
    session.apply(Command::ExportGlb(first.clone()));
    pump_export(&mut session);
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );
    assert_eq!(session.source_kind, SourceKind::Glb);
    assert!(!session.is_new_document());
    assert!(!session.doc_unsaved());
    let model_ref = session.model_ref.clone().expect("作業の対象が移る");
    assert_eq!(model_ref.file_name, "無題.glb");
    assert!(
        session
            .dialog
            .as_ref()
            .unwrap()
            .body
            .contains(NEW_EXPORT_NOTE)
    );
    assert!(session.status_text.contains("無題.glb"));
    session.apply(Command::DismissDialog);
    // 書き出した後は GLB と同じ扱い（既定の名前は付けない）。
    assert!(model_reviewer::export_file_name(&session).is_none());
    let bytes = std::fs::read(&first).unwrap();
    let model = load_glb(&bytes).expect("開ける");
    assert_eq!(model.nodes.len(), 1);
    assert_eq!(model.nodes[0].name, "立方体");

    // 書き出したファイルを土台に、もう一度書き出せる（動かした位置が入る）。
    let mut trs = decompose(&session.doc.as_ref().unwrap().node(NodeId(0)).unwrap().local)
        .expect("分けられる");
    trs.translation = Vec3::new(2.0, 0.0, 0.0);
    session.apply(Command::CommitLocal(trs));
    let second = dir.join("second.glb");
    session.apply(Command::ExportGlb(second.clone()));
    pump_export(&mut session);
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );
    // 二度目の結果の一覧に、新規の文の行は出ない。
    assert!(
        session
            .dialog
            .as_ref()
            .is_none_or(|dialog| !dialog.body.contains(NEW_EXPORT_NOTE))
    );
    let model = load_glb(&std::fs::read(&second).unwrap()).expect("開ける");
    assert_eq!(model.nodes.len(), 1);
    let moved = Vec3::from(model.nodes[0].local.translation);
    assert!(support::close3(moved, Vec3::new(2.0, 0.0, 0.0)), "{moved}");
    // 最初のファイルは書き換えていない。
    assert_eq!(std::fs::read(&first).unwrap(), bytes);
    // 書き出したファイルは「開く」で開いたものではないので、上書きの書き出しもできる。
    session.apply(Command::ExportGlb(first.clone()));
    pump_export(&mut session);
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );
}

#[test]
fn empty_document_export_is_refused() {
    let dir = scratch("refuse");
    let target = dir.join("empty.glb");
    let mut session = new_session();
    session.apply(Command::ExportGlb(target.clone()));
    assert!(!session.is_exporting());
    assert_eq!(session.dialog.as_ref().unwrap().body, NOTHING_TO_EXPORT);
    assert!(session.export_failed());
    assert!(!target.exists());
    session.apply(Command::DismissDialog);
    session.apply(Command::ExportGlbDialog);
    assert_eq!(session.dialog.as_ref().unwrap().body, NOTHING_TO_EXPORT);
    session.apply(Command::DismissDialog);
    // 置いて消した後も同じ。
    session.apply(Command::AddShape(ShapeKind::Plane));
    session.apply(Command::DeleteSelected);
    session.apply(Command::ExportGlb(target.clone()));
    assert_eq!(session.dialog.as_ref().unwrap().body, NOTHING_TO_EXPORT);
    assert!(!target.exists());
    assert!(
        std::fs::read_dir(&dir).unwrap().next().is_none(),
        "一時ファイルも作らない"
    );
    // 置けば書き出せる。
    session.apply(Command::DismissDialog);
    session.apply(Command::AddShape(ShapeKind::Plane));
    session.apply(Command::ExportGlb(target.clone()));
    pump_export(&mut session);
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );
    assert!(target.is_file());
}

fn key_n(harness: &mut egui_kittest::Harness<'_, Session>) {
    harness.event(egui::Event::Key {
        key: egui::Key::N,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    });
    harness.step();
}

#[test]
fn cmd_n_works_over_panels_but_not_while_typing() {
    // 何も開いていない画面の［新規作成］。
    let (mut harness, seen) = support::harness(Session::default(), egui::vec2(1100.0, 720.0));
    harness.step();
    let button = support::widget_rect(&harness, ids::EMPTY_NEW).expect("［新規作成］");
    support::click(&mut harness, button.center());
    assert!(seen.lock().unwrap().contains(&Command::NewDocument));
    assert!(is_empty_new(harness.state()));

    // 部品の一覧（パネル）の上でも Cmd+N が働く。
    let (mut harness, seen) = support::harness(support::session(), egui::vec2(1100.0, 720.0));
    harness.step();
    let row = format!("{}.0", ids::PARTS_ROW);
    let panel = support::widget_rect(&harness, &row).expect("部品の一覧の行");
    harness.event(egui::Event::PointerMoved(panel.center()));
    harness.step();
    assert!(harness.ctx.is_pointer_over_egui());
    // パネルの上では、1文字のキー（A の全体表示）は働かない。Cmd+N は働く。
    harness.event(egui::Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    assert!(!seen.lock().unwrap().contains(&Command::FitAll));
    key_n(&mut harness);
    assert!(seen.lock().unwrap().contains(&Command::NewDocument));
    assert!(is_empty_new(harness.state()));

    // 入力中は働かない（視点の名前の欄に文字を打っている）。
    let (mut harness, seen) = support::harness(support::session(), egui::vec2(1100.0, 720.0));
    harness.step();
    let field = support::widget_rect(&harness, ids::VIEWS_NAME).expect("視点の名前");
    support::click(&mut harness, field.center());
    assert!(harness.ctx.text_edit_focused(), "欄に入っている");
    key_n(&mut harness);
    assert!(!seen.lock().unwrap().contains(&Command::NewDocument));
    assert!(!harness.state().is_new_document());

    // ダイアログの間も働かない。
    let mut dialog = support::session();
    dialog.apply(Command::AddShape(ShapeKind::Cube));
    dialog.apply(Command::NewDocument);
    assert!(dialog.dialog.is_some());
    let (mut harness, seen) = support::harness(dialog, egui::vec2(1100.0, 720.0));
    harness.step();
    seen.lock().unwrap().clear();
    key_n(&mut harness);
    assert!(!seen.lock().unwrap().contains(&Command::NewDocument));
    // N だけは注釈の切り替えで、新規作成ではない。
    let (mut harness, seen) = support::harness(support::session(), egui::vec2(1100.0, 720.0));
    support::hover_view(&mut harness);
    harness.event(egui::Event::Key {
        key: egui::Key::N,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
    assert!(!seen.lock().unwrap().contains(&Command::NewDocument));
}

#[test]
fn status_bar_names_the_new_document() {
    let (mut harness, _) = support::harness(new_session(), egui::vec2(1100.0, 720.0));
    harness.step();
    use egui_kittest::kittest::Queryable;
    harness.get_by_label("新規（未書き出し）");
    // 図形を置けないときの文（何も開いていない）。
    let mut empty = Session::default();
    empty.apply(Command::AddShape(ShapeKind::Cube));
    assert_eq!(
        empty.status_text,
        "［新規作成］（Cmd+N）か、モデルを開くと図形を置けます"
    );
}

#[test]
fn new_script_parses() {
    let out = scratch("script");
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("e2e")
        .join("new-1.json");
    let launch = model_reviewer::parse_args([
        "model-reviewer",
        "--script",
        script.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ])
    .unwrap();
    let automation = model_reviewer::automation::Automation::from_launch(&launch);
    assert!(
        automation.script_error().is_none(),
        "{}",
        automation.script_error().unwrap_or_default()
    );
}
