//! 試験用に手で組む小さなモデルと、GPU・PNGの下回り。
#![allow(dead_code)]

use std::future::Future;
use std::ops::Range;
use std::path::PathBuf;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use std::sync::Arc;

use glam::{Affine3A, Quat, Vec3};
use model_reviewer_core::math::Aabb;
use model_reviewer_core::scene::{
    Draw, Model, ModelStats, Node, NodeId, PrimId, Primitive, SceneRead, Vertex,
};
use model_reviewer_core::texture::Textures;

pub fn cube(has_normals: bool) -> Primitive {
    let faces: [(Vec3, Vec3, Vec3); 6] = [
        (Vec3::X, Vec3::Y, Vec3::Z),
        (Vec3::NEG_X, Vec3::Y, Vec3::NEG_Z),
        (Vec3::Y, Vec3::Z, Vec3::X),
        (Vec3::NEG_Y, Vec3::NEG_Z, Vec3::X),
        (Vec3::Z, Vec3::Y, Vec3::NEG_X),
        (Vec3::NEG_Z, Vec3::Y, Vec3::X),
    ];
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (n, u, v) in faces {
        let base = vertices.len() as u32;
        for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = (n + u * a + v * b) * 0.5;
            let normal = if has_normals { n.to_array() } else { [0.0; 3] };
            vertices.push(Vertex {
                position: p.to_array(),
                normal,
            });
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Primitive {
        vertices,
        indices,
        local_aabb: Aabb {
            min: Vec3::splat(-0.5),
            max: Vec3::splat(0.5),
        },
        has_normals,
    }
}

/// 法線を持たない四面体（面法線はシェーダーが導く）。
pub fn tetra() -> Primitive {
    let p = [
        [0.0, 0.6, 0.0],
        [-0.5, -0.4, 0.4],
        [0.5, -0.4, 0.4],
        [0.0, -0.4, -0.5],
    ];
    Primitive {
        vertices: p
            .iter()
            .map(|position| Vertex {
                position: *position,
                normal: [0.0; 3],
            })
            .collect(),
        indices: vec![0, 1, 2, 0, 2, 3, 0, 3, 1, 1, 3, 2],
        local_aabb: Aabb {
            min: Vec3::new(-0.5, -0.4, -0.5),
            max: Vec3::new(0.5, 0.6, 0.4),
        },
        has_normals: false,
    }
}

pub struct NodeSpec {
    pub name: &'static str,
    pub parent: Option<u32>,
    pub world: Affine3A,
    pub draws: Vec<(u32, [f32; 4])>,
}

/// ノードは行きがけ順で渡す。`subtree_end` は親の指定から求める。
pub fn model(primitives: Vec<Primitive>, specs: Vec<NodeSpec>) -> Model {
    let count = specs.len();
    let mut depth = vec![0u32; count];
    for (i, spec) in specs.iter().enumerate() {
        if let Some(parent) = spec.parent {
            depth[i] = depth[parent as usize] + 1;
        }
    }
    let mut nodes = Vec::new();
    let mut draws = Vec::new();
    let mut bounds = Aabb::empty();
    for (i, spec) in specs.iter().enumerate() {
        let mut end = i + 1;
        while end < count && depth[end] > depth[i] {
            end += 1;
        }
        let start = draws.len() as u32;
        let mut world_aabb = Aabb::empty();
        for (prim, color) in &spec.draws {
            draws.push(Draw {
                prim: PrimId(*prim),
                base_color: *color,
                tex: None,
            });
            if let Some(p) = primitives.get(*prim as usize) {
                world_aabb = world_aabb.union(p.local_aabb.transformed(&spec.world));
            }
        }
        bounds = bounds.union(world_aabb);
        nodes.push(Node {
            name: spec.name.to_owned(),
            gltf_index: i as u32,
            parent: spec.parent.map(NodeId),
            subtree_end: end as u32,
            depth: depth[i],
            local: spec.world,
            world: spec.world,
            draws: start..draws.len() as u32,
            world_aabb,
        });
    }
    let stats = ModelStats {
        nodes: count as u32,
        meshes: primitives.len() as u32,
        primitives: primitives.len() as u32,
        unique_triangles: primitives.iter().map(|p| p.indices.len() as u64 / 3).sum(),
        placed_triangles: 0,
    };
    Model {
        textures: Textures::default(),
        nodes,
        draws,
        primitives,
        bounds,
        stats,
        warnings: Vec::new(),
    }
}

pub fn node(
    name: &'static str,
    parent: Option<u32>,
    world: Affine3A,
    draws: Vec<(u32, [f32; 4])>,
) -> NodeSpec {
    NodeSpec {
        name,
        parent,
        world,
        draws,
    }
}

pub fn trs(scale: Vec3, rotation: Quat, translation: Vec3) -> Affine3A {
    Affine3A::from_scale_rotation_translation(scale, rotation, translation)
}

pub fn range(r: Range<u32>) -> Range<u32> {
    r
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
        std::thread::yield_now();
    }
}

pub fn device() -> (wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("GPUのアダプタが見つからない");
    block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .expect("デバイスを作れない")
}

