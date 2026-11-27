//! 色の変わった部品の書き出し（材質とメッシュの写し）と、その照合。
//! 断る試験を先に置く（照合をゆるめた所が、写しでないものまで通していないことを先に固定する）。

use gltf::json::Value;
use model_reviewer_core::scene::Model;
use model_reviewer_gltf::export::{
    Built, ExportError, ExportNode, ExportNote, ExportScene, build, verify, write_container,
};
use model_reviewer_gltf::load_glb;
use model_reviewer_testdata::rich::{ARM, BODY, HAND, ORPHAN, ROOT, SKEWED, SKINNED};
use model_reviewer_testdata::rich_glb;

// ---- 試験の補助 ----

const RED: [f32; 4] = [0.9, 0.05, 0.05, 1.0];
const BLUE: [f32; 4] = [0.05, 0.2, 0.9, 1.0];

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
    }
}

fn position(scene: &ExportScene, file: u32) -> usize {
    scene
        .nodes
        .iter()
        .position(|node| node.file_index == Some(file))
        .unwrap_or_else(|| panic!("node {file} not in scene"))
}

/// 部品の描画の色を、透明度を残して `rgb` にする（文書の `SetColors` に当たる）。
fn paint(scene: &mut ExportScene, at: usize, color: [f32; 4]) {
    for draw in &mut scene.nodes[at].draws {
        let alpha = draw.base_color[3];
        draw.base_color = [color[0], color[1], color[2], alpha];
    }
}

/// `file` のノードを写す複製を、根の最後の子として足す（行きがけ順の末尾）。
fn duplicate(scene: &mut ExportScene, file: u32) -> usize {
    let source = scene.nodes[position(scene, file)].clone();
    let root = position(scene, ROOT) as u32;
    scene.nodes.push(ExportNode {
        parent: Some(root),
        name: format!("{} の複製", source.name),
        file_index: None,
        template: Some(file),
        ..source
    });
    scene.nodes.len() - 1
}

fn write(base: &[u8], built: &Built) -> Vec<u8> {
    let mut written = Vec::new();
    let length = write_container(&mut written, base, built).expect("write");
    assert_eq!(length, written.len() as u64);
    written
}

fn export(base: &[u8], scene: &ExportScene) -> (Built, Vec<u8>) {
    let built = build(base, scene).unwrap_or_else(|error| panic!("build: {error:?}"));
    let written = write(base, &built);
    verify(&written, base, scene, &built).unwrap_or_else(|error| panic!("verify: {error:?}"));
    (built, written)
}

fn chunk(bytes: &[u8], at: usize) -> (&[u8], usize) {
    let len = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    (&bytes[at + 8..at + 8 + len], at + 8 + len)
}

fn json_of(glb: &[u8]) -> Value {
    gltf::json::deserialize::from_slice(chunk(glb, 12).0).expect("json")
}

fn bin_of(glb: &[u8]) -> Vec<u8> {
    let (_, next) = chunk(glb, 12);
    if next == glb.len() {
        return Vec::new();
    }
    chunk(glb, next).0.to_vec()
}

fn reparse(value: &Value) -> Value {
    let bytes = gltf::json::serialize::to_vec(value).unwrap();
    gltf::json::deserialize::from_slice(&bytes).unwrap()
}

fn items<'v>(json: &'v Value, key: &str) -> &'v [Value] {
    json.get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn mesh_of(json: &Value, node: usize) -> u64 {
    json["nodes"][node]["mesh"].as_u64().expect("mesh")
}

fn without(value: &Value, path: &[&str]) -> Value {
    let mut copy = value.clone();
    let mut at = &mut copy;
    for key in &path[..path.len() - 1] {
        at = &mut at[*key];
    }
    if let Value::Object(map) = at {
        map.remove(path[path.len() - 1]);
    }
    copy
}

fn rich() -> (Vec<u8>, Model, ExportScene) {
    let base = rich_glb();
    let model = load_glb(&base).expect("rich loads");
    let scene = scene_of(&model);
    (base, model, scene)
}

/// 腕だけを赤にした書き出し（照合の壊し方の土台）。
fn arm_red() -> (Vec<u8>, ExportScene, Built) {
    let (base, _, mut scene) = rich();
    let arm = position(&scene, ARM);
    paint(&mut scene, arm, RED);
    let (built, _) = export(&base, &scene);
    (base, scene, built)
}

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

