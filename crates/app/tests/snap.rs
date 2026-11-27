//! 吸着する計測の接合: クリックの吸着、控えを使わないクリック、X、休み、候補の消え方。GPUは使わない。

mod support;

use std::time::Duration;

use glam::{Vec2, Vec3};
use model_reviewer::command::{Command, DialogButton, DialogRequest, Projection, Tool};
use model_reviewer::session::{Session, measure_delta, measure_distance, snap_rest_after};
use model_reviewer::ui::ids;
use model_reviewer_core::snap::SnapKind;
use support::{FAR_CENTER, NEAR_CENTER, close3, ndc_of, near};

/// 手前の立方体の、こちらを向いた面（z = 3）の角。
const CORNER_A: Vec3 = Vec3::new(1.0, 1.0, 3.0);
const CORNER_B: Vec3 = Vec3::new(-1.0, 1.0, 3.0);

fn measuring() -> Session {
    let mut session = support::session();
    session.apply(Command::SetTool(Tool::Measure));
    session
}

/// ポインタを動かす。先に休みを明ける。遅い実行環境（デバッグ構成の CI）では1回の探索が
/// 8ミリ秒を超えて休みに入り、続く移動の候補が出なくなるため。休みの動きは
/// `slow_search_backs_off` で確かめる。
fn hover(session: &mut Session, ndc: Vec2) {
    session.rest_snap_for(Duration::ZERO);
    session.apply(Command::ViewHover { ndc: Some(ndc) });
}

fn local_of(session: &Session, index: usize) -> Vec3 {
    let point = [session.measurement.first, session.measurement.second][index].unwrap();
    point.local
}

#[test]
fn measure_click_snaps_to_vertex() {
    // 吸着が入っている（既定）: 角の4画素内側を押すと、角そのものが点になる。
    let mut session = measuring();
    assert!(session.snap.enabled, "既定で吸着は入");
    let a = ndc_of(&session, CORNER_A, -4.0, -4.0);
    let b = ndc_of(&session, CORNER_B, 4.0, -4.0);
    session.apply(Command::ViewClick { ndc: a });
    session.apply(Command::ViewClick { ndc: b });
    assert_eq!(session.measurement.kinds, [SnapKind::Vertex; 2]);
    assert_eq!(
        local_of(&session, 0),
        CORNER_A - NEAR_CENTER,
        "頂点の座標そのもの"
    );
    assert_eq!(local_of(&session, 1), CORNER_B - NEAR_CENTER);
    assert!(near(measure_distance(&session).unwrap(), 2.0));
    let delta = measure_delta(&session).unwrap();
    assert!(close3(delta, Vec3::new(-2.0, 0.0, 0.0)), "{delta}");
    assert!(session.history.is_empty() && !session.has_unsaved());

    // 辺の近く（角から離れた所）は辺。
    let on_edge = Vec3::new(0.2, 1.0, 3.0);
    session.apply(Command::ClearMeasure);
    session.apply(Command::ViewClick {
        ndc: ndc_of(&session, on_edge, 0.0, -4.0),
    });
    assert_eq!(session.measurement.kinds[0], SnapKind::Edge);
    let world = local_of(&session, 0) + NEAR_CENTER;
    assert!(
        (world.y - 1.0).abs() < 1.0e-5 && (world.z - 3.0).abs() < 1.0e-5,
        "{world}"
    );

    // 吸着を切ると、同じ所を押しても面の上の点（第2段階と同じ）。
    let mut off = measuring();
    off.apply(Command::SetSnap(false));
    off.apply(Command::ViewClick { ndc: a });
    assert_eq!(off.measurement.kinds[0], SnapKind::Face);
    let point = local_of(&off, 0) + NEAR_CENTER;
    assert!((point.z - 3.0).abs() < 1.0e-4, "面の上: {point}");
    assert!(point.distance(CORNER_A) > 1.0e-3, "角には付かない: {point}");
    // 面の中央は、吸着が入っていても面。
    let mut center = measuring();
    center.apply(Command::ViewClick {
        ndc: ndc_of(&center, Vec3::new(0.1, 0.1, 3.0), 0.0, 0.0),
    });
    assert_eq!(center.measurement.kinds[0], SnapKind::Face);
}

