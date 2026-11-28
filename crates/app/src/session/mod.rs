//! 画面が読む状態。書き換えは `apply` だけが行う。

use std::cell::Cell;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use model_reviewer_core::camera::OrbitCamera;
use model_reviewer_core::doc::Document;
use model_reviewer_core::gizmo::Tool;
use model_reviewer_core::history::History;
use model_reviewer_core::identity::{ModelIdentity, ModelRef};
use model_reviewer_core::measure::Measurement;
use model_reviewer_core::pick::PickIndex;
use model_reviewer_core::review::ReviewState;
use model_reviewer_core::review_file::ReviewFile;
use model_reviewer_core::scene::{Model, NodeId, SceneRead};
use model_reviewer_core::section::{Axis, SectionPlane};
use model_reviewer_core::snap::{SnapSettings, SnapTarget};
use model_reviewer_core::units::{UnitLabel, Units, UpAxis};
use model_reviewer_render::DisplayMode;

use crate::command::{Command, DialogButton, DialogRequest, PanelKind, SceneDirty};
use crate::exporter::protection::FileHistory;
use crate::exporter::{ExportHandle, ExportHooks, ExportStage};
use crate::keymap::store::Store;
use crate::keymap::{KeyLabels, Keymap};
use crate::loader::{self, LoadEvent, LoadHandle, LoadHooks, LoadOptions, LoadPhase, LoadedModel};
use crate::metrics::Metrics;
use crate::ui::ids;

mod camera;
pub mod color;
mod dims;
mod edit;
mod export;
pub mod keymove;
mod keys;
mod measure;
mod notes;
mod open;
mod parts;
mod persist;
mod pick;
mod section;
mod snap;
pub mod texture;
mod view;

pub use color::{ColorState, ColorView, selection_color};
pub use dims::{DimsView, ShapeDims, selection_dims};
pub use edit::{
    GIZMO_SIZE, KeyEntry, Transform, TransformSource, gizmo_frame, moved_frame, tool_edits,
    tool_has_gizmo,
};
pub use export::{NEW_EXPORT_NOTE, export_scene, note_ja, nothing_to_export, stl_export_note};
pub use keys::{KeyConflict, KeysState};
pub use measure::{delta as measure_delta, distance as measure_distance};
pub use notes::Draft;
pub use open::TextureProgress;
pub use pick::ray_hit;
pub use section::section_bounds;
pub use snap::{SNAP_RADIUS_POINTS, find as find_snap, rest_after as snap_rest_after};
pub use texture::{TextureView, texture_view};

#[derive(Clone, Debug)]
pub(super) struct PendingRestore {
    pub file: ReviewFile,
    pub remaining: Vec<PathBuf>,
    pub first_mismatch: Option<ModelIdentity>,
}

/// 開いているモデルの元の形式。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceKind {
    #[default]
    Glb,
    /// STL を開いたときに仮定した単位と上の向き。
    Stl { unit: UnitLabel, up: UpAxis },
    /// 新規作成した文書（まだ書き出していない）。書き出すと `Glb` へ移る。
    New,
}

/// 新規の文書の初期視点の距離。格子の間隔が 0.1 になる（要件1.6）。
pub const NEW_DISTANCE: f32 = 4.0;

/// 部品の無い新規の文書を書き出そうとしたときの文（要件2.2）。
pub const NOTHING_TO_EXPORT: &str = "書き出す部品がありません。図形を置いてから書き出してください";

/// 新規の文書でレビューを保存しようとしたときの文（要件2.6）。
pub const REVIEW_NEEDS_EXPORT: &str = "先に GLB を書き出すと、レビューを保存できます";

#[derive(Clone, Debug, PartialEq)]
enum Pending {
    OpenModel(PathBuf, LoadOptions, Option<ModelIdentity>),
    LoadReview(PathBuf),
    Quit,
    /// 新規作成（未保存の確認の後に続ける）。
    New,
}

