//! 編集できる文書。形状は読み込んだ `Arc<Model>` を共有したまま、部品の表だけを持つ。
//!
//! 部品は行きがけ順の配列で、子孫は `[id, subtree_end)` に連続する（`Model` と同じ並び）。
//! 構造の変更は連続範囲の挿入か削除に限り、番号のずれは `Remap` 1つで表す。履歴と書き出しは
//! `NodeKey`（文書の中で変わらない鍵）で部品を指し、画面と計算は `NodeId`（配列の添字）を使う。
//!
//! 描画の表も部品の並びに連続して並べる（部品 i の描画は、部品 i−1 の描画の直後）。こうすると
//! 部分木の描画も連続範囲になり、挿入と削除で描画の表を同じように継ぎ足し・切り取りできる。
//!
//! 挿入は、部分木が持つ `world`・`world_aabb` をそのまま使う（計算し直さない）。部分木は、
//! 同じ場所から取り除いたもの（取り消し・やり直し）か、いまの文書から作ったもの（複製・図形）
//! なので、値は挿入先で正しい。こうして取り消しの後の文書が元とビット単位で等しくなる。

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use glam::Affine3A;

use crate::math::Aabb;
use crate::review::Annotation;
use crate::scene::{Draw, Model, NodeId, PrimId, Primitive, SceneRead};
use crate::shapes::ShapeSpec;

/// 文書の中で変わらない鍵。開いたときは並び順に 0 から振る。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeKey(pub u64);

/// 文書の部品1件（約200バイト）。名前は `Arc<str>` にして、表の作り直しで文字列を複製しない。
#[derive(Clone, Debug, PartialEq)]
pub struct DocNode {
    pub key: NodeKey,
    pub name: Arc<str>,
    pub parent: Option<NodeId>,
    pub subtree_end: u32,
    pub depth: u32,
    pub local: Affine3A,
    pub world: Affine3A,
    /// `Document` の描画の表の範囲。
    pub draws: Range<u32>,
    pub world_aabb: Aabb,
    /// まだファイルに無い複製が、どのノード（ファイル内の番号）のJSONを写すか。書き出し用。
    pub template: Option<u32>,
}

/// 番号の付け替え。構造の変更1回につき1つ。`map` は定数時間。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remap {
    /// `at` の位置に `len` 個が入った。`at` 以降は `len` だけ後ろへずれる。
    Inserted { at: u32, len: u32 },
    /// `at` から `len` 個が消えた。消えた番号は `None`、後ろは前へずれる。
    Removed { at: u32, len: u32 },
}

impl Remap {
    /// 古い番号を新しい番号へ移す。消えた部品は `None`。
    pub fn map(&self, id: NodeId) -> Option<NodeId> {
        match *self {
            Self::Inserted { at, len } => {
                if id.0 < at {
                    Some(id)
                } else {
                    id.0.checked_add(len).map(NodeId)
                }
            }
            Self::Removed { at, len } => {
                if id.0 < at {
                    Some(id)
                } else if id.0 - at < len {
                    None
                } else {
                    Some(NodeId(id.0 - len))
                }
            }
        }
    }
}

/// 部分木を戻す場所。「親の鍵と、次の兄弟の鍵」で一意に決まる（親が `None` なら根の並び、
/// 次の兄弟が `None` なら末尾）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    pub parent: Option<NodeKey>,
    pub next_sibling: Option<NodeKey>,
}

/// 取り除いた（または足す）部分木。部品の表・描画・表示の設定・注釈を持ち、形状は含まない。
/// `nodes` の `parent`・`subtree_end`・`depth`・`draws` は部分木の中の相対値（先頭が 0、
/// 先頭の `parent` は `None`）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Subtree {
    pub nodes: Vec<DocNode>,
    pub draws: Vec<Draw>,
    /// 各部品の自身の表示設定（`nodes` と同じ並び）。空なら全部表示として扱う。
    pub visible_own: Vec<bool>,
    /// 付いていた注釈と、`ReviewState::annotations` の中の位置。注釈の `node` は部分木の中の
    /// 相対の番号（先頭が 0）。位置は小さい順。
    pub annotations: Vec<(usize, Annotation)>,
}

impl Subtree {
    /// 履歴の上限に数える大きさ（バイトの見積もり）。名前と注釈の文字列も数える。
    pub fn bytes(&self) -> u64 {
        let nodes: usize = self
            .nodes
            .iter()
            .map(|node| std::mem::size_of::<DocNode>() + node.name.len())
            .sum();
        let notes: usize = self
            .annotations
            .iter()
            .map(|(_, note)| std::mem::size_of::<(usize, Annotation)>() + note.text.len())
            .sum();
        (nodes + self.draws.len() * std::mem::size_of::<Draw>() + self.visible_own.len() + notes)
            as u64
    }