#[test]
fn click_does_not_use_stale_preview() {
    let mut session = measuring();
    let corner = ndc_of(&session, CORNER_A, -4.0, -4.0);
    let middle = ndc_of(&session, Vec3::new(0.1, 0.1, 3.0), 0.0, 0.0);
    // 角で候補が出たまま、移動の出来事より先に面の中央でクリックが届いた。
    hover(&mut session, corner);
    assert_eq!(session.snap_preview.map(|t| t.kind), Some(SnapKind::Vertex));
    session.apply(Command::ViewClick { ndc: middle });
    assert_eq!(session.measurement.kinds[0], SnapKind::Face);
    let point = local_of(&session, 0) + NEAR_CENTER;
    assert!(
        point.distance(CORNER_A) > 0.5,
        "古い候補の角に付かない: {point}"
    );
    // 逆に、面の中央で候補が出たまま角でクリックしても、角に付く。
    hover(&mut session, middle);
    assert_eq!(session.snap_preview.map(|t| t.kind), Some(SnapKind::Face));
    session.apply(Command::ViewClick { ndc: corner });
    assert_eq!(session.measurement.kinds[1], SnapKind::Vertex);
    assert_eq!(local_of(&session, 1), CORNER_A - NEAR_CENTER);
}

#[test]
fn hover_searches_only_in_the_measure_tool_and_clears() {
    let mut session = support::session();
    let corner = ndc_of(&session, CORNER_A, -4.0, -4.0);
    hover(&mut session, corner);
    assert_eq!(session.snap_preview, None, "選択の道具では探さない");
    assert_eq!(session.metrics.snap_last, None, "探していない");
    session.apply(Command::SetTool(Tool::Measure));
    hover(&mut session, corner);
    let preview = session.snap_preview.expect("候補");
    assert_eq!(preview.kind, SnapKind::Vertex);
    assert!(close3(preview.world, CORNER_A));
    assert!(session.metrics.snap_last.is_some() && session.metrics.snap_max.is_some());
    // 3Dビューを出ると消える。
    session.apply(Command::ViewHover { ndc: None });
    assert_eq!(session.snap_preview, None);
    // 道具を替える・視点を変える・吸着を替える・断面を変えると消える。
    let restore = |session: &mut Session| {
        hover(session, corner);
        assert!(session.snap_preview.is_some());
    };
    restore(&mut session);
    session.apply(Command::Orbit(Vec2::new(3.0, 0.0)));
    assert_eq!(session.snap_preview, None, "視点");
    session.apply(Command::Home);
    session.camera = support::session().camera;
    restore(&mut session);
    session.apply(Command::ToggleSnap);
    assert_eq!(session.snap_preview, None, "吸着の切り替え");
    session.apply(Command::ToggleSnap);
    restore(&mut session);
    session.apply(Command::ToggleSection);
    assert_eq!(session.snap_preview, None, "断面");
    session.apply(Command::ToggleSection);
    restore(&mut session);
    session.apply(Command::AddShape(
        model_reviewer_core::shapes::ShapeKind::Cube,
    ));
    assert_eq!(session.snap_preview, None, "部品の表が変わった");
    session.apply(Command::SetTool(Tool::Measure));
    restore(&mut session);
    session.apply(Command::SetTool(Tool::Select));
    assert_eq!(session.snap_preview, None, "道具");
    // 吸着の入・切は履歴にも未保存にも入らない（図形を足した分だけ履歴がある）。
    assert_eq!(session.history.len(), 1);
}

#[test]
fn slow_search_backs_off() {
    // 8ミリ秒を超えた探索は、かかった時間の4倍だけ休む。
    assert_eq!(snap_rest_after(Duration::from_millis(7)), None);
    assert_eq!(snap_rest_after(Duration::from_millis(8)), None);
    assert_eq!(
        snap_rest_after(Duration::from_millis(9)),
        Some(Duration::from_millis(36))
    );
    let mut session = measuring();
    let corner = ndc_of(&session, CORNER_A, -4.0, -4.0);
    hover(&mut session, corner);
    assert!(session.snap_preview.is_some());
    // 遅い探索の後と同じ状態。休んでいる間は探さず、古い候補を消す。
    session.rest_snap_for(Duration::from_secs(60));
    assert!(session.snap_resting());
    session.metrics.snap_last = None;
    session.apply(Command::ViewHover {
        ndc: Some(ndc_of(&session, CORNER_B, 4.0, -4.0)),
    });
    assert_eq!(session.snap_preview, None, "休んでいる間は候補を見せない");
    assert_eq!(session.metrics.snap_last, None, "休んでいる間は探さない");
    // クリックは休まず探す。
    session.apply(Command::ViewClick { ndc: corner });
    assert_eq!(session.measurement.kinds[0], SnapKind::Vertex);
    assert!(session.metrics.snap_last.is_some(), "クリックの探索も測る");
    // 休みが明けると、また探す。探索が遅ければ次の休みに入るので、休みの有無は探索の前に見る。
    session.rest_snap_for(Duration::ZERO);
    assert!(!session.snap_resting());
    session.apply(Command::ViewHover { ndc: Some(corner) });
    assert!(session.snap_preview.is_some());
}

