//! レビューJSON。往復、別モデル、未知のノード、版（版1と版2）、形式、非数。

use glam::{Affine3A, Vec3};

use model_reviewer_core::camera::{OrbitCamera, Projection};
use model_reviewer_core::error::ReviewError;
use model_reviewer_core::identity::{ModelIdentity, ModelRef};
use model_reviewer_core::math::Aabb;
use model_reviewer_core::pick::Hit;
use model_reviewer_core::review::ReviewState;
use model_reviewer_core::review_file::{parse, restore, to_json};
use model_reviewer_core::scene::{Model, ModelStats, Node, NodeId};

const TEXT: &str = "ここを\n直して「角」";

fn node(name: &str, gltf_index: u32, parent: Option<NodeId>, end: u32) -> Node {
    Node {
        name: name.to_owned(),
        gltf_index,
        parent,
        subtree_end: end,
        depth: u32::from(parent.is_some()),
        local: Affine3A::IDENTITY,
        world: Affine3A::IDENTITY,
        draws: 0..0,
        world_aabb: Aabb {
            min: Vec3::ZERO,
            max: Vec3::ONE,
        },
    }
}

fn review_model() -> Model {
    Model {
        nodes: vec![
            node("台車", 0, None, 4),
            node("腕", 4, Some(NodeId(0)), 3),
            node("先端", 9, Some(NodeId(1)), 3),
            node("車輪", 6, Some(NodeId(0)), 4),
        ],
        draws: Vec::new(),
        primitives: Vec::new(),
        bounds: Aabb {
            min: Vec3::ZERO,
            max: Vec3::ONE,
        },
        stats: ModelStats {
            nodes: 4,
            ..ModelStats::default()
        },
        warnings: Vec::new(),
    }
}

fn model_ref(bytes: &[u8]) -> ModelRef {
    ModelRef {
        file_name: "review-sample.glb".to_owned(),
        path: "/work/models/review-sample.glb".to_owned(),
        relative_path: Some("../models/review-sample.glb".to_owned()),
        identity: ModelIdentity::from_bytes(bytes),
        stl_unit: None,
    }
}

fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 1.0e-4,
        "{actual} vs {expected}"
    );
}

fn close3(actual: Vec3, expected: Vec3) {
    assert!(
        (actual - expected).length() <= 1.0e-4,
        "{actual:?} vs {expected:?}"
    );
}

fn render(state: &ReviewState, model: &Model, model_ref: &ModelRef) -> String {
    to_json(state, model, model_ref).expect("有限な見本")
}

fn sample() -> (Model, ModelRef, ReviewState) {
    let model = review_model();
    let model_ref = model_ref(b"model-a");
    let mut state = ReviewState::new(&model);
    state.visibility.set(&model, NodeId(1), false);
    state.visibility.set(&model, NodeId(3), false);
    let hit = Hit {
        node: NodeId(2),
        t: 1.5,
        world_pos: Vec3::new(0.2, 0.3, 0.4),
        local_pos: Vec3::new(0.12, 0.4, -0.05),
        local_normal: Vec3::Y,
        prim: model_reviewer_core::scene::PrimId(0),
        triangle: 0,
    };
    assert_eq!(state.add_annotation(&hit, TEXT.to_owned()), Some(1));
    let camera = OrbitCamera {
        target: Vec3::new(0.0, 0.5, 0.0),
        yaw: 10.0_f32.to_radians(),
        pitch: 20.0_f32.to_radians(),
        distance: 4.2,
        fov_y: 40.0_f32.to_radians(),
        ..OrbitCamera::default()
    };
    assert_eq!(state.add_view("正面".to_owned(), camera), Some(2));
    (model, model_ref, state)
}

