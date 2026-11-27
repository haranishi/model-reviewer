//! 元のJSONを値として直す。変えた所だけを書き換え、足す物は配列の末尾に追記する。

use gltf::json::Value;

use super::append::{Appender, Counts};
use super::base::{self, Base, same_bits};
use super::container::{self, align4};
use super::json::{obj, set, uint, uints, write_children, write_name, write_transform};
use super::{Built, ExportError, ExportNote, ExportScene};

/// `ExportScene` を元のファイルに当てはめた形。
pub(crate) struct Shape {
    /// `scene.nodes[i]` の子（`scene.nodes` の添字、並びは文書の並び）。
    pub children: Vec<Vec<u32>>,
    /// 根（`scene.nodes` の添字）。
    pub roots: Vec<u32>,
    /// 元のノードごとの、`scene.nodes` の添字。
    pub in_scene: Vec<Option<u32>>,
    /// `scene.nodes[i]` のファイル内の番号。
    pub assigned: Vec<u32>,
}

pub(crate) fn build(bytes: &[u8], scene: &ExportScene) -> Result<Built, ExportError> {
    let base = base::read(bytes)?;
    let shape = shape(&base, scene)?;
    let mut root = base.value.clone();
    let mut matrix_written = 0u32;
    let mut animated = 0u32;
    let mut skinned = 0u32;

    // ファイルにある部品：変えた所だけを書き換える。
    {
        let nodes = array_mut(&mut root, "nodes")?;
        for (index, node) in scene.nodes.iter().enumerate() {
            let Some(file) = node.file_index else {
                continue;
            };
            let file = file as usize;
            let new_children: Vec<u32> = shape.children[index]
                .iter()
                .map(|child| shape.assigned[*child as usize])
                .collect();
            let transform_changed =
                !base.locals[file].is_some_and(|local| same_bits(local, node.local));
            let name_changed = node.name != base.names[file];
            let children_changed = new_children != base.children[file];
            if !(transform_changed || name_changed || children_changed) {
                continue;
            }
            let target = node_mut(nodes, file)?;
            if transform_changed {
                if write_transform(target, node.local)? {
                    matrix_written += 1;
                }
                animated += u32::from(base.animated[file]);
                skinned += u32::from(base.skinned[file]);
            }
            if name_changed {
                write_name(target, &node.name);
            }
            if children_changed {
                write_children(target, &new_children);
            }
        }
        // 文書に無いノードは配列に残す。ただし、文書にある部品を子に持っていれば外す
        // （外さないと、その部品に親が2つできる）。
        for file in 0..base.node_count {
            if shape.in_scene[file].is_some() {
                continue;
            }
            let kept: Vec<u32> = base.children[file]
                .iter()
                .copied()
                .filter(|child| shape.in_scene[*child as usize].is_none())
                .collect();
            if kept.len() != base.children[file].len() {
                write_children(node_mut(nodes, file)?, &kept);
            }
        }
    }

    // 足す部品：行きがけ順に配列の末尾へ。
    let bin_start = base.container.bin.map_or(0, |bin| align4(bin.len() as u64));
    let mut appender = Appender::new(
        bin_start,
        scene.base_primitive_count,
        &scene.added,
        Counts {
            meshes: array_len(&root, "meshes"),
            materials: array_len(&root, "materials"),
            accessors: array_len(&root, "accessors"),
            views: array_len(&root, "bufferViews"),
        },
    );
    let mut new_nodes = Vec::new();
    for (index, node) in scene.nodes.iter().enumerate() {
        if node.file_index.is_some() {
            continue;
        }
        let children: Vec<u32> = shape.children[index]
            .iter()
            .map(|child| shape.assigned[*child as usize])
            .collect();
        let mut value = match node.template {
            Some(template) => {
                let template = template as usize;
                let mut copy = base_node(&base.value, template)?.clone();
                if base.skinned[template]
                    && !base.locals[template].is_some_and(|local| same_bits(local, node.local))
                {
                    skinned += 1;
                }
                copy.as_object_mut()
                    .map(|map| map.remove("children"))
                    .ok_or_else(|| {
                        ExportError::InvalidBase("ノードがオブジェクトではありません".to_owned())
                    })?;
                copy
            }
            None => {
                let mut fresh = obj();
                if !node.draws.is_empty() {
                    set(&mut fresh, "mesh", uint(appender.mesh(&node.draws)?));
                }
                fresh
            }
        };
        write_name(&mut value, &node.name);
        if write_transform(&mut value, node.local)? {
            matrix_written += 1;
        }
        write_children(&mut value, &children);
        new_nodes.push(value);
    }
    array_mut(&mut root, "nodes")?.extend(new_nodes);

    // 根。読み込みが使ったシーンだけを書き換える。
    let desired: Vec<u32> = shape
        .roots
        .iter()
        .map(|index| shape.assigned[*index as usize])
        .collect();
    let mut scene_added = false;
    if base.has_scenes {
        if desired != base.roots {
            let scenes = array_mut(&mut root, "scenes")?;
            let target = scenes
                .get_mut(base.scene_index)
                .ok_or_else(|| ExportError::InvalidBase("シーンがありません".to_owned()))?;
            set(target, "nodes", uints(&desired));
        }
    } else if implicit_roots(&root)? != desired {
        set(&mut root, "scenes", Value::Array(vec![scene_of(&desired)]));
        set(&mut root, "scene", uint(0));
        scene_added = true;
    }

    // 足した形状。
    let appended_bin = std::mem::take(&mut appender.bin);
    extend(&mut root, "meshes", std::mem::take(&mut appender.meshes))?;
    extend(
        &mut root,
        "materials",
        std::mem::take(&mut appender.materials),
    )?;
    extend(
        &mut root,
        "accessors",
        std::mem::take(&mut appender.accessors),
    )?;
    extend(
        &mut root,
        "bufferViews",
        std::mem::take(&mut appender.views),
    )?;
    if !appended_bin.is_empty() {
        let length = bin_start + appended_bin.len() as u64;
        let buffers = array_mut(&mut root, "buffers")?;
        match buffers.first_mut() {
            Some(buffer) => set(buffer, "byteLength", uint(length)),
            None => {
                let mut buffer = obj();
                set(&mut buffer, "byteLength", uint(length));
                buffers.push(buffer);
            }
        }
    }

    let json = gltf::json::serialize::to_vec(&root)
        .map_err(|error| ExportError::InvalidScene(format!("JSONを書けません: {error}")))?;
    super::glb_length(
        json.len() as u64,
        container::bin_chunk_len(base.container.bin, &appended_bin),
    )?;

    let detached = (0..base.node_count)
        .filter(|file| base.loaded[*file] && shape.in_scene[*file].is_none())
        .count() as u32;
    let mut notes = Vec::new();
    if detached > 0 {
        notes.push(ExportNote::DetachedNodes { count: detached });
    }
    if animated > 0 {
        notes.push(ExportNote::AnimatedNodesChanged { count: animated });
    }
    if skinned > 0 {
        notes.push(ExportNote::SkinnedNodesChanged { count: skinned });
    }
    if matrix_written > 0 {
        notes.push(ExportNote::MatrixWritten {
            count: matrix_written,
        });
    }
    if scene_added {
        notes.push(ExportNote::SceneAdded);
    }
    Ok(Built {
        json,
        appended_bin,
        assigned: shape.assigned,
        notes,
    })
}

