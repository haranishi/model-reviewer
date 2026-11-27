//! 格子・表示の切り替え・変形中の行列の試験（GPUが要る）。画像を読み戻して画素で確かめ、
//! `target/tmp/render-gpu/` にPNGで残す。

mod common;

use std::sync::Arc;

use common::{
    EditedScene, cube, device, device_with_lines, differing_pixels, model, node, out_dir, trs,
    write_png,
};
use glam::{Affine3A, Mat4, Quat, Vec3};
use model_reviewer_core::scene::Model;
use model_reviewer_core::visibility::Visibility;
use model_reviewer_render::{
    DisplayMode, DragPreview, FrameParams, GridParams, Image, SceneRenderer,
};

const RED: [f32; 4] = [0.8, 0.05, 0.04, 1.0];
const YELLOW: [f32; 4] = [0.9, 0.8, 0.1, 1.0];
const GREEN: [f32; 4] = [0.08, 0.6, 0.12, 1.0];
const GREY: [f32; 4] = [0.45, 0.45, 0.45, 1.0];
const CLEAR: [f32; 4] = [0.016, 0.0185, 0.0242, 1.0];
const SIZE: [u32; 2] = [320, 200];
/// `FrameUniform` の大きさ。変形中に毎フレーム送るのはこれだけ。
const FRAME_BYTES: u64 = 304;

fn perspective(eye: Vec3, target: Vec3, up: Vec3, size: [u32; 2]) -> FrameParams {
    let aspect = size[0] as f32 / size[1] as f32;
    let view = glam::camera::rh::view::look_at_mat4(eye, target, up);
    let proj = glam::camera::rh::proj::directx::perspective_infinite_reverse(0.7, aspect, 0.05);
    params(proj * view, eye, size)
}

/// 平行投影。反転Zにするため、近い面と遠い面の引数を入れ替える。
fn orthographic(
    eye: Vec3,
    target: Vec3,
    up: Vec3,
    half_height: f32,
    size: [u32; 2],
) -> FrameParams {
    let aspect = size[0] as f32 / size[1] as f32;
    let hw = half_height * aspect;
    let view = glam::camera::rh::view::look_at_mat4(eye, target, up);
    let proj = glam::camera::rh::proj::directx::orthographic(
        -hw,
        hw,
        -half_height,
        half_height,
        100.0,
        0.05,
    );
    params(proj * view, eye, size)
}

fn params(view_proj: Mat4, eye: Vec3, size: [u32; 2]) -> FrameParams {
    FrameParams {
        size_px: size,
        view_proj,
        eye,
        selected: None,
        clear: CLEAR,
        display: DisplayMode::Faces,
        grid: GridParams::default(),
        drag: None,
    }
}

fn with_grid(mut f: FrameParams) -> FrameParams {
    f.grid = GridParams {
        spacing: 1.0,
        visible: true,
    };
    f
}

fn to_pixel(f: &FrameParams, p: Vec3) -> (u32, u32) {
    let c = f.view_proj * p.extend(1.0);
    let n = c.truncate() / c.w;
    let x = (n.x * 0.5 + 0.5) * f.size_px[0] as f32;
    let y = (0.5 - n.y * 0.5) * f.size_px[1] as f32;
    (x as u32, y as u32)
}

fn upload_all(
    renderer: &mut SceneRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    m: &Arc<Model>,
) {
    renderer
        .begin_upload(device, Arc::clone(m))
        .expect("転送を始められる");
    while !renderer.continue_upload(queue, 1 << 20).done {}
    renderer.set_instances(device, queue, m, &Visibility::all_visible(m.nodes.len()));
}

fn draw(
    renderer: &mut SceneRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    f: &FrameParams,
) -> Image {
    let _ = renderer.render(device, queue, f);
    renderer.read_image(device, queue).expect("読み戻せる")
}

fn save(image: &Image, name: &str) {
    write_png(
        &out_dir().join(name),
        image.width,
        image.height,
        &image.rgba,
    );
}

fn is_background(p: [u8; 4]) -> bool {
    (i32::from(p[0]) - 34).abs() <= 3
        && (i32::from(p[1]) - 37).abs() <= 3
        && (i32::from(p[2]) - 43).abs() <= 3
}

