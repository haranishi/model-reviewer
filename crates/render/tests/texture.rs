//! 復号済みの画素を形状の後に送り、面の色と転送量をオフスクリーンで確かめる。

mod common;

use std::sync::Arc;

use common::{cube, device_with_lines, differing_pixels, model, node};
use glam::{Affine3A, Vec3};
use model_reviewer_core::math::Aabb;
use model_reviewer_core::scene::{Model, Primitive, Vertex};
use model_reviewer_core::texture::{
    IDENTITY_TRANSFORM, ImageHash, ImageId, ImageInfo, ImageSource, Mime, Mip, MipChain, Sampling,
    TexId, TexUse, UvMap, Wrap, box_uv, chain_bytes,
};
use model_reviewer_core::visibility::Visibility;
use model_reviewer_render::{DisplayMode, FrameParams, Image, ImageProgress, SceneRenderer};

const WHITE: [f32; 4] = [1.0; 4];
const ID: ImageId = ImageId(0);
const COLORS: [[u8; 4]; 4] = [
    [255, 0, 0, 255],
    [0, 255, 0, 255],
    [0, 0, 255, 255],
    [255, 255, 0, 255],
];
const UV: UvMap = UvMap::Vertex {
    transform: IDENTITY_TRANSFORM,
    tex_coord: 0,
    info_from: None,
};

fn usage(map: UvMap) -> TexUse {
    TexUse {
        image: Some(ID),
        map,
        sampling: Sampling {
            mag_linear: false,
            min_linear: false,
            mip: Mip::Nearest,
            ..Default::default()
        },
        cutoff: None,
        baked_box: None,
    }
}

fn pattern() -> MipChain {
    MipChain {
        size: [2, 2],
        levels: vec![COLORS.concat(), vec![180, 180, 128, 255]],
    }
}

fn solid(color: [u8; 4]) -> MipChain {
    MipChain {
        size: [1, 1],
        levels: vec![color.to_vec()],
    }
}

fn mapped(prim: Primitive, world: Affine3A, color: [f32; 4], material: Option<TexUse>) -> Model {
    let mut m = model(
        vec![prim],
        vec![node("部品", None, world, vec![(0, color)])],
    );
    if let Some(material) = material {
        m.draws[0].tex = Some(TexId(0));
        m.textures.uses.push(material);
        m.textures.images.push(ImageInfo {
            hash: ImageHash::of(&[]),
            mime: Mime::Png,
            source: ImageSource::File { offset: 0, len: 0 },
        });
    }
    m
}

fn quad(material: Option<TexUse>, color: [f32; 4]) -> Model {
    let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    let prim = Primitive {
        vertices: uvs
            .iter()
            .map(|&[u, v]| Vertex {
                position: [u - 0.5, 0.5 - v, 0.0],
                normal: [0.0, 0.0, 1.0],
            })
            .collect(),
        indices: vec![0, 1, 2, 0, 2, 3],
        local_aabb: Aabb {
            min: Vec3::new(-0.5, -0.5, 0.0),
            max: Vec3::new(0.5, 0.5, 0.0),
        },
        has_normals: true,
    };
    let mut m = mapped(prim, Affine3A::IDENTITY, color, material);
    if material.is_some_and(|t| matches!(t.map, UvMap::Vertex { .. }) && t.image.is_some()) {
        m.textures.prim_uvs.push(uvs.to_vec());
    }
    m
}

struct Scene {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: SceneRenderer,
}

impl Scene {
    fn new() -> Self {
        let (device, queue) = device_with_lines();
        let renderer = SceneRenderer::new(&device);
        Self {
            device,
            queue,
            renderer,
        }
    }

    fn show(&mut self, model: Model) {
        let m = Arc::new(model);
        self.renderer
            .begin_upload(&self.device, Arc::clone(&m))
            .unwrap();
        while !self.renderer.continue_upload(&self.queue, 36).done {}
        self.renderer.set_instances(
            &self.device,
            &self.queue,
            &m,
            &Visibility::all_visible(m.nodes.len()),
        );
        assert_eq!(self.renderer.take_error(), None);
    }

