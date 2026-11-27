//! レビューの保存と復元。GPUは使わない。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::App as EframeApp;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use glam::Vec3;
use model_reviewer::App;
use model_reviewer::automation::WidgetFrame;
use model_reviewer::command::Command;
use model_reviewer::loader::{LoadHooks, LoadPhase};
use model_reviewer::session::{Draft, Session};
use model_reviewer::ui::ids;
use model_reviewer_core::identity::ModelIdentity;
use model_reviewer_core::scene::NodeId;
use model_reviewer_testdata::{inputs, sample_glb};

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("persist")
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

fn pump(session: &mut model_reviewer::Session) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let _ = session.poll_loader();
        let settled = matches!(session.load, LoadPhase::Idle | LoadPhase::Failed(_));
        let pick_ready = session.model.is_none() || session.pick_index.is_some();
        if settled && pick_ready && session.loading_file_name.is_none() {
            break;
        }
        if Instant::now() > deadline {
            panic!(
                "終わりません status={} phase={:?}",
                session.status_text,
                session.load.label()
            );
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn open_sample(dir: &Path) -> (model_reviewer::Session, PathBuf, Vec<u8>) {
    let bytes = sample_glb();
    let path = write_bytes(dir, "sample.glb", &bytes);
    let mut session = model_reviewer::Session::default();
    session.apply(Command::OpenModel(path.clone()));
    pump(&mut session);
    assert_eq!(session.status_text, "読み込み完了");
    (session, path, bytes)
}

fn hide_one(session: &mut model_reviewer::Session) {
    session.apply(Command::SetVisible {
        node: NodeId(1),
        visible: false,
    });
    assert!(session.unsaved());
}

struct Hold {
    release: Arc<AtomicBool>,
}

impl Drop for Hold {
    fn drop(&mut self) {
        self.release.store(true, Ordering::Release);
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

fn wait_flag(flag: &AtomicUsize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while flag.load(Ordering::Acquire) == 0 {
        if Instant::now() > deadline {
            panic!("読み込みが始まりません");
        }
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn save_is_atomic_and_records_both_paths() {
    let dir = scratch("save");
    let (mut session, glb, original) = open_sample(&dir);
    hide_one(&mut session);
    let review_path = dir.join("review.json");
    session.apply(Command::SaveReview(review_path.clone()));
    assert_eq!(session.status_text, "レビューを保存しました");
    assert!(!session.unsaved());
    assert!(!dir.join(".review.json.tmp").exists());
    assert_eq!(std::fs::read(&glb).unwrap(), original);
    let model_ref = session.model_ref.as_ref().unwrap();
    let absolute = &model_ref.path;
    assert!(absolute.is_absolute());
    let relative = model_ref.relative_path.as_deref().unwrap();
    let via_relative = std::fs::canonicalize(dir.join(relative)).unwrap();
    assert_eq!(via_relative, std::fs::canonicalize(&glb).unwrap());
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&review_path).unwrap()).unwrap();
    assert_eq!(
        json["model"]["path"].as_str().unwrap(),
        model_ref.path.to_string_lossy()
    );
    assert_eq!(json["model"]["relative_path"].as_str().unwrap(), relative);
}

#[test]
fn review_save_preserves_a_hard_link_at_the_old_temp_name() {
    let dir = scratch("save-temp-link");
    let (mut session, glb, original) = open_sample(&dir);
    hide_one(&mut session);
    let link = dir.join(".review.json.tmp");
    std::fs::hard_link(&glb, &link).unwrap();

    let path = dir.join("review.json");
    session.apply(Command::SaveReview(path.clone()));
    assert_eq!(session.status_text, "レビューを保存しました");
    assert!(
        std::fs::read(&glb).unwrap() == original,
        "原本の内容が変わった"
    );
    assert!(model_reviewer::exporter::same_file(&link, &glb));
    let file = model_reviewer_core::review_file::parse(&std::fs::read_to_string(path).unwrap())
        .expect("保存したレビュー");
    assert_eq!(file.hidden_nodes.len(), 1);
}

#[test]
fn failed_review_save_removes_only_its_own_temp_file() {
    let dir = scratch("save-temp-failure");
    let (mut session, glb, original) = open_sample(&dir);
    hide_one(&mut session);
    let path = dir.join("review.json");
    std::fs::create_dir(&path).unwrap();
    let link = dir.join(".review.json.tmp");
    std::fs::hard_link(&glb, &link).unwrap();

    session.apply(Command::SaveReview(path.clone()));
    assert_eq!(session.status_text, "レビューを保存できません");
    assert!(session.review_unsaved());
    assert!(path.is_dir());
    assert!(model_reviewer::exporter::same_file(&link, &glb));
    assert!(std::fs::read(&glb).unwrap() == original);
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 3);
}

#[test]
fn same_identity_restores_in_place() {
    let dir = scratch("inplace");
    let (mut session, _, _) = open_sample(&dir);
    hide_one(&mut session);
    let review_path = dir.join("review.json");
    session.apply(Command::SaveReview(review_path.clone()));
    let saved = session.review.clone();
    let model = Arc::clone(session.model.as_ref().unwrap());
    session.apply(Command::SetVisible {
        node: NodeId(1),
        visible: true,
    });
    assert!(session.unsaved());
    session.apply(Command::LoadReview(review_path));
    assert!(
        session
            .dialog
            .as_ref()
            .unwrap()
            .title
            .contains("保存していない")
    );
    session.apply(Command::ConfirmDiscard);
    assert_eq!(session.status_text, "レビューを復元しました");
    assert!(!session.unsaved());
    assert!(matches!(session.load, LoadPhase::Idle));
    assert_eq!(session.review, saved);
    assert!(Arc::ptr_eq(session.model.as_ref().unwrap(), &model));
    assert!(session.dialog.is_none());
}

#[test]
fn load_does_not_replace_the_review_before_the_model_arrives() {
    let dir = scratch("async");
    let (mut session, _, _) = open_sample(&dir);
    hide_one(&mut session);
    let review_path = dir.join("review.json");
    session.apply(Command::SaveReview(review_path.clone()));
    let other = write_bytes(&dir, "other.glb", &inputs::triangle());
    session.mark_saved();
    session.apply(Command::OpenModel(other));
    pump(&mut session);
    let release = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(AtomicUsize::new(0));
    let _hold = Hold {
        release: Arc::clone(&release),
    };
    session.set_load_hooks(LoadHooks {
        before_read: Some(block_first(Arc::clone(&release), Arc::clone(&entered))),
        ..LoadHooks::default()
    });
    let before = session.review.clone();
    session.apply(Command::LoadReview(review_path));
    wait_flag(&entered);
    assert_eq!(session.review, before);
    assert_ne!(session.status_text, "レビューを復元しました");
    release.store(true, Ordering::Release);
    pump(&mut session);
    assert_eq!(session.status_text, "レビューを復元しました");
    assert!(!session.review.as_ref().unwrap().visibility.own(NodeId(1)));
}

#[test]
fn relative_mismatch_falls_back_to_absolute_and_mismatches_only_when_all_fail() {
    let dir = scratch("mismatch-fallback");
    let (mut session, glb, original) = open_sample(&dir);
    hide_one(&mut session);
    let review_path = dir.join("review.json");
    session.apply(Command::SaveReview(review_path.clone()));
    let elsewhere = dir.join("kept");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let kept = elsewhere.join("sample.glb");
    std::fs::write(&kept, &original).unwrap();
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&review_path).unwrap()).unwrap();
    json["model"]["path"] = serde_json::Value::String(kept.display().to_string());
    std::fs::write(&review_path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
    let triangle = inputs::triangle();
    std::fs::write(&glb, &triangle).unwrap();

    // 相対パスが別物でも、絶対パス側が一致すれば読み込みスレッド経由で復元する。
    let mut session = Session::default();
    let reads = Arc::new(AtomicUsize::new(0));
    let reads_hook = Arc::clone(&reads);
    session.set_load_hooks(LoadHooks {
        before_read: Some(Arc::new(move || {
            reads_hook.fetch_add(1, Ordering::AcqRel);
        })),
        ..LoadHooks::default()
    });
    session.apply(Command::LoadReview(review_path.clone()));
    pump(&mut session);
    assert_eq!(session.status_text, "レビューを復元しました");
    assert!(!session.review.as_ref().unwrap().visibility.own(NodeId(1)));
    assert!(session.dialog.is_none());
    assert_eq!(
        std::fs::canonicalize(&session.model_ref.as_ref().unwrap().path).unwrap(),
        std::fs::canonicalize(&kept).unwrap()
    );
    assert!(
        reads.load(Ordering::Acquire) >= 2,
        "候補の確認は読み込みスレッドで順に行う"
    );

    // 絶対パス側も別物なら、すべての候補を確かめたうえで不一致を出す。
    std::fs::write(&kept, &triangle).unwrap();
    let mut session = Session::default();
    session.apply(Command::LoadReview(review_path));
    pump(&mut session);
    assert!(session.model.is_none());
    assert!(session.review.is_none());
    assert!(matches!(session.load, LoadPhase::Idle));
    assert_eq!(session.status_text, "モデルが一致しません");
    let body = session.dialog.as_ref().unwrap().body.clone();
    let expected = ModelIdentity::from_bytes(&original);
    let actual = ModelIdentity::from_bytes(&triangle);
    assert!(
        body.contains(&format!("{} バイト", expected.size_bytes)),
        "{body}"
    );
    assert!(
        body.contains(&format!("{} バイト", actual.size_bytes)),
        "{body}"
    );
    assert!(body.contains(&expected.blake3_hex[..16]), "{body}");
    assert!(body.contains(&actual.blake3_hex[..16]), "{body}");
    assert!(body.contains("別のGLB"), "{body}");
}

#[test]
fn missing_model_lists_places_and_relink_saves_the_new_path() {
    let dir = scratch("missing");
    let (mut session, glb, bytes) = open_sample(&dir);
    hide_one(&mut session);
    let review_path = dir.join("review.json");
    session.apply(Command::SaveReview(review_path.clone()));
    let moved_dir = dir.join("moved");
    std::fs::create_dir_all(&moved_dir).unwrap();
    let moved = moved_dir.join("sample.glb");
    std::fs::rename(&glb, &moved).unwrap();
    let mut session = model_reviewer::Session::default();
    session.apply(Command::LoadReview(review_path));
    pump(&mut session);
    assert!(session.model.is_none());
    assert!(matches!(session.load, LoadPhase::Idle));
    assert_eq!(session.status_text, "モデルが見つかりません");
    let body = session.dialog.as_ref().unwrap().body.clone();
    assert!(body.contains("探した場所"), "{body}");
    assert!(body.contains("モデルを指定"), "{body}");
    assert!(
        session
            .dialog
            .as_ref()
            .unwrap()
            .buttons
            .iter()
            .any(|button| button.label.contains("モデルを指定"))
    );
    session.apply(Command::RelinkModel(moved.clone()));
    pump(&mut session);
    assert_eq!(session.status_text, "レビューを復元しました");
    assert!(!session.review.as_ref().unwrap().visibility.own(NodeId(1)));
    let again = dir.join("again.json");
    session.apply(Command::SaveReview(again.clone()));
    assert_eq!(session.status_text, "レビューを保存しました");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&again).unwrap()).unwrap();
    let path = PathBuf::from(json["model"]["path"].as_str().unwrap());
    assert_eq!(path.file_name().unwrap(), "sample.glb");
    assert_eq!(path.parent().unwrap().file_name().unwrap(), "moved");
    let relative = json["model"]["relative_path"].as_str().unwrap();
    let via = std::fs::canonicalize(again.parent().unwrap().join(relative)).unwrap();
    assert_eq!(via, std::fs::canonicalize(&moved).unwrap());
    assert_eq!(std::fs::read(&moved).unwrap(), bytes);
}

