//! GLBの書き出し（`model_reviewer_gltf::export`）。文書（`core::doc`）は使わず、読み込んだ
//! `Model` から `ExportScene` を組み立てて試す。

use std::sync::Arc;

use glam::{Affine3A, Mat3, Quat, Vec3};
use gltf::json::Value;
use model_reviewer_core::math::Aabb;
use model_reviewer_core::scene::{
    Draw, LoadWarning, Model, ModelStats, Node, NodeId, PrimId, Primitive, Vertex,
};
use model_reviewer_core::texture::Textures;
use model_reviewer_gltf::export::{
    Built, ExportError, ExportNode, ExportNote, ExportScene, build, glb_length, synthesize_base,
    verify, write_container,
};
use model_reviewer_gltf::load_glb;
use model_reviewer_testdata::rich::{
    ARM, BIN_BYTE_LENGTH, BODY, HAND, JOINT, LOADED_NODES, ORPHAN, ROOT, SKEWED, SKINNED,
};
use model_reviewer_testdata::{nested_descendants, rich_glb, sample_glb, stress_nested_glb};

// ---- 試験の補助 ----

/// 読み込んだ `Model` をそのまま写した `ExportScene`（何も変えていない文書に当たる）。
fn scene_of(model: &Model) -> ExportScene {
    ExportScene {
        nodes: model
            .nodes
            .iter()
            .map(|node| ExportNode {
                parent: node.parent.map(|parent| parent.0),
                name: node.name.clone(),
                local: node.local,
                file_index: Some(node.gltf_index),
                template: None,
                draws: model.draws[node.draws.start as usize..node.draws.end as usize].to_vec(),
            })
            .collect(),
        added: Vec::new(),
        base_primitive_count: model.primitives.len() as u32,
        textures: model_reviewer_gltf::export::ExportTextures::from_tables(
            &model.textures.images,
            &model.textures.uses,
        ),
        originals: Default::default(),
    }
}

fn position(scene: &ExportScene, file: u32) -> usize {
    scene
        .nodes
        .iter()
        .position(|node| node.file_index == Some(file))
        .unwrap_or_else(|| panic!("node {file} not in scene"))
}

/// `index` の部分木の終わり（行きがけ順の次の兄弟の位置）。
fn subtree_end(scene: &ExportScene, index: usize) -> usize {
    let mut end = index + 1;
    while end < scene.nodes.len() && is_descendant(scene, end, index) {
        end += 1;
    }
    end
}

fn is_descendant(scene: &ExportScene, mut node: usize, ancestor: usize) -> bool {
    while let Some(parent) = scene.nodes[node].parent {
        if parent as usize == ancestor {
            return true;
        }
        node = parent as usize;
    }
    false
}

/// 部分木を取り除く（文書の削除に当たる）。親の番号を詰め直す。
fn remove_subtree(scene: &mut ExportScene, index: usize) {
    let end = subtree_end(scene, index);
    let removed = (end - index) as u32;
    scene.nodes.drain(index..end);
    for node in &mut scene.nodes {
        if let Some(parent) = node.parent.as_mut()
            && *parent as usize >= end
        {
            *parent -= removed;
        }
    }
}

/// `at` の位置に部品を差し込む（親の番号を詰め直す）。
fn insert(scene: &mut ExportScene, at: usize, nodes: Vec<ExportNode>) {
    let count = nodes.len() as u32;
    for node in &mut scene.nodes[at..] {
        if let Some(parent) = node.parent.as_mut()
            && *parent as usize >= at
        {
            *parent += count;
        }
    }
    scene.nodes.splice(at..at, nodes);
}

/// `parent` の最後の子として部品を足す位置。
fn last_child_slot(scene: &ExportScene, parent: usize) -> usize {
    subtree_end(scene, parent)
}

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

fn added_node(parent: Option<u32>, name: &str, local: Affine3A, draws: Vec<Draw>) -> ExportNode {
    ExportNode {
        parent,
        name: name.to_owned(),
        local,
        file_index: None,
        template: None,
        draws,
    }
}

fn export(base: &[u8], scene: &ExportScene) -> (Built, Vec<u8>) {
    let built = build(base, scene).unwrap_or_else(|error| panic!("build: {error:?}"));
    let written = write(base, &built);
    verify(&written, base, scene, &built).unwrap_or_else(|error| panic!("verify: {error:?}"));
    (built, written)
}

fn write(base: &[u8], built: &Built) -> Vec<u8> {
    let mut written = Vec::new();
    let length = write_container(&mut written, base, built).expect("write");
    assert_eq!(length, written.len() as u64);
    written
}

fn chunk(bytes: &[u8], at: usize) -> (&[u8], usize) {
    let len = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    (&bytes[at + 8..at + 8 + len], at + 8 + len)
}

fn json_of(glb: &[u8]) -> Value {
    let (json, _) = chunk(glb, 12);
    gltf::json::deserialize::from_slice(json).expect("json")
}

fn bin_of(glb: &[u8]) -> Vec<u8> {
    let (_, next) = chunk(glb, 12);
    if next == glb.len() {
        return Vec::new();
    }
    chunk(glb, next).0.to_vec()
}

/// 書いて読み直した値（数の読み方の丸めをそろえて比べる）。
fn reparse(value: &Value) -> Value {
    let bytes = gltf::json::serialize::to_vec(value).unwrap();
    gltf::json::deserialize::from_slice(&bytes).unwrap()
}

/// JSONの値とBINからGLBを作る。BINは0で4バイト境界に詰める。
fn glb(json: &Value, bin: Option<&[u8]>) -> Vec<u8> {
    let mut json = gltf::json::serialize::to_vec(json).unwrap();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut out = Vec::new();
    let bin = bin.map(|bin| {
        let mut bin = bin.to_vec();
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        bin
    });
    let total = 12 + 8 + json.len() + bin.as_ref().map_or(0, |bin| 8 + bin.len());
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    if let Some(bin) = bin {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
    }
    out
}

/// GLBのJSONだけを直したもの。
fn with_json(base: &[u8], edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut json = json_of(base);
    edit(&mut json);
    let bin = bin_of(base);
    glb(&json, (!bin.is_empty()).then_some(bin.as_slice()))
}

fn node(json: &Value, index: u32) -> &Value {
    &json["nodes"][index as usize]
}

fn nodes_len(json: &Value) -> usize {
    json["nodes"].as_array().unwrap().len()
}

fn translated(local: Affine3A, by: Vec3) -> Affine3A {
    Affine3A::from_translation(by) * local
}

fn translation_of(affine: Affine3A) -> Vec3 {
    affine.translation.into()
}

/// 元と書き出したJSONで、`skip` 以外の最上位の項目と、`changed` 以外のノードが等しいこと。
fn assert_only_nodes_changed(base: &Value, written: &Value, changed: &[u32]) {
    let base = reparse(base);
    for (key, value) in base.as_object().unwrap() {
        if key == "nodes" {
            continue;
        }
        assert_eq!(Some(value), written.get(key), "top-level {key}");
    }
    for (index, value) in base["nodes"].as_array().unwrap().iter().enumerate() {
        if changed.contains(&(index as u32)) {
            continue;
        }
        assert_eq!(value, &written["nodes"][index], "node {index}");
    }
}

fn rich() -> (Vec<u8>, Model, ExportScene) {
    let base = rich_glb();
    let model = load_glb(&base).expect("rich loads");
    let scene = scene_of(&model);
    (base, model, scene)
}

// ---- 入力の前提 ----

