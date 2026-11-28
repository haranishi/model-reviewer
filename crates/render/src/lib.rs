//! シーンの描画。3Dはオフスクリーンの画像へ描き、アプリはそれを egui の画像として貼る（design-render.md）。
//!
//! GPU資源はすべて `SceneRenderer` が持つ。モデルは「表示中」と「転送中」の2組を持ち、転送を送り切った回に
//! 入れ替えて古い組を解放する。転送の失敗・中止・新しい `begin_upload` では転送中の組だけを捨てるので、
//! 表示中のモデルは残る（要件2.7）。

use std::collections::VecDeque;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use glam::{Affine3A, Mat4};
use model_reviewer_core::scene::{Model, Primitive, SceneRead};
use model_reviewer_core::texture::{ImageId, MipChain, UvMap};
use model_reviewer_core::visibility::Visibility;

pub mod display;
pub mod grid;
mod image_upload;
pub mod instances;
mod outline;
mod pipelines;
pub mod plan;
pub mod targets;
mod textures;
pub mod upload;

pub use display::{DisplayMode, DragPreview};
pub use grid::GridParams;
pub use image_upload::ImageProgress;
pub use plan::{BufferPlan, FrameParams, PagePlan, PrimSlot, plan_buffers};
pub use upload::{DEFAULT_BUDGET_BYTES, UploadProgress};

use instances::InstanceList;
use outline::{OutlineResources, SelectionCache};
use pipelines::{FrameUniform, Layouts, Pipelines, XRAY_ALPHA, display_code};
use targets::{MSAA_SAMPLES, Targets, scoped};
use textures::GpuTextures;
use upload::{GpuGeometry, GpuPage, UploadJob};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderStats {
    /// 3Dを描いた回数（`render` が描画命令を送った回数）。
    pub draw_count: u64,
    /// GPUへ送ったバイト数の合計（形状とインスタンス）。
    pub uploaded_bytes: u64,
    /// インスタンスバッファを作り直した回数。
    pub instance_rebuilds: u64,
    /// 描画の定数として送ったバイト数の合計（`render` 1回につき定数1つ）。`uploaded_bytes` には含めない。
    pub frame_bytes: u64,
    /// 確定した変形の部分更新で、インスタンスを書き直した回数（`update_instances`）。
    pub instance_patches: u64,
    /// 表示中のモデルで転送を終えた画像の合計（全縮小版を含む RGBA8 の計算値）。
    pub texture_bytes: u64,
    /// 転送を終えた画像の枚数。白の代用は含まない。
    pub texture_count: u32,
    /// 転送済みの画像を使って発行した面の描画命令の累計。
    pub textured_draws: u64,
}

/// `add_primitives` の1回の上限。第1段階の1フレームの転送の予算と同じ。
pub const ADD_PRIMITIVES_LIMIT_BYTES: u64 = DEFAULT_BUDGET_BYTES;

pub struct RenderOutput<'a> {
    pub view: &'a wgpu::TextureView,
    pub recreated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    #[error("limit exceeded: {what} needed {needed} limit {limit}")]
    LimitExceeded {
        what: String,
        needed: u64,
        limit: u64,
    },
    #[error("out of memory")]
    OutOfMemory,
    #[error("device: {0}")]
    Device(String),
}

impl RenderError {
    pub fn message_ja(&self) -> String {
        match self {
            Self::LimitExceeded {
                what,
                needed,
                limit,
            } => format!("{what} が上限を超えています（{needed} / {limit}）"),
            Self::OutOfMemory => "GPUのメモリが足りません".to_owned(),
            Self::Device(detail) => format!("描画のエラー: {detail}"),
        }
    }
}

struct GpuInstances {
    buffer: wgpu::Buffer,
    capacity: u64,
    list: InstanceList,
    generation: u64,
}

struct Shown {
    geometry: GpuGeometry,
    textures: GpuTextures,
    instances: Option<GpuInstances>,
    /// `add_primitives` で足した形状のバイト数（足したページは `geometry.pages` の末尾にある）。
    added_bytes: u64,
}

impl Shown {
    fn destroy(&self) {
        self.geometry.destroy();
        if let Some(instances) = &self.instances {
            instances.buffer.destroy();
        }
    }
}

/// GPUで未完了の描画を、この回数より多く溜めない。
const MAX_FRAMES_IN_FLIGHT: usize = 2;

pub struct SceneRenderer {
    limits: wgpu::Limits,
    layouts: Layouts,
    pipelines: Option<Pipelines>,
    targets: Option<Targets>,
    outline: Option<OutlineResources>,
    srgb_supported: bool,
    /// 装置が `POLYGON_MODE_LINE` を持つか。偽なら線のパイプラインを作らない。
    lines_supported: bool,
    textures_shown: bool,
    frame_uniform: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    placeholder: wgpu::Texture,
    placeholder_view: wgpu::TextureView,
    shown: Option<Shown>,
    pending: Option<UploadJob>,
    selection: SelectionCache,
    instance_generation: u64,
    stats: RenderStats,
    last_error: Option<RenderError>,
    /// 送った描画のうち、完了を確かめていないもの（古い順）。
    in_flight: VecDeque<wgpu::SubmissionIndex>,
}

