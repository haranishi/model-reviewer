//! GPUバッファのページ分割と上限の検査。`wgpu::Limits` だけを見る純粋関数なので、GPUなしで試験できる。
//!
//! 形状は `Model.primitives` の単位で1回だけ詰める。共有された形状を参照する部品がいくつあっても、
//! 送るのは1回である（要件9.3）。

use std::ops::Range;

use glam::{Mat4, Vec3};
use model_reviewer_core::scene::{Model, Vertex};
use model_reviewer_core::section::SectionPlane;

use crate::RenderError;
use crate::display::{DisplayMode, DragPreview};
use crate::grid::GridParams;

/// 1ページ（頂点バッファ1本とインデックスバッファ1本）の目標の大きさ。
pub const PAGE_TARGET_BYTES: u64 = 64 * 1024 * 1024;
/// インスタンス1件の大きさ。ワールド行列・法線行列・色・NodeId・フラグ。
pub const INSTANCE_BYTES: u64 = 144;
pub(crate) const VERTEX_BYTES: u64 = std::mem::size_of::<Vertex>() as u64;
pub(crate) const INDEX_BYTES: u64 = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PagePlan {
    pub vertex_bytes: u64,
    pub index_bytes: u64,
}

/// プリミティブの置き場所。`draw_indexed(first_index..first_index+index_count, base_vertex, ..)` で描く。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrimSlot {
    pub page: u32,
    pub base_vertex: i32,
    pub first_index: u32,
    pub index_count: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BufferPlan {
    pub pages: Vec<PagePlan>,
    /// `Model.primitives` と同じ順。
    pub prims: Vec<PrimSlot>,
    /// 配置された部品の数（インスタンスの最大数）。
    pub placed_instances: u32,
    /// 頂点とインデックスの合計バイト数。インスタンスは含まない。
    pub geometry_bytes: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrameParams {
    pub size_px: [u32; 2],
    pub view_proj: Mat4,
    pub eye: Vec3,
    pub selected: Option<Range<u32>>,
    pub clear: [f32; 4],
    /// 既定（`Faces`）は第1段階と同じ絵。
    pub display: DisplayMode,
    /// 既定は描かない。
    pub grid: GridParams,
    /// 変形中だけ `Some`。
    pub drag: Option<DragPreview>,
    /// 断面。既定の `None` は第2段階と同じ絵（定数の `params.z`・`params.w` は 0）。
    pub section: Option<SectionPlane>,
}

pub fn plan_buffers(model: &Model, limits: &wgpu::Limits) -> Result<BufferPlan, RenderError> {
    let max_buffer = limits.max_buffer_size;
    let target = PAGE_TARGET_BYTES.min(max_buffer);
    let mut plan = BufferPlan::default();
    let mut current: Option<(u32, PagePlan)> = None;

    for prim in &model.primitives {
        let vertex_count = prim.vertices.len() as u64;
        let index_count = prim.indices.len() as u64;
        check_u32("頂点数", vertex_count)?;
        check_u32("インデックス数", index_count)?;
        let vbytes = vertex_count * VERTEX_BYTES;
        let ibytes = index_count * INDEX_BYTES;
        if vbytes > max_buffer {
            return Err(limit("頂点バッファ", vbytes, max_buffer));
        }
        if ibytes > max_buffer {
            return Err(limit("インデックスバッファ", ibytes, max_buffer));
        }

        let fits = current.is_some_and(|(_, page)| {
            page.vertex_bytes + vbytes <= target && page.index_bytes + ibytes <= target
        });
        if !fits {
            if let Some((_, page)) = current.take() {
                plan.pages.push(page);
            }
            current = Some((plan.pages.len() as u32, PagePlan::default()));
        }
        let Some((page_index, page)) = current.as_mut() else {
            unreachable!("直前で用意した");
        };
        let base_vertex = page.vertex_bytes / VERTEX_BYTES;
        if base_vertex > i32::MAX as u64 {
            return Err(limit("頂点の位置", base_vertex, i32::MAX as u64));
        }
        plan.prims.push(PrimSlot {
            page: *page_index,
            base_vertex: base_vertex as i32,
            first_index: (page.index_bytes / INDEX_BYTES) as u32,
            index_count: index_count as u32,
        });
        page.vertex_bytes += vbytes;
        page.index_bytes += ibytes;
        plan.geometry_bytes += vbytes + ibytes;
    }
    if let Some((_, page)) = current {
        plan.pages.push(page);
    }

    let placed: u64 = model
        .nodes
        .iter()
        .map(|node| u64::from(node.draws.end.saturating_sub(node.draws.start)))
        .sum();
    check_u32("インスタンス数", placed)?;
    let instance_bytes = placed * INSTANCE_BYTES;
    if instance_bytes > max_buffer {
        return Err(limit("インスタンスバッファ", instance_bytes, max_buffer));
    }
    for draw in &model.draws {
        if draw.prim.0 as usize >= model.primitives.len() {
            return Err(RenderError::Device(format!(
                "存在しない形状 {} を参照しています",
                draw.prim.0
            )));
        }
    }
    plan.placed_instances = placed as u32;
    Ok(plan)
}

pub(crate) fn check_u32(what: &str, value: u64) -> Result<(), RenderError> {
    if value > u64::from(u32::MAX) {
        Err(limit(what, value, u64::from(u32::MAX)))
    } else {
        Ok(())
    }
}

pub(crate) fn limit(what: &str, needed: u64, limit: u64) -> RenderError {
    RenderError::LimitExceeded {
        what: what.to_owned(),
        needed,
        limit,
    }
}
