//! ハンドルの計算。形と当たり、軸の移動、回転の角度、拡大、平行な視線、差分から `local`、
//! 分解と合成。透視と平行の両方で確かめる。

use glam::{Affine3A, Mat3, Quat, Vec2, Vec3};

use model_reviewer_core::camera::{OrbitCamera, Projection, ViewDir};
use model_reviewer_core::gizmo::{
    Drag, GizmoFrame, Handle, Readout, Tool, Trs, apply_world_delta, compose, decompose, euler_deg,
    from_euler_deg, hit, layout,
};

const VIEWPORT: [f32; 2] = [800.0, 600.0];
const ASPECT: f32 = 800.0 / 600.0;
const WORLD_AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];
const BOTH: [Projection; 2] = [Projection::Perspective, Projection::Orthographic];

fn camera(projection: Projection, yaw_deg: f32, pitch_deg: f32) -> OrbitCamera {
    OrbitCamera {
        target: Vec3::new(1.0, 0.5, -0.5),
        yaw: yaw_deg.to_radians(),
        pitch: pitch_deg.to_radians(),
        distance: 6.0,
        fov_y: 40.0_f32.to_radians(),
        projection,
    }
}

fn frame(camera: OrbitCamera, axes: [Vec3; 3]) -> GizmoFrame {
    GizmoFrame {
        pivot: Vec3::new(1.0, 0.5, -0.5),
        axes,
        camera,
        viewport_px: VIEWPORT,
        size_px: 90.0,
    }
}

fn ndc_of(frame: &GizmoFrame, world: Vec3) -> Vec2 {
    frame.camera.project(world, ASPECT).expect("in front")
}

fn px_of(frame: &GizmoFrame, world: Vec3) -> Vec2 {
    let ndc = ndc_of(frame, world);
    Vec2::new(
        (ndc.x + 1.0) * 0.5 * VIEWPORT[0],
        (1.0 - ndc.y) * 0.5 * VIEWPORT[1],
    )
}

fn px_to_ndc(px: Vec2) -> Vec2 {
    Vec2::new(
        px.x / VIEWPORT[0] * 2.0 - 1.0,
        1.0 - px.y / VIEWPORT[1] * 2.0,
    )
}

fn close(actual: f32, expected: f32, eps: f32) {
    assert!(
        (actual - expected).abs() <= eps,
        "{actual} vs {expected} (eps {eps})"
    );
}

fn close3(actual: Vec3, expected: Vec3, eps: f32) {
    assert!(
        (actual - expected).length() <= eps,
        "{actual:?} vs {expected:?} (eps {eps})"
    );
}

fn close_affine(actual: &Affine3A, expected: &Affine3A, eps: f32) {
    assert!(
        actual.abs_diff_eq(*expected, eps),
        "{actual:?}\nvs\n{expected:?}"
    );
}

fn handles(tool: Tool, frame: &GizmoFrame) -> Vec<Handle> {
    let mut found: Vec<Handle> = layout(tool, frame).iter().map(|s| s.handle).collect();
    found.dedup();
    found
}

