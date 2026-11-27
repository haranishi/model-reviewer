//! GPUが要る試験。手で組んだ形状を画像へ描いて読み戻し、`target/tmp/render-gpu/` にPNGで残す。
//! 画素を基準画像と比べることはしない（GPUの違いで割れる）。中心の色・選択の有無・大きさで確かめる。

mod common;

use std::sync::Arc;

use common::{cube, device, model, node, out_dir, tetra, trs, write_png};
use glam::{Affine3A, Mat4, Quat, Vec3};
use model_reviewer_core::scene::{Model, PrimId};
use model_reviewer_core::visibility::Visibility;
use model_reviewer_render::{FrameParams, Image, SceneRenderer};

const RED: [f32; 4] = [0.8, 0.05, 0.04, 1.0];
const YELLOW: [f32; 4] = [0.9, 0.8, 0.1, 1.0];
const GREEN: [f32; 4] = [0.08, 0.6, 0.12, 1.0];
const BLUE: [f32; 4] = [0.1, 0.3, 0.9, 1.0];
const GREY: [f32; 4] = [0.45, 0.45, 0.45, 1.0];
const ORANGE: [f32; 4] = [0.85, 0.28, 0.04, 1.0];
const WHITE: [f32; 4] = [0.95, 0.95, 0.95, 1.0];
/// #22252B のリニア値。
const CLEAR: [f32; 4] = [0.016, 0.0185, 0.0242, 1.0];
const SIZE: [u32; 2] = [320, 200];

/// 手前の赤い板、その真後ろに完全に隠れた黄色い箱、奥の緑の板、右に法線なしの青い四面体、
/// 左に同じ立方体を共有する灰色の箱2つ。立方体の形状は5部品で共有する。
fn scene() -> Model {
    model(
        vec![cube(true), tetra()],
        vec![
            node("root", None, Affine3A::IDENTITY, vec![]),
            node(
                "front",
                Some(0),
                trs(
                    Vec3::new(1.6, 1.6, 0.3),
                    Quat::IDENTITY,
                    Vec3::new(0.0, 0.0, 1.5),
                ),
                vec![(0, RED)],
            ),
            node(
                "hidden",
                Some(0),
                trs(Vec3::splat(0.5), Quat::from_rotation_y(0.4), Vec3::ZERO),
                vec![(0, YELLOW)],
            ),
            node(
                "back",
                Some(0),
                trs(
                    Vec3::new(3.0, 2.0, 0.3),
                    Quat::IDENTITY,
                    Vec3::new(0.0, 0.0, -1.5),
                ),
                vec![(0, GREEN)],
            ),
            node(
                "tetra",
                Some(0),
                trs(
                    Vec3::splat(1.3),
                    Quat::from_rotation_y(0.5),
                    Vec3::new(2.1, 0.2, 0.5),
                ),
                vec![(1, BLUE)],
            ),
            node(
                "grey_low",
                Some(0),
                trs(
                    Vec3::splat(0.6),
                    Quat::from_rotation_y(0.7),
                    Vec3::new(-2.1, -0.6, 0.0),
                ),
                vec![(0, GREY)],
            ),
            node(
                "grey_high",
                Some(0),
                trs(
                    Vec3::splat(0.6),
                    Quat::from_rotation_x(0.5),
                    Vec3::new(-2.1, 0.8, 0.0),
                ),
                vec![(0, GREY)],
            ),
        ],
    )
}

/// 正面に大きな青い箱が1つだけのモデル。
fn blue_model() -> Model {
    model(
        vec![cube(true)],
        vec![node(
            "blue",
            None,
            trs(Vec3::new(3.0, 3.0, 0.5), Quat::IDENTITY, Vec3::ZERO),
            vec![(0, BLUE)],
        )],
    )
}

