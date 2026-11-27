//! 道具・変形・数値入力・取り消し・図形の追加・複製・削除。
//!
//! 約束: 変形中（ハンドルのドラッグ・数値の欄のドラッグ）は文書を書き換えず、差分の行列だけを
//! `gpu_view` へ渡す。確定したときに初めて文書を書き換え、履歴に1回分を積む。Esc の取り消しは
//! 差分を消すだけ。構造の変更は `Applied.remaps` で選択・下書き・選択中の注釈を付け替える。
//! 確定した変形は、動かした範囲だけを `gpu_view` が部分更新する（部品の数と表示は変わらない）。

use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;

use glam::{Affine3A, Vec2, Vec3};
use model_reviewer_core::doc::NodeKey;
use model_reviewer_core::gizmo::{
    self, Drag, GizmoFrame, Handle, Readout, Tool, Trs, apply_world_delta, compose,
};
use model_reviewer_core::history::{Applied, Edit, Pushed, Step};
use model_reviewer_core::math::Aabb;
use model_reviewer_core::scene::{Draw, NodeId, SceneRead};
use model_reviewer_core::shapes::{self, SHAPE_COLOR, ShapeKind};

use super::Session;
use crate::ui::status::grouped;

/// ハンドルの画面上の長さ（ポイント。ux.md の約90ポイント）。
pub const GIZMO_SIZE: f32 = 90.0;

/// 履歴の上限で、それより前へ戻れなくなったときの知らせ。
const CLEARED_TOO_LARGE: &str = "操作が大きいため、これより前へは戻せません";
const NOT_FINITE: &str = "変形の結果が有限でないため、確定しませんでした";
const ZERO_SIZE: &str = "大きさが0になるため、確定しませんでした";
const NO_MODEL: &str = "［新規作成］（Cmd+N）か、モデルを開くと図形を置けます";

/// 変形の出どころ。
#[derive(Clone, Debug, PartialEq)]
pub enum TransformSource {
    /// ハンドルのドラッグ。掴んだ時点の値を持つ。
    Gizmo(Drag),
    /// 数値の欄のドラッグ。
    Field,
    /// キーで向きを固定し、数値を打っている途中（要件11）。
    Keys(KeyEntry),
}

/// キーで打っている途中の量。`text` は「-12.5」など。空もある。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEntry {
    pub handle: Handle,
    pub text: String,
}

/// 変形中の状態。文書はまだ書き換えていない。
#[derive(Clone, Debug, PartialEq)]
pub struct Transform {
    /// 動かす部品（選択の根）の鍵。
    pub key: NodeKey,
    /// 動かす範囲（選択と子孫の `NodeId` の連続範囲）。
    pub nodes: Range<u32>,
    pub source: TransformSource,
    /// ワールド空間での差分の行列 D。
    pub delta: Affine3A,
    pub readout: Option<Readout>,
}

impl Transform {
    /// 掴んでいるハンドル（キーで向きを固定しているときは、その向きのハンドル）。数値の欄なら `None`。
    pub fn handle(&self) -> Option<Handle> {
        match &self.source {
            TransformSource::Gizmo(drag) => Some(drag.handle()),
            TransformSource::Field => None,
            TransformSource::Keys(entry) => Some(entry.handle),
        }
    }
}

/// 形を変える道具か（読み込み中は選べない）。
pub fn tool_edits(tool: Tool) -> bool {
    matches!(
        tool,
        Tool::Move | Tool::Rotate | Tool::Scale | Tool::Shape | Tool::Annotate
    )
}

/// ハンドルを出す道具か。
pub fn tool_has_gizmo(tool: Tool) -> bool {
    matches!(tool, Tool::Move | Tool::Rotate | Tool::Scale)
}

