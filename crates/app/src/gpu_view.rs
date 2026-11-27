//! GPUとの接続。`SceneRenderer` が描いた画像を egui に登録し、中央の3Dビューへ貼る。
//!
//! - `logic` で `poll`（転送の続き）、`ui` の最後で `frame`（インスタンスの更新と描き直し）を呼ぶ。
//! - 3Dは `SceneDirty` のどれかが立ったフレームだけ描き直す。それ以外は前の画像を貼るだけ（要件9.6）。
//! - GPUが無い（`wgpu_render_state` が `None`）ときや、捕まえなかったGPUのエラーが出たときは、
//!   3Dビューに理由を出すだけで、他の機能は動かす（要件3.9）。

use std::sync::{Arc, Mutex};
use std::time::Instant;

use egui_wgpu::RenderState;
use model_reviewer_core::camera::grid_spacing;
use model_reviewer_core::scene::{Model, SceneRead};
use model_reviewer_core::visibility::Visibility;
use model_reviewer_render::{
    DEFAULT_BUDGET_BYTES, DragPreview, FrameParams, GridParams, SceneRenderer, UploadProgress,
};

use crate::session::Session;
use crate::theme;

/// 中央の3Dビューに貼る画像。`ui::viewport` が `scene_image` で受け取る。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneImage {
    pub texture: egui::TextureId,
    pub size_px: [u32; 2],
}

/// 3Dビューに出す知らせ（画像が無い理由）。
#[derive(Clone, Debug, Default, PartialEq)]
struct SceneNotice(Option<String>);

fn image_id() -> egui::Id {
    egui::Id::unique("model_reviewer.scene_image")
}

fn notice_id() -> egui::Id {
    egui::Id::unique("model_reviewer.scene_notice")
}

/// いま貼る画像。まだ一度も描いていなければ `None`。
pub fn scene_image(ctx: &egui::Context) -> Option<SceneImage> {
    ctx.data(|data| data.get_temp::<SceneImage>(image_id()))
}

/// 3Dの画像を `rect` に貼る。画像が無ければ背景を塗り、理由があれば中央に出す。
pub fn paint_scene(ui: &egui::Ui, rect: egui::Rect) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme::VIEW_BG);
    if let Some(image) = scene_image(ui.ctx()) {
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(image.texture, rect, uv, egui::Color32::WHITE);
    }
    let notice = ui
        .ctx()
        .data(|data| data.get_temp::<SceneNotice>(notice_id()))
        .and_then(|n| n.0);
    if let Some(text) = notice {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("✕ {text}"),
            egui::FontId::proportional(theme::BODY_SIZE),
            theme::ERROR,
        );
    }
}

struct GpuState {
    render_state: RenderState,
    renderer: SceneRenderer,
    texture: Option<egui::TextureId>,
    /// 表示中・転送中のモデル（`Session::model` との照合用）。
    shown: Option<Arc<Model>>,
    pending: Option<Arc<Model>>,
    /// 転送を始められなかったモデル。同じモデルで毎フレーム失敗し直さない。
    rejected: Option<Arc<Model>>,
    /// `poll` で表示中が入れ替わった。次の `frame` でインスタンスを作り、描き直す。
    swapped: bool,
    progress: Option<UploadProgress>,
    device_errors: Arc<Mutex<Vec<String>>>,
    /// 表示中のモデルへ送った、足した図形の形状（送った順）。モデルが替わると空にする。
    added: Vec<Arc<model_reviewer_core::scene::Primitive>>,
    /// 足した形状を送れなかった理由（次のフレームで3Dビューに出す）。
    add_error: Option<String>,
}

pub struct GpuView {
    state: Option<GpuState>,
    reason: Option<String>,
}