fn count(image: &Image, pred: impl Fn(u32, u32, [u8; 4]) -> bool) -> usize {
    let mut n = 0;
    for y in 0..image.height {
        for x in 0..image.width {
            if pred(x, y, image.pixel(x, y)) {
                n += 1;
            }
        }
    }
    n
}

/// 周りの3×3がすべて背景でない画素（部品の内側）。縁の多重サンプルの混ざりを避ける。
fn interior(image: &Image, x: u32, y: u32) -> bool {
    if x == 0 || y == 0 || x + 1 >= image.width || y + 1 >= image.height {
        return false;
    }
    (y - 1..=y + 1).all(|yy| (x - 1..=x + 1).all(|xx| !is_background(image.pixel(xx, yy))))
}

fn is_x_axis(p: [u8; 4]) -> bool {
    u32::from(p[0]) > u32::from(p[1]) + 60 && u32::from(p[0]) > u32::from(p[2]) + 50
}

fn is_z_axis(p: [u8; 4]) -> bool {
    u32::from(p[2]) > u32::from(p[0]) + 70 && u32::from(p[2]) > u32::from(p[1]) + 30
}

/// 床の上に浮いた灰色の箱が1つ。原点（軸の交点）からは離してある。
fn floor_box() -> Model {
    model(
        vec![cube(true)],
        vec![node(
            "box",
            None,
            trs(
                Vec3::splat(1.5),
                Quat::from_rotation_y(0.4),
                Vec3::new(-1.5, 1.0, -1.0),
            ),
            vec![(0, GREY)],
        )],
    )
}

/// 格子ありの画像と無しの画像を比べる。部品の内側は1画素も変わらず（格子は部品の向こうで隠れる）、
/// 背景には格子と軸が出る。
fn check_grid(plain: &Image, grid: &Image, name: &str) {
    save(plain, &format!("{name}-plain.png"));
    save(grid, &format!("{name}-grid.png"));
    let inside = count(plain, |x, y, _| interior(plain, x, y));
    assert!(inside > 200, "{name}: 箱が見える: {inside}");
    let covered = count(plain, |x, y, p| {
        interior(plain, x, y) && grid.pixel(x, y) != p
    });
    assert_eq!(covered, 0, "{name}: 箱の上に格子が出ない");
    let changed = differing_pixels(&plain.rgba, &grid.rgba, 2);
    assert!(changed > 500, "{name}: 格子が描かれる: {changed}");
    let x_axis = count(grid, |x, y, p| {
        is_x_axis(p) && is_background(plain.pixel(x, y))
    });
    let z_axis = count(grid, |x, y, p| {
        is_z_axis(p) && is_background(plain.pixel(x, y))
    });
    assert!(x_axis > 20, "{name}: X軸の赤い線: {x_axis}");
    assert!(z_axis > 20, "{name}: Z軸の青い線: {z_axis}");
}

#[test]
#[ignore = "GPUが必要"]
fn grid_and_axes_are_drawn_and_hidden_behind_parts() {
    let (device, queue) = device();
    let mut renderer = SceneRenderer::new(&device);
    let m = Arc::new(floor_box());
    upload_all(&mut renderer, &device, &queue, &m);
    let f = perspective(
        Vec3::new(4.0, 5.0, 8.0),
        Vec3::new(0.0, 0.5, 0.0),
        Vec3::Y,
        SIZE,
    );
    let plain = draw(&mut renderer, &device, &queue, &f);
    // 多重サンプルの描画先で、シェーダーの出す深度が効いていることを確かめる試験である。
    assert_eq!(renderer.sample_count(), Some(4), "多重サンプルで描いている");
    let grid = draw(&mut renderer, &device, &queue, &with_grid(f.clone()));
    check_grid(&plain, &grid, "grid-perspective");
    // 遠くの格子は薄れる（地平線の近くの行は、手前の行より背景との差が小さい）。
    let row_change = |y: u32| -> u32 {
        (0..grid.width)
            .filter(|x| is_background(plain.pixel(*x, y)))
            .map(|x| {
                let (a, b) = (grid.pixel(x, y), plain.pixel(x, y));
                u32::from(a[0].abs_diff(b[0])) + u32::from(a[1].abs_diff(b[1]))
            })
            .sum()
    };
    let (horizon, _) = {
        let (_, y) = to_pixel(&f, Vec3::new(0.0, 0.0, -1000.0));
        (y, ())
    };
    let near_rows: u32 = (grid.height - 20..grid.height).map(row_change).sum();
    let far_rows: u32 = (horizon + 1..horizon + 21).map(row_change).sum();
    assert!(
        far_rows * 2 < near_rows,
        "遠くは薄い: 遠く {far_rows} 手前 {near_rows}"
    );
}

