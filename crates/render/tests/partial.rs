//! 確定した変形の部分更新と、足した図形の形状の転送の試験（GPUが要る）。

mod common;

use std::sync::Arc;

use common::{EditedScene, cube, device, model, node, out_dir, tetra, trs, write_png};
use glam::{Affine3A, Quat, Vec3};
use model_reviewer_core::scene::{Model, NodeId, Primitive, Vertex};
use model_reviewer_core::visibility::Visibility;
use model_reviewer_render::{
    ADD_PRIMITIVES_LIMIT_BYTES, FrameParams, Image, RenderError, SceneRenderer,
};

const CLEAR: [f32; 4] = [0.016, 0.0185, 0.0242, 1.0];
const SIZE: [u32; 2] = [320, 200];
const INSTANCE_BYTES: u64 = 144;
const RED: [f32; 4] = [0.8, 0.05, 0.04, 1.0];
const BLUE: [f32; 4] = [0.1, 0.3, 0.9, 1.0];
const YELLOW: [f32; 4] = [0.9, 0.8, 0.1, 1.0];

fn frame() -> FrameParams {
    let eye = Vec3::new(0.0, 4.0, 9.0);
    let view = glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y);
    let proj = glam::camera::rh::proj::directx::perspective_infinite_reverse(
        0.7,
        SIZE[0] as f32 / SIZE[1] as f32,
        0.05,
    );
    FrameParams {
        size_px: SIZE,
        view_proj: proj * view,
        eye,
        selected: None,
        clear: CLEAR,
        display: Default::default(),
        grid: Default::default(),
        drag: None,
        section: None,
    }
}

fn to_pixel(f: &FrameParams, p: Vec3) -> (u32, u32) {
    let c = f.view_proj * p.extend(1.0);
    let n = c.truncate() / c.w;
    (
        ((n.x * 0.5 + 0.5) * f.size_px[0] as f32) as u32,
        ((0.5 - n.y * 0.5) * f.size_px[1] as f32) as u32,
    )
}

fn upload(
    renderer: &mut SceneRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    m: &Arc<Model>,
) {
    renderer
        .begin_upload(device, Arc::clone(m))
        .expect("転送を始められる");
    while !renderer.continue_upload(queue, 1 << 20).done {}
}