#[test]
fn unknown_node_restores_nothing() {
    let dir = scratch("unknown");
    let (mut session, _, _) = open_sample(&dir);
    hide_one(&mut session);
    let review_path = dir.join("review.json");
    session.apply(Command::SaveReview(review_path.clone()));
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&review_path).unwrap()).unwrap();
    json["hidden_nodes"][0]["name"] = serde_json::Value::String("missing-node".to_owned());
    std::fs::write(&review_path, serde_json::to_string_pretty(&json).unwrap()).unwrap();
    session.apply(Command::SetVisible {
        node: NodeId(2),
        visible: false,
    });
    let before = session.review.clone();
    assert!(session.unsaved());
    session.apply(Command::LoadReview(review_path));
    assert_eq!(session.review, before);
    assert!(
        session
            .dialog
            .as_ref()
            .unwrap()
            .title
            .contains("保存していない")
    );
    session.apply(Command::ConfirmDiscard);
    assert_eq!(session.review, before);
    assert!(session.unsaved());
    assert_ne!(session.status_text, "レビューを復元しました");
    let body = session.dialog.as_ref().expect("理由").body.clone();
    assert!(body.contains("何も復元していません"), "{body}");
    assert!(
        body.contains("gltf_index") || body.contains("部品"),
        "{body}"
    );
}

