use std::path::PathBuf;

use glam::Vec3;
use model_reviewer_core::math::Aabb;
use model_reviewer_gltf::load_glb;
use model_reviewer_testdata::{
    EXPECTED_WORLD_POSITIONS, MESH_COUNT, NODE_COUNT, NODE_NAMES, PLACED_TRIANGLES,
    PRIMITIVE_COUNT, UNIQUE_TRIANGLES, sample_glb,
};

fn near(got: [f32; 3], expected: [f32; 3]) {
    for (actual, want) in got.iter().zip(expected) {
        assert!(
            (actual - want).abs() < 1e-5,
            "got {got:?}, expected {expected:?}"
        );
    }
}

fn near4(got: [f32; 4], expected: [f32; 4]) {
    for (actual, want) in got.iter().zip(expected) {
        assert!(
            (actual - want).abs() < 1e-5,
            "got {got:?}, expected {expected:?}"
        );
    }
}

#[test]
fn sample_hierarchy_matches_the_published_cart() {
    let model = load_glb(&sample_glb()).expect("sample");
    let names: Vec<_> = model.nodes.iter().map(|node| node.name.as_str()).collect();
    assert_eq!(names, NODE_NAMES);

    for (name, expected) in EXPECTED_WORLD_POSITIONS {
        let node = model
            .nodes
            .iter()
            .find(|node| node.name == name)
            .unwrap_or_else(|| panic!("missing {name}"));
        let translation: Vec3 = node.world.translation.into();
        near(translation.to_array(), expected);
    }

    let arm_tip = model
        .nodes
        .iter()
        .find(|node| node.name == "arm_tip")
        .expect("arm_tip");
    let arm_origin: Vec3 = arm_node_translation(&model);
    let arm_tip_y: Vec3 = arm_tip.world.translation.into();
    let parent_scale = arm_tip_y.distance(arm_origin);
    assert!(
        parent_scale > 0.4,
        "parent scale was dropped: arm {arm_origin:?} tip {arm_tip_y:?} distance {parent_scale}"
    );

    let lamp = model
        .nodes
        .iter()
        .find(|node| node.name == "lamp")
        .expect("lamp");
    let lamp_pos: Vec3 = lamp.world.translation.into();
    assert!(
        lamp_pos.length() > 0.1,
        "matrix translation stayed at the origin"
    );

    let base = model
        .nodes
        .iter()
        .find(|node| node.name == "base")
        .expect("base");
    let extent = base.world_aabb.max.y - base.world_aabb.min.y;
    assert!((extent - 0.2).abs() < 1e-4, "base Y extent {extent}");

    let arm_node = model
        .nodes
        .iter()
        .find(|node| node.name == "arm")
        .expect("arm");
    assert!(
        arm_node.world_aabb.min.y > base.world_aabb.max.y,
        "腕が台を突き抜けている {:?} {:?}",
        arm_node.world_aabb,
        base.world_aabb
    );
    let fl = model
        .nodes
        .iter()
        .find(|node| node.name == "wheel_FL")
        .expect("fl");
    let fr = model
        .nodes
        .iter()
        .find(|node| node.name == "wheel_FR")
        .expect("fr");
    let axis = fl.world.transform_vector3(Vec3::Y);
    let between = Vec3::from(fr.world.translation) - Vec3::from(fl.world.translation);
    let align = axis.normalize().dot(between.normalize()).abs();
    assert!(align > 0.99, "車輪の回転軸が車軸と違う: {align}");
    let spare = model
        .nodes
        .iter()
        .find(|node| node.name == "spare")
        .expect("spare");
    let spare_up = spare.world.transform_vector3(Vec3::Y).normalize();
    assert!(
        spare_up.dot(Vec3::Y).abs() > 0.99,
        "予備の車輪が寝ていない: {spare_up:?}"
    );
    // 根の Y15° のあと、軸平行の world_aabb は台と車輪、腕と予備車輪で重なる。
    // 数値で分けた組立座標では重ならない。
    let base_box = aabb_in_assembly(&model, "base");
    let arm_box = aabb_in_assembly(&model, "arm");
    for name in [
        "wheel_FL", "wheel_FR", "wheel_RL", "wheel_RR", "spare", "lamp",
    ] {
        let part = aabb_in_assembly(&model, name);
        assert!(
            separated(base_box, part),
            "台と{name}が組立座標で重なる {base_box:?} {part:?}"
        );
    }
    for name in ["spare", "lamp"] {
        let part = aabb_in_assembly(&model, name);
        assert!(
            separated(arm_box, part),
            "腕と{name}が組立座標で重なる {arm_box:?} {part:?}"
        );
    }
    assert_faces_outward(&model);

    let prim = |name: &str| {
        let node = model
            .nodes
            .iter()
            .find(|node| node.name == name)
            .unwrap_or_else(|| panic!("missing {name}"));
        model.draws[node.draws.start as usize..node.draws.end as usize]
            .iter()
            .map(|draw| draw.prim)
            .collect::<Vec<_>>()
    };
    let wheel = prim("wheel_FL");
    assert_eq!(wheel.len(), 1);
    assert_eq!(
        model.primitives[wheel[0].0 as usize].indices.len() / 3,
        96,
        "円柱は24分割"
    );
    assert_eq!(wheel, prim("wheel_FR"));
    assert_eq!(wheel, prim("wheel_RL"));
    assert_eq!(wheel, prim("wheel_RR"));
    assert_eq!(wheel, prim("spare"));
    assert_ne!(prim("base"), prim("arm"));
    assert_ne!(prim("arm")[0], prim("arm_tip")[0]);

    let color = |name: &str| {
        let node = model
            .nodes
            .iter()
            .find(|node| node.name == name)
            .expect(name);
        model.draws[node.draws.start as usize..node.draws.end as usize]
            .iter()
            .map(|draw| draw.base_color)
            .collect::<Vec<_>>()
    };
    near4(color("spare")[0], [0.15, 0.7, 0.25, 1.0]);
    near4(color("wheel_FL")[0], [0.15, 0.15, 0.16, 1.0]);
    near4(color("arm_tip")[0], [0.95, 0.85, 0.15, 1.0]);
    near4(color("arm_tip")[1], [0.95, 0.95, 0.95, 1.0]);

    let lamp_prim = &model.primitives[prim("lamp")[0].0 as usize];
    assert!(!lamp_prim.has_normals);
    assert!(
        lamp_prim
            .vertices
            .iter()
            .all(|vertex| vertex.normal == [0.0, 0.0, 0.0])
    );
    assert!(model.primitives[prim("base")[0].0 as usize].has_normals);
    assert!(model.warnings.iter().any(|warning| {
        matches!(
            warning,
            model_reviewer_core::scene::LoadWarning::DerivedNormals { count: 1 }
        )
    }));

    let wheel_fl = model
        .nodes
        .iter()
        .find(|node| node.name == "wheel_FL")
        .expect("fl");
    let parent = model.node(wheel_fl.parent.expect("parent"));
    assert_eq!(parent.name, "axle_front");
    let arm = model
        .nodes
        .iter()
        .position(|node| node.name == "arm")
        .expect("arm") as u32;
    let tip = model
        .nodes
        .iter()
        .position(|node| node.name == "arm_tip")
        .expect("tip") as u32;
    assert!(
        model
            .subtree(model_reviewer_core::scene::NodeId(arm))
            .contains(&tip)
    );
    assert_eq!(
        model.subtree(model_reviewer_core::scene::NodeId(0)),
        0..NODE_COUNT
    );

    assert_eq!(model.stats.nodes, NODE_COUNT);
    assert_eq!(model.stats.meshes, MESH_COUNT);
    assert_eq!(model.stats.primitives, PRIMITIVE_COUNT);
    assert_eq!(model.stats.unique_triangles, UNIQUE_TRIANGLES);
    assert_eq!(model.stats.placed_triangles, PLACED_TRIANGLES);

    let mut cursor = 0u32;
    for node in &model.nodes {
        assert_eq!(node.draws.start, cursor);
        cursor = node.draws.end;
    }
    assert_eq!(cursor as usize, model.draws.len());
}

