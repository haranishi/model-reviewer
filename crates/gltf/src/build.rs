//! BIN を自分で読み、共有と階層を組み立てる。gltf のリーダーは使わない。

use std::collections::{HashMap, HashSet};

use glam::{Affine3A, Mat4, Quat, Vec3};
use gltf::json::accessor::{ComponentType, GenericComponentType, Type};
use gltf::json::material::AlphaMode;
use gltf::json::mesh::{Mode, Semantic};
use gltf::json::validation::Checked;
use gltf::json::{Accessor, Root};
use model_reviewer_core::math::Aabb;
use model_reviewer_core::scene::{
    Draw, LoadWarning, Model, ModelStats, Node, NodeId, PrimId, Primitive, Vertex,
};

use crate::error::LoadError;
use crate::inspect::{self, Opened};

pub fn assemble(opened: Opened<'_>) -> Result<Model, LoadError> {
    assemble_with_sources(opened).map(|assembled| assembled.model)
}

/// 組み立ての内部の結果。`sources[i]` は `model.draws[i]` が、どのメッシュの何番目の
/// プリミティブから来たか（ファイルの番号）。点・線・読めないプリミティブは描画にならないので、
/// 描画の番号とプリミティブの番号は一致しない。書き出しはこの対応をそのまま使い、数え直さない。
pub(crate) struct Assembled {
    pub model: Model,
    pub sources: Vec<DrawSource>,
}

/// 描画の出どころ（メッシュの番号、その中のプリミティブの番号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DrawSource {
    pub mesh: u32,
    pub primitive: u32,
}

fn assemble_with_sources(opened: Opened<'_>) -> Result<Assembled, LoadError> {
    let root = opened.document.as_json();
    let mut assembly = Assembly::new(root, opened.bin);
    let mut warnings = assembly.file_warnings()?;
    let mut nodes = Vec::new();
    let mut open = Vec::new();
    let mut emitted = vec![false; root.nodes.len()];
    let mut stack = Vec::new();
    for index in inspect::scene_roots(root)?.into_iter().rev() {
        stack.push(Frame {
            index,
            parent_world: Affine3A::IDENTITY,
            parent: None,
            depth: 0,
        });
    }
    while let Some(frame) = stack.pop() {
        if frame.index >= root.nodes.len()
            || emitted[frame.index]
            || nodes.len() >= root.nodes.len()
        {
            return Err(LoadError::NodeGraph("循環".to_owned()));
        }
        emitted[frame.index] = true;
        close_open(&mut nodes, &mut open, frame.depth)?;
        let source = &root.nodes[frame.index];
        let local = local_transform(source)?;
        let world = frame.parent_world * local;
        if !affine_finite(world) {
            return Err(bad("変換"));
        }
        let gltf_index = inspect::fit_u32(frame.index as u64, "インスタンス数")?;
        let id = inspect::fit_u32(nodes.len() as u64, "インスタンス数")?;
        let draw_start = inspect::fit_u32(assembly.draws.len() as u64, "インスタンス数")?;
        let world_aabb = match source.mesh.as_ref().map(gltf::json::Index::value) {
            Some(mesh) => assembly.add_mesh(mesh, &world)?,
            None => Aabb::empty(),
        };
        let draw_end = inspect::fit_u32(assembly.draws.len() as u64, "インスタンス数")?;
        nodes.push(Node {
            name: node_name(source, gltf_index),
            gltf_index,
            parent: frame.parent,
            subtree_end: 0,
            depth: frame.depth,
            local,
            world,
            draws: draw_start..draw_end,
            world_aabb,
        });
        open.push(nodes.len() - 1);
        let mut children = Vec::new();
        if let Some(list) = &source.children {
            for child in list {
                children.push(child.value());
            }
        }
        let depth = frame
            .depth
            .checked_add(1)
            .ok_or_else(|| LoadError::TooLarge {
                what: "インスタンス数".to_owned(),
            })?;
        for child in children.into_iter().rev() {
            stack.push(Frame {
                index: child,
                parent_world: world,
                parent: Some(NodeId(id)),
                depth,
            });
        }
    }
    close_open(&mut nodes, &mut open, 0)?;
    if assembly.draws.is_empty() {
        return Err(LoadError::NoMesh);
    }
    let mut bounds = Aabb::empty();
    for node in &nodes {
        bounds = bounds.union(node.world_aabb);
    }
    assembly.push_runtime_warnings(&mut warnings)?;
    for name in &opened.simplified {
        warnings.push(LoadWarning::SimplifiedExtension { name: name.clone() });
    }
    for name in &opened.unknown_optional {
        warnings.push(LoadWarning::UnsupportedOptionalExtension { name: name.clone() });
    }
    let model = Model {
        stats: ModelStats {
            nodes: inspect::fit_u32(nodes.len() as u64, "インスタンス数")?,
            meshes: inspect::fit_u32(assembly.meshes.len() as u64, "インスタンス数")?,
            primitives: inspect::fit_u32(assembly.primitives.len() as u64, "インスタンス数")?,
            unique_triangles: assembly.unique_triangles,
            placed_triangles: assembly.placed_triangles,
        },
        nodes,
        draws: assembly.draws,
        primitives: assembly.primitives,
        bounds,
        warnings,
    };
    Ok(Assembled {
        model,
        sources: assembly.sources,
    })
}