impl SceneRenderer {
    pub fn new(device: &wgpu::Device) -> Self {
        let layouts = Layouts::new(device);
        let frame_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame uniform"),
            size: std::mem::size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &layouts.frame,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_uniform.as_entire_binding(),
            }],
        });
        let placeholder = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("placeholder"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: targets::color_format(),
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let placeholder_view = placeholder.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            limits: device.limits(),
            layouts,
            pipelines: None,
            targets: None,
            outline: None,
            srgb_supported: true,
            textures_shown: true,
            lines_supported: device
                .features()
                .contains(wgpu::Features::POLYGON_MODE_LINE),
            frame_uniform,
            frame_bind_group,
            placeholder,
            placeholder_view,
            shown: None,
            pending: None,
            selection: SelectionCache::default(),
            instance_generation: 0,
            in_flight: VecDeque::new(),
            stats: RenderStats::default(),
            last_error: None,
        }
    }

    /// 転送中の組を作る。表示中の組は残す。前の転送中の組は捨てる。
    pub fn begin_upload(
        &mut self,
        device: &wgpu::Device,
        model: Arc<Model>,
    ) -> Result<(), RenderError> {
        self.cancel_upload();
        let plan = plan_buffers(&model, &self.limits)?;
        self.pending = Some(UploadJob::new(device, model, plan)?);
        Ok(())
    }

    /// 予算まで送る。送り切った回に転送中の組を表示中にし、古い組を解放する。
    /// 呼び出し側は、`done` になった回に `set_instances` を呼ぶ。
    pub fn continue_upload(&mut self, queue: &wgpu::Queue, budget_bytes: u64) -> UploadProgress {
        let Some(job) = self.pending.as_mut() else {
            return UploadProgress {
                uploaded_bytes: 0,
                total_bytes: 0,
                done: true,
            };
        };
        let sent = job.step(queue, budget_bytes);
        self.stats.uploaded_bytes += sent;
        let progress = job.progress();
        if progress.done
            && let Some(job) = self.pending.take()
        {
            if let Some(old) = self.shown.take() {
                old.destroy();
            }
            self.selection.clear();
            self.shown = Some(Shown {
                geometry: job.geometry,
                textures: GpuTextures::default(),
                instances: None,
                added_bytes: 0,
            });
        }
        // write_buffer の退避メモリは submit まで残る。分割した回ごとに空の submit で手放す。
        // 送るものが無い回（pending が無い早期 return）では submit しない。
        if sent > 0 {
            queue.submit(std::iter::empty::<wgpu::CommandBuffer>());
        }
        progress
    }

    /// 転送中の組だけを捨てる。
    pub fn cancel_upload(&mut self) {
        if let Some(job) = self.pending.take() {
            job.geometry.destroy();
        }
    }

    /// 表示中と転送中の両方を捨てる。
    pub fn clear_model(&mut self) {
        self.cancel_upload();
        if let Some(old) = self.shown.take() {
            old.destroy();
        }
        self.selection.clear();
    }

    /// 転送中のモデルがあるか。
    pub fn is_uploading(&self) -> bool {
        self.pending.is_some()
    }

    /// 表示中のモデル。
    pub fn shown_model(&self) -> Option<&Arc<Model>> {
        self.shown.as_ref().map(|shown| &shown.geometry.model)
    }

    /// 同じモデルから文書を作り直すと、追加分の画像・貼り方の番号は再利用される。
    pub fn discard_texture_edits(&mut self) {
        if let Some(shown) = &mut self.shown {
            shown
                .textures
                .discard_edits(shown.geometry.model.textures.images.len());
        }
    }

    /// 表示中のモデルへ1枚送る。同じ番号は置き換え、別の画像の転送中はエラーを返す。
    pub fn begin_image(
        &mut self,
        device: &wgpu::Device,
        id: ImageId,
        chain: MipChain,
    ) -> Result<(), RenderError> {
        let shown = self
            .shown
            .as_mut()
            .ok_or_else(|| RenderError::Device("画像を送る先のモデルがありません".into()))?;
        shown.textures.begin(device, id, chain)
    }

    /// 形状を送った後の残り予算で呼ぶ。4バイト未満の回は次の画素を送らない。
    pub fn continue_images(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        budget_bytes: u64,
    ) -> ImageProgress {
        self.shown
            .as_mut()
            .map_or_else(ImageProgress::default, |shown| {
                shown
                    .textures
                    .step(device, queue, &self.layouts.texture, budget_bytes)
            })
    }

    /// 次の描画から面の画像を表示するか。GPU に置いた画像は再利用する。
    pub fn set_textures_shown(&mut self, shown: bool) {
        self.textures_shown = shown;
    }

    /// 表示状態からインスタンスを作り直す。`model` は表示中のモデル（またはそれを土台にした文書）で
    /// なければならない（違えば何もしない）。
    pub fn set_instances(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        model: &impl SceneRead,
        vis: &Visibility,
    ) {
        self.set_instances_with(device, queue, model, &|id| vis.effective(id));
    }

    /// `set_instances` の本体。表示の判定を関数で受け取る。
    pub fn set_instances_with(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        model: &impl SceneRead,
        visible: &dyn Fn(model_reviewer_core::scene::NodeId) -> bool,
    ) {
        let Some(shown) = self.shown.as_mut() else {
            return;
        };
        if !model.shares_geometry_with(&shown.geometry.model) {
            log::warn!("set_instances: 表示中でないモデルが渡されたので無視した");
            return;
        }
        let list = instances::build(model, visible);
        let bytes: &[u8] = bytemuck::cast_slice(&list.raw);
        let needed = (bytes.len() as u64).max(plan::INSTANCE_BYTES);
        if needed > self.limits.max_buffer_size {
            let limit = self.limits.max_buffer_size;
            log::error!("instance buffer: {needed} > {limit}");
            self.last_error = Some(RenderError::LimitExceeded {
                what: "インスタンスバッファ".to_owned(),
                needed,
                limit,
            });
            return;
        }
        if let Err(err) =
            shown
                .textures
                .prepare(device, queue, &self.layouts.texture, model, &list.batches)
        {
            self.last_error = Some(err);
            return;
        }
        let reuse = shown
            .instances
            .as_ref()
            .is_some_and(|old| old.capacity >= needed);
        if reuse {
            if let Some(old) = shown.instances.as_mut() {
                if !bytes.is_empty() {
                    queue.write_buffer(&old.buffer, 0, bytes);
                }
                self.stats.uploaded_bytes += bytes.len() as u64;
                self.instance_generation += 1;
                self.stats.instance_rebuilds += 1;
                old.list = list;
                old.generation = self.instance_generation;
            }
            return;
        }
        let created = scoped(device, || {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("instances"),
                size: needed,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        match created {
            Ok(buffer) => {
                if let Some(old) = shown.instances.take() {
                    old.buffer.destroy();
                }
                if !bytes.is_empty() {
                    queue.write_buffer(&buffer, 0, bytes);
                }
                self.stats.uploaded_bytes += bytes.len() as u64;
                self.instance_generation += 1;
                self.stats.instance_rebuilds += 1;
                shown.instances = Some(GpuInstances {
                    buffer,
                    capacity: needed,
                    list,
                    generation: self.instance_generation,
                });
            }
            Err(err) => {
                log::error!("instance buffer: {err}");
                self.last_error = Some(err);
            }
        }
    }

    /// 「面と線」「線」の表示が使えるか（`Features::POLYGON_MODE_LINE` を持つ装置か）。
    /// 偽のときは線のパイプラインを作らず、その表示を求められても面で描く。
    pub fn supports_lines(&self) -> bool {
        self.lines_supported
    }

    /// 足した図形の形状を、足した分だけの小さなバッファで1回で送り、形状の位置の表の末尾に足す。
    /// 既存のページには触らない。モデルを閉じると一緒に捨てる。1回が32MiBを超えるときは断る。
    ///
    /// `prims` の順に、表の続きの番号（`primitive_slot_count()` から）が付く。文書の `PrimId` と
    /// 合わせるため、呼び出し側は足した形状を足した順に、まだ送っていない分だけ渡す。
    pub fn add_primitives(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        prims: &[Arc<Primitive>],
    ) -> Result<(), RenderError> {
        let Some(shown) = self.shown.as_mut() else {
            return Err(RenderError::Device(
                "形状を足す先のモデルがありません".to_owned(),
            ));
        };
        if prims.is_empty() {
            return Ok(());
        }
        let mut vertex_bytes = 0u64;
        let mut index_bytes = 0u64;
        for prim in prims {
            plan::check_u32("頂点数", prim.vertices.len() as u64)?;
            plan::check_u32("インデックス数", prim.indices.len() as u64)?;
            vertex_bytes += prim.vertices.len() as u64 * plan::VERTEX_BYTES;
            index_bytes += prim.indices.len() as u64 * plan::INDEX_BYTES;
        }
        let total = vertex_bytes + index_bytes;
        if total > ADD_PRIMITIVES_LIMIT_BYTES {
            return Err(plan::limit("足した形状", total, ADD_PRIMITIVES_LIMIT_BYTES));
        }
        let max_buffer = self.limits.max_buffer_size;
        if vertex_bytes > max_buffer {
            return Err(plan::limit("頂点バッファ", vertex_bytes, max_buffer));
        }
        if index_bytes > max_buffer {
            return Err(plan::limit("インデックスバッファ", index_bytes, max_buffer));
        }
        let page_index = shown.geometry.pages.len() as u32;
        let page = scoped(device, || GpuPage {
            uv: None,
            vertex: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("added vertex page {page_index}")),
                size: vertex_bytes.max(4),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            index: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("added index page {page_index}")),
                size: index_bytes.max(4),
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
        })?;
        let mut base_vertex = 0u64;
        let mut first_index = 0u64;
        for prim in prims {
            let vertices: &[u8] = bytemuck::cast_slice(&prim.vertices);
            let indices: &[u8] = bytemuck::cast_slice(&prim.indices);
            if !vertices.is_empty() {
                queue.write_buffer(&page.vertex, base_vertex * plan::VERTEX_BYTES, vertices);
            }
            if !indices.is_empty() {
                queue.write_buffer(&page.index, first_index * plan::INDEX_BYTES, indices);
            }
            shown.geometry.plan.prims.push(PrimSlot {
                page: page_index,
                // 32MiB以内なので i32 と u32 に収まる。
                base_vertex: base_vertex as i32,
                first_index: first_index as u32,
                index_count: prim.indices.len() as u32,
            });
            base_vertex += prim.vertices.len() as u64;
            first_index += prim.indices.len() as u64;
        }
        shown.geometry.plan.pages.push(PagePlan {
            vertex_bytes,
            index_bytes,
            uv_bytes: 0,
        });
        shown.geometry.plan.geometry_bytes += total;
        shown.geometry.pages.push(page);
        shown.added_bytes += total;
        self.stats.uploaded_bytes += total;
        Ok(())
    }

    /// 表示中のモデルの形状の位置の表の長さ（読み込んだ形状と足した形状の合計）。表示中が無ければ0。
    /// 次に `add_primitives` で足す形状には、この番号から付く。
    pub fn primitive_slot_count(&self) -> usize {
        self.shown
            .as_ref()
            .map_or(0, |shown| shown.geometry.plan.prims.len())
    }

    /// 表示中のモデルに `add_primitives` で足して、まだ持っている形状のバイト数。モデルを閉じると0。
    pub fn added_geometry_bytes(&self) -> u64 {
        self.shown.as_ref().map_or(0, |shown| shown.added_bytes)
    }

    /// 変形か色を確定した範囲（`NodeId` の連続範囲）の部品のインスタンスだけを `queue.write_buffer` で
    /// 書き直す。行列と色の両方を文書から読み直す（`instances::patch`）。構造と表示状態は
    /// 変わっていない前提（変わったら `set_instances`）。隣り合う位置は
    /// まとめて1回で送る。送るのは範囲の部品のインスタンスの分だけ。
    pub fn update_instances(
        &mut self,
        queue: &wgpu::Queue,
        model: &impl SceneRead,
        nodes: Range<u32>,
    ) {
        let Some(shown) = self.shown.as_mut() else {
            return;
        };
        if !model.shares_geometry_with(&shown.geometry.model) {
            log::warn!("update_instances: 表示中でないモデルが渡されたので無視した");
            return;
        }
        let Some(instances) = shown.instances.as_mut() else {
            return;
        };
        let list = &mut instances.list;
        if list.node_starts.len() != model.node_count() + 1 {
            log::warn!(
                "update_instances: 部品の数が変わっているので無視した（set_instances を使う）"
            );
            return;
        }
        let patched = instances::patch(list, model, &nodes);
        if patched.mismatched > 0 {
            log::warn!(
                "update_instances: 描画の数とインスタンスの数が合わない部品が {} 個あり、色を前の値のままにした",
                patched.mismatched
            );
        }
        let slots = patched.slots;
        if slots.is_empty() {
            return;
        }
        let mut sent = 0u64;
        let mut run_start = 0usize;
        for i in 1..=slots.len() {
            if i == slots.len() || slots[i] != slots[i - 1] + 1 {
                let first = slots[run_start] as usize;
                let last = slots[i - 1] as usize;
                let bytes: &[u8] = bytemuck::cast_slice(&list.raw[first..=last]);
                queue.write_buffer(
                    &instances.buffer,
                    first as u64 * plan::INSTANCE_BYTES,
                    bytes,
                );
                sent += bytes.len() as u64;
                run_start = i;
            }
        }
        self.stats.uploaded_bytes += sent;
        self.stats.instance_patches += 1;
    }

    /// いまの描画先の多重サンプル数。描画先がまだ無ければ `None`。
    pub fn sample_count(&self) -> Option<u32> {
        self.targets.as_ref().map(|targets| targets.samples)
    }

    /// 描画先へ描く。大きさが0なら描かず、前の画像（無ければ1画素の画像）を返す。
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &FrameParams,
    ) -> RenderOutput<'_> {
        let max_dim = targets::MAX_DIMENSION.min(self.limits.max_texture_dimension_2d);
        let Some(size) = targets::fit_size(frame.size_px, max_dim) else {
            return self.current_output();
        };
        let mut recreated = false;
        if self.targets.as_ref().is_none_or(|t| t.size != size) {
            match self.create_targets(device, queue, size) {
                Ok(()) => recreated = true,
                Err(err) => {
                    log::error!("render targets: {err}");
                    self.last_error = Some(err);
                    return self.current_output();
                }
            }
        }
        let Some(targets) = self.targets.as_ref() else {
            return self.current_output();
        };
        let format = targets.draw_format();
        if self
            .pipelines
            .as_ref()
            .is_none_or(|p| p.format != format || p.samples != targets.samples)
        {
            self.pipelines = Some(Pipelines::new(
                device,
                &self.layouts,
                format,
                targets.samples,
                self.lines_supported,
            ));
        }
        let Some(pipelines) = self.pipelines.as_ref() else {
            return self.current_output();
        };

        let selected = frame.selected.clone().unwrap_or(0..0);
        // 線のパイプラインが無ければ、線の表示を求められても面で描く。
        let mode = match frame.display {
            DisplayMode::FacesAndLines | DisplayMode::Lines if !pipelines.lines => {
                DisplayMode::Faces
            }
            mode => mode,
        };
        let inverse = frame.view_proj.inverse();
        let grid_visible = frame.grid.visible && inverse.is_finite();
        let uniform = frame_uniform(frame, &selected, mode, inverse, !targets.srgb_view);
        queue.write_buffer(&self.frame_uniform, 0, bytemuck::bytes_of(&uniform));
        self.stats.frame_bytes += std::mem::size_of::<FrameUniform>() as u64;

        let clear = if targets.srgb_view {
            frame.clear
        } else {
            let c = frame.clear;
            [
                outline::linear_to_srgb(c[0]),
                outline::linear_to_srgb(c[1]),
                outline::linear_to_srgb(c[2]),
                c[3],
            ]
        };
        let clear = wgpu::Color {
            r: f64::from(clear[0]),
            g: f64::from(clear[1]),
            b: f64::from(clear[2]),
            a: f64::from(clear[3]),
        };
        let (color_view, resolve_target) = match &targets.msaa {
            Some((_, view)) => (view, Some(&targets.draw_view)),
            None => (&targets.draw_view, None),
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("scene"),
        });
        let drawable = self
            .shown
            .as_ref()
            .and_then(|shown| shown.instances.as_ref().map(|i| (shown, i)));
        let textured = drawable.is_some_and(|(shown, _)| {
            self.textures_shown
                && matches!(mode, DisplayMode::Faces | DisplayMode::FacesAndLines)
                && frame.display != DisplayMode::Lines
                && shown.textures.count() > 0
                && !shown.textures.materials.is_empty()
        });
        if let Some((shown, instances)) = drawable.filter(|_| textured) {
            // 画像を描く描画があるときだけ、そのパイプラインを作る。
            for batch in &instances.list.batches {
                if let Some(material) = batch_material(shown, batch) {
                    pipelines.textured(
                        device,
                        &self.layouts,
                        matches!(material.map, UvMap::Box { .. }),
                    );
                }
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color_view,
                    depth_slice: None,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: if resolve_target.is_some() {
                            wgpu::StoreOp::Discard
                        } else {
                            wgpu::StoreOp::Store
                        },
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            if let Some((shown, instances)) = drawable {
                let geometry = &shown.geometry;
                pass.set_vertex_buffer(1, instances.buffer.slice(..));
                let batches = &instances.list.batches;
                match mode {
                    DisplayMode::Lines => {
                        pass.set_pipeline(pipelines.line_only.as_ref().expect("線の表示に対応"));
                        draw_plain(&mut pass, geometry, &instances.list);
                    }
                    DisplayMode::XRay => {
                        pass.set_pipeline(&pipelines.xray);
                        draw_plain(&mut pass, geometry, &instances.list);
                    }
                    DisplayMode::Faces | DisplayMode::FacesAndLines => {
                        if textured {
                            self.stats.textured_draws +=
                                draw_faces(&mut pass, shown, pipelines, batches);
                        } else {
                            pass.set_pipeline(&pipelines.scene);
                            draw_plain(&mut pass, geometry, &instances.list);
                        }
                        if mode == DisplayMode::FacesAndLines {
                            pass.set_pipeline(
                                pipelines.line_overlay.as_ref().expect("面と線の表示に対応"),
                            );
                            draw_plain(&mut pass, geometry, &instances.list);
                        }
                    }
                }
            }
            // 格子は形状の後。深度は比べるだけなので、部品の向こうの格子は隠れる。
            if grid_visible {
                pass.set_pipeline(&pipelines.grid);
                pass.draw(0..3, 0..1);
            }
        }
        if let (Some((shown, instances)), Some(resources)) = (drawable, self.outline.as_ref())
            && !selected.is_empty()
        {
            let ranges = self
                .selection
                .ranges(&selected, instances.generation, &instances.list);
            outline::encode(
                &mut encoder,
                pipelines,
                targets,
                resources,
                &self.frame_bind_group,
                &shown.geometry,
                &instances.buffer,
                ranges,
            );
        }
        // 画面に出ていない間などは、GPUより速く描画を積めてしまう。未完了の分は
        // 形状の分だけメモリを持ち続けるので、古いものの完了を待ってから次を送る。
        while self.in_flight.len() >= MAX_FRAMES_IN_FLIGHT {
            let Some(oldest) = self.in_flight.pop_front() else {
                break;
            };
            let waited = device.poll(wgpu::PollType::Wait {
                submission_index: Some(oldest),
                timeout: Some(Duration::from_secs(2)),
            });
            if let Err(err) = waited {
                log::warn!("描画の完了を待てなかった: {err}");
                break;
            }
        }
        self.in_flight
            .push_back(queue.submit(Some(encoder.finish())));
        self.stats.draw_count += 1;
        RenderOutput {
            view: &targets.resolve_view,
            recreated,
        }
    }

    pub fn stats(&self) -> RenderStats {
        let mut stats = self.stats;
        if let Some(shown) = &self.shown {
            stats.texture_count = shown.textures.count();
            stats.texture_bytes = shown.textures.bytes;
        }
        stats
    }

    /// 直近の、呼び出し元へ返せなかったGPUのエラー（描画先やインスタンスの確保の失敗）。
    pub fn take_error(&mut self) -> Option<RenderError> {
        self.last_error.take()
    }

    /// いまの描画先の画像を Rgba8 で読み戻す（試験と診断用。GPUの完了を待つので毎フレームには使わない）。
    pub fn read_image(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Image> {
        let targets = self.targets.as_ref()?;
        let [width, height] = targets.size;
        let row = width * 4;
        let padded =
            row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(padded) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &targets.resolve,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));
        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        receiver.recv().ok()?.ok()?;
        let mapped = slice.get_mapped_range().ok()?;
        let mut rgba = Vec::with_capacity((row * height) as usize);
        for y in 0..height {
            let start = (y * padded) as usize;
            rgba.extend_from_slice(&mapped[start..start + row as usize]);
        }
        drop(mapped);
        buffer.unmap();
        buffer.destroy();
        Some(Image {
            width,
            height,
            rgba,
        })
    }

    fn current_output(&self) -> RenderOutput<'_> {
        let view = self
            .targets
            .as_ref()
            .map_or(&self.placeholder_view, |t| &t.resolve_view);
        RenderOutput {
            view,
            recreated: false,
        }
    }

    /// 多重サンプル4・sRGBのビューから順に試し、確保できた組み合わせを使う。
    fn create_targets(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: [u32; 2],
    ) -> Result<(), RenderError> {
        let mut attempts = vec![(MSAA_SAMPLES, true), (1, true)];
        if !self.srgb_supported {
            attempts.clear();
        }
        attempts.extend([(MSAA_SAMPLES, false), (1, false)]);
        let mut last = RenderError::Device("描画先を作れません".to_owned());
        for (samples, srgb) in attempts {
            match Targets::create(device, size, samples, srgb) {
                Ok(targets) => {
                    let outline = OutlineResources::new(device, queue, &self.layouts, &targets);
                    if !srgb && self.srgb_supported {
                        log::warn!("sRGBのビューが使えないため、シェーダーで色を変換する");
                        self.srgb_supported = false;
                    }
                    if samples != MSAA_SAMPLES {
                        log::warn!("多重サンプルの描画先を確保できず、サンプル数1で描く");
                    }
                    if let Some(old) = self.targets.take() {
                        old.destroy();
                    }
                    self.outline = Some(outline);
                    self.targets = Some(targets);
                    return Ok(());
                }
                Err(err) => last = err,
            }
        }
        Err(last)
    }
}