pub struct Session {
    /// 読み込んだモデル（形状と、読み込んだ時点の部品の表の控え）。GPUへ送った形状の照合と、
    /// 読み込みの統計・警告にだけ使う。部品を読むときは `doc` を見る。
    pub model: Option<Arc<Model>>,
    /// 画面と計算が読む部品の表。モデルの表示が始まるときに `model` から作る。
    pub doc: Option<Document>,
    pub model_ref: Option<ModelRef>,
    pub review: Option<ReviewState>,
    pub camera: OrbitCamera,
    /// 表示（面・面と線・線・透かし）。作業の間だけの設定で、保存しない。
    pub display: DisplayMode,
    /// 床の格子と軸を出すか。既定は出す。
    pub grid_visible: bool,
    pub textures_shown: bool,
    pub textures: TextureProgress,
    paste_job: Option<texture::PasteJob>,
    drop_probe: Option<open::DropProbe>,
    pub(crate) paste_error: Option<String>,
    texture_cache: Cell<Option<texture::Selection>>,
    /// 書き出しで新しいファイル番号を得た、スキン・モーフつきの複製。
    exported_no_box: std::collections::HashSet<u32>,
    export_colors: export::ExportColors,
    /// 開いた後に、書き出したファイルへ入った貼り付け画像。外しても次のファイルに残る。
    exported_pasted: std::collections::HashSet<model_reviewer_core::texture::ImageHash>,
    pub(crate) texture_options: loader::TextureOptions,
    /// 装置が線の表示（`POLYGON_MODE_LINE`）を持つか。GPUが無いときは偽。
    pub lines_supported: bool,
    pub selection: Option<NodeId>,
    color_cache: Cell<Option<ColorView>>,
    /// 2点の計測。保存せず、履歴にも未保存の印にも入れない。部品を消すと消える。
    pub measurement: Measurement,
    /// ファイルの単位（前提）と表示の単位。作業の間だけの設定で、保存しない。
    /// GLB を開くと `{ source: M, display: None }`（第2段階と同じ数値と文字）に戻る。
    pub units: Units,
    /// 開いているモデルの元の形式。
    pub source_kind: SourceKind,
    /// 吸着の設定。作業の間だけの設定で、保存しない。`radius_px` はポイントで持ち、探索のときに
    /// `pixels_per_point` を掛けて物理画素にする。
    pub snap: SnapSettings,
    /// 遅い探索の後、ポインタの移動に合わせた探索を休む期限。
    snap_rest_until: Option<Instant>,
    /// ポインタの下の吸着の候補（計測の道具のとき）。クリックはこれを使わない。
    pub snap_preview: Option<SnapTarget>,
    /// 断面。履歴・未保存の印・レビューJSONに入れない。
    pub section: Option<SectionPlane>,
    /// 断面を切っている間の控え（もう一度入れると、この軸と位置を使う）。
    pub section_memory: SectionPlane,
    /// 控えが一度でも置かれたか（偽なら、入れたときに Z 軸の中央から始める）。
    section_remembered: bool,
    /// 文書の部品が置く三角形の数（追加・複製・削除のたびに数え直す）。
    placed_triangles: u64,
    /// いまの道具。`Annotate` のときは `annotate_mode` も真（第1段階の口として残す）。
    pub tool: Tool,
    pub annotate_mode: bool,
    /// 取り消しとやり直し。モデルやレビューを開くと空にする。
    pub history: History,
    /// 変形中（ハンドルか数値の欄のドラッグ）。文書はまだ書き換えていない。
    pub transform: Option<Transform>,
    /// 行列または基本色が変わった範囲。次のフレームでインスタンスを部分更新する。
    pending_patches: Vec<Range<u32>>,
    /// 最後に書き出した時点の文書の版。いまの版と違えば「形の編集 未書き出し」。
    saved_doc_rev: u64,
    /// 最後に保存した時点のレビューの版。`None` は書き出し直後など、今のファイル向けに未保存。
    saved_review_rev: Option<u64>,
    /// 画面のポイントあたりの物理画素。ハンドルの大きさを物理画素で求めるのに使う。
    pub pixels_per_point: f32,
    pub draft: Option<Draft>,
    pub pick_index: Option<PickIndex>,
    pub load: LoadPhase,
    pub dialog: Option<DialogRequest>,
    /// 下書き欄に、まだ登録していない文字がある。
    draft_pending: bool,
    /// 注釈本文を編集中で、まだ確定していない。
    body_pending: bool,
    pub scene_dirty: SceneDirty,
    pub metrics: Metrics,
    pub panel_parts: bool,
    pub panel_notes: bool,
    pub panel_details: bool,
    pending: Option<Pending>,
    pub(super) restore: Option<PendingRestore>,
    pub viewport_px: [u32; 2],
    pub selected_annotation: Option<u64>,
    pub status_text: String,
    /// ドロップが複数だったときなど、工程名とは別に残す知らせ。
    pub notice: String,
    /// いま開こうとしているファイル名。表示中のモデルとは別に出す。
    pub loading_file_name: Option<String>,
    quit_after_confirm: bool,
    generation: u64,
    shown_generation: u64,
    /// モデルまたはレビューが切り替わるたびに進める世代。編集途中の一時メモリの区別に使う。
    review_generation: u64,
    /// 候補の読み込みは、転送の成功時に表示中の受信口と入れ替える。
    loader: Option<LoadHandle>,
    shown_loader: Option<LoadHandle>,
    load_meta: Option<LoadMeta>,
    incoming: Option<Incoming>,
    staged_pick: Option<StagedPick>,
    load_hooks: LoadHooks,
    /// 実行中のGLB書き出し。
    pub(super) export_job: Option<ExportJob>,
    pub(super) export_hooks: ExportHooks,
    pub(super) paste_hooks: texture::PasteHooks,
    /// 直近の書き出しの失敗理由（自動操作の `wait_idle` 用）。次の書き出し・読み込みで消える。
    pub(super) export_error: Option<String>,
    /// 直近の状態文が書き出し失敗のものか（状態欄を赤字にするため）。
    pub(super) export_failed: bool,
    /// この作業で「開く」から開いたファイル。書き出し先にできない。
    pub(super) opened_files: Vec<PathBuf>,
    file_history: FileHistory,
    /// いまのモデルを読み込んだ後に1回以上書き出したか（`session.model` が今の土台と異なる）。
    pub(super) exported_since_load: bool,
    pub(super) egui_ctx: Option<egui::Context>,
    /// GPUがあるときは転送完了まで `model` を入れ替えない。
    gpu_upload: bool,
    cancel_upload: bool,
    /// 未保存の確認を済ませた操作の途中。もう一度確認を出さない。
    discard_bypass: bool,
    open_started: Option<Instant>,
    awaiting_present: bool,
    home_on_resize: bool,
    upload_cpu: Duration,
    upload_frames: u64,
    /// 操作とキーの表。割り当ての変更は履歴にも未保存の印にも入れない。
    pub keymap: Keymap,
    /// 画面のキーの文字。`keymap.rev()` が変わったときだけ作り直す。
    pub key_labels: KeyLabels,
    /// ショートカットの設定の画面の状態。
    pub keys: KeysState,
    /// 設定ファイルの持ち手。既定は場所の無いもの（読まない・書かない）。
    pub store: Store,
    pub(crate) scripted: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            model: None,
            doc: None,
            model_ref: None,
            review: None,
            camera: OrbitCamera::default(),
            display: DisplayMode::Faces,
            grid_visible: true,
            textures_shown: true,
            textures: TextureProgress::default(),
            paste_job: None,
            drop_probe: None,
            paste_error: None,
            texture_cache: Cell::new(None),
            exported_no_box: Default::default(),
            export_colors: Default::default(),
            exported_pasted: Default::default(),
            texture_options: loader::TextureOptions::default(),
            lines_supported: false,
            selection: None,
            color_cache: Cell::new(None),
            measurement: Measurement::default(),
            units: Units::default(),
            source_kind: SourceKind::Glb,
            snap: SnapSettings {
                radius_px: snap::SNAP_RADIUS_POINTS,
                ..SnapSettings::default()
            },
            snap_rest_until: None,
            snap_preview: None,
            section: None,
            section_memory: SectionPlane {
                axis: Axis::Z,
                offset: 0.0,
                flipped: false,
            },
            section_remembered: false,
            placed_triangles: 0,
            tool: Tool::Select,
            annotate_mode: false,
            history: History::default(),
            transform: None,
            pending_patches: Vec::new(),
            saved_doc_rev: 0,
            saved_review_rev: Some(0),
            pixels_per_point: 1.0,
            draft: None,
            pick_index: None,
            load: LoadPhase::Idle,
            dialog: None,
            draft_pending: false,
            body_pending: false,
            scene_dirty: SceneDirty::default(),
            metrics: Metrics::default(),
            panel_parts: true,
            panel_notes: true,
            panel_details: false,
            pending: None,
            restore: None,
            viewport_px: [0, 0],
            selected_annotation: None,
            status_text: String::new(),
            notice: String::new(),
            loading_file_name: None,
            quit_after_confirm: false,
            generation: 0,
            shown_generation: 0,
            review_generation: 0,
            loader: None,
            shown_loader: None,
            load_meta: None,
            incoming: None,
            staged_pick: None,
            load_hooks: LoadHooks::default(),
            export_job: None,
            export_hooks: ExportHooks::default(),
            paste_hooks: texture::PasteHooks::default(),
            export_error: None,
            export_failed: false,
            opened_files: Vec::new(),
            file_history: FileHistory::default(),
            exported_since_load: false,
            egui_ctx: None,
            gpu_upload: false,
            cancel_upload: false,
            discard_bypass: false,
            open_started: None,
            awaiting_present: false,
            home_on_resize: false,
            upload_cpu: Duration::ZERO,
            upload_frames: 0,
            keymap: Keymap::default(),
            key_labels: KeyLabels::default(),
            keys: KeysState::default(),
            store: Store::none(),
            scripted: false,
        }
    }
}

struct LoadMeta {
    generation: u64,
    source_path: PathBuf,
}

pub(super) struct ExportJob {
    pub generation: u64,
    pub doc_rev: u64,
    pub handle: ExportHandle,
    pub stage: ExportStage,
    pub peak_mem: Option<u64>,
}

struct Incoming {
    generation: u64,
    doc: Document,
    review: Option<ReviewState>,
    /// 新規作成では `None`（ファイルが無い）。そのときファイルの大きさと時間の項目は 0。
    model_ref: Option<ModelRef>,
    source_kind: SourceKind,
    source_path: Option<PathBuf>,
    file_bytes: u64,
    read: Duration,
    identity: Duration,
    parse: Duration,
}

struct StagedPick {
    generation: u64,
    index: PickIndex,
    elapsed: Duration,
}

impl Session {
    pub fn apply(&mut self, cmd: Command) {
        let tool = self.tool;
        let doc_rev = self.history.doc_rev();
        let camera = self.camera;
        let shown = self.shown_generation;
        let key_rev = self.keymap.rev();
        let history_action = matches!(cmd, Command::Undo | Command::Redo);
        let patch_count = self.pending_patches.len();
        self.apply_inner(cmd);
        if history_action
            && self.history.doc_rev() != doc_rev
            && self.pending_patches.len() == patch_count
        {
            // 見た目の履歴は、画像が替わると色・行列の部分更新範囲を返さない。
            self.scene_dirty.instances = true;
            self.pending_patches.clear();
        }
        if self.history.doc_rev() != doc_rev || self.is_loading() || self.is_exporting() {
            if let Some(job) = &mut self.paste_job {
                job.stale = true;
            }
            if let Some(job) = &mut self.drop_probe {
                job.stale = true;
            }
        }
        if self.scene_dirty.display || self.scene_dirty.instances {
            self.color_cache.set(None);
        }
        if self.keymap.rev() != key_rev {
            keys::save_changes(self);
        }
        self.clear_keys_for_dialog();
        // 道具・部品の表・視点・モデルが変わったら、吸着の候補は古い（次の移動で探し直す）。
        if self.tool != tool
            || self.history.doc_rev() != doc_rev
            || self.camera != camera
            || self.shown_generation != shown
        {
            snap::set_preview(self, None);
        }
    }