#[test]
#[ignore = "GPUが必要"]
fn grid_works_with_orthographic_projection_and_straight_down_view() {
    let (device, queue) = device();
    let mut renderer = SceneRenderer::new(&device);
    let m = Arc::new(floor_box());
    upload_all(&mut renderer, &device, &queue, &m);

    let side = orthographic(
        Vec3::new(4.0, 5.0, 8.0),
        Vec3::new(0.0, 0.5, 0.0),
        Vec3::Y,
        4.0,
        SIZE,
    );
    let plain = draw(&mut renderer, &device, &queue, &side);
    let grid = draw(&mut renderer, &device, &queue, &with_grid(side.clone()));
    check_grid(&plain, &grid, "grid-ortho");

    // 真上から（仰角90°）。X軸は画面の横、Z軸は縦に、原点で交わる。
    let top = orthographic(
        Vec3::new(0.0, 10.0, 0.0),
        Vec3::ZERO,
        Vec3::NEG_Z,
        3.0,
        SIZE,
    );
    let plain = draw(&mut renderer, &device, &queue, &top);
    let grid = draw(&mut renderer, &device, &queue, &with_grid(top.clone()));
    check_grid(&plain, &grid, "grid-top");
    let (cx, cy) = to_pixel(&top, Vec3::ZERO);
    let (ex, _) = to_pixel(&top, Vec3::new(2.5, 0.0, 0.0));
    let (_, sy) = to_pixel(&top, Vec3::new(0.0, 0.0, 1.5));
    let x_hits = (cy - 2..=cy + 2)
        .filter(|y| is_x_axis(grid.pixel(ex, *y)))
        .count();
    let z_hits = (cx - 2..=cx + 2)
        .filter(|x| is_z_axis(grid.pixel(*x, sy)))
        .count();
    assert!(x_hits > 0, "X軸が原点を通る横の線");
    assert!(z_hits > 0, "Z軸が原点を通る縦の線");
    // 1マス（1.0）ごとに線がある。原点から右へ、格子の線の位置で背景より明るい。
    for k in 1..=2 {
        let (lx, ly) = to_pixel(&top, Vec3::new(k as f32, 0.0, 0.8));
        let (mx, _) = to_pixel(&top, Vec3::new(k as f32 + 0.45, 0.0, 0.8));
        let on = (lx.saturating_sub(1)..=lx + 1)
            .map(|x| grid.pixel(x, ly)[1])
            .max()
            .unwrap_or(0);
        let off = grid.pixel(mx, ly)[1];
        assert!(on > off + 15, "{k}: 線 {on} マスの中 {off}");
    }

    // 真横から（平面が線になる）。描くものが無く、壊れた画素も出ない。
    let flat = orthographic(Vec3::new(0.0, 0.0, 8.0), Vec3::ZERO, Vec3::Y, 3.0, SIZE);
    let plain = draw(&mut renderer, &device, &queue, &flat);
    let grid = draw(&mut renderer, &device, &queue, &with_grid(flat.clone()));
    save(&grid, "grid-flat.png");
    let changed = differing_pixels(&plain.rgba, &grid.rgba, 2);
    assert!(
        changed <= SIZE[0] as usize * 4,
        "真横では格子は高々数行: {changed}"
    );
}

/// 平行投影の行列を、近い面と遠い面を指定して作る（反転Zなので引数は遠い・近いの順）。
fn orthographic_planes(
    eye: Vec3,
    target: Vec3,
    half_height: f32,
    near: f32,
    far: f32,
) -> FrameParams {
    let aspect = SIZE[0] as f32 / SIZE[1] as f32;
    let hw = half_height * aspect;
    let view = glam::camera::rh::view::look_at_mat4(eye, target, Vec3::Y);
    let proj = glam::camera::rh::proj::directx::orthographic(
        -hw,
        hw,
        -half_height,
        half_height,
        far,
        near,
    );
    params(proj * view, eye, SIZE)
}