#[test]
fn snap_metric_is_reported() {
    let mut session = measuring();
    let lines = model_reviewer::metrics::report_lines(&session.metrics, None);
    let line = lines
        .iter()
        .find(|line| line.label == "吸着の探索")
        .unwrap();
    assert_eq!(line.value, "未計測");
    session.apply(Command::ViewClick {
        ndc: ndc_of(&session, CORNER_A, -4.0, -4.0),
    });
    let lines = model_reviewer::metrics::report_lines(&session.metrics, None);
    let line = lines
        .iter()
        .find(|line| line.label == "吸着の探索")
        .unwrap();
    assert!(line.value.starts_with("直近 "), "{}", line.value);
    let report = model_reviewer::metrics::report_value(&session.metrics);
    assert!(report["snap_ms"]["value"].is_number());
    assert!(report["snap_max_ms"]["value"].is_number());
}

/// 平行投影でも、透視と同じ角に付く（画面上の距離で選ぶので、投影に依らない）。
#[test]
fn orthographic_snaps_to_the_same_vertex() {
    for projection in [Projection::Perspective, Projection::Orthographic] {
        let mut session = measuring();
        session.apply(Command::SetProjection(projection));
        session.apply(Command::ViewClick {
            ndc: ndc_of(&session, CORNER_A, -4.0, -4.0),
        });
        session.apply(Command::ViewClick {
            ndc: ndc_of(&session, CORNER_B, 4.0, -4.0),
        });
        assert_eq!(
            session.measurement.kinds,
            [SnapKind::Vertex; 2],
            "{projection:?}"
        );
        assert_eq!(local_of(&session, 0), CORNER_A - NEAR_CENTER);
        assert!(near(measure_distance(&session).unwrap(), 2.0));
    }
}

/// 輪郭のすぐ外（角の4画素右・2画素下）から押しても、角に付く（要件1.6）。
#[test]
fn just_outside_the_silhouette_snaps_to_the_corner() {
    let mut session = measuring();
    session.apply(Command::ViewClick {
        ndc: ndc_of(&session, CORNER_A, 4.0, -2.0),
    });
    assert_eq!(session.measurement.kinds[0], SnapKind::Vertex);
    assert_eq!(local_of(&session, 0), CORNER_A - NEAR_CENTER);
    // 吸着を切ると、外は外れ。
    let mut off = measuring();
    off.apply(Command::SetSnap(false));
    off.apply(Command::ViewClick {
        ndc: ndc_of(&off, CORNER_A, 4.0, -2.0),
    });
    assert!(off.measurement.first.is_none());
    assert!(off.status_text.contains("表面"), "{}", off.status_text);
}

/// 吸着の半径は12ポイント。画面の倍率が2なら物理画素で24。
#[test]
fn radius_follows_pixels_per_point() {
    let mut session = measuring();
    session.pixels_per_point = 2.0;
    session.apply(Command::ViewClick {
        ndc: ndc_of(&session, CORNER_A, -15.0, -15.0),
    });
    assert_eq!(
        session.measurement.kinds[0],
        SnapKind::Vertex,
        "約21画素（24画素の内側）"
    );
    let mut session = measuring();
    session.apply(Command::ViewClick {
        ndc: ndc_of(&session, CORNER_A, -15.0, -15.0),
    });
    assert_ne!(
        session.measurement.kinds[0],
        SnapKind::Vertex,
        "約21画素（12画素の外）"
    );
}