#[test]
fn roundtrip_keeps_japanese_text_visibility_and_views() {
    let (model, model_ref, mut state) = sample();
    assert!(state.edit_annotation(1, format!("{TEXT}、追記")));
    assert!(!state.edit_annotation(99, "無い".to_owned()));
    assert!(state.edit_annotation(1, TEXT.to_owned()));
    assert!(!state.remove_annotation(99));
    assert!(!state.remove_view(99));

    let json = render(&state, &model, &model_ref);
    assert!(json.contains("ここを"));
    assert!(json.contains("「角」"));
    assert!(json.contains("正面"));
    assert!(json.contains("腕"));
    assert!(json.contains("車輪"));
    assert!(json.contains("先端"));
    assert!(json.contains("\\n"));
    assert!(!json.contains("model-reviewer.review\u{0000}"));

    let file = parse(&json).expect("parse");
    assert_eq!(file.format, "model-reviewer.review");
    assert_eq!(file.version, 1);
    assert_eq!(file.app_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(file.model.file_name, model_ref.file_name);
    assert_eq!(file.model.path, model_ref.path);
    assert_eq!(file.model.relative_path, model_ref.relative_path);
    assert_eq!(file.model.size_bytes, model_ref.identity.size_bytes);
    assert_eq!(file.model.hash.algorithm, "blake3");
    assert_eq!(file.model.hash.hex, model_ref.identity.blake3_hex);
    assert_eq!(file.model.hash.hex.len(), 64);
    assert_eq!(
        file.hidden_nodes
            .iter()
            .map(|node| node.gltf_index)
            .collect::<Vec<_>>(),
        vec![4, 6]
    );

    let mut with_extra: serde_json::Value = serde_json::from_str(&json).unwrap();
    with_extra["ignored"] = serde_json::json!({"note": "残す"});
    let file = parse(&with_extra.to_string()).expect("unknown field");
    let mut upper_case = file.clone();
    upper_case.model.hash.hex.make_ascii_uppercase();
    let file = parse(&serde_json::to_string(&upper_case).unwrap()).unwrap();
    assert_eq!(file.model.hash.hex, model_ref.identity.blake3_hex);

    let mut counted = file.clone();
    counted.model.node_count = 999;
    let restored = restore(&counted, &model, &model_ref.identity).expect("restore");
    assert!(restored.visibility.own(NodeId(2)));
    assert!(!restored.visibility.effective(NodeId(2)));
    assert!(!restored.visibility.own(NodeId(1)));
    assert!(!restored.visibility.effective(NodeId(1)));
    assert!(restored.visibility.own(NodeId(0)));
    assert!(restored.visibility.effective(NodeId(0)));
    assert!(!restored.visibility.own(NodeId(3)));
    assert_eq!(restored.annotations.len(), 1);
    assert_eq!(restored.annotations[0].id, 1);
    assert_eq!(restored.annotations[0].node, NodeId(2));
    assert_eq!(restored.annotations[0].text, TEXT);
    close3(
        restored.annotations[0].local_pos,
        Vec3::new(0.12, 0.4, -0.05),
    );
    close3(restored.annotations[0].local_normal, Vec3::Y);
    assert_eq!(restored.views.len(), 1);
    assert_eq!(restored.views[0].id, 2);
    assert_eq!(restored.views[0].name, "正面");
    let camera = &restored.views[0].camera;
    close3(camera.target, Vec3::new(0.0, 0.5, 0.0));
    close(camera.yaw.to_degrees(), 10.0);
    close(camera.pitch.to_degrees(), 20.0);
    close(camera.distance, 4.2);
    close(camera.fov_y.to_degrees(), 40.0);

    let mut again = restored;
    let next = again.add_annotation(
        &Hit {
            node: NodeId(0),
            t: 1.0,
            world_pos: Vec3::ZERO,
            local_pos: Vec3::ZERO,
            local_normal: Vec3::Z,
            prim: model_reviewer_core::scene::PrimId(0),
            triangle: 0,
        },
        "次".to_owned(),
    );
    assert_eq!(next, Some(3));
    assert_eq!(
        again.add_view("側面".to_owned(), OrbitCamera::default()),
        Some(4)
    );

    let zero_normal = Hit {
        node: NodeId(0),
        t: 1.0,
        world_pos: Vec3::ZERO,
        local_pos: Vec3::ZERO,
        local_normal: Vec3::ZERO,
        prim: model_reviewer_core::scene::PrimId(0),
        triangle: 0,
    };
    let mut state = ReviewState::new(&model);
    state.add_annotation(&zero_normal, "零".to_owned());
    let json = render(&state, &model, &model_ref);
    let restored = restore(&parse(&json).unwrap(), &model, &model_ref.identity).unwrap();
    close3(restored.annotations[0].local_normal, Vec3::ZERO);
}

#[test]
fn one_byte_difference_is_model_mismatch_before_unknown_nodes() {
    let (model, model_ref, state) = sample();
    let other = ModelIdentity::from_bytes(b"model-b");
    assert_eq!(model_ref.identity.size_bytes, other.size_bytes);
    assert_ne!(model_ref.identity.blake3_hex, other.blake3_hex);
    let file = parse(&render(&state, &model, &model_ref)).unwrap();
    let err = restore(&file, &model, &other).unwrap_err();
    assert_eq!(
        err,
        ReviewError::ModelMismatch {
            expected: model_ref.identity.clone(),
            actual: other.clone(),
        }
    );
    assert!(err.message_ja().contains(&other.size_bytes.to_string()));

    let mut broken = file.clone();
    broken.annotations[0].node.gltf_index = 404;
    let err = restore(&broken, &model, &other).unwrap_err();
    assert!(matches!(err, ReviewError::ModelMismatch { .. }));
}

#[test]
fn missing_or_renamed_nodes_are_unknown() {
    let (model, model_ref, state) = sample();
    let file = parse(&render(&state, &model, &model_ref)).unwrap();

    let mut missing = file.clone();
    missing.hidden_nodes[0].gltf_index = 99;
    let err = restore(&missing, &model, &model_ref.identity).unwrap_err();
    assert_eq!(err, ReviewError::UnknownNode { gltf_index: 99 });
    assert!(err.message_ja().contains("99"));

    let mut renamed = file.clone();
    renamed.annotations[0].node.name = "別の名前".to_owned();
    let err = restore(&renamed, &model, &model_ref.identity).unwrap_err();
    assert_eq!(err, ReviewError::UnknownNode { gltf_index: 9 });

    let mut bad_hash = file.clone();
    bad_hash.model.hash.algorithm = "sha256".to_owned();
    assert!(matches!(
        restore(&bad_hash, &model, &model_ref.identity),
        Err(ReviewError::InvalidValue(_))
    ));
    bad_hash = file;
    bad_hash.model.hash.hex = "abcd".to_owned();
    assert!(matches!(
        restore(&bad_hash, &model, &model_ref.identity),
        Err(ReviewError::InvalidValue(_))
    ));
}

#[test]
fn newer_version_wrong_format_and_non_finite_numbers_are_rejected() {
    let (model, model_ref, state) = sample();
    let json = render(&state, &model, &model_ref);
    let file = parse(&json).unwrap();
    let rendered = serde_json::to_string(&file).unwrap();

    let mut newer_value: serde_json::Value = serde_json::from_str(&json).unwrap();
    newer_value["version"] = serde_json::json!(3);
    assert_eq!(
        parse(&newer_value.to_string()).unwrap_err(),
        ReviewError::NewerVersion {
            found: 3,
            supported: 2,
        }
    );
    let wrong = "{\"format\":\"other\",\"version\":9}";
    assert_eq!(parse(wrong).unwrap_err(), ReviewError::WrongFormat);
    assert_eq!(
        parse("{\"version\":2}").unwrap_err(),
        ReviewError::WrongFormat
    );

    let position = serde_json::to_string(&file.annotations[0].local_position).unwrap();
    let huge = rendered.replacen(&position, "[1e39,0.0,0.0]", 1);
    assert!(
        matches!(parse(&huge), Err(ReviewError::InvalidValue(_))),
        "{}",
        parse(&huge).unwrap_err()
    );
    let distance = serde_json::to_string(&file.views[0].camera.distance).unwrap();
    let negative_huge = rendered.replacen(&distance, "-1e40", 1);
    assert!(matches!(
        parse(&negative_huge),
        Err(ReviewError::InvalidValue(_))
    ));
    let overflow = rendered.replacen(&position, "[1e999,0.0,0.0]", 1);
    assert!(
        matches!(parse(&overflow), Err(ReviewError::Json(_))),
        "{}",
        parse(&overflow).unwrap_err()
    );

    let mut camera = file.clone();
    camera.views[0].camera.projection = "ortho".to_owned();
    assert!(matches!(
        restore(&camera, &model, &model_ref.identity),
        Err(ReviewError::InvalidValue(_))
    ));
    camera = file.clone();
    camera.views[0].camera.distance = 0.0;
    assert!(matches!(
        restore(&camera, &model, &model_ref.identity),
        Err(ReviewError::InvalidValue(_))
    ));
    camera.views[0].camera.distance = 4.2;
    camera.views[0].camera.fov_y_deg = 180.0;
    assert!(matches!(
        restore(&camera, &model, &model_ref.identity),
        Err(ReviewError::InvalidValue(_))
    ));
    camera.views[0].camera.fov_y_deg = 40.0;
    camera.views[0].camera.yaw_deg = f32::INFINITY;
    assert!(matches!(
        restore(&camera, &model, &model_ref.identity),
        Err(ReviewError::InvalidValue(_))
    ));

    let mut wide = file;
    wide.model.size_bytes = u64::MAX;
    let text = serde_json::to_string(&wide).unwrap();
    assert!(text.contains(&u64::MAX.to_string()));
    let parsed = parse(&text).expect("u64 size is not an f32");
    assert_eq!(parsed.model.size_bytes, u64::MAX);
}

#[test]
fn parse_rejects_duplicate_ids_and_ids_near_u64_max() {
    let (model, model_ref, state) = sample();
    let json = render(&state, &model, &model_ref);
    let mut shared = serde_json::from_str::<serde_json::Value>(&json).unwrap();
    shared["views"][0]["id"] = shared["annotations"][0]["id"].clone();
    let err = parse(&shared.to_string()).unwrap_err();
    assert!(
        matches!(err, ReviewError::InvalidValue(ref detail) if detail.contains("id")),
        "{err}"
    );

    let mut duplicated = serde_json::from_str::<serde_json::Value>(&json).unwrap();
    duplicated["annotations"][0]["id"] = serde_json::json!(7);
    let extra = duplicated["annotations"][0].clone();
    duplicated["annotations"]
        .as_array_mut()
        .unwrap()
        .push(extra);
    let err = parse(&duplicated.to_string()).unwrap_err();
    assert!(
        matches!(err, ReviewError::InvalidValue(ref detail) if detail.contains("duplicate")),
        "{err}"
    );

    let mut duplicated_views = serde_json::from_str::<serde_json::Value>(&json).unwrap();
    duplicated_views["views"][0]["id"] = serde_json::json!(9);
    let extra_view = duplicated_views["views"][0].clone();
    duplicated_views["views"]
        .as_array_mut()
        .unwrap()
        .push(extra_view);
    let err = parse(&duplicated_views.to_string()).unwrap_err();
    assert!(
        matches!(err, ReviewError::InvalidValue(ref detail) if detail.contains("duplicate")),
        "{err}"
    );

    for id in [u64::MAX - 10, u64::MAX - 2, u64::MAX - 1, u64::MAX] {
        let mut near = serde_json::from_str::<serde_json::Value>(&json).unwrap();
        near["annotations"][0]["id"] = serde_json::json!(id);
        let err = parse(&near.to_string()).unwrap_err();
        assert!(
            matches!(err, ReviewError::InvalidValue(ref detail) if detail == "id"),
            "annotation {id}: {err}"
        );

        let mut near_view = serde_json::from_str::<serde_json::Value>(&json).unwrap();
        near_view["views"][0]["id"] = serde_json::json!(id);
        let err = parse(&near_view.to_string()).unwrap_err();
        assert!(
            matches!(err, ReviewError::InvalidValue(ref detail) if detail == "id"),
            "view {id}: {err}"
        );
    }
}

#[test]
fn to_json_rejects_non_finite_and_invalid_values() {
    let (model, model_ref, state) = sample();

    let mut nan_pos = state.clone();
    nan_pos.annotations[0].local_pos.x = f32::NAN;
    assert!(matches!(
        to_json(&nan_pos, &model, &model_ref),
        Err(ReviewError::InvalidValue(_))
    ));

    let mut inf_normal = state.clone();
    inf_normal.annotations[0].local_normal.y = f32::NEG_INFINITY;
    assert!(matches!(
        to_json(&inf_normal, &model, &model_ref),
        Err(ReviewError::InvalidValue(_))
    ));

    let mut nan_target = state.clone();
    nan_target.views[0].camera.target.z = f32::NAN;
    assert!(matches!(
        to_json(&nan_target, &model, &model_ref),
        Err(ReviewError::InvalidValue(_))
    ));

    let mut overflow_deg = state.clone();
    overflow_deg.views[0].camera.yaw = f32::MAX;
    assert!(matches!(
        to_json(&overflow_deg, &model, &model_ref),
        Err(ReviewError::InvalidValue(_))
    ));

    let mut zero_distance = state;
    zero_distance.views[0].camera.distance = 0.0;
    assert!(matches!(
        to_json(&zero_distance, &model, &model_ref),
        Err(ReviewError::InvalidValue(_))
    ));
}

#[test]
fn broken_json_returns_an_error_without_panicking() {
    let samples = [
        "",
        "{",
        "}",
        "null",
        "[]",
        "\"x\"",
        "{]",
        "\u{0000}",
        "{\"format\":\"model-reviewer.review\",\"version\":1,\"views\":[{\"camera\":{\"yaw_deg\":NaN}}]}",
        "{\"format\":\"model-reviewer.review\",\"version\":1,\"annotations\":[{\"local_position\":[1e39,0,0]}]}",
    ];
    for sample in samples {
        let result = std::panic::catch_unwind(|| parse(sample));
        assert!(result.is_ok(), "panicked on {sample:?}");
        assert!(result.unwrap().is_err(), "accepted {sample:?}");
    }
}

#[test]
fn orthographic_views_are_written_as_version_two_and_read_back() {
    let (model, model_ref, mut state) = sample();
    let perspective = render(&state, &model, &model_ref);
    assert_eq!(parse(&perspective).unwrap().version, 1);
    assert!(!perspective.contains("orthographic"));

    let top = OrbitCamera {
        target: Vec3::new(0.25, 0.0, -0.5),
        yaw: 0.0,
        pitch: 90.0_f32.to_radians(),
        distance: 3.0,
        fov_y: 40.0_f32.to_radians(),
        projection: Projection::Orthographic,
    };
    assert_eq!(state.add_view("上".to_owned(), top), Some(3));
    let json = render(&state, &model, &model_ref);
    assert!(json.contains("\"orthographic\""));
    assert!(json.contains("\"perspective\""));
    let file = parse(&json).expect("version 2 parses");
    assert_eq!(file.version, 2);
    let restored = restore(&file, &model, &model_ref.identity).expect("restore v2");
    assert_eq!(restored.views.len(), 2);
    assert_eq!(restored.views[0].camera.projection, Projection::Perspective);
    let back = &restored.views[1].camera;
    assert_eq!(back.projection, Projection::Orthographic);
    close(back.pitch.to_degrees(), 90.0);
    close(back.distance, 3.0);
    close3(back.target, top.target);
    assert!(back.view().is_finite());

    // 平行の視点を消すと版1へ戻る。
    assert!(state.remove_view(3));
    assert_eq!(
        parse(&render(&state, &model, &model_ref)).unwrap().version,
        1
    );
}

#[test]
fn version_one_files_still_load_and_cannot_claim_orthographic() {
    let (model, model_ref, state) = sample();
    let json = render(&state, &model, &model_ref);
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["version"], 1);
    let file = parse(&json).expect("version 1");
    let restored = restore(&file, &model, &model_ref.identity).unwrap();
    assert_eq!(restored.views[0].camera.projection, Projection::Perspective);
    // 保存した視点の値は第1段階と同じ（度で書き、読み直して同じ値）。
    close(restored.views[0].camera.yaw.to_degrees(), 10.0);
    close(restored.views[0].camera.pitch.to_degrees(), 20.0);
    close(restored.views[0].camera.distance, 4.2);
    close(restored.views[0].camera.fov_y.to_degrees(), 40.0);
    close3(restored.views[0].camera.target, Vec3::new(0.0, 0.5, 0.0));

    value["views"][0]["camera"]["projection"] = serde_json::json!("orthographic");
    assert!(matches!(
        parse(&value.to_string()),
        Err(ReviewError::InvalidValue(_))
    ));
    value["version"] = serde_json::json!(2);
    let upgraded = parse(&value.to_string()).expect("orthographic is a version 2 value");
    assert_eq!(
        restore(&upgraded, &model, &model_ref.identity)
            .unwrap()
            .views[0]
            .camera
            .projection,
        Projection::Orthographic
    );
    value["version"] = serde_json::json!(0);
    assert!(matches!(
        parse(&value.to_string()),
        Err(ReviewError::InvalidValue(_))
    ));
}