#[test]
fn gpu_error_keeps_the_model_and_still_saves() {
    let dir = scratch("gpu-error");
    let (mut session, glb, original) = open_sample(&dir);
    hide_one(&mut session);
    let model = Arc::clone(session.model.as_ref().unwrap());
    let review = session.review.clone();
    session.load = LoadPhase::Uploading {
        uploaded_bytes: 1,
        total_bytes: 4,
    };
    session.close_for_gpu_error("device lost".to_owned());
    assert!(Arc::ptr_eq(session.model.as_ref().unwrap(), &model));
    assert_eq!(session.review, review);
    assert!(session.unsaved());
    assert!(matches!(session.load, LoadPhase::Idle));
    assert_eq!(session.status_text, "3D表示を止めました");
    let body = session.dialog.as_ref().unwrap().body.clone();
    assert!(body.contains("device lost"), "{body}");
    session.apply(Command::SaveReview(dir.join("review.json")));
    assert_eq!(session.status_text, "レビューを保存しました");
    assert!(!session.unsaved());
    assert_eq!(std::fs::read(&glb).unwrap(), original);
    assert!(session.model.is_some());
}

#[test]
fn unsaved_review_waits_before_replacing() {
    let dir = scratch("unsaved-review");
    let (mut session, _, _) = open_sample(&dir);
    hide_one(&mut session);
    let review = session.review.clone();
    let other = dir.join("other.json");
    std::fs::write(&other, "{}").unwrap();
    session.apply(Command::LoadReview(other));
    assert_eq!(session.review, review);
    assert!(session.unsaved());
    let dialog = session.dialog.as_ref().unwrap();
    assert!(dialog.title.contains("保存していない"), "{}", dialog.title);
    assert!(matches!(session.load, LoadPhase::Idle));
}