    fn begin(&mut self, chain: MipChain) {
        self.renderer.begin_image(&self.device, ID, chain).unwrap();
    }

    fn step(&mut self, budget: u64) -> ImageProgress {
        let p = self
            .renderer
            .continue_images(&self.device, &self.queue, budget);
        assert!(p.uploaded_bytes <= budget);
        p
    }

    fn upload(&mut self, chain: MipChain) {
        let bytes = chain_bytes(chain.size);
        self.begin(chain);
        assert_eq!(
            self.step(bytes),
            ImageProgress {
                uploaded_bytes: bytes,
                completed: Some(ID)
            }
        );
    }

    fn draw(&mut self, frame: &FrameParams) -> Image {
        self.renderer.render(&self.device, &self.queue, frame);
        assert_eq!(self.renderer.take_error(), None);
        self.renderer.read_image(&self.device, &self.queue).unwrap()
    }
}

fn frame(eye: Vec3, perspective: bool) -> FrameParams {
    let up = if eye.normalize().dot(Vec3::Y).abs() > 0.9 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    let view = glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, up);
    use glam::camera::rh::proj::directx;
    let proj = if perspective {
        directx::perspective_infinite_reverse(0.65, 1.0, 0.05)
    } else {
        directx::orthographic(-0.75, 0.75, -0.75, 0.75, 100.0, 0.05)
    };
    FrameParams {
        size_px: [180; 2],
        view_proj: proj * view,
        eye,
        selected: None,
        clear: [0.0, 0.0, 0.0, 1.0],
        display: DisplayMode::Faces,
        grid: Default::default(),
        drag: None,
        section: None,
    }
}

fn sample(image: &Image, frame: &FrameParams, world: Vec3) -> [u8; 4] {
    let p = frame.view_proj.project_point3(world);
    let x = (p.x * 0.5 + 0.5) * image.width as f32;
    let y = (0.5 - p.y * 0.5) * image.height as f32;
    image.pixel(x as u32, y as u32)
}

// 4色の明暗には照明分の幅を持たせる。
fn assert_color(pixel: [u8; 4], color: [u8; 4]) {
    for i in 0..3 {
        assert!(
            if color[i] == 0 {
                pixel[i] < 12
            } else {
                pixel[i] > 150
            },
            "画素 {pixel:?}、元の色 {color:?}"
        );
    }
}

fn quadrants(image: &Image, frame: &FrameParams) -> [[u8; 4]; 4] {
    [(-0.25, 0.25), (0.25, 0.25), (-0.25, -0.25), (0.25, -0.25)]
        .map(|(x, y)| sample(image, frame, Vec3::new(x, y, 0.0)))
}

#[test]
#[ignore = "GPUが必要"]
fn textured_quad_shows_image_not_factor() {
    let mut scene = Scene::new();
    let f = frame(3.0 * Vec3::Z, false);
    scene.show(quad(Some(usage(UV)), WHITE));
    scene.upload(pattern());
    for (p, c) in quadrants(&scene.draw(&f), &f).into_iter().zip(COLORS) {
        assert_color(p, c);
    }

    // u'=1+v を反転して繰り返し、v'=u。行列の上下の行・ずらしとサンプラを一緒に通す。
    let mut swapped = usage(UvMap::Vertex {
        transform: [0.0, 1.0, 1.0, 1.0, 0.0, 0.0],
        tex_coord: 1,
        info_from: None,
    });
    swapped.sampling.wrap = [Wrap::Mirror, Wrap::Clamp];
    scene.show(quad(Some(swapped), WHITE));
    scene.upload(pattern());
    for (p, i) in quadrants(&scene.draw(&f), &f).into_iter().zip([1, 3, 0, 2]) {
        assert_color(p, COLORS[i]);
    }

    // 赤と緑の中間は、リニアで混ぜてから画面の sRGB に戻る。
    let mut linear = usage(UV);
    linear.sampling = Sampling::default();
    scene.show(quad(Some(linear), WHITE));
    let mut stripes = pattern();
    stripes.levels[0] = [COLORS[0], COLORS[1]].concat().repeat(2);
    scene.upload(stripes);
    let [r, g, b, _] = sample(&scene.draw(&f), &f, Vec3::ZERO);
    assert!(
        (155..205).contains(&r) && r.abs_diff(g) < 9 && b < 12,
        "{r} {g} {b}"
    );
}

