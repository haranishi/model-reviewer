//! 書いたバイト列を読み直して照合する。`build` の判断をなぞらず、`scene` と元のファイルから
//! 「書き換えてよい所」を別に導いて確かめる（`build` の誤りを `build` と同じ式で見逃さないため）。

mod images;
mod looks;

use std::collections::HashSet;

use super::textures::{Look, box_size, can_mark, model_look};
use glam::{Affine3A, Vec3};
use gltf::json::Value;
use model_reviewer_core::scene::{Model, Primitive, SceneRead};
use model_reviewer_core::texture::{UvMap, box_uv, same_look};

use super::base::{self, Base, same_bits};
use super::container::{self, align4};
use super::json::{array, read_children};
use super::patch::{self, Shape};
use super::{Built, ExportError, ExportNote, ExportScene};

const OWNED: [&str; 11] = [
    "nodes",
    "scenes",
    "scene",
    "meshes",
    "materials",
    "accessors",
    "bufferViews",
    "buffers",
    "images",
    "textures",
    "samplers",
];
/// ノードの編集対象。`mesh` は見た目の写しとして別に照合する。
const NODE_EDITABLE: [&str; 7] = [
    "matrix",
    "translation",
    "rotation",
    "scale",
    "children",
    "name",
    "mesh",
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
    let base = base::read(base_bytes, scene)?;
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
    let restored = check_nodes(&old_value, &new_value, &base, scene, &shape, built)?;
    check_scenes(&old_value, &new_value, &base, &shape, built)?;
    check_appended(&old_value, &new_value, &base, built, scene)?;
    let model = crate::load_glb(written)
        .map_err(|error| fail(format!("開けません: {}", error.message_ja())))?;
    let reopened = crate::inspect::open(written)
        .map_err(|error| fail(format!("開けません: {}", error.message_ja())))?;
    let written = Written {
        root: reopened.document.as_json(),
        bin: reopened.bin,
        value: &new_value,
    };
    check_model(&model, &base, scene, built, &written, &restored)?;
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

/// 2. 書き出しが扱う11項目以外は値として等しい。
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
) -> Result<HashSet<(usize, usize)>, ExportError> {
    let mut meshes = looks::MeshChecks::new(old, new, base, scene);
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
                meshes.node(index, node, new_node)?;
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
                meshes.node(index, node, new_node)?;
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
                meshes.node(index, node, new_node)?;
            }
        }
    }
    meshes.finish()
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
    scene: &ExportScene,
) -> Result<(), ExportError> {
    for key in [
        "meshes",
        "materials",
        "accessors",
        "bufferViews",
        "images",
        "textures",
        "samplers",
    ] {
        let old_items = array(old, key);
        let new_items = array(new, key);
        if !new_items.starts_with(old_items)
            || (new_items.len() == old_items.len() && old.get(key) != new.get(key))
        {
            return Err(fail(format!("{key} の元の要素が元と違います")));
        }
        // 材質・テクスチャの参照だけを替えるときは BIN が増えない。
        let geometry = matches!(key, "accessors" | "bufferViews");
        if geometry && built.appended_bin.is_empty() && new_items.len() != old_items.len() {
            return Err(fail(format!(
                "{key} に足した要素があるのにBINを足していません"
            )));
        }
    }
    let start = base.container.bin.map_or(0, |bin| align4(bin.len() as u64));
    let end = start + built.appended_bin.len() as u64;
    images::check(old, new, base, built, &scene.textures, start)?;
    let old_views = array(old, "bufferViews");
    let new_views = array(new, "bufferViews");
    for view in &new_views[old_views.len()..] {
        let buffer = view.get("buffer").and_then(Value::as_u64);
        let offset = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0);
        let length = view.get("byteLength").and_then(Value::as_u64).unwrap_or(0);
        if buffer != Some(0)
            || offset < start
            || !offset.is_multiple_of(4)
            || length == 0
            || offset.saturating_add(length) > end
        {
            return Err(fail(
                "足した bufferView が足したBINの範囲の外を指しています".to_owned(),
            ));
        }
    }
    let old_buffers = array(old, "buffers");
    let new_buffers = array(new, "buffers");
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

