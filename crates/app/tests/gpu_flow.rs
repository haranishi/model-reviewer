//! 通しの操作のあと、3D領域が背景一色でないこと。実行にはGPUが要る。

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use model_reviewer::App;
use model_reviewer::automation::WidgetFrame;
use model_reviewer::command::Command;
use model_reviewer::gpu_view::scene_image;
use model_reviewer::loader::LoadPhase;
use model_reviewer::ui::ids;
use model_reviewer_testdata::sample_glb;

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("gpu-flow")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
#[ignore = "GPUが必要"]
fn gpu_flow_paints_more_than_the_clear_color() {
    let dir = scratch("paint");
    let glb = dir.join("sample.glb");
    std::fs::write(&glb, sample_glb()).unwrap();
    let launch = model_reviewer::parse_args(["model-reviewer"]).unwrap();
    let mut harness = egui_kittest::Harness::builder()
        .wgpu()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_max_steps(800)
        .with_size(egui::vec2(1100.0, 720.0))
        .with_context_setup(model_reviewer::fonts::install)
        .build_eframe(|cc| App::new(cc, launch));
    harness
        .state_mut()
        .session_mut()
        .apply(Command::DroppedFiles(vec![glb]));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        harness.step();
        let ready = {
            let session = harness.state().session();
            matches!(session.load, LoadPhase::Idle)
                && session.pick_index.is_some()
                && scene_image(&harness.ctx).is_some()
        };
        if ready {
            break;
        }
        if Instant::now() > deadline {
            let session = harness.state().session();
            panic!(
                "3Dが出ません status={} phase={:?}",
                session.status_text,
                session.load.label()
            );
        }
        thread::sleep(Duration::from_millis(2));
    }
    harness.state_mut().session_mut().apply(Command::ViewClick {
        ndc: glam::Vec2::ZERO,
    });
    harness.step();
    if harness.state().session().selection.is_some() {
        harness
            .state_mut()
            .session_mut()
            .apply(Command::HideSelected);
    }
    harness
        .state_mut()
        .session_mut()
        .apply(Command::SetAnnotateMode(true));
    harness.state_mut().session_mut().apply(Command::ViewClick {
        ndc: glam::Vec2::new(0.04, -0.2),
    });
    if harness.state().session().draft.is_some() {
        harness
            .state_mut()
            .session_mut()
            .apply(Command::CommitDraft {
                text: "先端".to_owned(),
            });
    }
    harness.state_mut().session_mut().apply(Command::SaveView {
        name: "確認".to_owned(),
    });
    harness
        .state_mut()
        .session_mut()
        .apply(Command::SaveReview(dir.join("review.json")));
    harness.step();
    let image = harness.render().expect("描画");
    let frame = harness
        .ctx
        .data(|data| data.get_temp::<WidgetFrame>(egui::Id::NULL))
        .unwrap_or_default();
    let entry = frame
        .entries()
        .iter()
        .find(|entry| entry.name == ids::VIEWPORT)
        .expect("3D領域");
    let ppp = harness.ctx.pixels_per_point();
    let x0 = (entry.min[0] * ppp).round().max(0.0) as u32;
    let y0 = (entry.min[1] * ppp).round().max(0.0) as u32;
    let x1 = ((entry.max[0] * ppp).round() as u32).min(image.width());
    let y1 = ((entry.max[1] * ppp).round() as u32).min(image.height());
    assert!(x1 > x0 + 8 && y1 > y0 + 8, "3D領域が小さすぎる");
    let mut colors = BTreeSet::new();
    let mut y = y0;
    while y < y1 {
        let mut x = x0;
        while x < x1 {
            let pixel = image.get_pixel(x, y).0;
            colors.insert([pixel[0], pixel[1], pixel[2]]);
            x += 8;
        }
        y += 8;
    }
    assert!(
        colors.len() > 1,
        "3D領域が単色です {colors:?} rect=({x0},{y0})-({x1},{y1})"
    );
}