#[test]
fn rich_has_the_structures_the_export_must_keep() {
    let (base, model, scene) = rich();
    assert_eq!(model.nodes.len() as u32, LOADED_NODES);
    assert_eq!(scene.nodes.len() as u32, LOADED_NODES);
    let json = json_of(&base);
    for key in [
        "textures",
        "images",
        "samplers",
        "skins",
        "animations",
        "cameras",
        "extensions",
        "extensionsUsed",
        "extras",
    ] {
        assert!(json.get(key).is_some(), "{key}");
    }
    assert!(node(&json, BODY).get("matrix").is_some());
    assert!(node(&json, SKEWED).get("matrix").is_some());
    assert!(node(&json, JOINT).get("name").is_none());
    assert!(node(&json, HAND).get("extensions").is_some());
    assert_eq!(json["buffers"][0]["byteLength"], BIN_BYTE_LENGTH);
    assert!(bin_of(&base).len() > BIN_BYTE_LENGTH as usize);
    assert_eq!(json["scenes"].as_array().unwrap().len(), 2);
    assert_eq!(model.nodes[position(&scene, JOINT)].name, "ノード 5");
}

// ---- 1. 何も変えない ----

#[test]
fn unchanged_export_keeps_bin_json_and_model() {
    for base in [rich_glb(), sample_glb()] {
        let model = load_glb(&base).unwrap();
        let scene = scene_of(&model);
        let (built, written) = export(&base, &scene);
        assert!(built.notes.is_empty(), "{:?}", built.notes);
        assert!(built.appended_bin.is_empty());
        let expected: Vec<u32> = model.nodes.iter().map(|node| node.gltf_index).collect();
        assert_eq!(built.assigned, expected);
        assert_eq!(bin_of(&written), bin_of(&base));
        assert_eq!(json_of(&written), reparse(&json_of(&base)));
        assert_eq!(load_glb(&written).unwrap(), model);
    }
}

// ---- 2. 1個だけ動かす ----

#[test]
fn moving_one_node_rewrites_only_that_node_and_children_follow() {
    let (base, model, mut scene) = rich();
    let arm = position(&scene, ARM);
    let by = Vec3::new(0.0, 0.0, 2.0);
    scene.nodes[arm].local = translated(scene.nodes[arm].local, by);
    let (built, written) = export(&base, &scene);
    let json = json_of(&written);
    assert_only_nodes_changed(&json_of(&base), &json, &[ARM]);
    // 変えたのは変形だけで、extras や子は元のまま。
    let before = json_of(&base);
    assert_eq!(node(&json, ARM)["extras"], node(&before, ARM)["extras"]);
    assert_eq!(node(&json, ARM)["children"], node(&before, ARM)["children"]);
    assert_eq!(node(&json, ARM)["mesh"], node(&before, ARM)["mesh"]);
    assert_eq!(
        built.notes,
        vec![ExportNote::AnimatedNodesChanged { count: 1 }]
    );
    let reloaded = load_glb(&written).unwrap();
    let skinned = position(&scene, SKINNED);
    let moved =
        translation_of(reloaded.nodes[skinned].world) - translation_of(model.nodes[skinned].world);
    assert!((moved - by).length() < 1e-5, "child moved by {moved:?}");
    let body = position(&scene, BODY);
    assert_eq!(reloaded.nodes[body].world, model.nodes[body].world);
}

#[test]
fn skinned_and_renamed_nodes_are_reported_and_written() {
    let (base, _, mut scene) = rich();
    let skinned = position(&scene, SKINNED);
    scene.nodes[skinned].local = translated(scene.nodes[skinned].local, Vec3::X);
    let hand = position(&scene, HAND);
    scene.nodes[hand].name = "手".to_owned();
    // 名前の無いノードは、読み込みが付けた名前のままなら触らない。
    let joint = position(&scene, JOINT);
    assert_eq!(scene.nodes[joint].name, "ノード 5");
    let (built, written) = export(&base, &scene);
    assert_eq!(
        built.notes,
        vec![ExportNote::SkinnedNodesChanged { count: 1 }]
    );
    let json = json_of(&written);
    assert_only_nodes_changed(&json_of(&base), &json, &[SKINNED, HAND]);
    assert_eq!(node(&json, HAND)["name"], "手");
    assert!(node(&json, JOINT).get("name").is_none());
    assert_eq!(load_glb(&written).unwrap().nodes[hand].name, "手");
}

// ---- 3. matrix で書かれたノード ----

#[test]
fn moved_matrix_nodes_use_trs_when_possible_and_matrix_otherwise() {
    let (base, model, mut scene) = rich();
    let body = position(&scene, BODY);
    scene.nodes[body].local = translated(scene.nodes[body].local, Vec3::new(0.5, 0.0, 0.0));
    let (built, written) = export(&base, &scene);
    assert!(built.notes.is_empty(), "{:?}", built.notes);
    let json = json_of(&written);
    assert_only_nodes_changed(&json_of(&base), &json, &[BODY]);
    let body_json = node(&json, BODY);
    assert!(body_json.get("matrix").is_none());
    assert_eq!(body_json["translation"], serde_vec(&[0.5, 1.0, 0.0]));
    assert_eq!(body_json["scale"], serde_vec(&[2.0, 2.0, 2.0]));
    assert!(body_json.get("rotation").is_none());
    // 動かしていない matrix のノードは触らない（上の assert_only_nodes_changed に含む）。
    assert_eq!(node(&json, SKEWED), &reparse(node(&json_of(&base), SKEWED)));
    let hand = position(&scene, HAND);
    let reloaded = load_glb(&written).unwrap();
    let moved =
        translation_of(reloaded.nodes[hand].world) - translation_of(model.nodes[hand].world);
    assert!((moved - Vec3::new(0.5, 0.0, 0.0)).length() < 1e-5);

    // 傾いた拡大は分けられないので matrix で書き、一覧に出す。
    let skewed = position(&scene, SKEWED);
    scene.nodes[skewed].local = translated(scene.nodes[skewed].local, Vec3::Y);
    let (built, written) = export(&base, &scene);
    assert_eq!(built.notes, vec![ExportNote::MatrixWritten { count: 1 }]);
    let json = json_of(&written);
    let skewed_json = node(&json, SKEWED);
    assert!(skewed_json.get("translation").is_none());
    let matrix: Vec<f64> = skewed_json["matrix"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_f64().unwrap())
        .collect();
    assert_eq!(
        matrix,
        vec![
            1.0, 0.0, 0.0, 0.0, 0.5, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 3.0, 1.0, 0.0, 1.0
        ]
    );
}

#[test]
fn rotation_and_negative_scale_round_trip_through_trs() {
    let (base, _, mut scene) = rich();
    let hand = position(&scene, HAND);
    scene.nodes[hand].local = Affine3A::from_scale_rotation_translation(
        Vec3::new(-1.0, 2.0, 0.5),
        Quat::from_rotation_y(0.7) * Quat::from_rotation_x(-0.3),
        Vec3::new(1.0, -2.0, 3.0),
    );
    let (built, written) = export(&base, &scene);
    assert!(built.notes.is_empty(), "{:?}", built.notes);
    let json = json_of(&written);
    assert!(node(&json, HAND).get("matrix").is_none());
    assert!(node(&json, HAND).get("rotation").is_some());
    let reloaded = load_glb(&written).unwrap();
    let local = reloaded.nodes[hand].local.to_cols_array();
    let want = scene.nodes[hand].local.to_cols_array();
    for (got, want) in local.iter().zip(want) {
        assert!((got - want).abs() < 1e-5, "{local:?}");
    }
}

// ---- 4. 複製 ----

