//! 分割転送。バッファは `begin_upload` で確保し、中身は `continue_upload` が予算の分だけ送る。
//!
//! `queue.write_buffer` は呼んだ時点でデータを複製するので、予算がそのまま1フレームのCPU負荷の上限になる。
//! 頂点とインデックスは、ここで1回送るだけで、以後は送り直さない（要件9.2）。

use std::sync::Arc;

use model_reviewer_core::scene::{Model, PrimId, SceneRead};

use crate::RenderError;
use crate::plan::{BufferPlan, INDEX_BYTES, UV_BYTES, VERTEX_BYTES};
use crate::targets::scoped;

/// `continue_upload` の予算の初期値。
pub const DEFAULT_BUDGET_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UploadProgress {
    pub uploaded_bytes: u64,
    pub total_bytes: u64,
    pub done: bool,
}

pub(crate) struct GpuPage {
    pub vertex: wgpu::Buffer,
    pub index: wgpu::Buffer,
    pub uv: Option<wgpu::Buffer>,
}

/// GPUに置いた1つのモデルの形状。
pub(crate) struct GpuGeometry {
    pub model: Arc<Model>,
    pub plan: BufferPlan,
    pub pages: Vec<GpuPage>,
}

impl GpuGeometry {
    pub fn destroy(&self) {
        for page in &self.pages {
            page.vertex.destroy();
            page.index.destroy();
            if let Some(uv) = &page.uv {
                uv.destroy();
            }
        }
    }
}

/// 転送中の組。どこまで送ったかを持つ。
pub(crate) struct UploadJob {
    pub geometry: GpuGeometry,
    prim: usize,
    /// 0: 頂点、1: インデックス、2: UV（あるページだけ）。
    part: u8,
    offset: u64,
    pub uploaded: u64,
}

impl UploadJob {
    pub fn new(
        device: &wgpu::Device,
        model: Arc<Model>,
        plan: BufferPlan,
    ) -> Result<Self, RenderError> {
        let pages = scoped(device, || {
            plan.pages
                .iter()
                .enumerate()
                .map(|(index, page)| GpuPage {
                    vertex: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(&format!("vertex page {index}")),
                        size: page.vertex_bytes.max(4),
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                    index: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(&format!("index page {index}")),
                        size: page.index_bytes.max(4),
                        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                    uv: (page.uv_bytes > 0).then(|| {
                        device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some(&format!("uv page {index}")),
                            size: page.uv_bytes,
                            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        })
                    }),
                })
                .collect::<Vec<_>>()
        })?;
        Ok(Self {
            geometry: GpuGeometry { model, plan, pages },
            prim: 0,
            part: 0,
            offset: 0,
            uploaded: 0,
        })
    }

    pub fn total(&self) -> u64 {
        self.geometry.plan.geometry_bytes
    }

    pub fn done(&self) -> bool {
        self.prim >= self.geometry.model.primitives.len()
    }

    pub fn progress(&self) -> UploadProgress {
        UploadProgress {
            uploaded_bytes: self.uploaded,
            total_bytes: self.total(),
            done: self.done(),
        }
    }

    /// 予算の分だけ送り、送ったバイト数を返す。予算が4バイト未満でも4バイトは進める。
    pub fn step(&mut self, queue: &wgpu::Queue, budget_bytes: u64) -> u64 {
        let mut left = budget_bytes.max(4) & !3;
        let mut sent = 0;
        let model = Arc::clone(&self.geometry.model);
        while left > 0 && !self.done() {
            let prim = &model.primitives[self.prim];
            let slot = self.geometry.plan.prims[self.prim];
            let page = &self.geometry.pages[slot.page as usize];
            let (bytes, buffer, base): (&[u8], &wgpu::Buffer, u64) = match self.part {
                0 => (
                    bytemuck::cast_slice(&prim.vertices),
                    &page.vertex,
                    slot.base_vertex as u64 * VERTEX_BYTES,
                ),
                1 => (
                    bytemuck::cast_slice(&prim.indices),
                    &page.index,
                    u64::from(slot.first_index) * INDEX_BYTES,
                ),
                _ => (
                    bytemuck::cast_slice(model.prim_uvs(PrimId(self.prim as u32))),
                    page.uv.as_ref().expect("UVのあるページ"),
                    slot.base_vertex as u64 * UV_BYTES,
                ),
            };
            let remaining = bytes.len() as u64 - self.offset;
            let take = remaining.min(left);
            if take > 0 {
                let start = self.offset as usize;
                let end = start + take as usize;
                queue.write_buffer(buffer, base + self.offset, &bytes[start..end]);
                self.offset += take;
                left -= take;
                sent += take;
            }
            if self.offset as usize >= bytes.len() {
                self.offset = 0;
                self.part += 1;
                if self.part == 3 || (self.part == 2 && page.uv.is_none()) {
                    self.part = 0;
                    self.prim += 1;
                }
            }
        }
        self.uploaded += sent;
        sent
    }
}