/// 何も開いていない状態で、1回の転送の予算（32MiB）を超える形状を開く。
/// 描画側が「表示中のモデルが無い」と見て転送中の組まで消すと、読み込みが終わらない。
#[test]
#[ignore = "GPUが必要"]
fn first_model_larger_than_one_upload_step_finishes_loading() {
    let dir = scratch("large-first");
    let glb = dir.join("large.glb");
    std::fs::write(
        &glb,
        model_reviewer_testdata::stress_glb(1, 600_000).unwrap(),
    )
    .unwrap();
    let launch = model_reviewer::parse_args(["model-reviewer"]).unwrap();
    let mut harness = egui_kittest::Harness::builder()
        .wgpu()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_size(egui::vec2(1100.0, 720.0))
        .with_context_setup(model_reviewer::fonts::install)
        .build_eframe(|cc| App::new(cc, launch));
    harness
        .state_mut()
        .session_mut()
        .apply(Command::OpenModel(glb));
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        harness.step();
        let session = harness.state().session();
        if matches!(session.load, LoadPhase::Idle)
            && session.model.is_some()
            && session.pick_index.is_some()
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "読み込みが終わりません: {:?}",
            session.load.label()
        );
        thread::sleep(Duration::from_millis(2));
    }
    let frames = harness.state().session().metrics.upload_frames;
    assert!(
        frames.is_some_and(|n| n > 1),
        "転送が分割されていません {frames:?}"
    );
}

/// 何も開いていない状態から32MiB超のモデルを開き、転送が終わった直後に変形と取り消しを行う。
/// ドラッグ中は定数しか送らず、確定は動かした範囲の部分更新、取り消しで元の行列に戻る。
#[test]
#[ignore = "GPUが必要"]
fn transform_and_undo_right_after_a_large_first_upload() {
    use model_reviewer::command::{Handle, Tool};
    use model_reviewer::session::{GIZMO_SIZE, gizmo_frame};
    use model_reviewer::ui::gizmo_view::grab_points;
    use model_reviewer_core::scene::NodeId;

    let dir = scratch("large-edit");
    let glb = dir.join("large.glb");
    std::fs::write(
        &glb,
        model_reviewer_testdata::stress_glb(1, 600_000).unwrap(),
    )
    .unwrap();
    let launch = model_reviewer::parse_args(["model-reviewer"]).unwrap();
    let mut harness = egui_kittest::Harness::builder()
        .wgpu()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_size(egui::vec2(1100.0, 720.0))
        .with_context_setup(model_reviewer::fonts::install)
        .build_eframe(|cc| App::new(cc, launch));
    harness
        .state_mut()
        .session_mut()
        .apply(Command::OpenModel(glb));
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut tried_during_upload = 0;
    loop {
        harness.step();
        let session = harness.state().session();
        if matches!(session.load, LoadPhase::Idle) && session.model.is_some() {
            break;
        }
        // 転送の途中に図形の追加・取り消しを送っても、文書も履歴もできない（断られる）。
        if matches!(session.load, LoadPhase::Uploading { .. }) {
            let session = harness.state_mut().session_mut();
            session.apply(Command::AddShape(
                model_reviewer_core::shapes::ShapeKind::Cube,
            ));
            session.apply(Command::Undo);
            assert!(session.doc.is_none() || session.history.is_empty());
            tried_during_upload += 1;
        }
        assert!(Instant::now() < deadline, "読み込みが終わりません");
        thread::sleep(Duration::from_millis(2));
    }
    assert!(tried_during_upload > 0, "転送の途中を通っていない");
    {
        let session = harness.state().session();
        assert!(session.history.is_empty(), "転送中の操作が履歴に入った");
        assert_eq!(
            session.doc.as_ref().unwrap().nodes().len(),
            session.model.as_ref().unwrap().nodes.len(),
            "転送中に図形が足された"
        );
    }
    assert!(
        harness
            .state()
            .session()
            .metrics
            .upload_frames
            .is_some_and(|n| n > 1),
        "転送が分割されていない"
    );
    // 転送の直後（当たり判定の索引を待たない）。部品を選んでハンドルを掴む。
    let part = {
        let session = harness.state().session();
        let doc = session.doc.as_ref().unwrap();
        (0..doc.nodes().len() as u32)
            .map(NodeId)
            .find(|id| !doc.node(*id).unwrap().draws.is_empty())
            .expect("形のある部品")
    };
    let before = harness
        .state()
        .session()
        .doc
        .as_ref()
        .unwrap()
        .node(part)
        .unwrap()
        .local;
    {
        let session = harness.state_mut().session_mut();
        session.apply(Command::Select(Some(part)));
        session.apply(Command::SetTool(Tool::Move));
    }
    harness.step();
    let start = {
        let session = harness.state().session();
        let [w, h] = session.viewport_px;
        let size = [w as f32, h as f32];
        let frame = gizmo_frame(session, size, GIZMO_SIZE * session.pixels_per_point)
            .expect("ハンドルの枠");
        let (_, px) = grab_points(Tool::Move, &frame)
            .into_iter()
            .find(|(handle, _)| *handle == Handle::MoveY)
            .expect("Y の矢印");
        glam::Vec2::new(px.x / size[0] * 2.0 - 1.0, 1.0 - px.y / size[1] * 2.0)
    };
    harness
        .state_mut()
        .session_mut()
        .apply(Command::BeginTransform {
            handle: Handle::MoveY,
            ndc: start,
        });
    assert!(harness.state().session().transform.is_some(), "掴めた");
    harness.step();
    let sent_before = harness.state().gpu_stats().unwrap().uploaded_bytes;
    for i in 1..=10 {
        harness
            .state_mut()
            .session_mut()
            .apply(Command::DragTransform {
                ndc: start + glam::Vec2::new(0.0, 0.01 * i as f32),
            });
        harness.step();
    }
    let stats = harness.state().gpu_stats().unwrap();
    let sent = stats.uploaded_bytes - sent_before;
    assert!(
        sent <= 10 * 1024,
        "ドラッグ中に {sent} バイト送った（定数だけのはず）"
    );
    let patches = stats.instance_patches;
    harness
        .state_mut()
        .session_mut()
        .apply(Command::CommitTransform);
    harness.step();
    let session = harness.state().session();
    assert_eq!(session.history.len(), 1);
    assert_ne!(
        session.doc.as_ref().unwrap().node(part).unwrap().local,
        before
    );
    assert!(
        harness.state().gpu_stats().unwrap().instance_patches > patches,
        "確定は部分更新"
    );
    harness.state_mut().session_mut().apply(Command::Undo);
    harness.step();
    let session = harness.state().session();
    assert_eq!(
        session.doc.as_ref().unwrap().node(part).unwrap().local,
        before
    );
    assert!(session.status_text.contains("戻しました"));
    assert!(scene_image(&harness.ctx).is_some());

    // 続けて図形を足し、取り消し、やり直し、書き出す（大きな転送の直後でも番号がそろう）。
    let nodes = harness
        .state()
        .session()
        .doc
        .as_ref()
        .unwrap()
        .nodes()
        .len();
    harness.state_mut().session_mut().apply(Command::AddShape(
        model_reviewer_core::shapes::ShapeKind::Cube,
    ));
    harness.step();
    assert_eq!(
        harness
            .state()
            .session()
            .doc
            .as_ref()
            .unwrap()
            .nodes()
            .len(),
        nodes + 1
    );
    harness.state_mut().session_mut().apply(Command::Undo);
    harness.step();
    harness.state_mut().session_mut().apply(Command::Redo);
    harness.step();
    let out = dir.join("edited.glb");
    harness
        .state_mut()
        .session_mut()
        .apply(Command::ExportGlb(out.clone()));
    let deadline = Instant::now() + Duration::from_secs(60);
    while harness.state().session().is_exporting() {
        harness.step();
        assert!(Instant::now() < deadline, "書き出しが終わりません");
        thread::sleep(Duration::from_millis(5));
    }
    let session = harness.state().session();
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );
    assert!(!session.doc_unsaved());
    assert!(out.is_file());
}