/// `scene` を確かめ、元のファイルに当てはめる。行きがけ順でない、番号が範囲の外、
/// 同じノードが2回、などは断る。
pub(crate) fn shape(base: &Base<'_>, scene: &ExportScene) -> Result<Shape, ExportError> {
    let invalid = |what: String| ExportError::InvalidScene(what);
    let count =
        u32::try_from(scene.nodes.len()).map_err(|_| invalid("部品が多すぎます".to_owned()))?;
    let mut children = vec![Vec::new(); scene.nodes.len()];
    let mut roots = Vec::new();
    let mut in_scene = vec![None; base.node_count];
    let mut assigned = Vec::with_capacity(scene.nodes.len());
    let mut next_new =
        u32::try_from(base.node_count).map_err(|_| invalid("ノードが多すぎます".to_owned()))?;
    let added_end = u64::from(scene.base_primitive_count) + scene.added.len() as u64;
    let mut stack: Vec<u32> = Vec::new();
    let mut draws = 0usize;
    for (index, node) in (0..count).zip(&scene.nodes) {
        match node.parent {
            None => {
                stack.clear();
                roots.push(index);
            }
            Some(parent) => {
                if parent >= index {
                    return Err(invalid(format!("部品 {index} の親が後ろにあります")));
                }
                while stack.last().is_some_and(|top| *top != parent) {
                    stack.pop();
                }
                if stack.is_empty() {
                    return Err(invalid(format!(
                        "部品 {index} が行きがけ順に並んでいません"
                    )));
                }
                children[parent as usize].push(index);
            }
        }
        stack.push(index);
        draws += node.draws.len();
        match (node.file_index, node.template) {
            (Some(file), None) => {
                let slot = in_scene
                    .get_mut(file as usize)
                    .ok_or_else(|| invalid(format!("ファイルにないノード番号 {file}")))?;
                if slot.is_some() {
                    return Err(invalid(format!("ノード {file} が2回あります")));
                }
                *slot = Some(index);
                assigned.push(file);
            }
            (Some(_), Some(_)) => {
                return Err(invalid(format!(
                    "部品 {index} がファイルの番号と複製の元を両方持っています"
                )));
            }
            (None, template) => {
                if let Some(template) = template
                    && template as usize >= base.node_count
                {
                    return Err(invalid(format!(
                        "複製の元 {template} がファイルにありません"
                    )));
                }
                if template.is_none() {
                    for draw in &node.draws {
                        let prim = u64::from(draw.prim.0);
                        if prim < u64::from(scene.base_primitive_count) || prim >= added_end {
                            return Err(invalid(format!(
                                "足した部品 {index} の形状 {prim} が足した形状の範囲の外です"
                            )));
                        }
                    }
                }
                assigned.push(next_new);
                next_new = next_new
                    .checked_add(1)
                    .ok_or_else(|| invalid("ノードが多すぎます".to_owned()))?;
            }
        }
    }
    if draws == 0 {
        return Err(invalid("表示できる形状が1つも残りません".to_owned()));
    }
    Ok(Shape {
        children,
        roots,
        in_scene,
        assigned,
    })
}