#[test]
fn duplicates_copy_the_template_json_and_share_its_mesh() {
    let (base, _, mut scene) = rich();
    // 腕（子にスキン付きの部品）を部分木ごと複製して、根の最後の子にする。
    let arm = position(&scene, ARM);
    let skinned = position(&scene, SKINNED);
    let root = position(&scene, ROOT) as u32;
    let slot = last_child_slot(&scene, root as usize);
    let mut copy_arm = scene.nodes[arm].clone();
    copy_arm.parent = Some(root);
    copy_arm.file_index = None;
    copy_arm.template = Some(ARM);
    copy_arm.name = "arm のコピー".to_owned();
    copy_arm.local = translated(copy_arm.local, Vec3::new(0.0, 0.0, -1.0));
    let mut copy_skinned = scene.nodes[skinned].clone();
    copy_skinned.parent = Some(slot as u32);
    copy_skinned.file_index = None;
    copy_skinned.template = Some(SKINNED);
    // 名前の無い関節も複製する（名前は空のまま）。
    let joint = position(&scene, JOINT);
    let mut copy_joint = scene.nodes[joint].clone();
    copy_joint.parent = Some(root);
    copy_joint.file_index = None;
    copy_joint.template = Some(JOINT);
    copy_joint.name = String::new();
    insert(&mut scene, slot, vec![copy_arm, copy_skinned, copy_joint]);
    let (built, written) = export(&base, &scene);
    assert!(built.notes.is_empty(), "{:?}", built.notes);
    assert_eq!(&built.assigned[slot..slot + 3], &[8, 9, 10]);
    let json = json_of(&written);
    let before = json_of(&base);
    assert_eq!(nodes_len(&json), 11);
    assert_only_nodes_changed(&before, &json, &[ROOT, 8, 9, 10]);
    assert_eq!(
        node(&json, ROOT)["children"],
        serde_uints(&[1, 2, 5, 6, 8, 10])
    );
    let copy = node(&json, 8);
    assert_eq!(copy["mesh"], node(&before, ARM)["mesh"]);
    assert_eq!(copy["extras"], node(&before, ARM)["extras"]);
    assert_eq!(copy["children"], serde_uints(&[9]));
    assert_eq!(copy["name"], "arm のコピー");
    assert_eq!(node(&json, 9)["skin"], node(&before, SKINNED)["skin"]);
    assert_eq!(node(&json, 9)["name"], "skinned");
    assert!(node(&json, 10).get("name").is_none());
    assert_eq!(node(&json, 10)["camera"], 0);
    // 既存の番号を指すもの（アニメーション・スキン・拡張）は変わらない。
    for key in ["animations", "skins", "extensions", "meshes", "accessors"] {
        assert_eq!(json[key], reparse(&before[key]), "{key}");
    }
    let reloaded = load_glb(&written).unwrap();
    assert_eq!(reloaded.nodes.len(), LOADED_NODES as usize + 3);
    assert_eq!(reloaded.nodes[slot + 2].name, "ノード 10");
    assert_eq!(
        reloaded.stats.primitives,
        load_glb(&base).unwrap().stats.primitives
    );
}

// ---- 5. 図形を足す ----

#[test]
fn added_shapes_append_geometry_and_round_trip() {
    let (base, model, mut scene) = rich();
    let prim = PrimId(scene.base_primitive_count);
    scene.added.push(tetra());
    let red = [0.9, 0.1, 0.1, 1.0];
    let blue = [0.1, 0.1, 0.9, 0.5];
    let root = position(&scene, ROOT) as u32;
    let slot = last_child_slot(&scene, root as usize);
    let place = |x: f32| Affine3A::from_translation(Vec3::new(x, 0.0, 0.0));
    insert(
        &mut scene,
        slot,
        vec![
            added_node(
                Some(root),
                "四面体",
                place(1.0),
                vec![Draw {
                    prim,
                    base_color: red,
                    tex: None,
                }],
            ),
            added_node(
                Some(root),
                "四面体 2",
                place(2.0),
                vec![Draw {
                    prim,
                    base_color: red,
                    tex: None,
                }],
            ),
            added_node(
                Some(root),
                "",
                place(3.0),
                vec![Draw {
                    prim,
                    base_color: blue,
                    tex: None,
                }],
            ),
        ],
    );
    let (built, written) = export(&base, &scene);
    assert!(built.notes.is_empty(), "{:?}", built.notes);
    let before = json_of(&base);
    let json = json_of(&written);
    let len = |value: &Value, key: &str| value[key].as_array().unwrap().len();
    // 形状は1回、色は2つ、描画の並びは2通り。
    assert_eq!(len(&json, "accessors"), len(&before, "accessors") + 3);
    assert_eq!(len(&json, "bufferViews"), len(&before, "bufferViews") + 3);
    assert_eq!(len(&json, "materials"), len(&before, "materials") + 2);
    assert_eq!(len(&json, "meshes"), len(&before, "meshes") + 2);
    assert_eq!(node(&json, 8)["mesh"], node(&json, 9)["mesh"]);
    assert_ne!(node(&json, 8)["mesh"], node(&json, 10)["mesh"]);
    for key in ["accessors", "bufferViews", "materials", "meshes"] {
        let old = reparse(&before[key]);
        let old = old.as_array().unwrap();
        assert_eq!(
            &json[key].as_array().unwrap()[..old.len()],
            &old[..],
            "{key}"
        );
    }
    let mesh = &json["meshes"][node(&json, 8)["mesh"].as_u64().unwrap() as usize];
    let position_accessor = &json["accessors"][mesh["primitives"][0]["attributes"]["POSITION"]
        .as_u64()
        .unwrap() as usize];
    assert_eq!(position_accessor["min"], serde_vec(&[0.0, 0.0, -0.75]));
    assert_eq!(position_accessor["max"], serde_vec(&[1.0, 1.5, 0.0]));
    assert_eq!(position_accessor["componentType"], 5126);
    // BINは元の範囲がそのままで、足した分は4バイト境界から始まる。
    let base_bin = bin_of(&base);
    let bin = bin_of(&written);
    assert_eq!(&bin[..base_bin.len()], &base_bin[..]);
    assert_eq!(&bin[base_bin.len()..], &built.appended_bin[..]);
    assert_eq!(
        json["buffers"][0]["byteLength"],
        (base_bin.len() + built.appended_bin.len()) as u64
    );
    for view in &json["bufferViews"].as_array().unwrap()[len(&before, "bufferViews")..] {
        assert!(view["byteOffset"].as_u64().unwrap() >= base_bin.len() as u64);
        assert_eq!(view["byteOffset"].as_u64().unwrap() % 4, 0);
    }
    for material in &json["materials"].as_array().unwrap()[len(&before, "materials")..] {
        assert_eq!(
            material["pbrMetallicRoughness"]["metallicFactor"].as_f64(),
            Some(0.0)
        );
    }
    let reloaded = load_glb(&written).unwrap();
    let added = &reloaded.primitives[reloaded.draws[reloaded.nodes[slot].draws.start as usize]
        .prim
        .0 as usize];
    assert_eq!(added.vertices, tetra().vertices);
    assert_eq!(added.indices, tetra().indices);
    assert_eq!(reloaded.nodes[slot + 2].name, "ノード 10");
    assert_eq!(
        reloaded.stats.placed_triangles,
        model.stats.placed_triangles + 3 * 4
    );
    assert_eq!(
        reloaded.draws[reloaded.nodes[slot + 2].draws.start as usize].base_color,
        blue
    );
}