impl GpuView {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let Some(render_state) = cc.wgpu_render_state.clone() else {
            return Self {
                state: None,
                reason: Some("GPUが使えないため、3D表示はできません".to_owned()),
            };
        };
        let device_errors = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&device_errors);
        // wgpu は捕まえなかったエラーで既定ではパニックする。記録して3D表示だけ止め、モデルとレビューは残す。
        render_state
            .device
            .on_uncaptured_error(Arc::new(move |err| {
                log::error!("wgpu: {err}");
                if let Ok(mut errors) = sink.lock() {
                    errors.push(err.to_string());
                }
            }));
        let renderer = SceneRenderer::new(&render_state.device);
        Self {
            state: Some(GpuState {
                render_state,
                renderer,
                texture: None,
                shown: None,
                pending: None,
                rejected: None,
                swapped: false,
                progress: None,
                device_errors,
                added: Vec::new(),
                add_error: None,
            }),
            reason: None,
        }
    }

    pub(crate) fn has_device(&self) -> bool {
        self.state.is_some()
    }

    /// 線の表示（「面と線」「線」）が使えるか。GPUが無ければ偽。
    pub(crate) fn supports_lines(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(|state| state.renderer.supports_lines())
    }

    /// GPUが使えない、または使えなくなった理由。
    pub fn unavailable_reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    /// 転送中なら、その進み具合。
    pub fn upload_progress(&self) -> Option<UploadProgress> {
        self.state.as_ref().and_then(|state| state.progress)
    }

    /// 転送の続き（予算つき）と、GPUのエラーの取り込み。`logic` から毎回呼ぶ。
    /// 送り切った回に、描画器の表示中と `Session` のモデルを同じ呼び出しで入れ替える。
    pub fn poll(&mut self, session: &mut Session) {
        if self.state.is_none() {
            if session.has_incoming() {
                session.commit_pending(false);
            }
            let _ = session.take_cancel_upload();
            return;
        }
        let device_fault = self.state.as_mut().and_then(|state| {
            state
                .device_errors
                .lock()
                .map(|mut errors| std::mem::take(&mut *errors))
                .unwrap_or_default()
                .into_iter()
                .last()
        });
        if let Some(last) = device_fault {
            self.stop_for_device_error(session, &last);
            return;
        }
        let Some(state) = self.state.as_mut() else {
            return;
        };
        if session.take_cancel_upload() {
            state.renderer.cancel_upload();
            state.pending = None;
            state.progress = None;
        }
        if let Some(model) = session.incoming_model() {
            let same = state
                .pending
                .as_ref()
                .is_some_and(|pending| Arc::ptr_eq(pending, &model));
            if !same {
                let device = &state.render_state.device;
                match state.renderer.begin_upload(device, Arc::clone(&model)) {
                    Ok(()) => {
                        state.pending = Some(model);
                        state.rejected = None;
                        self.reason = None;
                    }
                    Err(err) => {
                        log::error!("begin_upload: {err}");
                        state.renderer.cancel_upload();
                        state.pending = None;
                        state.progress = None;
                        self.reason = Some(err.message_ja());
                        session.reject_upload(err.message_ja());
                        return;
                    }
                }
            }
        }
        if state.pending.is_none() {
            return;
        }
        let queue = &state.render_state.queue;
        let started = Instant::now();
        let progress = state.renderer.continue_upload(queue, DEFAULT_BUDGET_BYTES);
        session.record_upload_step(started.elapsed());
        if progress.done {
            state.shown = state.pending.take();
            state.added.clear();
            state.swapped = true;
            state.progress = None;
            session.commit_pending(true);
        } else {
            state.progress = Some(progress);
            session.set_upload_progress(progress.uploaded_bytes, progress.total_bytes);
        }
    }

    /// 転送が残っている、または入れ替えた直後で描き直しが要る。
    pub fn needs_repaint(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(|state| state.pending.is_some() || state.swapped)
    }

    /// `ui` の最後に呼ぶ。モデルの切り替え・インスタンスの更新・描き直しをして、画像を登録する。
    pub fn frame(&mut self, ctx: &egui::Context, session: &mut Session) {
        let dirty = session.take_scene_dirty();
        // 確定した変形で動いた範囲。全体を作り直すフレームでは使わない。
        let patches = session.take_instance_patches();
        let (device_fault, alloc_message) = {
            let Some(state) = self.state.as_mut() else {
                publish_notice(ctx, self.reason.clone());
                return;
            };
            let device = &state.render_state.device;
            let queue = &state.render_state.queue;

            let mut changed = false;
            // 最初の1つを転送している間は、表示中のモデルがまだ無い。そのときは消さない
            // （消すと転送が毎回やり直しになり、読み込みが終わらない）。
            if session.model.is_none()
                && !session.has_incoming()
                && (state.shown.is_some() || state.pending.is_some())
            {
                state.renderer.clear_model();
                state.shown = None;
                state.pending = None;
                state.progress = None;
                state.added.clear();
                changed = true;
            }
            let uploading = state.pending.is_some();
            let aligned = match (session.model.as_ref(), state.shown.as_ref()) {
                (Some(session_model), Some(shown)) => Arc::ptr_eq(session_model, shown),
                (None, None) => true,
                _ => false,
            };
            if !aligned && !uploading {
                log::error!("表示中のモデルと Session のモデルが一致しません");
            }

            let draws_before = state.renderer.stats().draw_count;
            let swapped = std::mem::take(&mut state.swapped);
            if let Some(model) = state.shown.clone()
                && (swapped || dirty.instances || dirty.model)
            {
                // 部品の表と表示状態は Session の文書のもの。表示中が別のモデル（入れ替えの途中）なら、
                // そのモデルの読み込んだ時点の表で全部を表示する。
                let same_model = session
                    .model
                    .as_ref()
                    .is_some_and(|m| Arc::ptr_eq(m, &model));
                if same_model && let Some(doc) = session.doc.as_ref() {
                    upload_added(
                        &mut state.renderer,
                        &mut state.added,
                        &mut state.add_error,
                        (device, queue),
                        doc,
                    );
                }
                match (session.doc.as_ref(), session.review.as_ref()) {
                    (Some(doc), Some(review)) if same_model => {
                        state
                            .renderer
                            .set_instances(device, queue, doc, &review.visibility);
                    }
                    (Some(doc), None) if same_model => {
                        let all = Visibility::all_visible(doc.node_count());
                        state.renderer.set_instances(device, queue, doc, &all);
                    }
                    _ => {
                        let all = Visibility::all_visible(model.node_count());
                        state.renderer.set_instances(device, queue, &model, &all);
                    }
                }
                changed = true;
            } else if !patches.is_empty()
                && let (Some(model), Some(doc)) = (state.shown.as_ref(), session.doc.as_ref())
                && session
                    .model
                    .as_ref()
                    .is_some_and(|m| Arc::ptr_eq(m, model))
            {
                // 部品の数と表示は変わっていないので、動かした範囲のインスタンスだけを書き直す。
                for range in patches {
                    state.renderer.update_instances(queue, doc, range);
                }
                changed = true;
            }

            let should_draw = changed || swapped || dirty.any() || state.texture.is_none();
            if should_draw {
                let params = frame_params(session, state.shown.as_ref());
                let output = state.renderer.render(device, queue, &params);
                let recreated = output.recreated;
                let mut egui_renderer = state.render_state.renderer.write();
                match state.texture {
                    None if recreated => {
                        state.texture = Some(egui_renderer.register_native_texture(
                            device,
                            output.view,
                            egui_wgpu::wgpu::FilterMode::Linear,
                        ));
                    }
                    Some(id) if recreated => egui_renderer.update_egui_texture_from_wgpu_texture(
                        device,
                        output.view,
                        egui_wgpu::wgpu::FilterMode::Linear,
                        id,
                    ),
                    _ => {}
                }
                // 描画中の未捕捉エラーは、次の poll まで届かない。ここで取り出して次のフレームで見せる。
                let _ = device.poll(egui_wgpu::wgpu::PollType::Poll);
            }
            if state.renderer.stats().draw_count > draws_before && aligned {
                session.note_presented();
            }
            let device_fault = state
                .device_errors
                .lock()
                .map(|mut errors| std::mem::take(&mut *errors))
                .unwrap_or_default()
                .into_iter()
                .last();
            let add_error = state.add_error.take();
            let alloc_message = if device_fault.is_some() {
                let _ = state.renderer.take_error();
                None
            } else {
                state
                    .renderer
                    .take_error()
                    .map(|err| {
                        let message = err.message_ja();
                        log::error!("render error: {message}");
                        message
                    })
                    .or(add_error)
            };
            (device_fault, alloc_message)
        };
        let mut surfaced = false;
        if let Some(last) = device_fault {
            self.stop_for_device_error(session, &last);
            surfaced = true;
        } else if let Some(message) = alloc_message {
            // 確保の失敗は前の描画先とインスタンスを残している。モデルは閉じない。
            self.reason = Some(message);
            surfaced = true;
        }

        let texture = self.state.as_ref().and_then(|state| state.texture);
        if let Some(texture) = texture {
            let image = SceneImage {
                texture,
                size_px: session.viewport_px,
            };
            ctx.data_mut(|data| data.insert_temp(image_id(), image));
        }
        publish_notice(ctx, self.reason.clone());
        // 転送中・入れ替え直後・いま出したエラーのときだけ次を予約する。空のフレームでは予約しない。
        if self.needs_repaint() || surfaced {
            ctx.request_repaint();
        }
    }

    /// 捕まえなかったGPUエラー。3D表示だけを止め、次のフレームで理由を出す。
    fn stop_for_device_error(&mut self, session: &mut Session, detail: &str) {
        let message = format!("GPUのエラーのため3D表示を止めました（{detail}）");
        if let Some(state) = self.state.as_mut() {
            state.renderer.clear_model();
            state.shown = None;
            state.pending = None;
            state.added.clear();
            state.rejected = None;
            state.progress = None;
            state.swapped = true;
        }
        self.reason = Some(message.clone());
        session.close_for_gpu_error(message);
    }

    /// 描いた回数と送ったバイト数（試験と詳細表示用）。
    pub fn stats(&self) -> Option<model_reviewer_render::RenderStats> {
        self.state.as_ref().map(|state| state.renderer.stats())
    }
}