impl Drop for SceneRenderer {
    fn drop(&mut self) {
        self.clear_model();
        if let Some(targets) = self.targets.take() {
            targets.destroy();
        }
        self.placeholder.destroy();
    }
}

/// 読み戻した画像。行の詰め物は除いてある。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }
}

/// 描画の定数を作る。変形中の差分の行列とその法線用の行列も、ここで1つの定数に入れる。
fn frame_uniform(
    frame: &FrameParams,
    selected: &Range<u32>,
    mode: DisplayMode,
    inverse: Mat4,
    to_srgb: bool,
) -> FrameUniform {
    let drag = frame
        .drag
        .as_ref()
        .filter(|drag| drag.nodes.start < drag.nodes.end);
    let (delta, range) = match drag {
        Some(drag) => (drag.world_delta, [drag.nodes.start, drag.nodes.end, 0, 0]),
        None => (Mat4::IDENTITY, [0; 4]),
    };
    let normal = instances::normal_matrix(&Affine3A::from_mat4(delta)).to_cols_array_2d();
    let section = plan::section_params(frame.section.as_ref());
    let inverse = if inverse.is_finite() {
        inverse
    } else {
        Mat4::IDENTITY
    };
    FrameUniform {
        view_proj: frame.view_proj.to_cols_array_2d(),
        eye: [frame.eye.x, frame.eye.y, frame.eye.z, 1.0],
        selected: [
            selected.start,
            selected.end,
            u32::from(to_srgb),
            display_code(mode),
        ],
        inv_view_proj: inverse.to_cols_array_2d(),
        drag_delta: delta.to_cols_array_2d(),
        drag_normal: [
            [normal[0][0], normal[0][1], normal[0][2], 0.0],
            [normal[1][0], normal[1][1], normal[1][2], 0.0],
            [normal[2][0], normal[2][1], normal[2][2], 0.0],
        ],
        drag: range,
        params: [
            frame.grid.spacing,
            if mode == DisplayMode::XRay {
                XRAY_ALPHA
            } else {
                1.0
            },
            section[0],
            section[1],
        ],
    }
}