    fn apply_inner(&mut self, cmd: Command) {
        if self.transform.is_some() && interrupts_transform(&cmd, self.transform.as_ref()) {
            // 変形の途中に別の操作が来たら、変形をやめてから行う（文書はまだ書き換えていない）。
            edit::cancel_transform(self);
        }
        match cmd {
            Command::OpenModelDialog | Command::LoadReviewDialog | Command::RelinkModelDialog => {
                if self.is_exporting() {
                    self.status_text = "書き出し中は別のファイルを開けません".to_owned();
                }
            }
            Command::SaveReviewDialog => {
                if let Some(reason) = self.review_save_block_reason() {
                    self.status_text = reason.to_owned();
                } else if self.doc_unsaved() {
                    self.dialog = Some(export::export_first_dialog());
                    self.status_text = "先にGLBを書き出してください".to_owned();
                } else if self.is_new_document() {
                    persist::refuse_new_review(self);
                }
            }
            Command::ExportGlbDialog => {
                if self.is_exporting() {
                    self.status_text = "すでに書き出し中です".to_owned();
                } else if self.is_loading_replacement() {
                    self.block_edit_if_locked();
                } else if self.doc.is_none()
                    || (self.model_ref.is_none() && !self.is_new_document())
                {
                    self.status_text = "モデルを開くと書き出せます".to_owned();
                } else if export::nothing_to_export(self) {
                    export::refuse_empty(self);
                }
            }
            Command::NewDocument => open::new_guarded(self),
            Command::OpenModel(path) => open::open_guarded(self, path, LoadOptions::default()),
            Command::LoadReview(path) => open::review_guarded(self, path),
            Command::DroppedFiles(paths) => open::dropped(self, paths),
            Command::SaveReview(path) => persist::save_review(self, path),
            Command::RelinkModel(path) => persist::relink(self, path),
            Command::Orbit(delta) => camera::orbit(self, delta),
            Command::Pan(delta) => camera::pan(self, delta),
            Command::Zoom { factor, cursor_ndc } => camera::zoom(self, factor, cursor_ndc),
            Command::FitAll => camera::fit_all(self),
            Command::FitSelection => camera::fit_selection(self),
            Command::Home => {
                // 範囲が空（新規の直後や全部消した後）なら、新規の文書の初期視点へ戻す。
                // `OrbitCamera::home` は空の範囲で距離 1 にするため、ここで分ける。
                if self.doc.as_ref().is_some_and(|doc| doc.bounds().is_empty()) {
                    let before = self.camera;
                    self.camera = initial_camera(
                        &model_reviewer_core::math::Aabb::empty(),
                        aspect_of(self.viewport_px),
                        self.camera.projection,
                    );
                    camera::mark_camera(self, before);
                } else {
                    camera::home(self);
                }
            }
            Command::ViewClick { ndc } => {
                if self.model.is_some() && self.pick_index.is_none() {
                    self.status_text = "選択の準備中です".to_owned();
                } else {
                    pick::view_click(self, ndc);
                }
            }
            Command::Select(id) => parts::select(self, id),
            Command::SetVisible { node, visible } => {
                if self.block_edit_if_locked() {
                    return;
                }
                parts::set_visible(self, node, visible);
            }
            Command::HideSelected => {
                if self.block_edit_if_locked() {
                    return;
                }
                parts::hide_selected(self);
            }
            Command::ShowAll => {
                if self.block_edit_if_locked() {
                    return;
                }
                parts::show_all(self);
            }
            Command::SetAnnotateMode(on) => {
                if on {
                    if self.block_edit_if_locked() || self.model.is_none() {
                        return;
                    }
                    edit::set_tool(self, Tool::Annotate);
                    notes::set_mode(self, true);
                } else {
                    if self.tool == Tool::Annotate {
                        edit::set_tool(self, Tool::Select);
                    }
                    notes::set_mode(self, false);
                }
            }
            Command::CommitDraft { text } => {
                if self.block_edit_if_locked() {
                    return;
                }
                notes::commit(self, text);
            }
            Command::CancelDraft => notes::cancel(self),
            Command::EditAnnotation { id, text } => {
                if self.block_edit_if_locked() {
                    return;
                }
                notes::edit(self, id, text);
            }
            Command::DeleteAnnotation(id) => {
                if self.block_edit_if_locked() {
                    return;
                }
                notes::delete_annotation(self, id);
            }
            Command::SelectAnnotation(id) => notes::select_annotation(self, id),
            Command::SaveView { name } => {
                if self.block_edit_if_locked() {
                    return;
                }
                notes::save_view(self, name);
            }
            Command::ApplyView(id) => notes::apply_view(self, id),
            Command::DeleteView(id) => {
                if self.block_edit_if_locked() {
                    return;
                }
                notes::delete_view(self, id);
            }
            Command::SetPanel { panel, open } => self.set_panel(panel, open),
            Command::SetDraftPending(pending) => {
                if self.draft_pending != pending {
                    self.draft_pending = pending;
                }
            }
            Command::SetBodyPending(pending) => {
                if self.body_pending != pending {
                    self.body_pending = pending;
                }
            }
            Command::DismissDialog => {
                self.dialog = None;
            }
            Command::CancelRestore => {
                if self.restore.take().is_some()
                    || self
                        .incoming
                        .as_ref()
                        .is_some_and(|incoming| incoming.review.is_some())
                {
                    self.cancel_pending_load();
                    self.status_text = "レビューの復元をやめました".to_owned();
                }
                self.dialog = None;
                self.discard_bypass = false;
            }
            Command::ConfirmDiscard => self.confirm_discard(),
            Command::CancelPending => {
                self.pending = None;
                self.dialog = None;
                self.discard_bypass = false;
            }
            // 第2段階の分。書き出し・図形・計測は後の接合が書く。
            Command::SnapView(dir) => view::snap_view(self, dir),
            Command::SetDisplay(mode) => view::set_display(self, mode),
            Command::SetProjection(projection) => view::set_projection(self, projection),
            Command::ToggleProjection => view::toggle_projection(self),
            Command::SetGrid(visible) => view::set_grid(self, visible),
            Command::SetTexturesShown(shown) => {
                if self.texture_options.enabled && self.textures_shown != shown {
                    self.textures_shown = shown;
                    self.scene_dirty.display = true;
                }
            }
            Command::SetTool(tool) => edit::set_tool(self, tool),
            Command::BeginTransform { handle, ndc } => edit::begin_transform(self, handle, ndc),
            Command::DragTransform { ndc } => edit::drag_transform(self, ndc),
            Command::CommitTransform => edit::commit_transform(self),
            Command::CancelTransform => edit::cancel_transform(self),
            Command::PreviewLocal(trs) => edit::preview_local(self, trs),
            Command::CommitLocal(trs) => edit::commit_local(self, trs),
            Command::SetColor(rgb) => color::set_color(self, rgb),
            Command::PasteTextureDialog => {
                if let Some(reason) = texture::paste_block(self) {
                    self.status_text = reason.into();
                }
            }
            Command::PasteTexture(path) => texture::paste(self, path),
            Command::RemoveTexture => texture::adjust(self, texture::Adjustment::Remove),
            Command::SetTextureSize(size) => texture::adjust(self, texture::Adjustment::Size(size)),
            Command::Undo => edit::undo(self),
            Command::Redo => edit::redo(self),
            Command::AddShape(kind) => edit::add_shape(self, kind),
            Command::DuplicateSelected => edit::duplicate_selected(self),
            Command::DeleteSelected => edit::delete_selected(self),
            Command::ExportGlb(path) => export::export_glb(self, path),
            Command::ClearMeasure => measure::clear_measure(self),
            Command::SetUnit(unit) => measure::set_unit(self, unit),
            // 第3段階の分。中身は接合の作業で書く。
            Command::ViewHover { ndc } => snap::hover(self, ndc),
            Command::SetSnap(on) => snap::set_snap(self, on),
            Command::ToggleSnap => snap::toggle_snap(self),
            Command::SetSection(plane) => section::set_section(self, plane),
            Command::ToggleSection => section::toggle_section(self),
            Command::SetDisplayUnit(unit) => measure::set_display_unit(self, unit),
            Command::SetStlUnit(unit) => set_stl_assumptions(self, Some(unit), None),
            Command::SetStlUp(up) => set_stl_assumptions(self, None, Some(up)),
            Command::SetKeysOpen(open) => keys::set_open(self, open),
            Command::BeginKeyCapture(action) => keys::begin_capture(self, action),
            Command::CancelKeyCapture => keys::cancel_capture(self),
            Command::CaptureKey(key) => keys::capture(self, key),
            Command::ResolveKeyConflict { replace } => keys::resolve_conflict(self, replace),
            Command::ClearKey(action) => keys::clear(self, action),
            Command::ResetKey(action) => keys::reset(self, action),
            Command::ArmResetAllKeys(armed) => keys::arm_reset_all(self, armed),
            Command::ResetAllKeys => keys::reset_all(self),
            Command::BeginKeyTransform(axis) => keymove::begin(self, axis),
            Command::KeyTransformInput(key) => keymove::input(self, key),
            Command::Nudge { axis, sign, fine } => keymove::nudge(self, axis, sign, fine),
        }
    }

