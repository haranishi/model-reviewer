//! 足した図形の形状を、BINの末尾と `meshes`・`materials`・`accessors`・`bufferViews` の末尾に足す。
//! 同じ形状・同じ色・同じ描画の並びは1回だけ足して共有する。

use std::collections::HashMap;

use gltf::json::Value;
use model_reviewer_core::scene::{Draw, Primitive};

use super::ExportError;
use super::json::{f32s, obj, set, uint};

const FLOAT: u64 = 5126;
const UNSIGNED_INT: u64 = 5125;
const ARRAY_BUFFER: u64 = 34962;
const ELEMENT_ARRAY_BUFFER: u64 = 34963;

#[derive(Clone, Copy)]
struct PrimRefs {
    position: u64,
    normal: Option<u64>,
    indices: u64,
}

pub(crate) struct Appender<'a> {
    /// 足すデータがBINの中で始まる位置（元のBINを4バイト境界にそろえた長さ）。
    start: u64,
    base_primitive_count: u32,
    added: &'a [std::sync::Arc<Primitive>],
    first_mesh: u64,
    first_material: u64,
    first_accessor: u64,
    first_view: u64,
    pub bin: Vec<u8>,
    pub meshes: Vec<Value>,
    pub materials: Vec<Value>,
    pub accessors: Vec<Value>,
    pub views: Vec<Value>,
    prims: HashMap<u32, PrimRefs>,
    colors: HashMap<[u32; 4], u64>,
    mesh_keys: HashMap<Vec<(u32, [u32; 4])>, u64>,
}

/// 足す先の配列の、いまの長さ。
pub(crate) struct Counts {
    pub meshes: usize,
    pub materials: usize,
    pub accessors: usize,
    pub views: usize,
}

impl<'a> Appender<'a> {
    pub fn new(
        start: u64,
        base_primitive_count: u32,
        added: &'a [std::sync::Arc<Primitive>],
        counts: Counts,
    ) -> Self {
        Self {
            start,
            base_primitive_count,
            added,
            first_mesh: counts.meshes as u64,
            first_material: counts.materials as u64,
            first_accessor: counts.accessors as u64,
            first_view: counts.views as u64,
            bin: Vec::new(),
            meshes: Vec::new(),
            materials: Vec::new(),
            accessors: Vec::new(),
            views: Vec::new(),
            prims: HashMap::new(),
            colors: HashMap::new(),
            mesh_keys: HashMap::new(),
        }
    }

    /// 描画の並びに対応する `mesh` の番号。同じ並びなら同じ番号を返す。
    pub fn mesh(&mut self, draws: &[Draw]) -> Result<u64, ExportError> {
        let key: Vec<(u32, [u32; 4])> = draws
            .iter()
            .map(|draw| (draw.prim.0, draw.base_color.map(f32::to_bits)))
            .collect();
        if let Some(index) = self.mesh_keys.get(&key) {
            return Ok(*index);
        }
        let mut primitives = Vec::with_capacity(draws.len());
        for draw in draws {
            let refs = self.primitive(draw.prim.0)?;
            let material = self.material(draw.base_color)?;
            let mut attributes = obj();
            set(&mut attributes, "POSITION", uint(refs.position));
            if let Some(normal) = refs.normal {
                set(&mut attributes, "NORMAL", uint(normal));
            }
            let mut primitive = obj();
            set(&mut primitive, "attributes", attributes);
            set(&mut primitive, "indices", uint(refs.indices));
            set(&mut primitive, "material", uint(material));
            primitives.push(primitive);
        }
        let mut mesh = obj();
        set(&mut mesh, "primitives", Value::Array(primitives));
        let index = self.first_mesh + self.meshes.len() as u64;
        self.meshes.push(mesh);
        self.mesh_keys.insert(key, index);
        Ok(index)
    }

    fn material(&mut self, color: [f32; 4]) -> Result<u64, ExportError> {
        let key = color.map(f32::to_bits);
        if let Some(index) = self.colors.get(&key) {
            return Ok(*index);
        }
        let mut pbr = obj();
        set(&mut pbr, "baseColorFactor", f32s(&color)?);
        // glTF の metallicFactor は既定で 1.0（金属）なので、0 を明示して他のソフトでも非金属にする。
        set(&mut pbr, "metallicFactor", Value::from(0.0));
        let mut material = obj();
        set(&mut material, "pbrMetallicRoughness", pbr);
        let index = self.first_material + self.materials.len() as u64;
        self.materials.push(material);
        self.colors.insert(key, index);
        Ok(index)
    }