fn frame(size: [u32; 2], selected: Option<std::ops::Range<u32>>) -> FrameParams {
    let eye = Vec3::new(0.0, 1.2, 6.0);
    let aspect = size[0] as f32 / size[1] as f32;
    let view = glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y);
    let proj = glam::camera::rh::proj::directx::perspective_infinite_reverse(
        40f32.to_radians(),
        aspect,
        0.1,
    );
    FrameParams {
        size_px: size,
        view_proj: proj * view,
        eye,
        selected,
        clear: CLEAR,
        display: Default::default(),
        grid: Default::default(),
        drag: None,
        section: None,
    }
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
    loop {
        if renderer.continue_upload(queue, 1 << 20).done {
            break;
        }
    }
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

fn center(image: &Image) -> [u8; 4] {
    image.pixel(image.width / 2, image.height / 2)
}

fn is_background(p: [u8; 4]) -> bool {
    (i32::from(p[0]) - 34).abs() <= 3
        && (i32::from(p[1]) - 37).abs() <= 3
        && (i32::from(p[2]) - 43).abs() <= 3
}

fn is_bright_line(p: [u8; 4]) -> bool {
    p[0] > 220 && p[1] > 220 && p[2] > 220
}

fn is_dark_line(p: [u8; 4]) -> bool {
    u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2]) < 90 && p[0] < 40 && p[1] < 40 && p[2] < 48
}

