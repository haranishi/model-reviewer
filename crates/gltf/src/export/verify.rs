//! 書いたバイト列を読み直して照合する。`build` の判断をなぞらず、`scene` と元のファイルから
//! 「書き換えてよい所」を別に導いて確かめる（`build` の誤りを `build` と同じ式で見逃さないため）。

use std::collections::HashSet;

use glam::Affine3A;
use gltf::json::Value;
use model_reviewer_core::scene::{Model, Primitive};

use super::base::{self, Base, same_bits};
use super::container::{self, align4};
use super::json::read_children;
use super::patch::{self, Shape};
use super::{Built, ExportError, ExportNote, ExportScene};

const OWNED: [&str; 8] = [
    "nodes",
    "scenes",
    "scene",
    "meshes",
    "materials",
    "accessors",
    "bufferViews",
    "buffers",
];
const NODE_EDITABLE: [&str; 6] = [
    "matrix",
    "translation",
    "rotation",
    "scale",
    "children",
    "name",
];
const TRANSFORM: [&str; 4] = ["matrix", "translation", "rotation", "scale"];
const NEW_NODE_KEYS: [&str; 7] = [
    "name",
    "mesh",
    "matrix",
    "translation",
    "rotation",
    "scale",
    "children",
];

pub(crate) fn verify(
    written: &[u8],
    base_bytes: &[u8],
    scene: &ExportScene,
    built: &Built,
) -> Result<Model, ExportError> {
    let base = base::read(base_bytes)?;
    let shape = patch::shape(&base, scene)?;
    check_container(written, &base, built)?;
    let new_value: Value = gltf::json::deserialize::from_slice(&built.json)
        .map_err(|error| fail(format!("JSONを読み直せません: {error}")))?;
    // 元の値は、一度書いて読み直した形で比べる。数の読み方の丸めが両方に同じく効くので、
    // 「書いた文字列が元の値を表しているか」だけを比べられる。
    let old_value: Value = gltf::json::serialize::to_vec(&base.value)
        .ok()
        .and_then(|bytes| gltf::json::deserialize::from_slice(&bytes).ok())
        .ok_or_else(|| fail("元のJSONを読み直せません".to_owned()))?;
    check_assigned(&base, scene, &shape, built)?;
    check_top_level(&old_value, &new_value, built)?;
    check_nodes(&old_value, &new_value, &base, scene, &shape, built)?;
    check_scenes(&old_value, &new_value, &base, &shape, built)?;
    check_appended(&old_value, &new_value, &base, built)?;
    let model = crate::load_glb(written)
        .map_err(|error| fail(format!("開けません: {}", error.message_ja())))?;
    check_model(&model, &base.model, scene, built)?;
    Ok(model)
}

fn fail(what: String) -> ExportError {
    ExportError::Verify(what)
}

/// 1. 容器。JSONチャンクは `built.json`（空白で詰める）、BINは元の範囲・0の詰め物・足した分の順。
fn check_container(written: &[u8], base: &Base<'_>, built: &Built) -> Result<(), ExportError> {
    let parsed = container::parse(written).map_err(fail)?;
    let json_end = built.json.len();
    if parsed.json.get(..json_end) != Some(built.json.as_slice())
        || parsed.json[json_end..].iter().any(|byte| *byte != b' ')
        || parsed.json.len() as u64 != align4(json_end as u64)
    {
        return Err(fail("JSONチャンクが組み立てた結果と違います".to_owned()));
    }
    let expected = container::bin_chunk_len(base.container.bin, &built.appended_bin);
    match (parsed.bin, expected) {
        (None, None) => Ok(()),
        (Some(bin), Some(len)) if bin.len() as u64 == len => {
            let original = base.container.bin.unwrap_or(&[]);
            if bin.get(..original.len()) != Some(original) {
                return Err(fail("元のBINの範囲が元と違います".to_owned()));
            }
            let start = align4(original.len() as u64) as usize;
            if bin[original.len()..start].iter().any(|byte| *byte != 0) {
                return Err(fail("BINの詰め物が0ではありません".to_owned()));
            }
            let tail = &bin[start..];
            if tail.get(..built.appended_bin.len()) != Some(built.appended_bin.as_slice())
                || tail[built.appended_bin.len()..]
                    .iter()
                    .any(|byte| *byte != 0)
            {
                return Err(fail("足したBINが組み立てた結果と違います".to_owned()));
            }
            Ok(())
        }
        _ => Err(fail("BINチャンクの有無か長さが違います".to_owned())),
    }
}