/// まとまりごとに描く。ページが変わったときだけ頂点とインデックスのバッファを付け替える。
pub(crate) fn draw_ranges(
    pass: &mut wgpu::RenderPass<'_>,
    geometry: &GpuGeometry,
    ranges: &[(u32, Range<u32>)],
) {
    let mut bound_page = None;
    for (prim, instances) in ranges {
        draw_slot(pass, geometry, &mut bound_page, *prim, instances.clone());
    }
}

/// 画像を使わないパスでは、形状・部品・描画の順を保つ。
fn draw_plain(pass: &mut wgpu::RenderPass<'_>, geometry: &GpuGeometry, list: &InstanceList) {
    if !list.plain_ranges.is_empty() {
        draw_ranges(pass, geometry, &list.plain_ranges);
        return;
    }
    let mut bound_page = None;
    for batch in &list.batches {
        draw_slot(
            pass,
            geometry,
            &mut bound_page,
            batch.prim,
            batch.instances.clone(),
        );
    }
}

/// 画像で描くまとまりの材質。UV で貼る材質は、形状に UV があるときだけ使う。
fn batch_material<'a>(
    shown: &'a Shown,
    batch: &instances::Batch,
) -> Option<&'a textures::Material> {
    let slot = shown.geometry.plan.prims.get(batch.prim as usize)?;
    batch
        .tex
        .and_then(|id| shown.textures.materials.get(&id))
        .filter(|m| {
            matches!(m.map, UvMap::Box { .. })
                || shown.geometry.pages[slot.page as usize].uv.is_some()
        })
}

