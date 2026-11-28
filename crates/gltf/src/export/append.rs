//! 形状と圧縮画像を、元の BIN に続く4バイト境界のビューに追記する。

use gltf::json::Value;
use model_reviewer_core::scene::{Primitive, Vertex};
use model_reviewer_core::texture::bake_box;
use std::collections::HashMap;

use super::ExportError;
use super::base::SourceSets;
use super::json::{array, f32s, obj, set, uint};

const FLOAT: u64 = 5126;
const UNSIGNED_INT: u64 = 5125;
const ARRAY_BUFFER: u64 = 34962;
const ELEMENT_ARRAY_BUFFER: u64 = 34963;

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct PrimRefs {
    position: u64,
    normal: Option<u64>,
    /// 組の番号と accessor。元の組を同じ番号で写し、箱形の UV は使っていない番号に置く。
    uvs: Vec<(u32, u64)>,
    indices: u64,
    /// 箱形の UV の組の番号。
    pub box_set: u32,
}

impl PrimRefs {
    pub fn primitive(&self, material: usize) -> Value {
        let mut attributes = obj();
        attributes["POSITION"] = uint(self.position);
        if let Some(normal) = self.normal {
            attributes["NORMAL"] = uint(normal);
        }
        let mut primitive = obj();
        for &(set, uv) in &self.uvs {
            attributes[format!("TEXCOORD_{set}")] = uint(uv);
        }
        if !self.uvs.is_empty() {
            primitive["mode"] = uint(4);
        }
        primitive["attributes"] = attributes;
        primitive["indices"] = uint(self.indices);
        primitive["material"] = material.into();
        primitive
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Geometry {
    Base(u32),
    Added(u32),
    /// 開いた時点のファイルの形状（`meshes[mesh].primitives[slot]`）。
    File {
        mesh: u32,
        slot: u32,
    },
}

/// 焼き込みで写す元の UV の組。`key` は元の形状の `TEXCOORD_n` の accessor（形状番号が同じでも
/// 組が違う形状を分ける）。
pub(super) struct SourceUvs<'a> {
    pub key: Vec<(u32, u64)>,
    pub read: Box<dyn FnOnce() -> Result<SourceSets, ExportError> + 'a>,
}

impl SourceUvs<'_> {
    pub fn none() -> Self {
        Self {
            key: Vec::new(),
            read: Box::new(|| Ok(SourceSets::default())),
        }
    }
}

/// 元の組の次の番号。組が無ければ 0。
pub(super) fn box_set(sets: impl IntoIterator<Item = u32>) -> u32 {
    sets.into_iter().max().map_or(0, |n| n + 1)
}

/// 形状の出どころ、箱形の幅と高さ（ビット）、元の UV の組の accessor。
type PrimKey = (Geometry, Option<[u32; 2]>, Vec<(u32, u64)>);

pub(super) struct Appender {
    start: u64,
    first_accessor: u64,
    first_view: u64,
    pub bin: Vec<u8>,
    pub accessors: Vec<Value>,
    pub views: Vec<Value>,
    prims: HashMap<PrimKey, PrimRefs>,
    pub baked: u32,
    /// 焼き込みで写さなかった、読めない UV の組の数。
    pub dropped_sets: u32,
}

impl Appender {
    pub fn new(start: u64, root: &Value) -> Self {
        Self {
            start,
            first_accessor: array(root, "accessors").len() as u64,
            first_view: array(root, "bufferViews").len() as u64,
            bin: Vec::new(),
            accessors: Vec::new(),
            views: Vec::new(),
            prims: HashMap::new(),
            baked: 0,
            dropped_sets: 0,
        }
    }

