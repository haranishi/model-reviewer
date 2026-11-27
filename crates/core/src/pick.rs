//! 光線と三角形の交差。方向はローカルへ移しても正規化し直さない。

use glam::{Affine3A, Vec3};

use crate::bvh::Bvh;
use crate::math::Ray;
use crate::scene::{NodeId, PrimId, SceneRead};
use crate::visibility::Visibility;

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub node: NodeId,
    pub t: f32,
    pub world_pos: Vec3,
    pub local_pos: Vec3,
    pub local_normal: Vec3,
}

pub struct PickIndex {
    bvhs: Vec<Bvh>,
}

impl PickIndex {
    pub fn build(model: &impl SceneRead) -> Self {
        Self::build_cancellable(model, &|| false).unwrap_or_else(|| Self { bvhs: Vec::new() })
    }

    /// プリミティブの間と、大きいプリミティブの構築中に `cancel` を見る。true なら `None`。
    pub fn build_cancellable(model: &impl SceneRead, cancel: &dyn Fn() -> bool) -> Option<Self> {
        let count = model.primitive_count();
        let mut bvhs = Vec::with_capacity(count);
        for index in 0..count {
            if cancel() {
                return None;
            }
            let prim = model.primitive(PrimId(index as u32))?;
            bvhs.push(Bvh::build(prim, cancel)?);
        }
        Some(Self { bvhs })
    }

    /// まだ索引の無い形状（足した図形。番号の続き）の索引を末尾に足す。足した数を返す。
    /// 既にある索引は作り直さない。どの担当にも属さない小さな処理なので、ここで書いておく。
    pub fn extend(&mut self, scene: &impl SceneRead) -> usize {
        let mut added = 0;
        for index in self.bvhs.len()..scene.primitive_count() {
            let Some(bvh) = scene
                .primitive(PrimId(index as u32))
                .and_then(|prim| Bvh::build(prim, &|| false))
            else {
                break;
            };
            self.bvhs.push(bvh);
            added += 1;
        }
        added
    }
}

pub fn pick(model: &impl SceneRead, index: &PickIndex, vis: &Visibility, ray: &Ray) -> Option<Hit> {
    if !ray.origin.is_finite() || !ray.dir.is_finite() || ray.dir.length_squared() <= 1.0e-20 {
        return None;
    }
    let mut candidates = Vec::new();
    for node_index in 0..model.node_count() {
        let id = NodeId(node_index as u32);
        let world_aabb = model.world_aabb(id);
        if !vis.effective(id) || model.draws(id).is_empty() || world_aabb.is_empty() {
            continue;
        }
        let Some(inverse) = invert_affine(*model.world(id)) else {
            continue;
        };
        let Some(entry) = crate_ray_aabb(ray, world_aabb.min, world_aabb.max) else {
            continue;
        };
        candidates.push((entry, id, inverse));
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.0.cmp(&b.1.0)));

    let mut best: Option<Hit> = None;
    for (entry, id, inverse) in candidates {
        if let Some(hit) = &best
            && entry > hit.t * (1.0 + 1.0e-5) + 1.0e-6
        {
            break;
        }
        let local_origin = inverse.transform_point3(ray.origin);
        let local_dir = inverse.transform_vector3(ray.dir);
        if !local_origin.is_finite()
            || !local_dir.is_finite()
            || local_dir.length_squared() <= 1.0e-20
        {
            continue;
        }
        let max_t = best.as_ref().map(|hit| hit.t).unwrap_or(f32::INFINITY);
        for draw in model.draws(id) {
            let Some(prim) = model.primitive(draw.prim) else {
                continue;
            };
            let Some(bvh) = index.bvhs.get(draw.prim.0 as usize) else {
                continue;
            };
            let Some(tri) = bvh.closest(
                local_origin,
                local_dir,
                &prim.vertices,
                &prim.indices,
                max_t,
            ) else {
                continue;
            };
            if best.as_ref().is_some_and(|hit| tri.t >= hit.t) {
                continue;
            }
            best = Some(Hit {
                node: id,
                t: tri.t,
                world_pos: ray.origin + ray.dir * tri.t,
                local_pos: tri.local_pos,
                local_normal: tri.normal,
            });
        }
    }
    best
}

fn invert_affine(world: Affine3A) -> Option<Affine3A> {
    let matrix3 = world.matrix3.try_inverse()?;
    let translation = -(matrix3 * world.translation);
    if matrix3.is_finite() && translation.is_finite() {
        Some(Affine3A {
            matrix3,
            translation,
        })
    } else {
        None
    }
}

fn crate_ray_aabb(ray: &Ray, min: Vec3, max: Vec3) -> Option<f32> {
    let mut t0 = 0.0f32;
    let mut t1 = f32::INFINITY;
    for axis in 0..3 {
        let o = ray.origin[axis];
        let d = ray.dir[axis];
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