fn wait_ready(harness: &mut egui_kittest::Harness<'_, App>) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        harness.step();
        let ready = {
            let session = harness.state().session();
            matches!(session.load, LoadPhase::Idle)
                && session.pick_index.is_some()
                && scene_image(&harness.ctx).is_some()
        };
        if ready {
            return;
        }
        assert!(Instant::now() < deadline, "3Dが出ません");
        thread::sleep(Duration::from_millis(2));
    }
}

/// 3D領域の中央付近（8×8画素）の平均色。
fn center_color(harness: &mut egui_kittest::Harness<'_, App>) -> [f32; 3] {
    let image = harness.render().expect("描画");
    let frame = harness
        .ctx
        .data(|data| data.get_temp::<WidgetFrame>(egui::Id::NULL))
        .unwrap_or_default();
    let entry = frame
        .entries()
        .iter()
        .find(|entry| entry.name == ids::VIEWPORT)
        .expect("3D領域");
    let ppp = harness.ctx.pixels_per_point();
    let cx = ((entry.min[0] + entry.max[0]) * 0.5 * ppp) as u32;
    let cy = ((entry.min[1] + entry.max[1]) * 0.5 * ppp) as u32;
    let mut sum = [0.0_f32; 3];
    for y in cy - 4..cy + 4 {
        for x in cx - 4..cx + 4 {
            let pixel = image.get_pixel(x, y).0;
            for c in 0..3 {
                sum[c] += f32::from(pixel[c]) / 64.0;
            }
        }
    }
    sum
}