#[test]
fn committed_sample_matches_generator_within_tolerance() {
    let fresh = sample_glb();
    assert_eq!(sample_glb(), fresh);
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/samples/review-sample.glb");
    let stored = std::fs::read(&path).expect("committed sample");
    assert_same_structure(&fresh, &stored);
}

fn arm_node_translation(model: &model_reviewer_core::scene::Model) -> Vec3 {
    model
        .nodes
        .iter()
        .find(|node| node.name == "arm")
        .expect("arm")
        .world
        .translation
        .into()
}

fn aabb_in_assembly(model: &model_reviewer_core::scene::Model, name: &str) -> Aabb {
    let assembly = model
        .nodes
        .iter()
        .find(|node| node.name == "assembly")
        .expect("assembly");
    let node = model
        .nodes
        .iter()
        .find(|node| node.name == name)
        .unwrap_or_else(|| panic!("missing {name}"));
    let into_assembly = assembly.world.inverse() * node.world;
    let mut bounds = Aabb::empty();
    for draw in &model.draws[node.draws.start as usize..node.draws.end as usize] {
        let primitive = &model.primitives[draw.prim.0 as usize];
        bounds = bounds.union(primitive.local_aabb.transformed(&into_assembly));
    }
    bounds
}

fn separated(a: Aabb, b: Aabb) -> bool {
    a.max.x <= b.min.x
        || b.max.x <= a.min.x
        || a.max.y <= b.min.y
        || b.max.y <= a.min.y
        || a.max.z <= b.min.z
        || b.max.z <= a.min.z
}