fn changed_rows(plain: &Image, grid: &Image, rows: std::ops::Range<u32>) -> usize {
    rows.map(|y| {
        (0..plain.width)
            .filter(|x| {
                let (a, b) = (plain.pixel(*x, y), grid.pixel(*x, y));
                a.iter().zip(b.iter()).any(|(p, q)| p.abs_diff(*q) > 2)
            })
            .count()
    })
    .sum()
}

/// 平行投影の遠い面は全体の境界のすぐ外、近い面は目の後ろ（負）にもなる。床はその外まで続くので、
/// 遠い面より先の格子も、目の後ろ側の格子も切れずに描かれる。
#[test]
#[ignore = "GPUが必要"]
fn orthographic_grid_is_not_cut_at_the_far_plane_or_behind_the_eye() {
    let (device, queue) = device();
    let mut renderer = SceneRenderer::new(&device);
    let empty = Arc::new(model(
        vec![cube(true)],
        vec![node("empty", None, Affine3A::IDENTITY, vec![])],
    ));
    upload_all(&mut renderer, &device, &queue, &empty);

    // 遠い面は目から6。原点（目から約10.6）とその先の床も描かれる。
    let far = orthographic_planes(Vec3::new(4.0, 5.0, 8.0), Vec3::ZERO, 4.0, -3.0, 6.0);
    let plain = draw(&mut renderer, &device, &queue, &far);
    let grid = draw(&mut renderer, &device, &queue, &with_grid(far.clone()));
    save(&grid, "grid-ortho-far-plane.png");
    let top = changed_rows(&plain, &grid, 0..20);
    assert!(top > 300, "画面の上端（遠い面より先）にも格子がある: {top}");
    let x_axis = count(&grid, |_, _, p| is_x_axis(p));
    let z_axis = count(&grid, |_, _, p| is_z_axis(p));
    assert!(
        x_axis > 20 && z_axis > 20,
        "遠い面より先の軸: {x_axis} {z_axis}"
    );

    // 浅い角度で見下ろす。画面の下側の視線は、目の後ろ側（目から約8手前）で床に当たる。
    let behind = orthographic_planes(Vec3::new(0.0, 0.5, 4.0), Vec3::ZERO, 1.5, -20.0, 30.0);
    let plain = draw(&mut renderer, &device, &queue, &behind);
    let grid = draw(&mut renderer, &device, &queue, &with_grid(behind.clone()));
    save(&grid, "grid-ortho-behind-eye.png");
    let bottom = changed_rows(&plain, &grid, SIZE[1] - 15..SIZE[1]);
    assert!(bottom > 200, "目の後ろ側の床にも格子がある: {bottom}");
}

/// 手前に大きな赤い板、その右上の真後ろに隠れた黄色い箱。
fn plate_and_hidden_box() -> Model {
    model(
        vec![cube(true)],
        vec![
            node("root", None, Affine3A::IDENTITY, vec![]),
            node(
                "plate",
                Some(0),
                trs(
                    Vec3::new(3.0, 2.0, 0.2),
                    Quat::IDENTITY,
                    Vec3::new(0.0, 0.0, 1.0),
                ),
                vec![(0, RED)],
            ),
            node(
                "hidden",
                Some(0),
                trs(
                    Vec3::splat(0.5),
                    Quat::from_rotation_y(0.6),
                    Vec3::new(0.8, 0.5, -0.5),
                ),
                vec![(0, YELLOW)],
            ),
        ],
    )
}

fn window(image: &Image, (cx, cy): (u32, u32), r: u32) -> Vec<(u32, u32, [u8; 4])> {
    let mut out = Vec::new();
    for y in cy - r..=cy + r {
        for x in cx - r..=cx + r {
            out.push((x, y, image.pixel(x, y)));
        }
    }
    out
}

