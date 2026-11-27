//! 当たり判定。総当たりとの一致、変換、非表示、中止、構築時間。

use std::time::Instant;

use glam::{Affine3A, Quat, Vec3};

use model_reviewer_core::camera::{OrbitCamera, Projection};
use model_reviewer_core::math::{Aabb, Ray};
use model_reviewer_core::pick::{Hit, PickIndex, pick, pick_clipped};
use model_reviewer_core::scene::{
    Draw, Model, ModelStats, Node, NodeId, PrimId, Primitive, Vertex,
};
use model_reviewer_core::section::{Axis, SectionPlane};
use model_reviewer_core::visibility::Visibility;

fn vertex(position: [f32; 3]) -> Vertex {
    Vertex {
        position,
        normal: [0.0, 0.0, 1.0],
    }
}

/// 原点の三角形と、光線が外れる位置の埋め草。葉の上限8を超えて木が分かれる。
fn plate() -> Primitive {
    let mut vertices = vec![
        vertex([0.0, 0.0, 0.0]),
        vertex([1.0, 0.0, 0.0]),
        vertex([0.0, 1.0, 0.0]),
    ];
    let mut indices = vec![0, 1, 2];
    for i in 0..12 {
        let base = vertices.len() as u32;
        let x = 2.0 + i as f32 * 0.3;
        vertices.push(vertex([x, 0.0, 10.0]));
        vertices.push(vertex([x + 0.2, 0.0, 10.0]));
        vertices.push(vertex([x, 0.2, 10.0]));
        indices.extend([base, base + 1, base + 2]);
    }
    primitive(vertices, indices)
}

fn primitive(vertices: Vec<Vertex>, indices: Vec<u32>) -> Primitive {
    let mut local_aabb = Aabb::empty();
    for vertex in &vertices {
        let point = Vec3::from(vertex.position);
        if point.is_finite() {
            local_aabb = local_aabb.union(Aabb {
                min: point,
                max: point,
            });
        }
    }
    Primitive {
        vertices,
        indices,
        local_aabb,
        has_normals: true,
    }
}

fn model_of(prim: Primitive, worlds: &[Affine3A]) -> Model {
    let draws = worlds
        .iter()
        .map(|_| Draw {
            prim: PrimId(0),
            base_color: [1.0, 1.0, 1.0, 1.0],
        })
        .collect();
    let nodes = worlds
        .iter()
        .enumerate()
        .map(|(index, world)| Node {
            name: format!("n{index}"),
            gltf_index: index as u32,
            parent: None,
            subtree_end: index as u32 + 1,
            depth: 0,
            local: *world,
            world: *world,
            draws: (index as u32)..(index as u32 + 1),
            world_aabb: prim.local_aabb.transformed(world),
        })
        .collect();
    Model {
        nodes,
        draws,
        primitives: vec![prim],
        bounds: Aabb::empty(),
        stats: ModelStats::default(),
        warnings: Vec::new(),
    }
}

fn close(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 1.0e-3,
        "{actual} vs {expected}"
    );
}

fn close3(actual: Vec3, expected: Vec3) {
    assert!(
        (actual - expected).length() <= 1.0e-3,
        "{actual:?} vs {expected:?}"
    );
}

fn world_ray(world: Affine3A, local_origin: Vec3, local_dir: Vec3) -> (Ray, f32) {
    let origin = world.transform_point3(local_origin);
    let mapped = world.transform_vector3(local_dir);
    let length = mapped.length();
    (
        Ray {
            origin,
            dir: mapped / length,
        },
        length,
    )
}