#[test]
fn a_node_without_children_can_get_a_child_and_an_empty_group_needs_no_mesh() {
    let (base, _, mut scene) = rich();
    let prim = PrimId(scene.base_primitive_count);
    scene.added.push(tetra());
    let skinned = position(&scene, SKINNED) as u32;
    let slot = last_child_slot(&scene, skinned as usize);
    insert(
        &mut scene,
        slot,
        vec![
            added_node(Some(skinned), "まとめ", Affine3A::IDENTITY, Vec::new()),
            added_node(
                Some(slot as u32),
                "中身",
                Affine3A::from_rotation_z(0.5),
                vec![Draw {
                    prim,
                    base_color: [1.0; 4],
                    tex: None,
                }],
            ),
        ],
    );
    let (_, written) = export(&base, &scene);
    let json = json_of(&written);
    assert_eq!(node(&json, SKINNED)["children"], serde_uints(&[8]));
    assert!(node(&json, 8).get("mesh").is_none());
    assert_eq!(node(&json, 8)["children"], serde_uints(&[9]));
    assert!(node(&json, 9).get("children").is_none());
}

// ---- 6. 削除 ----

#[test]
fn deleting_detaches_from_the_parent_and_keeps_the_nodes() {
    let (base, _, mut scene) = rich();
    let arm = position(&scene, ARM);
    remove_subtree(&mut scene, arm);
    let (built, written) = export(&base, &scene);
    assert_eq!(built.notes, vec![ExportNote::DetachedNodes { count: 2 }]);
    let before = json_of(&base);
    let json = json_of(&written);
    assert_eq!(nodes_len(&json), nodes_len(&before));
    assert_only_nodes_changed(&before, &json, &[ROOT]);
    assert_eq!(node(&json, ROOT)["children"], serde_uints(&[1, 5, 6]));
    // 外した部品のデータは残る（子の番号も、アニメーションの対象も元のまま）。
    assert_eq!(node(&json, ARM), &reparse(node(&before, ARM)));
    assert_eq!(json["animations"], reparse(&before["animations"]));
    assert_eq!(json["animations"][0]["channels"][0]["target"]["node"], ARM);
    let reloaded = load_glb(&written).unwrap();
    assert_eq!(reloaded.nodes.len(), LOADED_NODES as usize - 2);
    assert!(reloaded.nodes.iter().all(|node| node.name != "arm"));
}

#[test]
fn deleting_a_root_rewrites_only_the_scene_in_use() {
    let (base, _, mut scene) = rich();
    // 根を消すと表示できる物が無くなるので、先に足してから根を消す。
    let prim = PrimId(scene.base_primitive_count);
    scene.added.push(tetra());
    scene.nodes.push(added_node(
        None,
        "新しい根",
        Affine3A::IDENTITY,
        vec![Draw {
            prim,
            base_color: [1.0; 4],
            tex: None,
        }],
    ));
    let at = position(&scene, ROOT);
    remove_subtree(&mut scene, at);
    let (built, written) = export(&base, &scene);
    assert_eq!(
        built.notes,
        vec![ExportNote::DetachedNodes {
            count: LOADED_NODES
        }]
    );
    let json = json_of(&written);
    let before = reparse(&json_of(&base));
    assert_eq!(json["scenes"][0]["nodes"], serde_uints(&[8]));
    assert_eq!(json["scenes"][0]["extras"], before["scenes"][0]["extras"]);
    assert_eq!(json["scenes"][1], before["scenes"][1]);
    assert_eq!(json["scene"], 0);
    assert_eq!(load_glb(&written).unwrap().nodes.len(), 1);
}

// ---- 7. 書き出したファイルを土台にもう一度 ----

#[test]
fn re_export_after_undo_detaches_and_redo_restores() {
    let (base, _, mut scene) = rich();
    let prim = PrimId(scene.base_primitive_count);
    scene.added.push(tetra());
    let root = position(&scene, ROOT) as u32;
    let slot = last_child_slot(&scene, root as usize);
    insert(
        &mut scene,
        slot,
        vec![added_node(
            Some(root),
            "足した物",
            Affine3A::from_translation(Vec3::Z),
            vec![Draw {
                prim,
                base_color: [0.5, 0.5, 0.5, 1.0],
                tex: None,
            }],
        )],
    );
    let (built, first) = export(&base, &scene);
    let added_index = built.assigned[slot];

    // 書き出した後の文書：足した物はファイルにある部品になる（番号を受け取る）。
    let after = load_glb(&first).unwrap();
    let redo = scene_of(&after);
    assert_eq!(redo.nodes[slot].file_index, Some(added_index));

    // 足した物を取り消してから書く → シーンから外れる（データは残る）。
    let mut undone = redo.clone();
    remove_subtree(&mut undone, slot);
    let (built, second) = export(&first, &undone);
    assert_eq!(built.notes, vec![ExportNote::DetachedNodes { count: 1 }]);
    assert_eq!(nodes_len(&json_of(&second)), nodes_len(&json_of(&first)));
    assert_eq!(
        load_glb(&second).unwrap().nodes.len(),
        LOADED_NODES as usize
    );

    // やり直してから書く → 1回目に書いた物と同じ中身に戻る。
    let (built, third) = export(&first, &redo);
    assert!(built.notes.is_empty());
    assert_eq!(json_of(&third), reparse(&json_of(&first)));
    assert_eq!(bin_of(&third), bin_of(&first));
    // 外れたファイルを土台にしても、やり直した文書を書けば同じ中身に戻る。
    let (_, fourth) = export(&second, &redo);
    assert_eq!(json_of(&fourth), reparse(&json_of(&first)));
    assert_eq!(load_glb(&fourth).unwrap(), after);
}

// ---- シーンが無い・複数ある ----

fn without_scenes() -> Vec<u8> {
    with_json(&rich_glb(), |json| {
        let map = json.as_object_mut().unwrap();
        map.remove("scenes");
        map.remove("scene");
    })
}

#[test]
fn a_file_without_scenes_gets_a_scene_only_when_the_roots_change() {
    let base = without_scenes();
    let model = load_glb(&base).unwrap();
    let scene = scene_of(&model);
    // 親の無いノード（根と、もとはシーン1にいた ORPHAN）がすべて根になる。
    assert_eq!(scene.nodes.len() as u32, LOADED_NODES + 1);
    let (built, written) = export(&base, &scene);
    assert!(built.notes.is_empty());
    assert!(json_of(&written).get("scenes").is_none());

    // 新しい根を足す：親が無いので、シーンが無くても根として読まれる。
    let mut grown = scene.clone();
    let prim = PrimId(grown.base_primitive_count);
    grown.added.push(tetra());
    grown.nodes.push(added_node(
        None,
        "足した根",
        Affine3A::IDENTITY,
        vec![Draw {
            prim,
            base_color: [1.0; 4],
            tex: None,
        }],
    ));
    let (built, written) = export(&base, &grown);
    assert!(built.notes.is_empty(), "{:?}", built.notes);
    assert!(json_of(&written).get("scenes").is_none());

    // 根を外す：ノードは残り、親が無いので根として読まれてしまう → シーンを足す。
    let mut shrunk = scene.clone();
    let at = position(&shrunk, ORPHAN);
    remove_subtree(&mut shrunk, at);
    let (built, written) = export(&base, &shrunk);
    assert_eq!(
        built.notes,
        vec![
            ExportNote::DetachedNodes { count: 1 },
            ExportNote::SceneAdded
        ]
    );
    let json = json_of(&written);
    assert_eq!(json["scenes"], serde_json_scenes(&[0]));
    assert_eq!(json["scene"], 0);
    assert_eq!(
        load_glb(&written).unwrap().nodes.len(),
        LOADED_NODES as usize
    );

    // 子を外す：外した子は親を失い、根として読まれてしまう → シーンを足す。
    let mut child_gone = scene.clone();
    let at = position(&child_gone, HAND);
    remove_subtree(&mut child_gone, at);
    let (built, written) = export(&base, &child_gone);
    assert_eq!(
        built.notes,
        vec![
            ExportNote::DetachedNodes { count: 1 },
            ExportNote::SceneAdded
        ]
    );
    assert_eq!(json_of(&written)["scenes"], serde_json_scenes(&[0, 7]));
}