/// ハンドルを置く枠。`viewport` と `size` は同じ単位（画面のポイント、または物理画素）。
/// 道具にハンドルが無い、選択が無い、部品の位置が定まらないときは `None`。
/// 中心は、選択した部品と子孫の見えている範囲の中心（見えていなければ全体、それも無ければ原点）。
pub fn gizmo_frame(session: &Session, viewport: [f32; 2], size: f32) -> Option<GizmoFrame> {
    if !tool_has_gizmo(session.tool) {
        return None;
    }
    let id = session.selection?;
    let doc = session.doc.as_ref()?;
    let node = doc.node(id)?;
    let range = doc.subtree(id);
    let mut visible = Aabb::empty();
    if let Some(review) = session.review.as_ref() {
        for index in range.clone() {
            let child = NodeId(index);
            if review.visibility.effective(child) {
                visible = visible.union(doc.world_aabb(child));
            }
        }
    }
    let pivot = if !visible.is_empty() {
        visible.center()
    } else {
        let all = doc.subtree_bounds(id);
        if all.is_empty() {
            node.world.translation.into()
        } else {
            all.center()
        }
    };
    let axes = if session.tool == Tool::Scale {
        let m = node.world.matrix3;
        [
            unit_or(m.x_axis.into(), Vec3::X),
            unit_or(m.y_axis.into(), Vec3::Y),
            unit_or(m.z_axis.into(), Vec3::Z),
        ]
    } else {
        [Vec3::X, Vec3::Y, Vec3::Z]
    };
    let frame = GizmoFrame {
        pivot,
        axes,
        camera: session.camera,
        viewport_px: viewport,
        size_px: size,
    };
    (pivot.is_finite() && viewport[0] > 0.0 && viewport[1] > 0.0).then_some(frame)
}

/// 変形中なら、見せる枠を差分の分だけ動かす（ハンドルが部品と一緒に動く）。
pub fn moved_frame(mut frame: GizmoFrame, transform: Option<&Transform>) -> GizmoFrame {
    if let Some(transform) = transform {
        let pivot = transform.delta.transform_point3(frame.pivot);
        if pivot.is_finite() {
            frame.pivot = pivot;
        }
        if matches!(
            transform.handle(),
            Some(Handle::ScaleX | Handle::ScaleY | Handle::ScaleZ | Handle::ScaleAll)
        ) {
            for axis in &mut frame.axes {
                *axis = unit_or(transform.delta.transform_vector3(*axis), *axis);
            }
        }
    }
    frame
}

fn unit_or(value: Vec3, fallback: Vec3) -> Vec3 {
    if value.is_finite() && value.length_squared() > 1.0e-20 {
        value.normalize()
    } else {
        fallback
    }
}

/// `Session` が持つ物理画素の大きさで、ハンドルの枠を作る（`BeginTransform` の計算用）。
fn session_frame(session: &Session) -> Option<GizmoFrame> {
    let [w, h] = session.viewport_px;
    let ppp = if session.pixels_per_point.is_finite() && session.pixels_per_point > 0.0 {
        session.pixels_per_point
    } else {
        1.0
    };
    gizmo_frame(session, [w as f32, h as f32], GIZMO_SIZE * ppp)
}

pub(super) fn set_tool(session: &mut Session, tool: Tool) {
    if tool != Tool::Select && session.model.is_none() {
        return;
    }
    if tool_edits(tool) && session.block_edit_if_locked() {
        return;
    }
    if session.tool == tool {
        return;
    }
    cancel_transform(session);
    let was_annotate = session.tool == Tool::Annotate;
    session.tool = tool;
    session.annotate_mode = tool == Tool::Annotate;
    if was_annotate && tool != Tool::Annotate {
        super::notes::cancel(session);
    }
    // ハンドルの出し入れは egui の描画だけ。3Dは描き直さない。
}

pub(super) fn begin_transform(session: &mut Session, handle: Handle, ndc: Vec2) {
    if session.block_edit_if_locked() {
        return;
    }
    cancel_transform(session);
    let Some(id) = session.selection else {
        return;
    };
    let Some(frame) = session_frame(session) else {
        return;
    };
    let Some(doc) = session.doc.as_ref() else {
        return;
    };
    let Some(key) = doc.key(id) else {
        return;
    };
    let nodes = doc.subtree(id);
    let Some(drag) = Drag::begin(handle, &frame, ndc) else {
        session.status_text = "この向きからは動かせません。視点を変えてください".to_owned();
        return;
    };
    session.transform = Some(Transform {
        key,
        nodes,
        source: TransformSource::Gizmo(drag),
        delta: Affine3A::IDENTITY,
        readout: None,
    });
    session.scene_dirty.drag = true;
}

