//! 注釈・視点・性能値。GPUは使わない。

use std::sync::Arc;
use std::time::{Duration, Instant};

use egui_kittest::kittest::Queryable;
use glam::{Affine3A, Vec3};
use model_reviewer::command::Command;
use model_reviewer::metrics::{self, FrameClock, Metrics};
use model_reviewer::session::{Draft, Session};
use model_reviewer_core::math::Aabb;
use model_reviewer_core::review::ReviewState;
use model_reviewer_core::scene::{Model, ModelStats, Node, NodeId};
use model_reviewer_core::texture::Textures;

fn install(session: &mut Session, name: &str) {
    let model = Arc::new(Model {
        textures: Textures::default(),
        nodes: vec![Node {
            name: name.to_owned(),
            gltf_index: 0,
            parent: None,
            subtree_end: 1,
            depth: 0,
            local: Affine3A::IDENTITY,
            world: Affine3A::IDENTITY,
            draws: 0..0,
            world_aabb: Aabb::empty(),
        }],
        draws: Vec::new(),
        primitives: Vec::new(),
        bounds: Aabb::empty(),
        stats: ModelStats {
            nodes: 1,
            meshes: 2,
            unique_triangles: 9,
            placed_triangles: 11,
            ..ModelStats::default()
        },
        warnings: Vec::new(),
    });
    session.review = Some(ReviewState::new(&model));
    session.set_model(model);
}

fn draft_at(node: NodeId) -> Draft {
    Draft {
        node,
        local_pos: Vec3::new(1.0, 2.0, 3.0),
        local_normal: Vec3::Y,
        text: "下書き".to_owned(),
    }
}

fn harness(session: Session) -> egui_kittest::Harness<'static, Session> {
    egui_kittest::Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_size(egui::vec2(420.0, 480.0))
        .with_context_setup(|ctx| {
            model_reviewer::fonts::install(ctx);
            model_reviewer::theme::install(ctx);
        })
        .build_ui_state(
            |ui, session| {
                let mut commands = Vec::new();
                model_reviewer::ui::annotations::show(ui, session, &mut commands);
                model_reviewer::ui::views::show(ui, session, &mut commands);
                for command in commands {
                    session.apply(command);
                }
            },
            session,
        )
}

#[test]
fn annotations_commit_edit_delete_and_do_not_mark_selection_unsaved() {
    let mut session = Session::default();
    install(&mut session, "lamp");
    session.annotate_mode = true;
    session.draft = Some(draft_at(NodeId(0)));
    session.apply(Command::CommitDraft {
        text: "   ".to_owned(),
    });
    assert!(session.draft.is_some());
    assert!(!session.unsaved());
    assert!(session.review.as_ref().unwrap().annotations.is_empty());

    session.apply(Command::CommitDraft {
        text: "  きず  ".to_owned(),
    });
    let note = &session.review.as_ref().unwrap().annotations[0];
    assert_eq!(note.id, 1);
    assert_eq!(note.text, "きず");
    assert_eq!(note.local_pos, Vec3::new(1.0, 2.0, 3.0));
    assert!(session.draft.is_none());
    assert!(session.annotate_mode);
    assert!(session.unsaved());
    assert_eq!(session.selected_annotation, Some(1));
    assert_eq!(session.selection, Some(NodeId(0)));
    session.take_scene_dirty();

    session.mark_saved();
    session.apply(Command::EditAnnotation {
        id: 1,
        text: "きず".to_owned(),
    });
    assert!(!session.unsaved());
    session.apply(Command::EditAnnotation {
        id: 1,
        text: String::new(),
    });
    assert_eq!(session.review.as_ref().unwrap().annotations[0].text, "");
    assert!(session.unsaved());

    session.mark_saved();
    session.apply(Command::SelectAnnotation(Some(1)));
    assert!(!session.unsaved());
    assert_eq!(session.selection, Some(NodeId(0)));
    session.apply(Command::SelectAnnotation(Some(99)));
    assert_eq!(session.selected_annotation, Some(1));

    session.apply(Command::SetVisible {
        node: NodeId(0),
        visible: false,
    });
    assert_eq!(session.review.as_ref().unwrap().annotations.len(), 1);
    assert!(
        !session
            .review
            .as_ref()
            .unwrap()
            .visibility
            .effective(NodeId(0))
    );

    session.apply(Command::DeleteAnnotation(1));
    assert!(session.review.as_ref().unwrap().annotations.is_empty());
    assert!(session.selected_annotation.is_none());
    assert!(session.unsaved());
    session.mark_saved();
    session.apply(Command::DeleteAnnotation(1));
    assert!(!session.unsaved());

    session.draft = Some(draft_at(NodeId(0)));
    session.annotate_mode = true;
    session.apply(Command::SetAnnotateMode(false));
    assert!(session.draft.is_none());
    assert!(!session.annotate_mode);
    assert!(!session.unsaved());
}

