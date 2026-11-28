//! 新規の文書の土台（`empty_base`）からの書き出し。

use std::sync::Arc;

use glam::{Affine3A, Quat, Vec3};
use model_reviewer_core::math::Aabb;
use model_reviewer_core::scene::{Draw, PrimId, Primitive, Vertex};
use model_reviewer_gltf::export::{
    Built, ExportError, ExportNode, ExportScene, build, empty_base, verify, verify_locals,
    write_container,
};
use model_reviewer_gltf::{LoadError, load_glb};

fn tetra() -> Arc<Primitive> {
    let positions = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.5, 0.0],
        [0.0, 0.0, -0.75],
    ];
    let normals = [
        [-0.577, -0.577, 0.577],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, -1.0],
    ];
    let vertices = positions
        .iter()
        .zip(normals)
        .map(|(position, normal)| Vertex {
            position: *position,
            normal,
        })
        .collect();
    Arc::new(Primitive {
        vertices,
        indices: vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
        local_aabb: Aabb {
            min: Vec3::new(0.0, 0.0, -0.75),
            max: Vec3::new(1.0, 1.5, 0.0),
        },
        has_normals: true,
    })
}

const COLOR: [f32; 4] = [0.8, 0.1, 0.05, 1.0];

fn local() -> Affine3A {
    Affine3A::from_scale_rotation_translation(
        Vec3::new(1.0, 2.0, 1.0),
        Quat::from_rotation_y(0.5),
        Vec3::new(0.25, -1.0, 3.0),
    )
}

fn added(parent: Option<u32>, name: &str, local: Affine3A, draws: Vec<Draw>) -> ExportNode {
    ExportNode {
        parent,
        name: name.to_owned(),
        local,
        file_index: None,
        template: None,
        draws,
    }
}

/// 足した図形1つの文書。
fn one_shape() -> ExportScene {
    ExportScene {
        nodes: vec![added(
            None,
            "立方体",
            local(),
            vec![Draw {
                prim: PrimId(0),
                base_color: COLOR,
                tex: None,
            }],
        )],
        added: vec![tetra()],
        base_primitive_count: 0,
    }
}

fn write(base: &[u8], built: &Built) -> Vec<u8> {
    let mut written = Vec::new();
    let length = write_container(&mut written, base, built).expect("write");
    assert_eq!(length, written.len() as u64);
    written
}

fn export(scene: &ExportScene) -> Vec<u8> {
    let base = empty_base();
    let built = build(&base, scene).unwrap_or_else(|error| panic!("build: {error:?}"));
    let written = write(&base, &built);
    verify(&written, &base, scene, &built).unwrap_or_else(|error| panic!("verify: {error:?}"));
    written
}

fn close(a: Affine3A, b: Affine3A) -> bool {
    a.to_cols_array()
        .iter()
        .zip(b.to_cols_array().iter())
        .all(|(x, y)| (x - y).abs() <= 1.0e-5 * (1.0 + y.abs()))
}

#[test]
fn empty_base_is_stable_bytes() {
    let first = empty_base();
    assert_eq!(first, empty_base());
    assert_eq!(&first[..4], b"glTF");
    assert_eq!(u32::from_le_bytes(first[4..8].try_into().unwrap()), 2);
    assert_eq!(
        u32::from_le_bytes(first[8..12].try_into().unwrap()) as usize,
        first.len()
    );
    let json_len = u32::from_le_bytes(first[12..16].try_into().unwrap()) as usize;
    assert_eq!(&first[16..20], b"JSON");
    // JSON チャンクだけで、BIN は無い。
    assert_eq!(first.len(), 20 + json_len);
    assert!(json_len.is_multiple_of(4));
    let json = std::str::from_utf8(&first[20..]).expect("UTF-8");
    assert_eq!(
        json.trim_end_matches(' '),
        r#"{"asset":{"generator":"model-reviewer","version":"2.0"},"scene":0,"scenes":[{"nodes":[]}]}"#
    );
}

#[test]
fn empty_base_is_not_openable_as_model() {
    assert_eq!(load_glb(&empty_base()), Err(LoadError::NoMesh));
}