/// 書き出しの土台を組み立てる入口。ノードが0個のファイルで `NoMesh` になったときだけ、
/// 空のモデルを返す（新規の文書の土台 `export::empty_base`）。ほかは `assemble` と同じ。
/// `load_glb` はこれを使わない（メッシュの無いファイルを開く操作は、今までどおり断る）。
pub(crate) fn assemble_base(opened: Opened<'_>) -> Result<Assembled, LoadError> {
    let no_nodes = opened.document.as_json().nodes.is_empty();
    match assemble_with_sources(opened) {
        Err(LoadError::NoMesh) if no_nodes => Ok(Assembled {
            model: Model::empty(),
            sources: Vec::new(),
        }),
        other => other,
    }
}

struct Frame {
    index: usize,
    parent_world: Affine3A,
    parent: Option<NodeId>,
    depth: u32,
}

fn close_open(nodes: &mut [Node], open: &mut Vec<usize>, depth: u32) -> Result<(), LoadError> {
    while let Some(&index) = open.last() {
        if nodes[index].depth < depth {
            break;
        }
        open.pop();
        nodes[index].subtree_end = inspect::fit_u32(nodes.len() as u64, "インスタンス数")?;
    }
    Ok(())
}

struct Assembly<'a> {
    root: &'a Root,
    bin: &'a [u8],
    primitives: Vec<Primitive>,
    draws: Vec<Draw>,
    /// `draws` と同じ並びの、描画の出どころ。
    sources: Vec<DrawSource>,
    cache: HashMap<ShareKey, PrimId>,
    meshes: HashSet<usize>,
    unique_triangles: u64,
    placed_triangles: u64,
    derived: u64,
    lines: u64,
    points: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ShareKey {
    position: u32,
    normal: Option<u32>,
    indices: Option<u32>,
    mode: u8,
}

#[derive(Clone, Copy)]
struct PrimInfo {
    position: usize,
    normal: Option<usize>,
    indices: Option<usize>,
    material: Option<usize>,
    mode: Mode,
}

impl<'a> Assembly<'a> {
    fn new(root: &'a Root, bin: &'a [u8]) -> Self {
        Self {
            root,
            bin,
            primitives: Vec::new(),
            draws: Vec::new(),
            sources: Vec::new(),
            cache: HashMap::new(),
            meshes: HashSet::new(),
            unique_triangles: 0,
            placed_triangles: 0,
            derived: 0,
            lines: 0,
            points: 0,
        }
    }