#[test]
fn saved_views_round_trip_the_camera_without_marking_apply_unsaved() {
    let mut session = Session::default();
    install(&mut session, "lamp");
    session.apply(Command::SaveView {
        name: "   ".to_owned(),
    });
    assert!(session.review.as_ref().unwrap().views.is_empty());
    assert!(!session.unsaved());

    session.camera.yaw = 1.25;
    session.camera.distance = 4.5;
    session.camera.target.x = 3.0;
    let saved = session.camera;
    session.apply(Command::SaveView {
        name: " 正面 ".to_owned(),
    });
    assert_eq!(session.review.as_ref().unwrap().views[0].id, 1);
    assert_eq!(session.review.as_ref().unwrap().views[0].name, "正面");
    assert!(session.unsaved());

    session.camera.yaw = 0.2;
    session.mark_saved();
    session.take_scene_dirty();
    session.apply(Command::ApplyView(1));
    assert_eq!(session.camera, saved);
    let dirty = session.take_scene_dirty();
    assert!(dirty.camera);
    assert!(!dirty.instances);
    assert!(!session.unsaved());
    session.apply(Command::ApplyView(1));
    assert!(!session.take_scene_dirty().any());

    session.apply(Command::DeleteView(1));
    assert!(session.review.as_ref().unwrap().views.is_empty());
    assert!(session.unsaved());
}

#[test]
fn hidden_part_annotation_shows_its_name_and_can_be_deleted() {
    let mut session = Session::default();
    install(&mut session, "lamp");
    session.draft = Some(draft_at(NodeId(0)));
    session.apply(Command::CommitDraft {
        text: "きず".to_owned(),
    });
    session.apply(Command::SetVisible {
        node: NodeId(0),
        visible: false,
    });
    let mut harness = harness(session);
    harness.get_by_label("lamp");
    harness.get_by_label("非表示の部品");
    harness.get_by_label("削除").click();
    harness.step();
    assert!(
        harness
            .state()
            .review
            .as_ref()
            .unwrap()
            .annotations
            .is_empty()
    );
    harness.step();
    assert!(harness.query_by_label("非表示の部品").is_none());
}

#[test]
fn view_name_is_saved_from_the_field_and_apply_matches_the_camera() {
    let mut session = Session::default();
    install(&mut session, "lamp");
    session.camera.yaw = 0.8;
    session.camera.distance = 6.0;
    let saved = session.camera;
    let mut harness = harness(session);
    let name = harness.get_by_role(egui::accesskit::Role::TextInput);
    name.set_text_value_accesskit("正面");
    harness.step();
    harness.get_by_label("視点を保存").click();
    harness.step();
    assert_eq!(
        harness.state().review.as_ref().unwrap().views[0].name,
        "正面"
    );
    assert_eq!(
        harness.state().review.as_ref().unwrap().views[0].camera,
        saved
    );
    assert!(harness.state().unsaved());

    harness.state_mut().camera.yaw = 0.1;
    harness.state_mut().mark_saved();
    harness.step();
    harness.get_by_label("正面").click();
    harness.step();
    assert_eq!(harness.state().camera, saved);
    assert!(!harness.state().unsaved());

    harness.get_by_label("削除").click();
    harness.step();
    assert!(harness.state().review.as_ref().unwrap().views.is_empty());
    assert!(harness.state().unsaved());
}

#[test]
fn missing_metrics_say_unmeasured_and_memory_is_omitted() {
    let lines = metrics::report_lines(&Metrics::default(), None);
    for line in &lines {
        assert_ne!(line.value, "0");
        assert_ne!(line.value, "0 ms");
        assert_ne!(line.value, "0.0 ms");
        let text = metrics::format_line(line);
        assert!(text.contains(line.about), "{text}");
        assert!(text.contains('（'));
    }
    assert!(
        lines
            .iter()
            .any(|line| line.label == "GPUの処理時間" && line.value == "未計測")
    );
    assert!(
        lines
            .iter()
            .any(|line| line.label == "フレームのCPU時間" && line.value == "未計測")
    );
    assert!(
        lines
            .iter()
            .any(|line| line.label == "フレーム間隔" && line.value == "未計測")
    );
    assert!(
        lines
            .iter()
            .any(|line| line.label == "ファイルサイズ" && line.value == "未計測")
    );
    assert!(lines.iter().any(|line| {
        line.label == "ノード数／メッシュ数／三角形数" && line.value == "未計測"
    }));
    assert!(lines.iter().all(|line| line.label != "メモリ"));

    let harness = egui_kittest::Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_size(egui::vec2(640.0, 480.0))
        .with_context_setup(model_reviewer::fonts::install)
        .build_ui(|ui| {
            let session = Session::default();
            let mut commands = Vec::new();
            model_reviewer::ui::details::show(ui, &session, &mut commands);
        });
    assert!(
        harness
            .query_all_by_label_contains("未計測")
            .next()
            .is_some()
    );
    harness.get_by_label_contains("GPUの処理時間:");
    assert!(harness.query_by_label_contains("メモリ").is_none());
    assert!(harness.query_by_label_contains("0.0 ms").is_none());

    let mut session = Session::default();
    install(&mut session, "lamp");
    session.metrics.physical_mem = Some(1234);
    session.metrics.file_bytes = Some(20);
    let counts = session
        .model
        .as_ref()
        .map(|model| metrics::Counts::from_stats(model.stats));
    let lines = metrics::report_lines(&session.metrics, counts);
    let memory = lines.iter().find(|line| line.label == "メモリ").unwrap();
    assert_eq!(memory.value, "1234 バイト");
    assert!(memory.about.contains("physical_mem"));
    let counts = lines
        .iter()
        .find(|line| line.label == "ノード数／メッシュ数／三角形数")
        .unwrap();
    assert!(counts.value.contains('1'));
    assert!(counts.value.contains("配置後 11"));
    assert!(
        lines
            .iter()
            .any(|line| line.label == "GPUの処理時間" && line.value == "未計測")
    );

    let harness = egui_kittest::Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_size(egui::vec2(720.0, 520.0))
        .with_context_setup(model_reviewer::fonts::install)
        .build_ui(move |ui| {
            let mut commands = Vec::new();
            model_reviewer::ui::details::show(ui, &session, &mut commands);
        });
    harness.get_by_label_contains("1234");
    harness.get_by_label_contains("physical_mem");
    harness.get_by_label_contains("配置後 11");
    assert!(
        harness
            .query_all_by_label_contains("未計測")
            .next()
            .is_some()
    );
}

