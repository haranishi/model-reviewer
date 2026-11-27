//! 足す図形の形状。立方体は面ごとに法線を分ける。球は経度32・緯度16、円柱は32分割。
//! 原点が中心で、1辺（球と円柱は直径、円柱は高さも同じ）が `size`。平面は高さ0の水平面で、
//! 表は +Y。三角形は外から見て反時計回りが表（読み込む形状と同じ約束）。
//!
//! 球は極を1点ずつにし、継ぎ目の頂点を重ねない。円柱は側面（滑らかな法線）と上下の蓋（平らな
//! 法線）で頂点を分ける。

use glam::Vec3;

use crate::math::Aabb;
use crate::scene::{Primitive, Vertex};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShapeKind {
    Cube,
    Sphere,
    Cylinder,
    Plane,
}

/// 足した図形の作り方（`build(kind, size)` の引数）。文書が形状ごとに覚える。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeSpec {
    pub kind: ShapeKind,
    pub size: f32,
}

/// 図形の寸法（シーンの単位）。立方体は3軸、球は直径、円柱は直径（X）と高さ（Y）、平面は X と Z。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dims {
    Cube { x: f32, y: f32, z: f32 },
    Sphere { d: f32 },
    Cylinder { d: f32, h: f32 },
    Plane { x: f32, z: f32 },
}

/// 寸法として受け付ける下限と上限。
pub const DIM_MIN: f32 = 1.0e-6;
pub const DIM_MAX: f32 = 1.0e9;

/// 球の3軸と円柱の X・Z がそろっているとみなす相対差の上限。
const UNIFORM_REL_TOL: f64 = 1.0e-4;

/// 大きさ（部品の中の3軸の拡大）から寸法を求める。球の3軸、円柱の X と Z が相対 1e-4 以内で
/// そろっていなければ `None`。
pub fn dims_of(spec: ShapeSpec, scale: Vec3) -> Option<Dims> {
    if !positive_finite(spec.size) || !scale.is_finite() {
        return None;
    }
    let dim = |component: f32| -> Option<f32> {
        if !positive_finite(component) {
            return None;
        }
        let value = spec.size * component;
        positive_finite(value).then_some(value)
    };
    match spec.kind {
        ShapeKind::Cube => Some(Dims::Cube {
            x: dim(scale.x)?,
            y: dim(scale.y)?,
            z: dim(scale.z)?,
        }),
        ShapeKind::Sphere => {
            let d = dim(scale.x)?;
            let _ = (dim(scale.y)?, dim(scale.z)?);
            if !uniform_triple(scale.x, scale.y, scale.z) {
                return None;
            }
            Some(Dims::Sphere { d })
        }
        ShapeKind::Cylinder => {
            let d = dim(scale.x)?;
            let h = dim(scale.y)?;
            let _ = dim(scale.z)?;
            if !uniform_pair(scale.x, scale.z) {
                return None;
            }
            Some(Dims::Cylinder { d, h })
        }
        ShapeKind::Plane => Some(Dims::Plane {
            x: dim(scale.x)?,
            z: dim(scale.z)?,
        }),
    }
}

/// 寸法から大きさを求める。図形の種類が合わない、有限でない、`DIM_MIN`〜`DIM_MAX` の外なら
/// `None`。平面の Y は `keep_y` をそのまま使う。
pub fn scale_for(spec: ShapeSpec, dims: Dims, keep_y: f32) -> Option<Vec3> {
    if !positive_finite(spec.size) {
        return None;
    }
    let ratio = |value: f32| -> Option<f32> {
        if !valid_dim(value) {
            return None;
        }
        let factor = value / spec.size;
        positive_finite(factor).then_some(factor)
    };
    match (spec.kind, dims) {
        (ShapeKind::Cube, Dims::Cube { x, y, z }) => {
            Some(Vec3::new(ratio(x)?, ratio(y)?, ratio(z)?))
        }
        (ShapeKind::Sphere, Dims::Sphere { d }) => Some(Vec3::splat(ratio(d)?)),
        (ShapeKind::Cylinder, Dims::Cylinder { d, h }) => {
            let xz = ratio(d)?;
            let y = ratio(h)?;
            Some(Vec3::new(xz, y, xz))
        }
        (ShapeKind::Plane, Dims::Plane { x, z }) => {
            if !keep_y.is_finite() {
                return None;
            }
            Some(Vec3::new(ratio(x)?, keep_y, ratio(z)?))
        }
        _ => None,
    }
}