fn draw_faces(
    pass: &mut wgpu::RenderPass<'_>,
    shown: &Shown,
    pipelines: &Pipelines,
    batches: &[instances::Batch],
) -> u64 {
    let mut bound_page = None;
    let mut bound_pipeline = None;
    let mut draws = 0;
    for batch in batches {
        let Some(slot) = shown.geometry.plan.prims.get(batch.prim as usize) else {
            continue;
        };
        if slot.index_count == 0 {
            continue;
        }
        let material = batch_material(shown, batch);
        let textured = match material.map(|m| m.map) {
            None => None,
            Some(UvMap::Vertex { .. }) => pipelines.scene_uv.get(),
            Some(UvMap::Box { .. }) => pipelines.scene_box.get(),
        };
        // 描く前に `Pipelines::textured` で作ってある。
        let Some(pipeline) = textured.or(material.is_none().then_some(&pipelines.scene)) else {
            continue;
        };
        if bound_pipeline != Some(pipeline) {
            pass.set_pipeline(pipeline);
            bound_pipeline = Some(pipeline);
        }
        if let Some(material) = material {
            pass.set_bind_group(1, &material.group, &[]);
            draws += u64::from(material.ready);
        }
        draw_slot(
            pass,
            &shown.geometry,
            &mut bound_page,
            batch.prim,
            batch.instances.clone(),
        );
    }
    draws
}

