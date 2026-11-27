//! カメラ。`fit` の8隅、射影と光線の逆、背面、カーソルズーム、仰角±90°、平行投影、決まった向き、格子の間隔。

use glam::{Vec2, Vec3, Vec4Swizzles};

use model_reviewer_core::camera::{
    DISTANCE_MAX_RATIO, DISTANCE_MIN_RATIO, ORBIT_DEG_PER_PX, OrbitCamera, Projection, ViewDir,
    ZOOM_EXP_PER_PX, grid_spacing,
};
use model_reviewer_core::math::Aabb;

fn bounds() -> Aabb {
    Aabb {
        min: Vec3::new(-1.0, -0.5, -2.0),
        max: Vec3::new(2.0, 1.5, 0.5),
    }
}

fn corners(bounds: Aabb) -> [Vec3; 8] {
    let (min, max) = (bounds.min, bounds.max);
    [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(min.x, max.y, max.z),
        Vec3::new(max.x, max.y, max.z),
    ]
}

fn close(actual: f32, expected: f32, eps: f32) {
    assert!(
        (actual - expected).abs() <= eps,
        "{actual} vs {expected} (eps {eps})"
    );
}

fn close2(actual: Vec2, expected: Vec2, eps: f32) {
    assert!(
        (actual - expected).length() <= eps,
        "{actual:?} vs {expected:?}"
    );
}

fn close3(actual: Vec3, expected: Vec3, eps: f32) {
    assert!(
        (actual - expected).length() <= eps,
        "{actual:?} vs {expected:?}"
    );
}

fn assert_corners_inside(camera: &OrbitCamera, bounds: &Aabb, aspect: f32) {
    for corner in corners(*bounds) {
        let ndc = camera
            .project(corner, aspect)
            .unwrap_or_else(|| panic!("corner {corner:?} is behind the camera"));
        assert!(
            ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0,
            "corner {corner:?} projects to {ndc:?} at aspect {aspect}"
        );
    }
}

#[test]
fn published_interaction_constants() {
    assert_eq!(ORBIT_DEG_PER_PX, 0.4);
    assert_eq!(ZOOM_EXP_PER_PX, 0.002);
    assert_eq!(DISTANCE_MIN_RATIO, 1.0e-3);
    assert_eq!(DISTANCE_MAX_RATIO, 1.0e3);
}

#[test]
fn fit_keeps_orientation_and_places_every_corner_on_screen() {
    let bounds = bounds();
    let aspects = [1.0, 16.0 / 9.0, 0.5];
    for aspect in aspects {
        let mut camera = OrbitCamera {
            yaw: 0.8,
            pitch: -0.35,
            fov_y: 50.0_f32.to_radians(),
            ..OrbitCamera::default()
        };
        camera.fit(&bounds, aspect);
        close(camera.yaw, 0.8, 0.0);
        close(camera.pitch, -0.35, 0.0);
        close(camera.fov_y, 50.0_f32.to_radians(), 0.0);
        close3(camera.target, bounds.center(), 1.0e-5);
        assert_corners_inside(&camera, &bounds, aspect);

        let home = OrbitCamera::home(&bounds, aspect);
        close(home.yaw.to_degrees(), 35.0, 1.0e-3);
        close(home.pitch.to_degrees(), 20.0, 1.0e-3);
        close(home.fov_y.to_degrees(), 40.0, 1.0e-3);
        assert_corners_inside(&home, &bounds, aspect);
    }

    let radius = bounds.radius();
    let half = 40.0_f32.to_radians() * 0.5;
    let expected = radius / half.sin() * 1.05;
    close(OrbitCamera::home(&bounds, 1.0).distance, expected, 1.0e-4);
}