pub(super) fn drag_transform(session: &mut Session, ndc: Vec2) {
    let Some(transform) = session.transform.as_mut() else {
        return;
    };
    let TransformSource::Gizmo(drag) = &transform.source else {
        return;
    };
    // 値が定まらないとき（視線と軸がほぼ平行）は前の値を保つ。
    let Some(state) = drag.update(ndc) else {
        return;
    };
    if !state.world_delta.is_finite() {
        return;
    }
    if transform.delta != state.world_delta {
        transform.delta = state.world_delta;
        session.scene_dirty.drag = true;
    }
    transform.readout = Some(state.readout);
    session.status_text = readout_status(state.readout);
}

fn readout_status(readout: Readout) -> String {
    let text = crate::ui::gizmo_view::readout_text(readout);
    match readout {
        Readout::Distance(_) => format!("移動 {text}"),
        Readout::AngleDeg(_) => format!("回転 {text}"),
        Readout::Factor(_) => format!("拡大 {text}"),
    }
}

pub(super) fn commit_transform(session: &mut Session) {
    // キーで打った量が確定できない（拡大の倍率が 0 以下など）なら、理由を出して入力を続ける
    // （要件11.10）。差分の行列は前に読めた量のままなので、そのまま入れてはいけない。
    if let Some(Transform {
        source: TransformSource::Keys(entry),
        ..
    }) = session.transform.as_ref()
        && let Some(reason) = super::keymove::commit_refusal(entry)
    {
        session.status_text = super::keymove::status_with(session, entry, reason);
        return;
    }
    let Some(transform) = session.transform.take() else {
        return;
    };
    session.scene_dirty.drag = true;
    if transform.delta == Affine3A::IDENTITY {
        return;
    }
    let started = Instant::now();
    let Some(doc) = session.doc.as_ref() else {
        return;
    };
    let Some(id) = doc.id_of(transform.key) else {
        return;
    };
    let Some(node) = doc.node(id) else {
        return;
    };
    let parent_world = node
        .parent
        .and_then(|parent| doc.node(parent))
        .map_or(Affine3A::IDENTITY, |parent| parent.world);
    let before = node.local;
    let Some(after) = apply_world_delta(&parent_world, &node.world, &transform.delta) else {
        session.status_text = NOT_FINITE.to_owned();
        return;
    };
    let _ = commit_local_matrix(session, transform.key, before, after, label_of(&transform));
    session.metrics.edit_commit = Some(started.elapsed());
}

fn label_of(transform: &Transform) -> &'static str {
    match transform.handle() {
        Some(Handle::MoveX | Handle::MoveY | Handle::MoveZ) => "移動",
        Some(Handle::RotX | Handle::RotY | Handle::RotZ) => "回転",
        Some(Handle::ScaleX | Handle::ScaleY | Handle::ScaleZ | Handle::ScaleAll) => "拡大",
        None => "数値の変更",
    }
}

/// 確定の共通部。有限で大きさのある行列だけを、履歴を通して文書へ入れる。積んだら真。
pub(super) fn commit_local_matrix(
    session: &mut Session,
    key: NodeKey,
    before: Affine3A,
    after: Affine3A,
    label: &'static str,
) -> bool {
    if !after.is_finite() {
        session.status_text = NOT_FINITE.to_owned();
        return false;
    }
    let det = after.matrix3.determinant();
    if !det.is_finite() || det.abs() < 1.0e-30 {
        session.status_text = ZERO_SIZE.to_owned();
        return false;
    }
    if after == before {
        return false;
    }
    let step = Step::new(
        label,
        vec![Edit::SetLocal { key, before, after }],
        Some(key),
    );
    if execute(session, step).is_some() {
        session.status_text = format!("{label}を確定しました");
        true
    } else {
        false
    }
}

pub(super) fn cancel_transform(session: &mut Session) {
    if session.transform.take().is_some() {
        session.scene_dirty.drag = true;
    }
}

pub(super) fn preview_local(session: &mut Session, trs: Trs) {
    if session.block_edit_if_locked() {
        return;
    }
    let Some(id) = session.selection else {
        return;
    };
    let Some(doc) = session.doc.as_ref() else {
        return;
    };
    let Some(node) = doc.node(id) else {
        return;
    };
    let after = compose(&trs);
    if !after.is_finite() {
        return;
    }
    let parent_world = node
        .parent
        .and_then(|parent| doc.node(parent))
        .map_or(Affine3A::IDENTITY, |parent| parent.world);
    // D × world = parent × after なので D = parent × after × world⁻¹。
    let det = node.world.matrix3.determinant();
    if !det.is_finite() || det.abs() < 1.0e-30 {
        return;
    }
    let delta = parent_world * after * node.world.inverse();
    if !delta.is_finite() {
        return;
    }
    let key = node.key;
    let nodes = doc.subtree(id);
    if matches!(
        session.transform.as_ref().map(|t| &t.source),
        Some(TransformSource::Gizmo(_))
    ) {
        return;
    }
    session.transform = Some(Transform {
        key,
        nodes,
        source: TransformSource::Field,
        delta,
        readout: None,
    });
    session.scene_dirty.drag = true;
}