    pub fn poll_loader(&mut self) -> bool {
        let events: Vec<_> = self
            .shown_loader
            .iter()
            .chain(&self.loader)
            .flat_map(LoadHandle::drain)
            .collect();
        let changed = !events.is_empty();
        for event in events {
            self.apply_load_event(event);
        }
        let routed = open::poll_drop(self);
        texture::poll(self) || routed || changed
    }

    pub fn poll_exporter(&mut self) -> bool {
        export::poll(self)
    }

    pub fn set_load_hooks(&mut self, hooks: LoadHooks) {
        self.load_hooks = hooks;
    }

    pub fn set_export_hooks(&mut self, hooks: ExportHooks) {
        self.export_hooks = hooks;
    }

    pub fn set_paste_hooks(&mut self, hooks: texture::PasteHooks) {
        self.paste_hooks = hooks;
    }

    /// 書き出し中にUIスレッドで観測した常駐メモリを最大値へ反映する。
    pub(crate) fn observe_export_memory(&mut self, bytes: Option<u64>) {
        if let (Some(job), Some(bytes)) = (self.export_job.as_mut(), bytes) {
            job.peak_mem = Some(job.peak_mem.map_or(bytes, |prev| prev.max(bytes)));
        }
    }

    pub fn is_exporting(&self) -> bool {
        self.export_job.is_some()
    }

    pub fn export_stage(&self) -> Option<ExportStage> {
        self.export_job.as_ref().map(|job| job.stage)
    }

    pub fn export_error(&self) -> Option<&str> {
        self.export_error.as_deref()
    }

    pub fn export_failed(&self) -> bool {
        self.export_failed
    }

    pub fn opened_files(&self) -> &[PathBuf] {
        &self.opened_files
    }

    pub(crate) fn set_egui_ctx(&mut self, ctx: &egui::Context) {
        self.egui_ctx = Some(ctx.clone());
    }

    /// 非同期の読み込み・書き出しでも、ダイアログを表示する前に取り込みと確認を解く。
    pub(crate) fn clear_keys_for_dialog(&mut self) {
        if self.dialog.is_some() {
            self.keys.clear_pending();
        }
    }

    /// 部品を読む口。モデルが無ければ `None`。
    pub fn scene(&self) -> Option<&Document> {
        self.doc.as_ref()
    }

    /// モデルを置き、その文書を作る。読み込み（`commit_pending`）と、読み込みを通さない試験が使う。
    /// レビュー・選択・視点には触れない。
    pub fn set_model(&mut self, model: Arc<Model>) {
        self.exported_no_box.clear();
        self.export_colors = Default::default();
        self.exported_pasted.clear();
        self.reset_textures();
        self.color_cache.set(None);
        self.doc = Some(Document::new(Arc::clone(&model)));
        self.model = Some(model);
        self.measurement.clear();
        self.snap_preview = None;
        section::reset(self);
        self.recount_triangles();
    }

    /// 文書の部品が置く三角形の数（状態欄）。読み込みの統計は足した図形と削除を数えない。
    pub fn placed_triangles(&self) -> u64 {
        self.placed_triangles
    }

    pub(super) fn recount_triangles(&mut self) {
        self.placed_triangles = self.doc.as_ref().map_or(0, |doc| {
            (0..doc.node_count())
                .flat_map(|index| doc.draws(NodeId(index as u32)))
                .map(|draw| {
                    doc.primitive(draw.prim)
                        .map_or(0, |prim| (prim.indices.len() / 3) as u64)
                })
                .sum()
        });
    }

    pub(crate) fn enable_gpu_upload(&mut self) {
        self.gpu_upload = true;
    }

    /// 装置が線の表示を持つか。起動時に `gpu_view` の結果で1回だけ決める。
    pub fn set_lines_supported(&mut self, supported: bool) {
        self.lines_supported = supported;
        if !supported && view::needs_lines(self.display) {
            self.display = DisplayMode::Faces;
            self.scene_dirty.display = true;
        }
    }

    pub(crate) fn open_launch(&mut self, path: PathBuf) {
        open::open_launch(self, path);
    }

    /// 未保存の確認を通して開く。STL は単位と上の向きを指定できる。
    pub fn open_model_with(&mut self, path: PathBuf, options: LoadOptions) {
        if self.transform.is_some() {
            edit::cancel_transform(self);
        }
        open::open_guarded(self, path, options);
        snap::set_preview(self, None);
    }

    pub fn set_viewport(&mut self, size_px: [u32; 2]) {
        if self.viewport_px != size_px {
            self.viewport_px = size_px;
            self.scene_dirty.size = true;
            if self.home_on_resize && size_px[0] > 0 && size_px[1] > 0 {
                self.home_on_resize = false;
                if let Some(doc) = &self.doc {
                    let bounds = doc.bounds();
                    self.camera =
                        initial_camera(&bounds, aspect_of(size_px), self.camera.projection);
                    self.scene_dirty.camera = true;
                }
            }
        }
    }

    /// 移動に合わせた吸着の探索を休んでいるか。
    pub fn snap_resting(&self) -> bool {
        self.snap_rest_until
            .is_some_and(|until| Instant::now() < until)
    }

    /// 移動に合わせた吸着の探索を `rest` だけ休ませる。遅い探索の後と同じ状態を、試験で作るのに
    /// 使う（実際の休みは `snap::rest_after` が決める）。
    pub fn rest_snap_for(&mut self, rest: Duration) {
        self.snap_rest_until = Some(Instant::now() + rest);
    }

    pub fn take_scene_dirty(&mut self) -> SceneDirty {
        std::mem::take(&mut self.scene_dirty)
    }

    /// 確定した変形や色の変更で部分更新する範囲。取り出すと空になる。
    pub fn take_instance_patches(&mut self) -> Vec<Range<u32>> {
        std::mem::take(&mut self.pending_patches)
    }

    /// 形の編集またはレビュー（確定済みの分）に未保存の変更があるか。入力途中は含めない
    /// （含めるのは `has_unsaved`）。
    pub fn unsaved(&self) -> bool {
        self.doc_unsaved() || self.review_state_unsaved()
    }

    /// 形の編集に未書き出しの変更があるか。
    pub fn doc_unsaved(&self) -> bool {
        self.history.doc_rev() != self.saved_doc_rev
    }

    /// レビュー（表示・注釈・視点）に未保存の変更があるか（入力途中を含む）。
    pub fn review_unsaved(&self) -> bool {
        self.review_state_unsaved() || self.draft_pending || self.body_pending
    }

    /// 確定済みのレビュー状態（入力途中を除く）に未保存の変更があるか。
    fn review_state_unsaved(&self) -> bool {
        self.saved_review_rev != Some(self.history.review_rev())
    }

    /// いまの版を、形もレビューも保存済みとする（入力途中の印は変えない）。
    /// 試験で「保存した直後」を作るのにも使う。
    pub fn mark_saved(&mut self) {
        self.saved_doc_rev = self.history.doc_rev();
        self.saved_review_rev = Some(self.history.review_rev());
    }

    /// レビューを未保存にする（保存した版を持たない状態）。試験で「保存していないレビュー」を
    /// 作るのに使う。
    pub fn mark_review_unsaved(&mut self) {
        self.saved_review_rev = None;
    }

    /// レビューを保存した。レビューの版をいまの版にそろえる。
    pub(super) fn mark_review_saved(&mut self) {
        self.saved_review_rev = Some(self.history.review_rev());
    }