#[test]
fn layout_depends_on_the_tool_and_skips_axes_along_the_view() {
    for projection in BOTH {
        let angled = frame(camera(projection, 35.0, 25.0), WORLD_AXES);
        assert_eq!(
            handles(Tool::Move, &angled),
            [Handle::MoveX, Handle::MoveY, Handle::MoveZ]
        );
        assert_eq!(
            handles(Tool::Rotate, &angled),
            [Handle::RotX, Handle::RotY, Handle::RotZ]
        );
        assert_eq!(
            handles(Tool::Scale, &angled),
            [
                Handle::ScaleX,
                Handle::ScaleY,
                Handle::ScaleZ,
                Handle::ScaleAll
            ]
        );
        for tool in [Tool::Select, Tool::Shape, Tool::Measure, Tool::Annotate] {
            assert!(layout(tool, &angled).is_empty(), "{tool:?}");
        }
        for shape in layout(Tool::Rotate, &angled) {
            assert!(shape.closed);
            assert_eq!(shape.points.len(), 64);
        }

        // 正面から見ると Z 軸は点になるので、移動と拡大の Z は出さない。輪は出す。
        let mut front_camera = camera(projection, 0.0, 0.0);
        front_camera.snap(ViewDir::Front);
        let front = frame(front_camera, WORLD_AXES);
        assert_eq!(handles(Tool::Move, &front), [Handle::MoveX, Handle::MoveY]);
        assert_eq!(
            handles(Tool::Scale, &front),
            [Handle::ScaleX, Handle::ScaleY, Handle::ScaleAll]
        );
        assert_eq!(handles(Tool::Rotate, &front).len(), 3);

        // 矢印の長さは画面上 size_px。
        let shapes = layout(Tool::Move, &front);
        let pivot = px_of(&front, front.pivot);
        let tip = shapes
            .iter()
            .filter(|shape| shape.handle == Handle::MoveX && shape.closed)
            .map(|shape| shape.points[0])
            .next()
            .unwrap();
        close(tip.distance(pivot), front.size_px, 0.5);
        assert!(tip.x > pivot.x, "X points right in the front view");
    }
}

#[test]
fn hit_picks_the_nearest_handle_within_the_tolerance() {
    for projection in BOTH {
        let mut front_camera = camera(projection, 0.0, 0.0);
        front_camera.snap(ViewDir::Front);
        let front = frame(front_camera, WORLD_AXES);
        let pivot = px_of(&front, front.pivot);
        let moves = layout(Tool::Move, &front);
        assert_eq!(
            hit(&moves, pivot + Vec2::new(60.0, 0.0), 12.0),
            Some(Handle::MoveX)
        );
        // 画面の y は下向きなので、+Y の矢印は上にある。
        assert_eq!(
            hit(&moves, pivot + Vec2::new(4.0, -50.0), 12.0),
            Some(Handle::MoveY)
        );
        // 矢じりの内側。
        assert_eq!(
            hit(&moves, pivot + Vec2::new(85.0, 1.0), 0.0),
            Some(Handle::MoveX)
        );
        assert_eq!(hit(&moves, pivot + Vec2::new(60.0, 30.0), 12.0), None);
        assert_eq!(hit(&moves, pivot + Vec2::new(-60.0, 0.0), 12.0), None);
        assert_eq!(hit(&moves, Vec2::splat(f32::NAN), 12.0), None);

        let scales = layout(Tool::Scale, &front);
        assert_eq!(hit(&scales, pivot, 12.0), Some(Handle::ScaleAll));
        assert_eq!(
            hit(&scales, pivot + Vec2::new(90.0, 0.0), 2.0),
            Some(Handle::ScaleX)
        );

        // 輪は縁だけが当たる（内側は素通り）。正面から見た Z の輪は円。
        let rings = layout(Tool::Rotate, &front);
        let edge = Vec2::new(1.0, 1.0).normalize() * front.size_px;
        assert_eq!(hit(&rings, pivot + edge, 3.0), Some(Handle::RotZ));
        assert_eq!(hit(&rings, pivot + Vec2::new(30.0, 40.0), 3.0), None);
    }
}

#[test]
fn moving_along_an_axis_changes_only_that_axis() {
    for projection in BOTH {
        let frame = frame(camera(projection, 35.0, 25.0), WORLD_AXES);
        for (index, handle) in [Handle::MoveX, Handle::MoveY, Handle::MoveZ]
            .into_iter()
            .enumerate()
        {
            let axis = WORLD_AXES[index];
            let start = ndc_of(&frame, frame.pivot + axis * 0.3);
            let drag = Drag::begin(handle, &frame, start).expect("axis is not along the view");
            assert_eq!(drag.handle(), handle);
            let state = drag
                .update(ndc_of(&frame, frame.pivot + axis * 0.8))
                .unwrap();
            close3(state.world_delta.translation.into(), axis * 0.5, 1.0e-3);
            assert!(
                state
                    .world_delta
                    .matrix3
                    .abs_diff_eq(glam::Mat3A::IDENTITY, 0.0)
            );
            match state.readout {
                Readout::Distance(value) => close(value, 0.5, 1.0e-3),
                other => panic!("{other:?}"),
            }
            // 軸から外れた所へ動かしても、動くのは軸の向きだけ。
            let off = drag.update(start + Vec2::new(0.2, 0.15)).unwrap();
            let moved: Vec3 = off.world_delta.translation.into();
            close(moved.cross(axis).length(), 0.0, 1.0e-4);
            assert!(moved.length() > 1.0e-3);
            // 掴んだ所から動かさなければ差分は無い。
            let still = drag.update(start).unwrap();
            close_affine(&still.world_delta, &Affine3A::IDENTITY, 1.0e-4);
        }
    }
}