/// 線の表示（`POLYGON_MODE_LINE`）をアダプタが持っていれば要求する装置。持っていなければ要求しない。
pub fn device_with_lines() -> (wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .expect("GPUのアダプタが見つからない");
    let features = adapter.features() & wgpu::Features::POLYGON_MODE_LINE;
    block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: features,
        ..Default::default()
    }))
    .expect("デバイスを作れない")
}

pub fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("render-gpu");
    std::fs::create_dir_all(&dir).expect("出力先を作れない");
    dir
}

/// 圧縮しないPNG（zlib の無圧縮ブロック）。目で確かめるための保存用。
pub fn write_png(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) {
    let mut raw = Vec::with_capacity(rgba.len() + height as usize);
    for row in rgba.chunks(width as usize * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut zlib = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65_535).peekable();
    if chunks.peek().is_none() {
        zlib.extend([1, 0, 0, 0xff, 0xff]);
    }
    while let Some(block) = chunks.next() {
        let last = chunks.peek().is_none();
        let len = block.len() as u16;
        zlib.push(u8::from(last));
        zlib.extend(len.to_le_bytes());
        zlib.extend((!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for byte in &raw {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    zlib.extend(((b << 16) | a).to_be_bytes());

    let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::new();
    ihdr.extend(width.to_be_bytes());
    ihdr.extend(height.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &zlib);
    chunk(&mut png, b"IEND", &[]);
    std::fs::write(path, png).expect("PNGを書けない");
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend(crc32(&out[start..]).to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// 編集中の文書の代わり。GPUへ送った形状は `base` のものを使い、部品の表（`view`）と
/// 足した形状（`added`）だけを持つ。`shares_geometry_with` は `base` と同じときだけ真。
pub struct EditedScene {
    pub base: Arc<Model>,
    pub view: Model,
    pub added: Vec<Arc<Primitive>>,
}

impl EditedScene {
    pub fn new(base: &Arc<Model>) -> Self {
        let mut view = (**base).clone();
        view.primitives.clear();
        Self {
            base: Arc::clone(base),
            view,
            added: Vec::new(),
        }
    }

    /// 部品のワールド行列を置き換える（子孫は動かさない。試験では部品ごとに指定する）。
    pub fn set_world(&mut self, id: u32, world: Affine3A) {
        self.view.nodes[id as usize].world = world;
    }

    /// 形状を足して、それを描く根の部品を末尾に足す。足した形状の番号を返す。
    pub fn add_part(&mut self, prim: Arc<Primitive>, world: Affine3A, color: [f32; 4]) -> PrimId {
        let id = PrimId((self.base.primitives.len() + self.added.len()) as u32);
        self.added.push(prim);
        let index = self.view.nodes.len() as u32;
        let start = self.view.draws.len() as u32;
        self.view.draws.push(Draw {
            prim: id,
            base_color: color,
            tex: None,
        });
        self.view.nodes.push(Node {
            name: format!("added{index}"),
            gltf_index: u32::MAX,
            parent: None,
            subtree_end: index + 1,
            depth: 0,
            local: world,
            world,
            draws: start..start + 1,
            world_aabb: Aabb::empty(),
        });
        id
    }
}

impl SceneRead for EditedScene {
    fn node_count(&self) -> usize {
        self.view.node_count()
    }
    fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.view.parent(id)
    }
    fn subtree_end(&self, id: NodeId) -> u32 {
        self.view.subtree_end(id)
    }
    fn depth(&self, id: NodeId) -> u32 {
        SceneRead::depth(&self.view, id)
    }
    fn name(&self, id: NodeId) -> &str {
        self.view.name(id)
    }
    fn world(&self, id: NodeId) -> &Affine3A {
        self.view.world(id)
    }
    fn world_aabb(&self, id: NodeId) -> Aabb {
        self.view.world_aabb(id)
    }
    fn draws(&self, id: NodeId) -> &[Draw] {
        SceneRead::draws(&self.view, id)
    }
    fn primitive(&self, id: PrimId) -> Option<&Primitive> {
        let base = self.base.primitives.len();
        match (id.0 as usize).checked_sub(base) {
            None => self.base.primitives.get(id.0 as usize),
            Some(added) => self.added.get(added).map(|prim| &**prim),
        }
    }
    fn primitive_count(&self) -> usize {
        self.base.primitives.len() + self.added.len()
    }
    fn file_index(&self, id: NodeId) -> Option<u32> {
        self.view.file_index(id)
    }
    fn find_file_index(&self, gltf_index: u32) -> Option<NodeId> {
        self.view.find_file_index(gltf_index)
    }
    fn bounds(&self) -> Aabb {
        self.view.bounds
    }
    fn shares_geometry_with(&self, model: &Model) -> bool {
        std::ptr::eq(&*self.base, model)
    }
}

/// 2枚の画像で、チャンネルの差が `tolerance` を超える画素の数。
pub fn differing_pixels(a: &[u8], b: &[u8], tolerance: u8) -> usize {
    a.chunks(4)
        .zip(b.chunks(4))
        .filter(|(p, q)| {
            p.iter()
                .zip(q.iter())
                .any(|(x, y)| x.abs_diff(*y) > tolerance)
        })
        .count()
}