    /// 根の鍵。空なら `None`。
    pub fn root_key(&self) -> Option<NodeKey> {
        self.nodes.first().map(|node| node.key)
    }

    /// 相対の値が部分木として正しいか。
    fn is_well_formed(&self) -> bool {
        let len = self.nodes.len();
        let Ok(len32) = u32::try_from(len) else {
            return false;
        };
        if len == 0 || self.nodes[0].parent.is_some() || self.nodes[0].subtree_end != len32 {
            return false;
        }
        let mut draw_cursor = 0u32;
        for (index, node) in self.nodes.iter().enumerate() {
            let index32 = index as u32;
            if node.subtree_end <= index32 || node.subtree_end > len32 {
                return false;
            }
            if node.draws.start != draw_cursor || node.draws.end < node.draws.start {
                return false;
            }
            draw_cursor = node.draws.end;
            if index > 0 {
                let Some(parent) = node.parent else {
                    return false;
                };
                let parent_node = &self.nodes[parent.0 as usize];
                if parent.0 >= index32
                    || parent_node.subtree_end < node.subtree_end
                    || node.depth != parent_node.depth + 1
                {
                    return false;
                }
            } else if node.depth != 0 {
                return false;
            }
        }
        draw_cursor as usize == self.draws.len()
    }
}

/// 削除の結果。
#[derive(Clone, Debug, PartialEq)]
pub struct Removed {
    pub place: Place,
    pub piece: Subtree,
    pub remap: Remap,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocError {
    UnknownKey(NodeKey),
    /// 戻す場所の親か兄弟が、いまの文書に無い。
    InvalidPlace,
    /// 行列が有限でない。
    NonFinite,
    /// 部品の数が `u32` に収まらない。
    TooLarge,
    /// 中身がまだ無い（E0 の置き場所）。
    NotImplemented,
    /// 入れる部分木の相対の値が壊れているか、鍵がすでに文書にある、または描画が知らない形状を指す。
    InvalidPiece,
    /// 履歴の適用で、戻す注釈・視点の番号がすでに使われている。
    IdInUse(u64),
    /// 履歴の適用で、指す注釈・視点が無い。
    UnknownNote(u64),
}

/// 編集できる文書。
#[derive(Clone, Debug)]
pub struct Document {
    base: Arc<Model>,
    added: Vec<Arc<Primitive>>,
    /// `added` と同じ並びで、足した図形の作り方を持つ。記録の無い形状は `None`。
    added_specs: Vec<Option<ShapeSpec>>,
    nodes: Vec<DocNode>,
    draws: Vec<Draw>,
    index_of: HashMap<NodeKey, u32>,
    /// 鍵→ファイル内の番号。取り除いた部品の分も残す（鍵は使い回さないので害が無く、
    /// 取り消しで戻したときにそのまま引ける）。
    file_index: HashMap<NodeKey, u32>,
    /// ファイル内の番号→鍵（`find_file_index` 用）。
    by_file: HashMap<u32, NodeKey>,
    bounds: Aabb,
    next_key: u64,
}

impl Document {
    /// 読み込んだモデルから作る。部品の表を1回だけ写し、鍵を並び順に 0 から振り、
    /// ファイル内の番号の表を作る。
    pub fn new(base: Arc<Model>) -> Self {
        let model = &*base;
        let contiguous = draws_are_contiguous(model);
        let mut draws = if contiguous {
            model.draws.clone()
        } else {
            Vec::new()
        };
        let mut nodes = Vec::with_capacity(model.nodes.len());
        let mut index_of = HashMap::with_capacity(model.nodes.len());
        let mut file_index = HashMap::with_capacity(model.nodes.len());
        let mut by_file = HashMap::with_capacity(model.nodes.len());
        for (index, node) in model.nodes.iter().enumerate() {
            let key = NodeKey(index as u64);
            let range = if contiguous {
                node.draws.clone()
            } else {
                let start = draws.len() as u32;
                draws.extend_from_slice(model.draws(NodeId(index as u32)));
                start..draws.len() as u32
            };
            nodes.push(DocNode {
                key,
                name: Arc::from(node.name.as_str()),
                parent: node.parent,
                subtree_end: node.subtree_end,
                depth: node.depth,
                local: node.local,
                world: node.world,
                draws: range,
                world_aabb: node.world_aabb,
                template: None,
            });
            index_of.insert(key, index as u32);
            file_index.insert(key, node.gltf_index);
            by_file.entry(node.gltf_index).or_insert(key);
        }
        Self {
            bounds: model.bounds,
            next_key: model.nodes.len() as u64,
            base,
            added: Vec::new(),
            added_specs: Vec::new(),
            nodes,
            draws,
            index_of,
            file_index,
            by_file,
        }
    }

    /// 形状の土台（読み込んだモデル）。部品の表は読まないこと（`SceneRead` を使う）。
    pub fn base(&self) -> &Arc<Model> {
        &self.base
    }