/// 書いたファイルの JSON と BIN。UV の組を描画ごとに読み直す。
struct Written<'a> {
    root: &'a gltf::json::Root,
    bin: &'a [u8],
    value: &'a Value,
}

impl Written<'_> {
    fn uv_sets(&self, node: u32, slot: usize, vertices: usize) -> Option<crate::build::UvSets> {
        let mesh = self.root.nodes.get(node as usize)?.mesh?.value();
        let primitive = self.root.meshes.get(mesh)?.primitives.get(slot)?;
        crate::build::uv_sets(self.root, self.bin, primitive, vertices)
            .ok()
            .flatten()
    }
}

/// 3. 本アプリの読み込みで開いた結果が `scene` と一致する。
fn check_model(
    model: &Model,
    base: &Base<'_>,
    scene: &ExportScene,
    built: &Built,
    written: &Written<'_>,
    restored: &HashSet<(usize, usize)>,
) -> Result<(), ExportError> {
    let new = written.value;
    if model.nodes.len() != scene.nodes.len() {
        return Err(fail(format!(
            "部品の数が違います（読み直し {}、文書 {}）",
            model.nodes.len(),
            scene.nodes.len()
        )));
    }
    let mut worlds: Vec<Affine3A> = Vec::with_capacity(scene.nodes.len());
    let mut compared = HashSet::new();
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
        let source_range = node
            .file_index
            .or(node.template)
            .map(|file| base.draw_range(file));
        let source_draws = match source_range.clone() {
            Some(range) => Some(
                range
                    .and_then(|r| base.model.draws.get(r))
                    .ok_or_else(|| fail(format!("{what} の元の描画がありません")))?,
            ),
            None => None,
        };
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
            let (key, expected) = match source_draws {
                Some(source) => {
                    let prim = source[slot].prim.0;
                    (u64::from(prim), &base.model.primitives[prim as usize])
                }
                None => {
                    let slot = want
                        .prim
                        .0
                        .checked_sub(scene.base_primitive_count)
                        .ok_or_else(|| fail(format!("{what} の形状の番号が範囲の外です")))?;
                    let prim = scene
                        .added
                        .get(slot as usize)
                        .ok_or_else(|| fail(format!("{what} の形状の番号が範囲の外です")))?;
                    ((1u64 << 32) | u64::from(slot), prim.as_ref())
                }
            };
            let look = scene
                .textures
                .look(want.tex)
                .map_err(|e| fail(e.message_ja()))?;
            let reloaded = model_look(model, got.tex);
            if !matches_look(look, reloaded, &base.value, new) {
                return Err(fail(format!(
                    "{what} の画像・貼り方・透明度の閾値が違います"
                )));
            }
            let sides = box_size(look).map(|(_, sides)| sides);
            let back = restored.contains(&(index, slot));
            if compared.insert((got.prim, key, sides.map(|s| s.map(f32::to_bits)), back)) {
                let matches = match sides {
                    Some(sides) => baked_matches(actual, model.prim_uvs(got.prim), expected, sides),
                    None if back => same_triangles(actual, expected),
                    None => same_primitive(actual, expected),
                };
                if !matches {
                    return Err(fail(format!("{what} の形状または UV が違います")));
                }
                // 焼いた形状は元の UV の組を、戻した形状は焼く前の UV の組を、角ごとに同じ値で持つ。
                let base_draw = source_range.clone().flatten().map(|r| r.start + slot);
                if let Some(draw) = base_draw.filter(|_| sides.is_some() || back) {
                    let file_slot = base.sources[draw].primitive as usize;
                    let got_sets = written
                        .uv_sets(built.assigned[index], file_slot, actual.vertices.len())
                        .ok_or_else(|| fail(format!("{what} の UV の組を読めません")))?;
                    let want_sets = base.uv_sets(draw).map_err(|e| fail(e.message_ja()))?;
                    if !same_uv_sets(actual, &got_sets, expected, &want_sets, back) {
                        return Err(fail(format!("{what} の元の UV の組が違います")));
                    }
                }
            }
            placed += (actual.indices.len() / 3) as u64;
        }
    }
    if model.stats.placed_triangles != placed {
        return Err(fail("配置後の三角形数が違います".to_owned()));
    }
    Ok(())
}

