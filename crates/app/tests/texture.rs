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

use app::command::ShapeKind;
use app::session::texture_view;
use model_reviewer_core::doc::DrawLook;
use model_reviewer_core::texture::{TexUse, same_look};
use model_reviewer_testdata::{sample_glb, stl_sample};

fn folder(test: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("paste")
        .join(test);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn png(test: &str) -> PathBuf {
    let path = folder(test).join("pattern.png");
    std::fs::write(&path, pattern_png(Pattern::Stripes, 8, 3)).unwrap();
    path
}

fn opened(test: &str, bytes: &[u8], selection: u32) -> Session {
    let mut s = Session::default();
    s.apply(Command::OpenModel(file(test, bytes)));
    CountingImages::default().finish(&mut s);
    assert!(s.doc.is_some(), "{}", s.status_text);
    s.apply(Command::Select(Some(NodeId(selection))));
    s
}

fn cube() -> Session {
    let mut s = Session::default();
    s.apply(Command::NewDocument);
    s.apply(Command::AddShape(ShapeKind::Cube));
    s
}

fn paste(s: &mut Session, path: PathBuf) {
    s.apply(Command::PasteTexture(path));
    CountingImages::default().finish(s);
    assert_eq!(s.status_text, "画像を貼りました");
}

fn looks(s: &Session, id: NodeId) -> Vec<DrawLook> {
    let doc = s.doc.as_ref().unwrap();
    doc.subtree_looks(doc.key(id).unwrap()).unwrap()
}

fn selected_tex(s: &Session) -> TexUse {
    let doc = s.doc.as_ref().unwrap();
    let look = looks(s, s.selection.unwrap())[0];
    *doc.tex_use(look.tex.unwrap()).unwrap()
}

fn export(s: &mut Session, to: &Path) {
    s.apply(Command::ExportGlb(to.to_owned()));
    until(|| {
        s.poll_exporter();
        !s.is_exporting()
    });
}

#[test]
fn paste_applies_to_selection_and_descendants() {
    let arm = NodeId(9);
    let mut s = opened("subtree", &sample_glb(), arm.0);
    let before = looks(&s, arm);
    let outside = looks(&s, NodeId(3));
    assert!(before.len() > 1);
    let _ = s.take_scene_dirty();
    paste(&mut s, png("subtree"));
    let after = looks(&s, arm);
    assert_eq!(before.len(), after.len());
    assert!(after.iter().all(|l| l.tex.is_some()));
    assert_eq!(looks(&s, NodeId(3)), outside);
    assert_eq!(s.doc.as_ref().unwrap().images().count(), 1);
    assert_eq!(s.textures.displayed(), 1);
    assert!(s.take_scene_dirty().instances && s.doc_unsaved());
    assert_eq!(s.history.len(), 1);
    s.apply(Command::Undo);
    assert_eq!(looks(&s, arm), before);
    assert!(s.take_scene_dirty().instances && !s.doc_unsaved());
    s.apply(Command::Redo);
    assert_eq!(looks(&s, arm), after);
    assert!(s.take_scene_dirty().instances && s.doc_unsaved());
}

#[test]
fn paste_sets_rgb_white_keeps_alpha() {
    let options = TexturedOptions {
        base_color_factor: [0.2, 0.4, 0.6, 0.25],
        mask_cutoff: Some(0.125),
        tex_coord: 1,
        sampler: Some(SamplerOptions {
            wrap_s: 33071,
            wrap_t: 33648,
            mag_filter: 9728,
            min_filter: 9984,
        }),
        transform: Some(TextureTransform {
            offset: [0.1, 0.3],
            rotation: 0.2,
            scale: [1.5, 2.0],
            tex_coord: None,
        }),
        ..Default::default()
    };
    let mut s = opened("rgba", &textured_quad_glb(&options), 0);
    let before = selected_tex(&s);
    paste(&mut s, png("rgba"));
    let after = selected_tex(&s);
    assert_ne!(before.image, after.image);
    assert_eq!(
        after,
        TexUse {
            image: after.image,
            ..before
        }
    );
    assert_eq!(looks(&s, NodeId(0))[0].base_color, [1., 1., 1., 0.25]);
    assert!(!texture_view(&s).unwrap().has_box);
    let target = folder("rgba").join("out.glb");
    export(&mut s, &target);
    assert!(s.export_error().is_none(), "{:?}", s.export_error());
    let reopened = opened("rgba-again", &std::fs::read(target).unwrap(), 0);
    let actual = selected_tex(&reopened);
    let hash = |s: &Session, t: TexUse| {
        s.doc
            .as_ref()
            .unwrap()
            .image(t.image.unwrap())
            .unwrap()
            .hash
    };
    assert!(same_look(
        Some((&after, Some(hash(&s, after)))),
        Some((&actual, Some(hash(&reopened, actual))))
    ));
}

#[test]
fn paste_without_selection_is_refused() {
    let mut s = cube();
    s.apply(Command::Select(None));
    let rev = s.history.doc_rev();
    for cmd in [
        Command::PasteTextureDialog,
        Command::PasteTexture(png("none")),
        Command::DroppedFiles(vec![png("none")]),
    ] {
        s.apply(cmd);
        assert_eq!(s.status_text, "貼る部品を選んでから、画像を貼ってください");
        assert!(!s.is_loading() && !s.textures_busy() && s.dialog.is_none());
        assert_eq!(s.history.doc_rev(), rev);
    }
}

#[test]
fn paste_targets_the_part_selected_at_request() {
    let mut s = opened("target", &sample_glb(), 3);
    s.apply(Command::PasteTexture(png("target")));
    s.apply(Command::Select(Some(NodeId(4))));
    CountingImages::default().finish(&mut s);
    assert_eq!(s.selection, Some(NodeId(4)));
    assert!(looks(&s, NodeId(3))[0].tex.is_some());
    assert!(looks(&s, NodeId(4))[0].tex.is_none());
}

#[test]
fn paste_is_dropped_when_another_model_opens_meanwhile() {
    let mut s = opened("generation-a", &sample_glb(), 3);
    let old_key = s.doc.as_ref().unwrap().key(NodeId(3));
    s.apply(Command::PasteTexture(png("generation")));
    s.apply(Command::OpenModel(file("generation-b", &sample_glb())));
    CountingImages::default().finish(&mut s);
    assert_eq!(s.doc.as_ref().unwrap().key(NodeId(3)), old_key);
    assert_eq!(s.doc.as_ref().unwrap().images().count(), 0);
    assert_eq!(s.textures.displayed(), 0);
    assert!(!s.doc_unsaved());
    assert!(!s.opened_files().contains(&png("generation")));
}

#[test]
fn paste_is_dropped_when_document_changes_meanwhile() {
    let mut s = cube();
    for revert in [false, true] {
        let path = png("revision");
        s.apply(Command::PasteTexture(path.clone()));
        s.apply(Command::DuplicateSelected);
        if revert {
            s.apply(Command::Undo);
        }
        let mut sink = CountingImages::default();
        sink.finish(&mut s);
        assert_eq!(
            s.status_text,
            "貼る間に文書が変わったため、貼りませんでした"
        );
        assert_eq!(s.doc.as_ref().unwrap().images().count(), 0);
        assert!(sink.started.is_empty() && !s.opened_files().contains(&path));
    }
}

#[test]
fn paste_rejects_invalid_files_and_full_budget() {
    let mut s = cube();
    let args = parse_args(["app", "--texture-budget-mib", "0"]).unwrap();
    s.configure_textures(&args, Some(4096));
    let rev = s.history.doc_rev();
    let dir = folder("reject");
    let large = dir.join("large.png");
    std::fs::File::create(&large)
        .unwrap()
        .set_len((64 << 20) + 1)
        .unwrap();
    let broken = dir.join("broken.png");
    std::fs::write(&broken, b"\x89PNG\r\n\x1a\n").unwrap();
    for (path, reason) in [
        (large, "64 MiB"),
        (broken, "画像を読めません"),
        (png("reject"), "上限（0 MiB）に空きがありません"),
    ] {
        s.apply(Command::PasteTexture(path.clone()));
        let mut sink = CountingImages::default();
        sink.finish(&mut s);
        assert!(s.status_text.contains(reason), "{}", s.status_text);
        assert_eq!(s.history.doc_rev(), rev);
        assert!(s.doc.as_ref().unwrap().images().next().is_none());
        assert!(sink.started.is_empty() && !s.opened_files().contains(&path));
    }
}

#[test]
fn paste_reuses_resident_image() {
    let mut s = cube();
    let path = png("reuse");
    paste(&mut s, path);
    let original = selected_tex(&s);
    let bytes = s.textures.bytes;
    let rev = s.history.doc_rev();
    let args = parse_args(["app", "--texture-budget-mib", "0"]).unwrap();
    s.configure_textures(&args, Some(4096));
    let same = png("reuse-other-name");
    s.apply(Command::PasteTexture(same.clone()));
    let mut sink = CountingImages::default();
    sink.finish(&mut s);
    assert_eq!(s.status_text, "画像を貼りました");
    assert_eq!(selected_tex(&s), original);
    assert_eq!(s.history.doc_rev(), rev);
    assert_eq!(s.textures.bytes, bytes);
    assert_eq!(s.textures.total, 1);
    assert!(sink.started.is_empty() && s.opened_files().contains(&same));
}

#[test]
fn box_paste_on_skinned_part_is_refused() {
    let mut s = opened("skin", &model_reviewer_testdata::rich_glb(), 0);
    let doc = s.doc.as_ref().unwrap();
    let skinned = doc.find_file_index(4).unwrap();
    // 親を選んだときも、子孫のうち1つでも対象なら何も貼らない。
    for id in [NodeId(0), skinned] {
        s.apply(Command::Select(Some(id)));
        s.apply(Command::PasteTexture(png("skin")));
        assert_eq!(
            s.status_text,
            "スキンかモーフのある部品には、この貼り方では貼れません"
        );
        assert!(!s.doc_unsaved() && !s.textures_busy());
    }
    s.apply(Command::DuplicateSelected);
    let copy = s.selection.unwrap();
    assert!(texture_view(&s).unwrap().paste_block.is_some());
    export(&mut s, &folder("skin").join("duplicate.glb"));
    assert!(s.export_error().is_none(), "{:?}", s.export_error());
    assert_eq!(s.selection, Some(copy));
    assert_eq!(
        texture_view(&s).unwrap().paste_block,
        Some("スキンかモーフのある部品には、この貼り方では貼れません")
    );
}

#[test]
fn box_paste_keeps_mask_cutoff() {
    let mut s = cube();
    let id = s.selection.unwrap();
    let doc = s.doc.as_mut().unwrap();
    let key = doc.key(id).unwrap();
    let mask = TexUse {
        image: None,
        sampling: Sampling::default(),
        map: UvMap::Vertex {
            transform: model_reviewer_core::texture::IDENTITY_TRANSFORM,
            tex_coord: 0,
            info_from: None,
        },
        baked_box: None,
        cutoff: Some(0.375),
    };
    let mask_id = doc.add_tex_use(mask).unwrap();
    doc.set_subtree_looks(
        key,
        &[DrawLook {
            base_color: [0.3; 4],
            tex: Some(mask_id),
        }],
    )
    .unwrap();
    paste(&mut s, png("mask"));
    let tex = selected_tex(&s);
    assert!(matches!(tex.map, UvMap::Box { .. }));
    assert_eq!(tex.cutoff, Some(0.375));
    assert_eq!(looks(&s, id)[0].base_color, [1., 1., 1., 0.3]);
    s.apply(Command::RemoveTexture);
    assert_eq!(selected_tex(&s), mask);
    s.apply(Command::Undo);
    assert_eq!(selected_tex(&s), tex);
}

#[test]
fn remove_texture_keeps_color() {
    let mut s = opened(
        "remove",
        &textured_quad_glb(&TexturedOptions {
            base_color_factor: [0.3, 0.4, 0.5, 0.6],
            ..Default::default()
        }),
        0,
    );
    let original = looks(&s, NodeId(0))[0];
    assert!(original.tex.is_some());
    s.apply(Command::RemoveTexture);
    assert_eq!(
        looks(&s, NodeId(0))[0],
        DrawLook {
            tex: None,
            ..original
        }
    );
    s.apply(Command::RemoveTexture);
    assert_eq!(s.history.len(), 1);
    s.apply(Command::Undo);
    assert_eq!(looks(&s, NodeId(0))[0], original);
}

#[test]
fn repasting_removed_mask_keeps_original_uv() {
    let options = TexturedOptions {
        tex_coord: 1,
        mask_cutoff: Some(0.4),
        transform: Some(TextureTransform {
            offset: [0.1, 0.4],
            rotation: 0.3,
            scale: [2., 3.],
            tex_coord: None,
        }),
        ..Default::default()
    };
    let mut s = opened("repaste", &textured_quad_glb(&options), 0);
    let original = selected_tex(&s);
    s.apply(Command::RemoveTexture);
    export(&mut s, &folder("repaste").join("removed.glb"));
    assert!(s.export_error().is_none(), "{:?}", s.export_error());
    paste(&mut s, png("repaste"));
    let actual = selected_tex(&s);
    assert_ne!(actual.image, original.image);
    assert_eq!(
        actual,
        TexUse {
            image: actual.image,
            ..original
        }
    );
    export(&mut s, &folder("repaste").join("restored.glb"));
    assert!(s.export_error().is_none(), "{:?}", s.export_error());
}

#[test]
fn size_field_changes_box_size() {
    use egui_kittest::{Harness, kittest::Queryable};
    use model_reviewer_core::units::UnitLabel;

    let mut s = cube();
    paste(&mut s, png("field"));
    s.apply(Command::SetDisplayUnit(Some(UnitLabel::Cm)));
    let original = selected_tex(&s);
    let mut h = Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_size(egui::vec2(320., 200.))
        .build_ui_state(
            |ui, session| {
                let mut commands = Vec::new();
                let input = texture_view(session).unwrap();
                app::ui::texture::show(ui, &input, &mut commands);
                for command in commands {
                    session.apply(command);
                }
            },
            s,
        );
    h.get_by_label("1枚の大きさ").click();
    h.step();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.event(egui::Event::Text("25".into()));
    h.step();
    assert_eq!(selected_tex(h.state()), original);
    h.key_press(egui::Key::Enter);
    h.step();
    assert_eq!(selected_tex(h.state()).map, UvMap::Box { size: 0.25 });
    let rev = h.state().history.doc_rev();
    h.state_mut().apply(Command::SetTextureSize(0.25));
    assert_eq!(h.state().history.doc_rev(), rev);
    h.state_mut().apply(Command::Undo);
    assert_eq!(selected_tex(h.state()), original);
}

#[test]
fn stl_paste_uses_scale_for_size() {
    let mut s = opened("millimeters", &stl_sample(), 0);
    paste(&mut s, png("millimeters"));
    let UvMap::Box { size } = selected_tex(&s).map else {
        panic!("箱形の投影")
    };
    assert!((size - 40.).abs() < 0.0001);
    let view = texture_view(&s).unwrap();
    assert!((view.world_size.unwrap() - 0.04).abs() < 1e-6);
    assert_eq!(view.units.shown().suffix(), "mm");
    s.apply(Command::SetTextureSize(0.01));
    let UvMap::Box { size } = selected_tex(&s).map else {
        panic!("箱形の投影")
    };
    assert!((size - 10.).abs() < 0.0001);
}

#[test]
fn dropped_image_pastes_dropped_glb_opens() {
    let mut s = cube();
    s.apply(Command::DroppedFiles(vec![file(
        "not-image-extension",
        &jpeg_8x8(),
    )]));
    CountingImages::default().finish(&mut s);
    assert!(selected_tex(&s).image.is_some() && s.is_new_document());
    let model = folder("drop").join("actually-model.jpg");
    std::fs::write(&model, sample_glb()).unwrap();
    s.apply(Command::DroppedFiles(vec![model.clone()]));
    s.apply(Command::ConfirmDiscard);
    CountingImages::default().finish(&mut s);
    assert_eq!(
        s.model_ref.unwrap().path,
        std::fs::canonicalize(model).unwrap()
    );
}

// 注釈の中にだけある文字。JPEG の先頭に正規の COM 区画を足す。
fn annotated_jpeg(comment: &[u8]) -> Vec<u8> {
    let jpeg = jpeg_8x8();
    let mut raw = jpeg[..2].to_vec();
    raw.extend_from_slice(&[0xff, 0xfe]);
    raw.extend_from_slice(&u16::try_from(comment.len() + 2).unwrap().to_be_bytes());
    raw.extend_from_slice(comment);
    raw.extend_from_slice(&jpeg[2..]);
    raw
}

fn protected_image(test: &str, moved: bool) {
    let dir = folder(test);
    let path = dir.join("input.dat");
    let raw = annotated_jpeg(b"private-comment");
    std::fs::write(&path, &raw).unwrap();
    let mut s = cube();
    paste(&mut s, path.clone());
    let doc = s.doc.as_ref().unwrap();
    let stored = doc.images().next().unwrap();
    assert!(matches!(&stored.source, ImageSource::Held(bytes) if **bytes == jpeg_8x8()));
    assert_ne!(stored.hash, ImageHash::of(&raw));
    export(&mut s, &dir.join("model.glb"));
    assert!(s.export_error().is_none(), "{:?}", s.export_error());
    let target = if moved {
        let renamed = dir.join("moved.json");
        std::fs::copy(&path, &renamed).unwrap();
        std::fs::remove_file(path).unwrap();
        renamed
    } else {
        path
    };
    export(&mut s, &target);
    assert!(s.export_error().unwrap().contains("別の名前"));
    assert_eq!(std::fs::read(&target).unwrap(), raw);
    s.apply(Command::DismissDialog);
    s.apply(Command::SaveReview(target.clone()));
    assert!(s.dialog.as_ref().unwrap().body.contains("別の名前"));
    assert_eq!(std::fs::read(&target).unwrap(), raw);
}

#[test]
fn pasted_file_is_protected_from_export_and_review() {
    protected_image("original", false);
}

#[test]
fn renamed_pasted_file_is_still_protected() {
    protected_image("renamed", true);
}

#[test]
fn no_file_name_in_state_or_glb() {
    use app::automation::Automation;
    let dir = folder("privacy");
    let private = dir.join("private-folder");
    std::fs::create_dir_all(&private).unwrap();
    let secret = "unpublished-image-name";
    let name = format!("{secret}.jpg");
    std::fs::write(private.join(&name), annotated_jpeg(secret.as_bytes())).unwrap();
    let script = dir.join("steps.json");
    let steps = serde_json::json!({ "steps": [
        {"op":"paste_texture", "path": format!("private-folder/{name}")},
        {"op":"wait_idle"}, {"op":"export_glb", "path":"shared.glb"},
        {"op":"wait_idle"}, {"op":"dump_state", "path":"state.json"}, {"op":"quit"}
    ]});
    std::fs::write(&script, steps.to_string()).unwrap();
    let args = parse_args([
        "app",
        "--script",
        script.to_str().unwrap(),
        "--out",
        dir.to_str().unwrap(),
    ])
    .unwrap();
    let mut automation = Automation::from_launch(&args);
    let mut s = cube();
    let ctx = egui::Context::default();
    let mut sink = CountingImages::default();
    until(|| {
        s.poll_loader();
        s.poll_images(&mut sink, 32);
        s.poll_exporter();
        automation.on_logic(&ctx, &s, false);
        automation.on_ui(&ctx, &mut s, false);
        automation.forcing_quit()
    });
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["ok"], true, "{report}");
    let state_bytes = std::fs::read(dir.join("state.json")).unwrap();
    let state: serde_json::Value = serde_json::from_slice(&state_bytes).unwrap();
    let draw = &state["selection_texture"]["draws"][0];
    let hash: String = ImageHash::of(&jpeg_8x8()).0[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(draw["hash"], hash);
    assert_eq!(draw["map"], "box");
    assert_eq!(draw["size"], 1.0);
    assert_eq!(state["selection_texture"]["total"], 1);
    assert_eq!(state["textures"]["displayed"], 1);
    s.apply(Command::SaveReview(dir.join("review.json")));
    for bytes in [
        state_bytes,
        std::fs::read(dir.join("shared.glb")).unwrap(),
        std::fs::read(dir.join("review.json")).unwrap(),
    ] {
        for word in [secret, "private-folder"] {
            assert!(
                !bytes
                    .windows(word.len())
                    .any(|part| part == word.as_bytes())
            );
        }
    }
}

#[test]
fn export_reopen_matches_looks() {
    let mut s = cube();
    paste(&mut s, png("roundtrip"));
    s.apply(Command::SetTextureSize(0.25));
    s.apply(Command::SetColor([0.2, 0.5, 0.7]));
    let tex = selected_tex(&s);
    let look = looks(&s, s.selection.unwrap())[0];
    let hash = s
        .doc
        .as_ref()
        .unwrap()
        .image(tex.image.unwrap())
        .unwrap()
        .hash;
    let first = folder("roundtrip").join("first.glb");
    export(&mut s, &first);
    assert!(s.export_error().is_none(), "{:?}", s.export_error());
    let note = &s.dialog.as_ref().unwrap().body;
    assert!(note.contains("画像を変えた部品 1 個: 画像 1 枚"));
    assert!(note.contains(
        "箱形の投影の部品 1 個は、UV 付きの形状を作って足しました（頂点の色と接線は写していません）"
    ));
    assert_eq!(selected_tex(&s), tex);
    assert!(!s.doc_unsaved());
    let second = folder("roundtrip").join("second.glb");
    export(&mut s, &second);
    assert!(s.export_error().is_none(), "{:?}", s.export_error());
    let first_bytes = std::fs::read(first).unwrap();
    let second_bytes = std::fs::read(&second).unwrap();
    assert_eq!(first_bytes, second_bytes);
    s.apply(Command::OpenModel(second));
    CountingImages::default().finish(&mut s);
    s.apply(Command::Select(Some(NodeId(0))));
    let baked = selected_tex(&s);
    let read_hash = s
        .doc
        .as_ref()
        .unwrap()
        .image(baked.image.unwrap())
        .unwrap()
        .hash;
    assert!(same_look(
        Some((&tex, Some(hash))),
        Some((&baked, Some(read_hash)))
    ));
    assert_eq!(looks(&s, NodeId(0))[0].base_color, look.base_color);
    assert!(!texture_view(&s).unwrap().has_box);
}

#[test]
fn export_color_note_follows_recolored_parts() {
    let mut s = opened("color-notes", &textured_quad_glb(&Default::default()), 0);
    s.apply(Command::AddShape(ShapeKind::Cube));
    let dir = folder("color-notes");
    export(&mut s, &dir.join("base.glb"));
    s.apply(Command::SetColor([0.2, 0.3, 0.4]));
    export(&mut s, &dir.join("cube.glb"));
    let note = &s.dialog.as_ref().unwrap().body;
    assert!(
        note.contains("色を変えた部品") && !note.contains("色は画像に掛け合わされて見えます"),
        "{note}"
    );
    s.apply(Command::Select(Some(NodeId(0))));
    s.apply(Command::SetColor([0.4, 0.3, 0.2]));
    export(&mut s, &dir.join("textured.glb"));
    assert!(
        s.dialog
            .as_ref()
            .unwrap()
            .body
            .contains("色は画像に掛け合わされて見えます")
    );
}