    /// 足した図形の形状。番号は `base.primitives.len()` からの続き。
    pub fn added_primitives(&self) -> &[Arc<Primitive>] {
        &self.added
    }

    /// 部品の表全体（行きがけ順）。
    pub fn nodes(&self) -> &[DocNode] {
        &self.nodes
    }

    /// 部品1件。範囲外は `None`。
    pub fn node(&self, id: NodeId) -> Option<&DocNode> {
        self.nodes.get(id.0 as usize)
    }

    /// 番号から鍵。
    pub fn key(&self, id: NodeId) -> Option<NodeKey> {
        self.node(id).map(|node| node.key)
    }

    /// 鍵から番号。
    pub fn id_of(&self, key: NodeKey) -> Option<NodeId> {
        self.index_of.get(&key).copied().map(NodeId)
    }

    /// `id .. subtree_end`。範囲外は空。
    pub fn subtree(&self, id: NodeId) -> Range<u32> {
        match self.node(id) {
            Some(node) => id.0..node.subtree_end,
            None => id.0..id.0,
        }
    }

    /// 部品と子孫のワールド境界箱の和。
    pub fn subtree_bounds(&self, id: NodeId) -> Aabb {
        let range = self.subtree(id);
        self.nodes[range.start as usize..range.end as usize]
            .iter()
            .fold(Aabb::empty(), |bounds, node| bounds.union(node.world_aabb))
    }

    /// `local` を置き換え、その部品と子孫の `world`・`world_aabb` と全体の境界を作り直す。
    /// 返すのは書き換えた番号の範囲（部分更新に使う）。有限でなければ何もせず `NonFinite`。
    pub fn set_local(&mut self, key: NodeKey, local: Affine3A) -> Result<Range<u32>, DocError> {
        if !local.is_finite() {
            return Err(DocError::NonFinite);
        }
        let id = self.id_of(key).ok_or(DocError::UnknownKey(key))?;
        let start = id.0 as usize;
        let end = self.nodes[start].subtree_end as usize;
        self.nodes[start].local = local;
        for index in start..end {
            let parent_world = match self.nodes[index].parent {
                Some(parent) => self.nodes[parent.0 as usize].world,
                None => Affine3A::IDENTITY,
            };
            let world = parent_world * self.nodes[index].local;
            let aabb = self.draw_bounds(self.nodes[index].draws.clone(), &world);
            let node = &mut self.nodes[index];
            node.world = world;
            node.world_aabb = aabb;
        }
        self.recompute_bounds();
        Ok(id.0..end as u32)
    }

    /// 部品とその子孫の、描画の基本色（部品の順、その中は描画の順）。
    pub fn subtree_colors(&self, key: NodeKey) -> Result<Vec<[f32; 4]>, DocError> {
        let draws = self.subtree_draw_range(key)?.1;
        Ok(self.draws[draws.start as usize..draws.end as usize]
            .iter()
            .map(|draw| draw.base_color)
            .collect())
    }

    /// `subtree_colors` と同じ並びで基本色を入れる。形状は共有したままなので、同じ形状を使う
    /// 別の部品には及ばない。数が合わなければ `InvalidPiece`、有限でない値があれば `NonFinite`
    /// で、どちらも何も変えない。返すのは部品の番号の範囲（部分更新に使う）。
    pub fn set_subtree_colors(
        &mut self,
        key: NodeKey,
        colors: &[[f32; 4]],
    ) -> Result<Range<u32>, DocError> {
        let (nodes, draws) = self.subtree_draw_range(key)?;
        if colors.len() != (draws.end - draws.start) as usize {
            return Err(DocError::InvalidPiece);
        }
        if colors.iter().flatten().any(|value| !value.is_finite()) {
            return Err(DocError::NonFinite);
        }
        for (draw, color) in self.draws[draws.start as usize..draws.end as usize]
            .iter_mut()
            .zip(colors)
        {
            draw.base_color = *color;
        }
        Ok(nodes)
    }

    /// 部分木の部品の番号の範囲と、描画の表の範囲。
    fn subtree_draw_range(&self, key: NodeKey) -> Result<(Range<u32>, Range<u32>), DocError> {
        let id = self.id_of(key).ok_or(DocError::UnknownKey(key))?;
        let start = id.0 as usize;
        let end = self.nodes[start].subtree_end as usize;
        let draws = self.nodes[start].draws.start..self.draw_cursor(end);
        Ok((id.0..end as u32, draws))
    }