    /// GLBを書き出した。文書の版を書き出し済みにし、作業中のレビューがあれば未保存にする。
    pub(super) fn mark_doc_exported(&mut self) {
        self.saved_doc_rev = self.history.doc_rev();
        if self.review.as_ref().is_some_and(review_has_content) {
            if self.saved_review_rev == Some(self.history.review_rev()) {
                self.saved_review_rev = None;
            }
        } else {
            self.saved_review_rev = Some(self.history.review_rev());
        }
    }

    /// モデルやレビューを開いた。履歴を空にし、いまの版を保存済みとする。
    pub(super) fn reset_history(&mut self) {
        self.color_cache.set(None);
        self.history.clear();
        self.transform = None;
        self.pending_patches.clear();
        self.saved_doc_rev = self.history.doc_rev();
        self.saved_review_rev = Some(self.history.review_rev());
        self.metrics.history_bytes = Some(self.history.bytes());
        self.metrics.history_len = Some(self.history.len() as u64);
    }

    /// 書き出し中、または未保存・入力途中なら閉じるのを止め、確認を出す。止めるときは `true`。
    pub fn on_close_requested(&mut self) -> bool {
        if self.is_exporting() {
            self.status_text = "書き出し中は閉じられません".to_owned();
            self.dialog = Some(exporting_close_dialog());
        } else if !self.needs_confirm() || self.quit_after_confirm {
            return false;
        } else if self.pending.is_none() {
            self.pending = Some(Pending::Quit);
            self.dialog = Some(self.discard_dialog());
        }
        self.clear_keys_for_dialog();
        true
    }

    pub fn take_quit(&mut self) -> bool {
        std::mem::take(&mut self.quit_after_confirm)
    }

    pub fn has_unsaved(&self) -> bool {
        self.needs_confirm()
    }

    fn needs_confirm(&self) -> bool {
        self.doc_unsaved() || self.review_unsaved()
    }

    fn hold(&mut self, pending: Pending) -> bool {
        if !self.needs_confirm() || self.discard_bypass {
            return false;
        }
        self.pending = Some(pending);
        self.dialog = Some(self.discard_dialog());
        true
    }

    fn discard_dialog(&self) -> DialogRequest {
        discard_dialog(
            self.doc_unsaved(),
            self.review_state_unsaved(),
            self.draft_pending || self.body_pending,
        )
    }

    fn confirm_discard(&mut self) {
        self.dialog = None;
        self.discard_bypass = true;
        match self.pending.take() {
            Some(Pending::OpenModel(path, options, expected)) => {
                open::open_model(self, path, options, expected)
            }
            Some(Pending::LoadReview(path)) => open::load_review(self, path),
            Some(Pending::New) => open::new_document(self),
            Some(Pending::Quit) => {
                self.discard_bypass = false;
                self.mark_saved();
                self.draft_pending = false;
                self.body_pending = false;
                self.quit_after_confirm = true;
            }
            None => self.discard_bypass = false,
        }
    }

    pub(super) fn end_discard_bypass(&mut self) {
        self.discard_bypass = false;
    }

    pub fn draft_pending(&self) -> bool {
        self.draft_pending
    }

    pub fn body_pending(&self) -> bool {
        self.body_pending
    }

    pub fn review_generation(&self) -> u64 {
        self.review_generation
    }

    pub(super) fn advance_review_generation(&mut self) {
        self.review_generation = self.review_generation.wrapping_add(1);
    }

    /// 新規作成した文書を、まだ書き出していない（`source_kind` が `New`）。
    pub fn is_new_document(&self) -> bool {
        self.doc.is_some() && self.source_kind == SourceKind::New
    }

    /// 読み込み（またはGPUへの転送）の途中。表示中のモデルの有無によらない。
    pub fn is_loading(&self) -> bool {
        self.incoming.is_some()
            || matches!(
                self.load,
                LoadPhase::Reading | LoadPhase::Parsing | LoadPhase::Uploading { .. }
            )
    }

    /// 表示中のモデルを置き換える読み込みの途中。
    pub fn is_loading_replacement(&self) -> bool {
        self.model.is_some()
            && (self.incoming.is_some()
                || matches!(
                    self.load,
                    LoadPhase::Reading | LoadPhase::Parsing | LoadPhase::Uploading { .. }
                ))
    }

    /// 表示中のモデルを置き換える読み込み、または書き出しの途中。
    pub fn edits_locked(&self) -> bool {
        self.is_loading_replacement() || self.is_exporting()
    }

