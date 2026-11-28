//! 取り消しとやり直し。持つのは逆の操作であり、文書の写しではない。
//!
//! 1回分（`Step`）は利用者から見た1回の操作。対象は未保存の印が付く操作すべて（変形、追加、
//! 複製、削除、表示の切り替え、注釈と視点の追加・編集・削除）。視点の移動と選択は入れない。
//! `Step` ごとに通しの版の番号を振り、文書の版とレビューの版を別々に進める。
//!
//! 使い方は2通り。`History::execute` に `Step` を渡すと、適用してから積む（削除は
//! `Edit::remove_request` で作った `Edit` を渡せば、取り除いた中身を `Edit` に埋める）。
//! 自分で適用した操作は `History::push` で積む。
//!
//! 適用済みの `Insert` と、取り消し済みの `Remove` は、`piece` に根の1件だけを残す（鍵を
//! 覚えるため）。中身は文書の中にあるので、履歴に二重に持たない。

use std::ops::Range;

use glam::Affine3A;

use crate::doc::{DocError, Document, DrawLook, NodeKey, Place, Remap, Subtree};
use crate::review::{Annotation, ReviewState, SavedView};
use crate::scene::NodeId;

/// 上限の初期値（実測で見直す）。
pub const DEFAULT_BYTE_LIMIT: u64 = 64 * 1024 * 1024;
pub const DEFAULT_STEP_LIMIT: usize = 500;

/// 部品を鍵で指す注釈。`note.node` は記録した時点の番号で、戻すときは `node` の鍵から引き直す。
#[derive(Clone, Debug, PartialEq)]
pub struct KeyedNote {
    pub node: NodeKey,
    pub note: Annotation,
}

/// 1つの操作とその逆。各 `Edit` は自分の大きさを `bytes` で申告する。
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    SetLocal {
        key: NodeKey,
        before: Affine3A,
        after: Affine3A,
    },
    Insert {
        at: Place,
        piece: Subtree,
    },
    Remove {
        at: Place,
        piece: Subtree,
    },
    SetVisible {
        key: NodeKey,
        before: bool,
        after: bool,
    },
    ShowAll {
        hidden: Vec<NodeKey>,
    },
    AddNote(KeyedNote),
    EditNote {
        id: u64,
        before: String,
        after: String,
    },
    RemoveNote {
        index: usize,
        note: KeyedNote,
    },
    AddView(SavedView),
    RemoveView {
        index: usize,
        view: SavedView,
    },
    /// 部品とその子孫の、すべての描画の基本色の赤・緑・青を `rgb`（リニア）にする。透明度は
    /// 描画ごとの元の値を保つ。`before` は描画ごとの元の色（`Document::subtree_colors` の並び）。
    /// 進むときは、いまの色で `before` を埋め直してから入れる。戻るときは `before` を入れる。
    /// 並びの対応は、履歴が1列であることに頼る（戻すときの文書は、進んだ直後と同じ形）。
    SetColors {
        key: NodeKey,
        before: Vec<[f32; 4]>,
        rgb: [f32; 3],
    },
    /// 部品と子孫の描画の色と貼り方をまとめて替える。どちらも `subtree_looks` の順。
    SetLooks {
        key: NodeKey,
        before: Vec<DrawLook>,
        after: Vec<DrawLook>,
    },
}

impl Edit {
    /// 履歴の上限に数える大きさ（バイトの見積もり）。型の大きさに、持っている表と文字列を足す。
    pub fn bytes(&self) -> u64 {
        let heap = match self {
            Self::SetLocal { .. } | Self::SetVisible { .. } => 0,
            Self::Insert { piece, .. } | Self::Remove { piece, .. } => piece.bytes(),
            Self::ShowAll { hidden } => (hidden.len() * std::mem::size_of::<NodeKey>()) as u64,
            Self::AddNote(note) | Self::RemoveNote { note, .. } => note.note.text.len() as u64,
            Self::EditNote { before, after, .. } => (before.len() + after.len()) as u64,
            Self::AddView(view) | Self::RemoveView { view, .. } => view.name.len() as u64,
            Self::SetColors { before, .. } => {
                (before.len() * std::mem::size_of::<[f32; 4]>()) as u64
            }
            Self::SetLooks { before, after, .. } => {
                (before.len() as u64 + after.len() as u64) * std::mem::size_of::<DrawLook>() as u64
            }
        };
        std::mem::size_of::<Self>() as u64 + heap
    }

