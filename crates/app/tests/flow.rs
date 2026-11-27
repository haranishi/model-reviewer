//! 開いてからレビューを別の App で戻す。GPUは使わない。

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use eframe::App as EframeApp;
use glam::Vec2;
use model_reviewer::App;
use model_reviewer::command::Command;
use model_reviewer::loader::LoadPhase;
use model_reviewer_core::camera::ZOOM_EXP_PER_PX;
use model_reviewer_core::review::ReviewState;
use model_reviewer_core::scene::NodeId;
use model_reviewer_testdata::{inputs, sample_glb};

const CENTER: [f32; 2] = [0.5, 0.5];
const NOTE: [f32; 2] = [0.52, 0.60];

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("flow")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn launch_app() -> (App, egui::Context, eframe::Frame) {
    let ctx = egui::Context::default();
    let cc = eframe::CreationContext::_new_kittest(ctx.clone());
    let launch = model_reviewer::parse_args(["model-reviewer"]).unwrap();
    let app = App::new(&cc, launch);
    (app, ctx, eframe::Frame::_new_kittest())
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

fn wait_ready(app: &mut App, ctx: &egui::Context, frame: &mut eframe::Frame) {
    pump(app, ctx, frame, |app| {
        let session = app.session();
        matches!(session.load, LoadPhase::Idle)
            && session.model.is_some()
            && session.pick_index.is_some()
    });
}

fn ndc(at: [f32; 2]) -> Vec2 {
    Vec2::new((at[0] - 0.5) * 2.0, (0.5 - at[1]) * 2.0)
}

fn replay_camera(session: &mut model_reviewer::Session, viewport_px: [u32; 2], ppp: f32) {
    session.set_viewport(viewport_px);
    session.apply(Command::Home);
    let point_w = viewport_px[0] as f32 / ppp;
    let point_h = viewport_px[1] as f32 / ppp;
    session.apply(Command::Orbit(Vec2::new(
        (0.56 - 0.48) * point_w,
        -((0.47 - 0.5) * point_h),
    )));
    session.apply(Command::Pan(Vec2::new(
        (0.52 - 0.5) * point_w * ppp,
        -((0.51 - 0.5) * point_h * ppp),
    )));
    session.apply(Command::Zoom {
        factor: (-24.0 * ZOOM_EXP_PER_PX).exp(),
        cursor_ndc: Some(ndc(CENTER)),
    });
}

fn click(session: &mut model_reviewer::Session, at: [f32; 2]) -> Option<NodeId> {
    session.annotate_mode = false;
    session.draft = None;
    session.apply(Command::ViewClick { ndc: ndc(at) });
    session.selection
}

fn node_name(session: &model_reviewer::Session, id: NodeId) -> String {
    session
        .model
        .as_ref()
        .and_then(|model| model.nodes.get(id.0 as usize))
        .map(|node| node.name.clone())
        .unwrap_or_else(|| id.0.to_string())
}

fn reviews_match(left: &ReviewState, right: &ReviewState) {
    let hidden = |review: &ReviewState| -> Vec<u32> {
        review.visibility.hidden_own().map(|id| id.0).collect()
    };
    assert_eq!(hidden(left), hidden(right));
    assert_eq!(left.annotations.len(), right.annotations.len());
    for (left, right) in left.annotations.iter().zip(&right.annotations) {
        assert_eq!(left.id, right.id);
        assert_eq!(left.node, right.node);
        assert_eq!(left.text, right.text);
        assert!((left.local_pos - right.local_pos).length() < 1.0e-4);
        assert!((left.local_normal - right.local_normal).length() < 1.0e-4);
    }
    assert_eq!(left.views.len(), right.views.len());
    for (left, right) in left.views.iter().zip(&right.views) {
        assert_eq!(left.id, right.id);
        assert_eq!(left.name, right.name);
        assert!((left.camera.yaw - right.camera.yaw).abs() < 1.0e-4);
        assert!((left.camera.pitch - right.camera.pitch).abs() < 1.0e-4);
        assert!((left.camera.fov_y - right.camera.fov_y).abs() < 1.0e-4);
        assert!((left.camera.distance - right.camera.distance).abs() < 1.0e-3);
        assert!((left.camera.target - right.camera.target).length() < 1.0e-3);
    }
}

fn open_bytes(
    dir: &Path,
    name: &str,
    bytes: &[u8],
) -> (App, egui::Context, eframe::Frame, PathBuf) {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    let (mut app, ctx, mut frame) = launch_app();
    app.session_mut()
        .apply(Command::DroppedFiles(vec![path.clone()]));
    wait_ready(&mut app, &ctx, &mut frame);
    (app, ctx, frame, path)
}

#[test]
fn drop_edit_save_and_restore_on_a_new_app() {
    let dir = scratch("roundtrip");
    let (mut app, _ctx, _frame, _) = open_bytes(&dir, "sample.glb", &sample_glb());
    replay_camera(app.session_mut(), [1280, 800], 1.0);
    let selected = match click(app.session_mut(), CENTER) {
        Some(id) => id,
        None => panic!("中心が当たりません status={}", app.session().status_text),
    };
    app.session_mut().apply(Command::HideSelected);
    assert!(app.session().unsaved());
    assert!(
        !app.session()
            .review
            .as_ref()
            .unwrap()
            .visibility
            .own(selected)
    );
    app.session_mut().apply(Command::SetAnnotateMode(true));
    app.session_mut()
        .apply(Command::ViewClick { ndc: ndc(NOTE) });
    let draft = match app.session().draft.clone() {
        Some(draft) => draft,
        None => panic!(
            "注釈位置が当たりません status={} 中心={}",
            app.session().status_text,
            node_name(app.session(), selected)
        ),
    };
    assert_ne!(draft.node, selected);
    app.session_mut().apply(Command::CommitDraft {
        text: "先端".to_owned(),
    });
    app.session_mut().apply(Command::SaveView {
        name: "確認".to_owned(),
    });
    let review = app.session().review.as_ref().unwrap();
    assert_eq!(review.annotations[0].id, 1);
    assert_eq!(review.annotations[0].text, "先端");
    assert_eq!(review.views[0].id, 2);
    assert_eq!(review.views[0].name, "確認");
    let saved = review.clone();
    let identity = app.session().model_ref.as_ref().unwrap().identity.clone();
    let review_path = dir.join("review.json");
    app.session_mut()
        .apply(Command::SaveReview(review_path.clone()));
    assert_eq!(app.session().status_text, "レビューを保存しました");

    let (mut again, ctx, mut frame) = launch_app();
    again.session_mut().apply(Command::LoadReview(review_path));
    pump(&mut again, &ctx, &mut frame, |app| {
        app.session().status_text == "レビューを復元しました" && app.session().pick_index.is_some()
    });
    reviews_match(again.session().review.as_ref().unwrap(), &saved);
    assert_eq!(
        again.session().model_ref.as_ref().unwrap().identity,
        identity
    );
    assert!(!again.session().unsaved());
}

#[test]
fn a_different_model_is_not_restored() {
    let dir = scratch("other");
    let (mut app, _ctx, _frame, glb) = open_bytes(&dir, "sample.glb", &sample_glb());
    app.session_mut().apply(Command::SetVisible {
        node: NodeId(1),
        visible: false,
    });
    let review_path = dir.join("review.json");
    app.session_mut()
        .apply(Command::SaveReview(review_path.clone()));
    drop(app);
    std::fs::write(&glb, inputs::triangle()).unwrap();

    let (mut again, ctx, mut frame, _) = open_bytes(&dir, "sample.glb", &inputs::triangle());
    again.session_mut().apply(Command::SetVisible {
        node: NodeId(0),
        visible: false,
    });
    let before = again.session().review.clone();
    assert!(again.session().unsaved());
    again.session_mut().apply(Command::LoadReview(review_path));
    assert_eq!(again.session().review, before);
    let title = again.session().dialog.as_ref().unwrap().title.clone();
    assert!(title.contains("保存していない"), "{title}");
    again.session_mut().apply(Command::ConfirmDiscard);
    pump(&mut again, &ctx, &mut frame, |app| {
        matches!(app.session().load, LoadPhase::Idle | LoadPhase::Failed(_))
            && app.session().dialog.is_some()
    });
    assert_eq!(again.session().review, before);
    assert!(again.session().unsaved());
    assert_ne!(again.session().status_text, "レビューを復元しました");
    let body = again.session().dialog.as_ref().unwrap().body.clone();
    assert!(
        body.contains("別のGLB") || body.contains("一致しません"),
        "{body}"
    );
}

#[test]
fn flow_script_clicks_hit_different_parts() {
    let dir = scratch("hits");
    let (mut app, _ctx, _frame, _) = open_bytes(&dir, "sample.glb", &sample_glb());
    let session = app.session_mut();
    for viewport in [[800, 600], [1280, 800], [1600, 1000]] {
        for ppp in [1.0, 2.0] {
            replay_camera(session, viewport, ppp);
            let center = match click(session, CENTER) {
                Some(id) => id,
                None => panic!("中心が当たらない {viewport:?} ppp={ppp}"),
            };
            let note = match click(session, NOTE) {
                Some(id) => id,
                None => panic!(
                    "注釈位置が当たらない {viewport:?} ppp={ppp} 中心={}",
                    node_name(session, center)
                ),
            };
            assert_ne!(
                center,
                note,
                "同じ部品 {} / {}",
                node_name(session, center),
                node_name(session, note)
            );
            session.apply(Command::Select(Some(center)));
            session.apply(Command::HideSelected);
            let note_after = click(session, NOTE);
            assert_eq!(
                note_after,
                Some(note),
                "隠したあとに {} へ当たらない。今は {}",
                node_name(session, note),
                note_after
                    .map(|id| node_name(session, id))
                    .unwrap_or_else(|| "なし".to_owned())
            );
            session.apply(Command::ShowAll);
        }
    }
}

#[test]
fn flow_scripts_parse() {
    let out = scratch("scripts");
    for name in ["flow-1.json", "flow-2.json"] {
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
        let automation = model_reviewer::automation::Automation::from_launch(&launch);
        assert!(
            automation.script_error().is_none(),
            "{name}: {}",
            automation.script_error().unwrap_or_default()
        );
    }
}