#[test]
fn window_close_asks_and_command_q_does_not() {
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let launch = model_reviewer::parse_args(["model-reviewer"]).unwrap();
    let mut app = App::new(&cc, launch);
    app.session_mut().mark_review_unsaved();
    let mut frame = eframe::Frame::_new_kittest();
    let mut raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        ..Default::default()
    };
    raw.viewports
        .get_mut(&egui::ViewportId::ROOT)
        .unwrap()
        .events
        .push(egui::ViewportEvent::Close);
    let mut output = ctx.run_ui(raw, |ui| {
        app.logic(ui.ctx(), &mut frame);
    });
    let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
    assert!(commands.contains(&egui::ViewportCommand::CancelClose));
    let body = app.session().dialog.as_ref().unwrap().body.clone();
    assert!(body.contains("保存していない"), "{body}");
    output.textures_delta.clear();

    // 実機の Cmd+Q はメニュー項目（macos.rs で performClose: に付け替えた）から
    // 閉じる要求として届く。egui に届くキーはショートカットにしないことを確かめる。
    app.session_mut().mark_saved();
    app.session_mut().apply(Command::DismissDialog);
    let mut raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        ..Default::default()
    };
    raw.events.push(egui::Event::Key {
        key: egui::Key::Q,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    });
    app.session_mut().mark_review_unsaved();
    let mut output = ctx.run_ui(raw, |ui| {
        let shortcuts = model_reviewer::shortcut_commands(ui.ctx(), app.session());
        assert!(shortcuts.is_empty());
        app.logic(ui.ctx(), &mut frame);
    });
    assert!(app.session().dialog.is_none());
    let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
    assert!(!commands.contains(&egui::ViewportCommand::CancelClose));
    output.textures_delta.clear();
}

