//! 吸着。立方体の角・辺・面、平らな面の対角線、縁、輪郭のすぐ外、断面、拡大と回転、
//! 頂点を共有しない形状。

use glam::{Affine3A, Quat, Vec2, Vec3};

use model_reviewer_core::camera::{OrbitCamera, Projection};
use model_reviewer_core::math::Aabb;
use model_reviewer_core::pick::PickIndex;
use model_reviewer_core::scene::{
    Draw, Model, ModelStats, Node, NodeId, PrimId, Primitive, Vertex,
};
use model_reviewer_core::section::{Axis, SectionPlane};
use model_reviewer_core::snap::{SnapKind, SnapQuery, SnapSettings, SnapTarget, snap};
use model_reviewer_core::texture::Textures;
use model_reviewer_core::visibility::Visibility;

const VIEWPORT: [f32; 2] = [800.0, 800.0];

fn vertex(position: Vec3) -> Vertex {
    Vertex {
        position: position.to_array(),
        normal: [0.0, 0.0, 1.0],
    }
}

fn primitive(vertices: Vec<Vertex>, indices: Vec<u32>) -> Primitive {
    let mut local_aabb = Aabb::empty();
    for vertex in &vertices {
        let point = Vec3::from(vertex.position);
        local_aabb = local_aabb.union(Aabb {
            min: point,
            max: point,
        });
    }
    Primitive {
        vertices,
        indices,
        local_aabb,
        has_normals: false,
    }
}

/// 中心が原点で一辺2の立方体。面は外向き。各面は2つの三角形で、対角線は面の
/// (−u,−v) の角から (+u,+v) の角へ通る。`shared = true` なら面ごとに4頂点（法線ごとに
/// 頂点を分けた形）、`false` なら三角形ごとに3頂点（頂点を共有しない STL 相当）。
fn cube(shared: bool) -> Primitive {
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
            if sign < 0.0 {
                std::mem::swap(&mut u, &mut v);
            }
            let corners = [n - u - v, n + u - v, n + u + v, n - u + v];
            if shared {
                let base = vertices.len() as u32;
                vertices.extend(corners.map(vertex));
                indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            } else {
                for k in [0, 1, 2, 0, 2, 3] {
                    indices.push(vertices.len() as u32);
                    vertices.push(vertex(corners[k]));
                }
            }
        }
    }
    primitive(vertices, indices)
}

/// z = 0 の平らな正方形（一辺2、+Z 向き）。外周は隣の無い縁。
fn plate() -> Primitive {
    let corners = [
        Vec3::new(-1.0, -1.0, 0.0),
        Vec3::new(1.0, -1.0, 0.0),
        Vec3::new(1.0, 1.0, 0.0),
        Vec3::new(-1.0, 1.0, 0.0),
    ];
    primitive(corners.map(vertex).to_vec(), vec![0, 1, 2, 0, 2, 3])
}

fn model_of(prim: Primitive, world: Affine3A) -> Model {
    let world_aabb = prim.local_aabb.transformed(&world);
    Model {
        textures: Textures::default(),
        nodes: vec![Node {
            name: "n0".into(),
            gltf_index: 0,
            parent: None,
            subtree_end: 1,
            depth: 0,
            local: world,
            world,
            draws: 0..1,
            world_aabb,
        }],
        draws: vec![Draw {
            prim: PrimId(0),
            base_color: [1.0; 4],
            tex: None,
        }],
        primitives: vec![prim],
        bounds: world_aabb,
        stats: ModelStats::default(),
        warnings: Vec::new(),
    }
}

/// +Z 側から原点を見るカメラ。
fn front_camera(projection: Projection) -> OrbitCamera {
    OrbitCamera {
        target: Vec3::ZERO,
        yaw: 0.0,
        pitch: 0.0,
        distance: 10.0,
        projection,
        ..OrbitCamera::default()
    }
}

struct Scene {
    model: Model,
    index: PickIndex,
    vis: Visibility,
}

impl Scene {
    fn new(prim: Primitive, world: Affine3A) -> Self {
        let model = model_of(prim, world);
        let index = PickIndex::build(&model);
        let vis = Visibility::all_visible(1);
        Self { model, index, vis }
    }

    fn snap(
        &self,
        camera: &OrbitCamera,
        ndc: Vec2,
        enabled: bool,
        clip: Option<SectionPlane>,
    ) -> Option<SnapTarget> {
        let query = SnapQuery {
            camera,
            aspect: VIEWPORT[0] / VIEWPORT[1],
            viewport_px: VIEWPORT,
            ndc,
            settings: SnapSettings {
                enabled,
                radius_px: 12.0,
            },
            clip,
        };
        snap(&self.model, &self.index, &self.vis, &query)
    }
}