#[test]
fn point_bounds_use_unit_distance_and_empty_fit_is_a_no_op() {
    let point = Aabb {
        min: Vec3::ONE,
        max: Vec3::ONE,
    };
    let mut camera = OrbitCamera {
        distance: 3.0,
        ..OrbitCamera::default()
    };
    camera.fit(&point, 1.0);
    close(camera.distance, 1.0, 0.0);
    close3(camera.target, Vec3::ONE, 0.0);
    assert_corners_inside(&camera, &point, 1.0);

    let empty = Aabb::empty();
    let before = OrbitCamera::default();
    let mut fitted = before;
    fitted.fit(&empty, 1.6);
    assert_eq!(fitted, before);
    let home = OrbitCamera::home(&empty, 1.6);
    close3(home.target, Vec3::ZERO, 0.0);
    close(home.distance, 1.0, 0.0);
}

#[test]
fn project_and_screen_ray_are_inverses_and_match_the_gpu_matrix() {
    let bounds = bounds();
    let mut cameras = Vec::new();
    cameras.push(OrbitCamera::home(&bounds, 16.0 / 9.0));
    let mut pitched = OrbitCamera::home(&bounds, 0.5);
    pitched.pitch = 89.0_f32.to_radians();
    pitched.yaw = -1.2;
    cameras.push(pitched);

    for camera in &cameras {
        for aspect in [1.0_f32, 16.0 / 9.0, 0.5] {
            for ndc in [
                Vec2::ZERO,
                Vec2::new(0.35, -0.22),
                Vec2::new(-0.8, 0.55),
                Vec2::new(0.15, 0.9),
            ] {
                let ray = camera.screen_ray(ndc, aspect);
                let point = ray.origin + ray.dir * (camera.distance * 0.5);
                let back = camera.project(point, aspect).expect("point in front");
                close2(back, ndc, 2.0e-4);
            }

            for corner in corners(bounds) {
                let Some(ndc) = camera.project(corner, aspect) else {
                    continue;
                };
                let ray = camera.screen_ray(ndc, aspect);
                let travel = (corner - ray.origin).dot(ray.dir);
                assert!(travel > 0.0, "ray does not face {corner:?}");
                close3(ray.origin + ray.dir * travel, corner, 2.0e-4);

                let clip = camera.proj(aspect, &bounds) * camera.view() * corner.extend(1.0);
                assert!(clip.w > 0.0, "matrix puts {corner:?} behind");
                close2(Vec2::new(clip.x / clip.w, clip.y / clip.w), ndc, 2.0e-4);
            }
        }
    }
}

#[test]
fn points_behind_the_camera_do_not_project() {
    let bounds = bounds();
    let camera = OrbitCamera::home(&bounds, 1.6);
    let eye = camera.eye();
    let forward = (camera.target - eye).normalize();
    assert!(camera.project(eye, 1.6).is_none());
    assert!(camera.project(eye - forward, 1.6).is_none());
    assert!(camera.project(eye - forward * 4.0, 1.6).is_none());
    assert!(camera.project(Vec3::new(f32::NAN, 0.0, 0.0), 1.6).is_none());

    let mut steep = camera;
    steep.pitch = 89.0_f32.to_radians();
    let eye = steep.eye();
    let forward = (steep.target - eye).normalize();
    assert!(steep.project(eye - forward * 2.0, 1.6).is_none());
}

#[test]
fn zoom_toward_the_cursor_keeps_the_point_under_the_cursor() {
    let bounds = bounds();
    let aspect = 1.6;
    let ndc = Vec2::new(0.35, -0.22);
    let mut camera = OrbitCamera::home(&bounds, aspect);
    camera.yaw = 0.4;
    camera.pitch = -0.25;
    let point = point_on_target_plane(&camera, ndc, aspect);
    let before = camera.project(point, aspect).expect("cursor point");
    close2(before, ndc, 2.0e-4);

    let distance = camera.distance;
    let target = camera.target;
    camera.zoom(0.4, Some(ndc), aspect, (1.0e-3, 1.0e6));
    assert!(camera.distance < distance);
    assert!(camera.target.distance(target) > 1.0e-4);
    let after = camera.project(point, aspect).expect("after zoom in");
    close2(after, ndc, 2.0e-4);

    camera.zoom(2.5, Some(ndc), aspect, (1.0e-3, 1.0e6));
    let widened = camera.project(point, aspect).expect("after zoom out");
    close2(widened, ndc, 2.0e-4);

    let parked = camera;
    camera.zoom(0.5, None, aspect, (1.0e-3, 1.0e6));
    close(camera.distance, parked.distance * 0.5, 1.0e-4);
    close3(camera.target, parked.target, 0.0);
    camera.zoom(f32::NAN, Some(ndc), aspect, (1.0e-3, 1.0e6));
    camera.zoom(0.0, Some(ndc), aspect, (1.0e-3, 1.0e6));
    camera.zoom(-2.0, Some(ndc), aspect, (1.0e-3, 1.0e6));
    close(camera.distance, parked.distance * 0.5, 1.0e-4);
    close3(camera.target, parked.target, 0.0);
}

