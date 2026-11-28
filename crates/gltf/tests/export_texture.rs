use std::sync::Arc;

use gltf::binary::Glb;
use gltf::json::Value;
use model_reviewer_core::doc::{Document, NodeKey};
use model_reviewer_core::history::{Edit, History, Step};
use model_reviewer_core::review::ReviewState;
use model_reviewer_core::scene::{Draw, NodeId, PrimId, SceneRead};
use model_reviewer_core::shapes::{self, ShapeKind};
use model_reviewer_core::texture::{
    IDENTITY_TRANSFORM, ImageHash, ImageId, Sampling, TexId, TexUse, UvMap,
};
use model_reviewer_gltf::export::{
    Built, ExportError, ExportImage, ExportNode, ExportNote, ExportScene, ExportTextures, build,
    empty_base, verify, write_container,
};
use model_reviewer_gltf::load_glb;
use model_reviewer_testdata::{
    Pattern, TextureTransform, TexturedOptions, inputs, jpeg_8x8, pattern_png, rich, rich_glb,
    textured_quad_glb, textured_scene_glb,
};

#[test]
fn exporting_twice_adds_nothing() {
    let base = textured_scene_glb(0, 8, 1);
    let mut scene = scene_of(&base);
    let image = add_image(&mut scene, pattern_png(Pattern::Noise, 5, 7));
    set_texture(&mut scene, 0, box_texture(image, 0.5));
    let (first, written) = export(&base, &scene);
    assert!(!first.appended_bin.is_empty());
    let material = material_of(&root(&written), 0).clone();
    assert_eq!(material["extras"]["model_reviewer"]["box_size"], 0.5);
    let (second, again) = export(&written, &scene);
    assert!(second.appended_bin.is_empty());
    assert!(second.notes.is_empty());
    assert_eq!(again, written);
}