    /// 部分木を取り除く（データは `Removed` に移す。形状は残す）。`piece` の表示の設定と注釈は
    /// 空（`ReviewState` を持つ側が埋める）。
    pub fn remove_subtree(&mut self, key: NodeKey) -> Result<Removed, DocError> {
        let id = self.id_of(key).ok_or(DocError::UnknownKey(key))?;
        let at = id.0;
        let start = at as usize;
        let end = self.nodes[start].subtree_end as usize;
        let len = (end - start) as u32;
        let parent = self.nodes[start].parent;
        let place = Place {
            parent: parent.map(|parent| self.nodes[parent.0 as usize].key),
            next_sibling: self
                .nodes
                .get(end)
                .filter(|next| next.parent == parent)
                .map(|next| next.key),
        };
        let draw_start = self.nodes[start].draws.start;
        let draw_end = self.draw_cursor(end);
        let draw_len = draw_end - draw_start;
        let root_depth = self.nodes[start].depth;

        let mut nodes: Vec<DocNode> = self.nodes.drain(start..end).collect();
        let draws: Vec<Draw> = self
            .draws
            .drain(draw_start as usize..draw_end as usize)
            .collect();
        for (offset, node) in nodes.iter_mut().enumerate() {
            self.index_of.remove(&node.key);
            node.parent = if offset == 0 {
                None
            } else {
                node.parent.map(|parent| NodeId(parent.0 - at))
            };
            node.subtree_end -= at;
            node.depth -= root_depth;
            node.draws = node.draws.start - draw_start..node.draws.end - draw_start;
        }
        // 祖先の範囲を縮める。
        let mut cursor = parent;
        while let Some(ancestor) = cursor {
            let node = &mut self.nodes[ancestor.0 as usize];
            node.subtree_end -= len;
            cursor = node.parent;
        }
        // 後ろの部品を前へずらす。
        for index in start..self.nodes.len() {
            let node = &mut self.nodes[index];
            if let Some(parent) = node.parent
                && parent.0 >= at
            {
                node.parent = Some(NodeId(parent.0 - len));
            }
            node.subtree_end -= len;
            node.draws = node.draws.start - draw_len..node.draws.end - draw_len;
            self.index_of.insert(node.key, index as u32);
        }
        self.recompute_bounds();
        Ok(Removed {
            place,
            piece: Subtree {
                nodes,
                draws,
                visible_own: Vec::new(),
                annotations: Vec::new(),
            },
            remap: Remap::Removed { at, len },
        })
    }

    /// 部分木を `place` に入れる。鍵は `piece` のものをそのまま使う。部分木の `world` と
    /// `world_aabb` は計算し直さない（モジュールの説明を参照）。`piece` の表示の設定と注釈は
    /// 読まない（`ReviewState` を持つ側が扱う）。
    pub fn insert_subtree(&mut self, place: Place, piece: Subtree) -> Result<Remap, DocError> {
        self.try_insert_subtree(place, piece)
            .map_err(|(error, _)| error)
    }

    /// `insert_subtree` と同じ。断ったときは部分木を返す（履歴が持ち直すため）。
    pub(crate) fn try_insert_subtree(
        &mut self,
        place: Place,
        piece: Subtree,
    ) -> Result<Remap, (DocError, Subtree)> {
        if let Err(error) = self.check_insert(place, &piece) {
            return Err((error, piece));
        }
        Ok(self.insert_checked(place, piece))
    }

    fn check_insert(&self, place: Place, piece: &Subtree) -> Result<(), DocError> {
        self.resolve(place)?;
        if !piece.is_well_formed() {
            return Err(DocError::InvalidPiece);
        }
        let primitive_count = self.primitive_count();
        if piece
            .draws
            .iter()
            .any(|draw| draw.prim.0 as usize >= primitive_count)
        {
            return Err(DocError::InvalidPiece);
        }
        let mut seen = HashSet::with_capacity(piece.nodes.len());
        for node in &piece.nodes {
            if self.index_of.contains_key(&node.key) || !seen.insert(node.key) {
                return Err(DocError::InvalidPiece);
            }
        }
        let total = self.nodes.len() as u64 + piece.nodes.len() as u64;
        let draw_total = self.draws.len() as u64 + piece.draws.len() as u64;
        if total > u64::from(u32::MAX) || draw_total > u64::from(u32::MAX) {
            return Err(DocError::TooLarge);
        }
        Ok(())
    }