fn last(json: &mut Value, key: &str) -> usize {
    json[key].as_array().unwrap().len() - 1
}

// ---- 照合が写しでないものを断る ----

#[test]
fn verify_rejects_non_copy_material() {
    let (base, scene, built) = arm_red();
    let cases: Vec<(&str, Edit)> = vec![
        (
            "metallicFactor added to the copy",
            Box::new(|json| {
                let at = last(json, "materials");
                json["materials"][at]["pbrMetallicRoughness"]["metallicFactor"] = Value::from(0.5);
            }),
        ),
        (
            "another field of the copy changed",
            Box::new(|json| {
                let at = last(json, "materials");
                json["materials"][at]["doubleSided"] = Value::from(false);
            }),
        ),
        (
            "a field added to the copy",
            Box::new(|json| {
                let at = last(json, "materials");
                json["materials"][at]["name"] = Value::from("別の材質");
            }),
        ),
        (
            "the copy is a copy of another material",
            Box::new(|json| {
                let mut other = json["materials"][0].clone();
                let at = last(json, "materials");
                other["pbrMetallicRoughness"]["baseColorFactor"] =
                    json["materials"][at]["pbrMetallicRoughness"]["baseColorFactor"].clone();
                json["materials"][at] = other;
            }),
        ),
        (
            "an unused material is appended",
            Box::new(|json| {
                let at = last(json, "materials");
                let extra = json["materials"][at].clone();
                json["materials"].as_array_mut().unwrap().push(extra);
            }),
        ),
        (
            "the copied mesh points to an original material",
            Box::new(|json| {
                let at = last(json, "meshes");
                json["meshes"][at]["primitives"][0]["material"] = Value::from(0u64);
            }),
        ),
    ];
    for (what, edit) in cases {
        assert_rejected(what, broken_json(&base, &scene, &built, edit));
    }
}

#[test]
fn verify_rejects_mesh_copy_with_other_changes() {
    let (base, scene, built) = arm_red();
    let cases: Vec<(&str, Edit)> = vec![
        (
            "attributes changed",
            Box::new(|json| {
                let at = last(json, "meshes");
                json["meshes"][at]["primitives"][0]["attributes"]["POSITION"] = Value::from(5u64);
            }),
        ),
        (
            "indices added",
            Box::new(|json| {
                let at = last(json, "meshes");
                json["meshes"][at]["primitives"][0]["indices"] = Value::from(9u64);
            }),
        ),
        (
            "mesh name added",
            Box::new(|json| {
                let at = last(json, "meshes");
                json["meshes"][at]["name"] = Value::from("写し");
            }),
        ),
        (
            "a primitive added",
            Box::new(|json| {
                let at = last(json, "meshes");
                let extra = json["meshes"][at]["primitives"][0].clone();
                json["meshes"][at]["primitives"]
                    .as_array_mut()
                    .unwrap()
                    .push(extra);
            }),
        ),
        (
            "the recolored node points to another original mesh",
            Box::new(|json| json["nodes"][ARM as usize]["mesh"] = Value::from(0u64)),
        ),
        (
            "a node whose color did not change points to the copy",
            Box::new(|json| {
                let copy = json["nodes"][ARM as usize]["mesh"].clone();
                json["nodes"][HAND as usize]["mesh"] = copy;
            }),
        ),
        (
            "an unused mesh is appended",
            Box::new(|json| {
                let at = last(json, "meshes");
                let extra = json["meshes"][at].clone();
                json["meshes"].as_array_mut().unwrap().push(extra);
            }),
        ),
    ];
    for (what, edit) in cases {
        assert_rejected(what, broken_json(&base, &scene, &built, edit));
    }
}

#[test]
fn verify_rejects_one_bit_color_difference() {
    let (base, scene, built) = arm_red();
    // 書いた色を、f32 で1ビットだけ違う値にする。
    let nudged = f32::from_bits(RED[0].to_bits() + 1);
    assert_rejected(
        "written color off by one bit",
        broken_json(&base, &scene, &built, |json| {
            let at = last(json, "materials");
            json["materials"][at]["pbrMetallicRoughness"]["baseColorFactor"][0] =
                Value::from(f64::from(nudged));
        }),
    );
    // 文書の側が1ビット違う（書いた物は正しい）。
    let written = write(&base, &built);
    let mut other = scene.clone();
    let arm = position(&other, ARM);
    other.nodes[arm].draws[0].base_color[0] = nudged;
    assert_rejected(
        "document color off by one bit",
        verify(&written, &base, &other, &built),
    );
}