#[test]
#[ignore = "GPUが必要"]
fn factor_tints_texture() {
    let mut scene = Scene::new();
    let f = frame(3.0 * Vec3::Z, false);
    let color = [0.25, 0.5, 0.75, 0.3];
    let texel = [128, 190, 78, 24];
    scene.show(quad(Some(usage(UV)), color));
    scene.upload(solid(texel));
    let tinted = scene.draw(&f);
    let mut plain = color;
    for i in 0..3 {
        let srgb = f32::from(texel[i]) / 255.0;
        plain[i] *= ((srgb + 0.055) / 1.055).powf(2.4);
    }
    scene.show(quad(None, plain));
    let reference = scene.draw(&f);
    assert!(sample(&reference, &f, Vec3::ZERO)[1] > 90);
    assert_eq!(differing_pixels(&tinted.rgba, &reference.rgba, 3), 0);
}

#[test]
#[ignore = "GPUが必要"]
fn mask_cuts_out_pixels() {
    let mut scene = Scene::new();
    let f = frame(3.0 * Vec3::Z, false);
    let mut mask = usage(UV);
    mask.cutoff = Some(0.5);
    scene.show(quad(Some(mask), [1.0, 1.0, 1.0, 0.5]));
    let mut image = pattern();
    let (pixels, _) = image.levels[0].as_chunks_mut::<4>();
    for (px, a) in pixels.iter_mut().zip([0, 200, 255, 255]) {
        px[3] = a;
    }
    scene.upload(image);
    let q = quadrants(&scene.draw(&f), &f);
    let clear = [0, 0, 0, 255];
    assert_color(q[0], clear);
    assert_color(q[1], clear); // 画像だけなら閾値を超える。
    assert_color(q[2], COLORS[2]); // 閾値と等しければ描く。
    assert_color(q[3], COLORS[3]);

    mask.image = None;
    scene.show(quad(Some(mask), [1.0, 1.0, 1.0, 0.25]));
    assert_color(sample(&scene.draw(&f), &f, Vec3::ZERO), clear);
    scene.show(quad(Some(mask), [1.0, 1.0, 1.0, 0.5]));
    assert_color(sample(&scene.draw(&f), &f, Vec3::ZERO), [255; 4]);
}

#[test]
#[ignore = "GPUが必要"]
fn mip_is_used_far_away() {
    let mut scene = Scene::new();
    let f = frame(20.0 * Vec3::Z, true);
    let levels = (0..=8)
        .map(|i| {
            let side = 256 >> i;
            COLORS[usize::from(i > 0)].repeat(side * side)
        })
        .collect();
    let chain = MipChain {
        size: [256; 2],
        levels,
    };
    for (mip, color) in [(Mip::Linear, COLORS[1]), (Mip::None, COLORS[0])] {
        let mut mat = usage(UV);
        mat.sampling = Sampling {
            mip,
            ..Default::default()
        };
        scene.show(quad(Some(mat), WHITE));
        scene.upload(chain.clone());
        assert_color(sample(&scene.draw(&f), &f, Vec3::ZERO), color);
    }
}

