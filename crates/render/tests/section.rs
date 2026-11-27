//! 断面の描画の試験（GPUが要る）。正面から見た立方体1つを、軸に垂直な平面で切る。
//! 画素は基準画像と比べず、決まった位置の色が「背景」「部品の色」「切り口の色」のどれかで確かめる。

mod common;

use std::sync::Arc;

use common::write_png;
use common::{EditedScene, cube, device_with_lines, differing_pixels, model, node, out_dir, trs};
use glam::{Affine3A, Quat, Vec3};
use model_reviewer_core::scene::Model;
use model_reviewer_core::section::{Axis, SectionPlane};
use model_reviewer_core::visibility::Visibility;
use model_reviewer_render::{DisplayMode, FrameParams, Image, SceneRenderer};

const CLEAR: [f32; 4] = [0.016, 0.0185, 0.0242, 1.0];
const SIZE: [u32; 2] = [240, 240];
const BLUE: [f32; 4] = [0.1, 0.3, 0.9, 1.0];
/// 切り口の色（sRGB の #D9822B）。
const CUT: [u8; 3] = [0xD9, 0x82, 0x2B];

/// 原点に一辺2の青い立方体。`scale_x` を負にすると鏡写しの部品になる。
fn cube_model(scale_x: f32) -> Model {
    model(
        vec![cube(true)],
        vec![node(
            "cube",
            None,
            trs(Vec3::new(scale_x, 2.0, 2.0), Quat::IDENTITY, Vec3::ZERO),
            vec![(0, BLUE)],
        )],
    )
}

fn frame(section: Option<SectionPlane>, display: DisplayMode) -> FrameParams {
    let eye = Vec3::new(0.0, 0.0, 6.0);
    let view = glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y);
    let proj = glam::camera::rh::proj::directx::perspective_infinite_reverse(
        40f32.to_radians(),
        SIZE[0] as f32 / SIZE[1] as f32,
        0.1,
    );
    FrameParams {
        size_px: SIZE,
        view_proj: proj * view,
        eye,
        selected: None,
        clear: CLEAR,
        display,
        grid: Default::default(),
        drag: None,
        section,
    }
}

fn plane(axis: Axis, offset: f32, flipped: bool) -> Option<SectionPlane> {
    Some(SectionPlane {
        axis,
        offset,
        flipped,
    })
}

fn to_pixel(f: &FrameParams, p: Vec3) -> (u32, u32) {
    let c = f.view_proj * p.extend(1.0);
    let n = c.truncate() / c.w;
    (
        ((n.x * 0.5 + 0.5) * f.size_px[0] as f32) as u32,
        ((0.5 - n.y * 0.5) * f.size_px[1] as f32) as u32,
    )
}

fn shown(device: &wgpu::Device, queue: &wgpu::Queue, m: &Arc<Model>) -> SceneRenderer {
    let mut renderer = SceneRenderer::new(device);
    renderer
        .begin_upload(device, Arc::clone(m))
        .expect("転送を始められる");
    while !renderer.continue_upload(queue, 1 << 20).done {}
    renderer.set_instances(device, queue, m, &Visibility::all_visible(m.nodes.len()));
    renderer
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

fn at(image: &Image, f: &FrameParams, p: Vec3) -> [u8; 4] {
    let (x, y) = to_pixel(f, p);
    image.pixel(x, y)
}

fn is_background(p: [u8; 4]) -> bool {
    (i32::from(p[0]) - 34).abs() <= 3
        && (i32::from(p[1]) - 37).abs() <= 3
        && (i32::from(p[2]) - 43).abs() <= 3
}

fn is_cut(p: [u8; 4]) -> bool {
    p.iter()
        .zip(CUT)
        .all(|(a, b)| (i32::from(*a) - i32::from(b)).abs() <= 6)
}

fn is_blue(p: [u8; 4]) -> bool {
    u32::from(p[2]) > 2 * u32::from(p[0]) && p[2] > 80
}

fn same(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 2)
}

/// 画面の矩形（左上と右下の画素、端を含む）の中で条件に合う画素の数。
fn count_in(
    image: &Image,
    (x0, y0): (u32, u32),
    (x1, y1): (u32, u32),
    pred: impl Fn([u8; 4]) -> bool,
) -> usize {
    let mut n = 0;
    for y in y0.min(y1)..=y0.max(y1).min(image.height - 1) {
        for x in x0.min(x1)..=x0.max(x1).min(image.width - 1) {
            if pred(image.pixel(x, y)) {
                n += 1;
            }
        }
    }
    n
}

// 正面の面（z = 1）の上の、左右の点。対角線を避けて少し上下にずらす。
const LEFT: Vec3 = Vec3::new(-0.5, 0.2, 1.0);
const RIGHT: Vec3 = Vec3::new(0.5, 0.2, 1.0);
// 奥の面（z = -1）の上の点。面の対角線（左下から右上）を避ける。
const BACK: Vec3 = Vec3::new(0.4, -0.3, -1.0);