#[test]
fn exports_from_empty_base() {
    let scene = one_shape();
    let written = export(&scene);
    let model = load_glb(&written).expect("開ける");
    assert_eq!(model.nodes.len(), 1);
    let node = &model.nodes[0];
    assert_eq!(node.name, "立方体");
    assert_eq!(node.parent, None);
    assert!(close(node.local, local()));
    assert!(close(node.world, local()));
    assert_eq!(model.draws.len(), 1);
    let color = model.draws[0].base_color;
    assert!(
        color
            .iter()
            .zip(COLOR.iter())
            .all(|(a, b)| a.to_bits() == b.to_bits()),
        "色 {color:?}"
    );
    assert_eq!(model.primitives.len(), 1);
    assert_eq!(model.stats.unique_triangles, 4);
    assert_eq!(model.stats.placed_triangles, 4);

    // 読み直した行列は、`verify_locals` が返す値と同じ。
    let base = empty_base();
    let built = build(&base, &scene).expect("build");
    let locals = verify_locals(&written, &base, &scene, &built).expect("verify");
    assert_eq!(locals.len(), 1);
    assert!(close(locals[0], local()));
}

#[test]
fn exports_a_group_with_children_from_empty_base() {
    let draw = |color: [f32; 4]| Draw {
        prim: PrimId(0),
        base_color: color,
        tex: None,
    };
    let scene = ExportScene {
        nodes: vec![
            added(None, "机", Affine3A::IDENTITY, Vec::new()),
            added(
                Some(0),
                "脚",
                Affine3A::from_translation(Vec3::X),
                vec![draw(COLOR)],
            ),
            added(
                Some(0),
                "天板",
                Affine3A::from_translation(Vec3::Y),
                vec![draw([0.2, 0.3, 0.4, 1.0])],
            ),
        ],
        added: vec![tetra()],
        base_primitive_count: 0,
    };
    let written = export(&scene);
    let model = load_glb(&written).expect("開ける");
    let names: Vec<&str> = model.nodes.iter().map(|node| node.name.as_str()).collect();
    assert_eq!(names, ["机", "脚", "天板"]);
    assert_eq!(
        model.nodes[1].parent,
        Some(model_reviewer_core::scene::NodeId(0))
    );
    assert_eq!(model.stats.placed_triangles, 8);
    // 形状は1回だけ書く。
    assert_eq!(model.primitives.len(), 1);
}

#[test]
fn same_document_same_bytes_from_empty_base() {
    let first = export(&one_shape());
    let second = export(&one_shape());
    assert_eq!(first, second);
}

#[test]
fn verify_returns_error_for_missing_shape_arrays() {
    let base = empty_base();
    let scene = one_shape();
    let built = build(&base, &scene).unwrap();
    for field in ["meshes", "materials", "accessors", "bufferViews", "buffers"] {
        let mut json: gltf::json::Value = gltf::json::deserialize::from_slice(&built.json).unwrap();
        json.as_object_mut().unwrap().remove(field);
        let broken = Built {
            json: gltf::json::serialize::to_vec(&json).unwrap(),
            ..built.clone()
        };
        assert!(
            matches!(
                verify(&write(&base, &broken), &base, &scene, &broken),
                Err(ExportError::Verify(_))
            ),
            "{field}"
        );
    }
}

#[test]
fn empty_scene_on_empty_base_is_refused() {
    let base = empty_base();
    let nothing = ExportScene::default();
    assert!(matches!(
        build(&base, &nothing),
        Err(ExportError::InvalidScene(_))
    ));
    // 描画の無い部品だけでも断る（開き直せないファイルを書かない）。
    let group_only = ExportScene {
        nodes: vec![added(None, "空の組", Affine3A::IDENTITY, Vec::new())],
        added: Vec::new(),
        base_primitive_count: 0,
    };
    assert!(matches!(
        build(&base, &group_only),
        Err(ExportError::InvalidScene(_))
    ));
    // ファイルにあるはずの番号を指す部品も断る（土台にはノードが無い）。
    let mut stray = one_shape();
    stray.nodes[0].file_index = Some(0);
    assert!(matches!(
        build(&base, &stray),
        Err(ExportError::InvalidScene(_))
    ));
}
