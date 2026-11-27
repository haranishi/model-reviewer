//! 画面が読む状態。書き換えは `apply` だけが行う。

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
use model_reviewer_core::units::{UnitLabel, Units};
use model_reviewer_render::DisplayMode;

use crate::command::{Command, DialogButton, DialogRequest, PanelKind, SceneDirty};
use crate::exporter::{ExportHandle, ExportHooks, ExportStage};
use crate::loader::{self, LoadEvent, LoadHandle, LoadHooks, LoadOptions, LoadPhase, LoadedModel};
use crate::metrics::Metrics;
use crate::ui::ids;

mod camera;
mod dims;
mod edit;
mod export;
mod measure;
mod notes;
mod open;
mod parts;
mod persist;
mod pick;
mod section;
mod snap;
mod view;

pub use dims::{DimsView, ShapeDims, selection_dims};
pub use edit::{
    GIZMO_SIZE, Transform, TransformSource, gizmo_frame, moved_frame, tool_edits, tool_has_gizmo,
};
pub use export::{export_scene, note_ja, stl_export_note};
pub use measure::{delta as measure_delta, distance as measure_distance};
pub use notes::Draft;
pub use pick::ray_hit;
pub use section::section_bounds;
pub use snap::{SNAP_RADIUS_POINTS, find as find_snap, rest_after as snap_rest_after};

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
    /// STL。`unit` は座標の単位の仮定。
    Stl { unit: UnitLabel },
}

#[derive(Clone, Debug, PartialEq)]
enum Pending {
    OpenModel(PathBuf, LoadOptions),
    LoadReview(PathBuf),
    Quit,
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
    /// 装置が線の表示（`POLYGON_MODE_LINE`）を持つか。GPUが無いときは偽。
    pub lines_supported: bool,
    pub selection: Option<NodeId>,
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
    /// 確定した変形で `world` が変わった範囲。`gpu_view` が次のフレームで取り出して部分更新する。
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
    loader: Option<LoadHandle>,
    load_meta: Option<LoadMeta>,
    incoming: Option<Incoming>,
    staged_pick: Option<StagedPick>,
    load_hooks: LoadHooks,
    /// 実行中のGLB書き出し。
    pub(super) export_job: Option<ExportJob>,
    pub(super) export_hooks: ExportHooks,
    /// 直近の書き出しの失敗理由（自動操作の `wait_idle` 用）。次の書き出し・読み込みで消える。
    pub(super) export_error: Option<String>,
    /// 直近の状態文が書き出し失敗のものか（状態欄を赤字にするため）。
    pub(super) export_failed: bool,
    /// この作業で「開く」から開いたファイル。書き出し先にできない。
    pub(super) opened_files: Vec<PathBuf>,
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
            lines_supported: false,
            selection: None,
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
            load_meta: None,
            incoming: None,
            staged_pick: None,
            load_hooks: LoadHooks::default(),
            export_job: None,
            export_hooks: ExportHooks::default(),
            export_error: None,
            export_failed: false,
            opened_files: Vec::new(),
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
        }
    }
}

