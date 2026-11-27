//! 元のGLBから、書き出しの判断に要る値を取り出す。読み込み（`inspect`・`build`）と同じ関数を
//! 使うので、「変えていない」の判定と、読み込みが使うシーンの根が読み込みと食い違わない。

use glam::Affine3A;
use gltf::json::Value;
use model_reviewer_core::scene::Model;

use super::container::{self, Container};
use super::{ExportError, ExportScene};
use crate::build::DrawSource;
use crate::{build, inspect};

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
}

pub(crate) fn read<'a>(bytes: &'a [u8], scene: &ExportScene) -> Result<Base<'a>, ExportError> {
    let container = container::parse(bytes).map_err(ExportError::InvalidBase)?;
    let opened =
        inspect::open(bytes).map_err(|error| ExportError::InvalidBase(error.message_ja()))?;
    let root = opened.document.as_json();
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
    })
}

impl Base<'_> {
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