fn expect_local_hit(world: Affine3A, from_positive_z: bool) {
    let model = model_of(plate(), &[world]);
    let index = PickIndex::build(&model);
    let vis = Visibility::all_visible(model.nodes.len());
    let travel = 4.0;
    let (origin_z, dir_z) = if from_positive_z {
        (travel, -1.0)
    } else {
        (-travel, 1.0)
    };
    let local_hit = Vec3::new(0.2, 0.3, 0.0);
    let (ray, stretch) = world_ray(
        world,
        Vec3::new(0.2, 0.3, origin_z),
        Vec3::new(0.0, 0.0, dir_z),
    );
    let hit = pick(&model, &index, &vis, &ray).expect("hit");
    assert_eq!(hit.node, NodeId(0));
    close(hit.t, travel * stretch);
    close3(hit.local_pos, local_hit);
    close3(hit.local_normal, Vec3::Z);
    close3(hit.world_pos, world.transform_point3(local_hit));
    close3(hit.world_pos, ray.origin + ray.dir * hit.t);
}

#[test]
fn transformed_parts_report_node_local_point_and_distance() {
    expect_local_hit(Affine3A::IDENTITY, true);
    expect_local_hit(Affine3A::IDENTITY, false);
    let rotated =
        Affine3A::from_translation(Vec3::new(1.5, -2.0, 3.0)) * Affine3A::from_rotation_y(0.7);
    expect_local_hit(rotated, true);
    let scaled = Affine3A::from_scale_rotation_translation(
        Vec3::new(2.0, 0.5, 3.0),
        Quat::from_rotation_y(0.4),
        Vec3::new(-3.0, 1.0, 2.0),
    );
    expect_local_hit(scaled, true);
    let mirrored = Affine3A::from_scale_rotation_translation(
        Vec3::new(-1.5, 2.0, 0.8),
        Quat::from_rotation_x(0.3),
        Vec3::new(0.5, 2.0, -4.0),
    );
    expect_local_hit(mirrored, true);
    expect_local_hit(mirrored, false);

    let model = model_of(plate(), &[Affine3A::IDENTITY]);
    let index = PickIndex::build(&model);
    let vis = Visibility::all_visible(1);
    let miss = Ray {
        origin: Vec3::new(5.0, 5.0, 5.0),
        dir: Vec3::new(0.0, 0.0, -1.0),
    };
    assert!(pick(&model, &index, &vis, &miss).is_none());
    let degenerate = Ray {
        origin: Vec3::ZERO,
        dir: Vec3::ZERO,
    };
    assert!(pick(&model, &index, &vis, &degenerate).is_none());
}

#[test]
fn nearer_differently_scaled_part_wins_until_it_is_hidden() {
    let far = Affine3A::from_scale(Vec3::new(4.0, 4.0, 1.0));
    let near = Affine3A::from_scale_rotation_translation(
        Vec3::new(0.5, 0.5, 1.0),
        Quat::IDENTITY,
        Vec3::new(0.0, 0.0, 6.0),
    );
    let model = model_of(plate(), &[far, near]);
    let index = PickIndex::build(&model);
    let mut vis = Visibility::all_visible(model.nodes.len());
    let ray = Ray {
        origin: Vec3::new(0.1, 0.15, 10.0),
        dir: Vec3::new(0.0, 0.0, -1.0),
    };
    let hit = pick(&model, &index, &vis, &ray).expect("near");
    assert_eq!(hit.node, NodeId(1));
    close(hit.t, 4.0);
    close3(hit.local_pos, Vec3::new(0.2, 0.3, 0.0));

    vis.set(&model, NodeId(1), false);
    let hit = pick(&model, &index, &vis, &ray).expect("far");
    assert_eq!(hit.node, NodeId(0));
    close(hit.t, 10.0);
    close3(hit.local_pos, Vec3::new(0.025, 0.0375, 0.0));

    vis.show_all();
    vis.set(&model, NodeId(0), false);
    vis.set(&model, NodeId(1), false);
    assert!(pick(&model, &index, &vis, &ray).is_none());
}