/// 既存の部品は元の番号、足した部品は元のノードの数から順に。
fn check_assigned(
    base: &Base<'_>,
    scene: &ExportScene,
    shape: &Shape,
    built: &Built,
) -> Result<(), ExportError> {
    if built.assigned.len() != scene.nodes.len() || built.assigned != shape.assigned {
        return Err(fail("部品のファイル内の番号が違います".to_owned()));
    }
    let mut next = base.node_count as u64;
    for (node, assigned) in scene.nodes.iter().zip(&built.assigned) {
        let expected = match node.file_index {
            Some(file) => u64::from(file),
            None => {
                next += 1;
                next - 1
            }
        };
        if u64::from(*assigned) != expected {
            return Err(fail("部品のファイル内の番号が違います".to_owned()));
        }
    }
    Ok(())
}

/// 2. 8つ以外の最上位の項目は値として等しい。
fn check_top_level(old: &Value, new: &Value, built: &Built) -> Result<(), ExportError> {
    let (Some(old_map), Some(new_map)) = (old.as_object(), new.as_object()) else {
        return Err(fail("JSONの最上位がオブジェクトではありません".to_owned()));
    };
    let keys: HashSet<&String> = old_map.keys().chain(new_map.keys()).collect();
    for key in keys {
        if OWNED.contains(&key.as_str()) {
            continue;
        }
        if old_map.get(key) != new_map.get(key) {
            return Err(fail(format!("最上位の項目 {key} が元と違います")));
        }
    }
    let scene_added = built.notes.contains(&ExportNote::SceneAdded);
    if !scene_added && old.get("scene") != new.get("scene") {
        return Err(fail("scene が元と違います".to_owned()));
    }
    Ok(())
}