#[test]
fn frame_clock_averages_continuous_gaps_and_keeps_a_window_of_120() {
    let mut clock = FrameClock::default();
    let mut metrics = Metrics::default();
    let start = Instant::now();
    clock.observe(start, Duration::from_millis(2), &mut metrics);
    assert!(metrics.frame_interval.is_none());
    assert_eq!(metrics.frame_cpu_max, Some(Duration::from_millis(2)));

    clock.observe(
        start + Duration::from_millis(10),
        Duration::from_millis(4),
        &mut metrics,
    );
    assert_eq!(metrics.frame_interval, Some(Duration::from_millis(10)));
    assert_eq!(metrics.frame_cpu_max, Some(Duration::from_millis(4)));

    clock.observe(
        start + Duration::from_millis(200),
        Duration::from_millis(1),
        &mut metrics,
    );
    assert_eq!(metrics.frame_interval, Some(Duration::from_millis(10)));
    clock.observe(
        start + Duration::from_millis(300),
        Duration::from_millis(1),
        &mut metrics,
    );
    assert_eq!(metrics.frame_interval, Some(Duration::from_millis(10)));
    clock.observe(
        start + Duration::from_millis(330),
        Duration::from_millis(1),
        &mut metrics,
    );
    assert_eq!(metrics.frame_interval, Some(Duration::from_millis(20)));
    assert_eq!(metrics.frame_interval_max, Some(Duration::from_millis(30)));

    let mut clock = FrameClock::default();
    let mut metrics = Metrics::default();
    clock.observe(start, Duration::from_millis(50), &mut metrics);
    for index in 1..=120 {
        clock.observe(
            start + Duration::from_millis(index),
            Duration::from_millis(1),
            &mut metrics,
        );
    }
    assert_eq!(metrics.frame_cpu_max, Some(Duration::from_millis(1)));
    assert_eq!(metrics.frame_cpu_avg, Some(Duration::from_millis(1)));
}

#[test]
fn details_lines_stay_above_the_status_bar() {
    for size in [egui::vec2(1280.0, 800.0), egui::vec2(960.0, 600.0)] {
        let mut session = Session::default();
        install(&mut session, "lamp");
        session.panel_details = true;
        let mut harness = egui_kittest::Harness::builder()
            .allow_missing_glyphs()
            .with_accessibility_check(false)
            .with_size(size)
            .with_context_setup(|ctx| {
                model_reviewer::fonts::install(ctx);
                model_reviewer::theme::install(ctx);
            })
            .build_ui_state(
                |ui, session| {
                    let mut commands = Vec::new();
                    model_reviewer::ui::show(ui, session, &mut commands);
                    for command in commands {
                        session.apply(command);
                    }
                },
                session,
            );
        for _ in 0..8 {
            harness.step();
        }
        let status = harness.get_by_label("詳細▴").rect();
        assert!(
            status.min.y > size.y * 0.5,
            "状態欄が下にありません {size:?} {status:?}"
        );
        assert!(status.max.y <= size.y + 1.0, "{status:?}");
        let first = harness.get_by_label_contains("ファイルサイズ:").rect();
        assert!(
            first.max.y <= status.min.y + 1.0,
            "先頭の行が状態欄に入っています size={size:?} line={first:?} status={status:?}"
        );
        // 後ろの行はパネルの中でスクロールして見る。重ならないことは、パネルの矩形で
        // tests/layout.rs が確かめる（行の矩形は切り取られた分も含むので使わない）。
    }
}