fn positive_finite(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

fn valid_dim(value: f32) -> bool {
    value.is_finite() && (DIM_MIN..=DIM_MAX).contains(&value)
}

fn uniform_pair(a: f32, b: f32) -> bool {
    let (a, b) = (f64::from(a), f64::from(b));
    let max = a.max(b);
    let min = a.min(b);
    max - min <= max * UNIFORM_REL_TOL
}

fn uniform_triple(a: f32, b: f32, c: f32) -> bool {
    let (a, b, c) = (f64::from(a), f64::from(b), f64::from(c));
    let max = a.max(b).max(c);
    let min = a.min(b).min(c);
    max - min <= max * UNIFORM_REL_TOL
}

/// 足した図形の色（中間の灰色1色）。
pub const SHAPE_COLOR: [f32; 4] = [0.5, 0.5, 0.5, 1.0];

const SPHERE_SEGMENTS: u32 = 32;
const SPHERE_RINGS: u32 = 16;
const CYLINDER_SEGMENTS: u32 = 32;

/// 形状を作る。`size` が有限の正の数でなければ 1 として作る。
pub fn build(kind: ShapeKind, size: f32) -> Primitive {
    let size = if size.is_finite() && size > 0.0 {
        size
    } else {
        1.0
    };
    let half = size * 0.5;
    let mut mesh = Mesh::default();
    match kind {
        ShapeKind::Cube => cube(&mut mesh, half),
        ShapeKind::Sphere => sphere(&mut mesh, half),
        ShapeKind::Cylinder => cylinder(&mut mesh, half),
        ShapeKind::Plane => plane(&mut mesh, half),
    }
    mesh.finish()
}

/// 置く大きさ。`10^round(log10(全体の境界球の半径 / 4))`。半径が有限の正の数でなければ 1。
pub fn size_for(bounds_radius: f32) -> f32 {
    if !bounds_radius.is_finite() || bounds_radius <= 0.0 {
        return 1.0;
    }
    let exponent = (bounds_radius / 4.0).log10().round().clamp(-30.0, 30.0);
    let size = 10.0_f32.powi(exponent as i32);
    if size.is_finite() && size > 0.0 {
        size
    } else {
        1.0
    }
}

/// 部品の名前（「立方体」「球」「円柱」「平面」）。
pub fn name_ja(kind: ShapeKind) -> &'static str {
    match kind {
        ShapeKind::Cube => "立方体",
        ShapeKind::Sphere => "球",
        ShapeKind::Cylinder => "円柱",
        ShapeKind::Plane => "平面",
    }
}

#[derive(Default)]
struct Mesh {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
}

impl Mesh {
    fn vertex(&mut self, position: Vec3, normal: Vec3) -> u32 {
        let index = self.vertices.len() as u32;
        self.vertices.push(Vertex {
            position: position.to_array(),
            normal: normal.normalize_or_zero().to_array(),
        });
        index
    }

    fn triangle(&mut self, a: u32, b: u32, c: u32) {
        self.indices.extend([a, b, c]);
    }

    fn finish(self) -> Primitive {
        let mut local_aabb = Aabb::empty();
        for vertex in &self.vertices {
            let point = Vec3::from(vertex.position);
            local_aabb.min = local_aabb.min.min(point);
            local_aabb.max = local_aabb.max.max(point);
        }
        Primitive {
            vertices: self.vertices,
            indices: self.indices,
            local_aabb,
            has_normals: true,
        }
    }
}