    fn file_warnings(&self) -> Result<Vec<LoadWarning>, LoadError> {
        let mut warnings = Vec::new();
        push_count(&mut warnings, self.root.textures.len(), |count| {
            LoadWarning::Textures { count }
        })?;
        let mut colors = 0usize;
        let mut morphs = 0usize;
        for mesh in &self.root.meshes {
            for primitive in &mesh.primitives {
                if primitive
                    .attributes
                    .keys()
                    .any(|key| matches!(key.as_ref(), Checked::Valid(Semantic::Colors(_))))
                {
                    colors += 1;
                }
                if primitive
                    .targets
                    .as_ref()
                    .is_some_and(|targets| !targets.is_empty())
                {
                    morphs += 1;
                }
            }
        }
        push_count(&mut warnings, colors, |count| LoadWarning::VertexColors {
            count,
        })?;
        let blends = self
            .root
            .materials
            .iter()
            .filter(|material| {
                matches!(
                    material.alpha_mode.as_ref(),
                    Checked::Valid(AlphaMode::Blend)
                )
            })
            .count();
        push_count(&mut warnings, blends, |count| LoadWarning::AlphaBlend {
            count,
        })?;
        push_count(&mut warnings, self.root.animations.len(), |count| {
            LoadWarning::Animations { count }
        })?;
        push_count(&mut warnings, self.root.skins.len(), |count| {
            LoadWarning::Skins { count }
        })?;
        push_count(&mut warnings, morphs, |count| LoadWarning::Morphs { count })?;
        Ok(warnings)
    }

    fn push_runtime_warnings(&self, warnings: &mut Vec<LoadWarning>) -> Result<(), LoadError> {
        if self.derived > 0 {
            warnings.push(LoadWarning::DerivedNormals {
                count: inspect::fit_u32(self.derived, "インスタンス数")?,
            });
        }
        if self.lines > 0 || self.points > 0 {
            warnings.push(LoadWarning::OmittedLinesAndPoints {
                lines: inspect::fit_u32(self.lines, "インスタンス数")?,
                points: inspect::fit_u32(self.points, "インスタンス数")?,
            });
        }
        Ok(())
    }

    fn add_mesh(&mut self, mesh_index: usize, world: &Affine3A) -> Result<Aabb, LoadError> {
        let mesh = self
            .root
            .meshes
            .get(mesh_index)
            .ok_or_else(|| bad("メッシュ"))?;
        let infos = mesh
            .primitives
            .iter()
            .map(prim_info)
            .collect::<Result<Vec<_>, _>>()?;
        let mut aabb = Aabb::empty();
        for (primitive, info) in infos.into_iter().enumerate() {
            let source = DrawSource {
                mesh: u32_index(mesh_index)?,
                primitive: u32_index(primitive)?,
            };
            if let Some(local) = self.add_primitive(mesh_index, source, info)? {
                aabb = aabb.union(local.transformed(world));
            }
        }
        Ok(aabb)
    }

    fn add_primitive(
        &mut self,
        mesh_index: usize,
        source: DrawSource,
        info: PrimInfo,
    ) -> Result<Option<Aabb>, LoadError> {
        match info.mode {
            Mode::Points => {
                self.points = bump(self.points)?;
                return Ok(None);
            }
            Mode::Lines | Mode::LineLoop | Mode::LineStrip => {
                self.lines = bump(self.lines)?;
                return Ok(None);
            }
            Mode::Triangles | Mode::TriangleStrip | Mode::TriangleFan => {}
        }
        let key = ShareKey {
            position: u32_index(info.position)?,
            normal: info.normal.map(u32_index).transpose()?,
            indices: info.indices.map(u32_index).transpose()?,
            mode: mode_tag(info.mode),
        };
        let color = base_color(self.root, info.material)?;
        if let Some(prim) = self.cache.get(&key).copied() {
            let stored = &self.primitives[prim.0 as usize];
            let triangles = triangle_len(stored.indices.len())?;
            let local = stored.local_aabb;
            let has_normals = stored.has_normals;
            self.place(mesh_index, source, prim, color, has_normals, triangles)?;
            return Ok(Some(local));
        }
        let Some(built) = decode(self.root, self.bin, info)? else {
            return Ok(None);
        };
        let prim = PrimId(inspect::fit_u32(
            self.primitives.len() as u64,
            "インスタンス数",
        )?);
        let local = built.primitive.local_aabb;
        let has_normals = built.primitive.has_normals;
        self.unique_triangles = self
            .unique_triangles
            .checked_add(built.triangles)
            .ok_or_else(|| LoadError::TooLarge {
                what: "頂点数".to_owned(),
            })?;
        self.primitives.push(built.primitive);
        self.cache.insert(key, prim);
        self.place(
            mesh_index,
            source,
            prim,
            color,
            has_normals,
            built.triangles,
        )?;
        Ok(Some(local))
    }