// ---- 写し方 ----

#[test]
fn recolor_adds_material_and_mesh_only() {
    let (base, model, mut scene) = rich();
    let arm = position(&scene, ARM);
    paint(&mut scene, arm, RED);
    let (built, written) = export(&base, &scene);
    let before = reparse(&json_of(&base));
    let after = json_of(&written);

    assert_eq!(bin_of(&written), bin_of(&base), "BIN is unchanged");
    for key in ["accessors", "bufferViews", "buffers", "textures", "images"] {
        assert_eq!(after.get(key), before.get(key), "{key}");
    }
    for key in ["materials", "meshes"] {
        let (old, new) = (items(&before, key), items(&after, key));
        assert_eq!(new.len(), old.len() + 1, "{key}");
        assert_eq!(&new[..old.len()], old, "{key} keep the originals in place");
    }
    let copy = mesh_of(&after, ARM as usize);
    assert_eq!(copy, items(&before, "meshes").len() as u64);
    assert_eq!(
        mesh_of(&after, HAND as usize),
        1,
        "the other user of mesh 1"
    );
    assert_eq!(mesh_of(&after, ORPHAN as usize), 1);
    // ノードは mesh だけが替わる。
    assert_eq!(
        without(&after["nodes"][ARM as usize], &["mesh"]),
        without(&before["nodes"][ARM as usize], &["mesh"])
    );
    for (index, node) in items(&before, "nodes").iter().enumerate() {
        if index != ARM as usize {
            assert_eq!(&after["nodes"][index], node, "node {index}");
        }
    }
    // 写した材質は、基本色だけが違う。
    let material = items(&after, "materials").last().unwrap();
    assert_eq!(
        without(material, &["pbrMetallicRoughness", "baseColorFactor"]),
        without(
            &before["materials"][1],
            &["pbrMetallicRoughness", "baseColorFactor"]
        )
    );
    assert_eq!(
        built.notes,
        vec![ExportNote::Recolored {
            nodes: 1,
            materials: 1,
            meshes: 1
        }]
    );
    let reloaded = load_glb(&written).unwrap();
    let color =
        |model: &Model, at: usize| model.draws[model.nodes[at].draws.start as usize].base_color;
    assert_eq!(
        color(&reloaded, arm).map(f32::to_bits),
        RED.map(f32::to_bits)
    );
    let hand = position(&scene, HAND);
    assert_eq!(
        color(&reloaded, hand).map(f32::to_bits),
        color(&model, hand).map(f32::to_bits)
    );
    assert_eq!(reloaded.primitives, model.primitives, "geometry is shared");
}

#[test]
fn recolor_shares_material_and_mesh() {
    let (base, _, mut scene) = rich();
    for file in [ARM, HAND] {
        let at = position(&scene, file);
        paint(&mut scene, at, BLUE);
    }
    // テクスチャ付きの mesh 0 を使う2部品も、同じ色にする。
    for file in [BODY, SKEWED] {
        let at = position(&scene, file);
        paint(&mut scene, at, RED);
    }
    let (built, written) = export(&base, &scene);
    let before = json_of(&base);
    let after = json_of(&written);
    assert_eq!(
        items(&after, "materials").len(),
        items(&before, "materials").len() + 2
    );
    assert_eq!(
        items(&after, "meshes").len(),
        items(&before, "meshes").len() + 2
    );
    assert_eq!(
        mesh_of(&after, ARM as usize),
        mesh_of(&after, HAND as usize)
    );
    assert_eq!(
        mesh_of(&after, BODY as usize),
        mesh_of(&after, SKEWED as usize)
    );
    assert_ne!(
        mesh_of(&after, ARM as usize),
        mesh_of(&after, BODY as usize)
    );
    assert_eq!(
        built.notes,
        vec![ExportNote::Recolored {
            nodes: 4,
            materials: 2,
            meshes: 2
        }]
    );
}