fn cube(mesh: &mut Mesh, h: f32) {
    // 面ごとに (法線, 面の上で右に進む軸, 上に進む軸)。右 × 上 = 法線なので、
    // 左下 → 右下 → 右上 → 左上 の順が外から見て反時計回りになる。
    let faces = [
        (Vec3::X, Vec3::NEG_Z, Vec3::Y),
        (Vec3::NEG_X, Vec3::Z, Vec3::Y),
        (Vec3::Y, Vec3::X, Vec3::NEG_Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
    ];
    for (normal, right, up) in faces {
        let center = normal * h;
        let corners = [
            center - right * h - up * h,
            center + right * h - up * h,
            center + right * h + up * h,
            center - right * h + up * h,
        ];
        let base = mesh.vertices.len() as u32;
        for corner in corners {
            mesh.vertex(corner, normal);
        }
        mesh.triangle(base, base + 1, base + 2);
        mesh.triangle(base, base + 2, base + 3);
    }
}

fn sphere(mesh: &mut Mesh, radius: f32) {
    let top = mesh.vertex(Vec3::Y * radius, Vec3::Y);
    // 緯度の輪は極を除いて SPHERE_RINGS - 1 本。
    let ring_count = SPHERE_RINGS - 1;
    let first = mesh.vertices.len() as u32;
    for ring in 1..SPHERE_RINGS {
        let polar = ring as f32 / SPHERE_RINGS as f32 * std::f32::consts::PI;
        let (sin_p, cos_p) = polar.sin_cos();
        for segment in 0..SPHERE_SEGMENTS {
            let normal = around(segment, SPHERE_SEGMENTS) * sin_p + Vec3::Y * cos_p;
            mesh.vertex(normal * radius, normal);
        }
    }
    let bottom = mesh.vertex(Vec3::NEG_Y * radius, Vec3::NEG_Y);
    let at = |ring: u32, segment: u32| first + ring * SPHERE_SEGMENTS + segment % SPHERE_SEGMENTS;
    for segment in 0..SPHERE_SEGMENTS {
        mesh.triangle(top, at(0, segment), at(0, segment + 1));
    }
    for ring in 0..ring_count - 1 {
        for segment in 0..SPHERE_SEGMENTS {
            let (a, b) = (at(ring, segment), at(ring, segment + 1));
            let (c, d) = (at(ring + 1, segment), at(ring + 1, segment + 1));
            mesh.triangle(a, c, d);
            mesh.triangle(a, d, b);
        }
    }
    let last = ring_count - 1;
    for segment in 0..SPHERE_SEGMENTS {
        mesh.triangle(bottom, at(last, segment + 1), at(last, segment));
    }
}

fn cylinder(mesh: &mut Mesh, h: f32) {
    let n = CYLINDER_SEGMENTS;
    let side = mesh.vertices.len() as u32;
    for segment in 0..n {
        let dir = around(segment, n);
        mesh.vertex(dir * h + Vec3::Y * h, dir);
        mesh.vertex(dir * h - Vec3::Y * h, dir);
    }
    for segment in 0..n {
        let top0 = side + segment * 2;
        let bottom0 = top0 + 1;
        let top1 = side + (segment + 1) % n * 2;
        let bottom1 = top1 + 1;
        mesh.triangle(top0, bottom0, bottom1);
        mesh.triangle(top0, bottom1, top1);
    }
    for (y, normal) in [(h, Vec3::Y), (-h, Vec3::NEG_Y)] {
        let center = mesh.vertex(Vec3::Y * y, normal);
        let ring = mesh.vertices.len() as u32;
        for segment in 0..n {
            mesh.vertex(around(segment, n) * h + Vec3::Y * y, normal);
        }
        for segment in 0..n {
            let a = ring + segment;
            let b = ring + (segment + 1) % n;
            if normal.y > 0.0 {
                mesh.triangle(center, a, b);
            } else {
                mesh.triangle(center, b, a);
            }
        }
    }
}

fn plane(mesh: &mut Mesh, h: f32) {
    let a = mesh.vertex(Vec3::new(-h, 0.0, h), Vec3::Y);
    let b = mesh.vertex(Vec3::new(h, 0.0, h), Vec3::Y);
    let c = mesh.vertex(Vec3::new(h, 0.0, -h), Vec3::Y);
    let d = mesh.vertex(Vec3::new(-h, 0.0, -h), Vec3::Y);
    mesh.triangle(a, b, c);
    mesh.triangle(a, c, d);
}

/// 水平面の上の向き。`segment` 番目を、+Z から始めて +X の側へ回る順（上から見て時計回り）に並べる。
/// 0°・90°・180°・270° はちょうどの値にして、境界が指定の大きさに一致するようにする。
fn around(segment: u32, count: u32) -> Vec3 {
    let quarter = count / 4;
    if count.is_multiple_of(4) && segment.is_multiple_of(quarter) {
        return match (segment / quarter) % 4 {
            0 => Vec3::Z,
            1 => Vec3::X,
            2 => Vec3::NEG_Z,
            _ => Vec3::NEG_X,
        };
    }
    let angle = segment as f32 / count as f32 * std::f32::consts::TAU;
    let (sin, cos) = angle.sin_cos();
    Vec3::new(sin, 0.0, cos)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const ALL: [ShapeKind; 4] = [
        ShapeKind::Cube,
        ShapeKind::Sphere,
        ShapeKind::Cylinder,
        ShapeKind::Plane,
    ];

    fn triangles(prim: &Primitive) -> impl Iterator<Item = [Vec3; 3]> + '_ {
        prim.indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|tri| [0, 1, 2].map(|k| Vec3::from(prim.vertices[tri[k] as usize].position)))
    }

    #[test]
    fn counts_match_the_documented_tessellation() {
        let expected = [
            (ShapeKind::Cube, 24, 12),
            (ShapeKind::Sphere, 2 + 32 * 15, 32 * 2 + 32 * 14 * 2),
            (ShapeKind::Cylinder, 32 * 2 + 2 * 33, 32 * 2 + 32 * 2),
            (ShapeKind::Plane, 4, 2),
        ];
        for (kind, vertices, tris) in expected {
            let prim = build(kind, 1.0);
            assert_eq!(prim.vertices.len(), vertices, "{kind:?}");
            assert_eq!(prim.indices.len(), tris * 3, "{kind:?}");
            assert!(prim.has_normals);
            assert!(
                prim.indices
                    .iter()
                    .all(|&i| (i as usize) < prim.vertices.len())
            );
        }
    }

    #[test]
    fn normals_are_unit_and_point_outward_and_winding_is_counter_clockwise() {
        for kind in ALL {
            let prim = build(kind, 2.0);
            for vertex in &prim.vertices {
                let normal = Vec3::from(vertex.normal);
                assert!((normal.length() - 1.0).abs() < 1.0e-5, "{kind:?}");
                let position = Vec3::from(vertex.position);
                if kind == ShapeKind::Plane {
                    assert_eq!(normal, Vec3::Y);
                } else {
                    assert!(normal.dot(position) > 0.0, "{kind:?} {position:?}");
                }
            }
            for [a, b, c] in triangles(&prim) {
                let face = (b - a).cross(c - a);
                assert!(face.length() > 1.0e-6, "{kind:?} degenerate");
                let outward = if kind == ShapeKind::Plane {
                    Vec3::Y
                } else {
                    (a + b + c) / 3.0
                };
                assert!(
                    face.dot(outward) > 0.0,
                    "{kind:?} winding {a:?} {b:?} {c:?}"
                );
            }
            // 頂点の法線と、その頂点を使う三角形の向きが同じ側。
            for tri in prim.indices.as_chunks::<3>().0 {
                let [a, b, c] =
                    [0, 1, 2].map(|k| Vec3::from(prim.vertices[tri[k] as usize].position));
                let face = (b - a).cross(c - a);
                for &index in tri {
                    let normal = Vec3::from(prim.vertices[index as usize].normal);
                    assert!(face.dot(normal) > 0.0, "{kind:?}");
                }
            }
        }
    }

    #[test]
    fn closed_shapes_share_every_edge_between_two_faces() {
        for kind in [ShapeKind::Cube, ShapeKind::Sphere, ShapeKind::Cylinder] {
            let prim = build(kind, 1.0);
            // 法線の違う重なった頂点を、位置でまとめる。
            let mut welded: HashMap<[u32; 3], u32> = HashMap::new();
            let ids: Vec<u32> = prim
                .vertices
                .iter()
                .map(|vertex| {
                    let key = vertex.position.map(|v| (v * 1.0e5).round() as i32 as u32);
                    let next = welded.len() as u32;
                    *welded.entry(key).or_insert(next)
                })
                .collect();
            let mut directed: HashMap<(u32, u32), u32> = HashMap::new();
            for tri in prim.indices.as_chunks::<3>().0 {
                let t = [
                    ids[tri[0] as usize],
                    ids[tri[1] as usize],
                    ids[tri[2] as usize],
                ];
                for k in 0..3 {
                    *directed.entry((t[k], t[(k + 1) % 3])).or_default() += 1;
                }
            }
            for (&(a, b), &count) in &directed {
                assert_eq!(count, 1, "{kind:?} edge {a}->{b} used twice the same way");
                assert_eq!(
                    directed.get(&(b, a)),
                    Some(&1),
                    "{kind:?} edge {a}-{b} is not shared by two faces"
                );
            }
        }
        // 平面は縁が開いている（4辺が1面だけ）。
        let plane = build(ShapeKind::Plane, 1.0);
        assert_eq!(plane.indices.len(), 6);
    }

    #[test]
    fn bounds_match_the_requested_size() {
        for size in [0.01_f32, 1.0, 250.0] {
            for kind in ALL {
                let prim = build(kind, size);
                let half = size * 0.5;
                let y = if kind == ShapeKind::Plane { 0.0 } else { half };
                let expected = Aabb {
                    min: Vec3::new(-half, -y, -half),
                    max: Vec3::new(half, y, half),
                };
                assert!(
                    (prim.local_aabb.min - expected.min).abs().max_element() <= size * 1.0e-6
                        && (prim.local_aabb.max - expected.max).abs().max_element()
                            <= size * 1.0e-6,
                    "{kind:?} {size}: {:?}",
                    prim.local_aabb
                );
            }
        }
        let fallback = build(ShapeKind::Cube, f32::NAN);
        assert_eq!(fallback.local_aabb.max, Vec3::splat(0.5));
    }

    #[test]
    fn size_and_names() {
        assert_eq!(size_for(4.0), 1.0);
        assert_eq!(size_for(40.0), 10.0);
        assert_eq!(size_for(0.4), 0.1);
        // log10(13/4) = 0.51… で切り上がる。
        assert_eq!(size_for(13.0), 10.0);
        assert_eq!(size_for(12.0), 1.0);
        assert_eq!(size_for(0.0), 1.0);
        assert_eq!(size_for(f32::NAN), 1.0);
        assert_eq!(size_for(f32::INFINITY), 1.0);
        assert_eq!(name_ja(ShapeKind::Cube), "立方体");
        assert_eq!(name_ja(ShapeKind::Sphere), "球");
        assert_eq!(name_ja(ShapeKind::Cylinder), "円柱");
        assert_eq!(name_ja(ShapeKind::Plane), "平面");
    }

    fn dims_close(actual: Dims, expected: Dims) {
        let values = |dims: Dims| -> Vec<f32> {
            match dims {
                Dims::Cube { x, y, z } => vec![0.0, x, y, z],
                Dims::Sphere { d } => vec![1.0, d],
                Dims::Cylinder { d, h } => vec![2.0, d, h],
                Dims::Plane { x, z } => vec![3.0, x, z],
            }
        };
        let (a, e) = (values(actual), values(expected));
        assert_eq!(a.len(), e.len(), "{actual:?} vs {expected:?}");
        assert_eq!(a[0], e[0], "{actual:?} vs {expected:?}");
        for (a, e) in a.iter().zip(&e).skip(1) {
            assert!(
                (a - e).abs() <= e.abs() * 1.0e-5,
                "{actual:?} vs {expected:?}"
            );
        }
    }

    #[test]
    fn dims_round_trip_per_kind() {
        let cases = [
            Dims::Cube {
                x: 0.02,
                y: 0.03,
                z: 0.04,
            },
            Dims::Sphere { d: 12.5 },
            Dims::Cylinder { d: 0.5, h: 3.0 },
            Dims::Plane { x: 250.0, z: 0.75 },
        ];
        for (kind, dims) in ALL.into_iter().zip(cases) {
            for size in [0.1_f32, 1.0, 10.0] {
                let spec = ShapeSpec { kind, size };
                let scale = scale_for(spec, dims, 1.0).expect("範囲の中の寸法");
                assert!(scale.is_finite() && scale.min_element() > 0.0);
                let back = dims_of(spec, scale).expect("そろった大きさ");
                dims_close(back, dims);
            }
        }
        // 寸法は spec.size × 大きさ。円柱は直径が X、高さが Y。
        let spec = ShapeSpec {
            kind: ShapeKind::Cylinder,
            size: 2.0,
        };
        dims_close(
            dims_of(spec, Vec3::new(1.5, 4.0, 1.5)).unwrap(),
            Dims::Cylinder { d: 3.0, h: 8.0 },
        );
        // 平面の Y は keep_y のまま。
        let plane = ShapeSpec {
            kind: ShapeKind::Plane,
            size: 1.0,
        };
        let scale = scale_for(plane, Dims::Plane { x: 2.0, z: 3.0 }, 7.0).unwrap();
        assert!((scale.y - 7.0).abs() <= 1.0e-6);
        dims_close(
            dims_of(plane, Vec3::new(2.0, 0.01, 3.0)).unwrap(),
            Dims::Plane { x: 2.0, z: 3.0 },
        );
    }

    #[test]
    fn uneven_sphere_has_no_dims() {
        let sphere = ShapeSpec {
            kind: ShapeKind::Sphere,
            size: 1.0,
        };
        assert!(dims_of(sphere, Vec3::splat(2.0)).is_some());
        assert_eq!(dims_of(sphere, Vec3::new(2.0, 2.0, 2.5)), None);
        assert_eq!(dims_of(sphere, Vec3::new(1.0, 1.5, 1.0)), None);
        let cylinder = ShapeSpec {
            kind: ShapeKind::Cylinder,
            size: 1.0,
        };
        // 高さ（Y）は自由。X と Z がそろっていないと None。
        assert!(dims_of(cylinder, Vec3::new(2.0, 9.0, 2.0)).is_some());
        assert_eq!(dims_of(cylinder, Vec3::new(2.0, 9.0, 3.0)), None);
        // 立方体と平面は軸ごとに違ってよい。
        let cube = ShapeSpec {
            kind: ShapeKind::Cube,
            size: 1.0,
        };
        assert!(dims_of(cube, Vec3::new(1.0, 2.0, 3.0)).is_some());
        // 鏡に映した（負の）大きさ、0、有限でない値は寸法にしない。
        assert_eq!(dims_of(cube, Vec3::new(-1.0, 1.0, 1.0)), None);
        assert_eq!(dims_of(cube, Vec3::new(0.0, 1.0, 1.0)), None);
        assert_eq!(dims_of(cube, Vec3::new(f32::NAN, 1.0, 1.0)), None);
        assert_eq!(dims_of(sphere, Vec3::splat(f32::INFINITY)), None);
    }

    #[test]
    fn out_of_range_dims_are_rejected() {
        let cube = ShapeSpec {
            kind: ShapeKind::Cube,
            size: 1.0,
        };
        let cube_with = |x: f32| Dims::Cube { x, y: 1.0, z: 1.0 };
        for bad in [
            0.0,
            -1.0,
            DIM_MIN * 0.5,
            DIM_MAX * 2.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ] {
            assert_eq!(scale_for(cube, cube_with(bad), 1.0), None, "{bad}");
        }
        assert!(scale_for(cube, cube_with(DIM_MIN * 2.0), 1.0).is_some());
        assert!(scale_for(cube, cube_with(DIM_MAX * 0.5), 1.0).is_some());
        let sphere = ShapeSpec {
            kind: ShapeKind::Sphere,
            size: 1.0,
        };
        assert_eq!(scale_for(sphere, Dims::Sphere { d: -2.0 }, 1.0), None);
        assert_eq!(
            scale_for(sphere, Dims::Cylinder { d: 1.0, h: 1.0 }, 1.0),
            None,
            "図形の種類が合わない"
        );
        let plane = ShapeSpec {
            kind: ShapeKind::Plane,
            size: 1.0,
        };
        assert_eq!(
            scale_for(plane, Dims::Plane { x: 1.0, z: 1.0 }, f32::NAN),
            None
        );
        let broken = ShapeSpec {
            kind: ShapeKind::Cube,
            size: 0.0,
        };
        assert_eq!(scale_for(broken, cube_with(1.0), 1.0), None);
        assert_eq!(dims_of(broken, Vec3::ONE), None);
    }
}
