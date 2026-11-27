//! 計測と単位を `Command` の列と状態欄で確かめる。GPUは使わない。

use std::sync::Arc;

use glam::{Vec2, Vec3};
use model_reviewer::command::{Command, Projection, Tool};
use model_reviewer::session::{Session, measure_distance};
use model_reviewer_core::camera::OrbitCamera;
use model_reviewer_core::gizmo::decompose;
use model_reviewer_core::identity::{ModelIdentity, ModelRef};
use model_reviewer_core::measure::{MeasurePoint, Measurement};
use model_reviewer_core::pick::PickIndex;
use model_reviewer_core::review::ReviewState;
use model_reviewer_core::scene::{NodeId, SceneRead};
use model_reviewer_core::units::{UnitLabel, format_distance};

const VIEW: [u32; 2] = [1100, 720];
const ASPECT: f32 = VIEW[0] as f32 / VIEW[1] as f32;

fn sample_session() -> Session {
    let bytes = model_reviewer_testdata::sample_glb();
    let model = Arc::new(model_reviewer_gltf::load_glb(&bytes).expect("見本のGLB"));
    let mut session = Session::default();
    session.review = Some(ReviewState::new(&model));
    session.model_ref = Some(ModelRef {
        file_name: "sample.glb".to_owned(),
        path: "sample.glb".into(),
        relative_path: None,
        identity: ModelIdentity::from_bytes(&bytes),
        stl_unit: None,
        stl_up: None,
    });
    session.pick_index = Some(PickIndex::build(&model));
    session.set_model(model);
    session.set_viewport(VIEW);
    let bounds = session.doc.as_ref().unwrap().bounds();
    session.camera = OrbitCamera::home(&bounds, ASPECT);
    session
}

/// 表面に当たる2点の正規化座標（見本の中央と、少し右下）。
const A: Vec2 = Vec2::new(0.0, 0.0);
const B: Vec2 = Vec2::new(0.25, -0.2);

fn world_points(session: &Session) -> Vec<Vec3> {
    session
        .measurement
        .world_points(session.doc.as_ref().unwrap())
}

fn measure(session: &mut Session, a: Vec2, b: Vec2) -> f32 {
    session.apply(Command::SetTool(Tool::Measure));
    assert_eq!(session.tool, Tool::Measure);
    session.apply(Command::ViewClick { ndc: a });
    assert!(
        session.measurement.first.is_some(),
        "1点目: {}",
        session.status_text
    );
    assert_eq!(session.status_text, "計測: もう1点をクリック");
    session.apply(Command::ViewClick { ndc: b });
    measure_distance(session).expect("2点目")
}

#[test]
fn two_clicks_give_the_distance_without_history_or_unsaved() {
    let mut session = sample_session();
    let distance = measure(&mut session, A, B);
    let points = world_points(&session);
    assert_eq!(points.len(), 2);
    assert!((points[0].distance(points[1]) - distance).abs() < 1.0e-6);
    assert!(distance > 0.01, "{distance}");
    assert!(session.status_text.is_empty(), "{}", session.status_text);
    assert!(session.history.is_empty(), "計測は履歴に入らない");
    assert!(!session.has_unsaved(), "計測は未保存にしない");
    // 点は表面の上（当たった部品の中の座標）にある。
    let doc = session.doc.as_ref().unwrap();
    let first = session.measurement.first.unwrap();
    let id = doc.id_of(first.node).unwrap();
    assert!(doc.world_aabb(id).min.cmple(points[0] + 1.0e-4).all());
    assert!(doc.world_aabb(id).max.cmpge(points[0] - 1.0e-4).all());

    // 3回目のクリックは新しい計測の1点目。
    session.apply(Command::ViewClick { ndc: B });
    assert!(session.measurement.second.is_none());
    assert_eq!(measure_distance(&session), None);
    // 外れたクリックは点を足さない。
    session.apply(Command::ViewClick {
        ndc: Vec2::new(0.98, 0.98),
    });
    assert!(session.measurement.second.is_none());
    assert!(
        session.status_text.contains("表面"),
        "{}",
        session.status_text
    );
}

#[test]
fn moving_a_part_changes_the_distance_and_deleting_it_clears_the_measure() {
    let mut session = sample_session();
    let before = measure(&mut session, A, B);
    let key = session.measurement.first.unwrap().node;
    let id = session.doc.as_ref().unwrap().id_of(key).unwrap();
    let old_world = world_points(&session)[0];

    // 1点目の部品を数値で動かす（どの道具でも欄は使える）。計測は残り、距離が変わる。
    session.apply(Command::Select(Some(id)));
    let local = session.doc.as_ref().unwrap().node(id).unwrap().local;
    let mut trs = decompose(&local).unwrap();
    let parent = session
        .doc
        .as_ref()
        .unwrap()
        .node(id)
        .unwrap()
        .parent
        .map(|p| *session.doc.as_ref().unwrap().world(p))
        .unwrap_or(glam::Affine3A::IDENTITY);
    let shift = Vec3::new(0.0, 0.5, 0.0);
    trs.translation += parent.inverse().transform_vector3(shift);
    session.apply(Command::CommitLocal(trs));
    assert_eq!(session.history.len(), 1, "動かした分だけ");
    let moved = world_points(&session)[0];
    assert!(
        (moved - old_world - shift).length() < 1.0e-4,
        "{moved} {old_world}"
    );
    let after = measure_distance(&session).unwrap();
    assert!((after - before).abs() > 1.0e-3, "{before} → {after}");
    let expected = moved.distance(world_points(&session)[1]);
    assert!((after - expected).abs() < 1.0e-6);

    // 取り消すと元の距離。
    session.apply(Command::Undo);
    assert!((measure_distance(&session).unwrap() - before).abs() < 1.0e-6);

    // 点の部品を削除すると計測が消え、取り消しても戻らない（計測は履歴に無い）。
    session.apply(Command::Select(Some(id)));
    session.apply(Command::DeleteSelected);
    assert_eq!(session.measurement, Measurement::default());
    assert_eq!(measure_distance(&session), None);
    session.apply(Command::Undo);
    assert_eq!(session.measurement, Measurement::default());
}

