//! 計測の点の吸着。ポインタの視線が当たった三角形の近くだけから、頂点・辺・面の候補を探す。
//! 形状ごとの頂点の表や辺の表は作らない（常駐のメモリを増やさない）。

use glam::{Affine3A, Vec2, Vec3};

use crate::camera::OrbitCamera;
use crate::math::Ray;
use crate::pick::{Hit, PickIndex, pick_clipped};
use crate::scene::{NodeId, Primitive, SceneRead};
use crate::section::SectionPlane;
use crate::visibility::Visibility;

/// 何に吸着したか。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SnapKind {
    Vertex,
    Edge,
    #[default]
    Face,
}

/// 吸着の設定。`radius_px` は物理画素（12ポイント × `pixels_per_point`）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapSettings {
    pub enabled: bool,
    pub radius_px: f32,
}

impl Default for SnapSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            radius_px: 12.0,
        }
    }
}

/// 1回の探索の入力。
#[derive(Clone, Copy, Debug)]
pub struct SnapQuery<'a> {
    pub camera: &'a OrbitCamera,
    pub aspect: f32,
    pub viewport_px: [f32; 2],
    pub ndc: Vec2,
    pub settings: SnapSettings,
    pub clip: Option<SectionPlane>,
}

/// 吸着した先。`local` は部品の中の座標、`world` はワールドの座標。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapTarget {
    pub node: NodeId,
    pub kind: SnapKind,
    pub local: Vec3,
    pub world: Vec3,
    /// 辺のときの両端（ワールド。強調に使う）。
    pub edge: Option<[Vec3; 2]>,
}

/// 辺とみなす、隣の面との角度（度）。
pub const FEATURE_ANGLE_DEG: f32 = 10.0;
/// 1回の探索で見る三角形の上限。超えたら当たった三角形だけを見る。
pub const MAX_NEAR_TRIANGLES: usize = 512;
/// 辺の候補にする距離の、吸着の半径に対する割合。
pub const EDGE_RADIUS_RATIO: f32 = 0.75;

/// 辺の候補を、辺の判定に通す前に近い順で何本まで取るか。
const EDGE_CANDIDATES: usize = 8;
/// 部品の行列の列の長さが、これ以下なら縮みすぎとして近くの三角形を探さない。
const MIN_AXIS_LENGTH: f32 = 1.0e-20;