#[test]
#[ignore = "GPUが必要"]
fn section_removes_the_cut_side_and_flip_swaps() {
    let (device, queue) = device_with_lines();
    let m = Arc::new(cube_model(2.0));
    let mut renderer = shown(&device, &queue, &m);
    let none = frame(None, DisplayMode::Faces);
    let plain = draw(&mut renderer, &device, &queue, &none);
    save(&plain, "section-none.png");
    assert!(is_blue(at(&plain, &none, LEFT)));
    assert!(is_blue(at(&plain, &none, RIGHT)));

    // X の正の側を消す。右は背景、左は断面なしと同じ色。
    let cut = frame(plane(Axis::X, 0.0, false), DisplayMode::Faces);
    let image = draw(&mut renderer, &device, &queue, &cut);
    save(&image, "section-x.png");
    let right = at(&image, &cut, RIGHT);
    assert!(is_background(right), "消す側は背景: {right:?}");
    let left = at(&image, &cut, LEFT);
    assert!(
        same(left, at(&plain, &none, LEFT)),
        "残す側は変わらない: {left:?}"
    );

    // 反転すると入れ替わる。
    let flipped = frame(plane(Axis::X, 0.0, true), DisplayMode::Faces);
    let image = draw(&mut renderer, &device, &queue, &flipped);
    save(&image, "section-x-flipped.png");
    let left = at(&image, &flipped, LEFT);
    assert!(is_background(left), "反転で左が消える: {left:?}");
    let right = at(&image, &flipped, RIGHT);
    assert!(
        same(right, at(&plain, &none, RIGHT)),
        "反転で右が残る: {right:?}"
    );
}

#[test]
#[ignore = "GPUが必要"]
fn back_faces_inside_the_cut_are_cut_colored_only_in_face_modes() {
    let (device, queue) = device_with_lines();
    let m = Arc::new(cube_model(2.0));
    let mut renderer = shown(&device, &queue, &m);
    // 手前の半分（Z の正の側）を消すと、奥の面の内側（裏）が見える。
    for (display, expect_cut, name) in [
        (DisplayMode::Faces, true, "section-cut-faces.png"),
        (
            DisplayMode::FacesAndLines,
            true,
            "section-cut-faces-lines.png",
        ),
        (DisplayMode::Lines, false, "section-cut-lines.png"),
        (DisplayMode::XRay, false, "section-cut-xray.png"),
    ] {
        let f = frame(plane(Axis::Z, 0.0, false), display);
        let image = draw(&mut renderer, &device, &queue, &f);
        save(&image, name);
        let p = at(&image, &f, BACK);
        if expect_cut {
            assert!(is_cut(p), "{display:?} で裏の面は切り口の色: {p:?}");
        } else {
            assert!(!is_cut(p), "{display:?} では切り口の色を塗らない: {p:?}");
        }
    }
    // 断面なしでは、外から見える面に切り口の色は出ない。
    let f = frame(None, DisplayMode::Faces);
    let image = draw(&mut renderer, &device, &queue, &f);
    assert_eq!(
        count_in(&image, (0, 0), (SIZE[0] - 1, SIZE[1] - 1), is_cut),
        0
    );
}

#[test]
#[ignore = "GPUが必要"]
fn mirrored_part_front_is_not_cut_colored() {
    let (device, queue) = device_with_lines();
    let m = Arc::new(cube_model(-2.0));
    let mut renderer = shown(&device, &queue, &m);
    // 何も消さない位置の平面でも、正面は部品の色のまま。
    let thin = frame(plane(Axis::X, 0.9, false), DisplayMode::Faces);
    let image = draw(&mut renderer, &device, &queue, &thin);
    save(&image, "section-mirrored.png");
    let front = at(&image, &thin, LEFT);
    assert!(
        is_blue(front),
        "鏡写しの部品の表は切り口の色にならない: {front:?}"
    );
    // 中をのぞくと、裏は切り口の色。
    let open = frame(plane(Axis::Z, 0.0, false), DisplayMode::Faces);
    let image = draw(&mut renderer, &device, &queue, &open);
    save(&image, "section-mirrored-open.png");
    let back = at(&image, &open, BACK);
    assert!(is_cut(back), "鏡写しの部品の裏は切り口の色: {back:?}");
}

#[test]
#[ignore = "GPUが必要"]
fn partial_update_keeps_the_mirrored_mark_in_step() {
    let (device, queue) = device_with_lines();
    let base = Arc::new(cube_model(2.0));
    let mut renderer = shown(&device, &queue, &base);
    let thin = frame(plane(Axis::X, 0.9, false), DisplayMode::Faces);
    // 確定した変形で鏡写しにする（部分更新の経路）。
    let mut mirrored = EditedScene::new(&base);
    mirrored.set_world(
        0,
        trs(Vec3::new(-2.0, 2.0, 2.0), Quat::IDENTITY, Vec3::ZERO),
    );
    renderer.update_instances(&queue, &mirrored, 0..1);
    let image = draw(&mut renderer, &device, &queue, &thin);
    save(&image, "section-partial-mirrored.png");
    let front = at(&image, &thin, LEFT);
    assert!(is_blue(front), "部分更新でも鏡写しの印が立つ: {front:?}");
    // 戻すと印が外れる。
    let mut back = EditedScene::new(&base);
    back.set_world(0, Affine3A::from_scale(Vec3::splat(2.0)));
    renderer.update_instances(&queue, &back, 0..1);
    let image = draw(&mut renderer, &device, &queue, &thin);
    let front = at(&image, &thin, LEFT);
    assert!(is_blue(front), "印が外れても表は部品の色: {front:?}");
    let open = frame(plane(Axis::Z, 0.0, false), DisplayMode::Faces);
    let image = draw(&mut renderer, &device, &queue, &open);
    assert!(is_cut(at(&image, &open, BACK)));
}