#[test]
#[ignore = "GPUが必要"]
fn line_display_modes_change_the_image() {
    let (device, queue) = device_with_lines();
    let mut renderer = SceneRenderer::new(&device);
    assert!(
        renderer.supports_lines(),
        "この装置は線の表示に対応している（Metal・Vulkan・DX12・GLは申告がある）"
    );
    let m = Arc::new(plate_and_hidden_box());
    upload_all(&mut renderer, &device, &queue, &m);
    let mut f = perspective(Vec3::new(0.0, 0.0, 6.0), Vec3::ZERO, Vec3::Y, SIZE);
    let hidden = to_pixel(&f, Vec3::new(0.8, 0.5, -0.5));
    let mut shot = |mode: DisplayMode, name: &str| {
        f.display = mode;
        let image = draw(&mut renderer, &device, &queue, &f);
        save(&image, &format!("display-{name}.png"));
        image
    };
    let faces = shot(DisplayMode::Faces, "faces");
    let both = shot(DisplayMode::FacesAndLines, "faces-and-lines");
    let lines = shot(DisplayMode::Lines, "lines");
    let xray = shot(DisplayMode::XRay, "xray");

    // 面と線: 板の上に暗い線（三角形の辺）が出る。隠れた箱の線は出ない。
    let dark_on_plate = count(&both, |x, y, p| {
        let base = faces.pixel(x, y);
        base[0] > 100 && u32::from(p[0]) + 60 < u32::from(base[0])
    });
    assert!(dark_on_plate > 100, "面の上の暗い線: {dark_on_plate}");
    let leaked = window(&both, hidden, 6)
        .into_iter()
        .filter(|(x, y, p)| *p != faces.pixel(*x, *y))
        .count();
    assert_eq!(leaked, 0, "隠れた箱の線は面と線の表示では見えない");

    // 線: 面を描かないので背景が増え、隠れていた箱の線が見える。
    let bg_faces = count(&faces, |_, _, p| is_background(p));
    let bg_lines = count(&lines, |_, _, p| is_background(p));
    assert!(
        bg_lines > bg_faces + 10_000,
        "面が消える: {bg_faces} -> {bg_lines}"
    );
    let yellow_lines = window(&lines, hidden, 12)
        .into_iter()
        .filter(|(_, _, p)| p[0] > 150 && p[1] > 140 && p[2] < 150)
        .count();
    assert!(yellow_lines > 5, "奥の箱の線も見える: {yellow_lines}");

    // 透かし: 板の向こうの箱が透けて見え、板の色も変わる。
    let through = window(&xray, hidden, 4)
        .into_iter()
        .filter(|(x, y, p)| {
            let base = faces.pixel(*x, *y);
            u32::from(p[1]) > u32::from(base[1]) + 15
        })
        .count();
    assert!(through > 20, "板の向こうの黄色い箱が透ける: {through}");
    let center = (SIZE[0] / 2 - 30, SIZE[1] / 2 + 20);
    assert_ne!(
        xray.pixel(center.0, center.1),
        faces.pixel(center.0, center.1)
    );
    assert!(!is_background(xray.pixel(center.0, center.1)));

    // 選択の輪郭線は、どの表示でも最後に重なる。
    for mode in [
        DisplayMode::Faces,
        DisplayMode::FacesAndLines,
        DisplayMode::Lines,
        DisplayMode::XRay,
    ] {
        f.display = mode;
        f.selected = Some(2..3);
        let image = draw(&mut renderer, &device, &queue, &f);
        let outline = window(&image, hidden, 14)
            .into_iter()
            .filter(|(_, _, p)| p[0] > 220 && p[1] > 220 && p[2] > 220)
            .count();
        assert!(outline > 5, "{mode:?}: 輪郭が出る: {outline}");
    }
}

#[test]
#[ignore = "GPUが必要"]
fn without_line_support_line_modes_fall_back_to_faces() {
    // 既定の装置は POLYGON_MODE_LINE を要求しない。線のパイプラインを作ると検証で失敗するので、作らない。
    let (device, queue) = device();
    assert!(
        !device
            .features()
            .contains(wgpu::Features::POLYGON_MODE_LINE)
    );
    let mut renderer = SceneRenderer::new(&device);
    assert!(!renderer.supports_lines());
    let m = Arc::new(plate_and_hidden_box());
    upload_all(&mut renderer, &device, &queue, &m);
    let mut f = perspective(Vec3::new(0.0, 0.0, 6.0), Vec3::ZERO, Vec3::Y, SIZE);
    let faces = draw(&mut renderer, &device, &queue, &f);
    for mode in [DisplayMode::FacesAndLines, DisplayMode::Lines] {
        f.display = mode;
        let image = draw(&mut renderer, &device, &queue, &f);
        assert_eq!(image.rgba, faces.rgba, "{mode:?} は面で描く");
    }
    assert!(renderer.take_error().is_none());
}