struct LoadMeta {
    generation: u64,
    source_path: PathBuf,
    forbid_on_commit: bool,
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
    model_ref: ModelRef,
    source_kind: SourceKind,
    resolved_path: PathBuf,
    source_path: Option<PathBuf>,
    forbid_on_commit: bool,
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
        self.apply_inner(cmd);
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
                if self.is_exporting() {
                    self.status_text =
                        "書き出しが終わってからレビューを保存してください".to_owned();
                } else if self.doc_unsaved() {
                    self.dialog = Some(export::export_first_dialog());
                    self.status_text = "先にGLBを書き出してください".to_owned();
                }
            }
            Command::ExportGlbDialog => {
                if self.is_exporting() {
                    self.status_text = "すでに書き出し中です".to_owned();
                } else if self.is_loading_replacement() {
                    self.block_edit_if_locked();
                } else if self.doc.is_none() || self.model_ref.is_none() {
                    self.status_text = "モデルを開くと書き出せます".to_owned();
                }
            }
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
            Command::Home => camera::home(self),
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
                self.dialog = None;
                self.restore = None;
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
            Command::SetTool(tool) => edit::set_tool(self, tool),
            Command::BeginTransform { handle, ndc } => edit::begin_transform(self, handle, ndc),
            Command::DragTransform { ndc } => edit::drag_transform(self, ndc),
            Command::CommitTransform => edit::commit_transform(self),
            Command::CancelTransform => edit::cancel_transform(self),
            Command::PreviewLocal(trs) => edit::preview_local(self, trs),
            Command::CommitLocal(trs) => edit::commit_local(self, trs),
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
            Command::SetStlUnit(unit) => set_stl_unit(self, unit),
        }
    }

    pub fn poll_loader(&mut self) -> bool {
        let Some(handle) = self.loader.as_ref() else {
            return false;
        };
        let events = handle.drain();
        if events.is_empty() {
            return false;
        }
        for event in events {
            self.apply_load_event(event);
        }
        true
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

    /// 部品を読む口。モデルが無ければ `None`。
    pub fn scene(&self) -> Option<&Document> {
        self.doc.as_ref()
    }

    /// モデルを置き、その文書を作る。読み込み（`commit_pending`）と、読み込みを通さない試験が使う。
    /// レビュー・選択・視点には触れない。
    pub fn set_model(&mut self, model: Arc<Model>) {
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

    /// `Command::OpenModel` と同じ（未保存の確認を通る）。STL の単位の仮定を指定できる
    /// （自動操作の `open_model` の `stl_unit`）。
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
                    let projection = self.camera.projection;
                    self.camera = OrbitCamera::home(&bounds, aspect_of(size_px));
                    self.camera.projection = projection;
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

    /// 確定した変形で動いた範囲（部分更新の対象）。取り出すと空になる。
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
            self.dialog = Some(exporting_close_dialog());
            self.status_text = "書き出し中は閉じられません".to_owned();
            return true;
        }
        if !self.needs_confirm() || self.quit_after_confirm {
            return false;
        }
        if self.pending.is_none() {
            self.pending = Some(Pending::Quit);
            self.dialog = Some(self.discard_dialog());
        }
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
            Some(Pending::OpenModel(path, options)) => open::open_model(self, path, options),
            Some(Pending::LoadReview(path)) => open::load_review(self, path),
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

    pub(super) fn start_model_load(&mut self, path: PathBuf, options: LoadOptions) {
        self.start_model_load_with_expected(path, None, true, options);
    }

    pub(super) fn start_model_load_for_restore(&mut self, path: PathBuf, expected: ModelIdentity) {
        let options = self.restore_options();
        self.start_model_load_with_expected(path, Some(expected), true, options);
    }

    /// 復元のときの読み込みの指定。レビューの `stl_unit`（無ければ mm）で STL を開く（要件8.5）。
    fn restore_options(&self) -> LoadOptions {
        LoadOptions {
            stl_unit: self
                .restore
                .as_ref()
                .and_then(|pending| pending.file.model.stl_unit),
        }
    }

    /// 書き出したファイルへのレビュー復元（未書き出し編集を破棄して開き直す場合）。
    /// 書き出したファイル自体は「開く」で開いた扱いにしない。
    pub(super) fn start_model_load_for_exported_restore(
        &mut self,
        path: PathBuf,
        expected: ModelIdentity,
    ) {
        let options = self.restore_options();
        self.start_model_load_with_expected(path, Some(expected), false, options);
    }

    fn start_model_load_with_expected(
        &mut self,
        path: PathBuf,
        expected_identity: Option<ModelIdentity>,
        forbid_on_commit: bool,
        options: LoadOptions,
    ) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(active) = self.loader.take() {
            active.cancel();
        }
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
            forbid_on_commit,
        });
        let hooks = self.load_hooks.clone();
        let ctx = self.egui_ctx.clone();
        self.loader = Some(loader::spawn(
            generation,
            path,
            expected_identity,
            options,
            hooks,
            ctx,
        ));
    }

    pub(super) fn cancel_pending_load(&mut self) {
        if self.shown_generation != self.generation {
            self.generation = self.generation.wrapping_add(1);
            if let Some(active) = self.loader.take() {
                active.cancel();
            }
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
                persist::on_candidate_mismatch(self, actual);
            }
            LoadEvent::Failed {
                generation,
                message,
            } => {
                if generation != self.generation {
                    return;
                }
                if self.recover_pick_after_shown(generation) {
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
        }
    }

    fn accept_loaded(&mut self, loaded: LoadedModel) {
        if loaded.generation != self.generation {
            return;
        }
        if persist::reject_mismatch(self, &loaded.model_ref) {
            self.open_started = None;
            self.awaiting_present = false;
            self.discard_bypass = false;
            self.load = LoadPhase::Idle;
            return;
        }
        // 転送が終わると描画器の表示が替わる。復元に失敗しうる処理は転送前に済ませる。
        let doc = Document::new(loaded.model);
        let review = match persist::take_restored(self, &doc, &loaded.model_ref.identity) {
            persist::Restored::Absent => None,
            persist::Restored::State(state) => Some(state),
            persist::Restored::Blocked => {
                self.cancel_pending_load();
                self.end_discard_bypass();
                return;
            }
        };
        let (source_path, forbid_on_commit) = match self
            .load_meta
            .as_ref()
            .filter(|meta| meta.generation == loaded.generation)
        {
            Some(meta) => (Some(meta.source_path.clone()), meta.forbid_on_commit),
            None => (None, true),
        };
        self.incoming = Some(Incoming {
            generation: loaded.generation,
            doc,
            review,
            model_ref: loaded.model_ref,
            source_kind: loaded.source_kind,
            resolved_path: loaded.resolved_path,
            source_path,
            forbid_on_commit,
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

    /// 表示まで成功したあとの索引づくりが落ちたときだけ、UIスレッドで作り直す。
    /// 読み込みそのものの失敗では呼ばない（`shown_generation` がまだ古い）。
    fn recover_pick_after_shown(&mut self, generation: u64) -> bool {
        if self.shown_generation != generation || self.pick_index.is_some() {
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
        if generation != self.generation {
            return;
        }
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
        let projection = self.camera.projection;
        self.camera = OrbitCamera::home(&doc.bounds(), aspect);
        self.camera.projection = projection;
        self.home_on_resize = !viewport_known;
        if incoming.forbid_on_commit {
            if let Some(source) = incoming.source_path.as_deref() {
                self.remember_opened_file(source);
            }
            self.remember_opened_file(&incoming.resolved_path);
        }
        self.exported_since_load = false;
        self.export_error = None;
        self.export_failed = false;
        self.model_ref = Some(incoming.model_ref);
        self.review = Some(review.unwrap_or_else(|| ReviewState::new(&doc)));
        self.selection = None;
        self.selected_annotation = None;
        self.annotate_mode = false;
        if self.tool == Tool::Annotate {
            self.tool = Tool::Select;
        }
        self.pick_index = None;
        self.shown_generation = incoming.generation;
        self.doc = Some(doc);
        self.model = Some(model);
        self.measurement.clear();
        self.snap_preview = None;
        self.snap_rest_until = None;
        section::reset(self);
        // GLB を開くと、ファイルの単位は m、表示は「ファイルと同じ」（要件7.5）。STL は部品の大きさで
        // メートルに合わせてあるので、シーンの単位は m、表示は STL の単位の仮定（要件7.6）。
        self.source_kind = incoming.source_kind;
        self.units = match incoming.source_kind {
            SourceKind::Glb => Units::default(),
            SourceKind::Stl { unit } => Units {
                source: UnitLabel::M,
                display: Some(unit),
            },
        };
        self.recount_triangles();
        self.reset_history();
        self.discard_bypass = false;
        self.loading_file_name = None;
        self.load = LoadPhase::Idle;
        self.status_text = if restored_review {
            "レビューを復元しました".to_owned()
        } else {
            "読み込み完了".to_owned()
        };
        self.dialog = None;
        self.scene_dirty.model = true;
        self.scene_dirty.instances = true;
        self.scene_dirty.selection = true;
        self.scene_dirty.camera = true;
        self.metrics.file_bytes = Some(incoming.file_bytes);
        self.metrics.read = Some(incoming.read);
        self.metrics.identity = Some(incoming.identity);
        self.metrics.parse = Some(incoming.parse);
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

/// STL の単位の仮定を替える（要件7.7）。同じファイルを新しい単位で開き直す。別のモデルを開くときと
/// 同じ経路（書き出し中は断る・未保存の確認・読み込み中の表示・失敗しても前のモデルが残る）を通す。
fn set_stl_unit(session: &mut Session, unit: UnitLabel) {
    let SourceKind::Stl { unit: current } = session.source_kind else {
        return;
    };
    if current == unit {
        return;
    }
    if session.is_loading_replacement() {
        session.block_edit_if_locked();
        return;
    }
    let Some(path) = session
        .model_ref
        .as_ref()
        .map(|model_ref| PathBuf::from(&model_ref.path))
    else {
        return;
    };
    open::open_guarded(
        session,
        path,
        LoadOptions {
            stl_unit: Some(unit),
        },
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
        | Command::Undo
        | Command::Redo
        | Command::AddShape(_)
        | Command::DuplicateSelected
        | Command::DeleteSelected
        | Command::ExportGlb(_)
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
            resolved_path: PathBuf::from(name),
            model_ref: ModelRef {
                file_name: name.to_owned(),
                path: name.to_owned(),
                relative_path: None,
                identity: ModelIdentity::from_bytes(bytes),
                stl_unit: None,
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