pub(super) fn commit_local(session: &mut Session, trs: Trs) {
    let had_preview = session.transform.take().is_some();
    if had_preview {
        session.scene_dirty.drag = true;
    }
    if session.block_edit_if_locked() {
        return;
    }
    let started = Instant::now();
    let Some(id) = session.selection else {
        return;
    };
    let Some(doc) = session.doc.as_ref() else {
        return;
    };
    let Some(node) = doc.node(id) else {
        return;
    };
    if !trs.translation.is_finite() || !trs.rotation.is_finite() || !trs.scale.is_finite() {
        session.status_text = NOT_FINITE.to_owned();
        return;
    }
    let before = node.local;
    let key = node.key;
    // 欄の値が今の行列と同じなら何もしない（分けて合成し直した誤差で履歴を増やさない）。
    if gizmo::decompose(&before).is_some_and(|now| same_trs(&now, &trs)) {
        return;
    }
    let _ = commit_local_matrix(session, key, before, compose(&trs), "数値の変更");
    session.metrics.edit_commit = Some(started.elapsed());
}

fn same_trs(a: &Trs, b: &Trs) -> bool {
    let close =
        |x: Vec3, y: Vec3| (x - y).abs().max_element() <= 1.0e-6 * x.abs().max_element().max(1.0);
    close(a.translation, b.translation)
        && close(a.scale, b.scale)
        && a.rotation.dot(b.rotation).abs() >= 1.0 - 1.0e-7
}

pub(super) fn undo(session: &mut Session) {
    step_history(session, true);
}

pub(super) fn redo(session: &mut Session) {
    step_history(session, false);
}

fn step_history(session: &mut Session, back: bool) {
    if session.block_edit_if_locked() {
        return;
    }
    cancel_transform(session);
    // 書きかけの注釈は履歴に入らないので、それが付いた部品を取り消し・やり直しで外すと
    // 戻せずに消える。削除と同じく断る（要件9.6）。
    if let (Some(draft), Some(doc)) = (session.draft.as_ref(), session.doc.as_ref()) {
        let removes_draft = session
            .history
            .next_removed_roots(back)
            .into_iter()
            .filter_map(|key| doc.id_of(key))
            .any(|root| doc.subtree(root).contains(&draft.node.0));
        if removes_draft {
            session.status_text = format!(
                "書きかけの注釈がある部品が消えるため、{}ませんでした。先に注釈を登録するか取り消してください",
                if back { "戻せ" } else { "やり直せ" }
            );
            return;
        }
    }
    let started = Instant::now();
    let (Some(doc), Some(review)) = (session.doc.as_mut(), session.review.as_mut()) else {
        return;
    };
    let applied = if back {
        session.history.undo(doc, review)
    } else {
        session.history.redo(doc, review)
    };
    let Some(applied) = applied else {
        let can = if back {
            session.history.can_undo()
        } else {
            session.history.can_redo()
        };
        session.status_text = if can {
            "この操作は戻せませんでした".to_owned()
        } else if back {
            "戻せる操作はありません".to_owned()
        } else {
            "やり直せる操作はありません".to_owned()
        };
        return;
    };
    let label = applied.label;
    after_applied(session, &applied, true);
    session.metrics.undo_redo = Some(started.elapsed());
    session.status_text = if back {
        format!("戻しました: {label}")
    } else {
        format!("やり直しました: {label}")
    };
}

/// 1回分を文書とレビューに適用してから履歴に積む。断られたら `None`（何も変わらない）。
pub(super) fn execute(session: &mut Session, step: Step) -> Option<Applied> {
    let (Some(doc), Some(review)) = (session.doc.as_mut(), session.review.as_mut()) else {
        return None;
    };
    match session.history.execute(step, doc, review) {
        Ok((applied, pushed)) => {
            after_applied(session, &applied, false);
            note_pushed(session, pushed);
            Some(applied)
        }
        Err(err) => {
            log::warn!("edit rejected: {err:?}");
            session.status_text = "この操作はできませんでした".to_owned();
            None
        }
    }
}