    /// 部品の表を変えるか（文書の版を進めるか）。
    pub fn changes_document(&self) -> bool {
        matches!(
            self,
            Self::SetLocal { .. }
                | Self::Insert { .. }
                | Self::Remove { .. }
                | Self::SetColors { .. }
                | Self::SetLooks { .. }
        )
    }

    /// 表示・注釈・視点を変えるか（レビューの版を進めるか）。挿入と削除は、表示の設定と
    /// 注釈も動かすので両方を変える。変形と見た目はレビューを変えない。
    pub fn changes_review(&self) -> bool {
        !matches!(
            self,
            Self::SetLocal { .. } | Self::SetColors { .. } | Self::SetLooks { .. }
        )
    }

    /// `key` の部分木を削除する `Edit`。`History::execute` に渡すと、取り除いた中身
    /// （部品の表・表示の設定・注釈）と戻す場所が埋まる。鍵が無ければ `None`。
    pub fn remove_request(doc: &Document, key: NodeKey) -> Option<Self> {
        let node = doc.node(doc.id_of(key)?)?.clone();
        Some(Self::Remove {
            at: Place {
                parent: None,
                next_sibling: None,
            },
            piece: Subtree {
                nodes: vec![node],
                ..Subtree::default()
            },
        })
    }
}

/// 1回分の操作。版の番号は `History::push` が振る。
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub label: &'static str,
    pub edits: Vec<Edit>,
    /// 取り消し・やり直しのあとで選ぶ部品。
    pub select: Option<NodeKey>,
    rev: u64,
    /// 積む前の文書の版とレビューの版（取り消しで戻す値）。
    before_doc_rev: u64,
    before_review_rev: u64,
}

impl Step {
    pub fn new(label: &'static str, edits: Vec<Edit>, select: Option<NodeKey>) -> Self {
        Self {
            label,
            edits,
            select,
            rev: 0,
            before_doc_rev: 0,
            before_review_rev: 0,
        }
    }

    /// 履歴の上限に数える大きさ（バイトの見積もり）。
    pub fn bytes(&self) -> u64 {
        std::mem::size_of::<Self>() as u64 + self.edits.iter().map(Edit::bytes).sum::<u64>()
    }

    fn changes_document(&self) -> bool {
        self.edits.iter().any(Edit::changes_document)
    }

    fn changes_review(&self) -> bool {
        self.edits.iter().any(Edit::changes_review)
    }

    /// `push` が振った版の番号（積む前は 0）。
    pub fn rev(&self) -> u64 {
        self.rev
    }
}

/// 取り消し・やり直しで起きたこと。`Session` はこれを見て、選択・下書き・選択中の注釈の
/// 番号を付け替え、GPUの更新（部分か全体か）と未保存の印を決める。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Applied {
    /// 起きた順の番号のずれ。文書と `ReviewState` の分は `History` が付け替え済み。
    pub remaps: Vec<Remap>,
    /// 変形で `world` が変わった番号の範囲（構造が変わったときは使わない）。
    pub touched: Vec<Range<u32>>,
    /// 色だけが変わった部品の番号の範囲（構造が変わったときは使わない）。
    pub recolored: Vec<Range<u32>>,
    /// 貼り方の番号が変わった描画があり、インスタンスを組み直す必要がある。
    pub retextured: bool,
    /// 部品の表の構造が変わった（インスタンスを作り直す）。
    pub structure: bool,
    /// 表示状態が変わった。
    pub visibility: bool,
    pub select: Option<NodeKey>,
    /// 適用の後の文書の版とレビューの版。
    pub doc_rev: u64,
    pub review_rev: u64,
    pub label: &'static str,
}

/// `push` の結果。1回分だけで上限を超えたときは、実行したうえで履歴を空にする。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pushed {
    Kept,
    /// 「これより前へは戻せません」と状態欄に出す。
    ClearedTooLarge,
}