fn draw_slot(
    pass: &mut wgpu::RenderPass<'_>,
    geometry: &GpuGeometry,
    bound_page: &mut Option<u32>,
    prim: u32,
    instances: Range<u32>,
) {
    let Some(slot) = geometry.plan.prims.get(prim as usize) else {
        return;
    };
    if slot.index_count == 0 || instances.is_empty() {
        return;
    }
    let Some(page) = geometry.pages.get(slot.page as usize) else {
        return;
    };
    if *bound_page != Some(slot.page) {
        pass.set_vertex_buffer(0, page.vertex.slice(..));
        pass.set_index_buffer(page.index.slice(..), wgpu::IndexFormat::Uint32);
        if let Some(uv) = &page.uv {
            pass.set_vertex_buffer(2, uv.slice(..));
        }
        *bound_page = Some(slot.page);
    }
    pass.draw_indexed(
        slot.first_index..slot.first_index + slot.index_count,
        slot.base_vertex,
        instances,
    );
}

#[cfg(test)]
mod failure_tests {
    use std::future::Future;
    use std::pin::pin;
    use std::sync::Arc;
    use std::task::{Context, Poll, Waker};

    use glam::{Affine3A, Vec3};
    use model_reviewer_core::math::Aabb;
    use model_reviewer_core::scene::{
        Draw, Model, ModelStats, Node, NodeId, PrimId, Primitive, Vertex,
    };
    use model_reviewer_core::texture::Textures;
    use model_reviewer_core::visibility::Visibility;

    use super::{FrameParams, RenderError, SceneRenderer, plan};

    const CLEAR: [f32; 4] = [0.016, 0.0185, 0.0242, 1.0];