fn ui_harness(session: Session) -> (Harness<'static, Session>, Arc<Mutex<Vec<Command>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let slot = Arc::clone(&seen);
    let harness = Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_max_steps(40)
        .with_size(egui::vec2(1100.0, 720.0))
        .with_context_setup(|ctx| {
            model_reviewer::fonts::install(ctx);
            model_reviewer::theme::install(ctx);
        })
        .build_ui_state(
            move |ui, session| {
                let mut commands = Vec::new();
                let pixels = model_reviewer::ui::show(ui, session, &mut commands);
                commands.extend(model_reviewer::shortcut_commands(ui.ctx(), session));
                slot.lock().unwrap().extend(commands.iter().cloned());
                for command in commands {
                    session.apply(command);
                }
                session.set_viewport(pixels);
            },
            session,
        );
    (harness, seen)
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

fn click_pos(harness: &mut Harness<Session>, pos: egui::Pos2) {
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(pos));
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.input_mut().events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    harness.step();
}

#[test]
fn replacement_load_blocks_edits_shows_reason_and_resumes_after_failure() {
    let dir = scratch("replace-lock");
    let (mut session, _, _) = open_sample(&dir);
    session.draft = Some(Draft {
        node: NodeId(0),
        local_pos: Vec3::ZERO,
        local_normal: Vec3::Y,
        text: String::new(),
    });
    session.apply(Command::CommitDraft {
        text: "初期".to_owned(),
    });
    session.apply(Command::SaveView {
        name: "正面".to_owned(),
    });
    session.apply(Command::Select(Some(NodeId(0))));
    session.mark_saved();

    let other = write_bytes(&dir, "other.glb", &inputs::triangle());
    let release = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(AtomicUsize::new(0));
    let _hold = Hold {
        release: Arc::clone(&release),
    };
    session.set_load_hooks(LoadHooks {
        before_read: Some(block_first(Arc::clone(&release), Arc::clone(&entered))),
        ..LoadHooks::default()
    });
    let review_before = session.review.clone();
    session.apply(Command::OpenModel(other));
    wait_flag(&entered);
    assert!(session.edits_locked());
    let reason = session.edit_block_reason().expect("理由");

    for command in [
        Command::SetVisible {
            node: NodeId(1),
            visible: false,
        },
        Command::HideSelected,
        Command::ShowAll,
        Command::SetAnnotateMode(true),
        Command::EditAnnotation {
            id: 1,
            text: "変更".to_owned(),
        },
        Command::DeleteAnnotation(1),
        Command::SaveView {
            name: "横".to_owned(),
        },
        Command::DeleteView(2),
    ] {
        session.status_text.clear();
        session.apply(command);
        assert_eq!(session.review, review_before);
        assert!(!session.unsaved());
        assert_eq!(session.status_text, reason);
    }

    // カメラ操作は読み込み中でも可能。
    let yaw_before = session.camera.yaw;
    session.apply(Command::Orbit(glam::Vec2::new(20.0, 0.0)));
    assert_ne!(session.camera.yaw, yaw_before);

    // 画面でも状態欄に理由が出て、編集系ボタンは押せない。
    let (mut harness, seen) = ui_harness(session);
    harness.step();
    harness.get_by_label_contains(reason);
    seen.lock().unwrap().clear();
    for widget in [
        ids::TOOLBAR_ANNOTATE,
        ids::PARTS_SHOW_ALL,
        &format!("{}.0", ids::PARTS_EYE),
        &format!("{}.1", ids::NOTES_DELETE),
        &format!("{}.2", ids::VIEWS_DELETE),
    ] {
        let pos = widget_rect(&harness, widget).center();
        click_pos(&mut harness, pos);
    }
    assert!(
        seen.lock().unwrap().is_empty(),
        "読み込み中に編集ボタンからコマンドが出た: {:?}",
        seen.lock().unwrap()
    );

    // 壊れたファイルの読み込みで失敗した後は、再び編集できる。
    let bad = write_bytes(&dir, "bad.glb", &inputs::bad_json());
    let session = harness.state_mut();
    session.set_load_hooks(LoadHooks::default());
    release.store(true, Ordering::Release);
    session.apply(Command::OpenModel(bad));
    pump(session);
    assert!(!session.edits_locked());
    session.apply(Command::SetVisible {
        node: NodeId(1),
        visible: false,
    });
    assert!(!session.review.as_ref().unwrap().visibility.own(NodeId(1)));
    assert!(session.unsaved());
}