fn nodes_of(value: &Value) -> &[Value] {
    value
        .get("nodes")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn without(node: &Value, keys: &[&str]) -> Value {
    let mut copy = node.clone();
    if let Value::Object(map) = &mut copy {
        for key in keys {
            map.remove(*key);
        }
    }
    copy
}

fn same_keys(old: &Value, new: &Value, keys: &[&str]) -> bool {
    keys.iter().all(|key| old.get(*key) == new.get(*key))
}

fn children_of(node: &Value, what: &str) -> Result<Vec<u64>, ExportError> {
    read_children(node).ok_or_else(|| fail(format!("{what} の children が番号の列ではありません")))
}

/// 2 のノード。元のノードは「変えてよい項目」以外が等しく、変えていない項目は元のまま。
/// 足したノードは末尾にあり、複製は元のノードの写し。
fn check_nodes(
    old: &Value,
    new: &Value,
    base: &Base<'_>,
    scene: &ExportScene,
    shape: &Shape,
    built: &Built,
) -> Result<(), ExportError> {
    let old_nodes = nodes_of(old);
    let new_nodes = nodes_of(new);
    let added = scene
        .nodes
        .iter()
        .filter(|node| node.file_index.is_none())
        .count();
    if new_nodes.len() != old_nodes.len() + added {
        return Err(fail("ノードの数が違います".to_owned()));
    }
    let expected_children = |index: usize| -> Vec<u64> {
        shape.children[index]
            .iter()
            .map(|child| u64::from(built.assigned[*child as usize]))
            .collect()
    };
    for (file, (old_node, new_node)) in old_nodes.iter().zip(new_nodes).enumerate() {
        let what = format!("ノード {file}");
        match shape.in_scene[file] {
            Some(index) => {
                let index = index as usize;
                let node = &scene.nodes[index];
                if without(old_node, &NODE_EDITABLE) != without(new_node, &NODE_EDITABLE) {
                    return Err(fail(format!("{what} の変えていない項目が元と違います")));
                }
                let untouched = base.locals[file].is_some_and(|local| same_bits(local, node.local));
                if untouched && !same_keys(old_node, new_node, &TRANSFORM) {
                    return Err(fail(format!(
                        "{what} の変形を変えていないのに書き換えています"
                    )));
                }
                if node.name == base.names[file] && !same_keys(old_node, new_node, &["name"]) {
                    return Err(fail(format!(
                        "{what} の名前を変えていないのに書き換えています"
                    )));
                }
                let expected = expected_children(index);
                if children_of(new_node, &what)? != expected {
                    return Err(fail(format!("{what} の子が文書と違います")));
                }
                if children_of(old_node, &what)? == expected
                    && !same_keys(old_node, new_node, &["children"])
                {
                    return Err(fail(format!(
                        "{what} の子を変えていないのに書き換えています"
                    )));
                }
            }
            None => {
                if without(old_node, &["children"]) != without(new_node, &["children"]) {
                    return Err(fail(format!(
                        "{what}（シーンから外した部品）が元と違います"
                    )));
                }
                let before = children_of(old_node, &what)?;
                let kept: Vec<u64> = before
                    .iter()
                    .copied()
                    .filter(|child| {
                        shape
                            .in_scene
                            .get(*child as usize)
                            .is_some_and(Option::is_none)
                    })
                    .collect();
                if children_of(new_node, &what)? != kept {
                    return Err(fail(format!("{what} の子の外し方が違います")));
                }
                if kept == before && !same_keys(old_node, new_node, &["children"]) {
                    return Err(fail(format!(
                        "{what} の子を変えていないのに書き換えています"
                    )));
                }
            }
        }
    }
    for (index, node) in scene.nodes.iter().enumerate() {
        if node.file_index.is_some() {
            continue;
        }
        let assigned = built.assigned[index] as usize;
        let what = format!("足したノード {assigned}");
        let new_node = new_nodes
            .get(assigned)
            .ok_or_else(|| fail(format!("{what} がありません")))?;
        if children_of(new_node, &what)? != expected_children(index) {
            return Err(fail(format!("{what} の子が文書と違います")));
        }
        match node.template {
            Some(template) => {
                let source = old_nodes
                    .get(template as usize)
                    .ok_or_else(|| fail(format!("{what} の複製の元がありません")))?;
                if without(source, &NODE_EDITABLE) != without(new_node, &NODE_EDITABLE) {
                    return Err(fail(format!("{what} が複製の元の写しになっていません")));
                }
            }
            None => {
                let Some(map) = new_node.as_object() else {
                    return Err(fail(format!("{what} がオブジェクトではありません")));
                };
                if map.keys().any(|key| !NEW_NODE_KEYS.contains(&key.as_str())) {
                    return Err(fail(format!("{what} に知らない項目があります")));
                }
                if map.contains_key("mesh") == node.draws.is_empty() {
                    return Err(fail(format!("{what} の形状の有無が文書と違います")));
                }
            }
        }
    }
    Ok(())
}

/// 2. シーン。読み込みが使うシーンの根だけが変わり、ほかのシーンは等しい。
fn check_scenes(
    old: &Value,
    new: &Value,
    base: &Base<'_>,
    shape: &Shape,
    built: &Built,
) -> Result<(), ExportError> {
    let desired: Vec<u64> = shape
        .roots
        .iter()
        .map(|index| u64::from(built.assigned[*index as usize]))
        .collect();
    let scene_added = built.notes.contains(&ExportNote::SceneAdded);
    if base.has_scenes {
        if scene_added {
            return Err(fail("シーンがあるのにシーンを足しています".to_owned()));
        }
        let empty = Vec::new();
        let old_scenes = old
            .get("scenes")
            .and_then(Value::as_array)
            .unwrap_or(&empty);
        let new_scenes = new
            .get("scenes")
            .and_then(Value::as_array)
            .unwrap_or(&empty);
        if old_scenes.len() != new_scenes.len() {
            return Err(fail("シーンの数が違います".to_owned()));
        }
        for (index, (old_scene, new_scene)) in old_scenes.iter().zip(new_scenes).enumerate() {
            if index != base.scene_index {
                if old_scene != new_scene {
                    return Err(fail(format!("使っていないシーン {index} が元と違います")));
                }
                continue;
            }
            if without(old_scene, &["nodes"]) != without(new_scene, &["nodes"]) {
                return Err(fail("シーンの根以外の項目が元と違います".to_owned()));
            }
            let roots: Option<Vec<u64>> = new_scene
                .get("nodes")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(Value::as_u64).collect());
            if roots.as_ref() != Some(&desired) {
                return Err(fail("シーンの根が文書と違います".to_owned()));
            }
            let unchanged: Vec<u64> = base.roots.iter().map(|root| u64::from(*root)).collect();
            if unchanged == desired && old_scene != new_scene {
                return Err(fail(
                    "根を変えていないのにシーンを書き換えています".to_owned(),
                ));
            }
        }
        return Ok(());
    }
    if scene_added {
        let expected = Value::Array(vec![patch::scene_of(
            &desired.iter().map(|root| *root as u32).collect::<Vec<_>>(),
        )]);
        if new.get("scenes") != Some(&expected) || new.get("scene") != Some(&Value::from(0u64)) {
            return Err(fail("足したシーンが文書の根と違います".to_owned()));
        }
        return Ok(());
    }
    if old.get("scenes") != new.get("scenes") {
        return Err(fail("シーンが元と違います".to_owned()));
    }
    let implicit: Vec<u64> = patch::implicit_roots(new)?
        .into_iter()
        .map(u64::from)
        .collect();
    if implicit != desired {
        return Err(fail("シーンの無いファイルの根が文書と違います".to_owned()));
    }
    Ok(())
}