#[derive(Clone, Debug)]
pub struct History {
    byte_limit: u64,
    step_limit: usize,
    undo: Vec<Step>,
    redo: Vec<Step>,
    next_rev: u64,
    doc_rev: u64,
    review_rev: u64,
    /// 両方の列の大きさの合計（毎回数え直さないための控え）。
    bytes: u64,
}

impl Default for History {
    fn default() -> Self {
        Self::with_limits(DEFAULT_BYTE_LIMIT, DEFAULT_STEP_LIMIT)
    }
}

impl History {
    pub fn with_limits(byte_limit: u64, step_limit: usize) -> Self {
        Self {
            byte_limit,
            step_limit,
            undo: Vec::new(),
            redo: Vec::new(),
            next_rev: 1,
            doc_rev: 0,
            review_rev: 0,
            bytes: 0,
        }
    }

    /// 実行済みの操作を積む。やり直しの列は捨てる。版の番号を振り、変えた側の版を進める。
    /// 上限を超えたら古い順に捨てる。1回分だけで上限を超えるときは、版を進めたうえで
    /// 履歴を空にし `ClearedTooLarge` を返す。`edits` が空の `Step` は何もせず `Kept`。
    pub fn push(&mut self, mut step: Step) -> Pushed {
        if step.edits.is_empty() {
            return Pushed::Kept;
        }
        for dropped in self.redo.drain(..) {
            self.bytes = self.bytes.saturating_sub(dropped.bytes());
        }
        step.before_doc_rev = self.doc_rev;
        step.before_review_rev = self.review_rev;
        step.rev = self.next_rev;
        self.next_rev += 1;
        if step.changes_document() {
            self.doc_rev = step.rev;
        }
        if step.changes_review() {
            self.review_rev = step.rev;
        }
        let size = step.bytes();
        if size > self.byte_limit || self.step_limit == 0 {
            self.undo.clear();
            self.bytes = 0;
            return Pushed::ClearedTooLarge;
        }
        self.undo.push(step);
        self.bytes += size;
        self.evict();
        Pushed::Kept
    }

    /// `step` を文書と `ReviewState` に適用してから積む。途中で断られたら、適用した分を
    /// 戻して何も積まずに誤りを返す。
    pub fn execute(
        &mut self,
        mut step: Step,
        doc: &mut Document,
        review: &mut ReviewState,
    ) -> Result<(Applied, Pushed), DocError> {
        let mut applied = apply_step(&mut step, true, doc, review)?;
        let pushed = self.push(step);
        applied.doc_rev = self.doc_rev;
        applied.review_rev = self.review_rev;
        Ok((applied, pushed))
    }

    /// 最後の1回分を逆に適用する。文書と `ReviewState` の番号の付け替えまで行う。
    /// 適用できなかったとき（履歴と文書が食い違ったとき）は、何も変えずに `None`。
    pub fn undo(&mut self, doc: &mut Document, review: &mut ReviewState) -> Option<Applied> {
        let mut step = self.undo.pop()?;
        self.bytes = self.bytes.saturating_sub(step.bytes());
        match apply_step(&mut step, false, doc, review) {
            Ok(mut applied) => {
                self.doc_rev = step.before_doc_rev;
                self.review_rev = step.before_review_rev;
                applied.doc_rev = self.doc_rev;
                applied.review_rev = self.review_rev;
                self.bytes += step.bytes();
                self.redo.push(step);
                Some(applied)
            }
            Err(_) => {
                self.bytes += step.bytes();
                self.undo.push(step);
                None
            }
        }
    }