#[test]
fn remove_export_undo_export_restores_original() {
    // 同じバイト列が元から2番目にもある。取り消しは最初に見つかる画像ではなく元の番号へ戻す。
    let base = edit_glb(&uv_quad(), |v| {
        let twin = v["images"][0].clone();
        v["images"].as_array_mut().unwrap().push(twin);
        v["textures"]
            .as_array_mut()
            .unwrap()
            .push(json(r#"{"source":1}"#));
        v["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["index"] = 1.into();
    });
    let mut doc = Document::new(Arc::new(load_glb(&base).unwrap()));
    let key = NodeKey(0);
    let mut review = ReviewState::new(&doc);
    let mut history = History::default();
    let before = doc.subtree_looks(key).unwrap();
    let cutoff = doc.tex_use(before[0].tex.unwrap()).unwrap().cutoff;
    let mask = doc
        .add_tex_use(TexUse {
            image: None,
            map: UvMap::Vertex {
                transform: IDENTITY_TRANSFORM,
                tex_coord: 0,
                info_from: None,
            },
            sampling: Sampling::default(),
            cutoff,
            baked_box: None,
        })
        .unwrap();
    let mut after = before.clone();
    after[0].tex = Some(mask);
    history
        .execute(
            Step::new(
                "画像を外す",
                vec![Edit::SetLooks { key, before, after }],
                Some(key),
            ),
            &mut doc,
            &mut review,
        )
        .unwrap();
    let (removed, written) = export(&base, &snapshot(&doc));
    assert!(
        material_of(&root(&written), 0)["pbrMetallicRoughness"]
            .get("baseColorTexture")
            .is_none()
    );
    doc.set_file_indices(&removed.assigned);
    history.undo(&mut doc, &mut review).unwrap();
    let (restored, bytes) = export(&written, &snapshot(&doc));
    assert!(restored.appended_bin.is_empty());
    let old = root(&base);
    let new = root(&bytes);
    assert_eq!(new["images"], old["images"]);
    assert_eq!(new["textures"], old["textures"]);
    let pbr = "pbrMetallicRoughness";
    let info = &material_of(&new, 0)[pbr]["baseColorTexture"];
    assert_eq!(info, &material_of(&old, 0)[pbr]["baseColorTexture"]);
    assert_eq!(info["texCoord"], 1);
    assert!(info["extensions"]["KHR_texture_transform"].is_object());
    assert_eq!(material_of(&new, 0)["alphaCutoff"], 0.375);
}

fn corner_uvs(bytes: &[u8], set: u32) -> Vec<[f32; 2]> {
    let file = gltf::Gltf::from_slice(bytes).unwrap();
    let mesh = file.nodes().next().unwrap().mesh().unwrap();
    let prim = mesh.primitives().next().unwrap();
    let reader = prim.reader(|_| file.blob.as_deref());
    let uvs: Vec<_> = reader.read_tex_coords(set).unwrap().into_f32().collect();
    reader
        .read_indices()
        .unwrap()
        .into_u32()
        .map(|i| uvs[i as usize])
        .collect()
}

#[test]
fn box_preserves_other_uv_sets() {
    let base = edit_glb(
        &textured_quad_glb(&TexturedOptions {
            tex_coord: 1,
            uv_format: model_reviewer_testdata::UvFormat::U16,
            ..Default::default()
        }),
        |root| {
            root["materials"][0]["normalTexture"] = json(r#"{"index":0,"texCoord":1}"#);
            root["materials"][0]["pbrMetallicRoughness"]
                .as_object_mut()
                .unwrap()
                .remove("baseColorTexture");
        },
    );
    // 折れた板の共有頂点は焼き込みで分かれる。両方が同じ元の UV を受け取る。
    let mut folded = Glb::from_slice(&base).unwrap();
    let value = root(&base);
    let a = primitive_of(&value, 0)["attributes"]["POSITION"]
        .as_u64()
        .unwrap() as usize;
    let view = value["accessors"][a]["bufferView"].as_u64().unwrap() as usize;
    let offset = value["bufferViews"][view]["byteOffset"]
        .as_u64()
        .unwrap_or(0) as usize
        + 3 * 12
        + 8;
    folded.bin.as_mut().unwrap().to_mut()[offset..offset + 4]
        .copy_from_slice(&4.0f32.to_le_bytes());
    let base = folded.to_vec().unwrap();
    let mut scene = scene_of(&base);
    let image = add_image(&mut scene, jpeg_8x8());
    set_texture(&mut scene, 0, box_texture(image, 2.0));
    let (built, written) = export(&base, &scene);
    let v = root(&written);
    let info = &material_of(&v, 0)["pbrMetallicRoughness"]["baseColorTexture"];
    assert_eq!(info["texCoord"], 2);
    for set in 0..2 {
        assert_eq!(corner_uvs(&written, set), corner_uvs(&base, set));
    }
    assert_eq!(
        material_of(&v, 0)["normalTexture"],
        root(&base)["materials"][0]["normalTexture"]
    );
    let mut damaged = built.clone();
    let uv = primitive_of(&v, 0)["attributes"]["TEXCOORD_1"]
        .as_u64()
        .unwrap() as usize;
    let view = v["accessors"][uv]["bufferView"].as_u64().unwrap() as usize;
    let at = v["bufferViews"][view]["byteOffset"].as_u64().unwrap() as usize
        - Glb::from_slice(&base).unwrap().bin.unwrap().len();
    damaged.appended_bin[at..at + 4].copy_from_slice(&0.25f32.to_le_bytes());
    rejects(&base, &scene, &damaged);
    assert_eq!(export(&written, &scene).1, written);
}

fn undo_box_paste(base: &[u8], remove_first: bool) -> Vec<u8> {
    let mut doc = Document::new(Arc::new(load_glb(base).unwrap()));
    let key = NodeKey(0);
    let mut history = History::default();
    let mut review = ReviewState::new(&doc);
    let image = doc
        .try_add_image(jpeg_8x8().into(), model_reviewer_core::texture::Mime::Jpeg)
        .unwrap();
    let pasted = doc.add_tex_use(box_texture(image, 0.75)).unwrap();
    for tex in [None, Some(pasted)]
        .into_iter()
        .skip(usize::from(!remove_first))
    {
        let before = doc.subtree_looks(key).unwrap();
        let mut after = before.clone();
        after[0].tex = tex;
        history
            .execute(
                Step::new(
                    "画像の変更",
                    vec![Edit::SetLooks { key, before, after }],
                    Some(key),
                ),
                &mut doc,
                &mut review,
            )
            .unwrap();
    }
    let (built, written) = export(base, &snapshot(&doc));
    doc.set_file_indices(&built.assigned);
    history.undo(&mut doc, &mut review).unwrap();
    if remove_first {
        history.undo(&mut doc, &mut review).unwrap();
    }
    let (restored, bytes) = export(&written, &snapshot(&doc));
    assert!(restored.appended_bin.is_empty());
    bytes
}

/// 箱形の大きさ（または画像）を変えながら毎回書き出し、全部を取り消して書き出す。
/// 書き出しごとの箱形の組の番号と、最後に書いたバイト列を返す。
fn repaste_and_undo(base: &[u8], pastes: &[(f32, bool)]) -> (Vec<u64>, Vec<u8>) {
    let mut doc = Document::new(Arc::new(load_glb(base).unwrap()));
    let key = NodeKey(0);
    let mut history = History::default();
    let mut review = ReviewState::new(&doc);
    let jpeg = doc
        .try_add_image(jpeg_8x8().into(), model_reviewer_core::texture::Mime::Jpeg)
        .unwrap();
    let png = doc
        .try_add_image(
            pattern_png(Pattern::Checker, 4, 1).into(),
            model_reviewer_core::texture::Mime::Png,
        )
        .unwrap();
    let mut current = base.to_vec();
    let mut sets = Vec::new();
    for &(size, other) in pastes {
        let image = if other { png } else { jpeg };
        let pasted = doc.add_tex_use(box_texture(image, size)).unwrap();
        let before = doc.subtree_looks(key).unwrap();
        let mut after = before.clone();
        after[0].tex = Some(pasted);
        history
            .execute(
                Step::new(
                    "貼る",
                    vec![Edit::SetLooks { key, before, after }],
                    Some(key),
                ),
                &mut doc,
                &mut review,
            )
            .unwrap();
        let (built, written) = export(&current, &snapshot(&doc));
        doc.set_file_indices(&built.assigned);
        let v = root(&written);
        sets.push(
            material_of(&v, 0)["pbrMetallicRoughness"]["baseColorTexture"]["texCoord"]
                .as_u64()
                .unwrap_or(0),
        );
        current = written;
    }
    for _ in pastes {
        history.undo(&mut doc, &mut review).unwrap();
    }
    let (restored, bytes) = export(&current, &snapshot(&doc));
    assert!(restored.appended_bin.is_empty());
    (sets, bytes)
}

#[test]
fn repeated_box_exports_then_undo_restore_opened_geometry() {
    let base = textured_quad_glb(&Default::default());
    for pastes in [
        &[(0.75, false), (0.5, false)][..],
        &[(0.75, false), (0.75, true)][..],
        &[(0.75, false), (0.5, false), (0.25, true)][..],
    ] {
        let (sets, restored) = repaste_and_undo(&base, pastes);
        // 開いた時点の組は TEXCOORD_0 だけ。箱形の組は何回書き出しても次の番号（1）。
        assert!(sets.iter().all(|&set| set == 1), "{sets:?}");
        let new = root(&restored);
        let old = root(&base);
        assert_eq!(
            primitive_of(&new, 0)["attributes"],
            primitive_of(&old, 0)["attributes"]
        );
        assert_eq!(corner_uvs(&restored, 0), corner_uvs(&base, 0));
        assert_eq!(material_of(&new, 0), material_of(&old, 0));
    }
}

#[test]
fn repeated_box_exports_do_not_carry_old_box_sets() {
    let base = textured_quad_glb(&TexturedOptions {
        tex_coord: 1,
        ..Default::default()
    });
    let (sets, _) = repaste_and_undo(&base, &[(0.75, false), (0.5, false), (0.25, true)]);
    assert_eq!(sets, [2, 2, 2]);
}

/// 使っていない TEXCOORD_1 を、頂点の数と合わない accessor にする。
fn broken_unused_set() -> Vec<u8> {
    edit_glb(&textured_quad_glb(&Default::default()), |v| {
        let uv = primitive_of(v, 0)["attributes"]["TEXCOORD_0"]
            .as_u64()
            .unwrap() as usize;
        let mut short = v["accessors"][uv].clone();
        short["count"] = (short["count"].as_u64().unwrap() - 1).into();
        let accessors = v["accessors"].as_array_mut().unwrap();
        accessors.push(short);
        let broken = accessors.len() - 1;
        let mesh = v["nodes"][0]["mesh"].as_u64().unwrap() as usize;
        v["meshes"][mesh]["primitives"][0]["attributes"]["TEXCOORD_1"] = broken.into();
    })
}

#[test]
fn unreadable_unused_uv_set_is_dropped_when_baking() {
    let base = broken_unused_set();
    let mut scene = scene_of(&base);
    let image = add_image(&mut scene, jpeg_8x8());
    set_texture(&mut scene, 0, box_texture(image, 0.5));
    let (built, written) = export(&base, &scene);
    assert!(
        built.notes.contains(&ExportNote::UvSetsDropped { sets: 1 }),
        "{:?}",
        built.notes
    );
    let v = root(&written);
    let attributes = primitive_of(&v, 0)["attributes"].as_object().unwrap();
    let mut texcoords: Vec<_> = attributes
        .keys()
        .filter(|k| k.starts_with("TEXCOORD_"))
        .cloned()
        .collect();
    texcoords.sort();
    assert_eq!(texcoords, ["TEXCOORD_0", "TEXCOORD_1"]);
    assert_eq!(corner_uvs(&written, 0), corner_uvs(&base, 0));
}

#[test]
fn unreadable_unused_uv_set_survives_undo() {
    let base = broken_unused_set();
    let restored = undo_box_paste(&base, false);
    assert_eq!(
        primitive_of(&root(&restored), 0)["attributes"],
        primitive_of(&root(&base), 0)["attributes"]
    );
    let (_, restored) = repaste_and_undo(&base, &[(0.75, false), (0.5, true)]);
    assert_eq!(
        primitive_of(&root(&restored), 0)["attributes"],
        primitive_of(&root(&base), 0)["attributes"]
    );
}

#[test]
fn unreadable_base_color_set_still_refuses_box() {
    let base = edit_glb(&broken_unused_set(), |v| {
        v["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["texCoord"] = 1.into();
    });
    let mut scene = scene_of(&base);
    let image = add_image(&mut scene, jpeg_8x8());
    set_texture(&mut scene, 0, box_texture(image, 0.5));
    assert!(matches!(
        build(&base, &scene),
        Err(ExportError::InvalidScene(_))
    ));
}

#[test]
fn undo_restores_uv0_geometry() {
    let base = textured_quad_glb(&Default::default());
    let restored = undo_box_paste(&base, true);
    assert_eq!(corner_uvs(&restored, 0), corner_uvs(&base, 0));
    assert_eq!(
        primitive_of(&root(&restored), 0)["attributes"],
        primitive_of(&root(&base), 0)["attributes"]
    );
    assert_eq!(
        material_of(&root(&restored), 0),
        material_of(&root(&base), 0)
    );
}

#[test]
fn undo_restores_skipped_reference() {
    let base = edit_glb(&textured_quad_glb(&Default::default()), |v| {
        v["images"][0] = json(r#"{"uri":"external.png"}"#);
        v["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["extras"] =
            json(r#"{"layer":7}"#);
    });
    let restored = root(&undo_box_paste(&base, false));
    assert_eq!(material_of(&restored, 0), material_of(&root(&base), 0));
    assert_eq!(source_image(&restored, 0), 0);
}

#[test]
fn undo_then_recolor_keeps_original_geometry() {
    let base = textured_quad_glb(&Default::default());
    let mut doc = Document::new(Arc::new(load_glb(&base).unwrap()));
    let key = NodeKey(0);
    let mut history = History::default();
    let mut review = ReviewState::new(&doc);
    let image = doc
        .try_add_image(jpeg_8x8().into(), model_reviewer_core::texture::Mime::Jpeg)
        .unwrap();
    let pasted = doc.add_tex_use(box_texture(image, 0.75)).unwrap();
    let before = doc.subtree_looks(key).unwrap();
    let mut after = before.clone();
    after[0].tex = Some(pasted);
    history
        .execute(
            Step::new(
                "貼る",
                vec![Edit::SetLooks { key, before, after }],
                Some(key),
            ),
            &mut doc,
            &mut review,
        )
        .unwrap();
    let (built, written) = export(&base, &snapshot(&doc));
    doc.set_file_indices(&built.assigned);
    history.undo(&mut doc, &mut review).unwrap();
    let before = doc.subtree_looks(key).unwrap();
    let mut after = before.clone();
    after[0].base_color[0] = 0.5;
    history
        .execute(
            Step::new("色", vec![Edit::SetLooks { key, before, after }], Some(key)),
            &mut doc,
            &mut review,
        )
        .unwrap();
    let (restored, bytes) = export(&written, &snapshot(&doc));
    assert!(restored.appended_bin.is_empty());
    let old = root(&base);
    let new = root(&bytes);
    assert_eq!(
        primitive_of(&new, 0)["attributes"],
        primitive_of(&old, 0)["attributes"]
    );
    assert_eq!(corner_uvs(&bytes, 0), corner_uvs(&base, 0));
    let pbr = "pbrMetallicRoughness";
    assert_eq!(
        material_of(&new, 0)[pbr]["baseColorTexture"],
        material_of(&old, 0)[pbr]["baseColorTexture"]
    );
    assert_eq!(material_of(&new, 0)[pbr]["baseColorFactor"][0], 0.5);
    assert!(material_of(&new, 0).get("extras").is_none());
}

fn uv_quad() -> Vec<u8> {
    textured_quad_glb(&TexturedOptions {
        tex_coord: 1,
        transform: Some(TextureTransform {
            offset: [0.25, -0.5],
            rotation: 0.25,
            scale: [2.0, 0.5],
            tex_coord: None,
        }),
        mask_cutoff: Some(0.375),
        ..Default::default()
    })
}

fn snapshot(doc: &Document) -> ExportScene {
    ExportScene {
        nodes: doc
            .nodes()
            .iter()
            .enumerate()
            .map(|(index, node)| {
                let id = NodeId(index as u32);
                ExportNode {
                    parent: node.parent.map(|id| id.0),
                    name: node.name.to_string(),
                    local: node.local,
                    file_index: doc.file_index(id),
                    template: node.template,
                    draws: doc.draws(id).to_vec(),
                }
            })
            .collect(),
        added: doc.added_primitives().to_vec(),
        base_primitive_count: doc.base().primitives.len() as u32,
        textures: ExportTextures::from_tables(doc.images(), doc.tex_uses()),
        originals: model_reviewer_gltf::export::originals(doc.base()),
    }
}

fn scene_of(base: &[u8]) -> ExportScene {
    snapshot(&Document::new(Arc::new(load_glb(base).unwrap())))
}

fn box_texture(image: ImageId, size: f32) -> TexUse {
    TexUse {
        image: Some(image),
        map: UvMap::Box { size, aspect: 1.0 },
        sampling: Sampling::default(),
        cutoff: None,
        baked_box: None,
    }
}

fn add_image(scene: &mut ExportScene, bytes: Vec<u8>) -> ImageId {
    let id = ImageId(scene.textures.images.len() as u32);
    scene.textures.images.push(ExportImage {
        hash: ImageHash::of(&bytes),
        mime: model_reviewer_core::texture::sniff(&bytes).unwrap(),
        bytes: Some(bytes.into()),
    });
    id
}

fn set_texture(scene: &mut ExportScene, node: usize, texture: TexUse) {
    let id = TexId(scene.textures.uses.len() as u32);
    scene.textures.uses.push(texture);
    scene.nodes[node].draws[0].tex = Some(id);
}

fn export(base: &[u8], scene: &ExportScene) -> (Built, Vec<u8>) {
    let built = build(base, scene).unwrap();
    let mut bytes = Vec::new();
    write_container(&mut bytes, base, &built).unwrap();
    verify(&bytes, base, scene, &built).unwrap();
    (built, bytes)
}

fn root(bytes: &[u8]) -> Value {
    gltf::json::deserialize::from_slice(&Glb::from_slice(bytes).unwrap().json).unwrap()
}

fn primitive_of(root: &Value, node: usize) -> &Value {
    let mesh = root["nodes"][node]["mesh"].as_u64().unwrap() as usize;
    &root["meshes"][mesh]["primitives"][0]
}

fn material_of(root: &Value, node: usize) -> &Value {
    let material = primitive_of(root, node)["material"].as_u64().unwrap() as usize;
    &root["materials"][material]
}

fn edit_glb(base: &[u8], edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut glb = Glb::from_slice(base).unwrap();
    let mut value = root(base);
    edit(&mut value);
    glb.json = gltf::json::serialize::to_vec(&value).unwrap().into();
    glb.to_vec().unwrap()
}

fn json(text: &str) -> Value {
    gltf::json::deserialize::from_str(text).unwrap()
}

fn image_bytes(bytes: &[u8], root: &Value, index: usize) -> Vec<u8> {
    let glb = Glb::from_slice(bytes).unwrap();
    let view = &root["bufferViews"][root["images"][index]["bufferView"].as_u64().unwrap() as usize];
    let start = view["byteOffset"].as_u64().unwrap_or(0) as usize;
    let len = view["byteLength"].as_u64().unwrap() as usize;
    glb.bin.unwrap()[start..start + len].to_vec()
}

fn source_image(root: &Value, node: usize) -> usize {
    let info = &material_of(root, node)["pbrMetallicRoughness"]["baseColorTexture"];
    root["textures"][info["index"].as_u64().unwrap() as usize]["source"]
        .as_u64()
        .unwrap() as usize
}

fn duplicate(scene: &mut ExportScene, node: usize) -> usize {
    let original = scene.nodes[node].clone();
    let copy = ExportNode {
        parent: None,
        template: original.file_index,
        file_index: None,
        ..original
    };
    scene.nodes.push(copy);
    scene.nodes.len() - 1
}

fn swap_image(scene: &mut ExportScene, node: usize, image: ImageId) {
    let id = scene.nodes[node].draws[0].tex.unwrap();
    let original = scene.textures.uses[id.0 as usize];
    set_texture(
        scene,
        node,
        TexUse {
            image: Some(image),
            ..original
        },
    );
}

fn fresh_box_scene() -> ExportScene {
    let mut scene = ExportScene {
        added: vec![Arc::new(shapes::build(ShapeKind::Cube, 2.0))],
        nodes: vec![ExportNode {
            file_index: None,
            template: None,
            parent: None,
            local: glam::Affine3A::IDENTITY,
            name: "箱".into(),
            draws: vec![Draw {
                prim: PrimId(0),
                base_color: [1.0; 4],
                tex: None,
            }],
        }],
        ..Default::default()
    };
    let image = add_image(&mut scene, pattern_png(Pattern::Checker, 3, 0));
    set_texture(&mut scene, 0, box_texture(image, 0.25));
    scene
}

#[test]
fn pasted_image_is_embedded_once() {
    let base = textured_scene_glb(0, 8, 1);
    let mut scene = scene_of(&base);
    let copy = duplicate(&mut scene, 0);
    // 同じ内容に異なる文書内番号を与え、番号だけで重複を判定できない条件にする。
    let png = (2..16)
        .map(|size| pattern_png(Pattern::Noise, size, 7))
        .find(|p| p.len() % 4 != 0)
        .unwrap();
    for node in [0, copy] {
        let image = add_image(&mut scene, png.clone());
        set_texture(&mut scene, node, box_texture(image, 2.0));
    }
    let (built, bytes) = export(&base, &scene);
    let written = root(&bytes);
    assert_eq!(written["images"].as_array().unwrap().len(), 1);
    assert_eq!(image_bytes(&bytes, &written, 0), png);
    assert_eq!(written["nodes"][0]["mesh"], written["nodes"][1]["mesh"]);
    assert_eq!(
        built.notes,
        [ExportNote::Retextured {
            nodes: 2,
            images: 1,
            image_bytes: png.len() as u64,
            baked: 1
        }]
    );
    for view in written["bufferViews"].as_array().unwrap() {
        assert_eq!(view["byteOffset"].as_u64().unwrap_or(0) % 4, 0);
    }
}

#[test]
fn same_image_as_base_is_reused() {
    let base = textured_scene_glb(2, 8, 2);
    let mut scene = scene_of(&base);
    let old = root(&base);
    let same = add_image(&mut scene, image_bytes(&base, &old, 1));
    swap_image(&mut scene, 0, same);
    let (built, bytes) = export(&base, &scene);
    let written = root(&bytes);
    assert_eq!(source_image(&written, 0), 1);
    assert!(built.appended_bin.is_empty());
    for key in ["images", "textures", "samplers", "accessors", "bufferViews"] {
        assert_eq!(written.get(key), old.get(key), "{key}");
    }
}

#[test]
fn uv_part_swaps_texture_index_only() {
    let base = edit_glb(&uv_quad(), |v| {
        v["samplers"] =
            json(r#"[{"wrapS":33071,"wrapT":33648,"magFilter":9728,"minFilter":9985}]"#);
        v["textures"][0]["sampler"] = 0.into();
        v["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["extras"] =
            json(r#"{"layer":3}"#);
    });
    let mut scene = scene_of(&base);
    let image = add_image(&mut scene, pattern_png(Pattern::Noise, 3, 11));
    swap_image(&mut scene, 0, image);
    let (_, bytes) = export(&base, &scene);
    let old = root(&base);
    let new = root(&bytes);
    let mut original = material_of(&old, 0).clone();
    original["pbrMetallicRoughness"]["baseColorTexture"]["index"] = 1.into();
    assert_eq!(material_of(&new, 0), &original);
    assert_eq!(new["textures"][1], json(r#"{"sampler":0,"source":1}"#));
    assert_eq!(new["samplers"], old["samplers"]);
    assert_eq!(new["accessors"], old["accessors"]);
    let mut primitive = primitive_of(&old, 0).clone();
    primitive["material"] = 1.into();
    assert_eq!(primitive_of(&new, 0), &primitive);

    let (again, repeated) = export(&bytes, &scene);
    assert!(again.notes.is_empty());
    assert_eq!(repeated, bytes);

    let tex = scene.nodes[0].draws[0].tex.unwrap().0 as usize;
    let UvMap::Vertex {
        ref mut info_from, ..
    } = scene.textures.uses[tex].map
    else {
        unreachable!()
    };
    *info_from = Some(99);
    assert!(matches!(
        build(&base, &scene),
        Err(ExportError::InvalidScene(_))
    ));
}

#[test]
fn removed_texture_drops_the_field() {
    let base = textured_quad_glb(&TexturedOptions::default());
    let mut scene = scene_of(&base);
    scene.nodes[0].draws[0].tex = None;
    let (built, bytes) = export(&base, &scene);
    let mut expected = material_of(&root(&base), 0).clone();
    expected["pbrMetallicRoughness"]
        .as_object_mut()
        .unwrap()
        .remove("baseColorTexture");
    assert_eq!(material_of(&root(&bytes), 0), &expected);
    assert!(built.appended_bin.is_empty());
    assert_eq!(
        built.notes,
        [ExportNote::Retextured {
            nodes: 1,
            images: 0,
            image_bytes: 0,
            baked: 0
        }]
    );
}

#[test]
fn box_part_gets_baked_mesh() {
    let mut scene = fresh_box_scene();
    let mut second = scene.nodes[0].clone();
    second.draws[0].base_color[1] = 0.25;
    scene.nodes.push(second);
    let (built, bytes) = export(&empty_base(), &scene);
    let new = root(&bytes);
    for node in 0..2 {
        let p = primitive_of(&new, node);
        assert_eq!(p["mode"], 4);
        assert_eq!(p["attributes"].as_object().unwrap().len(), 3);
        for (attribute, kind) in [
            ("POSITION", "VEC3"),
            ("NORMAL", "VEC3"),
            ("TEXCOORD_0", "VEC2"),
        ] {
            let accessor = &new["accessors"][p["attributes"][attribute].as_u64().unwrap() as usize];
            assert_eq!(accessor["type"], kind);
            assert_eq!(accessor["componentType"], 5126);
        }
        assert_eq!(
            material_of(&new, node)["pbrMetallicRoughness"]["metallicFactor"],
            0.0
        );
        assert_eq!(source_image(&new, node), 0);
        assert_eq!(
            material_of(&new, node)["extras"]["model_reviewer"]["box_size"],
            0.25
        );
    }
    assert_eq!(new["accessors"].as_array().unwrap().len(), 4);
    for (node, file) in scene.nodes.iter_mut().zip(built.assigned) {
        node.file_index = Some(file);
    }
    let (again, repeated) = export(&bytes, &scene);
    assert!(again.notes.is_empty());
    assert_eq!(repeated, bytes);
}

fn rejects(base: &[u8], scene: &ExportScene, built: &Built) {
    let mut bytes = Vec::new();
    write_container(&mut bytes, base, built).unwrap();
    assert!(matches!(
        verify(&bytes, base, scene, built),
        Err(ExportError::Verify(_))
    ));
}

#[test]
fn baked_mesh_keeps_triangles() {
    for base in [
        inputs::triangle_strip(),
        inputs::triangle_fan(),
        textured_scene_glb(0, 8, 1),
    ] {
        let model = load_glb(&base).unwrap();
        let old = &model.primitives[model.draws[0].prim.0 as usize];
        let mut scene = scene_of(&base);
        let image = add_image(&mut scene, pattern_png(Pattern::Checker, 3, 0));
        set_texture(&mut scene, 0, box_texture(image, 0.5));
        let (built, bytes) = export(&base, &scene);
        let loaded = load_glb(&bytes).unwrap();
        let p = &loaded.primitives[loaded.draws[0].prim.0 as usize];
        assert_eq!(p.indices.len(), old.indices.len());
        for (&i, &j) in old.indices.iter().zip(&p.indices) {
            assert_eq!(
                old.vertices[i as usize].position.map(f32::to_bits),
                p.vertices[j as usize].position.map(f32::to_bits)
            );
        }
        let value = root(&bytes);
        let primitive = primitive_of(&value, 0);
        let start = Glb::from_slice(&base).unwrap().bin.unwrap().len();
        for accessor in [
            &primitive["attributes"]["TEXCOORD_0"],
            &primitive["indices"],
        ] {
            let a = &value["accessors"][accessor.as_u64().unwrap() as usize];
            let view = &value["bufferViews"][a["bufferView"].as_u64().unwrap() as usize];
            let offset = view["byteOffset"].as_u64().unwrap() as usize - start;
            let mut damaged = built.clone();
            if accessor == &primitive["indices"] {
                damaged
                    .appended_bin
                    .copy_within(offset + 4..offset + 8, offset);
            } else {
                damaged.appended_bin[offset..offset + 4].copy_from_slice(&123.0f32.to_le_bytes());
            }
            rejects(&base, &scene, &damaged);
        }
    }
}

#[test]
fn duplicate_with_other_image_keeps_original() {
    let base = uv_quad();
    let mut scene = scene_of(&base);
    let copy = duplicate(&mut scene, 0);
    let image = add_image(&mut scene, jpeg_8x8());
    swap_image(&mut scene, copy, image);
    let (_, bytes) = export(&base, &scene);
    let new = root(&bytes);
    assert_eq!(new["nodes"][0], root(&base)["nodes"][0]);
    assert_eq!(source_image(&new, 0), 0);
    assert_eq!(source_image(&new, 1), 1);
    assert_eq!(image_bytes(&bytes, &new, 1), jpeg_8x8());
    assert_eq!(new["images"][1]["mimeType"], "image/jpeg");
}

#[test]
fn unreferenced_base_image_is_reused() {
    let base = edit_glb(&uv_quad(), |v| {
        v["materials"][0]["pbrMetallicRoughness"]
            .as_object_mut()
            .unwrap()
            .remove("baseColorTexture");
        v.as_object_mut().unwrap().remove("textures");
    });
    let mut scene = scene_of(&base);
    assert!(scene.textures.images.is_empty());
    let image = add_image(&mut scene, image_bytes(&base, &root(&base), 0));
    let mut tex = box_texture(image, 1.0);
    tex.cutoff = Some(0.375);
    set_texture(&mut scene, 0, tex);
    let (built, bytes) = export(&base, &scene);
    let new = root(&bytes);
    assert_eq!(new["images"], root(&base)["images"]);
    assert_eq!(source_image(&new, 0), 0);
    assert_eq!(
        built.notes,
        [ExportNote::Retextured {
            nodes: 1,
            images: 0,
            image_bytes: 0,
            baked: 1
        }]
    );
}

#[test]
fn box_paste_keeps_alpha_mode_and_cutoff() {
    for (extras, markable) in [
        (json(r#"{"custom":42,"model_reviewer":{"other":1}}"#), true),
        (json("[null]"), false),
        (json(r#"{"model_reviewer":5}"#), false),
    ] {
        let base = edit_glb(&uv_quad(), |v| {
            v["materials"][0]["extras"] = extras.clone();
            v["materials"][0]["pbrMetallicRoughness"]
                .as_object_mut()
                .unwrap()
                .remove("baseColorTexture");
        });
        let mut scene = scene_of(&base);
        let image = add_image(&mut scene, pattern_png(Pattern::Quadrants, 3, 4));
        let mut tex = box_texture(image, 1.5);
        tex.cutoff = Some(0.375);
        set_texture(&mut scene, 0, tex);
        let (built, written) = export(&base, &scene);
        let new = root(&written);
        let material = material_of(&new, 0);
        assert_eq!(material["alphaMode"], "MASK");
        assert_eq!(material["alphaCutoff"], 0.375);
        assert_eq!(
            built.notes.contains(&ExportNote::BoxSizeUnmarked),
            !markable
        );
        if markable {
            assert_eq!(material["extras"]["model_reviewer"]["other"], 1);
            assert_eq!(material["extras"]["model_reviewer"]["box_size"], 1.5);
            let mut missing_mark = new.clone();
            let id = primitive_of(&new, 0)["material"].as_u64().unwrap() as usize;
            missing_mark["materials"][id]["extras"]["model_reviewer"]
                .as_object_mut()
                .unwrap()
                .remove("box_size");
            let damaged = Built {
                json: gltf::json::serialize::to_vec(&missing_mark).unwrap(),
                ..built.clone()
            };
            rejects(&base, &scene, &damaged);
        } else {
            assert_eq!(material["extras"], extras);
            let (repeat, again) = export(&written, &scene);
            assert!(repeat.notes.contains(&ExportNote::BoxSizeUnmarked));
            assert_eq!(root(&again)["images"], new["images"]);
        }
    }
}

#[test]
fn skinned_or_morphed_box_is_refused() {
    let morphed = edit_glb(&textured_quad_glb(&TexturedOptions::default()), |v| {
        v["meshes"][0]["primitives"][0]["targets"] = json(r#"[{"POSITION":0}]"#);
    });
    for (base, file) in [(rich_glb(), rich::SKINNED), (morphed, 0)] {
        let mut scene = scene_of(&base);
        let at = scene
            .nodes
            .iter()
            .position(|node| node.file_index == Some(file))
            .unwrap();
        let copy = duplicate(&mut scene, at);
        let image = add_image(&mut scene, pattern_png(Pattern::Noise, 3, 4));
        for index in [copy, at] {
            let mut attempt = scene.clone();
            set_texture(&mut attempt, index, box_texture(image, 1.0));
            let Err(ExportError::InvalidScene(reason)) = build(&base, &attempt) else {
                panic!("箱形を拒否");
            };
            assert!(
                reason.contains("スキン") || reason.contains("モーフ"),
                "{reason}"
            );
        }
    }
}

#[test]
fn same_document_same_bytes_with_images() {
    let base = textured_scene_glb(0, 8, 1);
    let mut scene = scene_of(&base);
    let first = pattern_png(Pattern::Noise, 4, 9);
    let last = pattern_png(Pattern::Noise, 6, 5);
    let last_id = add_image(&mut scene, last.clone());
    let first_id = add_image(&mut scene, first.clone());
    set_texture(&mut scene, 0, box_texture(last_id, 1.0));
    let mut shape = fresh_box_scene();
    let mut node = shape.nodes.remove(0);
    node.draws[0].prim = PrimId(scene.base_primitive_count);
    scene.added = shape.added;
    scene.nodes.insert(0, node);
    set_texture(&mut scene, 0, box_texture(first_id, 1.0));
    let (built, bytes) = export(&base, &scene);
    assert_eq!(build(&base, &scene).unwrap(), built);
    let (_, again) = export(&base, &scene);
    assert_eq!(again, bytes);
    let new = root(&bytes);
    assert_eq!(image_bytes(&bytes, &new, 0), first);
    assert_eq!(image_bytes(&bytes, &new, 1), last);
}

#[test]
fn no_name_no_uri_in_images() {
    let base = textured_scene_glb(0, 8, 2);
    let mut scene = scene_of(&base);
    for (i, data) in [pattern_png(Pattern::Noise, 2, 2), jpeg_8x8()]
        .into_iter()
        .enumerate()
    {
        let image = add_image(&mut scene, data);
        set_texture(&mut scene, i, box_texture(image, 1.0));
    }
    let (_, bytes) = export(&base, &scene);
    let value = root(&bytes);
    assert_eq!(value["images"].as_array().unwrap().len(), 2);
    for (image, mime) in value["images"]
        .as_array()
        .unwrap()
        .iter()
        .zip(["image/png", "image/jpeg"])
    {
        assert_eq!(image.as_object().unwrap().len(), 2);
        assert_eq!(image["mimeType"], mime);
        let view = &value["bufferViews"][image["bufferView"].as_u64().unwrap() as usize];
        assert!(view.get("target").is_none());
    }
}

#[test]
fn untouched_parts_keep_json() {
    let base = edit_glb(&textured_scene_glb(1, 8, 2), |v| {
        v["materials"][0]["pbrMetallicRoughness"]
            .as_object_mut()
            .unwrap()
            .remove("baseColorTexture");
        v["materials"][0]["name"] = "塗装".into();
        v["extensionsUsed"] = json(r#"["KHR_materials_variants"]"#);
        v["extensions"]["KHR_materials_variants"] = json(r#"{"variants":[{"name":"色違い"}]}"#);
        v["meshes"][0]["primitives"][0]["extensions"]["KHR_materials_variants"] =
            json(r#"{"mappings":[{"material":0,"variants":[0]}]}"#);
    });
    let old = root(&base);
    let mut scene = scene_of(&base);
    let image = add_image(&mut scene, pattern_png(Pattern::Stripes, 4, 9));
    set_texture(&mut scene, 0, box_texture(image, 1.5));
    let (_, bytes) = export(&base, &scene);
    let new = root(&bytes);
    assert_eq!(new["nodes"][1], old["nodes"][1]);
    for key in [
        "meshes",
        "materials",
        "images",
        "textures",
        "accessors",
        "bufferViews",
    ] {
        let originals = old[key].as_array().unwrap();
        assert_eq!(&new[key].as_array().unwrap()[..originals.len()], originals);
    }
    assert_eq!(new["extensionsUsed"], old["extensionsUsed"]);
    assert_eq!(new["extensions"], old["extensions"]);
    assert!(primitive_of(&new, 0).get("extensions").is_none());
    assert_eq!(material_of(&new, 0)["name"], "塗装");
}

#[test]
fn verify_rejects_changed_image_bytes() {
    let base = empty_base();
    let scene = fresh_box_scene();
    let (mut built, bytes) = export(&base, &scene);
    let new = root(&bytes);
    let view = new["images"][0]["bufferView"].as_u64().unwrap() as usize;
    let start = new["bufferViews"][view]["byteOffset"].as_u64().unwrap() as usize;
    built.appended_bin[start + 12] ^= 1;
    rejects(&base, &scene, &built);
}

#[test]
fn verify_rejects_dropped_material_fields() {
    let base = edit_glb(&uv_quad(), |v| {
        let pbr = &mut v["materials"][0]["pbrMetallicRoughness"];
        pbr["roughnessFactor"] = 0.25.into();
        pbr["metallicFactor"] = 0.5.into();
        pbr["baseColorTexture"]["extras"] = json(r#"{"custom":true}"#);
        v["materials"][0]["doubleSided"] = true.into();
        v["materials"][0]["extras"] = json(r#"{"custom":7}"#);
    });
    let mut scene = scene_of(&base);
    let image = add_image(&mut scene, pattern_png(Pattern::Noise, 5, 2));
    swap_image(&mut scene, 0, image);
    let (built, _) = export(&base, &scene);
    for (pointer, field) in [
        ("", "doubleSided"),
        ("", "extras"),
        ("/pbrMetallicRoughness", "roughnessFactor"),
        ("/pbrMetallicRoughness", "metallicFactor"),
        ("/pbrMetallicRoughness/baseColorTexture", "extensions"),
        ("/pbrMetallicRoughness/baseColorTexture", "extras"),
    ] {
        let mut value: Value = gltf::json::deserialize::from_slice(&built.json).unwrap();
        let copied = value["materials"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap();
        assert!(
            copied
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(field)
                .is_some()
        );
        let damaged = Built {
            json: gltf::json::serialize::to_vec(&value).unwrap(),
            ..built.clone()
        };
        rejects(&base, &scene, &damaged);
    }
}