/// 2. 形状の配列と `buffers`。元の要素は等しく、足した要素は末尾で、足したBINの範囲だけを指す。
fn check_appended(
    old: &Value,
    new: &Value,
    base: &Base<'_>,
    built: &Built,
) -> Result<(), ExportError> {
    let empty = Vec::new();
    for key in ["meshes", "materials", "accessors", "bufferViews"] {
        let old_items = old.get(key).and_then(Value::as_array).unwrap_or(&empty);
        let new_items = new.get(key).and_then(Value::as_array).unwrap_or(&empty);
        if new_items.len() < old_items.len() || new_items[..old_items.len()] != old_items[..] {
            return Err(fail(format!("{key} の元の要素が元と違います")));
        }
        if built.appended_bin.is_empty() && new_items.len() != old_items.len() {
            return Err(fail(format!(
                "{key} に足した要素があるのにBINを足していません"
            )));
        }
    }
    let old_materials = old
        .get("materials")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let new_materials = new
        .get("materials")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    for material in &new_materials[old_materials.len()..] {
        let metallic = material
            .get("pbrMetallicRoughness")
            .and_then(|pbr| pbr.get("metallicFactor"))
            .and_then(Value::as_f64);
        if metallic != Some(0.0) {
            return Err(fail(
                "足した材質の metallicFactor が 0 ではありません".to_owned(),
            ));
        }
    }
    let start = base.container.bin.map_or(0, |bin| align4(bin.len() as u64));
    let end = start + built.appended_bin.len() as u64;
    let old_views = old
        .get("bufferViews")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let new_views = new
        .get("bufferViews")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    for view in &new_views[old_views.len()..] {
        let buffer = view.get("buffer").and_then(Value::as_u64);
        let offset = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0);
        let length = view.get("byteLength").and_then(Value::as_u64).unwrap_or(0);
        if buffer != Some(0) || offset < start || offset.saturating_add(length) > end {
            return Err(fail(
                "足した bufferView が足したBINの範囲の外を指しています".to_owned(),
            ));
        }
    }
    let old_buffers = old
        .get("buffers")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let new_buffers = new
        .get("buffers")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    if built.appended_bin.is_empty() {
        if old.get("buffers") != new.get("buffers") {
            return Err(fail("buffers が元と違います".to_owned()));
        }
        return Ok(());
    }
    let expected_len = Value::from(end);
    match (old_buffers.first(), new_buffers.first()) {
        (Some(old_buffer), Some(new_buffer)) => {
            if old_buffers.len() != new_buffers.len()
                || without(old_buffer, &["byteLength"]) != without(new_buffer, &["byteLength"])
                || new_buffer.get("byteLength") != Some(&expected_len)
                || old_buffers[1..] != new_buffers[1..]
            {
                return Err(fail("buffers の書き換えが違います".to_owned()));
            }
        }
        (None, Some(new_buffer)) => {
            let mut expected = super::json::obj();
            super::json::set(&mut expected, "byteLength", expected_len);
            if new_buffers.len() != 1 || *new_buffer != expected {
                return Err(fail("足した buffers が違います".to_owned()));
            }
        }
        _ => return Err(fail("buffers がありません".to_owned())),
    }
    Ok(())
}