/// 根の下に、子を1つ持つ赤い箱（1と2）と、動かさない緑の箱（3）。
fn drag_scene() -> Model {
    let box_at = |x: f32, y: f32| {
        trs(
            Vec3::splat(0.8),
            Quat::from_rotation_y(0.5),
            Vec3::new(x, y, 0.0),
        )
    };
    model(
        vec![cube(true)],
        vec![
            node("root", None, Affine3A::IDENTITY, vec![]),
            node("moved", Some(0), box_at(-1.5, -0.5), vec![(0, RED)]),
            node("child", Some(1), box_at(-1.5, 0.6), vec![(0, YELLOW)]),
            node("still", Some(0), box_at(1.6, 0.0), vec![(0, GREEN)]),
        ],
    )
}

#[test]
#[ignore = "GPUが必要"]
fn drag_preview_moves_parts_and_outline_without_rewriting_instances() {
    let (device, queue) = device();
    let mut renderer = SceneRenderer::new(&device);
    let m = Arc::new(drag_scene());
    upload_all(&mut renderer, &device, &queue, &m);
    let mut f = perspective(Vec3::new(0.0, 1.0, 7.0), Vec3::ZERO, Vec3::Y, SIZE);
    f.selected = Some(1..3);
    let rest = draw(&mut renderer, &device, &queue, &f);

    let delta =
        Mat4::from_rotation_translation(Quat::from_rotation_z(0.35), Vec3::new(0.9, 0.4, -0.3));
    f.drag = Some(DragPreview {
        nodes: 1..3,
        world_delta: delta,
    });
    let before = renderer.stats();
    let dragged = draw(&mut renderer, &device, &queue, &f);
    let after = renderer.stats();
    save(&dragged, "drag-preview.png");
    assert_ne!(dragged.rgba, rest.rgba, "変形中の部品が動く");
    assert_eq!(
        after.uploaded_bytes, before.uploaded_bytes,
        "インスタンスは送らない"
    );
    assert_eq!(after.instance_rebuilds, before.instance_rebuilds);
    assert_eq!(after.frame_bytes - before.frame_bytes, FRAME_BYTES);

    // 確定した後の絵（部品の表の行列を D 倍して作り直したもの）と同じになる。輪郭も一緒に動く。
    let mut committed = EditedScene::new(&m);
    let d = Affine3A::from_mat4(delta);
    for id in [1u32, 2] {
        let world = m.nodes[id as usize].world;
        committed.set_world(id, d * world);
    }
    f.drag = None;
    renderer.set_instances(
        &device,
        &queue,
        &committed,
        &Visibility::all_visible(m.nodes.len()),
    );
    let moved = draw(&mut renderer, &device, &queue, &f);
    save(&moved, "drag-committed.png");
    let differing = differing_pixels(&dragged.rgba, &moved.rgba, 3);
    assert!(
        differing * 1000 <= (SIZE[0] * SIZE[1]) as usize,
        "変形中の絵と確定後の絵がほぼ一致する: {differing}"
    );
    let outline = |image: &Image| count(image, |_, _, p| p[0] > 220 && p[1] > 220 && p[2] > 220);
    assert!(outline(&moved) > 50);
    assert!(
        outline(&dragged).abs_diff(outline(&moved)) * 20 < outline(&moved),
        "輪郭も動く: 変形中 {} 確定後 {}",
        outline(&dragged),
        outline(&moved)
    );
    // 動かさない緑の箱は、変形中も同じ場所に同じ色で描かれる。
    let (gx, gy) = to_pixel(&f, Vec3::new(1.6, 0.0, 0.0));
    assert_eq!(dragged.pixel(gx, gy), rest.pixel(gx, gy));
}