#[test]
fn a_part_without_an_inverse_is_skipped() {
    let prim = plate();
    let mut model = model_of(
        prim,
        &[
            Affine3A::from_scale(Vec3::ZERO),
            Affine3A::from_translation(Vec3::ZERO),
        ],
    );
    model.nodes[0].world_aabb = Aabb {
        min: Vec3::new(-1.0, -1.0, 5.0),
        max: Vec3::new(1.0, 1.0, 7.0),
    };
    let index = PickIndex::build(&model);
    let vis = Visibility::all_visible(model.nodes.len());
    let ray = Ray {
        origin: Vec3::new(0.2, 0.3, 8.0),
        dir: Vec3::new(0.0, 0.0, -1.0),
    };
    let hit = pick(&model, &index, &vis, &ray).expect("the invertible part");
    assert_eq!(hit.node, NodeId(1));
    close(hit.t, 8.0);
    close3(hit.local_pos, Vec3::new(0.2, 0.3, 0.0));
}

#[test]
fn bad_indices_and_non_finite_vertices_do_not_panic() {
    let vertices = vec![
        vertex([0.0, 0.0, 0.0]),
        vertex([1.0, 0.0, 0.0]),
        vertex([0.0, 1.0, 0.0]),
        vertex([f32::NAN, 0.0, 0.0]),
        vertex([2.0, 0.0, 0.0]),
        vertex([2.0, 1.0, 0.0]),
    ];
    let indices = vec![0, 1, 2, 0, 1, 99, 3, 4, 5];
    let model = model_of(primitive(vertices, indices), &[Affine3A::IDENTITY]);
    let index = PickIndex::build(&model);
    let vis = Visibility::all_visible(1);
    let ray = Ray {
        origin: Vec3::new(0.2, 0.3, 3.0),
        dir: Vec3::new(0.0, 0.0, -1.0),
    };
    let hit = pick(&model, &index, &vis, &ray).expect("valid triangle");
    close3(hit.local_pos, Vec3::new(0.2, 0.3, 0.0));
}

#[test]
fn build_can_be_cancelled_between_and_inside_primitives() {
    let tiny = model_of(plate(), &[Affine3A::IDENTITY]);
    assert!(PickIndex::build_cancellable(&tiny, &|| true).is_none());
    assert!(PickIndex::build_cancellable(&tiny, &|| false).is_some());

    let heavy = model_of(grid(64), &[Affine3A::IDENTITY]);
    assert!(heavy.primitives[0].indices.len() / 3 > 4096);
    let calls = std::cell::Cell::new(0u32);
    let cancelled = PickIndex::build_cancellable(&heavy, &|| {
        let next = calls.get() + 1;
        calls.set(next);
        next >= 3
    });
    assert!(cancelled.is_none(), "calls={}", calls.get());
    assert!(calls.get() >= 3);

    let index = PickIndex::build(&tiny);
    let vis = Visibility::all_visible(tiny.nodes.len());
    let ray = Ray {
        origin: Vec3::new(0.2, 0.3, 2.0),
        dir: Vec3::new(0.0, 0.0, -1.0),
    };
    assert!(pick(&tiny, &index, &vis, &ray).is_some());
}

#[test]
fn bvh_matches_a_fixed_seed_brute_force() {
    let worlds = [
        Affine3A::IDENTITY,
        Affine3A::from_translation(Vec3::new(0.0, 40.0, 0.0)) * Affine3A::from_rotation_y(0.6),
        Affine3A::from_scale_rotation_translation(
            Vec3::new(2.0, 0.5, 3.0),
            Quat::from_rotation_z(0.2),
            Vec3::new(40.0, 0.0, 0.0),
        ),
        Affine3A::from_scale_rotation_translation(
            Vec3::new(-1.5, 2.0, 0.8),
            Quat::from_rotation_x(-0.4),
            Vec3::new(0.0, 0.0, 40.0),
        ),
    ];
    let model = model_of(plate(), &worlds);
    let index = PickIndex::build(&model);
    let vis = Visibility::all_visible(model.nodes.len());
    let mut rays = Vec::new();
    for world in worlds {
        let (ray, _) = world_ray(world, Vec3::new(0.25, 0.2, 5.0), Vec3::new(0.0, 0.0, -1.0));
        rays.push(ray);
        let (ray, _) = world_ray(world, Vec3::new(0.7, 0.15, -3.0), Vec3::new(0.0, 0.0, 1.0));
        rays.push(ray);
    }
    let mut lcg = Lcg(0xA2_01_3D);
    for _ in 0..64 {
        let origin = Vec3::new(
            lcg.range(-5.0, 50.0),
            lcg.range(-5.0, 50.0),
            lcg.range(-5.0, 50.0),
        );
        let dir = Vec3::new(
            lcg.range(-1.0, 1.0),
            lcg.range(-1.0, 1.0),
            lcg.range(-1.0, 1.0),
        );
        if dir.length_squared() < 1.0e-6 {
            continue;
        }
        rays.push(Ray {
            origin,
            dir: dir.normalize(),
        });
    }
    for ray in &rays {
        match (pick(&model, &index, &vis, ray), brute(&model, &vis, ray)) {
            (None, None) => {}
            (Some(got), Some(want)) => {
                assert_eq!(got.node, want.node, "ray {ray:?}");
                close(got.t, want.t);
                close3(got.local_pos, want.local_pos);
            }
            (got, want) => panic!("ray {ray:?}\n bvh {got:?}\n brute {want:?}"),
        }
    }
}