/// 足した図形が描かれ、取り消すと消え、やり直すと形状を送り直さずに戻る。
#[test]
#[ignore = "GPUが必要"]
fn added_shape_is_drawn_and_undo_removes_it() {
    let dir = scratch("shape");
    let glb = dir.join("sample.glb");
    std::fs::write(&glb, sample_glb()).unwrap();
    let launch = model_reviewer::parse_args(["model-reviewer"]).unwrap();
    let mut harness = egui_kittest::Harness::builder()
        .wgpu()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_max_steps(800)
        .with_size(egui::vec2(1100.0, 720.0))
        .with_context_setup(model_reviewer::fonts::install)
        .build_eframe(|cc| App::new(cc, launch));
    harness
        .state_mut()
        .session_mut()
        .apply(Command::DroppedFiles(vec![glb]));
    wait_ready(&mut harness);
    // 何も無い所（床より上）を見る。
    {
        let session = harness.state_mut().session_mut();
        session.camera.target = glam::Vec3::new(8.0, 1.0, 0.0);
        session.apply(Command::SetGrid(false));
        session.apply(Command::Orbit(glam::Vec2::new(1.0, 0.0)));
    }
    harness.step();
    let empty = center_color(&mut harness);
    let sent = harness.state().gpu_stats().unwrap().uploaded_bytes;
    harness.state_mut().session_mut().apply(Command::AddShape(
        model_reviewer_core::shapes::ShapeKind::Cube,
    ));
    harness.step();
    harness.step();
    let with_cube = center_color(&mut harness);
    let sent_add = harness.state().gpu_stats().unwrap().uploaded_bytes - sent;
    let diff = |a: [f32; 3], b: [f32; 3]| (0..3).map(|c| (a[c] - b[c]).abs()).sum::<f32>();
    assert!(
        diff(empty, with_cube) > 30.0,
        "図形が描かれていない: 空 {empty:?} 図形 {with_cube:?}"
    );
    assert!(sent_add > 0, "形状を送った");
    assert!(harness.state().session().selection.is_some());

    harness.state_mut().session_mut().apply(Command::Undo);
    harness.step();
    harness.step();
    let undone = center_color(&mut harness);
    assert!(
        diff(empty, undone) < 3.0,
        "取り消すと消える {empty:?} {undone:?}"
    );

    let before_redo = harness.state().gpu_stats().unwrap().uploaded_bytes;
    harness.state_mut().session_mut().apply(Command::Redo);
    harness.step();
    harness.step();
    let redone = center_color(&mut harness);
    assert!(diff(with_cube, redone) < 3.0, "やり直すと戻る");
    let resent = harness.state().gpu_stats().unwrap().uploaded_bytes - before_redo;
    assert!(
        resent < sent_add,
        "やり直しで形状を送り直さない（インスタンスだけ）: {resent} / {sent_add}"
    );

    // 複製した図形も描かれる（形状を共有し、送り足さない）。
    let shape = harness.state().session().selection.unwrap();
    let prims = harness
        .state()
        .session()
        .doc
        .as_ref()
        .unwrap()
        .added_primitives()
        .len();
    harness
        .state_mut()
        .session_mut()
        .apply(Command::Select(Some(shape)));
    harness
        .state_mut()
        .session_mut()
        .apply(Command::DuplicateSelected);
    harness.step();
    assert_eq!(
        harness
            .state()
            .session()
            .doc
            .as_ref()
            .unwrap()
            .added_primitives()
            .len(),
        prims
    );
    assert!(harness.state().gpu_notice().is_none());
}