    /// `check_insert` を通った部分木を入れる。
    fn insert_checked(&mut self, place: Place, piece: Subtree) -> Remap {
        let Ok(at) = self.resolve(place) else {
            unreachable!("check_insert で確かめた場所");
        };
        let len = piece.nodes.len() as u32;
        let start = at as usize;
        let parent = place.parent.and_then(|key| self.id_of(key));
        let base_depth = parent.map_or(0, |parent| self.nodes[parent.0 as usize].depth + 1);
        let draw_at = self.draw_cursor(start);
        let draw_len = piece.draws.len() as u32;

        // 後ろの部品を後ろへずらす（挿入の前に行う。番号はまだ古い）。
        for node in &mut self.nodes[start..] {
            if let Some(p) = node.parent
                && p.0 >= at
            {
                node.parent = Some(NodeId(p.0 + len));
            }
            node.subtree_end += len;
            node.draws = node.draws.start + draw_len..node.draws.end + draw_len;
        }
        let mut cursor = parent;
        while let Some(ancestor) = cursor {
            let node = &mut self.nodes[ancestor.0 as usize];
            node.subtree_end += len;
            cursor = node.parent;
        }
        let Subtree { nodes, draws, .. } = piece;
        let placed = nodes.into_iter().map(|mut node| {
            node.parent = match node.parent {
                Some(rel) => Some(NodeId(at + rel.0)),
                None => parent,
            };
            node.subtree_end += at;
            node.depth += base_depth;
            node.draws = node.draws.start + draw_at..node.draws.end + draw_at;
            node
        });
        self.nodes.splice(start..start, placed);
        self.draws.splice(draw_at as usize..draw_at as usize, draws);
        for index in start..self.nodes.len() {
            let key = self.nodes[index].key;
            self.index_of.insert(key, index as u32);
            if index < start + len as usize && key.0 >= self.next_key {
                self.next_key = key.0 + 1;
            }
        }
        self.recompute_bounds();
        Remap::Inserted { at, len }
    }

    /// `key` の部分木を写した新しい部分木（新しい鍵、名前は「<元の名前> コピー」、`template` は
    /// 元のファイル内の番号）と、元の直後に置く場所を作る。文書は変えない（入れるのは
    /// `insert_subtree`）。名前を変えるのは根だけで、子孫は元の名前のまま。
    pub fn duplicate_subtree(&mut self, key: NodeKey) -> Result<(Place, Subtree), DocError> {
        let id = self.id_of(key).ok_or(DocError::UnknownKey(key))?;
        let start = id.0 as usize;
        let end = self.nodes[start].subtree_end as usize;
        let parent = self.nodes[start].parent;
        let place = Place {
            parent: parent.map(|parent| self.nodes[parent.0 as usize].key),
            next_sibling: self
                .nodes
                .get(end)
                .filter(|next| next.parent == parent)
                .map(|next| next.key),
        };
        let at = id.0;
        let root_depth = self.nodes[start].depth;
        let draw_start = self.nodes[start].draws.start;
        let draw_end = self.draw_cursor(end);
        let draws = self.draws[draw_start as usize..draw_end as usize].to_vec();
        let mut nodes = Vec::with_capacity(end - start);
        for index in start..end {
            let source = &self.nodes[index];
            let template = self
                .file_index
                .get(&source.key)
                .copied()
                .or(source.template);
            let name = if index == start {
                Arc::from(format!("{} コピー", source.name).as_str())
            } else {
                Arc::clone(&source.name)
            };
            let parent = if index == start {
                None
            } else {
                source.parent.map(|parent| NodeId(parent.0 - at))
            };
            let mut node = DocNode {
                key: NodeKey(0),
                name,
                parent,
                subtree_end: source.subtree_end - at,
                depth: source.depth - root_depth,
                local: source.local,
                world: source.world,
                draws: source.draws.start - draw_start..source.draws.end - draw_start,
                world_aabb: source.world_aabb,
                template,
            };
            node.key = self.allocate_key();
            nodes.push(node);
        }
        Ok((
            place,
            Subtree {
                nodes,
                draws,
                visible_own: Vec::new(),
                annotations: Vec::new(),
            },
        ))
    }

    /// 足した図形の部品1個からなる部分木（根の末尾に置く場所つき）を作る。文書は変えない。
    /// 形状は先に `add_primitive` で足しておく。
    pub fn shape_piece(
        &mut self,
        name: &str,
        local: Affine3A,
        draw: Draw,
    ) -> Result<(Place, Subtree), DocError> {
        if !local.is_finite() {
            return Err(DocError::NonFinite);
        }
        let Some(prim) = self.primitive(draw.prim) else {
            return Err(DocError::InvalidPiece);
        };
        let world_aabb = prim.local_aabb.transformed(&local);
        let key = self.allocate_key();
        let node = DocNode {
            key,
            name: Arc::from(name),
            parent: None,
            subtree_end: 1,
            depth: 0,
            local,
            world: Affine3A::IDENTITY * local,
            draws: 0..1,
            world_aabb,
            template: None,
        };
        Ok((
            Place {
                parent: None,
                next_sibling: None,
            },
            Subtree {
                nodes: vec![node],
                draws: vec![draw],
                visible_own: vec![true],
                annotations: Vec::new(),
            },
        ))
    }

    /// 形状を足して番号を返す。形状は作業中に消さない。
    pub fn add_primitive(&mut self, prim: Arc<Primitive>) -> PrimId {
        self.added.push(prim);
        self.added_specs.push(None);
        PrimId((self.base.primitives.len() + self.added.len() - 1) as u32)
    }

    /// 図形の形状を足して番号を返す。どの図形をどの大きさで作ったかを覚える（`shape_spec`）。
    pub fn add_shape_primitive(&mut self, prim: Arc<Primitive>, spec: ShapeSpec) -> PrimId {
        let id = self.add_primitive(prim);
        if let Some(slot) = self.added_specs.last_mut() {
            *slot = Some(spec);
        }
        id
    }