fn is_outline(p: [u8; 4]) -> bool {
    is_bright_line(p) || is_dark_line(p)
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

#[test]
#[ignore = "GPUが必要"]
fn draws_shapes_with_depth_colors_and_face_normals() {
    let (device, queue) = device();
    let mut renderer = SceneRenderer::new(&device);
    let m = Arc::new(scene());
    upload_all(&mut renderer, &device, &queue, &m);
    let image = draw(&mut renderer, &device, &queue, &frame(SIZE, None));
    save(&image, "scene.png");

    let c = center(&image);
    assert!(!is_background(c), "中心は背景色でない: {c:?}");
    assert!(
        u32::from(c[0]) > 2 * u32::from(c[1].max(c[2])),
        "中心は手前の赤い板（奥の緑・黄色より手前）: {c:?}"
    );
    let green = count(&image, |_, _, p| {
        u32::from(p[1]) > 2 * u32::from(p[0].max(p[2])) && p[1] > 60
    });
    assert!(green > 200, "奥の緑の板が手前の板の外側に見える: {green}");
    let yellow = count(&image, |_, _, p| p[0] > 150 && p[1] > 130 && p[2] < 60);
    assert_eq!(yellow, 0, "黄色い箱は赤い板の真後ろに隠れる");

    // 法線なしの四面体は、面ごとに明るさの違う青で描かれる（黒や一色にならない）。
    let blues: Vec<u8> = (0..image.height)
        .flat_map(|y| (image.width / 2..image.width).map(move |x| (x, y)))
        .map(|(x, y)| image.pixel(x, y))
        .filter(|p| p[2] > p[0].saturating_add(40) && p[2] > p[1].saturating_add(15))
        .map(|p| p[2])
        .collect();
    assert!(blues.len() > 200, "四面体が見える: {}", blues.len());
    let (lo, hi) = (
        blues.iter().min().copied().unwrap_or(0),
        blues.iter().max().copied().unwrap_or(0),
    );
    assert!(hi - lo > 25, "面ごとに陰影が違う: {lo}..{hi}");
}

#[test]
#[ignore = "GPUが必要"]
fn selection_changes_image_and_outline_shows_hidden_part() {
    let (device, queue) = device();
    let mut renderer = SceneRenderer::new(&device);
    let m = Arc::new(scene());
    upload_all(&mut renderer, &device, &queue, &m);
    let plain = draw(&mut renderer, &device, &queue, &frame(SIZE, None));
    let uploaded = renderer.stats().uploaded_bytes;
    let rebuilds = renderer.stats().instance_rebuilds;

    // 完全に隠れた黄色い箱を選ぶ。色の変化は見えないが、輪郭線は赤い板の上に出る。
    let hidden = draw(&mut renderer, &device, &queue, &frame(SIZE, Some(2..3)));
    save(&hidden, "selected-hidden.png");
    assert_ne!(plain.rgba, hidden.rgba, "選択で画像が変わる");
    let (w, h) = (hidden.width, hidden.height);
    let on_front = |x, y, p: [u8; 4]| {
        x > w * 3 / 8 && x < w * 5 / 8 && y > h / 4 && y < h * 3 / 4 && is_outline(p)
    };
    let bright_on_front = count(&hidden, |x, y, p| on_front(x, y, p) && is_bright_line(p));
    let dark_on_front = count(&hidden, |x, y, p| on_front(x, y, p) && is_dark_line(p));
    assert!(
        bright_on_front > 10 && dark_on_front > 10,
        "隠れた部品の二重の輪郭が見える: 明 {bright_on_front} 暗 {dark_on_front}"
    );
    assert_eq!(
        count(&plain, |_, _, p| is_outline(p)),
        0,
        "選択なしでは輪郭線が無い"
    );

    // 見えている奥の板を選ぶと、色が明るくなり輪郭も出る。
    let back = draw(&mut renderer, &device, &queue, &frame(SIZE, Some(3..4)));
    save(&back, "selected-back.png");
    assert_ne!(plain.rgba, back.rgba);
    let root = draw(&mut renderer, &device, &queue, &frame(SIZE, Some(0..7)));
    save(&root, "selected-root.png");
    assert!(count(&root, |_, _, p| is_outline(p)) > count(&hidden, |_, _, p| is_outline(p)));

    // 選択を変えてもバッファは書き換えない。
    assert_eq!(renderer.stats().uploaded_bytes, uploaded);
    assert_eq!(renderer.stats().instance_rebuilds, rebuilds);
    // 選択を外すと元に戻る。
    let again = draw(&mut renderer, &device, &queue, &frame(SIZE, None));
    assert_eq!(plain.rgba, again.rgba);
}

/// 橙・白・灰の箱を横に並べたモデル。輪郭が部品の色に埋もれないかを見る。
fn colored_boxes() -> Model {
    let boxed = |name: &'static str, x: f32, color: [f32; 4]| {
        node(
            name,
            Some(0),
            trs(
                Vec3::splat(1.1),
                Quat::from_rotation_y(0.5),
                Vec3::new(x, 0.0, 0.0),
            ),
            vec![(0, color)],
        )
    };
    model(
        vec![cube(true)],
        vec![
            node("root", None, Affine3A::IDENTITY, vec![]),
            boxed("orange", -2.0, ORANGE),
            boxed("white", 0.0, WHITE),
            boxed("grey", 2.0, GREY),
        ],
    )
}

/// 暗い外縁と明るい内側の二重の輪郭。どの色の部品の上でも、少なくとも外縁は見える。
#[test]
#[ignore = "GPUが必要"]
fn double_outline_is_visible_on_orange_white_and_grey_parts() {
    let (device, queue) = device();
    let mut renderer = SceneRenderer::new(&device);
    let m = Arc::new(colored_boxes());
    upload_all(&mut renderer, &device, &queue, &m);
    let size = [480, 200];
    let plain = draw(&mut renderer, &device, &queue, &frame(size, None));
    save(&plain, "outline-plain.png");
    for (node, name) in [(1u32, "orange"), (2, "white"), (3, "grey")] {
        let image = draw(
            &mut renderer,
            &device,
            &queue,
            &frame(size, Some(node..node + 1)),
        );
        save(&image, &format!("outline-{name}.png"));
        let dark = count(&image, |x, y, p| {
            is_dark_line(p) && !is_dark_line(plain.pixel(x, y))
        });
        let bright = count(&image, |x, y, p| {
            is_bright_line(p) && !is_bright_line(plain.pixel(x, y))
        });
        assert!(dark > 40, "{name}: 暗い外縁が見える: {dark}");
        if name != "white" {
            assert!(bright > 40, "{name}: 明るい内側が見える: {bright}");
        }
    }
}