/// 元の追加情報が印を書けない型なら、今回焼いた写しだけは、後段の全三角形の UV 照合で補う。
fn matches_look(want: Look<'_>, got: Look<'_>, old: &Value, new: &Value) -> bool {
    if same_look(want, got) {
        return true;
    }
    let (Some((size, _)), Some((tex, hash))) = (box_size(want), got) else {
        return false;
    };
    let UvMap::Vertex {
        info_from: Some(material),
        ..
    } = tex.map
    else {
        return false;
    };
    let id = material as usize;
    if id < array(old, "materials").len() || can_mark(&new["materials"][id]) {
        return false;
    }
    let marked = model_reviewer_core::texture::TexUse {
        baked_box: Some(size),
        ..*tex
    };
    same_look(want, Some((&marked, hash)))
}

/// 角ごとに、両方にある組の UV がビット単位で同じ。焼いた形状（`back` が偽）は元の組をすべて持ち、
/// 足すのは箱形の組1つだけ（土台で焼いた形状をそのまま使うなら同じ組）。戻した形状は、焼いた
/// 形状が写した組をすべて持つ（箱形の組を除く）。
fn same_uv_sets(
    actual: &Primitive,
    got: &[(u32, Vec<[f32; 2]>)],
    expected: &Primitive,
    want: &[(u32, Vec<[f32; 2]>)],
    back: bool,
) -> bool {
    let (smaller, larger) = if back { (got, want) } else { (want, got) };
    if larger.len() != smaller.len() + 1 && larger.len() != smaller.len() {
        return false;
    }
    smaller.iter().all(|(set, values)| {
        let Some((_, other)) = larger.iter().find(|(n, _)| n == set) else {
            return false;
        };
        let (got_values, want_values) = if back {
            (values, other)
        } else {
            (other, values)
        };
        actual
            .indices
            .iter()
            .zip(&expected.indices)
            .all(
                |(&g, &w)| match (got_values.get(g as usize), want_values.get(w as usize)) {
                    (Some(a), Some(b)) => a.map(f32::to_bits) == b.map(f32::to_bits),
                    _ => false,
                },
            )
    })
}

/// 三角形の数と、角ごとの位置（と元にあれば法線）がビット単位で同じ。頂点の並びは問わない。
fn same_triangles(actual: &Primitive, expected: &Primitive) -> bool {
    actual.has_normals == expected.has_normals
        && actual.indices.len() == expected.indices.len()
        && actual
            .indices
            .iter()
            .zip(&expected.indices)
            .all(|(&a, &b)| {
                match (
                    actual.vertices.get(a as usize),
                    expected.vertices.get(b as usize),
                ) {
                    (Some(a), Some(b)) => {
                        a.position.map(f32::to_bits) == b.position.map(f32::to_bits)
                            && (!expected.has_normals
                                || a.normal.map(f32::to_bits) == b.normal.map(f32::to_bits))
                    }
                    _ => false,
                }
            })
}

fn baked_matches(
    actual: &Primitive,
    uvs: &[[f32; 2]],
    expected: &Primitive,
    sides: [f32; 2],
) -> bool {
    let center = expected.local_aabb.center();
    if actual.indices.len() != expected.indices.len()
        || actual.has_normals != expected.has_normals
        || actual.vertices.len() != uvs.len()
    {
        return false;
    }
    actual
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .zip(expected.indices.as_chunks::<3>().0)
        .all(|(got, want)| {
            let [Some(a), Some(b), Some(c)] = want.map(|i| expected.vertices.get(i as usize))
            else {
                return false;
            };
            let [p, q, r] = [a, b, c].map(|v| Vec3::from(v.position));
            let normal = (q - p).cross(r - p);
            got.iter().zip([a, b, c]).all(|(&i, source)| {
                let vertex = &actual.vertices[i as usize];
                vertex.position.map(f32::to_bits) == source.position.map(f32::to_bits)
                    && (!expected.has_normals
                        || vertex.normal.map(f32::to_bits) == source.normal.map(f32::to_bits))
                    && uvs[i as usize]
                        .into_iter()
                        .zip(box_uv(Vec3::from(source.position), normal, center, sides))
                        .all(|(v, target)| {
                            v.is_finite()
                                && target.is_finite()
                                && (v - target).abs() <= 1e-6 * target.abs().max(1.0)
                        })
            })
        })
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