#[test]
#[ignore = "GPUが必要"]
fn drag_sends_only_the_constant_regardless_of_the_number_of_parts() {
    let (device, queue) = device();
    let mut renderer = SceneRenderer::new(&device);
    let mut specs = vec![node("root", None, Affine3A::IDENTITY, vec![])];
    for i in 0..400u32 {
        let x = (i % 20) as f32 * 0.3 - 3.0;
        let z = (i / 20) as f32 * 0.3 - 3.0;
        specs.push(node(
            "part",
            Some(0),
            trs(Vec3::splat(0.2), Quat::IDENTITY, Vec3::new(x, 0.0, z)),
            vec![(0, GREY)],
        ));
    }
    let m = Arc::new(model(vec![cube(true)], specs));
    upload_all(&mut renderer, &device, &queue, &m);
    let mut f = with_grid(perspective(
        Vec3::new(0.0, 6.0, 8.0),
        Vec3::ZERO,
        Vec3::Y,
        SIZE,
    ));
    f.display = DisplayMode::FacesAndLines;
    let _ = renderer.render(&device, &queue, &f);
    let mut sent = Vec::new();
    for nodes in [1..2, 1..401, 0..401] {
        let before = renderer.stats();
        for step in 0..3 {
            f.drag = Some(DragPreview {
                nodes: nodes.clone(),
                world_delta: Mat4::from_translation(Vec3::new(0.1 * step as f32, 0.2, 0.0)),
            });
            let _ = renderer.render(&device, &queue, &f);
        }
        let after = renderer.stats();
        assert_eq!(after.instance_rebuilds, before.instance_rebuilds);
        assert_eq!(after.instance_patches, before.instance_patches);
        sent.push((
            after.uploaded_bytes - before.uploaded_bytes,
            after.frame_bytes - before.frame_bytes,
        ));
    }
    assert!(
        sent.iter().all(|s| *s == (0, 3 * FRAME_BYTES)),
        "部品1個でも400個でも、送るのは1フレームに定数1つ: {sent:?}"
    );
}

#[test]
#[ignore = "GPUが必要"]
fn idle_frames_do_not_draw_with_grid_lines_and_drag_set() {
    let (device, queue) = device_with_lines();
    let mut renderer = SceneRenderer::new(&device);
    let m = Arc::new(drag_scene());
    upload_all(&mut renderer, &device, &queue, &m);
    let mut f = with_grid(perspective(
        Vec3::new(0.0, 2.0, 7.0),
        Vec3::ZERO,
        Vec3::Y,
        SIZE,
    ));
    f.display = DisplayMode::FacesAndLines;
    f.drag = Some(DragPreview {
        nodes: 1..3,
        world_delta: Mat4::from_translation(Vec3::Y),
    });
    let _ = renderer.render(&device, &queue, &f);
    let after_first = renderer.stats();
    assert_eq!(after_first.draw_count, 1);
    // 汚れの無い回（描画を求めない回）。転送も描画も進まない。
    for _ in 0..5 {
        assert!(renderer.continue_upload(&queue, 1 << 20).done);
    }
    assert_eq!(renderer.stats(), after_first);
    // 表示を変えて描き直しても、形状とインスタンスは送り直さない。
    for mode in [DisplayMode::Lines, DisplayMode::XRay, DisplayMode::Faces] {
        f.display = mode;
        let _ = renderer.render(&device, &queue, &f);
    }
    let stats = renderer.stats();
    assert_eq!(stats.draw_count, 4);
    assert_eq!(stats.uploaded_bytes, after_first.uploaded_bytes);
    assert_eq!(stats.instance_rebuilds, after_first.instance_rebuilds);
}

/// 斜めの面の上の線は、深度の偏りが無いと面に負けて途切れる（偏りなしで約3分の1に減ることを確かめた）。
#[test]
#[ignore = "GPUが必要"]
fn lines_stay_on_top_of_slanted_faces() {
    let (device, queue) = device_with_lines();
    let mut renderer = SceneRenderer::new(&device);
    let m = Arc::new(model(
        vec![cube(true)],
        vec![node(
            "slanted",
            None,
            trs(
                Vec3::new(4.0, 0.05, 4.0),
                Quat::from_rotation_x(0.25) * Quat::from_rotation_y(0.7),
                Vec3::ZERO,
            ),
            vec![(0, RED)],
        )],
    ));
    upload_all(&mut renderer, &device, &queue, &m);
    let mut f = perspective(Vec3::new(0.0, 1.2, 6.0), Vec3::ZERO, Vec3::Y, SIZE);
    let faces = draw(&mut renderer, &device, &queue, &f);
    f.display = DisplayMode::FacesAndLines;
    let both = draw(&mut renderer, &device, &queue, &f);
    save(&both, "display-slanted.png");
    let dark = count(&both, |x, y, p| {
        let base = faces.pixel(x, y);
        base[0] > 100 && u32::from(p[0]) + 60 < u32::from(base[0])
    });
    assert!(dark > 250, "斜めの面の上でも線が続く: {dark}");
}