    fn place(
        &mut self,
        mesh_index: usize,
        source: DrawSource,
        prim: PrimId,
        color: [f32; 4],
        has_normals: bool,
        triangles: u64,
    ) -> Result<(), LoadError> {
        let len = self.draws.len();
        if len >= u32::MAX as usize {
            return Err(LoadError::TooLarge {
                what: "インスタンス数".to_owned(),
            });
        }
        self.draws.push(Draw {
            prim,
            base_color: color,
        });
        self.sources.push(source);
        self.meshes.insert(mesh_index);
        self.placed_triangles = self
            .placed_triangles
            .checked_add(triangles)
            .ok_or_else(|| LoadError::TooLarge {
                what: "頂点数".to_owned(),
            })?;
        if !has_normals {
            self.derived = bump(self.derived)?;
        }
        Ok(())
    }
}

fn push_count(
    warnings: &mut Vec<LoadWarning>,
    count: usize,
    make: impl FnOnce(u32) -> LoadWarning,
) -> Result<(), LoadError> {
    if count == 0 {
        return Ok(());
    }
    warnings.push(make(inspect::fit_u32(count as u64, "インスタンス数")?));
    Ok(())
}

fn bump(value: u64) -> Result<u64, LoadError> {
    value.checked_add(1).ok_or_else(|| LoadError::TooLarge {
        what: "インスタンス数".to_owned(),
    })
}

fn prim_info(primitive: &gltf::json::mesh::Primitive) -> Result<PrimInfo, LoadError> {
    let mode = match primitive.mode.as_ref() {
        Checked::Valid(mode) => *mode,
        Checked::Invalid => return Err(bad("プリミティブ")),
    };
    let position = find_semantic(primitive, Semantic::Positions).ok_or_else(|| bad("POSITION"))?;
    Ok(PrimInfo {
        position,
        normal: find_semantic(primitive, Semantic::Normals),
        indices: primitive.indices.as_ref().map(gltf::json::Index::value),
        material: primitive.material.as_ref().map(gltf::json::Index::value),
        mode,
    })
}

fn find_semantic(primitive: &gltf::json::mesh::Primitive, want: Semantic) -> Option<usize> {
    primitive.attributes.iter().find_map(|(semantic, index)| {
        let found = match semantic.as_ref() {
            Checked::Valid(found) => found,
            Checked::Invalid => return None,
        };
        (found == &want).then_some(index.value())
    })
}

fn mode_tag(mode: Mode) -> u8 {
    match mode {
        Mode::Points => 0,
        Mode::Lines => 1,
        Mode::LineLoop => 2,
        Mode::LineStrip => 3,
        Mode::Triangles => 4,
        Mode::TriangleStrip => 5,
        Mode::TriangleFan => 6,
    }
}

fn u32_index(index: usize) -> Result<u32, LoadError> {
    u32::try_from(index).map_err(|_| bad("アクセサ"))
}

fn base_color(root: &Root, material: Option<usize>) -> Result<[f32; 4], LoadError> {
    let Some(index) = material else {
        return Ok([1.0, 1.0, 1.0, 1.0]);
    };
    let material = root.materials.get(index).ok_or_else(|| bad("マテリアル"))?;
    Ok(material.pbr_metallic_roughness.base_color_factor.0)
}

struct Built {
    primitive: Primitive,
    triangles: u64,
}

fn decode(root: &Root, bin: &[u8], info: PrimInfo) -> Result<Option<Built>, LoadError> {
    let positions = read_vec3(root, bin, info.position, true)?;
    let (normals, has_normals) = match info.normal {
        Some(index) => read_normals(root, bin, index, positions.len())?,
        None => (vec![[0.0; 3]; positions.len()], false),
    };
    let source = match info.indices {
        Some(index) => read_indices(root, bin, index, positions.len())?,
        None => (0..u32_index(positions.len())?).collect(),
    };
    let indices = triangulate(&source, info.mode, bin.len())?;
    if indices.is_empty() {
        return Ok(None);
    }
    let triangles = triangle_len(indices.len())?;
    let mut local_aabb = Aabb::empty();
    let mut vertices = Vec::new();
    guard_len(positions.len(), std::mem::size_of::<Vertex>(), bin.len())?;
    vertices.reserve(positions.len());
    for (position, normal) in positions.iter().zip(normals) {
        let point = Vec3::from(*position);
        local_aabb = local_aabb.union(Aabb {
            min: point,
            max: point,
        });
        vertices.push(Vertex {
            position: *position,
            normal,
        });
    }
    Ok(Some(Built {
        primitive: Primitive {
            vertices,
            indices,
            local_aabb,
            has_normals,
        },
        triangles,
    }))
}