#[test]
fn gpu_error_and_failed_discard_reset_bypass_and_keep_pending_edits() {
    let dir = scratch("bypass-reset");
    let (mut session, _, _) = open_sample(&dir);
    hide_one(&mut session);
    let second = write_bytes(&dir, "second.glb", &inputs::triangle());
    let third = write_bytes(&dir, "third.glb", &inputs::triangle());

    let release = Arc::new(AtomicBool::new(false));
    let entered = Arc::new(AtomicUsize::new(0));
    let _hold = Hold {
        release: Arc::clone(&release),
    };
    session.set_load_hooks(LoadHooks {
        before_read: Some(block_first(Arc::clone(&release), Arc::clone(&entered))),
        ..LoadHooks::default()
    });
    session.apply(Command::OpenModel(second));
    assert!(session.dialog.is_some());
    session.apply(Command::ConfirmDiscard);
    wait_flag(&entered);

    // 読み込み・転送中のGPUエラーで discard_bypass が戻り、次の開く操作で再び確認が出る。
    session.load = LoadPhase::Uploading {
        uploaded_bytes: 2,
        total_bytes: 8,
    };
    session.close_for_gpu_error("device lost".to_owned());
    release.store(true, Ordering::Release);
    assert!(!session.edits_locked());
    session.apply(Command::DismissDialog);
    session.apply(Command::OpenModel(third.clone()));
    let title = session
        .dialog
        .as_ref()
        .expect("GPUエラー後も未保存確認が出る")
        .title
        .clone();
    assert!(title.contains("保存していない"), "{title}");
    session.apply(Command::CancelPending);

    // 下書き入力途中だけの状態で破棄して続行し、新しい読み込みが失敗したら入力途中と確認を残す。
    session.mark_saved();
    session.draft = Some(Draft {
        node: NodeId(0),
        local_pos: Vec3::ZERO,
        local_normal: Vec3::Y,
        text: String::new(),
    });
    session.apply(Command::SetDraftPending(true));
    let bad = write_bytes(&dir, "bad.glb", &inputs::bad_json());
    session.set_load_hooks(LoadHooks::default());
    session.apply(Command::OpenModel(bad));
    assert!(session.dialog.is_some());
    session.apply(Command::ConfirmDiscard);
    pump(&mut session);
    assert!(session.draft.is_some());
    assert!(session.draft_pending());
    session.apply(Command::DismissDialog);
    session.apply(Command::OpenModel(third));
    assert!(
        session
            .dialog
            .as_ref()
            .expect("失敗後も下書き未保存の確認が出る")
            .title
            .contains("保存していない")
    );
}