    /// `sides` があれば箱形の UV を焼き、`source` の UV の組を分けた頂点へ写す。
    pub fn primitive(
        &mut self,
        origin: Geometry,
        source: &Primitive,
        sides: Option<[f32; 2]>,
        uvs: SourceUvs<'_>,
    ) -> Result<PrimRefs, ExportError> {
        let key = (origin, sides.map(|s| s.map(f32::to_bits)), uvs.key);
        if let Some(refs) = self.prims.get(&key) {
            return Ok(refs.clone());
        }
        check_primitive(source)?;
        let refs = match sides {
            Some(sides) => {
                if !sides.iter().all(|side| side.is_finite() && *side > 0.0) {
                    return Err(scene_error("箱形の大きさが正の有限の数ではありません"));
                }
                let baked = bake_box(source, sides);
                let unique: std::collections::HashSet<_> =
                    key.2.iter().map(|&(_, accessor)| accessor).collect();
                let stride =
                    12 + u64::from(source.has_normals) * 12 + (unique.len() as u64 + 1) * 8;
                let length = (baked.vertices.len() as u64)
                    .saturating_mul(stride)
                    .saturating_add(baked.indices.len() as u64 * 4);
                super::glb_length(
                    0,
                    Some((self.start + self.bin.len() as u64).saturating_add(length)),
                )?;
                let source_sets = (uvs.read)()?;
                self.dropped_sets += source_sets.skipped.len() as u32;
                let mut refs =
                    self.write_geometry(&baked.vertices, &baked.indices, source.has_normals)?;
                let mut written = HashMap::new();
                for (set, values) in &source_sets.sets {
                    // 同じ元配列に対する頂点の振り直しも、一度だけでよい。
                    let accessor = match written.entry(values.as_ptr()) {
                        std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            let pairs = baked
                                .source_indices
                                .iter()
                                .map(|&i| values.get(i as usize).copied());
                            *entry.insert(self.write_uvs(pairs)?)
                        }
                    };
                    refs.uvs.push((*set, accessor));
                }
                refs.box_set = box_set(refs.uvs.iter().map(|(set, _)| *set));
                let uv = self.write_uvs(baked.uvs.into_iter().map(Some))?;
                refs.uvs.push((refs.box_set, uv));
                self.baked += 1;
                refs
            }
            None => self.write_geometry(&source.vertices, &source.indices, source.has_normals)?,
        };
        self.prims.insert(key, refs.clone());
        Ok(refs)
    }

    fn write_geometry(
        &mut self,
        vertices: &[Vertex],
        indices: &[u32],
        has_normals: bool,
    ) -> Result<PrimRefs, ExportError> {
        let count = vertices.len() as u64;
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        let mut positions = Vec::with_capacity(vertices.len() * 12);
        for vertex in vertices {
            for axis in 0..3 {
                min[axis] = min[axis].min(vertex.position[axis]);
                max[axis] = max[axis].max(vertex.position[axis]);
                positions.extend_from_slice(&vertex.position[axis].to_le_bytes());
            }
        }
        let view = self.view(&positions, Some(ARRAY_BUFFER))?;
        let position = self.accessor(view, count, "VEC3", FLOAT, Some((&min, &max)))?;
        let normal = if has_normals {
            let mut normals = Vec::with_capacity(vertices.len() * 12);
            for vertex in vertices {
                for value in vertex.normal {
                    normals.extend_from_slice(&value.to_le_bytes());
                }
            }
            let view = self.view(&normals, Some(ARRAY_BUFFER))?;
            Some(self.accessor(view, count, "VEC3", FLOAT, None)?)
        } else {
            None
        };
        let mut index_bytes = Vec::with_capacity(indices.len() * 4);
        for index in indices {
            index_bytes.extend_from_slice(&index.to_le_bytes());
        }
        let view = self.view(&index_bytes, Some(ELEMENT_ARRAY_BUFFER))?;
        let indices = self.accessor(view, indices.len() as u64, "SCALAR", UNSIGNED_INT, None)?;
        Ok(PrimRefs {
            position,
            normal,
            uvs: Vec::new(),
            indices,
            box_set: 0,
        })
    }

    fn write_uvs(
        &mut self,
        pairs: impl ExactSizeIterator<Item = Option<[f32; 2]>>,
    ) -> Result<u64, ExportError> {
        let count = pairs.len();
        let mut bytes = Vec::with_capacity(count * 8);
        for pair in pairs {
            let pair = pair.ok_or_else(|| scene_error("元の UV の組の数が頂点の数と違います"))?;
            for coord in pair {
                if !coord.is_finite() {
                    return Err(scene_error("箱形の投影で有限でない UV ができました"));
                }
                bytes.extend_from_slice(&coord.to_le_bytes());
            }
        }
        let view = self.view(&bytes, Some(ARRAY_BUFFER))?;
        self.accessor(view, count as u64, "VEC2", FLOAT, None)
    }

    /// byteLength は中身だけを数え、次のビューの先頭を4バイト境界に置く。
    pub fn view(&mut self, data: &[u8], target: Option<u64>) -> Result<u64, ExportError> {
        let offset = self.start + self.bin.len() as u64;
        let padded = super::container::align4(data.len() as u64);
        super::glb_length(0, Some(offset + padded))?;
        let end = self.bin.len() + padded as usize;
        self.bin.extend_from_slice(data);
        self.bin.resize(end, 0);
        let mut view = obj();
        set(&mut view, "buffer", uint(0));
        set(&mut view, "byteOffset", uint(offset));
        set(&mut view, "byteLength", uint(data.len() as u64));
        if let Some(target) = target {
            set(&mut view, "target", uint(target));
        }
        let index = self.first_view + self.views.len() as u64;
        self.views.push(view);
        Ok(index)
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

#[cfg(test)]
mod tests {
    use super::*;
    use model_reviewer_core::shapes::{ShapeKind, build};

    #[test]
    fn preflight_precedes_uv_read() {
        let source = build(ShapeKind::Cube, 1.0);
        let baked = bake_box(&source, [1.0; 2]);
        // 位置・法線・箱形の組と元の1組なら収まる。元の2組目を含めて初めて上限を超える。
        let one_set = (baked.vertices.len() * (12 + 12 + 8 + 8) + baked.indices.len() * 4) as u64;
        let available = crate::export::glb_length(0, Some(one_set)).unwrap();
        let mut app = Appender::new(u64::from(u32::MAX) - 3 - available, &obj());
        let result = app.primitive(
            Geometry::Base(0),
            &source,
            Some([1.0; 2]),
            SourceUvs {
                key: vec![(0, 2), (1, 3)],
                read: Box::new(|| panic!("全体が入らないときは UV を展開しない")),
            },
        );
        assert!(matches!(result, Err(ExportError::TooLarge { .. })));
        assert!(app.bin.is_empty());
    }
}