    pub(crate) fn review_save_block_reason(&self) -> Option<&'static str> {
        if self.is_exporting() {
            Some("書き出しが終わってからレビューを保存してください")
        } else if self.is_loading_replacement() {
            Some("読み込みが終わってからレビューを保存してください")
        } else if self.paste_job.is_some() || self.drop_probe.is_some() {
            Some(texture::PENDING_REVIEW)
        } else {
            None
        }
    }

    pub fn edit_block_reason(&self) -> Option<&'static str> {
        if self.is_exporting() {
            Some("書き出し中は編集・表示・注釈・視点を変更できません")
        } else if self.is_loading_replacement() {
            Some("読み込み中は編集・表示・注釈・視点を変更できません")
        } else {
            None
        }
    }

    pub(super) fn block_edit_if_locked(&mut self) -> bool {
        if let Some(reason) = self.edit_block_reason() {
            self.status_text = reason.to_owned();
            true
        } else {
            false
        }
    }

    pub(super) fn start_model_load_for_restore(&mut self, path: PathBuf, expected: ModelIdentity) {
        let options = self
            .restore
            .as_ref()
            .map_or_else(LoadOptions::default, |pending| LoadOptions {
                stl_unit: pending.file.model.stl_unit,
                stl_up: Some(pending.file.model.saved_stl_up()),
            });
        self.start_model_load_with_expected(path, Some(expected), options);
    }

    fn start_model_load_with_expected(
        &mut self,
        path: PathBuf,
        expected_identity: Option<ModelIdentity>,
        options: LoadOptions,
    ) {
        if expected_identity.is_some() {
            self.protect_document(&path);
        }
        self.keys.clear_pending();
        self.generation = self.generation.wrapping_add(1);
        self.loader = None;
        edit::cancel_transform(self);
        self.incoming = None;
        self.staged_pick = None;
        self.cancel_upload = true;
        self.upload_cpu = Duration::ZERO;
        self.upload_frames = 0;
        self.awaiting_present = false;
        self.open_started = Some(Instant::now());
        self.export_error = None;
        self.export_failed = false;
        self.loading_file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .or_else(|| Some(path.display().to_string()));
        self.dialog = None;
        self.load = LoadPhase::Reading;
        let generation = self.generation;
        self.load_meta = Some(LoadMeta {
            generation,
            source_path: path.clone(),
        });
        let hooks = self.load_hooks.clone();
        let ctx = self.egui_ctx.clone();
        self.loader = Some(loader::spawn(
            generation,
            path,
            expected_identity,
            options,
            self.texture_options,
            hooks,
            ctx,
        ));
    }

    /// 空のモデルを、読み込んだモデルと同じ受け渡し（`Incoming` → 転送 → `commit_pending`）へ流す。
    /// ファイルは読まず、何も書かない（要件1.10）。
    pub(super) fn start_new_document(&mut self) {
        self.keys.clear_pending();
        self.generation = self.generation.wrapping_add(1);
        self.loader = None;
        edit::cancel_transform(self);
        self.staged_pick = None;
        self.load_meta = None;
        self.cancel_upload = true;
        self.upload_cpu = Duration::ZERO;
        self.upload_frames = 0;
        self.awaiting_present = false;
        self.open_started = Some(Instant::now());
        self.export_error = None;
        self.export_failed = false;
        self.loading_file_name = None;
        self.dialog = None;
        self.incoming = Some(Incoming {
            generation: self.generation,
            doc: Document::new(Arc::new(Model::empty())),
            review: None,
            model_ref: None,
            source_kind: SourceKind::New,
            source_path: None,
            file_bytes: 0,
            read: Duration::ZERO,
            identity: Duration::ZERO,
            parse: Duration::ZERO,
        });
        if self.gpu_upload {
            // 送るものは0バイトで、次の転送の1回で終わる。
            self.load = LoadPhase::Uploading {
                uploaded_bytes: 0,
                total_bytes: 0,
            };
        } else {
            self.commit_pending(false);
        }
    }

    pub(super) fn cancel_pending_load(&mut self) {
        if self.shown_generation != self.generation {
            self.generation = self.generation.wrapping_add(1);
            self.loader = None;
            self.incoming = None;
            self.staged_pick = None;
            self.loading_file_name = None;
            self.cancel_upload = true;
            self.open_started = None;
            self.awaiting_present = false;
            self.upload_cpu = Duration::ZERO;
            self.upload_frames = 0;
            if matches!(
                self.load,
                LoadPhase::Reading | LoadPhase::Parsing | LoadPhase::Uploading { .. }
            ) {
                self.load = LoadPhase::Idle;
            }
            if self.shown_loader.is_none() {
                self.rebuild_shown_pick();
            }
        }
    }

    pub(super) fn fail_load(&mut self, title: &str, message: String) {
        self.cancel_pending_load();
        self.incoming = None;
        self.staged_pick = None;
        self.loading_file_name = None;
        self.open_started = None;
        self.awaiting_present = false;
        self.upload_cpu = Duration::ZERO;
        self.upload_frames = 0;
        self.discard_bypass = false;
        self.load = LoadPhase::Failed(message.clone());
        self.status_text = "読み込みに失敗しました".to_owned();
        self.dialog = Some(failure_dialog(title, message));
        log::error!("load failed: {title}");
    }

    fn apply_load_event(&mut self, event: LoadEvent) {
        match event {
            LoadEvent::Stage { generation, phase } => {
                if generation == self.generation {
                    self.load = phase;
                }
            }
            LoadEvent::Loaded(loaded) => self.accept_loaded(*loaded),
            LoadEvent::Mismatch { generation, actual } => {
                if generation != self.generation {
                    return;
                }
                if self.restore.is_some() {
                    persist::on_candidate_mismatch(self, actual);
                } else {
                    self.fail_load(
                        "STL の単位や上の向きを変更できません",
                        "開いたときから STL の内容が変わっています。元のモデルと編集を残し、これまでの単位と上の向きで表示を続けます。"
                            .to_owned(),
                    );
                }
            }
            LoadEvent::Failed {
                generation,
                message,
            } => {
                if self.shown_generation == generation && self.rebuild_shown_pick() {
                    return;
                }
                if generation != self.generation {
                    return;
                }
                if persist::try_next_candidate_after_error(self) {
                    return;
                }
                self.fail_load("読み込めません", message);
            }
            LoadEvent::Pick {
                generation,
                index,
                elapsed,
            } => self.accept_pick(generation, index, elapsed),
            LoadEvent::TexturePlan {
                generation,
                total,
                shrunk,
                skipped,
            } => {
                if generation == self.shown_generation {
                    self.textures.total = total;
                    self.textures.planned_shrinks = shrunk;
                    for (image, reason) in skipped {
                        self.textures.fail(image, reason);
                    }
                }
            }
            LoadEvent::TextureFailed {
                generation,
                image,
                error,
            } => {
                if generation == self.shown_generation {
                    self.textures.fail(image, error);
                    self.finish_texture_metrics();
                }
            }
            LoadEvent::TexturesDone { generation } => {
                if generation == self.shown_generation {
                    self.shown_loader = None;
                    self.finish_texture_metrics();
                } else if generation == self.generation {
                    self.loader = None;
                }
            }
        }
    }

    fn accept_loaded(&mut self, loaded: LoadedModel) {
        if loaded.generation != self.generation {
            return;
        }
        if persist::reject_mismatch(self, &loaded.model_ref) {
            // 次の候補の読み込みに進んだ場合は、その工程と保存の制限を保つ。
            return;
        }
        // 転送が終わると描画器の表示が替わる。復元に失敗しうる処理は転送前に済ませる。
        let doc = Document::new(loaded.model);
        let review = match persist::take_restored(self, &doc, &loaded.model_ref) {
            persist::Restored::Absent => None,
            persist::Restored::State(state) => Some(state),
            persist::Restored::Blocked => {
                self.cancel_pending_load();
                self.end_discard_bypass();
                return;
            }
        };
        let source_path = self
            .load_meta
            .as_ref()
            .filter(|meta| meta.generation == loaded.generation)
            .map(|meta| meta.source_path.clone());
        self.incoming = Some(Incoming {
            generation: loaded.generation,
            doc,
            review,
            model_ref: Some(loaded.model_ref),
            source_kind: loaded.source_kind,
            source_path,
            file_bytes: loaded.file_bytes,
            read: loaded.read,
            identity: loaded.identity,
            parse: loaded.parse,
        });
        if self.gpu_upload {
            self.load = LoadPhase::Uploading {
                uploaded_bytes: 0,
                total_bytes: 0,
            };
        } else {
            self.commit_pending(false);
        }
    }

    /// 表示後の索引作成が失敗したか、次の読み込みに中断されたときの回復。
    fn rebuild_shown_pick(&mut self) -> bool {
        if self.pick_index.is_some() {
            return false;
        }
        let Some(model) = self.model.as_ref().map(Arc::clone) else {
            return false;
        };
        let started = Instant::now();
        let built = match self.doc.as_ref() {
            Some(doc) => {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| PickIndex::build(doc)))
            }
            None => {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| PickIndex::build(&model)))
            }
        };
        match built {
            Ok(index) => {
                self.pick_index = Some(index);
                self.metrics.pick = Some(started.elapsed());
                true
            }
            Err(payload) => {
                log::error!("pick rebuild panicked: {}", panic_message(&payload));
                false
            }
        }
    }

    fn accept_pick(&mut self, generation: u64, index: PickIndex, elapsed: Duration) {
        if self.shown_generation == generation && self.model.is_some() {
            let mut index = index;
            // 索引は読み込んだ形状だけで作られる。届く前に足した図形の分を足す。
            if let Some(doc) = self.doc.as_ref() {
                index.extend(doc);
            }
            self.pick_index = Some(index);
            self.metrics.pick = Some(elapsed);
            return;
        }
        if self
            .incoming
            .as_ref()
            .is_some_and(|incoming| incoming.generation == generation)
        {
            self.staged_pick = Some(StagedPick {
                generation,
                index,
                elapsed,
            });
        }
    }

    pub(crate) fn has_incoming(&self) -> bool {
        self.incoming.is_some()
    }

    pub(crate) fn incoming_model(&self) -> Option<Arc<Model>> {
        self.incoming
            .as_ref()
            .map(|incoming| Arc::clone(incoming.doc.base()))
    }

    pub(crate) fn take_cancel_upload(&mut self) -> bool {
        std::mem::take(&mut self.cancel_upload)
    }

    pub(crate) fn record_upload_step(&mut self, elapsed: Duration) {
        self.upload_cpu += elapsed;
        self.upload_frames = self.upload_frames.saturating_add(1);
    }

    pub(crate) fn set_upload_progress(&mut self, uploaded_bytes: u64, total_bytes: u64) {
        if self.incoming.is_some() {
            self.load = LoadPhase::Uploading {
                uploaded_bytes,
                total_bytes,
            };
        }
    }

    pub(crate) fn reject_upload(&mut self, message: String) {
        log::error!("upload failed: {message}");
        self.fail_load("表示できません", message);
    }

    /// デバイスエラー。モデルとレビューは残し、3Dの転送だけを捨てる。
    pub fn close_for_gpu_error(&mut self, message: String) {
        log::error!("gpu error, stopping 3D: {message}");
        self.cancel_pending_load();
        if self.shown_loader.take().is_some() {
            self.rebuild_shown_pick();
        }
        self.reset_textures();
        self.color_cache.set(None);
        self.incoming = None;
        self.staged_pick = None;
        self.loading_file_name = None;
        self.cancel_upload = true;
        self.open_started = None;
        self.awaiting_present = false;
        self.home_on_resize = false;
        self.discard_bypass = false;
        edit::cancel_transform(self);
        if matches!(
            self.load,
            LoadPhase::Reading | LoadPhase::Parsing | LoadPhase::Uploading { .. }
        ) {
            self.load = LoadPhase::Idle;
        }
        self.scene_dirty.model = true;
        self.status_text = "3D表示を止めました".to_owned();
        self.dialog = Some(failure_dialog("3D表示を止めました", message));
    }

    pub(crate) fn commit_pending(&mut self, from_gpu: bool) {
        let Some(incoming) = self.incoming.take() else {
            return;
        };
        if incoming.generation != self.generation {
            return;
        }
        let is_new = incoming.source_kind == SourceKind::New;
        let doc = incoming.doc;
        let review = incoming.review;
        notes::clear_body_edits(self);
        notes::cancel(self);
        self.body_pending = false;
        self.advance_review_generation();
        let restored_review = review.is_some();
        let model = Arc::clone(doc.base());
        let aspect = aspect_of(self.viewport_px);
        let viewport_known = self.viewport_px[0] > 0 && self.viewport_px[1] > 0;
        // 投影は利用者が選んだ見方なので、モデルを開き直しても保つ。
        self.camera = initial_camera(&doc.bounds(), aspect, self.camera.projection);
        self.home_on_resize = !viewport_known;
        let own_output = incoming.model_ref.as_ref().is_some_and(|loaded| {
            restored_review
                && self.exported_since_load
                && self.model_ref.as_ref().is_some_and(|current| {
                    current.identity == loaded.identity
                        && crate::exporter::same_file(&current.path, &loaded.path)
                        && self
                            .file_history
                            .is_written_output(&current.path, &loaded.identity)
                })
        });
        if !own_output {
            self.exported_pasted.clear();
            if let Some(loaded) = incoming.model_ref.as_ref() {
                if let Some(source) = incoming.source_path.as_deref() {
                    self.remember_opened_file(source);
                }
                self.remember_opened_file(&loaded.path);
                self.file_history.remember_opened(&loaded.identity);
            }
        }
        self.exported_since_load = false;
        self.export_error = None;
        self.export_failed = false;
        self.model_ref = incoming.model_ref;
        self.review = Some(review.unwrap_or_else(|| ReviewState::new(&doc)));
        self.selection = None;
        self.selected_annotation = None;
        self.annotate_mode = false;
        if self.tool == Tool::Annotate {
            self.tool = Tool::Select;
        }
        self.pick_index = None;
        self.shown_generation = incoming.generation;
        self.shown_loader = self.loader.take();
        self.doc = Some(doc);
        self.model = Some(model);
        self.exported_no_box.clear();
        self.export_colors = Default::default();
        self.reset_textures();
        if from_gpu {
            self.textures.opened = self.open_started;
        } else {
            self.allow_first_image();
        }
        self.measurement.clear();
        self.snap_preview = None;
        self.snap_rest_until = None;
        section::reset(self);
        // GLB を開くと、ファイルの単位は m、表示は「ファイルと同じ」（要件7.5）。STL は部品の大きさで
        // メートルに合わせてあるので、シーンの単位は m、表示は STL の単位の仮定（要件7.6）。
        self.source_kind = incoming.source_kind;
        // 新規の文書も GLB と同じ（シーンはメートル、表示は「ファイルと同じ」。要件1.5）。
        self.units = match incoming.source_kind {
            SourceKind::Glb | SourceKind::New => Units::default(),
            SourceKind::Stl { unit, .. } => Units {
                source: UnitLabel::M,
                display: Some(unit),
            },
        };
        self.recount_triangles();
        self.reset_history();
        self.discard_bypass = false;
        self.loading_file_name = None;
        self.load = LoadPhase::Idle;
        self.status_text = if is_new {
            "新規の文書を作りました".to_owned()
        } else if restored_review {
            "レビューを復元しました".to_owned()
        } else {
            "読み込み完了".to_owned()
        };
        self.dialog = None;
        self.scene_dirty.model = true;
        self.scene_dirty.instances = true;
        self.scene_dirty.selection = true;
        self.scene_dirty.camera = true;
        // 新規の文書はファイルを読んでいないので、ファイルの大きさと読み取り・識別・解析は測らない。
        let measured = !is_new;
        self.metrics.file_bytes = measured.then_some(incoming.file_bytes);
        self.metrics.read = measured.then_some(incoming.read);
        self.metrics.identity = measured.then_some(incoming.identity);
        self.metrics.parse = measured.then_some(incoming.parse);
        self.metrics.pick = None;
        self.metrics.first_present = None;
        self.metrics.upload_cpu = None;
        self.metrics.upload_frames = None;
        if from_gpu {
            self.metrics.upload_cpu = Some(self.upload_cpu);
            self.metrics.upload_frames = Some(self.upload_frames);
            self.awaiting_present = true;
        } else {
            self.awaiting_present = false;
            self.open_started = None;
        }
        self.upload_cpu = Duration::ZERO;
        self.upload_frames = 0;
        if let Some(pick) = self
            .staged_pick
            .take()
            .filter(|pick| pick.generation == incoming.generation)
        {
            self.pick_index = Some(pick.index);
            self.metrics.pick = Some(pick.elapsed);
        }
        // 新規の文書には読み込みのスレッドが無い。形状0個の索引はすぐ作れるので、ここで作る
        // （無いと、クリックが「選択の準備中」になり、自動操作の `wait_idle` が終わらない）。
        if is_new && self.pick_index.is_none() {
            let started = Instant::now();
            self.pick_index = self.doc.as_ref().map(PickIndex::build);
            self.metrics.pick = Some(started.elapsed());
        }
    }

    pub(crate) fn note_presented(&mut self) {
        if !self.awaiting_present {
            return;
        }
        if let Some(started) = self.open_started.take() {
            self.metrics.first_present = Some(started.elapsed());
        }
        self.awaiting_present = false;
    }

    fn remember_opened_file(&mut self, path: &Path) {
        if path.as_os_str().is_empty() {
            return;
        }
        if !self
            .opened_files
            .iter()
            .any(|existing| existing.as_os_str() == path.as_os_str())
        {
            self.opened_files.push(path.to_path_buf());
        }
    }

    fn set_panel(&mut self, panel: PanelKind, open: bool) {
        match panel {
            PanelKind::Parts => self.panel_parts = open,
            PanelKind::Notes => self.panel_notes = open,
            PanelKind::Details => self.panel_details = open,
        }
    }
}

