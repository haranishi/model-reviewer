use std::path::{Path, PathBuf};

use app::{Command, Session, parse_args, session::selection_color};
use model_reviewer as app;
use model_reviewer_core::scene::{NodeId, SceneRead};
use model_reviewer_core::texture::{
    ImageHash, ImageId, ImageSource, Mip, Sampling, UvMap, Wrap, chain_bytes,
};
use model_reviewer_testdata::{
    Pattern, QuadImage, SamplerOptions, TextureTransform, TexturedOptions, UvFormat, jpeg_8x8,
    pattern_png, textured_quad_glb, textured_scene_glb,
};

#[path = "support/texture_stream.rs"]
mod texture_stream;
use texture_stream::{CountingImages, until};

fn file(name: &str, bytes: &[u8]) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("textures-{name}.glb"));
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn opening_textured_glb_reports_progress() {
    let mut s = Session::default();
    let args = parse_args(["app", "--texture-budget-mib", "1"]).unwrap();
    s.configure_textures(&args, Some(64));
    s.apply(Command::OpenModel(file(
        "progress",
        &textured_scene_glb(3, 128, 5),
    )));
    until(|| {
        s.poll_loader();
        s.textures.total == 3
    });
    assert!(s.model.is_some() && s.pick_index.is_some() && !s.edits_locked());
    assert_eq!(
        s.texture_progress_label().as_deref(),
        Some("テクスチャ 0/3")
    );
    let mut sink = CountingImages::default();
    let mut labels = Vec::new();
    until(|| {
        s.poll_loader();
        let before = s.textures.bytes;
        s.poll_images(&mut sink, 1024);
        assert!(s.textures.bytes - before <= 1024);
        if let Some(label) = s.texture_progress_label()
            && labels.last() != Some(&label)
        {
            labels.push(label);
        }
        !s.textures_busy()
    });
    assert_eq!(
        labels,
        ["テクスチャ 0/3", "テクスチャ 1/3", "テクスチャ 2/3"]
    );
    assert_eq!(
        sink.started,
        [
            (ImageId(0), [64; 2]),
            (ImageId(1), [64; 2]),
            (ImageId(2), [64; 2])
        ]
    );
    assert_eq!(s.textures.bytes, 3 * chain_bytes([64; 2]));
    assert_eq!(
        (s.textures.done, s.textures.displayed(), s.textures.shrunk),
        (3, 3, 3)
    );
    assert!(s.textures.failed.is_empty());
}

#[test]
fn textures_can_be_hidden_and_shown() {
    let mut s = Session::default();
    s.apply(Command::OpenModel(file(
        "toggle",
        &textured_quad_glb(&TexturedOptions::default()),
    )));
    let mut sink = CountingImages::default();
    sink.finish(&mut s);
    let _ = s.take_scene_dirty();
    for shown in [false, true] {
        s.apply(Command::SetTexturesShown(shown));
        assert_eq!(s.textures_shown, shown);
        let dirty = s.take_scene_dirty();
        assert!(dirty.display && !dirty.instances && !dirty.model);
        assert!(!s.has_unsaved());
        assert!(!s.poll_images(&mut sink, 65536));
        assert_eq!(
            (s.textures.displayed(), s.textures.bytes),
            (1, chain_bytes([8; 2]))
        );
    }
    assert_eq!(sink.started.len(), 1);
}

#[test]
fn color_note_is_per_part() {
    use app::command::{DisplayMode, ShapeKind};
    let mut glb = textured_scene_glb(2, 8, 2);
    let png = pattern_png(Pattern::Stripes, 8, 1);
    let at = glb.windows(png.len()).position(|w| w == png).unwrap();
    // 形式と寸法は読めるが、画素を展開できない画像にする。
    let idat = png.windows(4).position(|w| w == b"IDAT").unwrap();
    glb[at + idat + 4] = 0;
    let mut s = Session::default();
    s.apply(Command::OpenModel(file("part", &glb)));
    until(|| {
        s.poll_loader();
        s.model.is_some()
    });
    s.apply(Command::Select(Some(NodeId(0))));
    let note = |s: &Session| selection_color(s).unwrap().note;
    assert_eq!(note(&s), None);
    CountingImages::default().finish(&mut s);
    assert_eq!(note(&s), Some("色は画像に掛け合わされて見えます"));
    s.set_lines_supported(true);
    for mode in [
        DisplayMode::Lines,
        DisplayMode::XRay,
        DisplayMode::FacesAndLines,
    ] {
        s.apply(Command::SetDisplay(mode));
        assert_eq!(note(&s).is_some(), mode == DisplayMode::FacesAndLines);
    }
    s.apply(Command::SetTexturesShown(false));
    assert_eq!(note(&s), None);
    s.apply(Command::SetTexturesShown(true));
    assert!(note(&s).is_some());
    s.apply(Command::Select(Some(NodeId(1))));
    assert_eq!(s.textures.failed[0].0, ImageId(1));
    assert_eq!(note(&s), None);
    s.apply(Command::AddShape(ShapeKind::Cube));
    assert_eq!(note(&s), None);
}

