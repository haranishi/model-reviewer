//! 画面の確認用。`UI_SHOT=before` のように接頭辞を付け、`--ignored` でだけ描く。
//! アプリ本体は起動しない。

use std::sync::Arc;

use egui_kittest::kittest::Queryable;
use model_reviewer::session::{Draft, Session};
use model_reviewer_core::camera::OrbitCamera;
use model_reviewer_core::identity::{ModelIdentity, ModelRef};
use model_reviewer_core::pick::{PickIndex, pick};
use model_reviewer_core::review::{Annotation, ReviewState};
use model_reviewer_core::scene::NodeId;

#[test]
#[ignore = "wgpuで画面をPNGへ書き出す"]
fn render_chrome_for_visual_check() {
    let wide = egui::vec2(1280.0, 800.0);
    let narrow = egui::vec2(960.0, 600.0);

    let mut details = harness(wide, sample_session());
    reveal_last(&mut details);
    save(&mut details, "1280-details");

    let mut details_narrow = harness(narrow, sample_session());
    reveal_last(&mut details_narrow);
    save(&mut details_narrow, "960-details");

    let mut help = harness(wide, sample_session());
    reveal_last(&mut help);
    model_reviewer::ui::help::toggle(&help.ctx);
    settle(&mut help);
    save(&mut help, "1280-help");

    let mut help_narrow = harness(narrow, sample_session());
    reveal_last(&mut help_narrow);
    model_reviewer::ui::help::toggle(&help_narrow.ctx);
    settle(&mut help_narrow);
    save(&mut help_narrow, "960-help");

    let mut hover = harness(wide, sample_session());
    reveal_last(&mut hover);
    hover.get_by_label("開く").hover();
    settle(&mut hover);
    save(&mut hover, "1280-hover");

    let mut pressed = harness(wide, sample_session());
    reveal_last(&mut pressed);
    let open = pressed.get_by_label("開く");
    pressed.drag_at(open.rect().center());
    // 押した色は次のフレームで付く。長く押したままだとクリック期限が切れて通常色に戻る。
    pressed.step();
    pressed.step();
    save(&mut pressed, "1280-pressed");

    let mut disabled = harness(wide, sample_session());
    settle(&mut disabled);
    save(&mut disabled, "1280-disabled");

    for (size, tag) in [(wide, "1280"), (narrow, "960")] {
        let mut empty = harness(size, Session::default());
        settle(&mut empty);
        save(&mut empty, &format!("{tag}-empty"));

        let mut notes = harness(size, notes_session(size));
        settle(&mut notes);
        notes.event(egui::Event::Text(
            "左の脚の付け根に隙間がある。図面では2mmなので、組み立て後に測り直して報告してほしい。塗装の前に直すこと。"
                .to_owned(),
        ));
        settle(&mut notes);
        save(&mut notes, &format!("{tag}-notes"));
        notes.state_mut().draft = None;
        settle(&mut notes);
        save(&mut notes, &format!("{tag}-markers"));

        let mut tree = harness(size, parent_hidden_session());
        settle(&mut tree);
        save(&mut tree, &format!("{tag}-tree"));
    }
}

/// 手前の面の印、その奥に隠れた印、長い本文の下書きを置いた状態。
fn notes_session(size: egui::Vec2) -> Session {
    let mut session = sample_session();
    let model = Arc::clone(session.model.as_ref().unwrap());
    let index = PickIndex::build(&model);
    let aspect = (size.x - 560.0) / (size.y - 64.0 - 280.0);
    session.camera = OrbitCamera::home(&model.bounds, aspect);
    let vis = session.review.as_ref().unwrap().visibility.clone();
    let mut hits = Vec::new();
    for ndc in [
        glam::Vec2::new(0.0, 0.0),
        glam::Vec2::new(-0.2, 0.1),
        glam::Vec2::new(0.15, -0.1),
    ] {
        let ray = session.camera.screen_ray(ndc, aspect);
        if let Some(hit) = pick(&model, &index, &vis, &ray) {
            hits.push((hit, ray));
        }
    }
    assert!(hits.len() >= 2, "見本の面に当たる");
    let review = session.review.as_mut().unwrap();
    let size_of = (model.bounds.max - model.bounds.min).length();
    let (front, _) = hits[0];
    review.annotations.push(Annotation {
        id: 1,
        node: front.node,
        local_pos: front.local_pos,
        local_normal: front.local_normal,
        text: "手前".to_owned(),
    });
    // 別の光線の奥に置き、手前の印と画面上で重ならないようにする。
    let (far, far_ray) = hits[hits.len() - 1];
    let behind = far.world_pos + far_ray.dir * size_of * 0.2;
    let inverse = model.nodes[front.node.0 as usize].world.inverse();
    review.annotations.push(Annotation {
        id: 2,
        node: front.node,
        local_pos: inverse.transform_point3(behind),
        local_normal: front.local_normal,
        text: "奥".to_owned(),
    });
    let (draft, _) = hits[1];
    session.draft = Some(Draft {
        node: draft.node,
        local_pos: draft.local_pos,
        local_normal: draft.local_normal,
        text: String::new(),
    });
    session.pick_index = Some(index);
    session
}

/// 子を持つ部品を1つ隠し、その子が「親により非表示」になった状態。
fn parent_hidden_session() -> Session {
    let mut session = sample_session();
    let model = Arc::clone(session.model.as_ref().unwrap());
    let parent = (1..model.nodes.len())
        .find(|&i| model.nodes[i].subtree_end as usize > i + 1)
        .expect("子を持つ部品");
    session
        .review
        .as_mut()
        .unwrap()
        .visibility
        .set(&model, NodeId(parent as u32), false);
    session
}

fn sample_session() -> Session {
    let bytes = model_reviewer_testdata::sample_glb();
    let loaded = model_reviewer_gltf::load_glb(&bytes).expect("見本のGLB");
    let model = Arc::new(loaded);
    let mut session = Session::default();
    session.review = Some(ReviewState::new(&model));
    session.model_ref = Some(ModelRef {
        file_name: "sample.glb".to_owned(),
        path: "sample.glb".to_owned(),
        relative_path: None,
        identity: ModelIdentity::from_bytes(&bytes),
    });
    session.set_model(model);
    session.panel_details = true;
    session
}

fn reveal_last(harness: &mut egui_kittest::Harness<'static, Session>) {
    settle(harness);
    harness.state_mut().selection = Some(NodeId(11));
    settle(harness);
}

fn settle(harness: &mut egui_kittest::Harness<'static, Session>) {
    for _ in 0..8 {
        harness.step();
    }
}

fn harness(size: egui::Vec2, session: Session) -> egui_kittest::Harness<'static, Session> {
    egui_kittest::Harness::builder()
        .with_size(size)
        .with_pixels_per_point(2.0)
        .wgpu()
        .with_accessibility_check(false)
        .allow_missing_glyphs()
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
        )
}

fn save(harness: &mut egui_kittest::Harness<'static, Session>, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-check");
    std::fs::create_dir_all(&dir).expect("画像の出力先");
    let tag = std::env::var("UI_SHOT").unwrap_or_else(|_| "shot".to_owned());
    let path = dir.join(format!("{tag}-{name}.png"));
    let image = harness.render().expect("描画");
    image.save(&path).expect("PNGの保存");
    eprintln!("wrote target/ui-check/{tag}-{name}.png");
}