#[test]
fn orbit_clamps_pitch_and_pan_moves_with_the_view_axes() {
    let mut camera = OrbitCamera::default();
    camera.orbit(Vec2::new(10.0, 5.0));
    close(camera.yaw, 10.0 * ORBIT_DEG_PER_PX.to_radians(), 1.0e-5);
    close(camera.pitch, 5.0 * ORBIT_DEG_PER_PX.to_radians(), 1.0e-5);
    camera.orbit(Vec2::new(0.0, 10_000.0));
    close(camera.pitch.to_degrees(), 90.0, 1.0e-3);
    camera.orbit(Vec2::new(0.0, -20_000.0));
    close(camera.pitch.to_degrees(), -90.0, 1.0e-3);
    camera.orbit(Vec2::new(f32::NAN, 1.0));
    close(camera.pitch.to_degrees(), -90.0, 1.0e-3);

    let mut camera = OrbitCamera::default();
    let height = 200.0;
    camera.pan(Vec2::new(10.0, -4.0), height);
    let scale = 2.0 * camera.distance * (camera.fov_y * 0.5).tan() / height;
    close3(
        camera.target,
        Vec3::new(10.0 * scale, -4.0 * scale, 0.0),
        1.0e-5,
    );
    camera.pan(Vec2::new(1.0, 1.0), 0.0);
    camera.pan(Vec2::new(f32::NAN, 0.0), height);
    close3(
        camera.target,
        Vec3::new(10.0 * scale, -4.0 * scale, 0.0),
        1.0e-5,
    );
}

fn point_on_target_plane(camera: &OrbitCamera, ndc: Vec2, aspect: f32) -> Vec3 {
    let ray = camera.screen_ray(ndc, aspect);
    let forward = (camera.target - camera.eye()).normalize();
    let denom = ray.dir.dot(forward);
    assert!(
        denom.abs() > 1.0e-6,
        "cursor ray is parallel to the target plane"
    );
    let t = (camera.target - ray.origin).dot(forward) / denom;
    assert!(t > 0.0);
    ray.origin + ray.dir * t
}

fn clip(camera: &OrbitCamera, aspect: f32, scene: &Aabb, point: Vec3) -> Vec3 {
    let clip = camera.proj(aspect, scene) * camera.view() * point.extend(1.0);
    assert!(clip.w > 0.0, "{point:?} has w {}", clip.w);
    clip.xyz() / clip.w
}