    /// 取り消した1回分をもう一度適用する。
    pub fn redo(&mut self, doc: &mut Document, review: &mut ReviewState) -> Option<Applied> {
        let mut step = self.redo.pop()?;
        self.bytes = self.bytes.saturating_sub(step.bytes());
        match apply_step(&mut step, true, doc, review) {
            Ok(mut applied) => {
                if step.changes_document() {
                    self.doc_rev = step.rev;
                }
                if step.changes_review() {
                    self.review_rev = step.rev;
                }
                applied.doc_rev = self.doc_rev;
                applied.review_rev = self.review_rev;
                self.bytes += step.bytes();
                self.undo.push(step);
                self.evict();
                Some(applied)
            }
            Err(_) => {
                self.bytes += step.bytes();
                self.redo.push(step);
                None
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// 持っている操作の大きさの合計（バイトの見積もり）。取り消しの列とやり直しの列の両方。
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// 取り消せる回数。
    pub fn len(&self) -> usize {
        self.undo.len()
    }

    pub fn is_empty(&self) -> bool {
        self.undo.is_empty()
    }

    /// 両方の列を空にする（モデルやレビューを開いたとき）。版の番号は戻さない。
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
    }

    /// いまの文書の版。未保存は「いまの版 ≠ 最後に書き出した版」で決める。
    pub fn doc_rev(&self) -> u64 {
        self.doc_rev
    }

    /// いまのレビューの版。
    pub fn review_rev(&self) -> u64 {
        self.review_rev
    }

    /// 次の取り消し（`back`）またはやり直しで文書から外れる部分木の根の鍵。取り消しでは
    /// 挿入が、やり直しでは削除が部品を外す。呼び出し側は、書きかけの注釈のように履歴に
    /// 入らない物が外れる部品を指していないかを、適用の前に確かめられる。
    pub fn next_removed_roots(&self, back: bool) -> Vec<NodeKey> {
        let step = if back {
            self.undo.last()
        } else {
            self.redo.last()
        };
        step.map_or_else(Vec::new, |step| {
            step.edits
                .iter()
                .filter_map(|edit| match (edit, back) {
                    (Edit::Insert { piece, .. }, true) | (Edit::Remove { piece, .. }, false) => {
                        piece.root_key()
                    }
                    _ => None,
                })
                .collect()
        })
    }

    /// 次に取り消す操作の名前（メニューの表示用）。
    pub fn undo_label(&self) -> Option<&'static str> {
        self.undo.last().map(|step| step.label)
    }

    /// 次にやり直す操作の名前。
    pub fn redo_label(&self) -> Option<&'static str> {
        self.redo.last().map(|step| step.label)
    }

    /// 上限を超えた分を古い順に捨てる。
    fn evict(&mut self) {
        let mut drop_count = 0;
        let mut total = self.bytes;
        while drop_count < self.undo.len()
            && (total > self.byte_limit || self.undo.len() - drop_count > self.step_limit)
        {
            total = total.saturating_sub(self.undo[drop_count].bytes());
            drop_count += 1;
        }
        if drop_count > 0 {
            self.undo.drain(..drop_count);
            self.bytes = total;
        }
    }
}

/// 1回分を前向き（実行・やり直し）か逆向き（取り消し）に適用する。途中で断られたら、
/// 適用した分を逆に戻して誤りを返す。
fn apply_step(
    step: &mut Step,
    forward: bool,
    doc: &mut Document,
    review: &mut ReviewState,
) -> Result<Applied, DocError> {
    let mut applied = Applied {
        select: step.select,
        label: step.label,
        ..Applied::default()
    };
    let count = step.edits.len();
    let order: Vec<usize> = if forward {
        (0..count).collect()
    } else {
        (0..count).rev().collect()
    };
    for (done, &index) in order.iter().enumerate() {
        if let Err(error) = apply_edit(&mut step.edits[index], forward, doc, review, &mut applied) {
            let mut scratch = Applied::default();
            for &back in order[..done].iter().rev() {
                let _ = apply_edit(&mut step.edits[back], !forward, doc, review, &mut scratch);
            }
            return Err(error);
        }
    }
    Ok(applied)
}