#[test]
fn x_toggles_snap() {
    let (mut harness, seen) = support::harness(measuring(), egui::vec2(1100.0, 720.0));
    for _ in 0..3 {
        harness.step();
    }
    support::hover_view(&mut harness);
    harness.key_press(egui::Key::X);
    harness.step();
    assert!(!harness.state().snap.enabled);
    assert_eq!(harness.state().status_text, "吸着: 切");
    assert!(!harness.state().has_unsaved());
    harness.key_press(egui::Key::X);
    harness.step();
    assert!(harness.state().snap.enabled);
    // 計測の道具でなくても切り替わる。
    harness.state_mut().apply(Command::SetTool(Tool::Select));
    harness.key_press(egui::Key::X);
    harness.step();
    assert!(!harness.state().snap.enabled);
    harness.state_mut().apply(Command::SetSnap(true));
    // Cmd+X は切り取りなので受けない。
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::X);
    harness.step();
    assert!(harness.state().snap.enabled);

    // ダイアログの間は働かない。
    harness.state_mut().dialog = Some(DialogRequest {
        title: "確認".to_owned(),
        body: "続けますか".to_owned(),
        buttons: vec![DialogButton {
            id: ids::DIALOG_CANCEL.to_owned(),
            label: "閉じる".to_owned(),
            command: Command::DismissDialog,
        }],
    });
    harness.step();
    seen.lock().unwrap().clear();
    harness.key_press(egui::Key::X);
    harness.step();
    assert!(harness.state().snap.enabled);
    harness.state_mut().dialog = None;
    harness.step();

    // 入力欄に文字を打っている間は働かない。
    let field = support::widget_rect(&harness, ids::VIEWS_NAME).expect("視点の名前の欄");
    support::click(&mut harness, field.center());
    assert!(harness.ctx.text_edit_focused());
    harness.key_press(egui::Key::X);
    harness.step();
    assert!(harness.state().snap.enabled);
    assert!(
        !seen
            .lock()
            .unwrap()
            .iter()
            .any(|command| matches!(command, Command::ToggleSnap)),
        "ダイアログ中と入力中に X が積まれた"
    );
}

/// 計測の道具では、3Dビューの上でポインタが動いたときだけ候補を探し、印は候補の点に描く。
#[test]
fn pointer_moves_push_hover_and_draw_the_mark() {
    let (mut harness, seen) = support::harness(measuring(), egui::vec2(1100.0, 720.0));
    for _ in 0..3 {
        harness.step();
    }
    let view = support::widget_rect(&harness, ids::VIEWPORT).unwrap();
    let aspect = view.width() / view.height();
    let ndc = harness.state().camera.project(CORNER_A, aspect).unwrap();
    let pos = egui::pos2(
        view.center().x + ndc.x * view.width() * 0.5 - 3.0,
        view.center().y - ndc.y * view.height() * 0.5 + 3.0,
    );
    seen.lock().unwrap().clear();
    harness.event(egui::Event::PointerMoved(pos));
    harness.step();
    harness.step();
    let hovers = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|command| matches!(command, Command::ViewHover { ndc: Some(_) }))
        .count();
    assert_eq!(hovers, 1, "動いたフレームの1回だけ");
    assert_eq!(
        harness.state().snap_preview.map(|t| t.kind),
        Some(SnapKind::Vertex)
    );
    // 止まっている間は積まない。
    seen.lock().unwrap().clear();
    harness.step();
    harness.step();
    assert!(
        seen.lock().unwrap().is_empty(),
        "{:?}",
        seen.lock().unwrap()
    );
    // 3Dビューを出ると1回だけ None。
    harness.event(egui::Event::PointerMoved(egui::pos2(2.0, 2.0)));
    harness.step();
    harness.step();
    let outs = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|command| matches!(command, Command::ViewHover { ndc: None }))
        .count();
    assert_eq!(outs, 1);
    assert_eq!(harness.state().snap_preview, None);
}

/// 計測のパネルに距離・差・2点の種類が出る。点の部品が奥でも同じ。
#[test]
fn measure_panel_shows_kinds_and_delta() {
    let mut session = measuring();
    session.apply(Command::ViewClick {
        ndc: ndc_of(&session, CORNER_A, -4.0, -4.0),
    });
    session.apply(Command::ViewClick {
        ndc: ndc_of(&session, Vec3::new(0.2, 1.0, 3.0), 0.0, -4.0),
    });
    let input = model_reviewer::ui::measure_panel_input(&session);
    assert_eq!(input.points, 2);
    assert_eq!(input.kinds, [SnapKind::Vertex, SnapKind::Edge]);
    let delta = input.delta.unwrap();
    assert!(
        near(delta.y, 0.0) && near(delta.z, 0.0) && delta.x < -0.5,
        "{delta}"
    );
    let rows = model_reviewer::ui::measure_panel::rows(&input);
    assert!(
        rows.iter()
            .any(|(name, value)| *name == "1点目" && value == "頂点")
    );
    assert!(
        rows.iter()
            .any(|(name, value)| *name == "2点目" && value == "辺")
    );
    let _ = FAR_CENTER;
}