#[test]
fn only_the_default_scene_is_rewritten_when_there_are_several() {
    let base = with_json(&rich_glb(), |json| {
        json["scene"] = Value::from(1u64);
    });
    let model = load_glb(&base).unwrap();
    let mut scene = scene_of(&model);
    assert_eq!(scene.nodes.len(), 1);
    let prim = PrimId(scene.base_primitive_count);
    scene.added.push(tetra());
    scene.nodes.push(added_node(
        None,
        "足した根",
        Affine3A::IDENTITY,
        vec![Draw {
            prim,
            base_color: [1.0; 4],
            tex: None,
        }],
    ));
    let (_, written) = export(&base, &scene);
    let json = json_of(&written);
    let before = reparse(&json_of(&base));
    assert_eq!(json["scenes"][0], before["scenes"][0]);
    assert_eq!(json["scenes"][1]["nodes"], serde_uints(&[ORPHAN, 8]));
    assert_eq!(json["scene"], 1);
}

// ---- 共有された形状 ----

#[test]
fn moving_one_of_two_nodes_sharing_a_mesh_leaves_the_mesh_alone() {
    let (base, model, mut scene) = rich();
    let hand = position(&scene, HAND);
    scene.nodes[hand].local = translated(scene.nodes[hand].local, Vec3::X);
    let (_, written) = export(&base, &scene);
    let json = json_of(&written);
    assert_only_nodes_changed(&json_of(&base), &json, &[HAND]);
    let reloaded = load_glb(&written).unwrap();
    let arm = position(&scene, ARM);
    assert_eq!(reloaded.nodes[arm].world, model.nodes[arm].world);
    assert_eq!(reloaded.primitives, model.primitives);
}

// ---- 断る場合（ファイルを扱わない分） ----

#[test]
fn bases_the_loader_rejects_are_refused_with_a_reason() {
    let rich = rich_glb();
    let no_bin = glb(&json_of(&rich), None);
    let uri = with_json(&rich, |json| {
        json["buffers"][0]["uri"] = Value::from("data.bin")
    });
    let two = with_json(&rich, |json| {
        let extra = json["buffers"][0].clone();
        json["buffers"].as_array_mut().unwrap().push(extra);
    });
    let mut trailing = rich.clone();
    trailing.extend_from_slice(&[4, 0, 0, 0]);
    trailing.extend_from_slice(b"XTRA");
    trailing.extend_from_slice(&[0; 4]);
    let total = trailing.len() as u32;
    trailing[8..12].copy_from_slice(&total.to_le_bytes());
    let scene = scene_of(&load_glb(&rich).unwrap());
    for (what, base) in [
        ("no BIN", no_bin),
        ("uri", uri),
        ("two buffers", two),
        ("chunk after BIN", trailing),
    ] {
        match build(&base, &scene) {
            Err(ExportError::InvalidBase(reason)) => {
                assert!(!reason.is_empty(), "{what}");
                let message = ExportError::InvalidBase(reason).message_ja();
                assert!(message.contains("土台"), "{what}: {message}");
            }
            other => panic!("{what}: {other:?}"),
        }
    }
}

#[test]
fn scenes_that_do_not_fit_the_file_are_refused() {
    let (base, _, scene) = rich();
    let mut cases: Vec<(&str, ExportScene)> = Vec::new();
    let mut bad = scene.clone();
    bad.nodes.swap(0, 1);
    cases.push(("order", bad));
    let mut bad = scene.clone();
    bad.nodes[2].file_index = bad.nodes[1].file_index;
    cases.push(("twice", bad));
    let mut bad = scene.clone();
    bad.nodes[1].file_index = Some(99);
    cases.push(("out of range", bad));
    let mut bad = scene.clone();
    bad.nodes[1].template = Some(0);
    cases.push(("file and template", bad));
    let mut bad = scene.clone();
    bad.nodes.push(ExportNode {
        template: Some(99),
        file_index: None,
        ..bad.nodes[0].clone()
    });
    cases.push(("template", bad));
    let mut bad = scene.clone();
    bad.nodes.push(added_node(
        None,
        "x",
        Affine3A::IDENTITY,
        vec![Draw {
            prim: PrimId(0),
            base_color: [1.0; 4],
            tex: None,
        }],
    ));
    cases.push(("added points at the file", bad));
    let mut bad = scene.clone();
    for node in &mut bad.nodes {
        node.draws.clear();
    }
    cases.push(("nothing to show", bad));
    let mut bad = scene.clone();
    let prim = PrimId(bad.base_primitive_count);
    bad.added.push(Arc::new(Primitive {
        indices: vec![0, 1, 9],
        ..(*tetra()).clone()
    }));
    bad.nodes.push(added_node(
        None,
        "x",
        Affine3A::IDENTITY,
        vec![Draw {
            prim,
            base_color: [1.0; 4],
            tex: None,
        }],
    ));
    cases.push(("index past vertices", bad));
    let mut bad = scene.clone();
    bad.nodes[1].local = Affine3A::from_translation(Vec3::new(f32::NAN, 0.0, 0.0));
    cases.push(("not finite", bad));
    for (what, scene) in cases {
        match build(&base, &scene) {
            Err(ExportError::InvalidScene(reason)) => assert!(!reason.is_empty(), "{what}"),
            other => panic!("{what}: {other:?}"),
        }
    }
}

#[test]
fn the_four_gib_limit_is_checked_before_writing() {
    let limit = u64::from(u32::MAX);
    // ヘッダ12 + JSONチャンクの頭8 + JSON 4 + BINチャンクの頭8 = 32。BINは4バイト境界。
    let max_bin = (limit - 32) & !3;
    assert_eq!(glb_length(4, Some(max_bin)), Ok(32 + max_bin));
    let error = glb_length(4, Some(max_bin + 4)).unwrap_err();
    assert!(matches!(error, ExportError::TooLarge { bytes } if bytes > limit));
    // 詰め物で境界を越える場合も数える。
    assert!(glb_length(4, Some(max_bin + 1)).is_err());
    assert!(glb_length(limit - 20 - 2, None).is_err());
    assert!(glb_length((limit - 20) & !3, None).is_ok());
    assert!(error.message_ja().contains("4GiB"));
    assert!(glb_length(limit, None).is_err());
    assert!(glb_length(u64::MAX, Some(u64::MAX)).is_err());
}

// ---- 照合が壊れた結果を必ず落とす ----

/// JSONの値を壊す手順。
type Edit = Box<dyn FnOnce(&mut Value)>;

/// 組み立てた結果のJSONを壊し、壊れた組み立てとして書いた物を照合させる。
fn broken_json(
    base: &[u8],
    scene: &ExportScene,
    built: &Built,
    edit: impl FnOnce(&mut Value),
) -> Result<(), ExportError> {
    let mut value: Value = gltf::json::deserialize::from_slice(&built.json).unwrap();
    edit(&mut value);
    let broken = Built {
        json: gltf::json::serialize::to_vec(&value).unwrap(),
        ..built.clone()
    };
    let written = write(base, &broken);
    verify(&written, base, scene, &broken)
}

fn assert_rejected(what: &str, result: Result<(), ExportError>) {
    match result {
        Err(ExportError::Verify(reason)) => assert!(!reason.is_empty(), "{what}"),
        other => panic!("{what}: verify accepted a broken result: {other:?}"),
    }
}

