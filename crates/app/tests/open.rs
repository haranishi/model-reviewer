//! GLBを開く。GPUが無くても通る。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use egui_kittest::kittest::Queryable;
use model_reviewer::command::Command;
use model_reviewer::loader::{LoadHooks, LoadPhase};
use model_reviewer::session::Session;
use model_reviewer_core::scene::NodeId;
use model_reviewer_testdata::{inputs, sample_glb};

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("open-tests")
        .join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_bytes(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn pump_until(session: &mut Session, done: impl Fn(&Session) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(session) {
        let _ = session.poll_loader();
        if Instant::now() > deadline {
            panic!(
                "読み込みが終わりません status={} phase={:?}",
                session.status_text,
                session.load.label()
            );
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn pump(session: &mut Session) {
    pump_until(session, |session| {
        let settled = matches!(session.load, LoadPhase::Idle | LoadPhase::Failed(_));
        let pick_ready = session.model.is_none() || session.pick_index.is_some();
        settled && pick_ready
    });
}

struct Hold {
    release: Arc<AtomicBool>,
}

impl Drop for Hold {
    fn drop(&mut self) {
        self.release.store(true, Ordering::Release);
    }
}

fn wait_flag(flag: &AtomicUsize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while flag.load(Ordering::Acquire) == 0 {
        if Instant::now() > deadline {
            panic!("工程の手前で止まりませんでした");
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn block_first(release: Arc<AtomicBool>, entered: Arc<AtomicUsize>) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        let turn = entered.fetch_add(1, Ordering::AcqRel);
        if turn == 0 {
            while !release.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(1));
            }
        }
    })
}

#[test]
fn drop_opens_sample_and_records_measured_times() {
    let dir = scratch("drop-open");
    let path = write_bytes(&dir, "sample.glb", &sample_glb());
    let bytes = std::fs::metadata(&path).unwrap().len();
    let mut session = Session::default();
    session.apply(Command::DroppedFiles(vec![path]));
    pump(&mut session);
    assert_eq!(
        session
            .model_ref
            .as_ref()
            .map(|model| model.file_name.as_str()),
        Some("sample.glb")
    );
    assert_eq!(session.metrics.file_bytes, Some(bytes));
    assert!(session.metrics.read.is_some());
    assert!(session.metrics.identity.is_some());
    assert!(session.metrics.parse.is_some());
    assert!(session.metrics.pick.is_some());
    assert!(session.metrics.upload_cpu.is_none());
    assert!(session.metrics.upload_frames.is_none());
    assert!(session.metrics.first_present.is_none());
    assert_eq!(session.status_text, "読み込み完了");
    assert!(!session.unsaved());
}

#[test]
fn failed_open_keeps_model_and_review() {
    let dir = scratch("keep-review");
    let good = write_bytes(&dir, "good.glb", &sample_glb());
    let bad = write_bytes(&dir, "bad.glb", &inputs::bad_json());
    let mut session = Session::default();
    session.apply(Command::OpenModel(good));
    pump(&mut session);
    let model = Arc::clone(session.model.as_ref().expect("model"));
    session
        .review
        .as_mut()
        .unwrap()
        .visibility
        .set(&model, NodeId(1), false);
    let review = session.review.clone();
    session.apply(Command::OpenModel(bad));
    pump(&mut session);
    assert!(Arc::ptr_eq(session.model.as_ref().unwrap(), &model));
    assert_eq!(session.review, review);
    assert!(!session.review.as_ref().unwrap().visibility.own(NodeId(1)));
    let body = session.dialog.as_ref().expect("dialog").body.clone();
    assert!(body.contains("読めません") || body.contains("破損") || body.contains("GLB"));
    assert!(matches!(session.load, LoadPhase::Failed(_)));
}

#[test]
fn older_load_does_not_replace_the_newer_open() {
    let dir = scratch("supersede");
    let first = write_bytes(&dir, "first.glb", &sample_glb());
    let second = write_bytes(&dir, "second.glb", &inputs::triangle());
    let release = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(AtomicUsize::new(0));
    let _hold = Hold {
        release: Arc::clone(&release),
    };
    let mut session = Session::default();
    session.set_load_hooks(LoadHooks {
        before_parse: Some(block_first(Arc::clone(&release), Arc::clone(&entered))),
        ..LoadHooks::default()
    });
    session.apply(Command::OpenModel(first));
    wait_flag(&entered);
    session.apply(Command::OpenModel(second));
    pump_until(&mut session, |session| {
        session
            .model_ref
            .as_ref()
            .is_some_and(|model| model.file_name == "second.glb")
            && matches!(session.load, LoadPhase::Idle)
            && session.pick_index.is_some()
    });
    let kept = Arc::clone(session.model.as_ref().unwrap());
    release.store(true, Ordering::Release);
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let _ = session.poll_loader();
        thread::sleep(Duration::from_millis(2));
    }
    assert!(Arc::ptr_eq(session.model.as_ref().unwrap(), &kept));
    assert_eq!(session.model_ref.as_ref().unwrap().file_name, "second.glb");
    assert_eq!(session.status_text, "読み込み完了");
}

#[test]
fn unsaved_confirm_stays_unsaved_when_the_open_fails() {
    let dir = scratch("unsaved-fail");
    let good = write_bytes(&dir, "good.glb", &inputs::triangle());
    let bad = write_bytes(&dir, "notes.txt", b"hello");
    let mut session = Session::default();
    session.apply(Command::OpenModel(good));
    pump(&mut session);
    session.mark_review_unsaved();
    let model = Arc::clone(session.model.as_ref().unwrap());
    session.apply(Command::OpenModel(bad));
    assert!(session.dialog.is_some());
    assert!(
        session
            .dialog
            .as_ref()
            .unwrap()
            .body
            .contains("保存していない")
    );
    assert!(Arc::ptr_eq(session.model.as_ref().unwrap(), &model));
    session.apply(Command::ConfirmDiscard);
    assert!(session.dialog.is_none());
    pump(&mut session);
    assert!(session.unsaved());
    assert!(Arc::ptr_eq(session.model.as_ref().unwrap(), &model));
    assert!(session.dialog.is_some());
}

#[test]
fn broken_missing_and_non_glb_do_not_crash() {
    let dir = scratch("rejects");
    let broken = write_bytes(&dir, "broken.glb", &inputs::bad_json());
    let wrong = write_bytes(&dir, "wrong.glb", &inputs::wrong_magic());
    let text = write_bytes(&dir, "note.txt", b"hello");
    let mut session = Session::default();
    for path in [broken, wrong, text, dir.join("missing.glb")] {
        session.apply(Command::OpenModel(path));
        pump(&mut session);
        assert!(session.model.is_none());
        assert!(session.dialog.is_some());
        assert!(matches!(session.load, LoadPhase::Failed(_)));
        assert!(!session.unsaved());
    }
}

#[test]
fn click_before_pick_index_says_preparing() {
    let dir = scratch("pick-wait");
    let path = write_bytes(&dir, "sample.glb", &inputs::triangle());
    let release = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(AtomicUsize::new(0));
    let _hold = Hold {
        release: Arc::clone(&release),
    };
    let mut session = Session::default();
    session.set_load_hooks(LoadHooks {
        before_pick: Some(block_first(Arc::clone(&release), Arc::clone(&entered))),
        ..LoadHooks::default()
    });
    session.apply(Command::OpenModel(path));
    pump_until(&mut session, |session| {
        session.model.is_some() && entered.load(Ordering::Acquire) > 0
    });
    assert!(session.pick_index.is_none());
    session.apply(Command::ViewClick {
        ndc: Default::default(),
    });
    assert!(session.status_text.contains("選択の準備中です"));
    release.store(true, Ordering::Release);
    pump(&mut session);
    assert!(session.pick_index.is_some());
}

#[test]
fn multiple_drops_use_the_first_and_json_is_a_review() {
    let dir = scratch("route");
    let glb = write_bytes(&dir, "first.glb", &inputs::triangle());
    let other = write_bytes(&dir, "second.glb", &sample_glb());
    let json = write_bytes(&dir, "notes.json", b"{}");
    let magic_json = write_bytes(&dir, "model.json", &inputs::triangle());
    let mut session = Session::default();
    session.apply(Command::DroppedFiles(vec![glb, other]));
    assert!(session.notice.contains("最初の1つ"));
    pump(&mut session);
    assert_eq!(session.model_ref.as_ref().unwrap().file_name, "first.glb");
    assert!(session.notice.contains("最初の1つ"));

    session.apply(Command::DroppedFiles(vec![json]));
    pump(&mut session);
    assert_eq!(session.status_text, "読み込みに失敗しました");
    let body = session.dialog.as_ref().expect("理由").body.clone();
    assert!(body.contains("レビューJSONではありません"), "{body}");
    assert_eq!(session.model_ref.as_ref().unwrap().file_name, "first.glb");

    session.apply(Command::DroppedFiles(vec![magic_json]));
    pump(&mut session);
    assert_eq!(session.model_ref.as_ref().unwrap().file_name, "model.json");
}

#[test]
fn empty_screen_shows_the_operation_table_until_a_load_starts() {
    let harness = egui_kittest::Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_size(egui::vec2(1100.0, 720.0))
        .with_context_setup(model_reviewer::fonts::install)
        .build_ui(|ui| {
            let session = Session::default();
            let mut commands = Vec::new();
            model_reviewer::ui::show(ui, &session, &mut commands);
        });
    harness.get_by_label("GLBをここへドロップ、または［開く］");
    harness.get_by_label("左ドラッグ");
    harness.get_by_label("トラックパッド");

    let loading = egui_kittest::Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_size(egui::vec2(800.0, 400.0))
        .with_context_setup(model_reviewer::fonts::install)
        .build_ui(|ui| {
            let mut session = Session::default();
            session.load = LoadPhase::Reading;
            session.loading_file_name = Some("waiting.glb".to_owned());
            let mut commands = Vec::new();
            model_reviewer::ui::empty::show(ui, &session, &mut commands);
        });
    loading.get_by_label("読み取り中");
    loading.get_by_label("waiting.glb");
    assert!(
        loading
            .query_by_label("GLBをここへドロップ、または［開く］")
            .is_none()
    );
}

#[derive(Debug)]
struct DiskFile(PathBuf);

impl egui::DroppedFile for DiskFile {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|err| err.to_string())
    }
}

#[test]
fn kittest_drop_opens_the_file() {
    let dir = scratch("kittest-drop");
    let path = write_bytes(&dir, "dropped.glb", &inputs::triangle());
    let launch = model_reviewer::parse_args(["model-reviewer"]).unwrap();
    let mut harness = egui_kittest::Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_max_steps(400)
        .with_context_setup(model_reviewer::fonts::install)
        .build_eframe(|cc| model_reviewer::App::new(cc, launch));
    harness
        .input_mut()
        .dropped_files
        .push(Arc::new(DiskFile(path)));
    let mut opened = false;
    for _ in 0..400 {
        harness.step();
        if harness.query_by_label_contains("読み込み完了").is_some() {
            opened = true;
            break;
        }
    }
    assert!(opened, "ドロップしたGLBが開きませんでした");
    harness.get_by_label_contains("dropped.glb");
}