/// ポインタの位置で吸着の先を探す。何にも当たらなければ `None`。
///
/// 手順は設計の文書（吸着の候補の探し方）の1〜8のとおり。ポインタの視線が当たった部品の形状の、
/// 当たった点の近くの三角形だけを見る。頂点があれば頂点、無ければ辺、どちらも無ければ面。
pub fn snap(
    scene: &impl SceneRead,
    index: &PickIndex,
    vis: &Visibility,
    query: &SnapQuery,
) -> Option<SnapTarget> {
    let [width, height] = query.viewport_px;
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0)
        || !query.ndc.is_finite()
    {
        return None;
    }
    let settings = query.settings;
    let clip = query.clip.as_ref();
    let bounds = scene.bounds();
    let camera = query.camera;
    let pointer_ray = camera.screen_ray_in(query.ndc, query.aspect, &bounds);

    // 手順1・2: 視線の当たり。外れたら、吸着の半径の半分だけずらした4本で探す。
    let (hit, on_surface) = match pick_clipped(scene, index, vis, &pointer_ray, clip) {
        Some(hit) => (hit, true),
        None => {
            if !settings.enabled {
                return None;
            }
            let half = settings.radius_px * 0.5;
            let step = Vec2::new(half * 2.0 / width, half * 2.0 / height);
            if !step.is_finite() {
                return None;
            }
            let mut nearest: Option<Hit> = None;
            for offset in [
                Vec2::new(step.x, 0.0),
                Vec2::new(-step.x, 0.0),
                Vec2::new(0.0, step.y),
                Vec2::new(0.0, -step.y),
            ] {
                let ray = camera.screen_ray_in(query.ndc + offset, query.aspect, &bounds);
                if let Some(hit) = pick_clipped(scene, index, vis, &ray, clip)
                    && nearest
                        .as_ref()
                        .is_none_or(|best| hit.t.total_cmp(&best.t).is_lt())
                {
                    nearest = Some(hit);
                }
            }
            (nearest?, false)
        }
    };
    let face = || {
        on_surface.then_some(SnapTarget {
            node: hit.node,
            kind: SnapKind::Face,
            local: hit.local_pos,
            world: hit.world_pos,
            edge: None,
        })
    };
    // 手順3。
    if !settings.enabled {
        return face();
    }
    let world = *scene.world(hit.node);
    let Some(prim) = scene.primitive(hit.prim) else {
        return face();
    };
    // 手順4: 吸着の半径のワールドでの長さ。
    let Some(half_height) = camera.half_height_at(hit.world_pos) else {
        return face();
    };
    let r = half_height * 2.0 * settings.radius_px / height;
    if !r.is_finite() || r <= 0.0 {
        return face();
    }
    let reach = 1.5 * r;
    let min_axis = world
        .matrix3
        .x_axis
        .length()
        .min(world.matrix3.y_axis.length())
        .min(world.matrix3.z_axis.length());

    // 手順5: 近くの三角形。上限を超えたら当たった三角形だけを見て、辺は作らない。
    let mut triangles = Vec::new();
    let mut edges_allowed = false;
    if min_axis.is_finite()
        && min_axis > MIN_AXIS_LENGTH
        && let Some(bvh) = index.bvh(hit.prim)
    {
        let local_radius = reach / min_axis;
        edges_allowed = bvh.triangles_near(
            hit.local_pos,
            local_radius,
            &prim.vertices,
            &prim.indices,
            MAX_NEAR_TRIANGLES,
            &mut triangles,
        );
    }
    if !edges_allowed {
        triangles.clear();
    }
    if !triangles.contains(&hit.triangle) {
        triangles.push(hit.triangle);
    }

    let view = View {
        camera,
        aspect: query.aspect,
        pixel_scale: Vec2::new(width * 0.5, height * 0.5),
        pointer: query.ndc,
        eye: camera.eye(),
        forward: camera.axes().2,
        world,
        clip,
        hit_world: hit.world_pos,
        reach,
    };

    // 手順6: 頂点。
    let mut best_vertex: Option<(Rank, Vec3)> = None;
    for &triangle in &triangles {
        let Some(corners) = triangle_positions(prim, triangle) else {
            continue;
        };
        for local in corners {
            let Some((rank, _)) = view.rank(local, triangle) else {
                continue;
            };
            if rank.distance_px > settings.radius_px {
                continue;
            }
            if best_vertex
                .as_ref()
                .is_none_or(|(best, _)| rank.is_better(best))
            {
                best_vertex = Some((rank, local));
            }
        }
    }
    if let Some((_, local)) = best_vertex {
        let world_pos = world.transform_point3(local);
        if world_pos.is_finite() {
            return Some(SnapTarget {
                node: hit.node,
                kind: SnapKind::Vertex,
                local,
                world: world_pos,
                edge: None,
            });
        }
    }

    // 手順7: 辺。
    if edges_allowed {
        let edge_limit = settings.radius_px * EDGE_RADIUS_RATIO;
        let mut candidates: Vec<EdgeCandidate> = Vec::new();
        for &triangle in &triangles {
            let Some(corners) = triangle_positions(prim, triangle) else {
                continue;
            };
            if is_degenerate(corners) {
                continue;
            }
            for side in 0..3 {
                let (a, b) = (corners[side], corners[(side + 1) % 3]);
                let Some(fraction) = view.closest_fraction(a, b, &pointer_ray) else {
                    continue;
                };
                let local = a + (b - a) * fraction;
                let Some((rank, _)) = view.rank(local, triangle) else {
                    continue;
                };
                if rank.distance_px > edge_limit {
                    continue;
                }
                let key = edge_key(a, b);
                if candidates.iter().any(|c| c.key == key) {
                    continue;
                }
                candidates.push(EdgeCandidate {
                    rank,
                    key,
                    triangle,
                    ends: [a, b],
                    local,
                });
            }
        }
        candidates.sort_by(|x, y| x.rank.cmp(&y.rank));
        for candidate in candidates.iter().take(EDGE_CANDIDATES) {
            if !is_feature_edge(prim, &triangles, candidate.triangle, candidate.ends) {
                continue;
            }
            let world_pos = world.transform_point3(candidate.local);
            let ends = candidate.ends.map(|end| world.transform_point3(end));
            if world_pos.is_finite() && ends.iter().all(|end| end.is_finite()) {
                return Some(SnapTarget {
                    node: hit.node,
                    kind: SnapKind::Edge,
                    local: candidate.local,
                    world: world_pos,
                    edge: Some(ends),
                });
            }
        }
    }

    // 手順8: どちらも無ければ面（表面の外から探したときは無し）。
    face()
}

/// 候補の比べ方。画面の上の距離、次に視点からの奥行き、次に三角形の番号。
#[derive(Clone, Copy, Debug)]
struct Rank {
    distance_px: f32,
    depth: f32,
    triangle: u32,
}

impl Rank {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.distance_px
            .total_cmp(&other.distance_px)
            .then(self.depth.total_cmp(&other.depth))
            .then(self.triangle.cmp(&other.triangle))
    }

    fn is_better(&self, other: &Self) -> bool {
        self.cmp(other).is_lt()
    }
}

struct EdgeCandidate {
    rank: Rank,
    key: [[u32; 3]; 2],
    triangle: u32,
    ends: [Vec3; 2],
    local: Vec3,
}

/// 候補を評価するための、1回の探索で変わらない値。
struct View<'a> {
    camera: &'a OrbitCamera,
    aspect: f32,
    pixel_scale: Vec2,
    pointer: Vec2,
    eye: Vec3,
    forward: Vec3,
    world: Affine3A,
    clip: Option<&'a SectionPlane>,
    hit_world: Vec3,
    reach: f32,
}