#[test]
fn no_textures_flag_skips_decoding() {
    let mut s = Session::default();
    let args = parse_args(["app", "--no-textures", "--texture-budget-mib", "0"]).unwrap();
    s.configure_textures(&args, Some(4096));
    s.apply(Command::OpenModel(file(
        "disabled",
        &model_reviewer_testdata::rich_glb(),
    )));
    let mut sink = CountingImages::default();
    sink.finish(&mut s);
    assert_eq!(s.model.unwrap().textures.images.len(), 1);
    assert!(sink.started.is_empty() && s.textures.failed.is_empty() && !s.textures_shown);
    assert_eq!((s.textures.total, s.textures.bytes), (0, 0));
    assert!(s.metrics.texture_present.is_none() && s.metrics.texture_peak_mem.is_none());
    for value in ["-1", "1.5", "4294967296", ""] {
        assert!(parse_args(["app", "--texture-budget-mib", value]).is_err());
    }
    assert!(parse_args(["app", "--texture-budget-mib"]).is_err());
}

#[test]
fn quad_image_range_and_material_match_the_fixture() {
    let jpeg = jpeg_8x8();
    for uv_format in [UvFormat::U8, UvFormat::U16] {
        let glb = textured_quad_glb(&TexturedOptions {
            uv_format,
            image: QuadImage::Jpeg,
            sampler: Some(SamplerOptions {
                wrap_s: 33071,
                wrap_t: 33648,
                mag_filter: 9728,
                min_filter: 9987,
            }),
            transform: Some(TextureTransform {
                offset: [0.5, 0.25],
                rotation: std::f32::consts::FRAC_PI_2,
                scale: [2.0, 1.0],
                tex_coord: Some(1),
            }),
            mask_cutoff: Some(0.375),
            ..Default::default()
        });
        let model = model_reviewer_gltf::load_glb(&glb).unwrap();
        let draw = model.draws(NodeId(0))[0];
        let tex = model.tex_use(draw.tex.unwrap()).unwrap();
        let image = model.image(tex.image.unwrap()).unwrap();
        let ImageSource::File { offset, len } = image.source else {
            panic!("BIN 内の画像")
        };
        let bin = 28 + u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let start = bin + offset as usize;
        assert_eq!(&glb[start..start + len as usize], jpeg);
        assert_eq!(image.hash, ImageHash::of(&jpeg));
        assert_eq!(
            model.prim_uvs(draw.prim),
            &[[1., 1.], [0., 1.], [0., 0.], [1., 0.]]
        );
        assert_eq!(
            tex.sampling,
            Sampling {
                wrap: [Wrap::Clamp, Wrap::Mirror],
                mag_linear: false,
                min_linear: true,
                mip: Mip::Linear
            }
        );
        let UvMap::Vertex {
            transform,
            tex_coord,
            info_from,
        } = tex.map
        else {
            panic!("頂点 UV")
        };
        assert_eq!(
            (tex_coord, info_from, tex.cutoff),
            (1, Some(0), Some(0.375))
        );
        for (actual, expected) in transform.into_iter().zip([0., -1., 0.5, 2., 0., 0.25]) {
            assert!((actual - expected).abs() < 1e-6);
        }
    }
}

#[test]
fn switching_models_drops_queued_images() {
    let mut s = Session::default();
    let previous = file("previous", &textured_scene_glb(4, 128, 4));
    let next = file("next", &textured_scene_glb(1, 8, 1));
    for replacement in [Command::OpenModel(next), Command::NewDocument] {
        s.apply(Command::OpenModel(previous.clone()));
        until(|| {
            s.poll_loader();
            s.textures.total == 4 && !s.is_loading()
        });
        let mut old = CountingImages::default();
        until(|| {
            s.poll_loader();
            s.poll_images(&mut old, 4);
            s.textures.bytes == 4
        });
        s.apply(replacement);
        let mut new = CountingImages::default();
        until(|| {
            s.poll_loader();
            !s.is_loading()
        });
        new.finish(&mut s);
        if s.is_new_document() {
            assert!(new.started.is_empty());
            assert_eq!((s.textures.done, s.textures.bytes), (0, 0));
        } else {
            assert_eq!(new.started, [(ImageId(0), [8; 2])]);
            assert_eq!(
                (s.textures.done, s.textures.bytes),
                (1, chain_bytes([8; 2]))
            );
        }
    }
}