#[test]
#[ignore = "GPUが必要"]
fn resize_recreates_targets_only_when_size_changes() {
    let (device, queue) = device();
    let mut renderer = SceneRenderer::new(&device);
    let m = Arc::new(scene());
    upload_all(&mut renderer, &device, &queue, &m);
    assert!(
        renderer
            .render(&device, &queue, &frame([64, 48], None))
            .recreated
    );
    assert!(
        !renderer
            .render(&device, &queue, &frame([64, 48], None))
            .recreated
    );
    assert!(
        renderer
            .render(&device, &queue, &frame([501, 333], Some(1..2)))
            .recreated
    );
    let big = renderer.read_image(&device, &queue).expect("読み戻せる");
    assert_eq!((big.width, big.height), (501, 333));
    save(&big, "resized.png");
    assert!(!is_background(center(&big)));
    // 大きさ0は描かない（落ちず、描画回数も増えない）。
    let before = renderer.stats().draw_count;
    let out = renderer.render(&device, &queue, &frame([0, 100], None));
    assert!(!out.recreated);
    assert_eq!(renderer.stats().draw_count, before);
}

#[test]
#[ignore = "GPUが必要"]
fn idle_frames_neither_draw_nor_upload() {
    let (device, queue) = device();
    let mut renderer = SceneRenderer::new(&device);
    let m = Arc::new(scene());
    upload_all(&mut renderer, &device, &queue, &m);
    let geometry: u64 = m
        .primitives
        .iter()
        .map(|p| (p.vertices.len() * 24 + p.indices.len() * 4) as u64)
        .sum();
    let instances = 6 * 144;
    assert_eq!(
        renderer.stats().uploaded_bytes,
        geometry + instances,
        "共有形状は1回だけ送る"
    );
    let _ = renderer.render(&device, &queue, &frame(SIZE, None));
    let after_first = renderer.stats();
    assert_eq!(after_first.draw_count, 1);
    // 汚れの無い回。転送も描画も進まず、描画回数は前後で同じ。
    let draws_before_idle = renderer.stats().draw_count;
    for _ in 0..5 {
        let progress = renderer.continue_upload(&queue, 1 << 20);
        assert!(progress.done);
        assert_eq!(renderer.stats().draw_count, draws_before_idle);
    }
    assert_eq!(renderer.stats().draw_count, draws_before_idle);
    assert_eq!(renderer.stats().uploaded_bytes, after_first.uploaded_bytes);
    assert_eq!(
        renderer.stats().instance_rebuilds,
        after_first.instance_rebuilds
    );
    // 描き直しても形状は送り直さない。
    let _ = renderer.render(&device, &queue, &frame(SIZE, Some(1..2)));
    assert_eq!(renderer.stats().draw_count, 2);
    assert_eq!(renderer.stats().uploaded_bytes, after_first.uploaded_bytes);
}