#[test]
fn straight_up_and_down_stay_finite_and_keep_orbiting() {
    let bounds = bounds();
    for pitch_deg in [90.0_f32, -90.0] {
        for yaw in [0.0_f32, 0.3, -2.0, 3.1] {
            let mut camera = OrbitCamera::home(&bounds, 1.5);
            camera.yaw = yaw;
            camera.pitch = pitch_deg.to_radians();
            for _ in 0..3 {
                let view = camera.view();
                let proj = camera.proj(1.5, &bounds);
                assert!(view.is_finite() && proj.is_finite(), "{pitch_deg} {yaw}");
                // 視線は真下（または真上）。
                let (right, up, forward) = camera.axes();
                close(forward.y, -pitch_deg.signum(), 1.0e-6);
                close(right.dot(up), 0.0, 1.0e-6);
                close(right.dot(forward), 0.0, 1.0e-6);
                close(up.dot(forward), 0.0, 1.0e-6);
                close(right.cross(up).dot(-forward), 1.0, 1.0e-5);
                // 注視点は画面の中心、目の位置は真上（真下）。
                let center = clip(&camera, 1.5, &bounds, camera.target);
                close2(center.truncate(), Vec2::ZERO, 1.0e-4);
                let eye = camera.eye() - camera.target;
                close(
                    eye.y,
                    camera.distance * pitch_deg.signum(),
                    camera.distance * 1.0e-5,
                );
                for ndc in [Vec2::new(0.3, -0.4), Vec2::new(-0.7, 0.2)] {
                    let ray = camera.screen_ray(ndc, 1.5);
                    assert!(ray.dir.is_finite() && ray.origin.is_finite());
                    let back = camera
                        .project(ray.origin + ray.dir * camera.distance, 1.5)
                        .unwrap();
                    close2(back, ndc, 2.0e-4);
                }
                // 方位の回転を続けても仰角は保たれ、行列は有限のまま。
                camera.orbit(Vec2::new(25.0, pitch_deg.signum() * 50.0));
                close(camera.pitch.to_degrees(), pitch_deg, 1.0e-3);
            }
        }
    }
}

#[test]
fn snap_replaces_only_the_angles() {
    let bounds = bounds();
    let mut camera = OrbitCamera::home(&bounds, 1.2);
    camera.projection = Projection::Orthographic;
    let before = camera;
    // 目の位置が、注視点からどちら側にあるか。
    let cases = [
        (ViewDir::Front, Vec3::Z),
        (ViewDir::Back, Vec3::NEG_Z),
        (ViewDir::Right, Vec3::X),
        (ViewDir::Left, Vec3::NEG_X),
        (ViewDir::Top, Vec3::Y),
        (ViewDir::Bottom, Vec3::NEG_Y),
    ];
    for (dir, side) in cases {
        camera.snap(dir);
        assert_eq!(camera.target, before.target);
        assert_eq!(camera.distance, before.distance);
        assert_eq!(camera.fov_y, before.fov_y);
        assert_eq!(camera.projection, Projection::Orthographic);
        let eye = (camera.eye() - camera.target) / camera.distance;
        close3(eye, side, 1.0e-5);
        assert!(camera.view().is_finite());
    }
    camera.snap(ViewDir::Front);
    close(camera.yaw, 0.0, 0.0);
    close(camera.pitch, 0.0, 0.0);
    camera.snap(ViewDir::Top);
    close(camera.pitch.to_degrees(), 90.0, 1.0e-4);
    // 上面図では、画面の下が前（+Z）。
    let below = camera.project(camera.target + Vec3::Z, 1.2).unwrap();
    assert!(below.y < 0.0 && below.x.abs() < 1.0e-4, "{below:?}");
    // 正面図では +X が画面の右、+Y が上。
    camera.snap(ViewDir::Front);
    let right = camera.project(camera.target + Vec3::X, 1.2).unwrap();
    let up = camera.project(camera.target + Vec3::Y, 1.2).unwrap();
    assert!(right.x > 0.0 && right.y.abs() < 1.0e-5);
    assert!(up.y > 0.0 && up.x.abs() < 1.0e-5);
}