/// 腕を動かし、図形を足し、関節を外した文書（照合の壊し方の土台）。
fn edited() -> (Vec<u8>, ExportScene, Built, Vec<u8>) {
    let (base, _, mut scene) = rich();
    let arm = position(&scene, ARM);
    scene.nodes[arm].local = translated(scene.nodes[arm].local, Vec3::Y);
    let joint = position(&scene, JOINT);
    remove_subtree(&mut scene, joint);
    let prim = PrimId(scene.base_primitive_count);
    scene.added.push(tetra());
    let root = position(&scene, ROOT) as u32;
    let slot = last_child_slot(&scene, root as usize);
    insert(
        &mut scene,
        slot,
        vec![added_node(
            Some(root),
            "足した",
            Affine3A::IDENTITY,
            vec![Draw {
                prim,
                base_color: [1.0; 4],
                tex: None,
            }],
        )],
    );
    let (built, written) = export(&base, &scene);
    (base, scene, built, written)
}

#[test]
fn verify_rejects_broken_containers_and_bin() {
    let (base, scene, built, written) = edited();
    let json_len = u32::from_le_bytes(written[12..16].try_into().unwrap()) as usize;
    let bin_start = 20 + json_len + 8;

    let mut bad = written.clone();
    bad[bin_start + 5] ^= 0xff;
    assert_rejected("original BIN byte", verify(&bad, &base, &scene, &built));

    let mut bad = written.clone();
    let last = bad.len() - 1;
    bad[last] ^= 0xff;
    assert_rejected("appended BIN byte", verify(&bad, &base, &scene, &built));

    let mut bad = written.clone();
    bad[8] ^= 1;
    assert_rejected("header length", verify(&bad, &base, &scene, &built));

    let mut bad = written.clone();
    bad[20] = b'[';
    assert_rejected("JSON chunk", verify(&bad, &base, &scene, &built));

    assert_rejected(
        "truncated",
        verify(&written[..written.len() - 4], &base, &scene, &built),
    );

    // 足すBINを壊した組み立て（形状が文書と違う）。
    let mut broken = built.clone();
    broken.appended_bin[0] ^= 0x40;
    let bad = write(&base, &broken);
    assert_rejected("appended geometry", verify(&bad, &base, &scene, &broken));

    // 番号の割り当てを壊した組み立て。
    let mut broken = built.clone();
    let last = broken.assigned.len() - 1;
    broken.assigned.swap(0, last);
    assert_rejected("assigned", verify(&written, &base, &scene, &broken));

    // 別の文書と照らす。
    let mut other = scene.clone();
    other.nodes[1].local = translated(other.nodes[1].local, Vec3::X);
    assert_rejected("other scene", verify(&written, &base, &other, &built));
}

#[test]
fn verify_rejects_json_that_changed_what_must_stay() {
    let (base, scene, built, _) = edited();
    let cases: Vec<(&str, Edit)> = vec![
        (
            "top-level animations",
            Box::new(|json| {
                json["animations"][0]["channels"][0]["target"]["node"] = Value::from(3u64)
            }),
        ),
        (
            "top-level extras",
            Box::new(|json| json["extras"]["author"] = Value::from("x")),
        ),
        (
            "unknown extension",
            Box::new(|json| {
                json["extensions"]["EXT_testdata_unknown"]["nodes"][0] = Value::from(4u64)
            }),
        ),
        (
            "extras dropped",
            Box::new(|json| {
                json.as_object_mut().unwrap().remove("extras");
            }),
        ),
        (
            "untouched node extras",
            Box::new(|json| json["nodes"][HAND as usize]["extensions"] = Value::Null),
        ),
        (
            "untouched node transform",
            Box::new(|json| json["nodes"][SKEWED as usize]["matrix"][0] = Value::from(1.5)),
        ),
        (
            "moved node mesh",
            Box::new(|json| json["nodes"][ARM as usize]["mesh"] = Value::from(0u64)),
        ),
        (
            "moved node extras",
            Box::new(|json| {
                json["nodes"][ARM as usize]
                    .as_object_mut()
                    .unwrap()
                    .remove("extras");
            }),
        ),
        (
            "moved node translation",
            Box::new(|json| json["nodes"][ARM as usize]["translation"][0] = Value::from(9.0)),
        ),
        (
            "untouched name",
            Box::new(|json| json["nodes"][BODY as usize]["name"] = Value::from("x")),
        ),
        (
            "detached node kept",
            Box::new(|json| json["nodes"][JOINT as usize]["camera"] = Value::from(1u64)),
        ),
        (
            "parent children",
            Box::new(|json| {
                json["nodes"][ROOT as usize]["children"] = serde_uints(&[1, 2, 6, 8, 5])
            }),
        ),
        (
            "extra detach",
            Box::new(|json| json["nodes"][ROOT as usize]["children"] = serde_uints(&[1, 2, 8])),
        ),
        (
            "other scene",
            Box::new(|json| json["scenes"][1]["nodes"] = serde_uints(&[0])),
        ),
        (
            "scene extras",
            Box::new(|json| json["scenes"][0]["extras"] = Value::Null),
        ),
        (
            "default scene",
            Box::new(|json| json["scene"] = Value::from(1u64)),
        ),
        (
            "old accessor",
            Box::new(|json| json["accessors"][0]["max"][0] = Value::from(2.0)),
        ),
        (
            "old material",
            Box::new(|json| json["materials"][0]["extras"] = Value::Null),
        ),
        (
            "old view",
            Box::new(|json| json["bufferViews"][0]["byteLength"] = Value::from(40u64)),
        ),
        (
            "buffer length",
            Box::new(|json| json["buffers"][0]["byteLength"] = Value::from(10_000u64)),
        ),
        (
            "buffer extras",
            Box::new(|json| json["buffers"][0]["extras"] = Value::Null),
        ),
        (
            "new view into old BIN",
            Box::new(|json| {
                let views = json["bufferViews"].as_array_mut().unwrap();
                let last = views.len() - 1;
                views[last]["byteOffset"] = Value::from(0u64);
            }),
        ),
        (
            "new node name",
            Box::new(|json| json["nodes"][8]["name"] = Value::from("y")),
        ),
        (
            "new node unknown key",
            Box::new(|json| json["nodes"][8]["extras"] = Value::from(1u64)),
        ),
        (
            "new node mesh dropped",
            Box::new(|json| {
                json["nodes"][8].as_object_mut().unwrap().remove("mesh");
            }),
        ),
        (
            "new material color",
            Box::new(|json| {
                let materials = json["materials"].as_array_mut().unwrap();
                let last = materials.len() - 1;
                materials[last]["pbrMetallicRoughness"]["baseColorFactor"][0] = Value::from(0.5);
            }),
        ),
        (
            "new material metallicFactor changed",
            Box::new(|json| {
                let materials = json["materials"].as_array_mut().unwrap();
                let last = materials.len() - 1;
                materials[last]["pbrMetallicRoughness"]["metallicFactor"] = Value::from(1.0);
            }),
        ),
        (
            "new material metallicFactor dropped",
            Box::new(|json| {
                let materials = json["materials"].as_array_mut().unwrap();
                let last = materials.len() - 1;
                materials[last]["pbrMetallicRoughness"]
                    .as_object_mut()
                    .unwrap()
                    .remove("metallicFactor");
            }),
        ),
        (
            "extra node",
            Box::new(|json| {
                let extra = json["nodes"][0].clone();
                json["nodes"].as_array_mut().unwrap().push(extra);
            }),
        ),
    ];
    for (what, edit) in cases {
        assert_rejected(what, broken_json(&base, &scene, &built, edit));
    }
}

