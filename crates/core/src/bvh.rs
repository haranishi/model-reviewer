//! プリミティブごとの三角形BVH。中央値で分割し、葉は三角形8個以下。たどるときは再帰しない。

use glam::Vec3;

use crate::scene::{Primitive, Vertex};

const LEAF_TRIANGLES: usize = 8;
const CANCEL_STRIDE: usize = 4096;

#[derive(Clone, Debug)]
pub struct Bvh {
    nodes: Vec<Node>,
    tri_indices: Vec<u32>,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    min: Vec3,
    max: Vec3,
    kind: NodeKind,
}

#[derive(Clone, Copy, Debug)]
enum NodeKind {
    Leaf { start: u32, count: u32 },
    Branch { left: u32, right: u32 },
}

struct Tri {
    index: u32,
    centroid: Vec3,
    min: Vec3,
    max: Vec3,
}

#[derive(Clone, Copy, Debug)]
pub struct TriHit {
    pub t: f32,
    pub local_pos: Vec3,
    pub normal: Vec3,
    /// 当たった三角形の番号（形状の番号の表で何番目の三角形か）。
    pub triangle: u32,
}

impl Bvh {
    pub fn build(prim: &Primitive, cancel: &dyn Fn() -> bool) -> Option<Self> {
        let tri_count = prim.indices.len() / 3;
        let mut tris = Vec::with_capacity(tri_count);
        for triangle in 0..tri_count {
            if triangle.is_multiple_of(CANCEL_STRIDE) && cancel() {
                return None;
            }
            let base = triangle * 3;
            let i0 = prim.indices[base] as usize;
            let i1 = prim.indices[base + 1] as usize;
            let i2 = prim.indices[base + 2] as usize;
            if i0 >= prim.vertices.len() || i1 >= prim.vertices.len() || i2 >= prim.vertices.len() {
                continue;
            }
            let p0 = Vec3::from(prim.vertices[i0].position);
            let p1 = Vec3::from(prim.vertices[i1].position);
            let p2 = Vec3::from(prim.vertices[i2].position);
            if !p0.is_finite() || !p1.is_finite() || !p2.is_finite() {
                continue;
            }
            tris.push(Tri {
                index: triangle as u32,
                centroid: (p0 + p1 + p2) * (1.0 / 3.0),
                min: p0.min(p1).min(p2),
                max: p0.max(p1).max(p2),
            });
        }
        if tris.is_empty() {
            return Some(Self {
                nodes: Vec::new(),
                tri_indices: Vec::new(),
            });
        }
        let mut nodes = Vec::new();
        let mut tri_indices = Vec::with_capacity(tris.len());
        let mut budget = 0usize;
        build_node(&mut tris, &mut nodes, &mut tri_indices, cancel, &mut budget)?;
        Some(Self { nodes, tri_indices })
    }

    /// `dir` は正規化しない。`t` は渡された方向のままの係数。
    pub fn closest(
        &self,
        origin: Vec3,
        dir: Vec3,
        vertices: &[Vertex],
        indices: &[u32],
        max_t: f32,
    ) -> Option<TriHit> {
        if self.nodes.is_empty()
            || max_t.is_nan()
            || max_t <= 0.0
            || !origin.is_finite()
            || !dir.is_finite()
        {
            return None;
        }
        let mut best_t = max_t;
        let mut best: Option<TriHit> = None;
        let mut stack = Vec::with_capacity(64);
        stack.push(0u32);
        while let Some(index) = stack.pop() {
            let Some(node) = self.nodes.get(index as usize) else {
                continue;
            };
            if ray_aabb(origin, dir, node.min, node.max, best_t).is_none() {
                continue;
            }
            match node.kind {
                NodeKind::Leaf { start, count } => {
                    for slot in start..start.saturating_add(count) {
                        let Some(triangle) = self.tri_indices.get(slot as usize).copied() else {
                            continue;
                        };
                        let Some(hit) =
                            ray_triangle(origin, dir, vertices, indices, triangle, best_t)
                        else {
                            continue;
                        };
                        if hit.t < best_t {
                            best_t = hit.t;
                            best = Some(hit);
                        }
                    }
                }
                NodeKind::Branch { left, right } => {
                    push_children(&mut stack, &self.nodes, origin, dir, left, right, best_t);
                }
            }
        }
        best
    }

    /// 境界箱が球（中心 `center`・半径 `radius`、形状の中の座標）に触れる三角形の番号を `out` に足す。
    /// `limit` を超えたら `false` を返す（`out` の中身は途中まで）。
    pub fn triangles_near(
        &self,
        center: Vec3,
        radius: f32,
        vertices: &[Vertex],
        indices: &[u32],
        limit: usize,
        out: &mut Vec<u32>,
    ) -> bool {
        // 契約の先置き。中身は後の作業で書く。
        let _ = (center, radius, vertices, indices, limit, out);
        true
    }
}