/// 断面: 入れると描き直し、送るのは定数だけ（形状とインスタンスは0バイト）。全部を消す位置では
/// 中央が背景になり、外すと元の絵に戻る。何もしないフレームでは描かない。
#[test]
#[ignore = "GPUが必要"]
fn section_redraws_with_constants_only() {
    use model_reviewer_core::scene::SceneRead;
    use model_reviewer_core::section::{Axis, SectionPlane};
    let dir = scratch("section");
    let glb = dir.join("sample.glb");
    std::fs::write(&glb, sample_glb()).unwrap();
    let launch = model_reviewer::parse_args(["model-reviewer"]).unwrap();
    let mut harness = egui_kittest::Harness::builder()
        .wgpu()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_max_steps(800)
        .with_size(egui::vec2(1100.0, 720.0))
        .with_context_setup(model_reviewer::fonts::install)
        .build_eframe(|cc| App::new(cc, launch));
    harness
        .state_mut()
        .session_mut()
        .apply(Command::DroppedFiles(vec![glb]));
    wait_ready(&mut harness);
    harness
        .state_mut()
        .session_mut()
        .apply(Command::SetGrid(false));
    harness.step();
    harness.step();
    let whole = center_color(&mut harness);
    let before = harness.state().gpu_stats().unwrap();
    // 何もしないフレームでは描かない。
    harness.step();
    assert_eq!(
        harness.state().gpu_stats().unwrap().draw_count,
        before.draw_count
    );
    // 視点の手前の軸で、全体より手前の位置まで消す（反転で、位置より大きい側を残す）。
    let bounds = harness.state().session().doc.as_ref().unwrap().bounds();
    harness
        .state_mut()
        .session_mut()
        .apply(Command::SetSection(Some(SectionPlane {
            axis: Axis::Y,
            offset: bounds.max.y,
            flipped: true,
        })));
    harness.step();
    harness.step();
    let cut = harness.state().gpu_stats().unwrap();
    assert!(cut.draw_count > before.draw_count, "断面で描き直す");
    assert_eq!(
        cut.uploaded_bytes, before.uploaded_bytes,
        "形状とインスタンスは送らない"
    );
    assert_eq!(cut.instance_rebuilds, before.instance_rebuilds);
    assert!(cut.frame_bytes > before.frame_bytes, "定数だけ送る");
    let empty = center_color(&mut harness);
    let diff = |a: [f32; 3], b: [f32; 3]| (0..3).map(|c| (a[c] - b[c]).abs()).sum::<f32>();
    assert!(diff(whole, empty) > 30.0, "消えた: {whole:?} → {empty:?}");
    harness
        .state_mut()
        .session_mut()
        .apply(Command::SetSection(None));
    harness.step();
    harness.step();
    let back = center_color(&mut harness);
    assert!(diff(whole, back) < 3.0, "外すと元の絵: {whole:?} {back:?}");
    assert!(harness.state().gpu_notice().is_none());
}

/// 新規作成（第5段階）: 部品0個の空のモデルの転送が終わり、図形を足して描ける。全部消しても、
/// もう一度置いても落ちない。開いていたモデルの形状は解放される。
#[test]
#[ignore = "GPUが必要"]
fn empty_model_uploads_and_accepts_shapes() {
    let dir = scratch("new-doc");
    let glb = dir.join("sample.glb");
    std::fs::write(&glb, sample_glb()).unwrap();
    let launch = model_reviewer::parse_args(["model-reviewer"]).unwrap();
    let mut harness = egui_kittest::Harness::builder()
        .wgpu()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_max_steps(800)
        .with_size(egui::vec2(1100.0, 720.0))
        .with_context_setup(model_reviewer::fonts::install)
        .build_eframe(|cc| App::new(cc, launch));
    // 先にモデルを開いておき、新規作成で入れ替わることも確かめる。
    harness
        .state_mut()
        .session_mut()
        .apply(Command::DroppedFiles(vec![glb]));
    wait_ready(&mut harness);
    harness
        .state_mut()
        .session_mut()
        .apply(Command::NewDocument);
    wait_ready(&mut harness);
    {
        let session = harness.state().session();
        assert!(session.is_new_document(), "status={}", session.status_text);
        assert!(session.doc.as_ref().unwrap().nodes().is_empty());
        assert_eq!(session.status_text, "新規の文書を作りました");
    }
    assert!(harness.state().gpu_notice().is_none());
    harness
        .state_mut()
        .session_mut()
        .apply(Command::SetGrid(false));
    harness.step();
    harness.step();
    let empty = center_color(&mut harness);
    let sent = harness.state().gpu_stats().unwrap().uploaded_bytes;
    // 視点は原点を見ている。1つ目の図形は原点に大きさ 1 で置かれ、画面の中央に映る。
    harness.state_mut().session_mut().apply(Command::AddShape(
        model_reviewer_core::shapes::ShapeKind::Cube,
    ));
    harness.step();
    harness.step();
    let with_cube = center_color(&mut harness);
    let diff = |a: [f32; 3], b: [f32; 3]| (0..3).map(|c| (a[c] - b[c]).abs()).sum::<f32>();
    assert!(
        diff(empty, with_cube) > 30.0,
        "図形が描かれていない: 空 {empty:?} 図形 {with_cube:?}"
    );
    assert!(harness.state().gpu_stats().unwrap().uploaded_bytes > sent);
    // 全部消すと空の画面に戻り、もう一度置ける。
    harness
        .state_mut()
        .session_mut()
        .apply(Command::DeleteSelected);
    harness.step();
    harness.step();
    let cleared = center_color(&mut harness);
    assert!(diff(empty, cleared) < 3.0, "消すと空 {empty:?} {cleared:?}");
    harness.state_mut().session_mut().apply(Command::AddShape(
        model_reviewer_core::shapes::ShapeKind::Sphere,
    ));
    harness.step();
    harness.step();
    let with_sphere = center_color(&mut harness);
    assert!(diff(empty, with_sphere) > 30.0, "2つ目も描かれる");
    assert!(harness.state().gpu_notice().is_none());
}

