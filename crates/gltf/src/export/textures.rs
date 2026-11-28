//! 文書の画像番号を保持し、書き出し時だけ土台の画像・サンプラ・テクスチャ番号へ解決する。

use std::collections::HashMap;
use std::sync::Arc;

use gltf::json::{Value, texture as gl};
use model_reviewer_core::scene::{Model, SceneRead};
use model_reviewer_core::texture::{
    ImageHash, ImageInfo, ImageSource, Mime, Mip, Sampling, TexId, TexUse, UvMap, Wrap, sniff,
};

use super::ExportError;
use super::append::Appender;
use super::base::Base;
use super::json::{array, obj, push};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportTextures {
    pub images: Vec<ExportImage>,
    pub uses: Vec<TexUse>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportImage {
    pub hash: ImageHash,
    pub mime: Mime,
    /// 貼った画像の付帯情報を除いたバイト列。元からある画像は土台からハッシュで引く。
    pub bytes: Option<Arc<[u8]>>,
}

pub(super) type Look<'a> = Option<(&'a TexUse, Option<ImageHash>)>;

impl ExportTextures {
    /// 元と追加分を合わせた文書の番号順で渡す。圧縮画像は共有する。
    pub fn from_tables<'a>(
        images: impl IntoIterator<Item = &'a ImageInfo>,
        uses: impl IntoIterator<Item = &'a TexUse>,
    ) -> Self {
        Self {
            images: images
                .into_iter()
                .map(|image| ExportImage {
                    hash: image.hash,
                    mime: image.mime,
                    bytes: match &image.source {
                        ImageSource::Held(bytes) => Some(Arc::clone(bytes)),
                        ImageSource::File { .. } => None,
                    },
                })
                .collect(),
            uses: uses.into_iter().copied().collect(),
        }
    }

    pub(super) fn look(&self, id: Option<TexId>) -> Result<Look<'_>, ExportError> {
        let Some(id) = id else {
            return Ok(None);
        };
        let tex = self
            .uses
            .get(id.0 as usize)
            .ok_or_else(|| invalid("貼り方の番号が範囲の外です"))?;
        let hash = tex
            .image
            .map(|id| {
                self.images
                    .get(id.0 as usize)
                    .map(|image| image.hash)
                    .ok_or_else(|| invalid("画像の番号が範囲の外です"))
            })
            .transpose()?;
        Ok(Some((tex, hash)))
    }
}

pub(super) fn model_look(model: &Model, id: Option<TexId>) -> Look<'_> {
    let tex = model.tex_use(id?)?;
    Some((
        tex,
        tex.image
            .and_then(|id| model.image(id))
            .map(|image| image.hash),
    ))
}

pub(super) fn mime_type(mime: Mime) -> &'static str {
    match mime {
        Mime::Png => "image/png",
        Mime::Jpeg => "image/jpeg",
    }
}

pub(super) fn box_size(look: Look<'_>) -> Option<f32> {
    let (tex, _) = look?;
    match (tex.image, tex.map) {
        (Some(_), UvMap::Box { size }) => Some(size),
        _ => None,
    }
}

pub(super) fn can_mark(material: &Value) -> bool {
    material.get("extras").is_none_or(Value::is_object)
        && material["extras"]
            .get("model_reviewer")
            .is_none_or(Value::is_object)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum TextureEdit {
    Keep,
    Remove,
    Vertex { texture: usize, from: u32 },
    Box { texture: usize, size: u32 },
}

pub(super) struct TextureWriter<'a> {
    base: &'a Base<'a>,
    added_images: HashMap<ImageHash, usize>,
    samplers: HashMap<[u32; 4], usize>,
    textures: HashMap<(usize, [u32; 4]), usize>,
    pub image_bytes: u64,
}

impl<'a> TextureWriter<'a> {
    pub fn new(base: &'a Base<'a>) -> Self {
        let mut writer = Self {
            base,
            added_images: HashMap::new(),
            samplers: HashMap::new(),
            textures: HashMap::new(),
            image_bytes: 0,
        };
        for (i, sampler) in array(&base.value, "samplers").iter().enumerate() {
            writer.samplers.entry(sampler_key(sampler)).or_insert(i);
        }
        for (i, texture) in array(&base.value, "textures").iter().enumerate() {
            // 別の形式の画像を優先する拡張を、新しい貼り方に持ち込まない。
            if texture.get("extensions").is_none()
                && let Some(key) = texture_key(&base.value, texture)
            {
                writer.textures.entry(key).or_insert(i);
            }
        }
        writer
    }

    pub fn image_count(&self) -> u32 {
        self.added_images.len() as u32
    }

