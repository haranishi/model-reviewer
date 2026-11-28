//! 元のGLBから、書き出しの判断に要る値を取り出す。読み込み（`inspect`・`build`）と同じ関数を
//! 使うので、「変えていない」の判定と、読み込みが使うシーンの根が読み込みと食い違わない。

use std::collections::HashMap;

use glam::Affine3A;
use gltf::json::Value;
use model_reviewer_core::scene::{Model, Primitive};
use model_reviewer_core::texture::ImageHash;

use super::container::{self, Container};
use super::{ExportError, ExportScene};
use crate::build::DrawSource;
use crate::{build, inspect};

/// 形状の UV の組（番号順）と、読めなかった組の番号。
#[derive(Default)]
pub(crate) struct SourceSets {
    pub sets: build::UvSets,
    pub skipped: Vec<u32>,
}

pub(crate) struct Base<'a> {
    pub container: Container<'a>,
    /// 元のJSON（値）。
    pub value: Value,
    pub node_count: usize,
    /// ファイルの値から作った行列。作れない（読み込みが通らない値の）ノードは `None`。
    pub locals: Vec<Option<Affine3A>>,
    pub children: Vec<Vec<u32>>,
    pub names: Vec<String>,
    pub skinned: Vec<bool>,
    pub animated: Vec<bool>,
    pub has_scenes: bool,
    /// 読み込みが使ったシーンの番号（シーンがあるときだけ意味を持つ）。
    pub scene_index: usize,
    /// 読み込みが使った根。シーンが無いときは、親の無いノードを番号順に。
    pub roots: Vec<u32>,
    /// 土台のシーンの部品数。復元部品の描画は、この範囲の後ろで参照できる。
    pub scene_nodes: usize,
    source_nodes: Vec<Option<usize>>,
    pub model: Model,
    /// `model.draws` と同じ並びの、描画の出どころ（読み込みが作った対応。数え直さない）。
    pub sources: Vec<DrawSource>,
    /// 材質が参照していない画像も、元の番号で再利用する。
    pub image_indices: HashMap<ImageHash, usize>,
    /// 同じ内容の画像が複数ある場合も、取り消しで元の材質の画像番号を復元する。
    pub image_hashes: Vec<Option<ImageHash>>,
    /// 読み込みが検証した JSON と BIN。焼き込みで元の UV の組を写すときに読む。
    root: gltf::json::Root,
    bin: &'a [u8],
}

pub(crate) fn read<'a>(bytes: &'a [u8], scene: &ExportScene) -> Result<Base<'a>, ExportError> {
    let container = container::parse(bytes).map_err(ExportError::InvalidBase)?;
    let opened =
        inspect::open(bytes).map_err(|error| ExportError::InvalidBase(error.message_ja()))?;
    let root = opened.document.as_json();
    let checked_root = root.clone();
    let checked_bin = opened.bin;
    let mut image_indices = HashMap::new();
    let image_hashes = root
        .images
        .iter()
        .enumerate()
        .map(|(index, image)| {
            let view = &root.buffer_views[image.buffer_view?.value()];
            // inspect::open が検証した BIN 範囲。末尾の詰め物は画像に含めない。
            let start = view.byte_offset.map_or(0, |offset| offset.0) as usize;
            let hash = ImageHash::of(&opened.bin[start..start + view.byte_length.0 as usize]);
            image_indices.entry(hash).or_insert(index);
            Some(hash)
        })
        .collect();
    let node_count = root.nodes.len();
    let mut locals = Vec::with_capacity(node_count);
    let mut children = Vec::with_capacity(node_count);
    let mut names = Vec::with_capacity(node_count);
    let mut skinned = Vec::with_capacity(node_count);
    for (index, node) in root.nodes.iter().enumerate() {
        let index = u32::try_from(index)
            .map_err(|_| ExportError::InvalidBase("ノードが多すぎます".to_owned()))?;
        locals.push(build::local_transform(node).ok());
        children.push(
            node.children
                .iter()
                .flatten()
                .map(|child| child.value() as u32)
                .collect(),
        );
        names.push(build::node_name(node, index));
        skinned.push(node.skin.is_some());
    }
    let mut animated = vec![false; node_count];
    for animation in &root.animations {
        for channel in &animation.channels {
            if let Some(flag) = animated.get_mut(channel.target.node.value()) {
                *flag = true;
            }
        }
    }
    let has_scenes = !root.scenes.is_empty();
    let scene_index = root.scene.map(|index| index.value()).unwrap_or(0);
    let roots = inspect::scene_roots(root)
        .map_err(|error| ExportError::InvalidBase(error.message_ja()))?
        .into_iter()
        .map(|index| index as u32)
        .collect();
    // ノードが0個の土台（新規の文書の `empty_base`）は、空のモデルを土台のモデルにする。
    let referenced = scene
        .nodes
        .iter()
        .filter_map(|node| node.file_index.or(node.template));
    let build::Assembled {
        model,
        sources,
        scene_nodes,
    } = build::assemble_base(opened, referenced)
        .map_err(|error| ExportError::InvalidBase(error.message_ja()))?;
    let mut source_nodes = vec![None; node_count];
    for (at, node) in model.nodes.iter().enumerate() {
        source_nodes[node.gltf_index as usize] = Some(at);
    }
    let value: Value = gltf::json::deserialize::from_slice(container.json)
        .map_err(|error| ExportError::InvalidBase(format!("JSONを読めません: {error}")))?;
    if !value.is_object() {
        return Err(ExportError::InvalidBase(
            "JSONの最上位がオブジェクトではありません".to_owned(),
        ));
    }
    Ok(Base {
        container,
        value,
        node_count,
        locals,
        children,
        names,
        skinned,
        animated,
        has_scenes,
        scene_index,
        roots,
        scene_nodes,
        source_nodes,
        model,
        sources,
        image_indices,
        image_hashes,
        root: checked_root,
        bin: checked_bin,
    })
}