fn build_node(
    tris: &mut [Tri],
    nodes: &mut Vec<Node>,
    indices: &mut Vec<u32>,
    cancel: &dyn Fn() -> bool,
    budget: &mut usize,
) -> Option<u32> {
    *budget += tris.len();
    if *budget >= CANCEL_STRIDE {
        *budget = 0;
        if cancel() {
            return None;
        }
    }
    if tris.len() <= LEAF_TRIANGLES {
        let start = indices.len() as u32;
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for tri in tris.iter() {
            indices.push(tri.index);
            min = min.min(tri.min);
            max = max.max(tri.max);
        }
        let id = nodes.len() as u32;
        nodes.push(Node {
            min,
            max,
            kind: NodeKind::Leaf {
                start,
                count: tris.len() as u32,
            },
        });
        return Some(id);
    }
    let (cmin, cmax) = centroid_bounds(tris);
    let axis = longest_axis(cmin, cmax);
    let mid = tris.len() / 2;
    tris.select_nth_unstable_by(mid, |a, b| match axis {
        0 => a.centroid.x.total_cmp(&b.centroid.x),
        1 => a.centroid.y.total_cmp(&b.centroid.y),
        _ => a.centroid.z.total_cmp(&b.centroid.z),
    });
    let id = nodes.len() as u32;
    nodes.push(Node {
        min: Vec3::ZERO,
        max: Vec3::ZERO,
        kind: NodeKind::Branch { left: 0, right: 0 },
    });
    let left = build_node(&mut tris[..mid], nodes, indices, cancel, budget)?;
    let right = build_node(&mut tris[mid..], nodes, indices, cancel, budget)?;
    let (min, max) = (
        nodes[left as usize].min.min(nodes[right as usize].min),
        nodes[left as usize].max.max(nodes[right as usize].max),
    );
    nodes[id as usize] = Node {
        min,
        max,
        kind: NodeKind::Branch { left, right },
    };
    Some(id)
}

fn centroid_bounds(tris: &[Tri]) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for tri in tris {
        min = min.min(tri.centroid);
        max = max.max(tri.centroid);
    }
    (min, max)
}

fn longest_axis(min: Vec3, max: Vec3) -> usize {
    let extent = max - min;
    if extent.y > extent.x && extent.y >= extent.z {
        1
    } else if extent.z > extent.x {
        2
    } else {
        0
    }
}

fn push_children(
    stack: &mut Vec<u32>,
    nodes: &[Node],
    origin: Vec3,
    dir: Vec3,
    left: u32,
    right: u32,
    max_t: f32,
) {
    let left_t = nodes
        .get(left as usize)
        .and_then(|node| ray_aabb(origin, dir, node.min, node.max, max_t));
    let right_t = nodes
        .get(right as usize)
        .and_then(|node| ray_aabb(origin, dir, node.min, node.max, max_t));
    match (left_t, right_t) {
        (Some(a), Some(b)) if a > b => {
            stack.push(left);
            stack.push(right);
        }
        (Some(_), Some(_)) => {
            stack.push(right);
            stack.push(left);
        }
        (Some(_), None) => stack.push(left),
        (None, Some(_)) => stack.push(right),
        (None, None) => {}
    }
}

fn ray_aabb(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3, max_t: f32) -> Option<f32> {
    let mut t0 = 0.0f32;
    let mut t1 = max_t;
    for axis in 0..3 {
        let o = origin[axis];
        let d = dir[axis];
        let lo = min[axis];
        let hi = max[axis];
        if d.abs() <= 1.0e-12 {
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let inv = 1.0 / d;
        let (ta, tb) = {
            let a = (lo - o) * inv;
            let b = (hi - o) * inv;
            if a <= b { (a, b) } else { (b, a) }
        };
        t0 = t0.max(ta);
        t1 = t1.min(tb);
        if t0 > t1 + 1.0e-5 {
            return None;
        }
    }
    if t1 < 0.0 { None } else { Some(t0.max(0.0)) }
}

fn ray_triangle(
    origin: Vec3,
    dir: Vec3,
    vertices: &[Vertex],
    indices: &[u32],
    triangle: u32,
    max_t: f32,
) -> Option<TriHit> {
    let base = triangle as usize * 3;
    let (Some(i0), Some(i1), Some(i2)) = (
        indices.get(base).copied(),
        indices.get(base + 1).copied(),
        indices.get(base + 2).copied(),
    ) else {
        return None;
    };
    let (Some(v0), Some(v1), Some(v2)) = (
        vertex(vertices, i0),
        vertex(vertices, i1),
        vertex(vertices, i2),
    ) else {
        return None;
    };
    let e1 = v1 - v0;
    let e2 = v2 - v0;
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if !det.is_finite() || det.abs() <= 1.0e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    let tvec = origin - v0;
    let u = tvec.dot(p) * inv_det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = tvec.cross(e1);
    let v = dir.dot(q) * inv_det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv_det;
    if !t.is_finite() || t < 1.0e-6 || t >= max_t {
        return None;
    }
    let normal = e1.cross(e2);
    let normal = if normal.length_squared() > 0.0 && normal.is_finite() {
        normal.normalize()
    } else {
        Vec3::Z
    };
    let local_pos = v0 * (1.0 - u - v) + v1 * u + v2 * v;
    if !local_pos.is_finite() {
        return None;
    }
    Some(TriHit {
        t,
        local_pos,
        normal,
        triangle,
    })
}

fn vertex(vertices: &[Vertex], index: u32) -> Option<Vec3> {
    vertices
        .get(index as usize)
        .map(|vertex| Vec3::from(vertex.position))
}