#[test]
fn stl_unit_round_trips() {
    use model_reviewer_core::units::UnitLabel;

    let (model, mut model_ref, state) = sample();
    for (unit, text) in [
        (UnitLabel::Mm, "\"mm\""),
        (UnitLabel::Cm, "\"cm\""),
        (UnitLabel::M, "\"m\""),
    ] {
        model_ref.stl_unit = Some(unit);
        let json = render(&state, &model, &model_ref);
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["model"]["stl_unit"].to_string(), text);
        assert_eq!(value["version"], serde_json::json!(1), "版は上げない");
        let file = parse(&json).expect("parse");
        assert_eq!(file.model.stl_unit, Some(unit));
        // 書き直しても同じ。
        let again = serde_json::to_string_pretty(&file).unwrap();
        assert_eq!(parse(&again).unwrap().model.stl_unit, Some(unit));
    }

    // 無ければ None。
    model_ref.stl_unit = None;
    let json = render(&state, &model, &model_ref);
    assert!(!json.contains("stl_unit"));
    assert_eq!(parse(&json).unwrap().model.stl_unit, None);

    // 知らない値と文字列でない値は読まない。
    let base: serde_json::Value = serde_json::from_str(&json).unwrap();
    for bad in [
        serde_json::json!("inch"),
        serde_json::json!("MM"),
        serde_json::json!(""),
        serde_json::json!(1),
        serde_json::json!(null),
    ] {
        let mut value = base.clone();
        value["model"]["stl_unit"] = bad.clone();
        assert!(
            matches!(parse(&value.to_string()), Err(ReviewError::InvalidValue(_))),
            "{bad}"
        );
    }
}