    /// 足した図形の作り方。読み込んだ形状と、記録の無い形状は `None`。
    pub fn shape_spec(&self, prim: PrimId) -> Option<ShapeSpec> {
        let index = (prim.0 as usize).checked_sub(self.base.primitives.len())?;
        self.added_specs.get(index).copied().flatten()
    }

    /// 使っていない鍵を1つ取る。
    pub fn allocate_key(&mut self) -> NodeKey {
        let key = NodeKey(self.next_key);
        self.next_key += 1;
        key
    }

    /// 書き出しの後、各部品のファイル内の番号を入れ、複製の `template` を消す。
    /// `assigned[i]` は行きがけ順で i 番目の部品の番号。長さが部品の数と違えば何もしない。
    /// 取り除いた部品の番号は残す（書き出しはノードを配列に残すので、取り消しで戻せば
    /// 同じ番号を指す）。
    pub fn set_file_indices(&mut self, assigned: &[u32]) {
        if assigned.len() != self.nodes.len() {
            return;
        }
        self.by_file.clear();
        for (node, &index) in self.nodes.iter_mut().zip(assigned) {
            node.template = None;
            self.file_index.insert(node.key, index);
            self.by_file.insert(index, node.key);
        }
        for (key, index) in &self.file_index {
            self.by_file.entry(*index).or_insert(*key);
        }
    }

    /// 書き出したファイルを読み直した行列（行きがけ順）を入れる。書き出しは行列を位置・回転・
    /// 大きさに分けて書くので、読み直すと最後の桁が変わることがある。入れておくと、文書の
    /// `world`・`world_aabb`・`bounds` が、そのファイルを開き直したときと同じ値になる（初期視点の
    /// 距離や、次の書き出しで「変えていない部品」の見分けがそろう）。ビット単位で違う部品が
    /// あれば、全部の `world` を読み込みと同じ式（親の `world` × `local`）で1回で作り直し、
    /// `true` を返す。長さが部品の数と違うか、有限でない行列があれば何もしない。
    pub fn adopt_file_locals(&mut self, locals: &[Affine3A]) -> bool {
        if locals.len() != self.nodes.len() || locals.iter().any(|local| !local.is_finite()) {
            return false;
        }
        let changed = self
            .nodes
            .iter()
            .zip(locals)
            .any(|(node, local)| !same_bits(&node.local, local));
        if !changed {
            return false;
        }
        for (index, &local) in locals.iter().enumerate() {
            let parent_world = match self.nodes[index].parent {
                Some(parent) => self.nodes[parent.0 as usize].world,
                None => Affine3A::IDENTITY,
            };
            let world = parent_world * local;
            let aabb = self.draw_bounds(self.nodes[index].draws.clone(), &world);
            let node = &mut self.nodes[index];
            node.local = local;
            node.world = world;
            node.world_aabb = aabb;
        }
        self.recompute_bounds();
        true
    }

    /// `place` が指す挿入位置。
    fn resolve(&self, place: Place) -> Result<u32, DocError> {
        let parent = match place.parent {
            Some(key) => Some(self.id_of(key).ok_or(DocError::InvalidPlace)?),
            None => None,
        };
        match place.next_sibling {
            Some(key) => {
                let sibling = self.id_of(key).ok_or(DocError::InvalidPlace)?;
                if self.nodes[sibling.0 as usize].parent != parent {
                    return Err(DocError::InvalidPlace);
                }
                Ok(sibling.0)
            }
            None => Ok(match parent {
                Some(parent) => self.nodes[parent.0 as usize].subtree_end,
                None => self.nodes.len() as u32,
            }),
        }
    }

    /// 部品 `index` の描画が始まる位置（末尾なら描画の表の長さ）。
    fn draw_cursor(&self, index: usize) -> u32 {
        self.nodes
            .get(index)
            .map_or(self.draws.len() as u32, |node| node.draws.start)
    }

    fn draw_bounds(&self, range: Range<u32>, world: &Affine3A) -> Aabb {
        let mut aabb = Aabb::empty();
        for draw in self
            .draws
            .get(range.start as usize..range.end as usize)
            .unwrap_or(&[])
        {
            if let Some(prim) = self.primitive(draw.prim) {
                aabb = aabb.union(prim.local_aabb.transformed(world));
            }
        }
        aabb
    }