/// ワールドの点の画面上の位置を、画面の画素で `(dx, dy)` だけずらした正規化座標
/// （+dy は上）。
fn ndc_of(camera: &OrbitCamera, world: Vec3, dx: f32, dy: f32) -> Vec2 {
    let ndc = camera.project(world, 1.0).expect("画面に映る点");
    ndc + Vec2::new(dx * 2.0 / VIEWPORT[0], dy * 2.0 / VIEWPORT[1])
}

fn close3(actual: Vec3, expected: Vec3) -> bool {
    (actual - expected).length() <= 1.0e-4 * (1.0 + expected.length())
}

fn expect(target: Option<SnapTarget>, kind: SnapKind) -> SnapTarget {
    let target = target.unwrap_or_else(|| panic!("{kind:?} の候補が無い"));
    assert_eq!(target.kind, kind, "{target:?}");
    target
}

#[test]
fn snaps_to_corner_edge_and_face() {
    for shared in [true, false] {
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let scene = Scene::new(cube(shared), Affine3A::IDENTITY);
            let camera = front_camera(projection);
            let corner = Vec3::new(1.0, 1.0, 1.0);

            // 角の3画素内側 → 頂点。
            let target = expect(
                scene.snap(&camera, ndc_of(&camera, corner, -3.0, -3.0), true, None),
                SnapKind::Vertex,
            );
            assert_eq!(target.local, corner);
            assert!(close3(target.world, corner));
            assert_eq!(target.node, NodeId(0));
            assert_eq!(target.edge, None);

            // 上の辺の3画素下 → 辺。点は辺の上にある。
            let on_top = Vec3::new(0.3, 1.0, 1.0);
            let target = expect(
                scene.snap(&camera, ndc_of(&camera, on_top, 0.0, -3.0), true, None),
                SnapKind::Edge,
            );
            assert!(close3(target.world, on_top), "{:?}", target.world);
            assert_eq!(target.local.y, 1.0);
            assert_eq!(target.local.z, 1.0);
            let [a, b] = target.edge.unwrap();
            assert!(close3(a, Vec3::new(-1.0, 1.0, 1.0)) || close3(b, Vec3::new(-1.0, 1.0, 1.0)));
            assert!(close3(a, corner) || close3(b, corner));

            // 面の中（対角線と縁から離れた所） → 面。
            let inside = Vec3::new(0.4, -0.3, 1.0);
            let target = expect(
                scene.snap(&camera, ndc_of(&camera, inside, 0.0, 0.0), true, None),
                SnapKind::Face,
            );
            assert!(close3(target.world, inside), "{:?}", target.world);

            // 吸着を切ると、角の近くでも面。
            let target = expect(
                scene.snap(&camera, ndc_of(&camera, corner, -3.0, -3.0), false, None),
                SnapKind::Face,
            );
            assert!(!close3(target.world, corner));
            assert!((target.world.z - 1.0).abs() < 1.0e-4);
        }
    }
}

#[test]
fn flat_diagonal_is_not_an_edge() {
    for shared in [true, false] {
        let scene = Scene::new(cube(shared), Affine3A::IDENTITY);
        let camera = front_camera(Projection::Perspective);
        // 前の面の対角線（y = x）の上と、そのすぐ脇。
        for (dx, dy) in [(0.0, 0.0), (2.0, -2.0), (-2.0, 2.0)] {
            let target = expect(
                scene.snap(
                    &camera,
                    ndc_of(&camera, Vec3::new(0.3, 0.3, 1.0), dx, dy),
                    true,
                    None,
                ),
                SnapKind::Face,
            );
            assert!((target.world.z - 1.0).abs() < 1.0e-4);
        }
    }
}

#[test]
fn open_border_is_an_edge() {
    let scene = Scene::new(plate(), Affine3A::IDENTITY);
    let camera = front_camera(Projection::Perspective);
    let on_border = Vec3::new(1.0, -0.2, 0.0);
    let target = expect(
        scene.snap(&camera, ndc_of(&camera, on_border, -3.0, 0.0), true, None),
        SnapKind::Edge,
    );
    assert!(close3(target.world, on_border), "{:?}", target.world);
    assert_eq!(target.local.x, 1.0);
    // 平らな板の対角線には付かない。
    expect(
        scene.snap(
            &camera,
            ndc_of(&camera, Vec3::new(0.2, 0.2, 0.0), 1.0, -1.0),
            true,
            None,
        ),
        SnapKind::Face,
    );
}