#[test]
fn view_along_the_axis_gives_none() {
    for projection in BOTH {
        let mut front_camera = camera(projection, 0.0, 0.0);
        front_camera.snap(ViewDir::Front);
        let front = frame(front_camera, WORLD_AXES);
        let pivot = ndc_of(&front, front.pivot);
        let near = pivot + Vec2::new(0.05, 0.02);
        assert!(Drag::begin(Handle::MoveZ, &front, near).is_none());
        assert!(Drag::begin(Handle::ScaleZ, &front, near).is_none());
        // X の輪の面は視線と平行（真横から見た輪）。
        assert!(Drag::begin(Handle::RotX, &front, near).is_none());

        // 掴んだ後に視線と平行になる位置は透視でしか起きないので、透視だけで確かめる。
        if projection == Projection::Perspective {
            let side = frame(camera(projection, 35.0, 0.0), WORLD_AXES);
            let start = ndc_of(&side, side.pivot + Vec3::X * 0.2);
            let drag = Drag::begin(Handle::MoveX, &side, start).unwrap();
            // X 軸の消失点（目から −X の向きの点）は、視線と X 軸が平行になる画面上の位置。
            let vanishing = side
                .camera
                .project(side.camera.eye() - Vec3::X, ASPECT)
                .expect("-X is in front of this camera");
            assert!(drag.update(vanishing).is_none());
            assert!(drag.update(vanishing + Vec2::new(0.3, 0.0)).is_some());
        }
    }
}

#[test]
fn rotation_angle_follows_the_pointer_around_the_pivot() {
    for projection in BOTH {
        let mut front_camera = camera(projection, 0.0, 0.0);
        front_camera.snap(ViewDir::Front);
        let front = frame(front_camera, WORLD_AXES);
        let pivot = px_of(&front, front.pivot);
        let right = px_to_ndc(pivot + Vec2::new(70.0, 0.0));
        let above = px_to_ndc(pivot + Vec2::new(0.0, -70.0));
        let drag = Drag::begin(Handle::RotZ, &front, right).unwrap();
        let state = drag.update(above).unwrap();
        // 画面で反時計回りに90°。手前（+Z）から見て反時計回りなので +90°。
        match state.readout {
            Readout::AngleDeg(value) => close(value, 90.0, 0.05),
            other => panic!("{other:?}"),
        }
        // 中心は動かず、+X の向きが +Y へ回る。
        close3(
            state.world_delta.transform_point3(front.pivot),
            front.pivot,
            1.0e-5,
        );
        close3(
            state.world_delta.transform_vector3(Vec3::X),
            Vec3::Y,
            1.0e-3,
        );
        // 画面上で45°なら45°。
        let diagonal = px_to_ndc(pivot + Vec2::new(40.0, 40.0));
        match drag.update(diagonal).unwrap().readout {
            Readout::AngleDeg(value) => close(value, -45.0, 0.05),
            other => panic!("{other:?}"),
        }

        // 斜めから見た Y の輪でも、輪の上の点をたどると角度が合う。
        let angled = frame(camera(projection, 35.0, 30.0), WORLD_AXES);
        let on_ring = |deg: f32| {
            let (s, c) = deg.to_radians().sin_cos();
            // Y まわりに +deg 回した X の向き（Y の正の向きから見て反時計回り）。
            angled.pivot + Vec3::new(c, 0.0, -s) * 0.4
        };
        let drag = Drag::begin(Handle::RotY, &angled, ndc_of(&angled, on_ring(10.0))).unwrap();
        let state = drag.update(ndc_of(&angled, on_ring(70.0))).unwrap();
        match state.readout {
            Readout::AngleDeg(value) => close(value, 60.0, 0.05),
            other => panic!("{other:?}"),
        }
        let expected = Quat::from_rotation_y(60.0_f32.to_radians());
        close3(
            state.world_delta.transform_vector3(Vec3::X),
            expected * Vec3::X,
            1.0e-3,
        );
    }
}

