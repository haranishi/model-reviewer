//! 色の変わった部品。元の材質とメッシュは書き換えず、写して配列の末尾に足す。
//!
//! - どの描画がどのプリミティブかは、読み込みが作った対応（`Base::sources`）をそのまま使う。
//!   点・線・読めないプリミティブは描画にならないので、ここで数え直さない。
//! - 変わったかどうかは、土台のモデルの色とビット単位で比べる（印は持たない）。ファイルにある部品は
//!   自分のノード、複製は写す元のノードと比べる。元に戻した色は「変わっていない」になる。
//! - 材質の写しは、元の材質の `pbrMetallicRoughness.baseColorFactor` だけを替える。材質の無い
//!   プリミティブには、基本色だけの材質を作る。鍵は（元の材質、新しい色のビット）。
//! - メッシュの写しは、色の変わったプリミティブの `material` だけを替える。鍵は
//!   （元のメッシュ、プリミティブごとの材質の並び）。アクセサ・バッファビュー・BIN は足さない。

use std::collections::HashMap;

use gltf::json::Value;

use super::base::Base;
use super::json::{f32s, obj, set, uint};
use super::{ExportError, ExportScene};

/// 色の写しの結果。`patch::build` がノードの `mesh` を替え、配列の末尾に足す。
pub(crate) struct Recolored {
    /// `scene.nodes[i]` が指す、写したメッシュの番号（色が変わった部品だけ）。
    pub mesh_of: Vec<Option<u64>>,
    /// `materials` の末尾に足す材質。
    pub materials: Vec<Value>,
    /// `meshes` の末尾に足すメッシュ。
    pub meshes: Vec<Value>,
    /// メッシュを替えた部品の数。
    pub nodes: u32,
}