fn triangulate(source: &[u32], mode: Mode, bin_len: usize) -> Result<Vec<u32>, LoadError> {
    let out_len = match mode {
        Mode::Triangles => {
            if !source.len().is_multiple_of(3) {
                return Err(bad("インデックス"));
            }
            source.len()
        }
        Mode::TriangleStrip | Mode::TriangleFan => {
            if source.len() < 3 {
                0
            } else {
                source
                    .len()
                    .checked_sub(2)
                    .and_then(|tris| tris.checked_mul(3))
                    .ok_or_else(|| LoadError::TooLarge {
                        what: "頂点数".to_owned(),
                    })?
            }
        }
        Mode::Points | Mode::Lines | Mode::LineLoop | Mode::LineStrip => 0,
    };
    guard_len(out_len, std::mem::size_of::<u32>(), bin_len)?;
    let mut out = Vec::with_capacity(out_len);
    match mode {
        Mode::Triangles => out.extend_from_slice(source),
        Mode::TriangleStrip => {
            for index in 0..source.len().saturating_sub(2) {
                if index % 2 == 0 {
                    out.extend_from_slice(&[source[index], source[index + 1], source[index + 2]]);
                } else {
                    out.extend_from_slice(&[source[index + 1], source[index], source[index + 2]]);
                }
            }
        }
        Mode::TriangleFan => {
            for index in 1..source.len().saturating_sub(1) {
                out.extend_from_slice(&[source[0], source[index], source[index + 1]]);
            }
        }
        Mode::Points | Mode::Lines | Mode::LineLoop | Mode::LineStrip => {}
    }
    Ok(out)
}

fn triangle_len(index_len: usize) -> Result<u64, LoadError> {
    if !index_len.is_multiple_of(3) {
        return Err(bad("インデックス"));
    }
    Ok((index_len / 3) as u64)
}

fn read_vec3(
    root: &Root,
    bin: &[u8],
    index: usize,
    positions: bool,
) -> Result<Vec<[f32; 3]>, LoadError> {
    let accessor = accessor(root, index)?;
    expect_f32_vec3(accessor)?;
    let (start, stride, count) = layout(root, accessor)?;
    let count_us = usize::try_from(count).map_err(|_| LoadError::TooLarge {
        what: "頂点数".to_owned(),
    })?;
    if count_us > bin.len() / 12 {
        return Err(bad("範囲"));
    }
    guard_len(count_us, 12, bin.len())?;
    let mut values = Vec::with_capacity(count_us);
    for item in 0..count {
        let offset = element_offset(start, stride, item)?;
        let x = read_f32(bin, offset)?;
        let y = read_f32(bin, offset.checked_add(4).ok_or_else(|| bad("範囲"))?)?;
        let z = read_f32(bin, offset.checked_add(8).ok_or_else(|| bad("範囲"))?)?;
        if positions && !(x.is_finite() && y.is_finite() && z.is_finite()) {
            return Err(bad("位置"));
        }
        values.push([x, y, z]);
    }
    Ok(values)
}

fn read_normals(
    root: &Root,
    bin: &[u8],
    index: usize,
    vertices: usize,
) -> Result<(Vec<[f32; 3]>, bool), LoadError> {
    let values = read_vec3(root, bin, index, false)?;
    if values.len() != vertices {
        return Err(bad("法線"));
    }
    let usable = values.iter().all(|normal| {
        normal.iter().all(|component| component.is_finite()) && normal_length_squared(normal) > 0.0
    });
    if usable {
        return Ok((values, true));
    }
    Ok((vec![[0.0; 3]; values.len()], false))
}

fn normal_length_squared(normal: &[f32; 3]) -> f32 {
    normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]
}