impl Drop for GpuView {
    fn drop(&mut self) {
        if let Some(state) = self.state.as_mut()
            && let Some(id) = state.texture.take()
        {
            state.render_state.renderer.write().free_texture(&id);
        }
    }
}

/// 文書に足した図形の形状のうち、まだGPUへ送っていない分を、足した順に送る。描画器は
/// `primitive_slot_count()` の続きの番号を付けるので、文書の `PrimId` とそろう。送れなかったら
/// （32MiB超など）理由を残し、その形状の部品は描かれない（インスタンスの作成が飛ばす）。
fn upload_added(
    renderer: &mut SceneRenderer,
    sent_list: &mut Vec<Arc<model_reviewer_core::scene::Primitive>>,
    add_error: &mut Option<String>,
    (device, queue): (&egui_wgpu::wgpu::Device, &egui_wgpu::wgpu::Queue),
    doc: &model_reviewer_core::doc::Document,
) {
    let base = doc.base().primitives.len();
    let added = doc.added_primitives();
    let slots = renderer.primitive_slot_count();
    let sent = slots.saturating_sub(base);
    // 送った分が文書の足した形状と同じ物か（同じ順に同じ `Arc`）。違えば番号がずれている。
    let aligned = sent_list.len() == sent
        && added.len() >= sent
        && sent_list
            .iter()
            .zip(added)
            .all(|(sent, now)| Arc::ptr_eq(sent, now));
    if !aligned {
        log::error!(
            "足した形状の番号が描画器とそろっていません（送った {sent}、文書 {}）",
            added.len()
        );
        return;
    }
    if added.len() <= sent {
        return;
    }
    let fresh = &added[sent..];
    match renderer.add_primitives(device, queue, fresh) {
        Ok(()) => sent_list.extend(fresh.iter().cloned()),
        Err(err) => {
            log::error!("add_primitives: {err}");
            *add_error = Some(err.message_ja());
        }
    }
}