fn assert_faces_outward(model: &model_reviewer_core::scene::Model) {
    for primitive in &model.primitives {
        let positions: Vec<[f32; 3]> = primitive
            .vertices
            .iter()
            .map(|vertex| vertex.position)
            .collect();
        let centroid = centroid_of(&positions);
        for face in primitive.indices.chunks(3) {
            let tri = [
                positions[face[0] as usize],
                positions[face[1] as usize],
                positions[face[2] as usize],
            ];
            let geometric = face_normal(tri);
            if primitive.has_normals {
                for index in face {
                    let vertex = primitive.vertices[*index as usize].normal;
                    let dot = geometric[0] * vertex[0]
                        + geometric[1] * vertex[1]
                        + geometric[2] * vertex[2];
                    assert!(dot > 0.0, "inward triangle {tri:?} dot {dot}");
                }
            } else {
                let mid = [
                    (tri[0][0] + tri[1][0] + tri[2][0]) / 3.0,
                    (tri[0][1] + tri[1][1] + tri[2][1]) / 3.0,
                    (tri[0][2] + tri[1][2] + tri[2][2]) / 3.0,
                ];
                let outward = [
                    mid[0] - centroid[0],
                    mid[1] - centroid[1],
                    mid[2] - centroid[2],
                ];
                let dot = geometric[0] * outward[0]
                    + geometric[1] * outward[1]
                    + geometric[2] * outward[2];
                assert!(dot > 0.0, "lamp face points inward {tri:?} dot {dot}");
            }
        }
    }
}

fn centroid_of(positions: &[[f32; 3]]) -> [f32; 3] {
    let mut sum = [0.0, 0.0, 0.0];
    for position in positions {
        sum[0] += position[0];
        sum[1] += position[1];
        sum[2] += position[2];
    }
    let n = positions.len() as f32;
    [sum[0] / n, sum[1] / n, sum[2] / n]
}