#[test]
#[ignore = "数百万三角形の計測"]
fn two_million_triangles_build_within_ten_seconds() {
    let cells = 1000;
    let triangles = cells * cells * 2;
    let model = model_of(grid(cells), &[Affine3A::IDENTITY]);
    let started = Instant::now();
    let index = PickIndex::build(&model);
    let elapsed = started.elapsed();
    println!(
        "pick index build: triangles={triangles} elapsed_ms={:.1}",
        elapsed.as_secs_f64() * 1000.0
    );
    assert!(
        elapsed.as_secs_f64() < 10.0,
        "build took {elapsed:?} for {triangles} triangles"
    );
    let vis = Visibility::all_visible(1);
    let ray = Ray {
        origin: Vec3::new(500.2, 500.3, 10.0),
        dir: Vec3::new(0.0, 0.0, -1.0),
    };
    let hit = pick(&model, &index, &vis, &ray).expect("grid hit");
    assert_eq!(hit.node, NodeId(0));
    close(hit.t, 10.0);
}

fn grid(cells: usize) -> Primitive {
    let n = cells + 1;
    let mut vertices = Vec::with_capacity(n * n);
    for y in 0..n {
        for x in 0..n {
            vertices.push(vertex([x as f32, y as f32, 0.0]));
        }
    }
    let mut indices = Vec::with_capacity(cells * cells * 6);
    for y in 0..cells {
        for x in 0..cells {
            let i = (y * n + x) as u32;
            let row = n as u32;
            indices.extend([i, i + 1, i + row, i + 1, i + row + 1, i + row]);
        }
    }
    primitive(vertices, indices)
}

/// Numerical Recipes の線形合同法。種は固定する。
struct Lcg(u32);

impl Lcg {
    fn next_u32(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }

    fn range(&mut self, min: f32, max: f32) -> f32 {
        let unit = (self.next_u32() >> 8) as f32 / (1_u32 << 24) as f32;
        min + (max - min) * unit
    }
}

fn brute(model: &Model, vis: &Visibility, ray: &Ray) -> Option<Hit> {
    if !ray.origin.is_finite() || !ray.dir.is_finite() || ray.dir.length_squared() <= 1.0e-20 {
        return None;
    }
    let mut best: Option<Hit> = None;
    for (index, node) in model.nodes.iter().enumerate() {
        let id = NodeId(index as u32);
        if !vis.effective(id) || node.draws.is_empty() || node.world_aabb.is_empty() {
            continue;
        }
        let Some(inverse) = invert(node.world) else {
            continue;
        };
        let origin = inverse.transform_point3(ray.origin);
        let dir = inverse.transform_vector3(ray.dir);
        if !origin.is_finite() || !dir.is_finite() || dir.length_squared() <= 1.0e-20 {
            continue;
        }
        for draw_index in node.draws.clone() {
            let draw = &model.draws[draw_index as usize];
            let prim = &model.primitives[draw.prim.0 as usize];
            let triangles = prim.indices.len() / 3;
            for triangle in 0..triangles {
                let Some((t, local_pos, normal)) = intersect(origin, dir, prim, triangle) else {
                    continue;
                };
                if best.as_ref().is_some_and(|hit| t >= hit.t) {
                    continue;
                }
                best = Some(Hit {
                    node: id,
                    t,
                    world_pos: ray.origin + ray.dir * t,
                    local_pos,
                    local_normal: normal,
                    prim: draw.prim,
                    triangle: triangle as u32,
                });
            }
        }
    }
    best
}