/// 自分で適用した操作を積む（注釈と視点の追加）。
pub(super) fn record(session: &mut Session, step: Step) {
    let pushed = session.history.push(step);
    note_pushed(session, pushed);
    session.metrics.history_bytes = Some(session.history.bytes());
    session.metrics.history_len = Some(session.history.len() as u64);
}

fn note_pushed(session: &mut Session, pushed: Pushed) {
    if pushed == Pushed::ClearedTooLarge {
        session.status_text = CLEARED_TOO_LARGE.to_owned();
    }
}

/// 適用の後始末。番号の付け替え、選択、GPUの更新の種類、未保存の印。
fn after_applied(session: &mut Session, applied: &Applied, from_history: bool) {
    let selection_before = session.selection;
    let draft_before = session.draft.as_ref().map(|draft| draft.node);
    for remap in &applied.remaps {
        session.selection = session.selection.and_then(|id| remap.map(id));
        if let Some(draft) = session.draft.as_mut() {
            match remap.map(draft.node) {
                Some(node) => draft.node = node,
                None => {
                    session.draft = None;
                    session.draft_pending = false;
                }
            }
        }
    }
    if !applied.remaps.is_empty() && session.transform.take().is_some() {
        // 構造が変わったら、変形の途中（古い番号の範囲）は続けられない。
        session.scene_dirty.drag = true;
    }
    let count = session.doc.as_ref().map_or(0, |doc| doc.node_count());
    if session.selection.is_some_and(|id| id.0 as usize >= count) {
        session.selection = None;
    }
    if from_history
        && let Some(key) = applied.select
        && let Some(id) = session.doc.as_ref().and_then(|doc| doc.id_of(key))
        && session.selection != Some(id)
    {
        session.selection = Some(id);
        session.selected_annotation = None;
    }
    // 選択中の注釈が消えた、または選択と別の部品を指すなら外す。
    if let Some(id) = session.selected_annotation {
        let node = session.review.as_ref().and_then(|review| {
            review
                .annotations
                .iter()
                .find(|note| note.id == id)
                .map(|note| note.node)
        });
        if node.is_none() || (node != session.selection && session.selection.is_some()) {
            session.selected_annotation = None;
        }
    }
    if applied.structure {
        // 消えた部品を指す計測は消す。三角形の数は文書から数え直す。
        if let Some(doc) = session.doc.as_ref() {
            session.measurement.prune(doc);
        }
        session.recount_triangles();
    }
    if applied.structure || applied.visibility {
        session.scene_dirty.instances = true;
        session.pending_patches.clear();
    } else if !applied.touched.is_empty() {
        session
            .pending_patches
            .extend(applied.touched.iter().cloned());
        // 描き直しの合図。インスタンスは gpu_view が範囲だけ書き直す。
        session.scene_dirty.drag = true;
    }
    if from_history {
        // 本文の編集途中の一時メモリは、取り消しの後の本文と食い違う。
        super::notes::clear_body_edits(session);
    }
    if session.selection != selection_before
        || session.draft.as_ref().map(|draft| draft.node) != draft_before
    {
        session.scene_dirty.selection = true;
    }
    session.metrics.history_bytes = Some(session.history.bytes());
    session.metrics.history_len = Some(session.history.len() as u64);
}

