//! 追加画像を圧縮バイト列のまま照合し、画像のビューと形状のビューが混ざっていないことを確かめる。

use std::collections::{HashMap, HashSet};

use gltf::json::Value;
use model_reviewer_core::texture::ImageHash;

use super::{fail, without};
use crate::export::base::Base;
use crate::export::json::{array, obj};
use crate::export::textures::mime_type;
use crate::export::{Built, ExportError, ExportTextures};

pub(super) fn check(
    old: &Value,
    new: &Value,
    base: &Base<'_>,
    built: &Built,
    table: &ExportTextures,
    start: u64,
) -> Result<(), ExportError> {
    let added = |key| &array(new, key)[array(old, key).len()..];
    let index = |value: &Value| value.as_u64().and_then(|n| usize::try_from(n).ok());
    let views = array(new, "bufferViews");
    let first_view = array(old, "bufferViews").len();
    let mut geometry = HashSet::new();
    for accessor in added("accessors") {
        let view = index(&accessor["bufferView"])
            .filter(|&i| i >= first_view && i < views.len())
            .ok_or_else(|| fail("足したアクセサが足したビューを指していません".into()))?;
        geometry.insert(view);
    }
    let held: HashMap<_, _> = table
        .images
        .iter()
        .filter(|image| image.bytes.is_some())
        .map(|image| (image.hash, image))
        .collect();
    let mut hashes = HashSet::new();
    let mut image_views = HashSet::new();
    for image in added("images") {
        let view = index(&image["bufferView"])
            .filter(|&i| i >= first_view && i < views.len())
            .ok_or_else(|| fail("足した画像のビューが範囲の外です".into()))?;
        let bytes = appended_view(&views[view], start, &built.appended_bin)
            .ok_or_else(|| fail("足した画像のバイト範囲が違います".into()))?;
        let hash = ImageHash::of(bytes);
        let matches = held.get(&hash).is_some_and(|source| {
            source.bytes.as_deref() == Some(bytes)
                && image["mimeType"].as_str() == Some(mime_type(source.mime))
        });
        if !matches
            || base.image_indices.contains_key(&hash)
            || !hashes.insert(hash)
            || !image_views.insert(view)
            || geometry.contains(&view)
            || without(image, &["mimeType", "bufferView"]) != obj()
            || without(&views[view], &["buffer", "byteOffset", "byteLength"]) != obj()
        {
            return Err(fail(
                "足した画像が文書の中身と違うか、重複または余分な項目があります".into(),
            ));
        }
    }
    if (first_view..views.len()).any(|v| !geometry.contains(&v) && !image_views.contains(&v)) {
        return Err(fail("足したビューを使う画像も形状もありません".into()));
    }
    let mut sources = HashSet::new();
    let mut samplers = HashSet::new();
    for texture in added("textures") {
        let source = index(&texture["source"]).filter(|&i| i < array(new, "images").len());
        let sampler = index(&texture["sampler"]).filter(|&i| i < array(new, "samplers").len());
        match (source, sampler) {
            (Some(i), Some(s)) if without(texture, &["source", "sampler"]) == obj() => {
                sources.insert(i);
                samplers.insert(s);
            }
            _ => {
                return Err(fail(
                    "足したテクスチャが画像とサンプラを指していません".into(),
                ));
            }
        }
    }
    let textures: HashSet<_> = added("materials")
        .iter()
        .filter_map(|m| index(&m["pbrMetallicRoughness"]["baseColorTexture"]["index"]))
        .collect();
    for (key, used) in [
        ("images", sources),
        ("samplers", samplers),
        ("textures", textures),
    ] {
        if (array(old, key).len()..array(new, key).len()).any(|i| !used.contains(&i)) {
            return Err(fail(format!("使われていない {key} を足しています")));
        }
    }
    for sampler in added("samplers") {
        let fields = ["wrapS", "wrapT", "magFilter", "minFilter"];
        if without(sampler, &fields) != obj()
            || fields.iter().any(|&key| sampler[key].as_u64().is_none())
        {
            return Err(fail("足したサンプラの項目が違います".into()));
        }
    }
    Ok(())
}

fn appended_view<'a>(view: &Value, start: u64, tail: &'a [u8]) -> Option<&'a [u8]> {
    let offset = view["byteOffset"].as_u64()?.checked_sub(start)?;
    let end = offset.checked_add(view["byteLength"].as_u64()?)?;
    tail.get(usize::try_from(offset).ok()?..usize::try_from(end).ok()?)
}