/// シーンが無いファイルで、読み込みが根にするノード（親の無いノードを番号順に）。
pub(crate) fn implicit_roots(root: &Value) -> Result<Vec<u32>, ExportError> {
    let nodes = root
        .get("nodes")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut has_parent = vec![false; nodes.len()];
    for node in nodes {
        let children = super::json::read_children(node).ok_or_else(|| {
            ExportError::InvalidBase("children が番号の列ではありません".to_owned())
        })?;
        for child in children {
            if let Some(flag) = has_parent.get_mut(child as usize) {
                *flag = true;
            }
        }
    }
    Ok((0..nodes.len() as u32)
        .filter(|index| !has_parent[*index as usize])
        .collect())
}

pub(crate) fn scene_of(roots: &[u32]) -> Value {
    let mut scene = obj();
    set(&mut scene, "nodes", uints(roots));
    scene
}

pub(crate) fn base_node(root: &Value, index: usize) -> Result<&Value, ExportError> {
    root.get("nodes")
        .and_then(Value::as_array)
        .and_then(|nodes| nodes.get(index))
        .filter(|node| node.is_object())
        .ok_or_else(|| ExportError::InvalidBase(format!("ノード {index} がありません")))
}

fn node_mut(nodes: &mut [Value], index: usize) -> Result<&mut Value, ExportError> {
    nodes
        .get_mut(index)
        .filter(|node| node.is_object())
        .ok_or_else(|| ExportError::InvalidBase(format!("ノード {index} がありません")))
}

/// 最上位の配列を取り出す。無ければ空の配列を作る。
fn array_mut<'v>(root: &'v mut Value, key: &str) -> Result<&'v mut Vec<Value>, ExportError> {
    let Value::Object(map) = root else {
        return Err(ExportError::InvalidBase(
            "JSONの最上位がオブジェクトではありません".to_owned(),
        ));
    };
    map.entry(key.to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| ExportError::InvalidBase(format!("{key} が配列ではありません")))
}

fn extend(root: &mut Value, key: &str, items: Vec<Value>) -> Result<(), ExportError> {
    if items.is_empty() {
        return Ok(());
    }
    array_mut(root, key)?.extend(items);
    Ok(())
}

fn array_len(root: &Value, key: &str) -> usize {
    root.get(key).and_then(Value::as_array).map_or(0, Vec::len)
}