pub(crate) fn plan(base: &Base<'_>, scene: &ExportScene) -> Result<Recolored, ExportError> {
    let first_material = array(&base.value, "materials").len() as u64;
    let first_mesh = array(&base.value, "meshes").len() as u64;
    let loaded = base.loaded_nodes();
    let mut out = Recolored {
        mesh_of: vec![None; scene.nodes.len()],
        materials: Vec::new(),
        meshes: Vec::new(),
        nodes: 0,
    };
    let mut material_keys: HashMap<(Option<u64>, [u32; 4]), u64> = HashMap::new();
    let mut mesh_keys: HashMap<(u64, Vec<Option<u64>>), u64> = HashMap::new();
    for (index, node) in scene.nodes.iter().enumerate() {
        let Some(file) = node.file_index.or(node.template) else {
            continue;
        };
        // 読み込まれていないノード（土台のモデルに無い）は比べる相手が無い。照合も形状を比べない。
        let Some(at) = loaded.get(file as usize).copied().flatten() else {
            continue;
        };
        let source = &base.model.nodes[at];
        let range = source.draws.start as usize..source.draws.end as usize;
        let base_draws = base
            .model
            .draws
            .get(range.clone())
            .ok_or_else(|| invalid_base("描画の範囲が不正です"))?;
        let sources = base
            .sources
            .get(range)
            .ok_or_else(|| invalid_base("描画の出どころが足りません"))?;
        if base_draws.len() != node.draws.len() {
            return Err(ExportError::InvalidScene(format!(
                "部品 {index} の描画の数が元のファイルと違います"
            )));
        }
        // 形状の番号（`Draw::prim`）は比べない。文書の番号は最初に開いたモデルの数え方のままで、
        // 書き出して土台が移った後（削除した部品がシーンから外れている土台など）は、土台を読み直した
        // 番号とずれることがある。同じノードの i 番目の描画どうしを、色だけで比べる。
        let changed: Vec<bool> = node
            .draws
            .iter()
            .zip(base_draws)
            .map(|(draw, original)| {
                draw.base_color.map(f32::to_bits) != original.base_color.map(f32::to_bits)
            })
            .collect();
        if !changed.contains(&true) {
            continue;
        }
        let mesh = u64::from(sources[0].mesh);
        if sources.iter().any(|item| u64::from(item.mesh) != mesh)
            || node_mesh(&base.value, file)? != Some(mesh)
        {
            return Err(invalid_base("部品の描画とメッシュの対応が合いません"));
        }
        let original_mesh = array(&base.value, "meshes")
            .get(mesh as usize)
            .filter(|value| value.is_object())
            .ok_or_else(|| invalid_base("メッシュがありません"))?;
        let primitives = original_mesh
            .get("primitives")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_base("メッシュにプリミティブの列がありません"))?;
        let original_materials = primitives
            .iter()
            .map(material_of)
            .collect::<Result<Vec<_>, _>>()?;
        let mut materials = original_materials.clone();
        for ((draw, source), changed) in node.draws.iter().zip(sources).zip(&changed) {
            if !changed {
                continue;
            }
            let slot = source.primitive as usize;
            let original = *original_materials
                .get(slot)
                .ok_or_else(|| invalid_base("プリミティブの番号が範囲の外です"))?;
            let key = (original, draw.base_color.map(f32::to_bits));
            let material = match material_keys.get(&key) {
                Some(material) => *material,
                None => {
                    let copy = material_copy(&base.value, original, draw.base_color)?;
                    let material = first_material + out.materials.len() as u64;
                    out.materials.push(copy);
                    material_keys.insert(key, material);
                    material
                }
            };
            materials[slot] = Some(material);
        }
        let key = (mesh, materials);
        let copied = match mesh_keys.get(&key) {
            Some(copied) => *copied,
            None => {
                let mut copy = original_mesh.clone();
                let list = copy
                    .get_mut("primitives")
                    .and_then(Value::as_array_mut)
                    .ok_or_else(|| invalid_base("メッシュにプリミティブの列がありません"))?;
                for (primitive, (material, original)) in
                    list.iter_mut().zip(key.1.iter().zip(&original_materials))
                {
                    if material != original
                        && let Some(material) = material
                    {
                        set(primitive, "material", uint(*material));
                    }
                }
                let copied = first_mesh + out.meshes.len() as u64;
                out.meshes.push(copy);
                mesh_keys.insert(key, copied);
                copied
            }
        };
        out.mesh_of[index] = Some(copied);
        out.nodes += 1;
    }
    Ok(out)
}

/// 元の材質の写しで、基本色だけを `color` にしたもの。元が無ければ基本色だけの材質。
fn material_copy(
    root: &Value,
    original: Option<u64>,
    color: [f32; 4],
) -> Result<Value, ExportError> {
    let mut material = match original {
        Some(index) => array(root, "materials")
            .get(index as usize)
            .filter(|value| value.is_object())
            .cloned()
            .ok_or_else(|| invalid_base("材質がありません"))?,
        None => obj(),
    };
    let Value::Object(map) = &mut material else {
        return Err(invalid_base("材質がオブジェクトではありません"));
    };
    let pbr = map
        .entry("pbrMetallicRoughness".to_owned())
        .or_insert_with(obj);
    if !pbr.is_object() {
        return Err(invalid_base(
            "pbrMetallicRoughness がオブジェクトではありません",
        ));
    }
    set(pbr, "baseColorFactor", f32s(&color)?);
    Ok(material)
}

fn material_of(primitive: &Value) -> Result<Option<u64>, ExportError> {
    match primitive.get("material") {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| invalid_base("material が番号ではありません")),
    }
}

fn node_mesh(root: &Value, file: u32) -> Result<Option<u64>, ExportError> {
    let node = array(root, "nodes")
        .get(file as usize)
        .ok_or_else(|| invalid_base("ノードがありません"))?;
    match node.get("mesh") {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| invalid_base("mesh が番号ではありません")),
    }
}

fn array<'v>(root: &'v Value, key: &str) -> &'v [Value] {
    root.get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn invalid_base(what: &str) -> ExportError {
    ExportError::InvalidBase(what.to_owned())
}