#[test]
#[ignore = "GPUが必要"]
fn hidden_textures_match_plain_render() {
    let mut scene = Scene::new();
    let mut f = frame(3.0 * Vec3::Z, false);
    let color = [0.5, 0.6, 0.7, 1.0];
    for mode in [
        DisplayMode::Faces,
        DisplayMode::FacesAndLines,
        DisplayMode::Lines,
        DisplayMode::XRay,
    ] {
        f.display = mode;
        scene.show(quad(None, color));
        let plain = scene.draw(&f);
        scene.show(quad(Some(usage(UV)), color));
        scene.upload(pattern());
        let before = scene.renderer.stats();
        scene.renderer.set_textures_shown(false);
        assert_eq!(differing_pixels(&scene.draw(&f).rgba, &plain.rgba, 2), 0);
        scene.renderer.set_textures_shown(true);
        let changed = differing_pixels(&scene.draw(&f).rgba, &plain.rgba, 4);
        if matches!(mode, DisplayMode::Lines | DisplayMode::XRay) {
            assert_eq!(changed, 0);
        } else {
            assert!(changed > 1000);
        }
        let after = scene.renderer.stats();
        assert_eq!(after.instance_rebuilds, before.instance_rebuilds);
        assert_eq!(after.uploaded_bytes, before.uploaded_bytes);
        assert_eq!((after.texture_count, after.texture_bytes), (1, 20));
    }

    // 同じ深度に材質の違う描画が重なると、まとめ直した順と元の順で絵が変わる。
    let layered = |mat| {
        let mut m = quad(mat, color);
        let mut second = m.draws[0];
        second.tex = None;
        second.base_color = [0.9, 0.1, 0.1, 1.0];
        m.draws.push(second);
        m.nodes[0].draws.end = 2;
        m
    };
    for mode in [DisplayMode::Faces, DisplayMode::XRay] {
        f.display = mode;
        scene.show(layered(None));
        let plain = scene.draw(&f);
        scene.show(layered(Some(usage(UV))));
        scene.upload(pattern());
        scene.renderer.set_textures_shown(mode == DisplayMode::XRay);
        assert_eq!(differing_pixels(&scene.draw(&f).rgba, &plain.rgba, 2), 0);
    }
}