#[test]
fn in_progress_draft_and_body_edit_are_unsaved_without_extra_commands() {
    let dir = scratch("pending-unsaved");
    let (mut session, _, _) = open_sample(&dir);
    session.set_viewport([800, 600]);
    session.draft = Some(Draft {
        node: NodeId(0),
        local_pos: Vec3::new(0.2, 0.2, 0.0),
        local_normal: Vec3::Z,
        text: String::new(),
    });
    let other = write_bytes(&dir, "other.glb", &inputs::triangle());
    let review_path = dir.join("review.json");
    session.apply(Command::SaveReview(review_path.clone()));
    assert!(!session.unsaved());

    let (mut harness, seen) = ui_harness(session);
    harness.step();
    harness.step();
    let draft_pos = widget_rect(&harness, ids::DRAFT_TEXT).center();
    click_pos(&mut harness, draft_pos);
    harness.event(egui::Event::Text("入力途中".to_owned()));
    harness.step();
    assert!(harness.state().draft_pending());
    harness.step();
    harness.get_by_label("●レビュー未保存");
    assert!(harness.query_by_label("●形の編集 未書き出し").is_none());

    // 同じ状態のまま描いても SetDraftPending を繰り返し送らない。
    seen.lock().unwrap().clear();
    let _ = harness.state_mut().take_scene_dirty();
    harness.step();
    assert!(
        !seen
            .lock()
            .unwrap()
            .iter()
            .any(|cmd| matches!(cmd, Command::SetDraftPending(_)))
    );
    assert!(!harness.state_mut().take_scene_dirty().any());

    // 閉じる・開く・レビュー読込の前に確認が出る。
    assert!(harness.state_mut().on_close_requested());
    assert!(harness.state().dialog.is_some());
    harness.state_mut().apply(Command::CancelPending);
    harness.state_mut().apply(Command::OpenModel(other.clone()));
    assert!(harness.state().dialog.is_some());
    harness.state_mut().apply(Command::CancelPending);
    harness
        .state_mut()
        .apply(Command::LoadReview(review_path.clone()));
    assert!(harness.state().dialog.is_some());
    harness.state_mut().apply(Command::CancelPending);

    // 登録して保存すると、下書きの未保存は消える。
    harness.state_mut().apply(Command::CommitDraft {
        text: "確定済み".to_owned(),
    });
    harness
        .state_mut()
        .apply(Command::SaveReview(review_path.clone()));
    assert!(!harness.state().unsaved());
    assert!(!harness.state().draft_pending());
    assert!(!harness.state_mut().on_close_requested());
    harness.step();
    harness.step();

    // 注釈本文を編集中（未確定）でも未保存として扱う。
    let note_pos = widget_rect(&harness, &format!("{}.1", ids::NOTES_TEXT)).center();
    click_pos(&mut harness, note_pos);
    harness.event(egui::Event::Text("追記".to_owned()));
    harness.step();
    assert!(harness.state().body_pending());
    harness.step();
    harness.get_by_label("●レビュー未保存");
    assert!(harness.query_by_label("●形の編集 未書き出し").is_none());

    // 同じ状態のまま描いても SetBodyPending を繰り返し送らない。
    seen.lock().unwrap().clear();
    let _ = harness.state_mut().take_scene_dirty();
    harness.step();
    assert!(
        !seen
            .lock()
            .unwrap()
            .iter()
            .any(|cmd| matches!(cmd, Command::SetBodyPending(_)))
    );
    assert!(!harness.state_mut().take_scene_dirty().any());

    assert!(harness.state_mut().on_close_requested());
    harness.state_mut().apply(Command::CancelPending);
    harness.state_mut().apply(Command::OpenModel(other));
    assert!(harness.state().dialog.is_some());
    harness.state_mut().apply(Command::CancelPending);
    harness.state_mut().apply(Command::LoadReview(review_path));
    assert!(harness.state().dialog.is_some());
}