fn read_indices(
    root: &Root,
    bin: &[u8],
    index: usize,
    vertices: usize,
) -> Result<Vec<u32>, LoadError> {
    let accessor = accessor(root, index)?;
    let width = index_width(accessor)?;
    let (start, stride, count) = layout(root, accessor)?;
    let count_us = usize::try_from(count).map_err(|_| LoadError::TooLarge {
        what: "頂点数".to_owned(),
    })?;
    guard_len(count_us, std::mem::size_of::<u32>(), bin.len())?;
    let mut values = Vec::with_capacity(count_us);
    for item in 0..count {
        let offset = element_offset(start, stride, item)?;
        let value = read_index(bin, offset, width)?;
        if value as usize >= vertices {
            return Err(bad("インデックス"));
        }
        values.push(value);
    }
    Ok(values)
}

fn index_width(accessor: &Accessor) -> Result<u64, LoadError> {
    let component = component_type(accessor)?;
    let kind = accessor_type(accessor)?;
    if !matches!(kind, Type::Scalar) {
        return Err(bad("アクセサの型"));
    }
    match component {
        ComponentType::U8 => Ok(1),
        ComponentType::U16 => Ok(2),
        ComponentType::U32 => Ok(4),
        ComponentType::I8 | ComponentType::I16 | ComponentType::F32 => Err(bad("アクセサの型")),
    }
}

fn expect_f32_vec3(accessor: &Accessor) -> Result<(), LoadError> {
    if matches!(component_type(accessor)?, ComponentType::F32)
        && matches!(accessor_type(accessor)?, Type::Vec3)
    {
        Ok(())
    } else {
        Err(bad("アクセサの型"))
    }
}

fn component_type(accessor: &Accessor) -> Result<ComponentType, LoadError> {
    match accessor.component_type.as_ref() {
        Checked::Valid(GenericComponentType(component)) => Ok(*component),
        Checked::Invalid => Err(bad("アクセサの型")),
    }
}

fn accessor_type(accessor: &Accessor) -> Result<Type, LoadError> {
    match accessor.type_.as_ref() {
        Checked::Valid(kind) => Ok(*kind),
        Checked::Invalid => Err(bad("アクセサの型")),
    }
}

fn accessor(root: &Root, index: usize) -> Result<&Accessor, LoadError> {
    root.accessors.get(index).ok_or_else(|| bad("アクセサ"))
}

fn layout(root: &Root, accessor: &Accessor) -> Result<(u64, u64, u32), LoadError> {
    if accessor.count.0 > u64::from(u32::MAX) {
        return Err(LoadError::TooLarge {
            what: "頂点数".to_owned(),
        });
    }
    if accessor.count.0 == 0 {
        return Err(bad("個数"));
    }
    let count = u32::try_from(accessor.count.0).map_err(|_| LoadError::TooLarge {
        what: "頂点数".to_owned(),
    })?;
    let view_index = accessor.buffer_view.ok_or_else(|| bad("bufferView"))?;
    let view = root
        .buffer_views
        .get(view_index.value())
        .ok_or_else(|| bad("bufferView"))?;
    let element = element_size(accessor)?;
    let stride = match view.byte_stride {
        Some(gltf::json::buffer::Stride(0)) | None => element,
        Some(gltf::json::buffer::Stride(stride)) => {
            let stride = u64::try_from(stride).map_err(|_| bad("stride"))?;
            if stride < element {
                return Err(bad("stride"));
            }
            stride
        }
    };
    let start = view
        .byte_offset
        .map(|value| value.0)
        .unwrap_or(0)
        .checked_add(accessor.byte_offset.map(|value| value.0).unwrap_or(0))
        .ok_or_else(|| bad("範囲"))?;
    Ok((start, stride, count))
}

fn element_size(accessor: &Accessor) -> Result<u64, LoadError> {
    let component: u64 = match component_type(accessor)? {
        ComponentType::I8 | ComponentType::U8 => 1,
        ComponentType::I16 | ComponentType::U16 => 2,
        ComponentType::U32 | ComponentType::F32 => 4,
    };
    let kind: u64 = match accessor_type(accessor)? {
        Type::Scalar => 1,
        Type::Vec2 => 2,
        Type::Vec3 => 3,
        Type::Vec4 | Type::Mat2 => 4,
        Type::Mat3 => 9,
        Type::Mat4 => 16,
    };
    component
        .checked_mul(kind)
        .ok_or_else(|| bad("アクセサの型"))
}