/// 変更しない仮定は今の値を引き継ぎ、未保存の確認を通して同じ STL を開き直す。
fn set_stl_assumptions(session: &mut Session, new_unit: Option<UnitLabel>, new_up: Option<UpAxis>) {
    let SourceKind::Stl { unit, up } = session.source_kind else {
        return;
    };
    let (next_unit, next_up) = (new_unit.unwrap_or(unit), new_up.unwrap_or(up));
    if (unit, up) == (next_unit, next_up) {
        return;
    }
    if session.is_loading_replacement() {
        session.block_edit_if_locked();
        return;
    }
    let Some(model_ref) = session.model_ref.as_ref() else {
        return;
    };
    open::open_guarded_with_identity(
        session,
        model_ref.path.clone(),
        LoadOptions {
            stl_unit: Some(next_unit),
            stl_up: Some(next_up),
        },
        Some(model_ref.identity.clone()),
    );
}

/// 変形の途中に届いたら、先に変形をやめる操作。視点の操作は、ハンドルのドラッグのときだけ
/// やめる（掴んだ時点の視点で計算しているため）。数値の欄のドラッグは視点に依らない。
fn interrupts_transform(cmd: &Command, transform: Option<&Transform>) -> bool {
    let gizmo = matches!(
        transform.map(|t| &t.source),
        Some(TransformSource::Gizmo(_))
    );
    match cmd {
        Command::Orbit(_)
        | Command::Pan(_)
        | Command::Zoom { .. }
        | Command::FitAll
        | Command::FitSelection
        | Command::Home
        | Command::SnapView(_)
        | Command::SetProjection(_)
        | Command::ToggleProjection
        | Command::ApplyView(_) => gizmo,
        Command::OpenModel(_)
        | Command::LoadReview(_)
        | Command::DroppedFiles(_)
        | Command::RelinkModel(_)
        | Command::ViewClick { .. }
        | Command::Select(_)
        | Command::SetVisible { .. }
        | Command::HideSelected
        | Command::ShowAll
        | Command::SetAnnotateMode(_)
        | Command::CommitDraft { .. }
        | Command::EditAnnotation { .. }
        | Command::DeleteAnnotation(_)
        | Command::SelectAnnotation(_)
        | Command::DeleteView(_)
        | Command::PasteTexture(_)
        | Command::RemoveTexture
        | Command::SetTextureSize(_)
        | Command::Undo
        | Command::Redo
        | Command::AddShape(_)
        | Command::DuplicateSelected
        | Command::DeleteSelected
        | Command::ExportGlb(_)
        | Command::NewDocument
        | Command::SetTool(_) => true,
        _ => false,
    }
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else {
        "不明".to_owned()
    }
}