/// 図形を視点の注視点に置き、選択して移動の道具へ切り替える（要件9.1・9.2）。形状は
/// `Document::add_shape_primitive` で足し、取り消しても残す（モデルを閉じると消える）。
pub(super) fn add_shape(session: &mut Session, kind: ShapeKind) {
    if session.block_edit_if_locked() {
        return;
    }
    let target = session.camera.target;
    let Some(doc) = session.doc.as_mut() else {
        session.status_text = NO_MODEL.to_owned();
        return;
    };
    if session.review.is_none() {
        session.status_text = NO_MODEL.to_owned();
        return;
    }
    let started = Instant::now();
    let size = shapes::size_for(doc.bounds().radius());
    // 作り方（種類と大きさ）を覚えておくと、右のパネルの「寸法」で直せる（要件9.1）。
    let prim = doc.add_shape_primitive(
        Arc::new(shapes::build(kind, size)),
        shapes::ShapeSpec { kind, size },
    );
    let local = Affine3A::from_translation(if target.is_finite() {
        target
    } else {
        Vec3::ZERO
    });
    let draw = Draw {
        prim,
        base_color: SHAPE_COLOR,
    };
    let Ok((place, piece)) = doc.shape_piece(shapes::name_ja(kind), local, draw) else {
        session.status_text = "図形を置けませんでした".to_owned();
        return;
    };
    let Some(key) = piece.root_key() else {
        return;
    };
    let step = Step::new(
        "図形の追加",
        vec![Edit::Insert { at: place, piece }],
        Some(key),
    );
    if execute(session, step).is_none() {
        return;
    }
    // 足した形状の当たり判定の索引（索引がまだ無ければ、届いたときに足す）。
    if let (Some(index), Some(doc)) = (session.pick_index.as_mut(), session.doc.as_ref()) {
        index.extend(doc);
    }
    select_key(session, key);
    set_tool(session, Tool::Move);
    session.metrics.edit_commit = Some(started.elapsed());
    session.status_text = format!(
        "{}を置きました（1辺 {}）",
        shapes::name_ja(kind),
        crate::ui::status::spacing_text(size)
    );
}

/// 選択した部品と子孫を、同じ親の下（元の直後）に複製し、複製を選ぶ（要件9.3）。
/// 形状は共有し、名前は「<元の名前> コピー」。表示の設定は全部「表示」で入る。
pub(super) fn duplicate_selected(session: &mut Session) {
    if session.block_edit_if_locked() {
        return;
    }
    let Some(id) = session.selection else {
        session.status_text = "複製する部品を選んでください".to_owned();
        return;
    };
    let started = Instant::now();
    let Some(doc) = session.doc.as_mut() else {
        return;
    };
    let Some(key) = doc.key(id) else {
        return;
    };
    let Ok((place, piece)) = doc.duplicate_subtree(key) else {
        session.status_text = "複製できませんでした".to_owned();
        return;
    };
    let Some(copy) = piece.root_key() else {
        return;
    };
    let count = piece.nodes.len();
    let step = Step::new("複製", vec![Edit::Insert { at: place, piece }], Some(copy));
    if execute(session, step).is_none() {
        return;
    }
    select_key(session, copy);
    session.metrics.edit_commit = Some(started.elapsed());
    session.status_text = format!("部品 {} 個を複製しました", grouped(count as u64));
}

/// 選択した部品と子孫を文書から除き、付いていた注釈も一緒に除く（要件9.4）。書きかけの
/// 注釈の下書きがある部品は削除しない（要件9.6）。
pub(super) fn delete_selected(session: &mut Session) {
    if session.block_edit_if_locked() {
        return;
    }
    let Some(id) = session.selection else {
        session.status_text = "削除する部品を選んでください".to_owned();
        return;
    };
    let Some(doc) = session.doc.as_ref() else {
        return;
    };
    let range = doc.subtree(id);
    if session
        .draft
        .as_ref()
        .is_some_and(|draft| range.contains(&draft.node.0))
    {
        session.status_text =
            "書きかけの注釈があるため削除できません。先に注釈を登録するか取り消してください"
                .to_owned();
        return;
    }
    let Some(key) = doc.key(id) else {
        return;
    };
    let Some(request) = Edit::remove_request(doc, key) else {
        return;
    };
    let notes = session.review.as_ref().map_or(0, |review| {
        review
            .annotations
            .iter()
            .filter(|note| range.contains(&note.node.0))
            .count()
    });
    let count = range.len();
    let started = Instant::now();
    let step = Step::new("削除", vec![request], Some(key));
    if execute(session, step).is_none() {
        return;
    }
    session.metrics.edit_commit = Some(started.elapsed());
    session.status_text = if notes > 0 {
        format!(
            "部品 {} 個と注釈 {} 件を削除しました",
            grouped(count as u64),
            grouped(notes as u64)
        )
    } else {
        format!("部品 {} 個を削除しました", grouped(count as u64))
    };
}

/// 鍵の部品を選ぶ（構造を変えた直後の選択）。
fn select_key(session: &mut Session, key: NodeKey) {
    let Some(id) = session.doc.as_ref().and_then(|doc| doc.id_of(key)) else {
        return;
    };
    if session.selection != Some(id) {
        session.selection = Some(id);
        session.selected_annotation = None;
        session.scene_dirty.selection = true;
    }
}