#[test]
#[ignore = "GPUが必要"]
fn lines_xray_and_selection_outline_are_cut_too() {
    let (device, queue) = device_with_lines();
    let m = Arc::new(cube_model(2.0));
    let mut renderer = shown(&device, &queue, &m);
    // 立方体の右半分の画面上の範囲（平面 x = 0 の少し右から、右端の少し外まで）。
    let probe = frame(None, DisplayMode::Faces);
    let top_left = to_pixel(&probe, Vec3::new(0.15, 1.1, 1.0));
    let bottom_right = to_pixel(&probe, Vec3::new(1.15, -1.1, 1.0));
    let visible = |p: [u8; 4]| !is_background(p);

    let mut modes = vec![DisplayMode::Faces, DisplayMode::XRay];
    if renderer.supports_lines() {
        modes.push(DisplayMode::Lines);
        modes.push(DisplayMode::FacesAndLines);
    }
    for display in modes {
        for selected in [None, Some(0..1)] {
            let mut uncut = frame(None, display);
            uncut.selected = selected.clone();
            let image = draw(&mut renderer, &device, &queue, &uncut);
            let before = count_in(&image, top_left, bottom_right, visible);
            assert!(before > 100, "{display:?} 切る前は右半分が見える: {before}");

            let mut cut = frame(plane(Axis::X, 0.0, false), display);
            cut.selected = selected.clone();
            let image = draw(&mut renderer, &device, &queue, &cut);
            save(
                &image,
                &format!("section-right-{display:?}-{}.png", selected.is_some()),
            );
            let after = count_in(&image, top_left, bottom_right, visible);
            assert_eq!(after, 0, "{display:?} 選択{selected:?} で右半分が消える");
        }
    }
}

#[test]
#[ignore = "GPUが必要"]
fn moving_the_plane_sends_only_the_frame_constants() {
    let (device, queue) = device_with_lines();
    let m = Arc::new(cube_model(2.0));
    let mut renderer = shown(&device, &queue, &m);
    let _ = draw(
        &mut renderer,
        &device,
        &queue,
        &frame(None, DisplayMode::Faces),
    );
    let start = renderer.stats();
    let mut images = Vec::new();
    for (i, offset) in [-0.5f32, 0.0, 0.5].into_iter().enumerate() {
        let image = draw(
            &mut renderer,
            &device,
            &queue,
            &frame(plane(Axis::X, offset, false), DisplayMode::Faces),
        );
        let now = renderer.stats();
        assert_eq!(
            now.uploaded_bytes, start.uploaded_bytes,
            "形状もインスタンスも送らない"
        );
        assert_eq!(now.instance_rebuilds, start.instance_rebuilds);
        assert_eq!(now.instance_patches, start.instance_patches);
        assert_eq!(now.frame_bytes, start.frame_bytes + 304 * (i as u64 + 1));
        assert_eq!(
            now.draw_count,
            start.draw_count + i as u64 + 1,
            "位置を変えた回は描く"
        );
        images.push(image);
    }
    assert!(differing_pixels(&images[0].rgba, &images[1].rgba, 4) > 100);
    assert!(differing_pixels(&images[1].rgba, &images[2].rgba, 4) > 100);
}

#[test]
#[ignore = "GPUが必要"]
fn no_section_and_a_plane_that_cuts_nothing_draw_the_same_image() {
    let (device, queue) = device_with_lines();
    let m = Arc::new(cube_model(2.0));
    let mut renderer = shown(&device, &queue, &m);
    let none = draw(
        &mut renderer,
        &device,
        &queue,
        &frame(None, DisplayMode::Faces),
    );
    let default_frame = FrameParams {
        section: Default::default(),
        ..frame(None, DisplayMode::Faces)
    };
    let again = draw(&mut renderer, &device, &queue, &default_frame);
    assert_eq!(none.rgba, again.rgba, "既定の断面は「なし」");
    // 境界箱の外の平面は何も消さず、外から見える面だけなので、切り口の色も出ない。
    let far = draw(
        &mut renderer,
        &device,
        &queue,
        &frame(plane(Axis::Y, 50.0, false), DisplayMode::Faces),
    );
    save(&far, "section-far.png");
    assert_eq!(differing_pixels(&none.rgba, &far.rgba, 1), 0);
}