/// 開いた直後の視点。範囲が空（新規の文書）なら、原点を `NEW_DISTANCE` から見る（格子 0.1）。
/// 投影は利用者が選んだ見方なので、引き継ぐ。
fn initial_camera(
    bounds: &model_reviewer_core::math::Aabb,
    aspect: f32,
    projection: model_reviewer_core::camera::Projection,
) -> OrbitCamera {
    let mut camera = OrbitCamera::home(bounds, aspect);
    if bounds.is_empty() {
        camera.target = glam::Vec3::ZERO;
        camera.distance = NEW_DISTANCE;
    }
    camera.projection = projection;
    camera
}

fn aspect_of(px: [u32; 2]) -> f32 {
    if px[0] == 0 || px[1] == 0 {
        1.0
    } else {
        px[0] as f32 / px[1] as f32
    }
}

pub(super) fn failure_dialog(title: &str, body: String) -> DialogRequest {
    DialogRequest {
        title: title.to_owned(),
        body,
        buttons: vec![DialogButton {
            id: ids::DIALOG_CANCEL.to_owned(),
            label: "閉じる".to_owned(),
            command: Command::DismissDialog,
        }],
    }
}

fn exporting_close_dialog() -> DialogRequest {
    DialogRequest {
        title: "書き出し中です".to_owned(),
        body: "GLBを書き出しています。終わってから閉じてください。".to_owned(),
        buttons: vec![DialogButton {
            id: ids::DIALOG_CANCEL.to_owned(),
            label: "閉じる".to_owned(),
            command: Command::DismissDialog,
        }],
    }
}

fn review_has_content(review: &ReviewState) -> bool {
    !review.annotations.is_empty()
        || !review.views.is_empty()
        || review.visibility.hidden_own().next().is_some()
}

fn discard_dialog(doc: bool, review: bool, draft: bool) -> DialogRequest {
    let mut lost = Vec::new();
    if doc {
        lost.push("書き出していない形の編集");
    }
    if review {
        lost.push("保存していないレビュー");
    }
    if draft {
        lost.push("入力途中の注釈");
    }
    let title = if doc && !review && !draft {
        "書き出していない形の編集"
    } else if doc {
        "書き出していない形の編集と保存していない変更"
    } else {
        "保存していない変更"
    };
    let body = if lost.is_empty() {
        "保存していない変更があります。破棄して続けますか。".to_owned()
    } else {
        format!(
            "次の変更が失われます: {}。\n破棄して続けますか。",
            lost.join("、")
        )
    };
    DialogRequest {
        title: title.to_owned(),
        body,
        buttons: vec![
            DialogButton {
                id: ids::DIALOG_CONFIRM.to_owned(),
                label: "破棄して続ける".to_owned(),
                command: Command::ConfirmDiscard,
            },
            DialogButton {
                id: ids::DIALOG_CANCEL.to_owned(),
                label: "やめる".to_owned(),
                command: Command::CancelPending,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use crate::command::{Command, PanelKind};
    use crate::loader::{LoadPhase, LoadedModel};
    use model_reviewer_core::identity::{ModelIdentity, ModelRef};
    use model_reviewer_core::review::ReviewState;
    use model_reviewer_core::review_file::{self, ReviewNodeRecord};
    use model_reviewer_core::scene::NodeId;
    use model_reviewer_testdata::{inputs, sample_glb};

    use super::{PendingRestore, Session, SourceKind};

    fn loaded_model(name: &str, bytes: &[u8], generation: u64) -> LoadedModel {
        LoadedModel {
            generation,
            model: Arc::new(model_reviewer_gltf::load_glb(bytes).unwrap()),
            model_ref: ModelRef {
                file_name: name.to_owned(),
                path: PathBuf::from(name),
                relative_path: None,
                identity: ModelIdentity::from_bytes(bytes),
                stl_unit: None,
                stl_up: None,
            },
            source_kind: SourceKind::Glb,
            file_bytes: bytes.len() as u64,
            read: Duration::ZERO,
            identity: Duration::ZERO,
            parse: Duration::ZERO,
        }
    }

    #[test]
    fn invalid_review_never_queues_a_model_for_upload() {
        let mut session = Session::default();
        session.accept_loaded(loaded_model(
            "original.glb",
            &sample_glb(),
            session.generation,
        ));
        session.apply(Command::Select(Some(NodeId(1))));
        session.apply(Command::DuplicateSelected);
        assert!(session.doc_unsaved());
        let selected = session.selection.unwrap();
        let previous = Arc::clone(session.model.as_ref().unwrap());
        let nodes = session.doc.as_ref().unwrap().nodes().to_vec();
        let review = session.review.clone();

        session.enable_gpu_upload();
        session.generation += 1;
        session.load = LoadPhase::Parsing;
        let incoming = loaded_model("other.glb", &inputs::triangle(), session.generation);
        let state = ReviewState::new(&incoming.model);
        let json = review_file::to_json(&state, &incoming.model, &incoming.model_ref).unwrap();
        let mut file = review_file::parse(&json).unwrap();
        file.hidden_nodes.push(ReviewNodeRecord {
            gltf_index: 999,
            name: "存在しない部品".to_owned(),
        });
        session.restore = Some(PendingRestore {
            file,
            remaining: Vec::new(),
            first_mismatch: None,
        });
        session.accept_loaded(incoming);

        assert!(
            session.incoming_model().is_none(),
            "復元できないモデルが転送待ちになった"
        );
        assert_eq!(session.load, LoadPhase::Idle);
        assert!(Arc::ptr_eq(session.model.as_ref().unwrap(), &previous));
        assert_eq!(session.doc.as_ref().unwrap().nodes(), nodes);
        assert_eq!(session.selection, Some(selected));
        assert_eq!(session.review, review);
        assert!(session.doc_unsaved());
        assert_eq!(
            session.status_text,
            "モデルに無い部品です（gltf_index 999）"
        );
        assert!(
            session
                .dialog
                .as_ref()
                .unwrap()
                .body
                .contains(&session.status_text)
        );
    }

    #[test]
    fn unsaved_open_waits_for_confirm() {
        let mut session = Session::default();
        session.mark_review_unsaved();
        session.apply(Command::OpenModel(PathBuf::from("a.glb")));
        assert!(session.dialog.is_some());
        assert!(session.status_text.is_empty());
        session.apply(Command::CancelPending);
        assert!(session.dialog.is_none());
        assert!(session.unsaved());
        assert!(session.pending.is_none());
    }

    #[test]
    fn confirm_discard_continues_and_quit_is_one_shot() {
        let mut session = Session::default();
        session.mark_review_unsaved();
        session.apply(Command::LoadReview(PathBuf::from("a.json")));
        session.apply(Command::ConfirmDiscard);
        assert!(session.unsaved());
        let body = session.dialog.as_ref().expect("failure").body.clone();
        assert!(body.contains("レビュー") || body.contains("見つかりません"));

        session.mark_review_unsaved();
        assert!(session.on_close_requested());
        assert!(session.dialog.is_some());
        session.apply(Command::ConfirmDiscard);
        assert!(session.take_quit());
        assert!(!session.take_quit());
    }

    #[test]
    fn clean_close_is_not_cancelled() {
        let mut session = Session::default();
        assert!(!session.on_close_requested());
    }

    #[test]
    fn set_panel_and_poll() {
        let mut session = Session::default();
        session.apply(Command::SetPanel {
            panel: PanelKind::Details,
            open: true,
        });
        assert!(session.panel_details);
        assert!(!session.poll_loader());
        session.set_viewport([10, 20]);
        assert!(session.take_scene_dirty().size);
        assert!(!session.take_scene_dirty().any());
    }
}