#[test]
fn orthographic_project_and_screen_ray_are_inverses_and_match_the_matrix() {
    let bounds = bounds();
    for (yaw, pitch) in [
        (0.6_f32, 0.3_f32),
        (-1.2, 90.0_f32.to_radians()),
        (2.5, -0.8),
    ] {
        let mut camera = OrbitCamera::home(&bounds, 1.6);
        camera.projection = Projection::Orthographic;
        camera.yaw = yaw;
        camera.pitch = pitch;
        for aspect in [1.0_f32, 16.0 / 9.0, 0.5] {
            for ndc in [Vec2::ZERO, Vec2::new(0.35, -0.22), Vec2::new(-0.8, 0.55)] {
                let ray = camera.screen_ray(ndc, aspect);
                close3(ray.dir, (camera.target - camera.eye()).normalize(), 1.0e-6);
                // 出発点は注視点の面から距離の2048倍手前なので、座標が大きく f32 の丸めが乗る
                // （画面の高さの約 0.03% 以内）。
                for travel in [2048.0_f32, 2049.0, 3000.0] {
                    let point = ray.origin + ray.dir * (camera.distance * travel);
                    let back = camera
                        .project(point, aspect)
                        .expect("ortho always projects");
                    close2(back, ndc, 1.0e-3);
                }
            }
            for corner in corners(bounds) {
                let ndc = camera.project(corner, aspect).unwrap();
                let ray = camera.screen_ray(ndc, aspect);
                let travel = (corner - ray.origin).dot(ray.dir);
                // 出発点は形状より手前にある（目の後ろの形状も当たる）。
                assert!(travel > 0.0);
                close3(
                    ray.origin + ray.dir * travel,
                    corner,
                    camera.distance * 1.0e-3,
                );
                let clipped = clip(&camera, aspect, &bounds, corner);
                close2(clipped.truncate(), ndc, 2.0e-4);
                assert!(
                    (0.0..=1.0).contains(&clipped.z),
                    "depth {} of {corner:?}",
                    clipped.z
                );
            }
        }
    }
}

#[test]
fn orthographic_depth_is_reversed_and_keeps_geometry_behind_the_eye() {
    let bounds = bounds();
    let mut camera = OrbitCamera::home(&bounds, 1.0);
    camera.projection = Projection::Orthographic;
    let (_, _, forward) = camera.axes();
    let near = clip(&camera, 1.0, &bounds, camera.target - forward * 0.5).z;
    let mid = clip(&camera, 1.0, &bounds, camera.target).z;
    let far = clip(&camera, 1.0, &bounds, camera.target + forward * 0.5).z;
    assert!(near > mid && mid > far, "{near} {mid} {far}");
    // 寄りすぎて目が形状の中に入っても、目の後ろの形状は描かれる（深度が 0〜1）。
    camera.distance = bounds.radius() * 0.01;
    let behind = camera.eye() - forward * bounds.radius() * 0.5;
    let depth = clip(&camera, 1.0, &bounds, behind).z;
    assert!((0.0..=1.0).contains(&depth), "{depth}");
    assert!(depth > clip(&camera, 1.0, &bounds, camera.target).z);
    assert!(camera.project(behind, 1.0).is_some());
    // 透視も手前ほど大きい（第1段階のまま）。
    camera.projection = Projection::Perspective;
    camera.distance = OrbitCamera::home(&bounds, 1.0).distance;
    let near = clip(&camera, 1.0, &bounds, camera.target - forward * 0.5).z;
    let far = clip(&camera, 1.0, &bounds, camera.target + forward * 0.5).z;
    assert!(near > far);
    // 空の境界でも有限で、近い面と遠い面が離れている。
    let empty = OrbitCamera {
        projection: Projection::Orthographic,
        ..OrbitCamera::default()
    };
    let proj = empty.proj(1.3, &Aabb::empty());
    assert!(proj.is_finite());
    let depth = clip(&empty, 1.3, &Aabb::empty(), empty.target).z;
    assert!((0.0..=1.0).contains(&depth));
}

#[test]
fn switching_projection_keeps_the_apparent_size_at_the_target() {
    let bounds = bounds();
    let aspect = 1.4;
    let mut camera = OrbitCamera::home(&bounds, aspect);
    camera.yaw = 0.9;
    camera.pitch = -0.4;
    let (right, up, _) = camera.axes();
    let offsets = [right * 0.3, up * -0.45, right * 0.2 + up * 0.1];
    let perspective: Vec<Vec2> = offsets
        .iter()
        .map(|offset| camera.project(camera.target + *offset, aspect).unwrap())
        .collect();
    camera.projection = Projection::Orthographic;
    for (offset, expected) in offsets.iter().zip(&perspective) {
        let ortho = camera.project(camera.target + *offset, aspect).unwrap();
        close2(ortho, *expected, 1.0e-5);
    }
    // パンの量も同じ（画面の1画素あたりの移動が注視点の面で一致する）。
    let mut panned = camera;
    panned.pan(Vec2::new(30.0, 0.0), 300.0);
    let moved = camera.project(panned.target, aspect).unwrap();
    close(moved.x, 30.0 / 300.0 * 2.0 / aspect, 1.0e-4);
}

