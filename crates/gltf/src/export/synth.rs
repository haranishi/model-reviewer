//! 土台の合成。部品1つ・描画1つ・形状1つの `Model`（STL から読んだもの）から、最小の GLB を作る。
//! 書き出しは、この GLB を土台にして第2段階の `build`・`write_container`・照合へ渡す。
//!
//! 作る GLB: BIN は位置（`f32` × 3）と番号（`u32`）だけ。JSON は `asset`、シーン1つ、ノード1つ
//! （名前、`mesh`、変形は既定値でない項目だけ）、メッシュ1つ（`POSITION` と `indices`、三角形リスト）、
//! 材質1つ、アクセサ2つ（位置には `min`・`max`）。`NORMAL` は書かない（STL の形状は法線を持たない）。

use gltf::json::Value;
use model_reviewer_core::scene::{Model, Node, Primitive};

use super::ExportError;
use super::container::align4;
use super::glb_length;
use super::json::{f32s, obj, set, uint, write_name, write_transform};

const FLOAT: u64 = 5126;
const UNSIGNED_INT: u64 = 5125;
const ARRAY_BUFFER: u64 = 34962;
const ELEMENT_ARRAY_BUFFER: u64 = 34963;
const TRIANGLES: u64 = 4;

/// 部品1つ・描画1つ・形状1つの `Model` から、最小の GLB を作る。同じ入力からは同じバイト列になる。
/// 形が違えば `InvalidScene`、4GiB を超えれば `TooLarge`。
pub fn synthesize_base(model: &Model) -> Result<Vec<u8>, ExportError> {
    let (node, draw, prim) = match (
        model.nodes.as_slice(),
        model.draws.as_slice(),
        model.primitives.as_slice(),
    ) {
        ([node], [draw], [prim]) => (node, draw, prim),
        _ => {
            return Err(scene_error(
                "土台を作れるのは、部品1つ・描画1つ・形状1つのモデルだけです",
            ));
        }
    };
    if node.parent.is_some() || node.draws != (0..1) || draw.prim.0 != 0 {
        return Err(scene_error("部品と描画と形状のつながりが合いません"));
    }
    check_primitive(prim)?;

    let vertex_count = prim.vertices.len() as u64;
    let index_count = prim.indices.len() as u64;
    let positions_len = vertex_count.saturating_mul(12);
    let indices_len = index_count.saturating_mul(4);
    let bin_len = positions_len.saturating_add(indices_len);
    // JSON は小さい（数百バイト）。先に BIN だけで上限を確かめ、大きな確保をしない。
    glb_length(0, Some(bin_len))?;

    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for vertex in &prim.vertices {
        for axis in 0..3 {
            min[axis] = min[axis].min(vertex.position[axis]);
            max[axis] = max[axis].max(vertex.position[axis]);
        }
    }

    let json = json_bytes(
        node,
        draw.base_color,
        &JsonSizes {
            vertex_count,
            index_count,
            positions_len,
            indices_len,
            min,
            max,
        },
    )?;
    let total = glb_length(json.len() as u64, Some(bin_len))?;
    let json_padded = align4(json.len() as u64);
    let total_us = usize::try_from(total).map_err(|_| ExportError::TooLarge { bytes: total })?;

    let mut out = Vec::with_capacity(total_us);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_padded as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json);
    out.resize(out.len() + (json_padded as usize - json.len()), b' ');
    // BIN は位置（12バイト × 頂点）と番号（4バイト × 番号）だけなので、常に4の倍数。
    out.extend_from_slice(&(bin_len as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    for vertex in &prim.vertices {
        for value in vertex.position {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    for index in &prim.indices {
        out.extend_from_slice(&index.to_le_bytes());
    }
    debug_assert_eq!(out.len() as u64, total);
    Ok(out)
}

struct JsonSizes {
    vertex_count: u64,
    index_count: u64,
    positions_len: u64,
    indices_len: u64,
    min: [f32; 3],
    max: [f32; 3],
}

fn json_bytes(node: &Node, color: [f32; 4], sizes: &JsonSizes) -> Result<Vec<u8>, ExportError> {
    let mut asset = obj();
    set(&mut asset, "version", Value::String("2.0".to_owned()));
    set(
        &mut asset,
        "generator",
        Value::String("model-reviewer".to_owned()),
    );

    let mut scene = obj();
    set(&mut scene, "nodes", Value::Array(vec![uint(0)]));

    let mut node_json = obj();
    write_name(&mut node_json, &node.name);
    set(&mut node_json, "mesh", uint(0));
    write_transform(&mut node_json, node.local)?;

    let mut attributes = obj();
    set(&mut attributes, "POSITION", uint(0));
    let mut primitive = obj();
    set(&mut primitive, "attributes", attributes);
    set(&mut primitive, "indices", uint(1));
    set(&mut primitive, "material", uint(0));
    set(&mut primitive, "mode", uint(TRIANGLES));
    let mut mesh = obj();
    set(&mut mesh, "primitives", Value::Array(vec![primitive]));

    let mut pbr = obj();
    set(&mut pbr, "baseColorFactor", f32s(&color)?);
    // glTF の metallicFactor は既定で 1.0（金属）なので、足した図形と同じく 0 を明示する。
    set(&mut pbr, "metallicFactor", Value::from(0.0));
    let mut material = obj();
    set(&mut material, "pbrMetallicRoughness", pbr);

    let mut position_accessor = obj();
    set(&mut position_accessor, "bufferView", uint(0));
    set(&mut position_accessor, "componentType", uint(FLOAT));
    set(&mut position_accessor, "count", uint(sizes.vertex_count));
    set(
        &mut position_accessor,
        "type",
        Value::String("VEC3".to_owned()),
    );
    set(&mut position_accessor, "min", f32s(&sizes.min)?);
    set(&mut position_accessor, "max", f32s(&sizes.max)?);
    let mut index_accessor = obj();
    set(&mut index_accessor, "bufferView", uint(1));
    set(&mut index_accessor, "componentType", uint(UNSIGNED_INT));
    set(&mut index_accessor, "count", uint(sizes.index_count));
    set(
        &mut index_accessor,
        "type",
        Value::String("SCALAR".to_owned()),
    );

    let view = |offset: u64, length: u64, target: u64| {
        let mut view = obj();
        set(&mut view, "buffer", uint(0));
        set(&mut view, "byteOffset", uint(offset));
        set(&mut view, "byteLength", uint(length));
        set(&mut view, "target", uint(target));
        view
    };
    let mut buffer = obj();
    set(
        &mut buffer,
        "byteLength",
        uint(sizes.positions_len + sizes.indices_len),
    );

    let mut root = obj();
    set(&mut root, "asset", asset);
    set(&mut root, "scene", uint(0));
    set(&mut root, "scenes", Value::Array(vec![scene]));
    set(&mut root, "nodes", Value::Array(vec![node_json]));
    set(&mut root, "meshes", Value::Array(vec![mesh]));
    set(&mut root, "materials", Value::Array(vec![material]));
    set(
        &mut root,
        "accessors",
        Value::Array(vec![position_accessor, index_accessor]),
    );
    set(
        &mut root,
        "bufferViews",
        Value::Array(vec![
            view(0, sizes.positions_len, ARRAY_BUFFER),
            view(sizes.positions_len, sizes.indices_len, ELEMENT_ARRAY_BUFFER),
        ]),
    );
    set(&mut root, "buffers", Value::Array(vec![buffer]));
    gltf::json::serialize::to_vec(&root)
        .map_err(|error| scene_error(&format!("JSONを作れません: {error}")))
}

/// 読み込みが同じ形状として読み戻せる形状か。読み戻せない物は作らずに断る。
fn check_primitive(prim: &Primitive) -> Result<(), ExportError> {
    if prim.vertices.is_empty() || prim.indices.is_empty() {
        return Err(scene_error("形状に頂点か三角形がありません"));
    }
    if u32::try_from(prim.vertices.len()).is_err() || u32::try_from(prim.indices.len()).is_err() {
        return Err(ExportError::TooLarge {
            bytes: (prim.vertices.len() as u64)
                .saturating_mul(12)
                .saturating_add((prim.indices.len() as u64).saturating_mul(4)),
        });
    }
    if !prim.indices.len().is_multiple_of(3) {
        return Err(scene_error("形状の番号の数が3の倍数ではありません"));
    }
    if prim
        .indices
        .iter()
        .any(|index| *index as usize >= prim.vertices.len())
    {
        return Err(scene_error("形状の番号が頂点の数を超えています"));
    }
    if prim
        .vertices
        .iter()
        .any(|vertex| !vertex.position.iter().all(|value| value.is_finite()))
    {
        return Err(scene_error("形状の位置に有限でない値があります"));
    }
    Ok(())
}

fn scene_error(what: &str) -> ExportError {
    ExportError::InvalidScene(what.to_owned())
}