/// `Edit` 1つを適用する。`forward` が偽なら逆の操作を行う。
fn apply_edit(
    edit: &mut Edit,
    forward: bool,
    doc: &mut Document,
    review: &mut ReviewState,
    out: &mut Applied,
) -> Result<(), DocError> {
    match edit {
        Edit::SetLocal { key, before, after } => {
            let target = if forward { *after } else { *before };
            let range = doc.set_local(*key, target)?;
            out.touched.push(range);
        }
        Edit::Insert { at, piece } => {
            if forward {
                insert_piece(doc, review, *at, piece, out)?;
            } else {
                remove_piece(doc, review, at, piece, out)?;
            }
        }
        Edit::Remove { at, piece } => {
            if forward {
                remove_piece(doc, review, at, piece, out)?;
            } else {
                insert_piece(doc, review, *at, piece, out)?;
            }
        }
        Edit::SetVisible { key, before, after } => {
            let id = doc.id_of(*key).ok_or(DocError::UnknownKey(*key))?;
            let target = if forward { *after } else { *before };
            review.visibility.set(doc, id, target);
            out.visibility = true;
        }
        Edit::ShowAll { hidden } => {
            if forward {
                review.visibility.show_all();
            } else {
                let ids: Vec<NodeId> = hidden.iter().filter_map(|key| doc.id_of(*key)).collect();
                review.visibility.set_many(doc, ids, false);
            }
            out.visibility = true;
        }
        Edit::AddNote(note) => {
            if forward {
                let restored = keyed_to_note(doc, note)?;
                let index = review.annotations.len();
                if !review.insert_annotation_at(index, restored) {
                    return Err(DocError::IdInUse(note.note.id));
                }
            } else {
                remove_note(review, note.note.id)?;
                review.release_id(note.note.id);
            }
        }
        Edit::EditNote { id, before, after } => {
            let text = if forward {
                after.clone()
            } else {
                before.clone()
            };
            if !review.edit_annotation(*id, text) {
                return Err(DocError::UnknownNote(*id));
            }
        }
        Edit::RemoveNote { index, note } => {
            if forward {
                remove_note(review, note.note.id)?;
            } else {
                let restored = keyed_to_note(doc, note)?;
                if !review.insert_annotation_at(*index, restored) {
                    return Err(DocError::IdInUse(note.note.id));
                }
            }
        }
        Edit::AddView(view) => {
            if forward {
                let index = review.views.len();
                if !review.insert_view_at(index, view.clone()) {
                    return Err(DocError::IdInUse(view.id));
                }
            } else {
                if !review.remove_view(view.id) {
                    return Err(DocError::UnknownNote(view.id));
                }
                review.release_id(view.id);
            }
        }
        Edit::SetColors { key, before, rgb } => {
            let range = if forward {
                let current = doc.subtree_colors(*key)?;
                if current.len() != before.len() {
                    return Err(DocError::InvalidPiece);
                }
                let colors: Vec<[f32; 4]> = current
                    .iter()
                    .map(|color| [rgb[0], rgb[1], rgb[2], color[3]])
                    .collect();
                let range = doc.set_subtree_colors(*key, &colors)?;
                *before = current;
                range
            } else {
                doc.set_subtree_colors(*key, before)?
            };
            out.recolored.push(range);
        }
        Edit::SetLooks { key, before, after } => {
            let (from, to) = if forward {
                (before, after)
            } else {
                (after, before)
            };
            if doc.iter_subtree_looks(*key)?.ne(from.iter().copied()) {
                return Err(DocError::InvalidPiece);
            }
            let range = doc.set_subtree_looks(*key, to)?;
            if from.iter().zip(to.iter()).any(|(a, b)| a.tex != b.tex) {
                out.retextured = true;
            } else if from != to {
                out.recolored.push(range);
            }
        }
        Edit::RemoveView { index, view } => {
            if forward {
                if !review.remove_view(view.id) {
                    return Err(DocError::UnknownNote(view.id));
                }
            } else if !review.insert_view_at(*index, view.clone()) {
                return Err(DocError::IdInUse(view.id));
            }
        }
    }
    Ok(())
}

fn keyed_to_note(doc: &Document, note: &KeyedNote) -> Result<Annotation, DocError> {
    let node = doc
        .id_of(note.node)
        .ok_or(DocError::UnknownKey(note.node))?;
    Ok(Annotation {
        node,
        ..note.note.clone()
    })
}

fn remove_note(review: &mut ReviewState, id: u64) -> Result<(), DocError> {
    if review.remove_annotation(id) {
        Ok(())
    } else {
        Err(DocError::UnknownNote(id))
    }
}

/// 根の1件だけを残した部分木（鍵を覚えるため）。
fn stub_of(piece: &Subtree) -> Subtree {
    Subtree {
        nodes: piece.nodes.first().cloned().into_iter().collect(),
        ..Subtree::default()
    }
}

