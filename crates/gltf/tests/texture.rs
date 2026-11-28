use gltf::binary::Glb;
use gltf::json::Value;
use model_reviewer_core::scene::{ImageSkipReason, LoadWarning, Model, SceneRead};
use model_reviewer_core::texture::{
    IDENTITY_TRANSFORM, ImageHash, ImageId, ImageSource, Mime, Mip, Sampling, TexUse, UvMap, Wrap,
};
use model_reviewer_gltf::{LoadError, load_glb};
use model_reviewer_testdata::{rich_glb, sample_glb};

#[test]
fn untextured_file_keeps_primitive_numbers() {
    let model = load_glb(&sample_glb()).unwrap();
    let numbers: Vec<_> = model.draws.iter().map(|draw| draw.prim.0).collect();
    assert_eq!(numbers, [0, 1, 1, 1, 1, 1, 2, 3, 4, 5]);
    assert_eq!(model.primitives.len(), 6);
    assert!(model.draws.iter().all(|draw| draw.tex.is_none()));
    assert!(model.textures.prim_uvs.is_empty());
    assert!(model.textures.images.is_empty());
    assert!(model.textures.uses.is_empty());
}

fn json(text: &str) -> Value {
    gltf::json::deserialize::from_str(text).unwrap()
}

fn edited(edit: impl FnOnce(&mut Value, &mut Vec<u8>)) -> Vec<u8> {
    let base = rich_glb();
    let mut glb = Glb::from_slice(&base).unwrap();
    let mut root: Value = gltf::json::deserialize::from_slice(&glb.json).unwrap();
    let bin = glb.bin.as_mut().unwrap().to_mut();
    bin.truncate(root["buffers"][0]["byteLength"].as_u64().unwrap() as usize);
    edit(&mut root, bin);
    root["buffers"][0]["byteLength"] = bin.len().into();
    glb.json = gltf::json::serialize::to_vec(&root).unwrap().into();
    glb.to_vec().unwrap()
}

fn model_with(edit: impl FnOnce(&mut Value)) -> Model {
    load_glb(&edited(|root, _| edit(root))).unwrap()
}

fn first_tex(model: &Model) -> &TexUse {
    model.tex_use(model.draws[0].tex.unwrap()).unwrap()
}

fn color_info(root: &mut Value) -> &mut Value {
    &mut root["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]
}

fn omit_base_image(root: &mut Value) {
    root["materials"][0]["pbrMetallicRoughness"]
        .as_object_mut()
        .unwrap()
        .remove("baseColorTexture");
}

fn assert_skipped(model: &Model, reason: ImageSkipReason) {
    assert_eq!(model.stats.placed_triangles, 7);
    assert!(model.textures.images.is_empty());
    assert!(model.textures.prim_uvs.is_empty());
    assert!(model.draws.iter().all(|draw| draw.tex.is_none()));
    assert!(
        model
            .warnings
            .contains(&LoadWarning::ImagesSkipped { count: 1, reason })
    );
    assert!(
        !model
            .warnings
            .iter()
            .any(|w| matches!(w, LoadWarning::Textures { .. }))
    );
}

fn near(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 1e-6 + expected.abs() * 1e-5,
        "{actual} != {expected}"
    );
}