fn face_normal(tri: [[f32; 3]; 3]) -> [f32; 3] {
    let e1 = [
        tri[1][0] - tri[0][0],
        tri[1][1] - tri[0][1],
        tri[1][2] - tri[0][2],
    ];
    let e2 = [
        tri[2][0] - tri[0][0],
        tri[2][1] - tri[0][1],
        tri[2][2] - tri[0][2],
    ];
    [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ]
}

fn assert_same_structure(left: &[u8], right: &[u8]) {
    let left_glb = gltf::binary::Glb::from_slice(left).expect("left glb");
    let right_glb = gltf::binary::Glb::from_slice(right).expect("right glb");
    let left_json: gltf::json::Value =
        gltf::json::deserialize::from_slice(&left_glb.json).expect("left json");
    let right_json: gltf::json::Value =
        gltf::json::deserialize::from_slice(&right_glb.json).expect("right json");
    assert_eq!(compare_value(&left_json, &right_json, 0), None);
    let left_bin = left_glb.bin.as_deref().unwrap_or_default();
    let right_bin = right_glb.bin.as_deref().unwrap_or_default();
    assert_eq!(left_bin.len(), right_bin.len());
    let groups = left_bin.len() / 4;
    for index in 0..groups {
        let at = index * 4;
        let left_f = f32::from_le_bytes(left_bin[at..at + 4].try_into().expect("4"));
        let right_f = f32::from_le_bytes(right_bin[at..at + 4].try_into().expect("4"));
        assert!(
            (left_f - right_f).abs() <= 1e-6,
            "bin {at}: {left_f} vs {right_f}"
        );
    }
    assert_eq!(&left_bin[groups * 4..], &right_bin[groups * 4..]);
}

fn compare_value(
    left: &gltf::json::Value,
    right: &gltf::json::Value,
    depth: u32,
) -> Option<String> {
    if depth > 64 {
        return Some("deep json".to_owned());
    }
    use gltf::json::Value::{Array, Bool, Null, Number, Object, String as JsonString};
    match (left, right) {
        (Null, Null) => None,
        (Bool(left), Bool(right)) if left == right => None,
        (JsonString(left), JsonString(right)) if left == right => None,
        (Number(left), Number(right)) => {
            let left = left.as_f64().unwrap_or(f64::NAN);
            let right = right.as_f64().unwrap_or(f64::NAN);
            if (left - right).abs() <= 1e-6 {
                None
            } else {
                Some(format!("{left} vs {right}"))
            }
        }
        (Array(left), Array(right)) if left.len() == right.len() => {
            for (left, right) in left.iter().zip(right) {
                if let Some(error) = compare_value(left, right, depth + 1) {
                    return Some(error);
                }
            }
            None
        }
        (Object(left), Object(right)) if left.len() == right.len() => {
            for (key, left) in left {
                let Some(right) = right.get(key) else {
                    return Some(format!("missing {key}"));
                };
                if let Some(error) = compare_value(left, right, depth + 1) {
                    return Some(error);
                }
            }
            None
        }
        _ => Some(format!("mismatch {left} {right}")),
    }
}

#[test]
fn sample_wheels_rest_on_the_floor() {
    // 格子（高さ 0）が車輪に重なって透けないよう、組み立ての最も低い所が床にちょうど載る。
    let model = load_glb(&sample_glb()).expect("sample");
    let min_y = model.bounds.min.y;
    assert!(
        min_y.abs() < 1e-4,
        "lowest point {min_y} should touch y = 0"
    );
    for wheel in ["wheel_FL", "wheel_FR", "wheel_RL", "wheel_RR"] {
        let node = model
            .nodes
            .iter()
            .find(|node| node.name == wheel)
            .expect("wheel");
        assert!(
            node.world_aabb.min.y.abs() < 1e-4,
            "{wheel} bottom {}",
            node.world_aabb.min.y
        );
    }
}