/// `piece` を `at` に入れ、表示の設定と注釈を戻す。成功したら `piece` は根の1件だけになる。
fn insert_piece(
    doc: &mut Document,
    review: &mut ReviewState,
    at: Place,
    piece: &mut Subtree,
    out: &mut Applied,
) -> Result<(), DocError> {
    let stub = stub_of(piece);
    let mut full = std::mem::take(piece);
    let count = full.nodes.len();
    let own = std::mem::take(&mut full.visible_own);
    let notes = std::mem::take(&mut full.annotations);
    let remap = match doc.try_insert_subtree(at, full) {
        Ok(remap) => remap,
        Err((error, mut back)) => {
            back.visible_own = own;
            back.annotations = notes;
            *piece = back;
            return Err(error);
        }
    };
    let Remap::Inserted { at: start, .. } = remap else {
        unreachable!("挿入は Inserted を返す");
    };
    review.apply_remap(&remap);
    if own.len() == count {
        review.visibility.splice(doc, NodeId(start), &own);
    } else {
        review
            .visibility
            .splice(doc, NodeId(start), &vec![true; count]);
    }
    for (index, mut note) in notes {
        note.node = NodeId(start + note.node.0);
        let restored = review.insert_annotation_at(index, note);
        debug_assert!(restored, "取り除いた注釈の番号は空いている");
    }
    out.remaps.push(remap);
    out.structure = true;
    out.visibility = true;
    *piece = stub;
    Ok(())
}