fn element_offset(start: u64, stride: u64, index: u32) -> Result<u64, LoadError> {
    stride
        .checked_mul(u64::from(index))
        .and_then(|offset| start.checked_add(offset))
        .ok_or_else(|| bad("範囲"))
}

fn read_f32(bin: &[u8], offset: u64) -> Result<f32, LoadError> {
    let bytes = read_bytes(bin, offset, 4)?;
    let mut buf = [0u8; 4];
    buf.copy_from_slice(bytes);
    Ok(f32::from_le_bytes(buf))
}

fn read_index(bin: &[u8], offset: u64, width: u64) -> Result<u32, LoadError> {
    let bytes = read_bytes(bin, offset, width)?;
    match width {
        1 => Ok(u32::from(bytes[0])),
        2 => {
            let mut buf = [0u8; 2];
            buf.copy_from_slice(bytes);
            Ok(u32::from(u16::from_le_bytes(buf)))
        }
        4 => {
            let mut buf = [0u8; 4];
            buf.copy_from_slice(bytes);
            Ok(u32::from_le_bytes(buf))
        }
        _ => Err(bad("アクセサの型")),
    }
}

fn read_bytes(bin: &[u8], offset: u64, width: u64) -> Result<&[u8], LoadError> {
    let start = usize::try_from(offset).map_err(|_| bad("範囲"))?;
    let width = usize::try_from(width).map_err(|_| bad("範囲"))?;
    let end = start.checked_add(width).ok_or_else(|| bad("範囲"))?;
    bin.get(start..end).ok_or_else(|| bad("範囲"))
}

fn guard_len(elements: usize, elem_size: usize, bin_len: usize) -> Result<(), LoadError> {
    if elem_size == 0 || elements > (isize::MAX as usize) / elem_size {
        return Err(LoadError::TooLarge {
            what: "頂点数".to_owned(),
        });
    }
    if elements > bin_len.saturating_mul(3) {
        return Err(LoadError::TooLarge {
            what: "頂点数".to_owned(),
        });
    }
    Ok(())
}

/// ファイルの値から親に対する行列を作る。書き出しが「変えていない部品」をビット単位で見分ける
/// ために、同じ関数を使う（`crate::export`）。
pub(crate) fn local_transform(node: &gltf::json::Node) -> Result<Affine3A, LoadError> {
    if let Some(matrix) = node.matrix {
        if matrix.iter().any(|value| !value.is_finite()) {
            return Err(bad("変換"));
        }
        let affine = Affine3A::from_mat4(Mat4::from_cols_array(&matrix));
        if !affine_finite(affine) {
            return Err(bad("変換"));
        }
        return Ok(affine);
    }
    let translation = node.translation.unwrap_or([0.0, 0.0, 0.0]);
    let rotation = node
        .rotation
        .map(|value| value.0)
        .unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let scale = node.scale.unwrap_or([1.0, 1.0, 1.0]);
    if !finite3(translation) || !finite4(rotation) || !finite3(scale) {
        return Err(bad("変換"));
    }
    Ok(Affine3A::from_scale_rotation_translation(
        Vec3::from(scale),
        Quat::from_xyzw(rotation[0], rotation[1], rotation[2], rotation[3]),
        Vec3::from(translation),
    ))
}

fn affine_finite(world: Affine3A) -> bool {
    finite_vec3(world.matrix3.x_axis.into())
        && finite_vec3(world.matrix3.y_axis.into())
        && finite_vec3(world.matrix3.z_axis.into())
        && finite_vec3(world.translation.into())
}

fn finite_vec3(value: Vec3) -> bool {
    value.x.is_finite() && value.y.is_finite() && value.z.is_finite()
}

fn finite3(value: [f32; 3]) -> bool {
    value.iter().all(|item| item.is_finite())
}

fn finite4(value: [f32; 4]) -> bool {
    value.iter().all(|item| item.is_finite())
}

/// 名前の無いノードに読み込みが付ける名前。書き出しの照合も同じ規則を使う（`crate::export`）。
pub(crate) fn node_name(node: &gltf::json::Node, index: u32) -> String {
    match node.name.as_deref() {
        Some(name) if !name.is_empty() => name.to_owned(),
        _ => format!("ノード {index}"),
    }
}

fn bad(what: &str) -> LoadError {
    LoadError::BadData {
        what: what.to_owned(),
    }
}