#[test]
fn units_change_only_the_label_and_escape_clears_then_leaves() {
    let mut session = sample_session();
    let distance = measure(&mut session, A, B);
    for unit in [UnitLabel::Cm, UnitLabel::Mm, UnitLabel::M] {
        session.apply(Command::SetUnit(unit));
        assert_eq!(session.units.source, unit);
        assert_eq!(
            measure_distance(&session),
            Some(distance),
            "数値は変わらない"
        );
        assert!(!session.has_unsaved());
    }
    assert_eq!(
        format_distance(distance, UnitLabel::Cm).split(' ').next(),
        format_distance(distance, UnitLabel::M).split(' ').next()
    );
    session.apply(Command::ClearMeasure);
    assert_eq!(session.measurement, Measurement::default());
    assert_eq!(session.tool, Tool::Measure);
}

#[test]
fn opening_another_review_or_model_clears_the_measure() {
    let mut session = sample_session();
    measure(&mut session, A, B);
    let model = Arc::clone(session.model.as_ref().unwrap());
    session.set_model(model);
    assert_eq!(session.measurement, Measurement::default());
}

/// 平行投影でも、同じ表面の点を押せば同じ距離になる（視線の出発点の丸めが表示の桁に出ない）。
/// 面の点どうしの比較なので吸着を切る（第2段階と同じ点）。吸着を入れた場合の透視と平行は
/// `snap.rs` の `orthographic_snaps_to_the_same_vertex` が見る（見本では、平行にすると角の手前に
/// 別の部品が来て、当たる部品が変わる所がある）。
#[test]
fn orthographic_gives_the_same_distance_as_perspective() {
    for zoom_out in [1.0_f32, 40.0, 2000.0] {
        let mut session = sample_session();
        session.apply(Command::SetSnap(false));
        session.camera.distance *= zoom_out;
        let perspective = measure(&mut session, A * (1.0 / zoom_out), B * (1.0 / zoom_out));
        let points = world_points(&session);
        let locals = [
            session.measurement.first.unwrap(),
            session.measurement.second.unwrap(),
        ];

        session.apply(Command::SetProjection(Projection::Orthographic));
        assert_eq!(session.camera.projection, Projection::Orthographic);
        session.apply(Command::ClearMeasure);
        for world in &points {
            let ndc = session.camera.project(*world, ASPECT).expect("画面の中");
            session.apply(Command::ViewClick { ndc });
        }
        let ortho = measure_distance(&session).expect("平行でも2点");
        let ortho_locals = [
            session.measurement.first.unwrap(),
            session.measurement.second.unwrap(),
        ];
        let scale = session.doc.as_ref().unwrap().bounds().radius();
        for (p, o) in locals.iter().zip(&ortho_locals) {
            assert_eq!(p.node, o.node, "同じ部品に当たる");
        }
        assert!(
            (ortho - perspective).abs() <= 1.0e-5 * scale.max(1.0),
            "倍率 {zoom_out}: 透視 {perspective} 平行 {ortho}"
        );
        for unit in [UnitLabel::M, UnitLabel::Mm] {
            assert_eq!(
                format_distance(ortho, unit),
                format_distance(perspective, unit),
                "倍率 {zoom_out}: 表示の桁で同じ"
            );
        }
    }
}

/// 文書を通した計測: 部品の行列を変えると、点が部品に付いて動く。
#[test]
fn measurement_reads_through_the_document() {
    let bytes = model_reviewer_testdata::sample_glb();
    let model = Arc::new(model_reviewer_gltf::load_glb(&bytes).unwrap());
    let mut doc = model_reviewer_core::doc::Document::new(model);
    let arm = doc.key(NodeId(9)).unwrap();
    let tip = doc.key(NodeId(10)).unwrap();
    let base = doc.key(NodeId(1)).unwrap();
    let mut m = Measurement::default();
    m.push(MeasurePoint {
        node: tip,
        local: Vec3::ZERO,
    });
    m.push(MeasurePoint {
        node: base,
        local: Vec3::ZERO,
    });
    let before = m.distance(&doc).unwrap();
    let tip_world = *doc.world(NodeId(10));
    let base_world = *doc.world(NodeId(1));
    assert!((before - tip_world.translation.distance(base_world.translation)).abs() < 1.0e-6);
    // 親（arm）を動かすと、子（tip）の点も動く。
    let local = doc.node(NodeId(9)).unwrap().local;
    doc.set_local(arm, glam::Affine3A::from_translation(Vec3::Y) * local)
        .unwrap();
    let moved = m.world_points(&doc)[0];
    let parent = doc
        .node(NodeId(9))
        .unwrap()
        .parent
        .map(|p| *doc.world(p))
        .unwrap();
    let expected = parent.transform_vector3(Vec3::Y);
    assert!((moved - Vec3::from(tip_world.translation) - expected).length() < 1.0e-5);
    assert!((m.distance(&doc).unwrap() - before).abs() > 1.0e-3);
    // 削除すると点が無くなり、計測は消える。
    let _ = doc.remove_subtree(arm).unwrap();
    assert_eq!(m.distance(&doc), None);
    assert!(m.prune(&doc));
    assert_eq!(m, Measurement::default());
}