/// `piece` の根の鍵の部分木を取り除き、中身（部品の表・表示の設定・注釈）を `piece` に、
/// 戻す場所を `at` に入れる。
fn remove_piece(
    doc: &mut Document,
    review: &mut ReviewState,
    at: &mut Place,
    piece: &mut Subtree,
    out: &mut Applied,
) -> Result<(), DocError> {
    let key = piece.root_key().ok_or(DocError::InvalidPiece)?;
    let removed = doc.remove_subtree(key)?;
    let Remap::Removed { at: start, len } = removed.remap else {
        unreachable!("削除は Removed を返す");
    };
    let range = start..start + len;
    let own = review.visibility.cut(range.clone());
    let mut notes = review.take_annotations_in(range);
    for (_, note) in &mut notes {
        note.node = NodeId(note.node.0 - start);
    }
    review.apply_remap(&removed.remap);
    let mut full = removed.piece;
    full.visible_own = own;
    full.annotations = notes;
    *piece = full;
    *at = removed.place;
    out.remaps.push(removed.remap);
    out.structure = true;
    out.visibility = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::tests::{BLUE, GREEN, GREY, RED, sample};

    const PAINT: [f32; 3] = [0.25, 0.5, 0.75];

    fn recolor(doc: &Document, key: NodeKey) -> Step {
        let before = doc.subtree_colors(key).unwrap();
        Step::new(
            "色の変更",
            vec![Edit::SetColors {
                key,
                before,
                rgb: PAINT,
            }],
            Some(key),
        )
    }

    fn bits(colors: &[[f32; 4]]) -> Vec<[u32; 4]> {
        colors.iter().map(|c| c.map(f32::to_bits)).collect()
    }

    #[test]
    fn undo_restores_per_draw_colors() {
        let mut doc = sample();
        let mut review = ReviewState::new(&doc);
        let mut history = History::default();
        let step = recolor(&doc, NodeKey(0));
        let (applied, pushed) = history.execute(step, &mut doc, &mut review).unwrap();
        assert_eq!(pushed, Pushed::Kept);
        assert_eq!(applied.label, "色の変更");
        assert_eq!(
            doc.subtree_colors(NodeKey(0)).unwrap(),
            vec![
                [0.25, 0.5, 0.75, 1.0],
                [0.25, 0.5, 0.75, 0.5],
                [0.25, 0.5, 0.75, 1.0]
            ]
        );
        // 別の根の部品は変わらない。
        assert_eq!(doc.subtree_colors(NodeKey(3)).unwrap(), vec![GREY]);

        let undone = history.undo(&mut doc, &mut review).unwrap();
        assert_eq!(undone.recolored, vec![0..3]);
        assert_eq!(
            bits(&doc.subtree_colors(NodeKey(0)).unwrap()),
            bits(&[RED, GREEN, BLUE])
        );
        let redone = history.redo(&mut doc, &mut review).unwrap();
        assert_eq!(redone.recolored, vec![0..3]);
        assert_eq!(
            doc.subtree_colors(NodeKey(0)).unwrap()[1],
            [0.25, 0.5, 0.75, 0.5]
        );
        history.undo(&mut doc, &mut review).unwrap();
        assert_eq!(
            bits(&doc.subtree_colors(NodeKey(0)).unwrap()),
            bits(&[RED, GREEN, BLUE])
        );
    }

    #[test]
    fn set_colors_keeps_alpha() {
        let mut doc = sample();
        let mut review = ReviewState::new(&doc);
        let mut history = History::default();
        history
            .execute(recolor(&doc, NodeKey(1)), &mut doc, &mut review)
            .unwrap();
        let alphas: Vec<f32> = doc
            .subtree_colors(NodeKey(1))
            .unwrap()
            .iter()
            .map(|c| c[3])
            .collect();
        assert_eq!(alphas, vec![RED[3], GREEN[3]]);
    }

    #[test]
    fn recolor_bumps_document_rev_only() {
        let mut doc = sample();
        let mut review = ReviewState::new(&doc);
        let mut history = History::default();
        let edit = Edit::SetColors {
            key: NodeKey(1),
            before: vec![RED, GREEN],
            rgb: PAINT,
        };
        assert!(edit.changes_document());
        assert!(!edit.changes_review());
        assert_eq!(edit.bytes(), std::mem::size_of::<Edit>() as u64 + 2 * 16);
        let (applied, _) = history
            .execute(recolor(&doc, NodeKey(1)), &mut doc, &mut review)
            .unwrap();
        assert_eq!(applied.recolored, vec![1..2]);
        assert!(applied.touched.is_empty());
        assert!(applied.remaps.is_empty());
        assert!(!applied.structure);
        assert!(!applied.visibility);
        assert!(applied.doc_rev > 0);
        assert_eq!(applied.review_rev, 0);
        assert_eq!(history.doc_rev(), applied.doc_rev);
        assert_eq!(history.review_rev(), 0);
        let undone = history.undo(&mut doc, &mut review).unwrap();
        assert_eq!(undone.doc_rev, 0);
        assert_eq!(undone.review_rev, 0);
    }

    #[test]
    fn mismatched_before_changes_nothing() {
        let mut doc = sample();
        let mut review = ReviewState::new(&doc);
        let mut history = History::default();
        let step = Step::new(
            "色の変更",
            vec![Edit::SetColors {
                key: NodeKey(0),
                before: vec![RED],
                rgb: PAINT,
            }],
            None,
        );
        assert_eq!(
            history.execute(step, &mut doc, &mut review).unwrap_err(),
            DocError::InvalidPiece
        );
        assert!(history.is_empty());
        assert_eq!(
            doc.subtree_colors(NodeKey(0)).unwrap(),
            vec![RED, GREEN, BLUE]
        );
        let bad = Step::new(
            "色の変更",
            vec![Edit::SetColors {
                key: NodeKey(0),
                before: vec![RED, GREEN, BLUE],
                rgb: [f32::NAN, 0.0, 0.0],
            }],
            None,
        );
        assert_eq!(
            history.execute(bad, &mut doc, &mut review).unwrap_err(),
            DocError::NonFinite
        );
        assert_eq!(
            doc.subtree_colors(NodeKey(0)).unwrap(),
            vec![RED, GREEN, BLUE]
        );
    }

    #[test]
    fn oversized_recolor_runs_and_clears_history() {
        let mut doc = sample();
        let mut review = ReviewState::new(&doc);
        let mut history = History::with_limits(64, 10);
        let (_, pushed) = history
            .execute(recolor(&doc, NodeKey(0)), &mut doc, &mut review)
            .unwrap();
        assert_eq!(pushed, Pushed::ClearedTooLarge);
        assert!(!history.can_undo());
        assert_eq!(doc.subtree_colors(NodeKey(2)).unwrap()[0][0], PAINT[0]);
    }
}