fn publish_notice(ctx: &egui::Context, reason: Option<String>) {
    ctx.data_mut(|data| data.insert_temp(notice_id(), SceneNotice(reason)));
}

fn frame_params(session: &Session, shown: Option<&Arc<Model>>) -> FrameParams {
    let [w, h] = session.viewport_px;
    let aspect = if h > 0 { w as f32 / h as f32 } else { 1.0 };
    let camera = &session.camera;
    // 選択は Session の文書の NodeId。表示中が別のモデル（転送中）なら強調しない。
    let same_model = match (session.model.as_ref(), shown) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
    };
    let doc = session.doc.as_ref().filter(|_| same_model);
    let bounds = match (doc, shown) {
        (Some(doc), _) => doc.bounds(),
        (None, Some(model)) => model.bounds(),
        (None, None) => model_reviewer_core::math::Aabb::empty(),
    };
    let selected = match (session.selection, doc) {
        (Some(id), Some(doc)) if (id.0 as usize) < doc.node_count() => Some(doc.subtree(id)),
        _ => None,
    };
    FrameParams {
        size_px: session.viewport_px,
        view_proj: camera.proj(aspect, &bounds) * camera.view(),
        eye: camera.eye(),
        selected,
        clear: theme::view_clear_linear(),
        display: session.display,
        grid: GridParams {
            spacing: grid_spacing(camera.distance),
            // モデルが無い間は格子を描かない。起動直後の視点は床の面の上にあり、格子を描くと
            // 画面全体が軸の色の模様になる。
            visible: session.grid_visible && session.model.is_some(),
        },
        drag: match (session.transform.as_ref(), doc) {
            (Some(transform), Some(doc)) if transform.nodes.end as usize <= doc.node_count() => {
                Some(DragPreview {
                    nodes: transform.nodes.clone(),
                    world_delta: glam::Mat4::from(transform.delta),
                })
            }
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::pin::pin;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};

    use egui_wgpu::wgpu;
    use egui_wgpu::{RenderState, Renderer, RendererOptions};

    use super::{GpuState, GpuView, frame_params};
    use crate::session::Session;

    /// 何も開いていない間は床の格子を描かない。モデルがあれば描く。
    #[test]
    fn grid_is_hidden_until_a_model_exists() {
        let mut session = Session::default();
        session.viewport_px = [64, 48];
        assert!(session.grid_visible);
        assert!(!frame_params(&session, None).grid.visible);
        let model = model_reviewer_gltf::load_glb(&model_reviewer_testdata::sample_glb());
        session.model = Some(Arc::new(model.expect("見本")));
        assert!(frame_params(&session, None).grid.visible);
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        let mut future = pin!(future);
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
                return value;
            }
            std::thread::yield_now();
        }
    }

    /// テクスチャができたあとの、汚れの無いフレームは描かず、再描画も予約しない。
    #[test]
    #[ignore = "GPUが必要"]
    fn idle_frame_does_not_draw_or_request_repaint() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("adapter");
        let (device, queue) =
            block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device");
        let device_errors = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&device_errors);
        device.on_uncaptured_error(Arc::new(move |err| {
            if let Ok(mut errors) = sink.lock() {
                errors.push(err.to_string());
            }
        }));
        let egui_renderer = Renderer::new(
            &device,
            wgpu::TextureFormat::Bgra8Unorm,
            RendererOptions::default(),
        );
        let render_state = RenderState {
            adapter,
            available_adapters: Vec::new(),
            instance,
            device,
            queue,
            target_format: wgpu::TextureFormat::Bgra8Unorm,
            renderer: Arc::new(egui::mutex::RwLock::new(egui_renderer)),
        };
        let renderer = model_reviewer_render::SceneRenderer::new(&render_state.device);
        let mut gpu = GpuView {
            state: Some(GpuState {
                render_state,
                renderer,
                texture: None,
                shown: None,
                pending: None,
                rejected: None,
                swapped: false,
                progress: None,
                device_errors,
                added: Vec::new(),
                add_error: None,
            }),
            reason: None,
        };
        let mut session = Session::default();
        session.viewport_px = [64, 48];
        let ctx = egui::Context::default();
        // 新しいビューポートは outstanding が 1 で has_requested_repaint が真になる。
        // 自分が出した要求だけを数える。
        let repaints = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&repaints);
        ctx.set_request_repaint_callback(move |_| {
            counter.fetch_add(1, Ordering::Relaxed);
        });
        gpu.frame(&ctx, &mut session);
        let draws = gpu.stats().expect("stats").draw_count;
        assert_eq!(draws, 1, "最初のフレームは描画先が無いので1回描く");
        assert!(
            gpu.reason.is_none(),
            "空のフレームでエラーを出さない: {:?}",
            gpu.reason
        );
        assert_eq!(repaints.load(Ordering::Relaxed), 0);
        gpu.frame(&ctx, &mut session);
        assert_eq!(gpu.stats().expect("stats").draw_count, draws);
        assert_eq!(repaints.load(Ordering::Relaxed), 0);
    }
}