#[test]
fn snaps_from_just_outside_the_silhouette() {
    for shared in [true, false] {
        let scene = Scene::new(cube(shared), Affine3A::IDENTITY);
        let camera = front_camera(Projection::Perspective);
        let corner = Vec3::new(1.0, 1.0, 1.0);
        // 角の4画素右・2画素下。ポインタの視線は立方体に当たらない。
        let ndc = ndc_of(&camera, corner, 4.0, -2.0);
        assert!(
            scene.snap(&camera, ndc, false, None).is_none(),
            "吸着なしでは当たらない"
        );
        let target = expect(scene.snap(&camera, ndc, true, None), SnapKind::Vertex);
        assert_eq!(target.local, corner);

        // 右の辺の4画素外側 → 辺。
        let on_right = Vec3::new(1.0, -0.4, 1.0);
        let target = expect(
            scene.snap(&camera, ndc_of(&camera, on_right, 4.0, 0.0), true, None),
            SnapKind::Edge,
        );
        assert_eq!(target.local.x, 1.0);

        // 輪郭から遠い所は何も無い（面の候補も返さない）。
        assert!(
            scene
                .snap(&camera, ndc_of(&camera, on_right, 40.0, 0.0), true, None)
                .is_none()
        );
    }
}

#[test]
fn cut_vertices_are_not_candidates() {
    let scene = Scene::new(plate(), Affine3A::IDENTITY);
    let camera = front_camera(Projection::Orthographic);
    let corner = Vec3::new(1.0, 1.0, 0.0);
    let ndc = ndc_of(&camera, corner, -3.0, -3.0);
    // 断面なしなら角に付く。
    let target = expect(scene.snap(&camera, ndc, true, None), SnapKind::Vertex);
    assert_eq!(target.local, corner);
    // x > 0.995 を消す。当たった点は残す側にあり、角と右の縁は消える側にある。
    let clip = SectionPlane {
        axis: Axis::X,
        offset: 0.995,
        flipped: false,
    };
    let target = scene
        .snap(&camera, ndc, true, Some(clip))
        .expect("残った面には当たる");
    assert_ne!(target.kind, SnapKind::Vertex, "{target:?}");
    assert!(!clip.cuts(target.world), "{target:?}");
    if let Some([a, b]) = target.edge {
        assert!(
            (a.y - 1.0).abs() < 1.0e-4 && (b.y - 1.0).abs() < 1.0e-4,
            "上の縁だけ"
        );
    }
    // 全部を消すと何も無い。
    let all = SectionPlane {
        axis: Axis::Z,
        offset: -1.0,
        flipped: false,
    };
    assert!(scene.snap(&camera, ndc, true, Some(all)).is_none());
}

#[test]
fn snap_local_equals_vertex_under_scale_and_rotation() {
    let world = Affine3A::from_scale_rotation_translation(
        Vec3::new(2.0, 3.0, 0.5),
        Quat::from_euler(glam::EulerRot::YXZ, 0.6, 0.4, 0.2),
        Vec3::new(0.5, -1.0, 2.0),
    );
    for shared in [true, false] {
        for projection in [Projection::Perspective, Projection::Orthographic] {
            let scene = Scene::new(cube(shared), world);
            let camera = OrbitCamera {
                target: world.translation.into(),
                distance: 20.0,
                ..front_camera(projection)
            };
            // 目にいちばん近い角（必ず見えている）。
            let eye = camera.eye();
            let corner = cube(true)
                .vertices
                .iter()
                .map(|v| Vec3::from(v.position))
                .min_by(|a, b| {
                    let da = world.transform_point3(*a).distance(eye);
                    let db = world.transform_point3(*b).distance(eye);
                    da.total_cmp(&db)
                })
                .unwrap();
            let corner_world = world.transform_point3(corner);
            let center = camera.project(world.translation.into(), 1.0).unwrap();
            let at = camera.project(corner_world, 1.0).unwrap();
            // 角から立方体の中心の方へ2画素。
            let toward =
                (center - at).normalize() * Vec2::new(2.0 / VIEWPORT[0], 2.0 / VIEWPORT[1]) * 2.0;
            let target = expect(
                scene.snap(&camera, at + toward, true, None),
                SnapKind::Vertex,
            );
            assert_eq!(target.local, corner, "{shared} {projection:?}");
            assert!(close3(target.world, corner_world));
        }
    }
}

#[test]
fn non_finite_and_degenerate_queries_do_not_panic() {
    let scene = Scene::new(cube(true), Affine3A::IDENTITY);
    let camera = front_camera(Projection::Perspective);
    for ndc in [Vec2::splat(f32::NAN), Vec2::new(f32::INFINITY, 0.0)] {
        assert!(scene.snap(&camera, ndc, true, None).is_none());
    }
    let query = SnapQuery {
        camera: &camera,
        aspect: 1.0,
        viewport_px: [0.0, 0.0],
        ndc: Vec2::ZERO,
        settings: SnapSettings::default(),
        clip: None,
    };
    assert!(snap(&scene.model, &scene.index, &scene.vis, &query).is_none());
    // 縮みきった部品でも落ちない（頂点だけを見る）。
    let flat = Scene::new(cube(true), Affine3A::from_scale(Vec3::new(1.0, 1.0, 0.0)));
    let _ = flat.snap(&camera, Vec2::ZERO, true, None);
}