fn invert(world: Affine3A) -> Option<Affine3A> {
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

fn intersect(
    origin: Vec3,
    dir: Vec3,
    prim: &Primitive,
    triangle: usize,
) -> Option<(f32, Vec3, Vec3)> {
    let base = triangle * 3;
    let v0 = Vec3::from(prim.vertices[prim.indices[base] as usize].position);
    let v1 = Vec3::from(prim.vertices[prim.indices[base + 1] as usize].position);
    let v2 = Vec3::from(prim.vertices[prim.indices[base + 2] as usize].position);
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
    if !t.is_finite() || t < 1.0e-6 {
        return None;
    }
    let normal = e1.cross(e2);
    let normal = if normal.length_squared() > 0.0 && normal.is_finite() {
        normal.normalize()
    } else {
        Vec3::Z
    };
    let local_pos = v0 * (1.0 - u - v) + v1 * u + v2 * v;
    if local_pos.is_finite() {
        Some((t, local_pos, normal))
    } else {
        None
    }
}

/// 中心が原点で一辺2の、閉じた箱（各面2つの三角形。面の並びは +X・−X・+Y・−Y・+Z・−Z）。
fn closed_box() -> Primitive {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for axis in 0..3 {
        for sign in [1.0f32, -1.0] {
            let mut n = Vec3::ZERO;
            n[axis] = sign;
            let mut u = Vec3::ZERO;
            let mut v = Vec3::ZERO;
            u[(axis + 1) % 3] = 1.0;
            v[(axis + 2) % 3] = 1.0;
            let base = vertices.len() as u32;
            for corner in [n - u - v, n + u - v, n + u + v, n - u + v] {
                vertices.push(vertex(corner.to_array()));
            }
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    primitive(vertices, indices)
}

fn front_cameras() -> [OrbitCamera; 2] {
    [Projection::Perspective, Projection::Orthographic].map(|projection| OrbitCamera {
        target: Vec3::ZERO,
        yaw: 0.0,
        pitch: 0.0,
        distance: 10.0,
        projection,
        ..OrbitCamera::default()
    })
}

#[test]
fn clipped_ray_hits_the_far_wall() {
    let model = model_of(closed_box(), &[Affine3A::IDENTITY]);
    let index = PickIndex::build(&model);
    let vis = Visibility::all_visible(1);
    let bounds = closed_box().local_aabb;
    // z > 0 を消す。前の壁（z = 1）は消え、奥の壁（z = −1）の内側に当たる。
    let clip = SectionPlane {
        axis: Axis::Z,
        offset: 0.0,
        flipped: false,
    };
    for camera in front_cameras() {
        for ndc in [glam::Vec2::ZERO, glam::Vec2::new(0.05, -0.04)] {
            let ray = camera.screen_ray_in(ndc, 1.0, &bounds);
            let plain = pick(&model, &index, &vis, &ray).expect("前の壁に当たる");
            close(plain.world_pos.z, 1.0);
            let hit = pick_clipped(&model, &index, &vis, &ray, Some(&clip)).expect("奥の壁");
            close(hit.world_pos.z, -1.0);
            close3(hit.world_pos, ray.origin + ray.dir * hit.t);
            close3(hit.local_pos, hit.world_pos);
            assert!(!clip.cuts(hit.world_pos));
            assert_eq!(hit.prim, PrimId(0));
            // 当たった三角形は奥の壁（−Z の面。三角形 10・11）。
            assert!(matches!(hit.triangle, 10 | 11), "{}", hit.triangle);

            // 反転すると z < 0 を消す。前の壁に当たる。
            let flipped = SectionPlane {
                flipped: true,
                ..clip
            };
            let front = pick_clipped(&model, &index, &vis, &ray, Some(&flipped)).unwrap();
            close(front.t, plain.t);
            assert_eq!(front.triangle, plain.triangle);

            // 全部が消す側（z > −5 を消す）なら当たりなし。
            let all = SectionPlane {
                offset: -5.0,
                ..clip
            };
            assert!(pick_clipped(&model, &index, &vis, &ray, Some(&all)).is_none());
        }
    }
}

#[test]
fn a_triangle_across_the_plane_is_hit_on_the_kept_side() {
    // z = 0 の三角形が x = 0.5 をまたぐ。
    let model = model_of(plate(), &[Affine3A::IDENTITY]);
    let index = PickIndex::build(&model);
    let vis = Visibility::all_visible(1);
    let clip = SectionPlane {
        axis: Axis::X,
        offset: 0.5,
        flipped: false,
    };
    let kept = Ray {
        origin: Vec3::new(0.2, 0.2, 5.0),
        dir: Vec3::NEG_Z,
    };
    let hit = pick_clipped(&model, &index, &vis, &kept, Some(&clip)).expect("残す側の点");
    close(hit.t, 5.0);
    close3(hit.world_pos, Vec3::new(0.2, 0.2, 0.0));
    assert_eq!(hit.triangle, 0);
    let cut = Ray {
        origin: Vec3::new(0.7, 0.2, 5.0),
        dir: Vec3::NEG_Z,
    };
    assert!(pick(&model, &index, &vis, &cut).is_some());
    assert!(pick_clipped(&model, &index, &vis, &cut, Some(&clip)).is_none());
}

#[test]
fn clip_none_equals_pick() {
    let worlds = [
        Affine3A::IDENTITY,
        Affine3A::from_scale_rotation_translation(
            Vec3::new(2.0, 0.5, 3.0),
            Quat::from_rotation_z(0.2),
            Vec3::new(3.0, 0.0, 1.0),
        ),
    ];
    let model = model_of(grid(12), &worlds);
    let index = PickIndex::build(&model);
    let vis = Visibility::all_visible(model.nodes.len());
    let far = SectionPlane {
        axis: Axis::Z,
        offset: 100.0,
        flipped: false,
    };
    let mut lcg = Lcg(0x5EC7);
    let mut hits = 0;
    for _ in 0..300 {
        let origin = Vec3::new(
            lcg.range(-2.0, 14.0),
            lcg.range(-2.0, 14.0),
            lcg.range(2.0, 9.0),
        );
        let dir = Vec3::new(lcg.range(-0.5, 0.5), lcg.range(-0.5, 0.5), -1.0).normalize();
        let ray = Ray { origin, dir };
        let a = pick(&model, &index, &vis, &ray);
        let b = pick_clipped(&model, &index, &vis, &ray, None);
        match (a, b) {
            (None, None) => {}
            (Some(a), Some(b)) => {
                hits += 1;
                assert_eq!(a.node, b.node);
                assert_eq!(a.t.to_bits(), b.t.to_bits());
                assert_eq!(a.world_pos, b.world_pos);
                assert_eq!(a.local_pos, b.local_pos);
                assert_eq!(a.local_normal, b.local_normal);
                assert_eq!((a.prim, a.triangle), (b.prim, b.triangle));
            }
            other => panic!("{other:?}"),
        }
        // 何も消さない位置の平面でも、当たる部品と三角形は同じ。
        let c = pick_clipped(&model, &index, &vis, &ray, Some(&far));
        assert_eq!(
            a.map(|h| (h.node, h.triangle)),
            c.map(|h| (h.node, h.triangle))
        );
        if let (Some(a), Some(c)) = (a, c) {
            close(a.t, c.t);
        }
    }
    assert!(hits > 50, "{hits}");
}