    fn primitive(&mut self, prim: u32) -> Result<PrimRefs, ExportError> {
        if let Some(refs) = self.prims.get(&prim) {
            return Ok(*refs);
        }
        let slot = prim
            .checked_sub(self.base_primitive_count)
            .ok_or_else(|| scene_error("足した部品が元のファイルの形状を指しています"))?;
        let source = self
            .added
            .get(slot as usize)
            .cloned()
            .ok_or_else(|| scene_error("足した形状の番号が範囲の外です"))?;
        check_primitive(&source)?;
        let count = source.vertices.len() as u64;
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        let mut positions = Vec::with_capacity(source.vertices.len() * 12);
        for vertex in &source.vertices {
            for axis in 0..3 {
                min[axis] = min[axis].min(vertex.position[axis]);
                max[axis] = max[axis].max(vertex.position[axis]);
                positions.extend_from_slice(&vertex.position[axis].to_le_bytes());
            }
        }
        let view = self.view(&positions, ARRAY_BUFFER);
        let position = self.accessor(view, count, "VEC3", FLOAT, Some((&min, &max)))?;
        let normal = if source.has_normals {
            let mut normals = Vec::with_capacity(source.vertices.len() * 12);
            for vertex in &source.vertices {
                for value in vertex.normal {
                    normals.extend_from_slice(&value.to_le_bytes());
                }
            }
            let view = self.view(&normals, ARRAY_BUFFER);
            Some(self.accessor(view, count, "VEC3", FLOAT, None)?)
        } else {
            None
        };
        let mut indices = Vec::with_capacity(source.indices.len() * 4);
        for index in &source.indices {
            indices.extend_from_slice(&index.to_le_bytes());
        }
        let view = self.view(&indices, ELEMENT_ARRAY_BUFFER);
        let indices = self.accessor(
            view,
            source.indices.len() as u64,
            "SCALAR",
            UNSIGNED_INT,
            None,
        )?;
        let refs = PrimRefs {
            position,
            normal,
            indices,
        };
        self.prims.insert(prim, refs);
        Ok(refs)
    }

    /// BINの末尾にデータを足し、`bufferView` を1つ足す。位置は4バイト境界（データは32ビット値だけ）。
    fn view(&mut self, data: &[u8], target: u64) -> u64 {
        let offset = self.start + self.bin.len() as u64;
        self.bin.extend_from_slice(data);
        let mut view = obj();
        set(&mut view, "buffer", uint(0));
        set(&mut view, "byteOffset", uint(offset));
        set(&mut view, "byteLength", uint(data.len() as u64));
        set(&mut view, "target", uint(target));
        let index = self.first_view + self.views.len() as u64;
        self.views.push(view);
        index
    }

    fn accessor(
        &mut self,
        view: u64,
        count: u64,
        kind: &str,
        component: u64,
        bounds: Option<(&[f32; 3], &[f32; 3])>,
    ) -> Result<u64, ExportError> {
        let mut accessor = obj();
        set(&mut accessor, "bufferView", uint(view));
        set(&mut accessor, "componentType", uint(component));
        set(&mut accessor, "count", uint(count));
        set(&mut accessor, "type", Value::String(kind.to_owned()));
        if let Some((min, max)) = bounds {
            set(&mut accessor, "min", f32s(min)?);
            set(&mut accessor, "max", f32s(max)?);
        }
        let index = self.first_accessor + self.accessors.len() as u64;
        self.accessors.push(accessor);
        Ok(index)
    }
}

/// 読み込みが同じ形状として読み戻せる形状か。読み戻せない物は書かずに断る。
fn check_primitive(prim: &Primitive) -> Result<(), ExportError> {
    if prim.vertices.is_empty() || prim.indices.is_empty() {
        return Err(scene_error("足した形状に頂点か三角形がありません"));
    }
    if u32::try_from(prim.vertices.len()).is_err() || u32::try_from(prim.indices.len()).is_err() {
        return Err(scene_error("足した形状が大きすぎます"));
    }
    if !prim.indices.len().is_multiple_of(3) {
        return Err(scene_error(
            "足した形状のインデックスが3の倍数ではありません",
        ));
    }
    if prim
        .indices
        .iter()
        .any(|index| *index as usize >= prim.vertices.len())
    {
        return Err(scene_error(
            "足した形状のインデックスが頂点の数を超えています",
        ));
    }
    for vertex in &prim.vertices {
        if !vertex.position.iter().all(|value| value.is_finite()) {
            return Err(scene_error("足した形状の位置に有限でない値があります"));
        }
        if prim.has_normals {
            let length = vertex.normal.iter().map(|value| value * value).sum::<f32>();
            if !vertex.normal.iter().all(|value| value.is_finite()) || length <= 0.0 {
                return Err(scene_error("足した形状の法線に使えない値があります"));
            }
        }
    }
    Ok(())
}

fn scene_error(what: &str) -> ExportError {
    ExportError::InvalidScene(what.to_owned())
}