impl Base<'_> {
    /// 読み込みの判定に加え、写すノードとメッシュの生の JSON でも確認する。
    pub fn can_bake(&self, file: u32) -> bool {
        let node = &self.value["nodes"][file as usize];
        let mesh = node["mesh"]
            .as_u64()
            .map(|id| &self.value["meshes"][id as usize]);
        self.model
            .textures
            .no_box_nodes
            .binary_search(&file)
            .is_err()
            && node.get("skin").is_none()
            && mesh.is_some_and(|m| {
                super::json::array(m, "primitives")
                    .iter()
                    .all(|p| p.get("targets").is_none())
            })
    }

    /// ファイルの `meshes[mesh].primitives[slot]` の UV の組を読む。読めない組は番号だけ返す。
    pub fn file_uv_sets(&self, mesh: usize, slot: usize) -> Result<SourceSets, ExportError> {
        let primitive = self
            .root
            .meshes
            .get(mesh)
            .and_then(|m| m.primitives.get(slot))
            .ok_or_else(|| ExportError::InvalidScene("元の形状がありません".into()))?;
        let (sets, skipped) = build::uv_sets(&self.root, self.bin, primitive)
            .map_err(|e| ExportError::InvalidBase(e.message_ja()))?;
        Ok(SourceSets { sets, skipped })
    }

    /// ファイルの `meshes[mesh].primitives[slot]` を、読み込みと同じ式で形状にする。
    pub fn file_primitive(&self, mesh: usize, slot: usize) -> Result<Primitive, ExportError> {
        let primitive = self
            .root
            .meshes
            .get(mesh)
            .and_then(|m| m.primitives.get(slot))
            .ok_or_else(|| ExportError::InvalidScene("開いた時点の形状がありません".to_owned()))?;
        build::decode_primitive(&self.root, self.bin, primitive)
            .map_err(|error| ExportError::InvalidBase(error.message_ja()))?
            .ok_or_else(|| {
                ExportError::InvalidScene("開いた時点の形状に三角形がありません".to_owned())
            })
    }

    /// 形状の材質の基本色の画像が使う UV の組の番号。画像が無ければ `None`。
    pub fn base_color_set(&self, mesh: usize, slot: usize) -> Option<u32> {
        let primitive = &self.value["meshes"][mesh]["primitives"][slot];
        let material = primitive["material"].as_u64()?;
        let info =
            &self.value["materials"][material as usize]["pbrMetallicRoughness"]["baseColorTexture"];
        if !info.is_object() {
            return None;
        }
        let set = info["extensions"]["KHR_texture_transform"]["texCoord"]
            .as_u64()
            .or_else(|| info["texCoord"].as_u64())
            .unwrap_or(0);
        u32::try_from(set).ok()
    }

    /// 焼き込みで写す UV の組。基本色の画像が使う組を読めなければ断る（それ以外は写さずに進む）。
    pub fn bake_uv_sets(&self, mesh: usize, slot: usize) -> Result<SourceSets, ExportError> {
        let sets = self.file_uv_sets(mesh, slot)?;
        if self
            .base_color_set(mesh, slot)
            .is_some_and(|set| sets.skipped.contains(&set))
        {
            return Err(ExportError::InvalidScene(
                "基本色の画像が使う UV の組を読めないため、箱形の形状を作れません".to_owned(),
            ));
        }
        Ok(sets)
    }

    /// ファイルのノード番号から、その部品の元の描画を引く。復元部品と複製の元も含む。
    pub fn draw_range(&self, file: u32) -> Option<std::ops::Range<usize>> {
        let at = self.source_nodes.get(file as usize).copied().flatten()?;
        let range = &self.model.nodes[at].draws;
        Some(range.start as usize..range.end as usize)
    }
}

/// 2つの行列がビット単位で同じか。
pub(crate) fn same_bits(a: Affine3A, b: Affine3A) -> bool {
    a.to_cols_array()
        .iter()
        .zip(b.to_cols_array().iter())
        .all(|(x, y)| x.to_bits() == y.to_bits())
}