#[test]
fn orthographic_zoom_keeps_the_point_under_the_cursor() {
    let bounds = bounds();
    let aspect = 1.6;
    let ndc = Vec2::new(-0.4, 0.3);
    let mut camera = OrbitCamera::home(&bounds, aspect);
    camera.projection = Projection::Orthographic;
    let point = point_on_target_plane(&camera, ndc, aspect);
    camera.zoom(0.5, Some(ndc), aspect, (1.0e-3, 1.0e6));
    close2(camera.project(point, aspect).unwrap(), ndc, 2.0e-4);
    camera.zoom(3.0, Some(ndc), aspect, (1.0e-3, 1.0e6));
    close2(camera.project(point, aspect).unwrap(), ndc, 2.0e-4);
}

#[test]
fn grid_spacing_follows_the_distance_in_powers_of_ten() {
    assert_eq!(grid_spacing(5.0), 1.0);
    assert_eq!(grid_spacing(49.0), 1.0);
    assert_eq!(grid_spacing(50.0), 10.0);
    assert_eq!(grid_spacing(4.9), 0.1);
    close(grid_spacing(0.5), 0.1, 1.0e-7);
    close(grid_spacing(0.049), 0.001, 1.0e-9);
    assert_eq!(grid_spacing(5.0e6), 1.0e6);
    for bad in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(grid_spacing(bad), 1.0);
    }
    assert!(grid_spacing(f32::MIN_POSITIVE).is_finite());
    assert!(grid_spacing(f32::MAX) > 0.0);
}

#[test]
fn orthographic_ray_in_scene_starts_just_before_the_bounds_on_the_same_line() {
    let scene = Aabb {
        min: glam::Vec3::new(-1.0, 0.0, -2.0),
        max: glam::Vec3::new(3.0, 2.0, 1.0),
    };
    let mut camera = OrbitCamera::home(&scene, 1.5);
    camera.yaw = 0.7;
    camera.pitch = 0.4;
    let ndc = glam::Vec2::new(0.3, -0.2);
    // 透視では同じ視線。
    let a = camera.screen_ray(ndc, 1.5);
    let b = camera.screen_ray_in(ndc, 1.5, &scene);
    assert_eq!((a.origin, a.dir), (b.origin, b.dir));
    camera.projection = Projection::Orthographic;
    let far = camera.screen_ray(ndc, 1.5);
    let near = camera.screen_ray_in(ndc, 1.5, &scene);
    assert_eq!(far.dir, near.dir);
    // 同じ直線の上で、出発点は境界の全部の角より手前、しかも境界の大きさの桁に収まる。
    let offset = near.origin - far.origin;
    assert!(offset.cross(far.dir).length() < 1.0e-2 * offset.length());
    for i in 0..8 {
        let corner = glam::Vec3::new(
            if i & 1 == 0 { scene.min.x } else { scene.max.x },
            if i & 2 == 0 { scene.min.y } else { scene.max.y },
            if i & 4 == 0 { scene.min.z } else { scene.max.z },
        );
        assert!((corner - near.origin).dot(near.dir) > 0.0);
    }
    assert!((near.origin - scene.center()).length() < 4.0 * scene.radius());
    // 投影した点へ戻る（画面上の位置が同じ）。
    let hit = near.origin + near.dir * (scene.center() - near.origin).dot(near.dir);
    let back = camera.project(hit, 1.5).unwrap();
    assert!((back - ndc).length() < 1.0e-4, "{back}");
    // 空の場面なら従来どおり。
    let empty = camera.screen_ray_in(ndc, 1.5, &Aabb::empty());
    assert_eq!((empty.origin, empty.dir), (far.origin, far.dir));
}