#[test]
fn scaling_along_a_part_axis_and_uniformly_keeps_the_pivot() {
    let tilt = Quat::from_rotation_y(0.6) * Quat::from_rotation_x(-0.3);
    let part_axes = [tilt * Vec3::X, tilt * Vec3::Y, tilt * Vec3::Z];
    for projection in BOTH {
        let frame = frame(camera(projection, 35.0, 25.0), part_axes);
        let axis = part_axes[0];
        let drag = Drag::begin(
            Handle::ScaleX,
            &frame,
            ndc_of(&frame, frame.pivot + axis * 0.5),
        )
        .unwrap();
        let state = drag
            .update(ndc_of(&frame, frame.pivot + axis * 1.0))
            .unwrap();
        match state.readout {
            Readout::Factor(value) => close(value, 2.0, 1.0e-3),
            other => panic!("{other:?}"),
        }
        let delta = state.world_delta;
        close3(delta.transform_point3(frame.pivot), frame.pivot, 1.0e-4);
        close3(delta.transform_vector3(axis), axis * 2.0, 2.0e-3);
        close3(delta.transform_vector3(part_axes[1]), part_axes[1], 2.0e-3);
        close3(delta.transform_vector3(part_axes[2]), part_axes[2], 2.0e-3);
        // 中心をまたいでも 0 や裏返しにならない。
        let crossed = drag
            .update(ndc_of(&frame, frame.pivot - axis * 0.5))
            .unwrap();
        match crossed.readout {
            Readout::Factor(value) => assert!(value > 0.0),
            other => panic!("{other:?}"),
        }
        assert!(crossed.world_delta.matrix3.determinant() > 0.0);
        // 中心の近くを掴むと値が定まらない。
        assert!(Drag::begin(Handle::ScaleX, &frame, ndc_of(&frame, frame.pivot)).is_none());

        let pivot = px_of(&frame, frame.pivot);
        let all = Drag::begin(
            Handle::ScaleAll,
            &frame,
            px_to_ndc(pivot + Vec2::new(20.0, 0.0)),
        )
        .unwrap();
        let state = all
            .update(px_to_ndc(pivot + Vec2::new(0.0, -40.0)))
            .unwrap();
        match state.readout {
            Readout::Factor(value) => close(value, 2.0, 1.0e-3),
            other => panic!("{other:?}"),
        }
        close3(
            state.world_delta.transform_point3(frame.pivot),
            frame.pivot,
            1.0e-4,
        );
        close3(
            state.world_delta.transform_vector3(Vec3::ONE),
            Vec3::splat(2.0),
            1.0e-3,
        );
        assert!(Drag::begin(Handle::ScaleAll, &frame, px_to_ndc(pivot)).is_none());
    }
}