    fn block_on<F: Future>(future: F) -> F::Output {
        let mut future = pin!(future);
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
                return value;
            }
            std::thread::yield_now();
        }
    }

    fn gpu_device() -> (wgpu::Device, wgpu::Queue) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("GPUのアダプタが見つからない");
        block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("デバイスを作れない")
    }

    /// インスタンスバッファがデバイス上限を少しだけ超える大きさに制限する。
    fn limited_device() -> (wgpu::Device, wgpu::Queue, u64) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("GPUのアダプタが見つからない");
        let mut limits = adapter.limits();
        let cap = 16 * 1024;
        limits.max_buffer_size = cap;
        limits.max_storage_buffer_binding_size = limits.max_storage_buffer_binding_size.min(cap);
        limits.max_uniform_buffer_binding_size = limits.max_uniform_buffer_binding_size.min(cap);
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: limits,
            ..Default::default()
        }))
        .expect("制限したデバイスを作れない");
        (device, queue, cap)
    }

    fn quad() -> Primitive {
        Primitive {
            vertices: vec![
                Vertex {
                    position: [-0.5, -0.5, 0.0],
                    normal: [0.0, 0.0, 1.0],
                },
                Vertex {
                    position: [0.5, -0.5, 0.0],
                    normal: [0.0, 0.0, 1.0],
                },
                Vertex {
                    position: [0.5, 0.5, 0.0],
                    normal: [0.0, 0.0, 1.0],
                },
                Vertex {
                    position: [-0.5, 0.5, 0.0],
                    normal: [0.0, 0.0, 1.0],
                },
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            local_aabb: Aabb {
                min: Vec3::new(-0.5, -0.5, 0.0),
                max: Vec3::new(0.5, 0.5, 0.0),
            },
            has_normals: true,
        }
    }

    fn plates(count: u32) -> Model {
        let world = Affine3A::from_scale(Vec3::new(4.0, 4.0, 1.0));
        let aabb = Aabb {
            min: Vec3::new(-2.0, -2.0, 0.0),
            max: Vec3::new(2.0, 2.0, 0.0),
        };
        let mut nodes = Vec::with_capacity(count as usize);
        let mut draws = Vec::with_capacity(count as usize);
        for index in 0..count {
            let start = draws.len() as u32;
            draws.push(Draw {
                prim: PrimId(0),
                base_color: [0.8, 0.05, 0.04, 1.0],
                tex: None,
            });
            nodes.push(Node {
                name: format!("n{index}"),
                gltf_index: index,
                parent: None,
                subtree_end: index + 1,
                depth: 0,
                local: world,
                world,
                draws: start..start + 1,
                world_aabb: aabb,
            });
        }
        Model {
            textures: Textures::default(),
            nodes,
            draws,
            primitives: vec![quad()],
            bounds: aabb,
            stats: ModelStats {
                nodes: count,
                meshes: 1,
                primitives: 1,
                unique_triangles: 2,
                placed_triangles: u64::from(count) * 2,
            },
            warnings: Vec::new(),
        }
    }

    fn frame(size: [u32; 2]) -> FrameParams {
        let eye = Vec3::new(0.0, 0.0, 4.0);
        let aspect = size[0] as f32 / size[1] as f32;
        let view = glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y);
        let proj = glam::camera::rh::proj::directx::perspective_infinite_reverse(
            40f32.to_radians(),
            aspect,
            0.1,
        );
        FrameParams {
            size_px: size,
            view_proj: proj * view,
            eye,
            selected: None,
            clear: CLEAR,
            display: Default::default(),
            grid: Default::default(),
            drag: None,
            section: None,
        }
    }

    fn upload(
        renderer: &mut SceneRenderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        model: &Arc<Model>,
    ) {
        renderer
            .begin_upload(device, Arc::clone(model))
            .expect("転送を始められる");
        loop {
            if renderer.continue_upload(queue, 1 << 20).done {
                break;
            }
        }
    }

    fn center_after(
        renderer: &mut SceneRenderer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: [u32; 2],
    ) -> [u8; 4] {
        let _ = renderer.render(device, queue, &frame(size));
        let image = renderer.read_image(device, queue).expect("読み戻せる");
        image.pixel(image.width / 2, image.height / 2)
    }

    #[test]
    #[ignore = "GPUが必要"]
    fn oversized_targets_keep_the_previous_image() {
        let (device, queue) = gpu_device();
        let mut renderer = SceneRenderer::new(&device);
        let model = Arc::new(plates(1));
        upload(&mut renderer, &device, &queue, &model);
        renderer.set_instances(&device, &queue, &model, &Visibility::all_visible(1));
        let size = [64, 48];
        let before = center_after(&mut renderer, &device, &queue, size);
        assert!(
            u32::from(before[0]) > 2 * u32::from(before[2]),
            "{before:?}"
        );
        let kept = renderer.targets.as_ref().expect("描画先").size;
        let dim = renderer.limits.max_texture_dimension_2d;
        let err = renderer
            .create_targets(&device, &queue, [dim.saturating_add(1), 8])
            .expect_err("上限を超える描画先は失敗する");
        assert!(matches!(
            err,
            RenderError::Device(_) | RenderError::OutOfMemory
        ));
        assert_eq!(renderer.targets.as_ref().expect("前の描画先").size, kept);
        assert!(renderer.take_error().is_none());
        let after = center_after(&mut renderer, &device, &queue, size);
        assert_eq!(after, before);
    }

    #[test]
    #[ignore = "GPUが必要"]
    fn texture_pipelines_are_lazy() {
        use model_reviewer_core::texture::{ImageId, MipChain, Sampling, TexId, TexUse, UvMap};
        let (device, queue) = gpu_device();
        let mut renderer = SceneRenderer::new(&device);
        let mut model = plates(1);
        let draw = |renderer: &mut SceneRenderer| {
            center_after(renderer, &device, &queue, [64, 48]);
            let pipelines = renderer.pipelines.as_ref().unwrap();
            (
                pipelines.scene_uv.get().is_some(),
                pipelines.scene_box.get().is_some(),
            )
        };
        let show = |renderer: &mut SceneRenderer, m| {
            let m = Arc::new(m);
            upload(renderer, &device, &queue, &m);
            renderer.set_instances(&device, &queue, &m, &Visibility::all_visible(1));
        };
        show(&mut renderer, model.clone());
        assert_eq!(draw(&mut renderer), (false, false));
        model.draws[0].tex = Some(TexId(0));
        model.textures.uses.push(TexUse {
            image: Some(ImageId(0)),
            sampling: Sampling::default(),
            map: UvMap::Box {
                size: 1.0,
                aspect: 1.0,
            },
            cutoff: None,
            baked_box: None,
        });
        show(&mut renderer, model);
        assert_eq!(draw(&mut renderer), (false, false));
        renderer
            .begin_image(
                &device,
                ImageId(0),
                MipChain {
                    size: [1; 2],
                    levels: vec![vec![255; 4]],
                },
            )
            .unwrap();
        renderer.continue_images(&device, &queue, 4);
        renderer.set_textures_shown(false);
        assert_eq!(draw(&mut renderer), (false, false));
        renderer.set_textures_shown(true);
        assert_eq!(draw(&mut renderer), (false, true));
    }

    #[test]
    #[ignore = "GPUが必要"]
    fn instance_limit_keeps_the_previous_buffer() {
        let (device, queue) = gpu_device();
        let mut renderer = SceneRenderer::new(&device);
        let model = Arc::new(plates(2));
        upload(&mut renderer, &device, &queue, &model);
        renderer.set_instances_with(&device, &queue, &model, &|id: NodeId| id.0 == 0);
        let before = center_after(&mut renderer, &device, &queue, [64, 48]);
        assert!(
            u32::from(before[0]) > 2 * u32::from(before[2]),
            "{before:?}"
        );
        let generation = renderer.instance_generation;
        let rebuilds = renderer.stats().instance_rebuilds;
        renderer.limits.max_buffer_size = plan::INSTANCE_BYTES;
        renderer.set_instances(&device, &queue, &model, &Visibility::all_visible(2));
        match renderer.take_error() {
            Some(RenderError::LimitExceeded {
                what,
                needed,
                limit,
            }) => {
                assert_eq!(what, "インスタンスバッファ");
                assert!(needed > limit);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(renderer.instance_generation, generation);
        assert_eq!(renderer.stats().instance_rebuilds, rebuilds);
        assert!(renderer.shown.as_ref().unwrap().instances.is_some());
        let after = center_after(&mut renderer, &device, &queue, [64, 48]);
        assert_eq!(after, before);
    }

    #[test]
    #[ignore = "GPUが必要"]
    fn failed_instance_allocation_keeps_the_previous_buffer() {
        let (device, queue, cap) = limited_device();
        let mut renderer = SceneRenderer::new(&device);
        let count = u32::try_from(cap / plan::INSTANCE_BYTES).expect("個数") + 2;
        renderer.limits.max_buffer_size =
            u64::from(count) * plan::INSTANCE_BYTES + plan::INSTANCE_BYTES;
        let model = Arc::new(plates(count));
        upload(&mut renderer, &device, &queue, &model);
        renderer.set_instances_with(&device, &queue, &model, &|id: NodeId| id.0 == 0);
        let before = center_after(&mut renderer, &device, &queue, [64, 48]);
        assert!(
            u32::from(before[0]) > 2 * u32::from(before[2]),
            "{before:?}"
        );
        let generation = renderer.instance_generation;
        let rebuilds = renderer.stats().instance_rebuilds;
        renderer.set_instances(
            &device,
            &queue,
            &model,
            &Visibility::all_visible(model.nodes.len()),
        );
        match renderer.take_error() {
            Some(RenderError::Device(_) | RenderError::OutOfMemory) => {}
            other => panic!("{other:?}"),
        }
        assert_eq!(renderer.instance_generation, generation);
        assert_eq!(renderer.stats().instance_rebuilds, rebuilds);
        assert!(renderer.shown.as_ref().unwrap().instances.is_some());
        let after = center_after(&mut renderer, &device, &queue, [64, 48]);
        assert_eq!(after, before);
    }
}

#[cfg(test)]
mod uniform_tests {
    use glam::{Mat4, Vec3};
    use model_reviewer_core::section::{Axis, SectionPlane};

    use super::{DisplayMode, FrameParams, frame_uniform};

    fn frame(section: Option<SectionPlane>) -> FrameParams {
        FrameParams {
            size_px: [64, 48],
            view_proj: Mat4::IDENTITY,
            eye: Vec3::Z,
            selected: None,
            clear: [0.0, 0.0, 0.0, 1.0],
            display: Default::default(),
            grid: Default::default(),
            drag: None,
            section,
        }
    }

    #[test]
    fn no_section_leaves_params_z_and_w_zero() {
        let u = frame_uniform(
            &frame(None),
            &(0..0),
            DisplayMode::Faces,
            Mat4::IDENTITY,
            false,
        );
        assert_eq!(u.params[2], 0.0);
        assert_eq!(u.params[3], 0.0);
        assert_eq!(u.params[1], 1.0);
        // 断面なしの定数は、項目を足す前と同じ値（格子の間隔と不透明度のほかは 0）。
        let x = frame_uniform(
            &frame(None),
            &(0..0),
            DisplayMode::XRay,
            Mat4::IDENTITY,
            false,
        );
        assert_eq!(x.params[2..], [0.0, 0.0]);
    }

    #[test]
    fn section_packs_offset_and_code() {
        for (axis, number) in [(Axis::X, 1.0), (Axis::Y, 2.0), (Axis::Z, 3.0)] {
            for flipped in [false, true] {
                let plane = SectionPlane {
                    axis,
                    offset: -2.5,
                    flipped,
                };
                let u = frame_uniform(
                    &frame(Some(plane)),
                    &(0..0),
                    DisplayMode::Faces,
                    Mat4::IDENTITY,
                    false,
                );
                assert_eq!(u.params[2], -2.5, "位置はそのまま入る");
                let expected = if flipped { -number } else { number };
                assert_eq!(u.params[3], expected);
                assert_eq!(u.params[3], plane.code());
            }
        }
    }

    #[test]
    fn non_finite_offset_cuts_nothing() {
        for offset in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let plane = SectionPlane {
                axis: Axis::Y,
                offset,
                flipped: false,
            };
            let u = frame_uniform(
                &frame(Some(plane)),
                &(0..0),
                DisplayMode::Faces,
                Mat4::IDENTITY,
                false,
            );
            assert_eq!(u.params[2..], [0.0, 0.0]);
        }
    }

    #[test]
    fn frame_uniform_is_still_304_bytes() {
        assert_eq!(std::mem::size_of::<super::FrameUniform>(), 304);
    }
}