#[test]
fn recolor_keeps_texture_refs_and_extensions() {
    let (base, _, mut scene) = rich();
    let body = position(&scene, BODY);
    paint(&mut scene, body, RED);
    let (_, written) = export(&base, &scene);
    let before = reparse(&json_of(&base));
    let after = json_of(&written);
    let copy = &after["meshes"][mesh_of(&after, BODY as usize) as usize];
    assert_eq!(
        without(copy, &["primitives"]),
        without(&before["meshes"][0], &["primitives"]),
        "name and extras are copied"
    );
    let primitive = &copy["primitives"][0];
    assert_eq!(
        without(primitive, &["material"]),
        without(&before["meshes"][0]["primitives"][0], &["material"]),
        "attributes (with TEXCOORD_0) and indices are copied"
    );
    let material = &after["materials"][primitive["material"].as_u64().unwrap() as usize];
    let original = &before["materials"][0];
    for key in ["extensions", "extras"] {
        assert_eq!(material[key], original[key], "{key}");
    }
    let pbr = &material["pbrMetallicRoughness"];
    assert_eq!(
        pbr["baseColorTexture"],
        original["pbrMetallicRoughness"]["baseColorTexture"]
    );
    assert_eq!(
        pbr["metallicFactor"],
        original["pbrMetallicRoughness"]["metallicFactor"]
    );
    assert_eq!(after["textures"], before["textures"]);
}

#[test]
fn recolor_without_material() {
    let (base, model, mut scene) = rich();
    let skinned = position(&scene, SKINNED);
    // 材質の無いプリミティブは既定の白。透明度は元のまま。
    let start = model.nodes[skinned].draws.start as usize;
    assert_eq!(model.draws[start].base_color, [1.0; 4]);
    paint(&mut scene, skinned, BLUE);
    let (_, written) = export(&base, &scene);
    let before = reparse(&json_of(&base));
    let after = json_of(&written);
    let copy = &after["meshes"][mesh_of(&after, SKINNED as usize) as usize];
    let primitive = &copy["primitives"][0];
    assert_eq!(
        without(primitive, &["material"]),
        before["meshes"][2]["primitives"][0],
        "JOINTS and WEIGHTS stay"
    );
    let material = &after["materials"][primitive["material"].as_u64().unwrap() as usize];
    let expected: Value = gltf::json::deserialize::from_slice(
        format!(
            r#"{{"pbrMetallicRoughness":{{"baseColorFactor":[{:?},{:?},{:?},{:?}]}}}}"#,
            f64::from(BLUE[0]),
            f64::from(BLUE[1]),
            f64::from(BLUE[2]),
            f64::from(BLUE[3])
        )
        .as_bytes(),
    )
    .unwrap();
    assert_eq!(material, &expected);
}

/// 1つのメッシュに、三角形・点・線・三角形の順でプリミティブがある。描画は2つ（0番と3番）。
fn mixed_glb() -> Vec<u8> {
    let mut bin = Vec::new();
    for value in [
        0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, // 三角形A
        0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0, 1.0, // 三角形B
    ] {
        bin.extend_from_slice(&value.to_le_bytes());
    }
    let json = r#"{"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],
"nodes":[{"name":"mixed","mesh":0}],
"meshes":[{"primitives":[
{"attributes":{"POSITION":0},"material":0},
{"attributes":{"POSITION":0},"material":1,"mode":0},
{"attributes":{"POSITION":1},"material":1,"mode":1},
{"attributes":{"POSITION":1},"material":1}]}],
"materials":[{"name":"a","pbrMetallicRoughness":{"baseColorFactor":[0.5,0.5,0.5,1.0]}},
{"name":"b","pbrMetallicRoughness":{"baseColorFactor":[0.25,0.75,0.25,0.5]}}],
"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0.0,0.0,0.0],"max":[1.0,1.0,0.0]},
{"bufferView":1,"componentType":5126,"count":3,"type":"VEC3","min":[0.0,0.0,1.0],"max":[1.0,1.0,1.0]}],
"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":36}],
"buffers":[{"byteLength":72}]}"#;
    glb(json.as_bytes(), &bin)
}