fn draw(renderer: &mut SceneRenderer, device: &wgpu::Device, queue: &wgpu::Queue) -> Image {
    let _ = renderer.render(device, queue, &frame());
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

fn near(image: &Image, (cx, cy): (u32, u32), pred: impl Fn([u8; 4]) -> bool) -> usize {
    let mut n = 0;
    for y in cy.saturating_sub(4)..=(cy + 4).min(image.height - 1) {
        for x in cx.saturating_sub(4)..=(cx + 4).min(image.width - 1) {
            if pred(image.pixel(x, y)) {
                n += 1;
            }
        }
    }
    n
}

fn is_blue(p: [u8; 4]) -> bool {
    u32::from(p[2]) > 2 * u32::from(p[0]) && p[2] > 80
}

fn is_yellow(p: [u8; 4]) -> bool {
    p[0] > 120 && p[1] > 100 && u32::from(p[2]) + 60 < u32::from(p[1])
}

/// 根の下に部品24個。立方体と四面体を交互に使うので、1つの部品の範囲のインスタンスは
/// バッファの中で2つのまとまりに散らばる。
fn rows() -> Model {
    let colors = [
        [0.8, 0.05, 0.04, 1.0],
        [0.08, 0.6, 0.12, 1.0],
        [0.1, 0.3, 0.9, 1.0],
        [0.9, 0.8, 0.1, 1.0],
    ];
    let mut specs = vec![node("root", None, Affine3A::IDENTITY, vec![])];
    for i in 0..24u32 {
        let x = (i % 6) as f32 * 1.1 - 2.75;
        let z = (i / 6) as f32 * 1.1 - 1.65;
        specs.push(node(
            "part",
            Some(0),
            trs(
                Vec3::splat(0.6),
                Quat::from_rotation_y(i as f32 * 0.3),
                Vec3::new(x, 0.0, z),
            ),
            vec![(i % 2, colors[i as usize % 4])],
        ));
    }
    model(vec![cube(true), tetra()], specs)
}

fn moved(base: &Arc<Model>, nodes: std::ops::Range<u32>) -> EditedScene {
    let mut edited = EditedScene::new(base);
    let delta =
        Affine3A::from_rotation_translation(Quat::from_rotation_x(0.4), Vec3::new(0.3, 0.8, 0.2));
    for id in nodes {
        let world = base.nodes[id as usize].world;
        edited.set_world(id, delta * world);
    }
    edited
}

#[test]
#[ignore = "GPUが必要"]
fn partial_update_matches_full_rebuild_and_sends_only_the_range() {
    let (device, queue) = device();
    let base = Arc::new(rows());
    let edited = moved(&base, 5..9);

    let mut renderer = SceneRenderer::new(&device);
    upload(&mut renderer, &device, &queue, &base);
    renderer.set_instances(
        &device,
        &queue,
        &base,
        &Visibility::all_visible(base.nodes.len()),
    );
    let before_image = draw(&mut renderer, &device, &queue);
    let before = renderer.stats();
    renderer.update_instances(&queue, &edited, 5..9);
    let after = renderer.stats();
    assert_eq!(
        after.uploaded_bytes - before.uploaded_bytes,
        4 * INSTANCE_BYTES,
        "送るのは動かした4部品のインスタンスだけ"
    );
    assert_eq!(after.instance_rebuilds, before.instance_rebuilds);
    assert_eq!(after.instance_patches, before.instance_patches + 1);
    let patched = draw(&mut renderer, &device, &queue);
    save(&patched, "partial-patched.png");
    assert_ne!(patched.rgba, before_image.rgba, "動かした部品が動く");

    // 全体を作り直した絵と1画素も違わない。
    let mut full = SceneRenderer::new(&device);
    upload(&mut full, &device, &queue, &base);
    full.set_instances(
        &device,
        &queue,
        &edited,
        &Visibility::all_visible(base.nodes.len()),
    );
    let rebuilt = draw(&mut full, &device, &queue);
    save(&rebuilt, "partial-rebuilt.png");
    assert_eq!(patched.rgba, rebuilt.rgba);

    // 送るバイト数は動かした範囲に比例する（1部品・12部品）。
    for nodes in [3..4, 10..22] {
        let edited = moved(&base, nodes.clone());
        let before = renderer.stats().uploaded_bytes;
        renderer.update_instances(&queue, &edited, nodes.clone());
        let sent = renderer.stats().uploaded_bytes - before;
        assert_eq!(sent, u64::from(nodes.end - nodes.start) * INSTANCE_BYTES);
    }
}

#[test]
#[ignore = "GPUが必要"]
fn partial_update_skips_hidden_parts_and_ignores_changed_structure() {
    let (device, queue) = device();
    let base = Arc::new(rows());
    let hidden = |id: NodeId| id.0 != 6 && id.0 != 7;
    let edited = moved(&base, 5..9);

    let mut renderer = SceneRenderer::new(&device);
    upload(&mut renderer, &device, &queue, &base);
    renderer.set_instances_with(&device, &queue, &base, &hidden);
    let before = renderer.stats().uploaded_bytes;
    renderer.update_instances(&queue, &edited, 5..9);
    assert_eq!(
        renderer.stats().uploaded_bytes - before,
        2 * INSTANCE_BYTES,
        "非表示の部品は送らない"
    );
    let patched = draw(&mut renderer, &device, &queue);

    let mut full = SceneRenderer::new(&device);
    upload(&mut full, &device, &queue, &base);
    full.set_instances_with(&device, &queue, &edited, &hidden);
    assert_eq!(patched.rgba, draw(&mut full, &device, &queue).rgba);

    // 部品の数が変わったら（追加・削除の後）、部分更新はせず何も送らない。
    let mut grown = moved(&base, 5..9);
    grown.add_part(Arc::new(cube(true)), Affine3A::IDENTITY, RED);
    let stats = renderer.stats();
    renderer.update_instances(&queue, &grown, 5..9);
    assert_eq!(renderer.stats(), stats);
    // 表示中でないモデルも受け付けない。
    let other = Arc::new(rows());
    renderer.update_instances(&queue, &other, 5..9);
    assert_eq!(renderer.stats(), stats);
}

fn prim_bytes(prim: &Primitive) -> u64 {
    (prim.vertices.len() * 24 + prim.indices.len() * 4) as u64
}

#[test]
#[ignore = "GPUが必要"]
fn added_primitives_are_drawn_and_released_with_the_model() {
    let (device, queue) = device();
    let base = Arc::new(model(
        vec![cube(true)],
        vec![node(
            "box",
            None,
            trs(Vec3::splat(1.2), Quat::IDENTITY, Vec3::ZERO),
            vec![(0, RED)],
        )],
    ));
    let mut renderer = SceneRenderer::new(&device);
    assert!(
        renderer
            .add_primitives(&device, &queue, &[Arc::new(tetra())])
            .is_err(),
        "表示中のモデルが無ければ断る"
    );
    upload(&mut renderer, &device, &queue, &base);
    renderer.set_instances(&device, &queue, &base, &Visibility::all_visible(1));
    let plain = draw(&mut renderer, &device, &queue);
    assert_eq!(renderer.primitive_slot_count(), 1);

    let f = frame();
    let right = to_pixel(&f, Vec3::new(2.5, 0.0, 0.0));
    let left = to_pixel(&f, Vec3::new(-2.5, 0.0, 0.0));
    assert_eq!(near(&plain, right, is_blue), 0);
    assert_eq!(near(&plain, left, is_yellow), 0);

    // 1つ目: 四面体を足す。送るのは足した形状の分だけ。
    let mut doc = EditedScene::new(&base);
    let tetra = Arc::new(tetra());
    let id = doc.add_part(
        Arc::clone(&tetra),
        trs(Vec3::splat(1.6), Quat::IDENTITY, Vec3::new(2.5, 0.0, 0.0)),
        BLUE,
    );
    assert_eq!(id.0 as usize, renderer.primitive_slot_count());
    let before = renderer.stats().uploaded_bytes;
    renderer
        .add_primitives(&device, &queue, &[Arc::clone(&tetra)])
        .expect("足せる");
    assert_eq!(renderer.stats().uploaded_bytes - before, prim_bytes(&tetra));
    assert_eq!(renderer.added_geometry_bytes(), prim_bytes(&tetra));
    assert_eq!(renderer.primitive_slot_count(), 2);
    renderer.set_instances(&device, &queue, &doc, &Visibility::all_visible(2));
    let one = draw(&mut renderer, &device, &queue);
    save(&one, "added-one.png");
    assert!(near(&one, right, is_blue) > 20, "足した四面体が出る");

    // 2つ目: 立方体を足す。前に足した形状と読み込んだ形状はそのまま描ける。
    let block = Arc::new(cube(true));
    let id = doc.add_part(
        Arc::clone(&block),
        trs(Vec3::splat(1.2), Quat::IDENTITY, Vec3::new(-2.5, 0.0, 0.0)),
        YELLOW,
    );
    assert_eq!(id.0, 2);
    renderer
        .add_primitives(&device, &queue, &[Arc::clone(&block)])
        .expect("足せる");
    renderer.set_instances(&device, &queue, &doc, &Visibility::all_visible(3));
    let two = draw(&mut renderer, &device, &queue);
    save(&two, "added-two.png");
    assert!(near(&two, right, is_blue) > 20);
    assert!(near(&two, left, is_yellow) > 20, "2つ目も出る");
    let center = (SIZE[0] / 2, SIZE[1] / 2);
    assert_eq!(
        two.pixel(center.0, center.1),
        plain.pixel(center.0, center.1)
    );
    assert_eq!(
        renderer.added_geometry_bytes(),
        prim_bytes(&tetra) + prim_bytes(&block)
    );

    // 1回が32MiBを超える追加は断り、何も変えない。
    let count = (ADD_PRIMITIVES_LIMIT_BYTES / 24 + 1) as usize;
    let huge = Arc::new(Primitive {
        vertices: vec![
            Vertex {
                position: [0.0; 3],
                normal: [0.0, 1.0, 0.0],
            };
            count
        ],
        indices: Vec::new(),
        local_aabb: tetra.local_aabb,
        has_normals: true,
    });
    let stats = renderer.stats();
    match renderer.add_primitives(&device, &queue, &[huge]) {
        Err(RenderError::LimitExceeded { needed, limit, .. }) => {
            assert!(needed > limit);
            assert_eq!(limit, 32 * 1024 * 1024);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(renderer.stats(), stats);
    assert_eq!(renderer.primitive_slot_count(), 3);

    // モデルを閉じると、足した形状も一緒に捨てる。次のモデルは読み込んだ形状だけから始まる。
    renderer.clear_model();
    assert_eq!(renderer.added_geometry_bytes(), 0);
    assert_eq!(renderer.primitive_slot_count(), 0);
    upload(&mut renderer, &device, &queue, &base);
    assert_eq!(renderer.primitive_slot_count(), 1);
    renderer.set_instances(&device, &queue, &base, &Visibility::all_visible(1));
    let reopened = draw(&mut renderer, &device, &queue);
    assert_eq!(reopened.rgba, plain.rgba);

    // 別のモデルへ入れ替わったときも、前のモデルに足した形状は残らない。
    renderer
        .add_primitives(&device, &queue, &[Arc::clone(&tetra)])
        .expect("足せる");
    assert!(renderer.added_geometry_bytes() > 0);
    let other = Arc::new(rows());
    upload(&mut renderer, &device, &queue, &other);
    assert_eq!(renderer.added_geometry_bytes(), 0);
    assert_eq!(renderer.primitive_slot_count(), 2);
}
