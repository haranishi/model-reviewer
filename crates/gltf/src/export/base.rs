//! 元のGLBから、書き出しの判断に要る値を取り出す。読み込み（`inspect`・`build`）と同じ関数を
//! 使うので、「変えていない」の判定と、読み込みが使うシーンの根が読み込みと食い違わない。

use glam::Affine3A;
use gltf::json::Value;
use model_reviewer_core::scene::Model;

use super::ExportError;
use super::container::{self, Container};
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
    /// 読み込んだ `Model` に部品として出たノード。
    pub loaded: Vec<bool>,
    pub model: Model,
    /// `model.draws` と同じ並びの、描画の出どころ（読み込みが作った対応。数え直さない）。
    pub sources: Vec<DrawSource>,
}

pub(crate) fn read(bytes: &[u8]) -> Result<Base<'_>, ExportError> {
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
    let build::Assembled { model, sources } = build::assemble_base(opened)
        .map_err(|error| ExportError::InvalidBase(error.message_ja()))?;
    let mut loaded = vec![false; node_count];
    for node in &model.nodes {
        if let Some(flag) = loaded.get_mut(node.gltf_index as usize) {
            *flag = true;
        }
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
        loaded,
        model,
        sources,
    })
}

impl Base<'_> {
    /// ファイルのノード番号から、土台のモデルの部品の位置。読み込まれていないノードは `None`。
    pub fn loaded_nodes(&self) -> Vec<Option<usize>> {
        let mut at = vec![None; self.node_count];
        for (index, node) in self.model.nodes.iter().enumerate() {
            if let Some(slot) = at.get_mut(node.gltf_index as usize) {
                *slot = Some(index);
            }
        }
        at
    }
}

/// 2つの行列がビット単位で同じか。
pub(crate) fn same_bits(a: Affine3A, b: Affine3A) -> bool {
    a.to_cols_array()
        .iter()
        .zip(b.to_cols_array().iter())
        .all(|(x, y)| x.to_bits() == y.to_bits())
}