#[test]
#[ignore = "GPUが必要"]
fn shown_model_survives_pending_cancel_and_failure() {
    let (device, queue) = device();
    let mut renderer = SceneRenderer::new(&device);
    let a = Arc::new(scene());
    upload_all(&mut renderer, &device, &queue, &a);
    let f = frame(SIZE, None);
    let red = center(&draw(&mut renderer, &device, &queue, &f));
    assert!(u32::from(red[0]) > 2 * u32::from(red[2]));

    // 転送中は前のモデルを描き続ける。
    let b = Arc::new(blue_model());
    renderer
        .begin_upload(&device, Arc::clone(&b))
        .expect("始められる");
    let first = renderer.continue_upload(&queue, 64);
    assert!(!first.done);
    assert_eq!(first.uploaded_bytes, 64);
    assert!(renderer.is_uploading());
    assert!(Arc::ptr_eq(renderer.shown_model().expect("表示中"), &a));
    assert_eq!(center(&draw(&mut renderer, &device, &queue, &f)), red);

    // 中止しても前のモデルが残る。
    renderer.cancel_upload();
    assert!(!renderer.is_uploading());
    assert_eq!(center(&draw(&mut renderer, &device, &queue, &f)), red);

    // 新しい begin_upload は転送中の組だけを置き換える。最後まで送った回に入れ替わる。
    renderer
        .begin_upload(&device, Arc::clone(&a))
        .expect("始められる");
    renderer
        .begin_upload(&device, Arc::clone(&b))
        .expect("始められる");
    let mut steps = 0;
    let mut last = 0;
    let total = loop {
        let progress = renderer.continue_upload(&queue, 256);
        steps += 1;
        assert!(progress.uploaded_bytes >= last && progress.uploaded_bytes - last <= 256);
        last = progress.uploaded_bytes;
        if progress.done {
            break progress.total_bytes;
        }
        assert!(Arc::ptr_eq(renderer.shown_model().expect("表示中"), &a));
    };
    assert_eq!(last, total);
    assert!(steps > 1, "予算で分割して送る");
    assert!(Arc::ptr_eq(renderer.shown_model().expect("表示中"), &b));
    renderer.set_instances(&device, &queue, &b, &Visibility::all_visible(b.nodes.len()));
    let blue = center(&draw(&mut renderer, &device, &queue, &f));
    save(
        &renderer.read_image(&device, &queue).expect("読み戻せる"),
        "swapped.png",
    );
    assert!(
        u32::from(blue[2]) > 2 * u32::from(blue[0]),
        "入れ替わった後は青: {blue:?}"
    );

    // 転送を始められないモデルでも、表示中のモデルは残る。
    let mut broken = blue_model();
    broken.draws[0].prim = PrimId(99);
    assert!(renderer.begin_upload(&device, Arc::new(broken)).is_err());
    assert!(!renderer.is_uploading());
    assert_eq!(center(&draw(&mut renderer, &device, &queue, &f)), blue);

    // 表示中でないモデルのインスタンスは受け付けない。
    renderer.set_instances(&device, &queue, &a, &Visibility::all_visible(a.nodes.len()));
    assert_eq!(center(&draw(&mut renderer, &device, &queue, &f)), blue);

    renderer.clear_model();
    assert!(renderer.shown_model().is_none());
    assert!(is_background(center(&draw(
        &mut renderer,
        &device,
        &queue,
        &f
    ))));
    let _ = Mat4::IDENTITY;
}

#[test]
#[ignore = "GPUが必要"]
fn writes_sample_initial_view() {
    use model_reviewer_core::camera::OrbitCamera;
    use model_reviewer_gltf::load_glb;
    use model_reviewer_testdata::sample_glb;

    let (device, queue) = device();
    let mut renderer = SceneRenderer::new(&device);
    let model = Arc::new(load_glb(&sample_glb()).expect("sample"));
    upload_all(&mut renderer, &device, &queue, &model);
    let size = [960u32, 640];
    let aspect = size[0] as f32 / size[1] as f32;
    let camera = OrbitCamera::home(&model.bounds, aspect);
    let params = FrameParams {
        size_px: size,
        view_proj: camera.proj(aspect, &model.bounds) * camera.view(),
        eye: camera.eye(),
        selected: None,
        clear: CLEAR,
        display: Default::default(),
        grid: Default::default(),
        drag: None,
        section: None,
    };
    let image = draw(&mut renderer, &device, &queue, &params);
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/sample-initial.png");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("target");
    }
    write_png(&path, image.width, image.height, &image.rgba);
    let foreground = count(&image, |_, _, pixel| !is_background(pixel));
    assert!(foreground > 1000, "見本が描かれていない: {foreground}");
}