/// 3. 本アプリの読み込みで開いた結果が `scene` と一致する。
fn check_model(
    model: &Model,
    base_model: &Model,
    scene: &ExportScene,
    built: &Built,
) -> Result<(), ExportError> {
    if model.nodes.len() != scene.nodes.len() {
        return Err(fail(format!(
            "部品の数が違います（読み直し {}、文書 {}）",
            model.nodes.len(),
            scene.nodes.len()
        )));
    }
    // 元のファイルのノード番号から、元のファイルを読み込んだ部品へ。
    let mut base_nodes = vec![None; base_node_count(base_model)];
    for (index, node) in base_model.nodes.iter().enumerate() {
        base_nodes[node.gltf_index as usize] = Some(index);
    }
    let mut worlds: Vec<Affine3A> = Vec::with_capacity(scene.nodes.len());
    let mut compared: HashSet<(u32, u64)> = HashSet::new();
    let mut placed = 0u64;
    for (index, (loaded, node)) in model.nodes.iter().zip(&scene.nodes).enumerate() {
        let what = format!("部品 {index}（{}）", node.name);
        if loaded.gltf_index != built.assigned[index] {
            return Err(fail(format!("{what} のファイル内の番号が違います")));
        }
        if loaded.parent.map(|parent| parent.0) != node.parent {
            return Err(fail(format!("{what} の親が違います")));
        }
        let name = if node.name.is_empty() {
            format!("ノード {}", built.assigned[index])
        } else {
            node.name.clone()
        };
        if loaded.name != name {
            return Err(fail(format!(
                "{what} の名前が違います（読み直し {}）",
                loaded.name
            )));
        }
        let world = match node.parent {
            Some(parent) => worlds[parent as usize] * node.local,
            None => node.local,
        };
        worlds.push(world);
        if !close(loaded.world, world) {
            return Err(fail(format!("{what} のワールド行列が違います")));
        }
        let draws = model
            .draws
            .get(loaded.draws.start as usize..loaded.draws.end as usize)
            .ok_or_else(|| fail(format!("{what} の描画の範囲が不正です")))?;
        if draws.len() != node.draws.len() {
            return Err(fail(format!("{what} の描画の数が違います")));
        }
        // 形状の出どころ。ファイルにある部品と複製は元のファイルの同じノード（読み込まれていれば）、
        // 足した部品は `scene.added`。
        let source = node
            .file_index
            .or(node.template)
            .and_then(|file| base_nodes.get(file as usize).copied().flatten())
            .map(|at| &base_model.nodes[at]);
        let source_draws = source.map(|source| {
            &base_model.draws[source.draws.start as usize..source.draws.end as usize]
        });
        if source_draws.is_some_and(|source| source.len() != draws.len()) {
            return Err(fail(format!("{what} の描画の数が元のファイルと違います")));
        }
        for (slot, (got, want)) in draws.iter().zip(&node.draws).enumerate() {
            if got.base_color.map(f32::to_bits) != want.base_color.map(f32::to_bits) {
                return Err(fail(format!("{what} の色が違います")));
            }
            let actual = model
                .primitives
                .get(got.prim.0 as usize)
                .ok_or_else(|| fail(format!("{what} の形状がありません")))?;
            let expected = match source_draws {
                Some(source) => {
                    let prim = source[slot].prim.0;
                    Some((u64::from(prim), &base_model.primitives[prim as usize]))
                }
                None if node.file_index.is_none() && node.template.is_none() => {
                    let slot = want
                        .prim
                        .0
                        .checked_sub(scene.base_primitive_count)
                        .ok_or_else(|| fail(format!("{what} の形状の番号が範囲の外です")))?;
                    let prim = scene
                        .added
                        .get(slot as usize)
                        .ok_or_else(|| fail(format!("{what} の形状の番号が範囲の外です")))?;
                    Some(((1u64 << 32) | u64::from(slot), prim.as_ref()))
                }
                // 元のファイルで読み込まれていないノード：JSONの照合で `mesh` が元のままなことを
                // 確かめてあるので、形状の中身はここでは比べない。
                None => None,
            };
            if let Some((key, expected)) = expected
                && compared.insert((got.prim.0, key))
                && !same_primitive(actual, expected)
            {
                return Err(fail(format!("{what} の形状が違います")));
            }
            placed += (actual.indices.len() / 3) as u64;
        }
    }
    if model.stats.placed_triangles != placed {
        return Err(fail("配置後の三角形数が違います".to_owned()));
    }
    Ok(())
}

fn base_node_count(model: &Model) -> usize {
    model
        .nodes
        .iter()
        .map(|node| node.gltf_index as usize + 1)
        .max()
        .unwrap_or(0)
}

/// 位置とインデックスはビット単位、法線は元に法線があるときだけ比べる
/// （法線の無い形状は、読み込みが0を入れて後で求める）。
fn same_primitive(actual: &Primitive, expected: &Primitive) -> bool {
    actual.has_normals == expected.has_normals
        && actual.indices == expected.indices
        && actual.vertices.len() == expected.vertices.len()
        && actual
            .vertices
            .iter()
            .zip(&expected.vertices)
            .all(|(a, b)| {
                a.position.map(f32::to_bits) == b.position.map(f32::to_bits)
                    && (!expected.has_normals
                        || a.normal.map(f32::to_bits) == b.normal.map(f32::to_bits))
            })
}

fn close(a: Affine3A, b: Affine3A) -> bool {
    let a = a.to_cols_array();
    let b = b.to_cols_array();
    let reach = a
        .iter()
        .chain(b.iter())
        .fold(1.0f32, |acc, value| acc.max(value.abs()));
    a.iter()
        .zip(b.iter())
        .all(|(x, y)| (x - y).abs() <= 1.0e-4 * reach)
}