/// JSON の文字列とBINからGLBを作る。JSONは空白、BINは0で4バイト境界まで詰める。
fn glb(json: &[u8], bin: &[u8]) -> Vec<u8> {
    let mut json = json.to_vec();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut bin = bin.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 12 + 8 + json.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

#[test]
fn recolor_maps_draws_past_skipped_primitives() {
    let base = mixed_glb();
    let model = load_glb(&base).expect("mixed loads");
    assert_eq!(model.draws.len(), 2, "points and lines are not drawn");
    let mut scene = scene_of(&model);
    // 2つ目の描画（メッシュの3番目のプリミティブ）だけを変える。透明度 0.5 は残る。
    scene.nodes[0].draws[1].base_color = [RED[0], RED[1], RED[2], 0.5];
    let (_, written) = export(&base, &scene);
    let before = reparse(&json_of(&base));
    let after = json_of(&written);
    let copy = &after["meshes"][mesh_of(&after, 0) as usize];
    let original = &before["meshes"][0];
    for slot in 0..3 {
        assert_eq!(
            copy["primitives"][slot], original["primitives"][slot],
            "primitive {slot}"
        );
    }
    assert_eq!(copy["primitives"][3]["material"], Value::from(2u64));
    assert_eq!(
        without(&copy["primitives"][3], &["material"]),
        without(&original["primitives"][3], &["material"])
    );
    let reloaded = load_glb(&written).unwrap();
    assert_eq!(reloaded.draws[0].base_color, model.draws[0].base_color);
    assert_eq!(
        reloaded.draws[1].base_color.map(f32::to_bits),
        [RED[0], RED[1], RED[2], 0.5].map(f32::to_bits)
    );
}

#[test]
fn recolor_of_duplicate_only() {
    let (base, model, mut scene) = rich();
    let copy = duplicate(&mut scene, ARM);
    paint(&mut scene, copy, RED);
    let (built, written) = export(&base, &scene);
    let before = reparse(&json_of(&base));
    let after = json_of(&written);
    assert_eq!(after["nodes"][ARM as usize], before["nodes"][ARM as usize]);
    let added = built.assigned[copy] as usize;
    assert_eq!(
        mesh_of(&after, added),
        items(&before, "meshes").len() as u64
    );
    assert_eq!(
        items(&after, "materials").len(),
        items(&before, "materials").len() + 1
    );
    let reloaded = load_glb(&written).unwrap();
    let arm = position(&scene, ARM);
    assert_eq!(
        reloaded.draws[reloaded.nodes[arm].draws.start as usize].base_color,
        model.draws[model.nodes[arm].draws.start as usize].base_color
    );
    assert_eq!(
        reloaded.draws[reloaded.nodes[copy].draws.start as usize]
            .base_color
            .map(f32::to_bits),
        RED.map(f32::to_bits)
    );
}

#[test]
fn restoring_color_restores_json() {
    let (base, _, scene) = rich();
    let (unchanged, _) = export(&base, &scene);
    let mut painted = scene.clone();
    let arm = position(&painted, ARM);
    paint(&mut painted, arm, RED);
    let (changed, _) = export(&base, &painted);
    assert_ne!(changed.json, unchanged.json);
    // 取り消しで元の色（ビットまで同じ）に戻す。
    painted.nodes[arm].draws = scene.nodes[arm].draws.clone();
    let (restored, _) = export(&base, &painted);
    assert_eq!(restored.json, unchanged.json);
    assert!(restored.notes.is_empty());
}

#[test]
fn second_export_adds_nothing() {
    let (base, _, mut scene) = rich();
    let arm = position(&scene, ARM);
    paint(&mut scene, arm, RED);
    let copy = duplicate(&mut scene, SKINNED);
    paint(&mut scene, copy, BLUE);
    let (_, first) = export(&base, &scene);
    // 書き出した後は、書いたファイルが土台になり、文書の色はそれを読み直した値と同じ。
    let reloaded = load_glb(&first).unwrap();
    let again = scene_of(&reloaded);
    let (built, second) = export(&first, &again);
    assert!(built.notes.is_empty(), "{:?}", built.notes);
    assert_eq!(json_of(&second), json_of(&first));
    assert_eq!(bin_of(&second), bin_of(&first));
}

#[test]
fn every_srgb8_color_is_written_bit_exact() {
    // 画面で選べる色（sRGB の8ビット）は、どれも書いて読み直すとビット単位で同じになる。
    // 1つでも戻らなければ、照合が「色が違います」で落ちる。
    let (base, _, scene) = rich();
    let arm = position(&scene, ARM);
    for value in 0..=255u8 {
        let rgb = model_reviewer_core::color::srgb8_to_linear([value, 255 - value, value / 2]);
        let mut painted = scene.clone();
        paint(&mut painted, arm, [rgb[0], rgb[1], rgb[2], 1.0]);
        let (_, written) = export(&base, &painted);
        let reloaded = load_glb(&written).unwrap();
        let got = reloaded.draws[reloaded.nodes[arm].draws.start as usize].base_color;
        assert_eq!(
            got[..3].iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            rgb.map(f32::to_bits),
            "{value}"
        );
    }
}