#[test]
fn verify_rejects_broken_duplicates_and_scene_changes() {
    let (base, _, mut scene) = rich();
    let hand = position(&scene, HAND);
    let root = position(&scene, ROOT) as u32;
    let slot = last_child_slot(&scene, root as usize);
    let mut copy = scene.nodes[hand].clone();
    copy.parent = Some(root);
    copy.file_index = None;
    copy.template = Some(HAND);
    insert(&mut scene, slot, vec![copy]);
    let (built, _) = export(&base, &scene);
    assert_rejected(
        "duplicate lost its extension",
        broken_json(&base, &scene, &built, |json| {
            json["nodes"][8]
                .as_object_mut()
                .unwrap()
                .remove("extensions");
        }),
    );
    assert_rejected(
        "duplicate points at another mesh",
        broken_json(&base, &scene, &built, |json| {
            json["nodes"][8]["mesh"] = Value::from(0u64)
        }),
    );

    // シーンの無いファイルで、足したシーンを壊す。
    let base = without_scenes();
    let mut scene = scene_of(&load_glb(&base).unwrap());
    let at = position(&scene, ORPHAN);
    remove_subtree(&mut scene, at);
    let (built, _) = export(&base, &scene);
    assert_rejected(
        "added scene roots",
        broken_json(&base, &scene, &built, |json| {
            json["scenes"][0]["nodes"] = serde_uints(&[0, 7])
        }),
    );
    let mut no_scene = built.clone();
    no_scene
        .notes
        .retain(|note| *note != ExportNote::SceneAdded);
    assert_rejected(
        "scene added without saying so",
        verify(&write(&base, &no_scene), &base, &scene, &no_scene),
    );
}

// ---- 重いモデル（WP-T1） ----

#[test]
fn nested_stress_model_has_half_of_the_parts_under_the_group_and_exports() {
    let parts = 100_000;
    let base = stress_nested_glb(parts, 1).unwrap();
    let model = load_glb(&base).unwrap();
    assert_eq!(model.nodes.len(), parts as usize);
    let group = model
        .nodes
        .iter()
        .position(|node| node.name == "group-half")
        .unwrap();
    let descendants = model.nodes[group].subtree_end as usize - group - 1;
    assert_eq!(descendants, nested_descendants(parts) as usize);
    assert_eq!(descendants, 50_000);
    assert_eq!(model.nodes[group].parent.map(|parent| parent.0), Some(0));
    assert_eq!(model.stats.placed_triangles, u64::from(parts));

    // まとめ役を動かして書き出す（子孫5万個がついて動く）。
    let mut scene = scene_of(&model);
    scene.nodes[group].local = translated(scene.nodes[group].local, Vec3::new(0.0, 0.0, 5.0));
    let (built, written) = export(&base, &scene);
    assert!(built.notes.is_empty());
    let reloaded = load_glb(&written).unwrap();
    let last = model.nodes[group].subtree_end as usize - 1;
    let moved =
        translation_of(reloaded.nodes[last].world) - translation_of(model.nodes[last].world);
    assert!((moved - Vec3::new(0.0, 0.0, 5.0)).length() < 1e-4);
}

// ---- 外部で作ったGLB（MR_EXTERNAL_GLB_DIR） ----

/// 環境変数 `MR_EXTERNAL_GLB_DIR` の `.glb` を、何も変えずに書き出す・編集して書き出す。
/// 読み込みで断られるファイル（Draco など）は、その理由を出して飛ばす。
#[test]
#[ignore]
fn external_glb_round_trip() {
    let Ok(dir) = std::env::var("MR_EXTERNAL_GLB_DIR") else {
        return;
    };
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("glb"))
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let base = std::fs::read(&path).expect("read");
        let model = match load_glb(&base) {
            Ok(model) => model,
            Err(error) => {
                println!("skip {name}: {}", error.message_ja());
                continue;
            }
        };
        let started = std::time::Instant::now();
        let scene = scene_of(&model);
        let (built, written) = export(&base, &scene);
        assert!(built.notes.is_empty(), "{name}: {:?}", built.notes);
        assert_eq!(bin_of(&written), bin_of(&base), "{name}");
        assert_eq!(json_of(&written), reparse(&json_of(&base)), "{name}");
        assert_eq!(load_glb(&written).unwrap(), model, "{name}");
        let unchanged = started.elapsed();

        // 最初の部品を動かし、最後の部品を複製し、図形を足し、最初の根でない部品を外す。
        let started = std::time::Instant::now();
        let mut edited = scene.clone();
        edited.nodes[0].local = translated(edited.nodes[0].local, Vec3::new(0.25, 0.0, 0.0));
        let last = edited.nodes.len() - 1;
        let mut copy = edited.nodes[last].clone();
        copy.file_index = None;
        copy.template = scene.nodes[last].file_index;
        copy.local =
            Affine3A::from_mat3_translation(Mat3::from_rotation_y(0.3), Vec3::ONE) * copy.local;
        edited.nodes.push(copy);
        let prim = PrimId(edited.base_primitive_count);
        edited.added.push(tetra());
        edited.nodes.push(added_node(
            None,
            "足した四面体",
            Affine3A::IDENTITY,
            vec![Draw {
                prim,
                base_color: [0.2, 0.8, 0.2, 1.0],
                tex: None,
            }],
        ));
        if let Some(child) = edited.nodes.iter().position(|node| node.parent.is_some()) {
            remove_subtree(&mut edited, child);
        }
        let (built, written) = export(&base, &edited);
        println!(
            "ok {name}: bytes {} -> {}, nodes {} -> {}, unchanged {:?}, edited {:?}, notes {:?}",
            base.len(),
            written.len(),
            scene.nodes.len(),
            edited.nodes.len(),
            unchanged,
            started.elapsed(),
            built.notes
        );
    }
}

// ---- 値の小さな道具 ----

fn serde_vec(values: &[f64]) -> Value {
    Value::Array(values.iter().map(|value| Value::from(*value)).collect())
}

fn serde_uints(values: &[u32]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|value| Value::from(u64::from(*value)))
            .collect(),
    )
}

fn serde_json_scenes(roots: &[u32]) -> Value {
    let mut scene: Value = gltf::json::deserialize::from_str("{}").unwrap();
    scene["nodes"] = serde_uints(roots);
    Value::Array(vec![scene])
}

// ---- 土台の合成（STL から読んだ形のモデル） ----

/// STL を読んだときと同じ形の `Model`（部品1つ・形状1つ・法線なし・mm の倍率）を手で組む。
/// `gltf` は `stl` に依存しないので、ここで組み立てる。
fn stl_like_model(scale: f32) -> Model {
    let size = Vec3::new(20.0, 30.0, 40.0);
    let corner = |i: u32| {
        [
            if i & 1 != 0 { size.x } else { 0.0 },
            if i & 2 != 0 { size.y } else { 0.0 },
            if i & 4 != 0 { size.z } else { 0.0 },
        ]
    };
    let vertices: Vec<Vertex> = (0..8)
        .map(|i| Vertex {
            position: corner(i),
            normal: [0.0; 3],
        })
        .collect();
    let quads: [[u32; 4]; 6] = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    let mut indices = Vec::new();
    for [a, b, c, d] in quads {
        indices.extend_from_slice(&[a, b, c, a, c, d]);
    }
    let local_aabb = Aabb {
        min: Vec3::ZERO,
        max: size,
    };
    let local = Affine3A::from_scale(Vec3::splat(scale));
    let world_aabb = local_aabb.transformed(&local);
    Model {
        textures: Textures::default(),
        nodes: vec![Node {
            name: "箱".to_owned(),
            gltf_index: 0,
            parent: None,
            subtree_end: 1,
            depth: 0,
            local,
            world: local,
            draws: 0..1,
            world_aabb,
        }],
        draws: vec![Draw {
            prim: PrimId(0),
            base_color: [0.72, 0.72, 0.72, 1.0],
            tex: None,
        }],
        primitives: vec![Primitive {
            vertices,
            indices,
            local_aabb,
            has_normals: false,
        }],
        bounds: world_aabb,
        stats: ModelStats {
            nodes: 1,
            meshes: 1,
            primitives: 1,
            unique_triangles: 12,
            placed_triangles: 12,
        },
        warnings: Vec::new(),
    }
}