#[test]
fn sourceless_texture_opens_with_warning() {
    for extension in ["KHR_texture_basisu", "EXT_texture_webp"] {
        for required in [false, true] {
            let model = model_with(|root| {
                root["textures"][0] = json("{}");
                root["textures"][0]["extensions"][extension] = json(r#"{"source":0}"#);
                root["extensionsUsed"] = vec![extension].into();
                if required {
                    root["extensionsRequired"] = vec![extension].into();
                }
            });
            assert_skipped(&model, ImageSkipReason::Format);
        }
    }
}

#[test]
fn out_of_range_image_source_is_rejected() {
    for index in [1, u64::from(u32::MAX), 1 << 32] {
        let bytes = edited(|root, _| {
            let mut unused = json("{}");
            unused["source"] = index.into();
            root["textures"].as_array_mut().unwrap().push(unused);
        });
        let Err(LoadError::Invalid(reason)) = load_glb(&bytes) else {
            panic!("範囲外の画像参照 {index}");
        };
        assert!(reason.contains("textures[1].source"));
        assert!(reason.contains(if index == 1 {
            "IndexOutOfBounds"
        } else {
            "範囲外"
        }));
    }
    let bytes = edited(|root, bin| {
        root["bufferViews"][13]["byteOffset"] = bin.len().into();
    });
    assert!(matches!(load_glb(&bytes), Err(LoadError::BadData { what }) if what == "範囲"));
}

#[test]
fn base_color_texture_becomes_tex_use() {
    let model = load_glb(&rich_glb()).unwrap();
    assert_eq!(model.draws[0].base_color, [0.8, 0.7, 0.6, 1.0]);
    assert_eq!(model.draws[0].tex, model.draws.last().unwrap().tex);
    assert_eq!(model.textures.uses.len(), 1);
    assert_eq!(model.textures.images.len(), 1);
    assert!(model.warnings.contains(&LoadWarning::Textures { count: 1 }));
    assert!(
        !model
            .warnings
            .iter()
            .any(|w| matches!(w, LoadWarning::ImagesSkipped { .. }))
    );
    assert_eq!(first_tex(&model).image, Some(ImageId(0)));
    assert_eq!(model.textures.images[0].mime, Mime::Png);
    assert_eq!(
        first_tex(&model).map,
        UvMap::Vertex {
            transform: IDENTITY_TRANSFORM,
            tex_coord: 0,
            info_from: Some(0),
        }
    );
    assert_eq!(
        model.prim_uvs(model.draws[0].prim),
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
    );
    assert_eq!(model.textures.prim_uvs.len(), model.primitives.len());
    assert!(model.prim_uvs(model.draws[1].prim).is_empty());

    // 同じ画像・UV でも、書き出しで写す textureInfo は材質ごとに区別する。
    let shared = model_with(|root| {
        let texture = color_info(root).clone();
        root["materials"][1]["pbrMetallicRoughness"]["baseColorTexture"] = texture;
        let primitives = root["meshes"][0]["primitives"].as_array_mut().unwrap();
        let mut second = primitives[0].clone();
        second["material"] = 1.into();
        primitives.push(second);
    });
    assert_eq!(shared.textures.images.len(), 1);
    assert_eq!(shared.textures.uses.len(), 2);
    assert_eq!(shared.draws[0].prim, shared.draws[1].prim);
    let second = shared.tex_use(shared.draws[1].tex.unwrap()).unwrap();
    assert_eq!(second.image, first_tex(&shared).image);
    assert!(matches!(
        second.map,
        UvMap::Vertex {
            info_from: Some(1),
            ..
        }
    ));

    // 点と、三角形を1枚も作らないストリップは画像表にも入らない。
    for mode in [0, 5] {
        let model = model_with(|root| {
            root["meshes"][0]["primitives"][0]["mode"] = mode.into();
            root["accessors"][3]["count"] = 2.into();
        });
        assert!(model.textures.images.is_empty());
        assert!(model.textures.uses.is_empty());
        assert!(model.textures.prim_uvs.is_empty());
    }
}

#[test]
fn texcoord_1_is_read() {
    let model = model_with(|root| {
        *color_info(root) = json(r#"{"index":0,"texCoord":1}"#);
        let attrs = root["meshes"][0]["primitives"][0]["attributes"]
            .as_object_mut()
            .unwrap();
        let uv = attrs.remove("TEXCOORD_0").unwrap();
        attrs.insert("TEXCOORD_1".into(), uv);
    });
    assert_eq!(
        first_tex(&model).map,
        UvMap::Vertex {
            transform: IDENTITY_TRANSFORM,
            tex_coord: 1,
            info_from: Some(0),
        }
    );
    assert_eq!(model.prim_uvs(model.draws[0].prim)[2], [1.0, 1.0]);
}

#[test]
fn normalized_u8_and_u16_uvs() {
    for (component, maximum) in [(5121, 255u16), (5123, 65535)] {
        let samples = [
            [0, maximum],
            [maximum / 2, 1],
            [maximum, 0],
            [2, maximum - 1],
        ];
        let bytes = edited(|root, bin| {
            bin.resize(bin.len().next_multiple_of(4), 0);
            let start = bin.len();
            bin.extend_from_slice(&[0xcd; 4]);
            for uv in samples {
                let row = bin.len();
                for channel in uv {
                    if component == 5121 {
                        bin.push(channel as u8);
                    } else {
                        bin.extend_from_slice(&channel.to_le_bytes());
                    }
                }
                bin.resize(row + 8, 0xcd);
            }
            let view = &mut root["bufferViews"][2];
            view["byteOffset"] = start.into();
            view["byteLength"] = (bin.len() - start).into();
            view["byteStride"] = 8.into();
            let accessor = &mut root["accessors"][2];
            accessor["byteOffset"] = 4.into();
            accessor["normalized"] = true.into();
            accessor["componentType"] = component.into();
        });
        let model = load_glb(&bytes).unwrap();
        let uvs = model.prim_uvs(model.draws[0].prim);
        assert_eq!(uvs.len(), samples.len());
        for (&[u, v], [x, y]) in uvs.iter().zip(samples) {
            near(u, f32::from(x) / f32::from(maximum));
            near(v, f32::from(y) / f32::from(maximum));
        }
    }
}

#[test]
fn sampler_defaults_and_values() {
    for no_sampler in [false, true] {
        let model = model_with(|root| {
            root["samplers"][0] = json("{}");
            if no_sampler {
                root["textures"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("sampler");
            }
        });
        assert_eq!(first_tex(&model).sampling, Sampling::default());
    }
    for (filter, min_linear, mip) in [
        (9728, false, Mip::None),
        (9729, true, Mip::None),
        (9984, false, Mip::Nearest),
        (9985, true, Mip::Nearest),
        (9986, false, Mip::Linear),
        (9987, true, Mip::Linear),
    ] {
        let model = model_with(|root| {
            root["samplers"][0] = json(r#"{"wrapS":33648,"wrapT":33071,"magFilter":9728}"#);
            root["samplers"][0]["minFilter"] = filter.into();
        });
        assert_eq!(
            first_tex(&model).sampling,
            Sampling {
                wrap: [Wrap::Mirror, Wrap::Clamp],
                mag_linear: false,
                min_linear,
                mip,
            }
        );
    }
}

#[test]
fn texture_transform_applies_scale_rotation_offset() {
    let model = model_with(|root| {
        root["extensionsRequired"] = vec!["KHR_texture_transform"].into();
        root["meshes"][0]["primitives"][0]["attributes"]["TEXCOORD_1"] = 2.into();
        let info = color_info(root);
        info["texCoord"] = 3.into();
        let transform = &mut info["extensions"]["KHR_texture_transform"];
        *transform = json(r#"{"offset":[0.5,0],"scale":[2,1],"texCoord":1}"#);
        transform["rotation"] = std::f64::consts::FRAC_PI_2.into();
    });
    let UvMap::Vertex {
        transform: [a, b, x, c, d, y],
        tex_coord: 1,
        info_from: Some(0),
    } = first_tex(&model).map
    else {
        panic!("変換後の UV の番号");
    };
    // T・R・S を (1,0)、(0,1) に掛ける。2本の軸で回転の符号と拡大の順を確かめる。
    for (actual, expected) in [(a + x, 0.5), (c + y, 2.0), (b + x, -0.5), (d + y, 0.0)] {
        near(actual, expected);
    }
    assert!(!model.warnings.iter().any(|w| matches!(w, LoadWarning::SimplifiedExtension { name } if name == "KHR_texture_transform")));
}

#[test]
fn mask_without_texture_keeps_cutoff() {
    for cutoff in [None, Some(0.25f32)] {
        let model = model_with(|root| {
            omit_base_image(root);
            for material in root["materials"].as_array_mut().unwrap() {
                material["alphaMode"] = "MASK".into();
                if let Some(value) = cutoff {
                    material["alphaCutoff"] = value.into();
                }
            }
        });
        assert_eq!(first_tex(&model).cutoff, Some(cutoff.unwrap_or(0.5)));
        assert_eq!(first_tex(&model).image, None);
        assert_eq!(
            first_tex(&model).map,
            UvMap::Vertex {
                transform: IDENTITY_TRANSFORM,
                tex_coord: 0,
                info_from: None,
            }
        );
        assert_eq!(model.textures.uses.len(), 1);
        assert!(model.textures.images.is_empty());
        assert!(model.textures.prim_uvs.is_empty());
    }
}

#[test]
fn box_mark_is_read_from_extras() {
    for (mark, expected) in [
        ("2.5", Some(2.5)),
        ("0", None),
        ("-1", None),
        ("1e200", None),
        (r#""2""#, None),
    ] {
        let model = model_with(|root| {
            root["materials"][0]["extras"]["model_reviewer"]["box_size"] = json(mark);
        });
        assert_eq!(first_tex(&model).baked_box, expected);
    }
}

#[test]
fn skinned_and_morphed_nodes_are_listed() {
    let model = model_with(|root| {
        let primitives = root["meshes"][1]["primitives"].as_array_mut().unwrap();
        let mut morphed = primitives[0].clone();
        morphed["targets"] = json(r#"[{"POSITION":4}]"#);
        primitives.push(morphed);
    });
    // 4 はスキン、2・3・別のシーンの 7 はモーフを含むメッシュを共有する。
    assert_eq!(model.textures.no_box_nodes, [2, 3, 4, 7]);
}

#[test]
fn mask_sets_cutoff_blend_does_not() {
    for (mode, cutoff) in [("MASK", Some(0.375)), ("BLEND", None), ("OPAQUE", None)] {
        let model = model_with(|root| {
            root["materials"][0]["alphaMode"] = mode.into();
            root["materials"][0]["alphaCutoff"] = 0.375.into();
        });
        assert_eq!(first_tex(&model).image, Some(ImageId(0)));
        assert_eq!(first_tex(&model).cutoff, cutoff);
        assert_eq!(
            model
                .warnings
                .contains(&LoadWarning::AlphaBlend { count: 1 }),
            mode == "BLEND"
        );
    }
}

#[test]
fn other_textures_are_counted_not_read() {
    let model = model_with(|root| {
        omit_base_image(root);
        let info = json(r#"{"index":0}"#);
        root["materials"][0]["normalTexture"] = info.clone();
        root["materials"][0]["emissiveTexture"] = info.clone();
        root["materials"][1]["occlusionTexture"] = info.clone();
        root["materials"][1]["pbrMetallicRoughness"]["metallicRoughnessTexture"] = info;
    });
    assert!(
        model
            .warnings
            .contains(&LoadWarning::OtherTextures { count: 4 })
    );
    assert!(!model.warnings.iter().any(|w| matches!(
        w,
        LoadWarning::Textures { .. } | LoadWarning::ImagesSkipped { .. }
    )));
    assert!(model.textures.images.is_empty());
    assert!(model.textures.uses.is_empty());
    assert!(model.textures.prim_uvs.is_empty());
}

#[test]
fn external_and_unknown_images_are_skipped() {
    for uri in ["relative.png", "data:image/png;base64,AA=="] {
        let model = model_with(|root| {
            root["images"][0] = json("{}");
            root["images"][0]["uri"] = uri.into();
        });
        assert_skipped(&model, ImageSkipReason::External);
    }
    for mime in ["image/webp", "image/png"] {
        let bytes = edited(|root, bin| {
            root["images"][0]["mimeType"] = mime.into();
            let offset = root["bufferViews"][13]["byteOffset"].as_u64().unwrap() as usize;
            bin[offset..offset + 4].copy_from_slice(b"RIFF");
        });
        assert_skipped(&load_glb(&bytes).unwrap(), ImageSkipReason::Format);
    }
}

#[test]
fn missing_uv_drops_texture_only() {
    let original = load_glb(&rich_glb()).unwrap();
    for issue in ["missing", "short", "nan", "inf", "unnormalized"] {
        let bytes = edited(|root, bin| match issue {
            "missing" => {
                color_info(root)["texCoord"] = 7.into();
            }
            "short" => {
                root["accessors"][2]["count"] = 3.into();
            }
            "unnormalized" => {
                root["accessors"][2]["componentType"] = 5123.into();
            }
            _ => {
                let end = root["bufferViews"][2]["byteOffset"].as_u64().unwrap() as usize + 32;
                let value = if issue == "nan" {
                    f32::NAN
                } else {
                    f32::INFINITY
                };
                bin[end - 4..end].copy_from_slice(&value.to_le_bytes());
            }
        });
        let model = load_glb(&bytes).unwrap();
        assert_skipped(&model, ImageSkipReason::NoUv);
        assert_eq!(model.primitives, original.primitives);
        assert_eq!(model.draws[0].base_color, original.draws[0].base_color);
    }
    let mask = model_with(|root| {
        root["materials"][0]["alphaMode"] = "MASK".into();
        color_info(root)["texCoord"] = 7.into();
    });
    assert_eq!(first_tex(&mask).cutoff, Some(0.5));
    assert!(first_tex(&mask).image.is_none());
    assert!(mask.textures.prim_uvs.is_empty());
    assert!(mask.warnings.contains(&LoadWarning::ImagesSkipped {
        count: 1,
        reason: ImageSkipReason::NoUv
    }));
}

#[test]
fn share_key_separates_different_uvs() {
    for textured in [true, false] {
        let model = model_with(|root| {
            if !textured {
                omit_base_image(root);
            }
            // 法線の x,y は全頂点で 0。元と異なる正しい UV アクセサを同じ位置に使う。
            root["bufferViews"][1]["byteStride"] = 12.into();
            let mut other_uv = root["accessors"][2].clone();
            other_uv["bufferView"] = 1.into();
            let accessors = root["accessors"].as_array_mut().unwrap();
            let new_index = accessors.len();
            accessors.push(other_uv);
            let primitives = root["meshes"][0]["primitives"].as_array_mut().unwrap();
            let first = primitives[0].clone();
            let mut second = first.clone();
            second["attributes"]["TEXCOORD_0"] = new_index.into();
            let mut plain = first.clone();
            plain["material"] = 1.into();
            let mut invalid_uv = first.clone();
            invalid_uv["attributes"]["TEXCOORD_0"] = 4.into();
            primitives.extend([second, plain, invalid_uv, first]);
        });
        let draws = &model.draws[..5];
        let ids: Vec<_> = draws.iter().map(|d| d.prim.0).collect();
        if textured {
            assert_eq!(ids, [0, 1, 2, 2, 0]);
            assert_eq!(draws[0].tex, draws[1].tex);
            assert!(draws[2..4].iter().all(|d| d.tex.is_none()));
            assert_eq!(model.prim_uvs(draws[0].prim)[2], [1.0, 1.0]);
            assert_eq!(model.prim_uvs(draws[1].prim), [[0.0, 0.0]; 4]);
            assert!(model.prim_uvs(draws[2].prim).is_empty());
        } else {
            assert_eq!(ids, [0; 5]);
            assert!(model.textures.prim_uvs.is_empty());
        }
    }
}

#[test]
fn image_hash_covers_exact_byte_range() {
    let image = [0xff, 0xd8, 0xff, 0xe0, 0, 4, 1, 2, 0xff, 0xd9];
    let mut start = 0;
    let bytes = edited(|root, bin| {
        bin.resize(bin.len().next_multiple_of(4) + 4, 0xad);
        start = bin.len();
        bin.extend_from_slice(&image);
        bin.extend_from_slice(&[0xbe; 3]);
        root["bufferViews"][13]["byteOffset"] = start.into();
        root["bufferViews"][13]["byteLength"] = image.len().into();
        root["images"][0]["mimeType"] = "image/png".into();
    });
    let model = load_glb(&bytes).unwrap();
    let info = model.image(first_tex(&model).image.unwrap()).unwrap();
    assert_eq!(info.mime, Mime::Jpeg);
    assert_eq!(info.hash, ImageHash::of(&image));
    assert_eq!(
        info.source,
        ImageSource::File {
            offset: start as u64,
            len: image.len() as u64
        }
    );
    let glb = Glb::from_slice(&bytes).unwrap();
    assert_eq!(
        &glb.bin.as_ref().unwrap()[start..start + image.len()],
        image
    );
}