#[test]
fn annotation_body_temp_memory_does_not_leak_across_review_or_model_switch() {
    let dir = scratch("body-generation");
    let (mut session, _, _) = open_sample(&dir);
    session.draft = Some(Draft {
        node: NodeId(0),
        local_pos: Vec3::ZERO,
        local_normal: Vec3::Y,
        text: String::new(),
    });
    session.apply(Command::CommitDraft {
        text: "レビュー1".to_owned(),
    });
    let review1 = dir.join("review1.json");
    session.apply(Command::SaveReview(review1.clone()));
    session.apply(Command::EditAnnotation {
        id: 1,
        text: "レビュー2".to_owned(),
    });
    let review2 = dir.join("review2.json");
    session.apply(Command::SaveReview(review2.clone()));
    session.apply(Command::LoadReview(review1));
    assert_eq!(
        session.review.as_ref().unwrap().annotations[0].text,
        "レビュー1"
    );

    let (mut harness, _) = ui_harness(session);
    harness.step();
    harness.step();
    let note_pos = widget_rect(&harness, &format!("{}.1", ids::NOTES_TEXT)).center();
    click_pos(&mut harness, note_pos);
    harness.event(egui::Event::Text("古い途中入力".to_owned()));
    harness.step();
    assert!(harness.state().body_pending());

    // 同じモデル上で別レビューへ切り替えても、古い入力途中が残らない。
    harness.state_mut().apply(Command::LoadReview(review2));
    assert!(harness.state().dialog.is_some());
    harness.state_mut().apply(Command::ConfirmDiscard);
    assert_eq!(harness.state().status_text, "レビューを復元しました");
    assert!(!harness.state().body_pending());
    harness.step();
    // 別の場所を押してフォーカスを外しても、古い本文で上書きされない。
    let viewport = widget_rect(&harness, ids::VIEWPORT).center();
    click_pos(&mut harness, viewport);
    harness.step();
    assert_eq!(
        harness.state().review.as_ref().unwrap().annotations[0].text,
        "レビュー2"
    );
    assert!(!harness.state().unsaved());

    // モデルを切り替えた先で同じ id=1 の注釈があっても、古い入力途中が残らない。
    let other_glb = write_bytes(&dir, "other.glb", &inputs::triangle());
    let mut other_session = Session::default();
    other_session.apply(Command::OpenModel(other_glb));
    pump(&mut other_session);
    other_session.draft = Some(Draft {
        node: NodeId(0),
        local_pos: Vec3::ZERO,
        local_normal: Vec3::Y,
        text: String::new(),
    });
    other_session.apply(Command::CommitDraft {
        text: "モデルBの注釈".to_owned(),
    });
    let review_b = dir.join("review_b.json");
    other_session.apply(Command::SaveReview(review_b.clone()));

    let note_pos = widget_rect(&harness, &format!("{}.1", ids::NOTES_TEXT)).center();
    click_pos(&mut harness, note_pos);
    harness.event(egui::Event::Text("モデルAの途中".to_owned()));
    harness.step();
    assert!(harness.state().body_pending());

    harness.state_mut().apply(Command::LoadReview(review_b));
    assert!(harness.state().dialog.is_some());
    harness.state_mut().apply(Command::ConfirmDiscard);
    pump(harness.state_mut());
    assert_eq!(harness.state().status_text, "レビューを復元しました");
    harness.step();
    let viewport = widget_rect(&harness, ids::VIEWPORT).center();
    click_pos(&mut harness, viewport);
    harness.step();
    assert_eq!(
        harness.state().review.as_ref().unwrap().annotations[0].text,
        "モデルBの注釈"
    );
    assert!(!harness.state().unsaved());
}

#[test]
fn save_failure_on_non_finite_values_keeps_existing_file() {
    let dir = scratch("save-non-finite");
    let (mut session, _, _) = open_sample(&dir);
    session.draft = Some(Draft {
        node: NodeId(0),
        local_pos: Vec3::ZERO,
        local_normal: Vec3::Y,
        text: String::new(),
    });
    session.apply(Command::CommitDraft {
        text: "正常".to_owned(),
    });
    session.apply(Command::SaveView {
        name: "正面".to_owned(),
    });
    let review_path = dir.join("review.json");
    session.apply(Command::SaveReview(review_path.clone()));
    assert_eq!(session.status_text, "レビューを保存しました");
    let original_bytes = std::fs::read(&review_path).unwrap();

    // 注釈に NaN が入った状態での保存は失敗し、前のファイルを残す。
    session.review.as_mut().unwrap().annotations[0].local_pos.x = f32::NAN;
    session.mark_review_unsaved();
    session.apply(Command::SaveReview(review_path.clone()));
    assert_ne!(session.status_text, "レビューを保存しました");
    assert!(session.unsaved());
    assert!(session.dialog.is_some());
    assert_eq!(std::fs::read(&review_path).unwrap(), original_bytes);

    // 視点の角度が度へ変換すると溢れる状態でも失敗し、前のファイルを残す。
    session.review.as_mut().unwrap().annotations[0].local_pos.x = 0.0;
    session.review.as_mut().unwrap().views[0].camera.yaw = f32::MAX;
    session.apply(Command::DismissDialog);
    session.apply(Command::SaveReview(review_path.clone()));
    assert_ne!(session.status_text, "レビューを保存しました");
    assert!(session.unsaved());
    assert!(session.dialog.is_some());
    assert_eq!(std::fs::read(&review_path).unwrap(), original_bytes);
}