fn relative_close(a: f32, b: f32) -> bool {
    let scale = a.abs().max(b.abs());
    if scale < 1.0e-3 {
        (a - b).abs() <= 1.0e-9
    } else {
        (a - b).abs() <= 1.0e-6 * scale
    }
}

fn assert_local_close(actual: Affine3A, expected: Affine3A) {
    let a = actual.to_cols_array();
    let e = expected.to_cols_array();
    for (x, y) in a.iter().zip(e.iter()) {
        assert!(relative_close(*x, *y), "{actual:?} != {expected:?}");
    }
}

#[test]
fn synth_base_round_trips_through_load_glb() {
    for scale in [0.001f32, 0.01, 1.0] {
        let model = stl_like_model(scale);
        let base = synthesize_base(&model).expect("synthesize");
        let loaded = load_glb(&base).expect("load");
        assert_eq!(loaded.nodes.len(), 1);
        assert_eq!(loaded.nodes[0].name, "箱");
        assert_eq!(loaded.nodes[0].gltf_index, 0);
        assert_eq!(loaded.nodes[0].parent, None);
        assert_local_close(loaded.nodes[0].local, model.nodes[0].local);
        assert_eq!(loaded.draws.len(), 1);
        assert_eq!(loaded.draws[0].base_color, model.draws[0].base_color);
        assert_eq!(loaded.primitives.len(), 1);
        let (got, want) = (&loaded.primitives[0], &model.primitives[0]);
        let bits = |prim: &Primitive| {
            prim.vertices
                .iter()
                .map(|vertex| vertex.position.map(f32::to_bits))
                .collect::<Vec<_>>()
        };
        assert_eq!(bits(got), bits(want));
        assert_eq!(got.indices, want.indices);
        assert!(!got.has_normals);
        assert_eq!(loaded.stats.unique_triangles, 12);
        // 法線を書かないので、開き直すと「法線の無い形状 1 件」が出る（事実なので、そのまま）。
        assert!(
            loaded
                .warnings
                .contains(&LoadWarning::DerivedNormals { count: 1 })
        );
        let json = json_of(&base);
        assert_eq!(json["asset"]["generator"], "model-reviewer");
        assert_eq!(json["asset"]["version"], "2.0");
        let mesh = &json["meshes"][0]["primitives"][0];
        assert!(mesh["attributes"].get("NORMAL").is_none());
        assert_eq!(json["accessors"][0]["min"], serde_vec(&[0.0, 0.0, 0.0]));
        assert_eq!(json["accessors"][0]["max"], serde_vec(&[20.0, 30.0, 40.0]));
        if scale == 1.0 {
            assert!(node(&json, 0).get("scale").is_none());
        } else {
            assert!(node(&json, 0).get("scale").is_some());
        }
        assert!(node(&json, 0).get("translation").is_none());
        assert!(node(&json, 0).get("rotation").is_none());
    }
}

#[test]
fn synthesized_base_keeps_rotation_and_scale() {
    use model_reviewer_core::units::UpAxis;

    for scale in [0.001, 0.01, 1.0] {
        let mut model = stl_like_model(scale);
        let local = Affine3A::from_mat3(UpAxis::Z.rotation() * scale);
        model.nodes[0].local = local;
        model.nodes[0].world = local;
        model.bounds = model.primitives[0].local_aabb.transformed(&local);
        model.nodes[0].world_aabb = model.bounds;
        let base = synthesize_base(&model).unwrap();
        let json = json_of(&base);
        let part = node(&json, 0);
        assert_eq!(part["rotation"].as_array().unwrap().len(), 4);
        assert!(part.get("matrix").is_none());
        assert_eq!(part.get("scale").is_some(), scale != 1.0);
        let loaded = load_glb(&base).unwrap();
        // 四元数から戻した行列の 0 付近の誤差も、元の倍率に対して相対 1e-6 以内。
        for (actual, expected) in loaded.nodes[0]
            .local
            .to_cols_array()
            .iter()
            .zip(local.to_cols_array())
        {
            assert!(
                (actual - expected).abs() <= scale * 1.0e-6,
                "{scale}: {actual} != {expected}"
            );
        }
    }
}

#[test]
fn synth_base_is_deterministic() {
    let model = stl_like_model(0.001);
    let first = synthesize_base(&model).unwrap();
    assert_eq!(first, synthesize_base(&model).unwrap());
    assert_eq!(first, synthesize_base(&model.clone()).unwrap());
    assert_eq!(first.len() % 4, 0);
    assert_eq!(
        u32::from_le_bytes(first[8..12].try_into().unwrap()) as usize,
        first.len()
    );
}

#[test]
fn export_on_synth_base_verifies() {
    let model = stl_like_model(0.001);
    let base = synthesize_base(&model).unwrap();
    let loaded = load_glb(&base).unwrap();
    // 何も変えない書き出し。
    let (_, written) = export(&base, &scene_of(&loaded));
    assert_eq!(bin_of(&written), bin_of(&base));
    // 動かして、図形を足して、書き出す。
    let mut scene = scene_of(&loaded);
    scene.nodes[0].local = translated(scene.nodes[0].local, Vec3::new(0.01, 0.0, -0.02));
    let prim = PrimId(scene.base_primitive_count);
    scene.added.push(tetra());
    scene.nodes.push(added_node(
        None,
        "四面体",
        Affine3A::from_translation(Vec3::new(0.05, 0.0, 0.0)),
        vec![Draw {
            prim,
            base_color: [0.2, 0.6, 0.9, 1.0],
            tex: None,
        }],
    ));
    let (built, written) = export(&base, &scene);
    assert_eq!(built.assigned, vec![0, 1]);
    let reloaded = load_glb(&written).unwrap();
    assert_eq!(reloaded.nodes.len(), 2);
    assert_eq!(reloaded.nodes[0].name, "箱");
    assert_eq!(reloaded.node(NodeId(1)).name, "四面体");
    assert_eq!(reloaded.stats.placed_triangles, 12 + 4);
    assert_eq!(reloaded.primitives[0].indices, model.primitives[0].indices);
    let moved = translation_of(reloaded.nodes[0].local);
    assert!(relative_close(moved.x, 0.01) && relative_close(moved.z, -0.02));
    let scale = reloaded.nodes[0].local.matrix3.x_axis.x;
    assert!(relative_close(scale, 0.001), "{scale}");
}

#[test]
fn synth_base_refuses_models_of_another_shape() {
    let model = stl_like_model(0.001);
    let mut two_nodes = model.clone();
    let mut second = two_nodes.nodes[0].clone();
    second.gltf_index = 1;
    two_nodes.nodes.push(second);
    let mut two_draws = model.clone();
    two_draws.draws.push(two_draws.draws[0]);
    let mut no_triangles = model.clone();
    no_triangles.primitives[0].indices.clear();
    let mut out_of_range = model.clone();
    out_of_range.primitives[0].indices[0] = 99;
    let mut not_finite = model.clone();
    not_finite.primitives[0].vertices[3].position[1] = f32::NAN;
    let mut wrong_prim = model.clone();
    wrong_prim.draws[0].prim = PrimId(1);
    let mut child = model;
    child.nodes[0].parent = Some(NodeId(0));
    for (what, broken) in [
        ("two nodes", two_nodes),
        ("two draws", two_draws),
        ("no triangles", no_triangles),
        ("index out of range", out_of_range),
        ("not finite", not_finite),
        ("wrong prim", wrong_prim),
        ("child", child),
    ] {
        assert!(
            matches!(synthesize_base(&broken), Err(ExportError::InvalidScene(_))),
            "{what}"
        );
    }
}