/// 同じ立方体を左右に置き、右の色を変えた前後で3Dビューの画素を比べる。
#[test]
#[ignore = "GPUが必要"]
fn recolor_changes_only_that_part() {
    use glam::Vec3;
    use model_reviewer::command::{Projection, ShapeKind, Tool, Trs, ViewDir};

    let launch = model_reviewer::parse_args(["model-reviewer"]).unwrap();
    let mut h = egui_kittest::Harness::builder()
        .wgpu()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_size(egui::vec2(1280.0, 800.0))
        .with_context_setup(model_reviewer::fonts::install)
        .build_eframe(|cc| App::new(cc, launch));
    h.state_mut().session_mut().apply(Command::NewDocument);
    wait_ready(&mut h);
    let session = h.state_mut().session_mut();
    session.apply(Command::SetGrid(false));
    session.apply(Command::SetProjection(Projection::Orthographic));
    session.apply(Command::SnapView(ViewDir::Front));
    session.apply(Command::AddShape(ShapeKind::Cube));
    session.apply(Command::CommitLocal(Trs {
        translation: Vec3::X * -0.8,
        ..Trs::default()
    }));
    session.apply(Command::DuplicateSelected);
    session.apply(Command::CommitLocal(Trs {
        translation: Vec3::X * 0.8,
        ..Trs::default()
    }));
    session.apply(Command::SetTool(Tool::Select));
    h.run_steps(4);
    let before = h.render().unwrap();
    let stats = h.state().gpu_stats().unwrap();

    let widgets = h
        .ctx
        .data(|data| data.get_temp::<WidgetFrame>(egui::Id::NULL))
        .unwrap();
    let viewport = widgets
        .entries()
        .iter()
        .find(|e| e.name == ids::VIEWPORT)
        .unwrap();
    let view = egui::Rect::from_min_max(viewport.min.into(), viewport.max.into())
        * h.ctx.pixels_per_point();
    let project = |point| {
        let p = h
            .state()
            .session()
            .camera
            .project(point, view.aspect_ratio())
            .unwrap();
        view.center() + egui::vec2(p.x * view.width(), -p.y * view.height()) * 0.5
    };
    let right = egui::Rect::from_two_pos(
        project(Vec3::new(0.3, -0.5, 0.0)),
        project(Vec3::new(1.3, 0.5, 0.0)),
    )
    .expand(3.0);
    let left = project(Vec3::X * -0.8);
    let center_pixel = |p: egui::Pos2| before.get_pixel(p.x as u32, p.y as u32);
    assert_ne!(
        center_pixel(left),
        center_pixel(view.center()),
        "比較する左の立方体が見える"
    );

    h.state_mut()
        .session_mut()
        .apply(Command::SetColor([1.0, 0.0, 0.0]));
    h.run_steps(3);
    let after = h.render().unwrap();
    let mut changed = 0;
    for y in view.top().ceil() as u32..view.bottom() as u32 {
        for x in view.left().ceil() as u32..view.right() as u32 {
            let old = before.get_pixel(x, y);
            let new = after.get_pixel(x, y);
            if (0..3).any(|c| old[c].abs_diff(new[c]) > 2) {
                assert!(
                    right.contains(egui::pos2(x as f32, y as f32)),
                    "対象の外の画素 ({x}, {y})"
                );
                changed += 1;
            }
        }
    }
    assert!(changed > 100, "右の立方体の色が変わる");
    let painted = h.state().gpu_stats().unwrap();
    assert_eq!(painted.instance_patches, stats.instance_patches + 1);
    assert_eq!(painted.instance_rebuilds, stats.instance_rebuilds);
}