#[test]
fn glb_review_bytes_are_unchanged() {
    let model = review_model();
    let model_ref = model_ref(b"model-a");
    let mut state = ReviewState::new(&model);
    state.visibility.set(&model, NodeId(3), false);
    let hit = Hit {
        node: NodeId(2),
        t: 1.0,
        world_pos: Vec3::ZERO,
        local_pos: Vec3::new(0.5, 0.25, -1.0),
        local_normal: Vec3::Y,
        prim: model_reviewer_core::scene::PrimId(0),
        triangle: 0,
    };
    assert_eq!(state.add_annotation(&hit, "角".to_owned()), Some(1));
    let json = render(&state, &model, &model_ref);
    let expected = format!(
        r#"{{
  "format": "model-reviewer.review",
  "version": 1,
  "app_version": "{version}",
  "model": {{
    "file_name": "review-sample.glb",
    "path": "/work/models/review-sample.glb",
    "relative_path": "../models/review-sample.glb",
    "size_bytes": 7,
    "hash": {{
      "algorithm": "blake3",
      "hex": "{hex}"
    }},
    "node_count": 4
  }},
  "hidden_nodes": [
    {{
      "gltf_index": 6,
      "name": "車輪"
    }}
  ],
  "annotations": [
    {{
      "id": 1,
      "node": {{
        "gltf_index": 9,
        "name": "先端"
      }},
      "local_position": [
        0.5,
        0.25,
        -1.0
      ],
      "local_normal": [
        0.0,
        1.0,
        0.0
      ],
      "text": "角"
    }}
  ],
  "views": []
}}"#,
        version = env!("CARGO_PKG_VERSION"),
        hex = model_ref.identity.blake3_hex,
    );
    assert_eq!(json, expected);
}
