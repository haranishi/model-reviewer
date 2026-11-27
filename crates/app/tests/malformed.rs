//! 壊れたGLBを開いても、前のモデルとレビューが残る。

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use eframe::App as EframeApp;
use model_reviewer::App;
use model_reviewer::command::Command;
use model_reviewer::loader::LoadPhase;
use model_reviewer_core::scene::NodeId;
use model_reviewer_testdata::{broken_glb, sample_glb};

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("malformed")
        .join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn app() -> (App, egui::Context, eframe::Frame) {
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let launch = model_reviewer::parse_args(["model-reviewer"]).unwrap();
    let app = App::new(&cc, launch);
    let frame = eframe::Frame::_new_kittest();
    (app, ctx, frame)
}

fn pump(
    app: &mut App,
    ctx: &egui::Context,
    frame: &mut eframe::Frame,
    done: impl Fn(&App) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        app.logic(ctx, frame);
        if Instant::now() > deadline {
            let session = app.session();
            panic!(
                "終わりません status={} phase={:?}",
                session.status_text,
                session.load.label()
            );
        }
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn broken_glb_keeps_the_open_model_and_review() {
    let dir = scratch("broken");
    let good = dir.join("good.glb");
    let bad = dir.join("broken.glb");
    std::fs::write(&good, sample_glb()).unwrap();
    std::fs::write(&bad, broken_glb()).unwrap();
    let (mut app, ctx, mut frame) = app();
    app.session_mut().apply(Command::OpenModel(good));
    pump(&mut app, &ctx, &mut frame, |app| {
        app.session().status_text == "読み込み完了" && app.session().pick_index.is_some()
    });
    let model = Arc::clone(app.session().model.as_ref().unwrap());
    app.session_mut()
        .review
        .as_mut()
        .unwrap()
        .visibility
        .set(&model, NodeId(1), false);
    let review = app.session().review.clone();
    app.session_mut().apply(Command::OpenModel(bad));
    pump(&mut app, &ctx, &mut frame, |app| {
        matches!(app.session().load, LoadPhase::Failed(_))
    });
    let session = app.session();
    assert!(Arc::ptr_eq(session.model.as_ref().unwrap(), &model));
    assert_eq!(session.review, review);
    assert!(!session.review.as_ref().unwrap().visibility.own(NodeId(1)));
    let body = session.dialog.as_ref().expect("理由").body.clone();
    assert!(
        body.contains("破損") || body.contains("読めません") || body.contains("GLB"),
        "{body}"
    );
}
