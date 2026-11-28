use crate::glb_writer::{Bank, num, store, vec4_json, write_glb};
use crate::pattern::{Pattern, encode_pattern, jpeg_8x8, pattern_png};
use crate::shapes::{Geometry, box_mesh};

#[cfg(test)]
mod tests;

/// 整数の UV は正規化する。U8 は頂点ごとに4バイト境界へそろえる。
#[derive(Clone, Copy, Debug)]
pub enum UvFormat {
    F32,
    U8,
    U16,
}

/// 各値は glTF のサンプラの列挙値。
#[derive(Clone, Copy, Debug)]
pub struct SamplerOptions {
    pub wrap_s: u32,
    pub wrap_t: u32,
    pub mag_filter: u32,
    pub min_filter: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct TextureTransform {
    pub offset: [f32; 2],
    pub rotation: f32,
    pub scale: [f32; 2],
    pub tex_coord: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
pub enum QuadImage {
    Png(Pattern),
    Jpeg,
}

#[derive(Clone, Copy, Debug)]
pub struct TexturedOptions {
    pub image: QuadImage,
    pub uv_format: UvFormat,
    /// 0番は左上が (0, 0)。奇数番は左右を反転し、読み込んだ組を区別できる。
    pub tex_coord: u32,
    pub sampler: Option<SamplerOptions>,
    pub transform: Option<TextureTransform>,
    pub base_color_factor: [f32; 4],
    /// Some なら MASK 材質。PNG の右半分は透明になる。
    pub mask_cutoff: Option<f32>,
}

impl Default for TexturedOptions {
    fn default() -> Self {
        Self {
            image: QuadImage::Png(Pattern::Quadrants),
            uv_format: UvFormat::F32,
            tex_coord: 0,
            sampler: None,
            transform: None,
            base_color_factor: [1.0; 4],
            mask_cutoff: None,
        }
    }
}

/// XY 平面で +Z を向く2三角形に、8×8の画像を貼る。
pub fn textured_quad_glb(options: &TexturedOptions) -> Vec<u8> {
    let mut scene = Scene::default();
    let geometry = quad();
    let stored = store(&mut scene.bank, &geometry);
    let last_set = options
        .transform
        .and_then(|t| t.tex_coord)
        .unwrap_or(0)
        .max(options.tex_coord);
    let uv_sets: Vec<_> = (0..=last_set)
        .map(|set| {
            add_uvs(
                &mut scene.bank,
                geometry.positions.len(),
                options.uv_format,
                set,
            )
        })
        .collect();
    scene.add_part(stored.primitive(Some(0), &uv_sets), [0, 0]);

    let (mime, bytes) = match options.image {
        QuadImage::Png(pattern) => (
            "image/png",
            encode_pattern(pattern, 8, 0, options.mask_cutoff.is_some()),
        ),
        QuadImage::Jpeg => ("image/jpeg", jpeg_8x8()),
    };
    scene.add_image(mime, &bytes);
    let transform = options.transform.map_or(String::new(), |t| {
        let set = t.tex_coord.map_or(String::new(), |n| format!(r#","texCoord":{n}"#));
        format!(
            r#","extensions":{{"KHR_texture_transform":{{"offset":{:?},"rotation":{},"scale":{:?}{set}}}}}"#,
            t.offset, num(t.rotation), t.scale
        )
    });
    let mask = options.mask_cutoff.map_or(String::new(), |v| {
        format!(r#","alphaMode":"MASK","alphaCutoff":{}"#, num(v))
    });
    scene.materials.push(format!(
        r#"{{"pbrMetallicRoughness":{{"baseColorFactor":{},"baseColorTexture":{{"index":0,"texCoord":{}{transform}}}}}{mask}}}"#,
        vec4_json(options.base_color_factor), options.tex_coord
    ));
    scene.finish(options.sampler, options.transform.is_some())
}

/// 立方体と板を交互に並べる。各画像を別の材質に割り当て、部品はその順に材質を使う。
/// 画像0枚では材質を省き、同じ形状と配置で比較できる。
pub fn textured_scene_glb(images: u32, size: u32, parts: u32) -> Vec<u8> {
    let mut scene = Scene::default();
    let mut shapes = Vec::new();
    for make_geometry in [box_mesh, quad].into_iter().take(parts.min(2) as usize) {
        let geometry = make_geometry();
        let stored = store(&mut scene.bank, &geometry);
        let uv = add_uvs(&mut scene.bank, geometry.positions.len(), UvFormat::F32, 0);
        shapes.push((stored, uv));
    }
    let columns = parts.isqrt().max(1);
    for part in 0..parts {
        let (stored, uv) = &shapes[(part % 2) as usize];
        let material = part.checked_rem(images);
        scene.add_part(
            stored.primitive(material, &[*uv]),
            [part % columns, part / columns],
        );
    }
    for i in 0..images {
        scene.add_image("image/png", &pattern_png(Pattern::Stripes, size, i));
        scene.materials.push(format!(
            r#"{{"pbrMetallicRoughness":{{"baseColorTexture":{{"index":{i}}}}}}}"#
        ));
    }
    scene.finish(None, false)
}

fn quad() -> Geometry {
    Geometry {
        positions: vec![
            [-0.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.5, 0.5, 0.0],
            [-0.5, 0.5, 0.0],
        ],
        normals: Some(vec![[0.0, 0.0, 1.0]; 4]),
        indices: vec![0, 1, 2, 0, 2, 3],
    }
}

fn add_uvs(bank: &mut Bank, count: usize, format: UvFormat, set: u32) -> usize {
    const CORNERS: [[u8; 2]; 4] = [[0, 1], [1, 1], [1, 0], [0, 0]];
    let mut bytes = Vec::new();
    for [u, v] in CORNERS.into_iter().cycle().take(count) {
        let u = if set.is_multiple_of(2) { u } else { 1 - u };
        match format {
            UvFormat::F32 => {
                bytes.extend([u, v].into_iter().flat_map(|n| f32::from(n).to_le_bytes()))
            }
            UvFormat::U8 => bytes.extend([u * 255, v * 255, 0, 0]),
            UvFormat::U16 => bytes.extend(
                [u, v]
                    .into_iter()
                    .flat_map(|n| (u16::from(n) * u16::MAX).to_le_bytes()),
            ),
        }
    }
    let (component, normalized, stride) = match format {
        UvFormat::F32 => (5126, false, None),
        UvFormat::U8 => (5121, true, Some(4)),
        UvFormat::U16 => (5123, true, None),
    };
    let view = bank.add_view(&bytes, stride);
    bank.accessors.push(format!(
        r#"{{"bufferView":{view},"componentType":{component},"normalized":{normalized},"count":{count},"type":"VEC2"}}"#
    ));
    bank.accessors.len() - 1
}

#[derive(Default)]
struct Scene {
    bank: Bank,
    nodes: Vec<String>,
    meshes: Vec<String>,
    materials: Vec<String>,
    images: Vec<String>,
}

impl Scene {
    fn add_part(&mut self, primitive: String, [column, row]: [u32; 2]) {
        let mesh = self.meshes.len();
        self.nodes.push(format!(
            r#"{{"mesh":{mesh},"translation":[{},{},0]}}"#,
            column * 2,
            row * 2
        ));
        self.meshes
            .push(format!(r#"{{"primitives":[{primitive}]}}"#));
    }

    fn add_image(&mut self, mime: &str, bytes: &[u8]) {
        let view = self.bank.add_view(bytes, None);
        self.images
            .push(format!(r#"{{"bufferView":{view},"mimeType":"{mime}"}}"#));
    }

    fn finish(self, sampler: Option<SamplerOptions>, transform: bool) -> Vec<u8> {
        let root_nodes = (0..self.nodes.len())
            .map(|i| i.to_string())
            .collect::<Vec<_>>();
        let mut json = r#"{"asset":{"version":"2.0"}"#.to_owned();
        if !root_nodes.is_empty() {
            json.push_str(&format!(
                r#","scene":0,"scenes":[{{"nodes":[{}]}}]"#,
                root_nodes.join(",")
            ));
        }
        let sampler_ref = if sampler.is_some() {
            r#","sampler":0"#
        } else {
            ""
        };
        let textures = (0..self.images.len())
            .map(|i| format!(r#"{{"source":{i}{sampler_ref}}}"#))
            .collect::<Vec<_>>();
        for (name, items) in [
            ("nodes", &self.nodes),
            ("meshes", &self.meshes),
            ("materials", &self.materials),
            ("textures", &textures),
            ("images", &self.images),
            ("bufferViews", &self.bank.views),
            ("accessors", &self.bank.accessors),
        ] {
            if !items.is_empty() {
                json.push_str(&format!(r#","{name}":[{}]"#, items.join(",")));
            }
        }
        if let Some(s) = sampler {
            json.push_str(&format!(
                r#","samplers":[{{"wrapS":{},"wrapT":{},"magFilter":{},"minFilter":{}}}]"#,
                s.wrap_s, s.wrap_t, s.mag_filter, s.min_filter
            ));
        }
        if transform {
            json.push_str(r#","extensionsUsed":["KHR_texture_transform"]"#);
        }
        let bin = &self.bank.bin;
        if !bin.is_empty() {
            json.push_str(&format!(r#","buffers":[{{"byteLength":{}}}]"#, bin.len()));
        }
        json.push('}');
        write_glb(&json, (!bin.is_empty()).then_some(bin.as_slice()))
    }
}