impl View<'_> {
    /// 形状の中の点を候補として評価する。断面で消えた側、当たった点から遠すぎる点、
    /// 投影できない点は `None`。
    fn rank(&self, local: Vec3, triangle: u32) -> Option<(Rank, Vec3)> {
        if !local.is_finite() {
            return None;
        }
        let world = self.world.transform_point3(local);
        if !world.is_finite() || self.clip.is_some_and(|plane| plane.cuts(world)) {
            return None;
        }
        if world.distance(self.hit_world) > self.reach {
            return None;
        }
        let ndc = self.camera.project(world, self.aspect)?;
        let distance_px = ((ndc - self.pointer) * self.pixel_scale).length();
        let depth = (world - self.eye).dot(self.forward);
        if !distance_px.is_finite() || !depth.is_finite() {
            return None;
        }
        Some((
            Rank {
                distance_px,
                depth,
                triangle,
            },
            world,
        ))
    }

    /// 線分 `a`〜`b`（形状の中）の上で、ポインタの視線に最も近い点の割合（0〜1）。
    fn closest_fraction(&self, a: Vec3, b: Vec3, ray: &Ray) -> Option<f32> {
        let wa = self.world.transform_point3(a);
        let wb = self.world.transform_point3(b);
        let segment = wb - wa;
        let dir = ray.dir.try_normalize()?;
        let length_sq = segment.length_squared();
        if !length_sq.is_finite() || length_sq <= 0.0 {
            return None;
        }
        let w = wa - ray.origin;
        let b_dot = segment.dot(dir);
        let denom = length_sq - b_dot * b_dot;
        let fraction = if denom > length_sq * 1.0e-8 {
            (b_dot * dir.dot(w) - segment.dot(w)) / denom
        } else {
            // 線分が視線とほぼ平行。手前の端を取る。
            if (wa - self.eye).dot(self.forward) <= (wb - self.eye).dot(self.forward) {
                0.0
            } else {
                1.0
            }
        };
        fraction.is_finite().then(|| fraction.clamp(0.0, 1.0))
    }
}

/// 三角形の3つの頂点の位置（形状の中）。番号が外れていれば `None`。
fn triangle_positions(prim: &Primitive, triangle: u32) -> Option<[Vec3; 3]> {
    let base = triangle as usize * 3;
    let corner = |k: usize| -> Option<Vec3> {
        let index = *prim.indices.get(base + k)? as usize;
        prim.vertices
            .get(index)
            .map(|vertex| Vec3::from(vertex.position))
    };
    Some([corner(0)?, corner(1)?, corner(2)?])
}

fn face_normal(corners: [Vec3; 3]) -> Vec3 {
    (corners[1] - corners[0]).cross(corners[2] - corners[0])
}

/// 面積が0（か、計算できない）三角形。
fn is_degenerate(corners: [Vec3; 3]) -> bool {
    let normal = face_normal(corners);
    let area = normal.length_squared();
    !area.is_finite() || area <= 0.0
}

/// 位置をビットで比べるための鍵。`-0.0` は `0.0` とみなす。
fn position_key(point: Vec3) -> [u32; 3] {
    point.to_array().map(|value| (value + 0.0).to_bits())
}

/// 向きを問わない辺の鍵。
fn edge_key(a: Vec3, b: Vec3) -> [[u32; 3]; 2] {
    let (ka, kb) = (position_key(a), position_key(b));
    if ka <= kb { [ka, kb] } else { [kb, ka] }
}

/// 辺の判定。両端の位置が同じ辺を持つ三角形（面積0を除く）を、対象の三角形の中から探す。
/// 相手が0個（縁）か2個以上（枝分かれ）なら辺。1個なら、2つの面の角度が
/// [`FEATURE_ANGLE_DEG`] 以上のときだけ辺。頂点の並びが逆向きの相手は、法線を裏返して比べる。
fn is_feature_edge(prim: &Primitive, triangles: &[u32], owner: u32, ends: [Vec3; 2]) -> bool {
    let Some(owner_corners) = triangle_positions(prim, owner) else {
        return false;
    };
    let (ka, kb) = (position_key(ends[0]), position_key(ends[1]));
    let mut neighbor: Option<Vec3> = None;
    let mut count = 0usize;
    for &other in triangles {
        if other == owner {
            continue;
        }
        let Some(corners) = triangle_positions(prim, other) else {
            continue;
        };
        if is_degenerate(corners) {
            continue;
        }
        let keys = corners.map(position_key);
        for side in 0..3 {
            let (pa, pb) = (keys[side], keys[(side + 1) % 3]);
            let normal = face_normal(corners);
            if pa == ka && pb == kb {
                // 同じ向きにたどる相手は、面の向きが逆にそろっている。
                neighbor = Some(-normal);
            } else if pa == kb && pb == ka {
                neighbor = Some(normal);
            } else {
                continue;
            }
            count += 1;
            break;
        }
        if count >= 2 {
            return true;
        }
    }
    match (count, neighbor) {
        (1, Some(n2)) => {
            let n1 = face_normal(owner_corners);
            let limit = FEATURE_ANGLE_DEG.to_radians().cos() * n1.length() * n2.length();
            let dot = n1.dot(n2);
            !dot.is_finite() || !limit.is_finite() || dot < limit
        }
        _ => true,
    }
}