#[test]
fn world_delta_becomes_a_new_local_under_the_parent() {
    let parent = Affine3A::from_scale_rotation_translation(
        Vec3::splat(2.0),
        Quat::from_rotation_z(0.4),
        Vec3::new(3.0, -1.0, 2.0),
    );
    let local = Affine3A::from_scale_rotation_translation(
        Vec3::new(1.0, 0.5, 1.5),
        Quat::from_rotation_x(0.9),
        Vec3::new(0.5, 0.25, 0.0),
    );
    let world = parent * local;
    let delta = Affine3A::from_translation(Vec3::new(1.0, 0.0, 0.0))
        * Affine3A::from_axis_angle(Vec3::Y, 0.3);
    let next = apply_world_delta(&parent, &world, &delta).unwrap();
    close_affine(&(parent * next), &(delta * world), 1.0e-5);
    // 根は単位行列を親として渡す。
    let root = apply_world_delta(&Affine3A::IDENTITY, &world, &delta).unwrap();
    close_affine(&root, &(delta * world), 1.0e-5);
    // 逆にできない親や有限でない値は None。
    let flat = Affine3A::from_scale(Vec3::new(1.0, 0.0, 1.0));
    assert!(apply_world_delta(&flat, &world, &delta).is_none());
    let broken = Affine3A::from_translation(Vec3::new(f32::NAN, 0.0, 0.0));
    assert!(apply_world_delta(&parent, &world, &broken).is_none());
}

#[test]
fn decompose_and_compose_round_trip_and_detect_skewed_scale() {
    let trs = Trs {
        translation: Vec3::new(1.5, -2.0, 0.25),
        rotation: from_euler_deg(Vec3::new(20.0, -35.0, 70.0)),
        scale: Vec3::new(1.0, 2.5, 0.5),
    };
    let local = compose(&trs);
    let back = decompose(&local).expect("plain TRS");
    close3(back.translation, trs.translation, 1.0e-6);
    close3(back.scale, trs.scale, 1.0e-5);
    assert!(back.rotation.dot(trs.rotation).abs() > 1.0 - 1.0e-6);
    close_affine(&compose(&back), &local, 1.0e-5);
    assert_eq!(decompose(&Affine3A::IDENTITY), Some(Trs::default()));

    // 裏返し（行列式が負）も分けられる。
    let mirrored = Affine3A::from_scale(Vec3::new(-1.0, 1.0, 1.0));
    let split = decompose(&mirrored).unwrap();
    close_affine(&compose(&split), &mirrored, 1.0e-6);

    // 傾いた拡大（せん断、回転の間に挟まった不均一な拡大）は分けられない。
    let shear = Affine3A::from_mat3(Mat3::from_cols(Vec3::X, Vec3::new(0.5, 1.0, 0.0), Vec3::Z));
    assert!(decompose(&shear).is_none());
    let skewed = Affine3A::from_quat(Quat::from_rotation_z(0.5))
        * Affine3A::from_scale(Vec3::new(3.0, 1.0, 1.0))
        * Affine3A::from_quat(Quat::from_rotation_z(0.7));
    assert!(decompose(&skewed).is_none());
    assert!(decompose(&Affine3A::from_scale(Vec3::new(1.0, 0.0, 1.0))).is_none());
    assert!(decompose(&Affine3A::from_translation(Vec3::splat(f32::INFINITY))).is_none());
    // 小さい部品でも、大きさに比べた誤差で判定する。
    let tiny = compose(&Trs {
        scale: Vec3::splat(1.0e-4),
        ..trs
    });
    assert!(decompose(&tiny).is_some());
}

#[test]
fn euler_angles_are_extrinsic_xyz_in_degrees() {
    for deg in [
        Vec3::ZERO,
        Vec3::new(30.0, 0.0, 0.0),
        Vec3::new(0.0, -45.0, 0.0),
        Vec3::new(10.0, 20.0, 30.0),
        Vec3::new(-170.0, 60.0, 120.0),
    ] {
        let rotation = from_euler_deg(deg);
        close3(euler_deg(rotation), deg, 1.0e-3);
        // ワールドの X、次に Y、次に Z のまわり（行列では Rz × Ry × Rx）。
        let expected = Quat::from_rotation_z(deg.z.to_radians())
            * Quat::from_rotation_y(deg.y.to_radians())
            * Quat::from_rotation_x(deg.x.to_radians());
        assert!(rotation.dot(expected).abs() > 1.0 - 1.0e-6, "{deg:?}");
    }
    assert_eq!(from_euler_deg(Vec3::splat(f32::NAN)), Quat::IDENTITY);
    assert_eq!(
        euler_deg(Quat::from_xyzw(f32::NAN, 0.0, 0.0, 1.0)),
        Vec3::ZERO
    );
}