    pub fn edit(
        &mut self,
        root: &mut Value,
        bin: &mut Appender,
        table: &ExportTextures,
        look: Look<'_>,
    ) -> Result<TextureEdit, ExportError> {
        let Some((tex, _)) = look else {
            return Ok(TextureEdit::Remove);
        };
        let Some(id) = tex.image else {
            return Ok(TextureEdit::Remove);
        };
        let image = &table.images[id.0 as usize];
        let filters = filters(tex.sampling);
        match tex.map {
            UvMap::Vertex { info_from, .. } => {
                let from =
                    info_from.ok_or_else(|| invalid("UV の貼り方に写す元の材質がありません"))?;
                let old = &self.base.value;
                let info =
                    &old["materials"][from as usize]["pbrMetallicRoughness"]["baseColorTexture"];
                let index = info["index"]
                    .as_u64()
                    .ok_or_else(|| invalid("貼り方の写し元に基本色の画像がありません"))?
                    as usize;
                let original = &old["textures"][index];
                // 外して書き出した後も、参照が残る元の textureInfo から正確な番号を取り戻せる。
                if let Some((source, sampling)) = texture_key(old, original)
                    && self.base.image_hashes.get(source) == Some(&Some(image.hash))
                    && sampling == filters
                {
                    return Ok(TextureEdit::Vertex {
                        texture: index,
                        from,
                    });
                }
                let source = self.image(root, bin, image)?;
                let texture = self.texture(root, source, filters)?;
                Ok(TextureEdit::Vertex { texture, from })
            }
            UvMap::Box { size } => {
                let source = self.image(root, bin, image)?;
                let texture = self.texture(root, source, filters)?;
                Ok(TextureEdit::Box {
                    texture,
                    size: size.to_bits(),
                })
            }
        }
    }

    fn image(
        &mut self,
        root: &mut Value,
        bin: &mut Appender,
        image: &ExportImage,
    ) -> Result<usize, ExportError> {
        if let Some(&index) = self
            .base
            .image_indices
            .get(&image.hash)
            .or_else(|| self.added_images.get(&image.hash))
        {
            return Ok(index);
        }
        let bytes = image
            .bytes
            .as_deref()
            .ok_or_else(|| invalid("元の画像が土台にありません"))?;
        if sniff(bytes) != Some(image.mime) || ImageHash::of(bytes) != image.hash {
            return Err(invalid("画像の中身が形式またはハッシュと違います"));
        }
        let mut value = obj();
        value["bufferView"] = bin.view(bytes, None)?.into();
        value["mimeType"] = mime_type(image.mime).into();
        let index = push(root, "images", value)?;
        self.added_images.insert(image.hash, index);
        self.image_bytes += bytes.len() as u64;
        Ok(index)
    }

    fn texture(
        &mut self,
        root: &mut Value,
        source: usize,
        filters: [u32; 4],
    ) -> Result<usize, ExportError> {
        if let Some(&index) = self.textures.get(&(source, filters)) {
            return Ok(index);
        }
        let sampler = match self.samplers.get(&filters) {
            Some(&index) => index,
            None => {
                let mut value = obj();
                for (field, n) in SAMPLER_FIELDS.iter().zip(filters) {
                    value[*field] = n.into();
                }
                let index = push(root, "samplers", value)?;
                self.samplers.insert(filters, index);
                index
            }
        };
        let mut value = obj();
        value["sampler"] = sampler.into();
        value["source"] = source.into();
        let index = push(root, "textures", value)?;
        self.textures.insert((source, filters), index);
        Ok(index)
    }
}

const SAMPLER_FIELDS: [&str; 4] = ["wrapS", "wrapT", "magFilter", "minFilter"];

fn texture_key(root: &Value, texture: &Value) -> Option<(usize, [u32; 4])> {
    let source = texture["source"].as_u64()? as usize;
    let sampler = texture["sampler"]
        .as_u64()
        .map_or(&Value::Null, |id| &root["samplers"][id as usize]);
    Some((source, sampler_key(sampler)))
}

fn sampler_key(value: &Value) -> [u32; 4] {
    let defaults = filters(Sampling::default());
    std::array::from_fn(|i| {
        value[SAMPLER_FIELDS[i]]
            .as_u64()
            .map_or(defaults[i], |n| n as u32)
    })
}

fn filters(s: Sampling) -> [u32; 4] {
    let [u, v] = s.wrap.map(|wrap| match wrap {
        Wrap::Repeat => gl::REPEAT,
        Wrap::Mirror => gl::MIRRORED_REPEAT,
        Wrap::Clamp => gl::CLAMP_TO_EDGE,
    });
    let min = match (s.min_linear, s.mip) {
        (false, Mip::None) => gl::NEAREST,
        (true, Mip::None) => gl::LINEAR,
        (false, Mip::Nearest) => gl::NEAREST_MIPMAP_NEAREST,
        (true, Mip::Nearest) => gl::LINEAR_MIPMAP_NEAREST,
        (false, Mip::Linear) => gl::NEAREST_MIPMAP_LINEAR,
        (true, Mip::Linear) => gl::LINEAR_MIPMAP_LINEAR,
    };
    [
        u,
        v,
        if s.mag_linear {
            gl::LINEAR
        } else {
            gl::NEAREST
        },
        min,
    ]
}

pub(super) fn invalid(message: &str) -> ExportError {
    ExportError::InvalidScene(message.to_owned())
}