    fn recompute_bounds(&mut self) {
        self.bounds = self
            .nodes
            .iter()
            .fold(Aabb::empty(), |bounds, node| bounds.union(node.world_aabb));
    }
}

fn same_bits(a: &Affine3A, b: &Affine3A) -> bool {
    a.to_cols_array()
        .iter()
        .zip(b.to_cols_array().iter())
        .all(|(x, y)| x.to_bits() == y.to_bits())
}

/// 部品の描画の範囲が、部品の並びどおりに隙間なく続いているか。
fn draws_are_contiguous(model: &Model) -> bool {
    let mut cursor = 0u32;
    for node in &model.nodes {
        if node.draws.start != cursor || node.draws.end < node.draws.start {
            return false;
        }
        cursor = node.draws.end;
    }
    cursor as usize == model.draws.len()
}

impl SceneRead for Document {
    fn node_count(&self) -> usize {
        self.nodes.len()
    }

    fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id.0 as usize].parent
    }

    fn subtree_end(&self, id: NodeId) -> u32 {
        self.nodes[id.0 as usize].subtree_end
    }

    fn depth(&self, id: NodeId) -> u32 {
        self.nodes[id.0 as usize].depth
    }

    fn name(&self, id: NodeId) -> &str {
        &self.nodes[id.0 as usize].name
    }

    fn world(&self, id: NodeId) -> &Affine3A {
        &self.nodes[id.0 as usize].world
    }

    fn world_aabb(&self, id: NodeId) -> Aabb {
        self.nodes[id.0 as usize].world_aabb
    }

    fn draws(&self, id: NodeId) -> &[Draw] {
        let range = &self.nodes[id.0 as usize].draws;
        self.draws
            .get(range.start as usize..range.end as usize)
            .unwrap_or(&[])
    }

    fn primitive(&self, id: PrimId) -> Option<&Primitive> {
        let index = id.0 as usize;
        let base = self.base.primitives.len();
        if index < base {
            self.base.primitives.get(index)
        } else {
            self.added.get(index - base).map(|prim| &**prim)
        }
    }

    fn primitive_count(&self) -> usize {
        self.base.primitives.len() + self.added.len()
    }

    fn file_index(&self, id: NodeId) -> Option<u32> {
        let key = self.nodes[id.0 as usize].key;
        self.file_index.get(&key).copied()
    }

    fn find_file_index(&self, gltf_index: u32) -> Option<NodeId> {
        self.by_file
            .get(&gltf_index)
            .and_then(|key| self.id_of(*key))
    }

    fn bounds(&self) -> Aabb {
        self.bounds
    }

    fn shares_geometry_with(&self, model: &Model) -> bool {
        std::ptr::eq(Arc::as_ptr(&self.base), model)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::scene::{ModelStats, Node};
    use glam::Vec3;

    pub(crate) const RED: [f32; 4] = [0.7, 0.1, 0.1, 1.0];
    pub(crate) const GREEN: [f32; 4] = [0.1, 0.6, 0.1, 0.5];
    pub(crate) const BLUE: [f32; 4] = [0.1, 0.2, 0.8, 1.0];
    pub(crate) const GREY: [f32; 4] = [0.5, 0.5, 0.5, 1.0];

    fn prim() -> Primitive {
        Primitive {
            vertices: Vec::new(),
            indices: Vec::new(),
            local_aabb: Aabb {
                min: Vec3::splat(-0.5),
                max: Vec3::splat(0.5),
            },
            has_normals: true,
        }
    }

    /// 0「台」（描画なし）の下に 1「脚A」（形状0 赤・形状1 緑）と 2「脚B」（形状0 青）。
    /// 根の並びに 3「棚」（形状0 灰）。形状0 は脚A・脚B・棚が共有する。
    pub(crate) fn sample() -> Document {
        let parts: [(&str, Option<u32>, u32, Vec<Draw>); 4] = [
            ("台", None, 3, vec![]),
            (
                "脚A",
                Some(0),
                2,
                vec![
                    Draw {
                        prim: PrimId(0),
                        base_color: RED,
                    },
                    Draw {
                        prim: PrimId(1),
                        base_color: GREEN,
                    },
                ],
            ),
            (
                "脚B",
                Some(0),
                3,
                vec![Draw {
                    prim: PrimId(0),
                    base_color: BLUE,
                }],
            ),
            (
                "棚",
                None,
                4,
                vec![Draw {
                    prim: PrimId(0),
                    base_color: GREY,
                }],
            ),
        ];
        let mut nodes = Vec::new();
        let mut draws = Vec::new();
        for (index, (name, parent, subtree_end, own)) in parts.into_iter().enumerate() {
            let start = draws.len() as u32;
            draws.extend(own);
            let local = Affine3A::from_translation(Vec3::new(index as f32, 0.0, 0.0));
            nodes.push(Node {
                name: name.to_owned(),
                gltf_index: index as u32,
                parent: parent.map(NodeId),
                subtree_end,
                depth: u32::from(parent.is_some()),
                local,
                world: local,
                draws: start..draws.len() as u32,
                world_aabb: prim().local_aabb.transformed(&local),
            });
        }
        let model = Model {
            nodes,
            draws,
            primitives: vec![prim(), prim()],
            bounds: Aabb::empty(),
            stats: ModelStats::default(),
            warnings: Vec::new(),
        };
        Document::new(Arc::new(model))
    }

    fn all_colors(doc: &Document) -> Vec<[f32; 4]> {
        (0..doc.node_count())
            .flat_map(|index| doc.draws(NodeId(index as u32)).iter())
            .map(|draw| draw.base_color)
            .collect()
    }

    #[test]
    fn subtree_colors_are_in_node_then_draw_order() {
        let doc = sample();
        assert_eq!(
            doc.subtree_colors(NodeKey(0)).unwrap(),
            vec![RED, GREEN, BLUE]
        );
        assert_eq!(doc.subtree_colors(NodeKey(2)).unwrap(), vec![BLUE]);
        assert_eq!(
            doc.subtree_colors(NodeKey(9)),
            Err(DocError::UnknownKey(NodeKey(9)))
        );
    }

    #[test]
    fn color_does_not_leak_to_shared_shape() {
        let mut doc = sample();
        let yellow = [0.9, 0.8, 0.1, 1.0];
        let range = doc
            .set_subtree_colors(NodeKey(1), &[yellow, yellow])
            .unwrap();
        assert_eq!(range, 1..2);
        // 形状0 を共有する脚B と棚は変わらない。形状も同じものを指したまま。
        assert_eq!(all_colors(&doc), vec![yellow, yellow, BLUE, GREY]);
        assert_eq!(doc.draws(NodeId(2))[0].prim, PrimId(0));
        assert_eq!(doc.draws(NodeId(1))[0].prim, PrimId(0));
        assert!(doc.added_primitives().is_empty());
        // 親を選ぶと子孫が対象になり、範囲は部分木。根の並びの別の部品には及ばない。
        let range = doc
            .set_subtree_colors(NodeKey(0), &[GREY, GREY, GREY])
            .unwrap();
        assert_eq!(range, 0..3);
        assert_eq!(all_colors(&doc), vec![GREY; 4]);
    }

    #[test]
    fn wrong_count_changes_nothing() {
        let mut doc = sample();
        let before = all_colors(&doc);
        assert_eq!(
            doc.set_subtree_colors(NodeKey(0), &[GREY, GREY]),
            Err(DocError::InvalidPiece)
        );
        assert_eq!(
            doc.set_subtree_colors(NodeKey(0), &[GREY; 4]),
            Err(DocError::InvalidPiece)
        );
        assert_eq!(
            doc.set_subtree_colors(NodeKey(0), &[GREY, [f32::NAN, 0.0, 0.0, 1.0], GREY]),
            Err(DocError::NonFinite)
        );
        assert_eq!(
            doc.set_subtree_colors(NodeKey(7), &[]),
            Err(DocError::UnknownKey(NodeKey(7)))
        );
        assert_eq!(all_colors(&doc), before);
        // 描画の無い部品は、空の並びなら受ける（何も変わらない）。
        let mut empty = sample();
        let piece = empty.shape_piece_without_draws();
        assert_eq!(empty.subtree_colors(piece).unwrap(), Vec::<[f32; 4]>::new());
        assert!(empty.set_subtree_colors(piece, &[]).is_ok());
    }

    impl Document {
        /// 試験用: 描画の無い部品を根の末尾に足し、その鍵を返す。
        fn shape_piece_without_draws(&mut self) -> NodeKey {
            let (place, mut piece) = self
                .shape_piece(
                    "空",
                    Affine3A::IDENTITY,
                    Draw {
                        prim: PrimId(0),
                        base_color: GREY,
                    },
                )
                .unwrap();
            piece.draws.clear();
            piece.nodes[0].draws = 0..0;
            let key = piece.nodes[0].key;
            self.insert_subtree(place, piece).unwrap();
            key
        }
    }

    #[test]
    fn duplicate_inherits_color() {
        let mut doc = sample();
        let yellow = [0.9, 0.8, 0.1, 1.0];
        doc.set_subtree_colors(NodeKey(1), &[yellow, GREEN])
            .unwrap();
        let (place, piece) = doc.duplicate_subtree(NodeKey(0)).unwrap();
        let colors: Vec<[f32; 4]> = piece.draws.iter().map(|draw| draw.base_color).collect();
        assert_eq!(colors, vec![yellow, GREEN, BLUE]);
        let copy = piece.nodes[0].key;
        doc.insert_subtree(place, piece).unwrap();
        assert_eq!(doc.subtree_colors(copy).unwrap(), vec![yellow, GREEN, BLUE]);
        // 複製の色だけを変えても、元は変わらない。
        doc.set_subtree_colors(copy, &[RED; 3]).unwrap();
        assert_eq!(
            doc.subtree_colors(NodeKey(0)).unwrap(),
            vec![yellow, GREEN, BLUE]
        );
        assert_eq!(doc.subtree_colors(copy).unwrap(), vec![RED; 3]);
    }
}