fn check_box(world: Affine3A, center: Vec3, wide: bool) {
    let mut scene = Scene::new();
    let size = 0.6;
    let mut prim = cube(false);
    for vertex in &mut prim.vertices {
        vertex.position = (Vec3::from(vertex.position) + center).to_array();
    }
    prim.local_aabb.min += center;
    prim.local_aabb.max += center;
    let world = world * Affine3A::from_translation(-center);
    scene.show(mapped(
        prim,
        world,
        WHITE,
        Some(usage(UvMap::Box {
            size,
            aspect: if wide { 2.0 } else { 1.0 },
        })),
    ));
    let mut image = pattern();
    if wide {
        image.size = [4, 2];
        image.levels[0] = COLORS.into_iter().flat_map(|c| [c, c]).flatten().collect();
        image.levels.insert(1, COLORS[..2].concat());
    }
    scene.upload(image);
    for n in [
        Vec3::X,
        Vec3::NEG_X,
        Vec3::Y,
        Vec3::NEG_Y,
        Vec3::Z,
        Vec3::NEG_Z,
    ] {
        let u = if n.x != 0.0 { Vec3::Z } else { Vec3::X };
        let v = n.cross(u);
        let eye = world.transform_point3(center + n * 3.0 + u * 0.25 + v * 0.2);
        for perspective in [false, true] {
            let f = frame(eye, perspective);
            let image = scene.draw(&f);
            for a in [-0.36, -0.11, 0.18, 0.35] {
                for b in [-0.36, -0.11, 0.18, 0.35] {
                    let pos = center + n * 0.5 + u * a + v * b;
                    let [x, y] =
                        box_uv(pos, n, center, [size, if wide { size / 2.0 } else { size }])
                            .map(|t| (t.rem_euclid(1.0) * 2.0) as usize);
                    assert_color(
                        sample(&image, &f, world.transform_point3(pos)),
                        COLORS[y * 2 + x],
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "GPUが必要"]
fn box_projection_matches_box_uv_on_six_faces() {
    check_box(Affine3A::IDENTITY, Vec3::ZERO, false);
}

#[test]
#[ignore = "GPUが必要"]
fn box_projection_on_mirrored_part() {
    check_box(
        Affine3A::from_scale(Vec3::new(-1.0, 1.0, 1.0)),
        Vec3::ZERO,
        false,
    );
}

#[test]
#[ignore = "GPUが必要"]
fn offset_box_centers_wide_image() {
    check_box(Affine3A::IDENTITY, Vec3::splat(0.5), true);
}

#[test]
#[ignore = "GPUが必要"]
fn placeholder_then_image() {
    let mut scene = Scene::new();
    let f = frame(3.0 * Vec3::Z, false);
    let mut m = quad(Some(usage(UV)), WHITE);
    m.textures.images.push(ImageInfo {
        hash: ImageHash::of(&[1]),
        mime: Mime::Png,
        source: ImageSource::File { offset: 0, len: 1 },
    });
    scene.show(m);
    let before = scene.renderer.stats();
    let plain = scene.draw(&f);
    assert_color(sample(&plain, &f, Vec3::ZERO), [255; 4]);
    // 別の画像が先に届いても、この材質は最後の縮小版まで白の組を使う。
    scene
        .renderer
        .begin_image(&scene.device, ImageId(1), solid(COLORS[0]))
        .unwrap();
    assert_eq!(scene.step(4).completed, Some(ImageId(1)));
    scene.begin(pattern());
    assert_eq!(scene.step(3), ImageProgress::default());
    for _ in 0..4 {
        assert_eq!(
            scene.step(7),
            ImageProgress {
                uploaded_bytes: 4,
                completed: None
            }
        );
        assert_eq!(differing_pixels(&scene.draw(&f).rgba, &plain.rgba, 2), 0);
    }
    assert_eq!(
        scene.step(7),
        ImageProgress {
            uploaded_bytes: 4,
            completed: Some(ID)
        }
    );
    for (p, c) in quadrants(&scene.draw(&f), &f).into_iter().zip(COLORS) {
        assert_color(p, c);
    }
    scene.upload(solid(COLORS[2]));
    assert_color(sample(&scene.draw(&f), &f, Vec3::ZERO), COLORS[2]);
    let after = scene.renderer.stats();
    assert_eq!((after.texture_count, after.texture_bytes), (2, 8));
    assert_eq!(after.instance_rebuilds, before.instance_rebuilds);
    assert_eq!(after.uploaded_bytes, before.uploaded_bytes);
    assert_eq!(after.textured_draws - before.textured_draws, 2);

    scene.begin(pattern());
    scene.step(4);
    scene.show(quad(Some(usage(UV)), WHITE));
    assert_eq!(scene.step(1000), ImageProgress::default());
    assert_eq!(scene.renderer.stats().texture_count, 0);
    assert_eq!(scene.renderer.stats().texture_bytes, 0);
    assert_eq!(differing_pixels(&scene.draw(&f).rgba, &plain.rgba, 2), 0);
}

#[test]
#[ignore = "GPUが必要"]
fn untextured_model_uploads_same_bytes() {
    let mut scene = Scene::new();
    scene.show(quad(None, WHITE));
    let f = frame(3.0 * Vec3::Z, false);
    let on = scene.draw(&f);
    scene.renderer.set_textures_shown(false);
    assert_eq!(differing_pixels(&scene.draw(&f).rgba, &on.rgba, 2), 0);
    assert_color(sample(&on, &f, Vec3::ZERO), [255; 4]);
    let stats = scene.renderer.stats();
    assert_eq!(stats.uploaded_bytes, 4 * 24 + 6 * 4 + 144);
    assert_eq!(
        (
            stats.texture_bytes,
            stats.texture_count,
            stats.textured_draws
        ),
        (0, 0, 0)
    );
}
