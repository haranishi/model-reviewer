//! GPUバッファのページ分割と上限の検査。`wgpu::Limits` だけを見る純粋関数なので、GPUなしで試験できる。
//!
//! 形状は `Model.primitives` の単位で1回だけ詰める。共有された形状を参照する部品がいくつあっても、
//! 送るのは1回である（要件9.3）。

use std::ops::Range;

use glam::{Mat4, Vec3};
use model_reviewer_core::scene::{Model, PrimId, SceneRead, Vertex};
use model_reviewer_core::section::SectionPlane;

use crate::RenderError;
use crate::display::{DisplayMode, DragPreview};
use crate::grid::GridParams;

/// ページ内の各バッファの目標の大きさ。
pub const PAGE_TARGET_BYTES: u64 = 64 * 1024 * 1024;
/// インスタンス1件の大きさ。ワールド行列・法線行列・色・NodeId・フラグ。
pub const INSTANCE_BYTES: u64 = 144;
pub(crate) const VERTEX_BYTES: u64 = std::mem::size_of::<Vertex>() as u64;
pub(crate) const INDEX_BYTES: u64 = 4;
pub(crate) const UV_BYTES: u64 = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PagePlan {
    pub vertex_bytes: u64,
    pub index_bytes: u64,
    pub uv_bytes: u64,
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
    /// 頂点・インデックス・UV の合計バイト数。インスタンスは含まない。
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

/// 描画の定数 `params` の z（断面の位置）と w（`SectionPlane::code`）。断面なしは必ず `[0, 0]`。
/// 位置が有限でない平面も、何も切らないように `[0, 0]` にする（0 以外が残ると画素が消える）。
pub fn section_params(section: Option<&SectionPlane>) -> [f32; 2] {
    match section {
        Some(plane) if plane.offset.is_finite() => [plane.offset, plane.code()],
        _ => [0.0, 0.0],
    }
}

pub fn plan_buffers(model: &Model, limits: &wgpu::Limits) -> Result<BufferPlan, RenderError> {
    let max_buffer = limits.max_buffer_size;
    let target = PAGE_TARGET_BYTES.min(max_buffer);
    let mut plan = BufferPlan::default();
    let mut current: [Option<usize>; 2] = [None; 2];

    for (index, prim) in model.primitives.iter().enumerate() {
        let vertex_count = prim.vertices.len() as u64;
        let index_count = prim.indices.len() as u64;
        check_u32("頂点数", vertex_count)?;
        check_u32("インデックス数", index_count)?;
        let vbytes = vertex_count * VERTEX_BYTES;
        let ibytes = index_count * INDEX_BYTES;
        let uvs = model.prim_uvs(PrimId(index as u32));
        if !uvs.is_empty() && uvs.len() != prim.vertices.len() {
            return Err(RenderError::Device(format!(
                "形状 {index} の頂点数とUVの数が一致しません"
            )));
        }
        let uvbytes = uvs.len() as u64 * UV_BYTES;
        if vbytes > max_buffer {
            return Err(limit("頂点バッファ", vbytes, max_buffer));
        }
        if ibytes > max_buffer {
            return Err(limit("インデックスバッファ", ibytes, max_buffer));
        }

        let open = &mut current[usize::from(!uvs.is_empty())];
        let existing = open.filter(|&i| {
            let page = &plan.pages[i];
            page.vertex_bytes + vbytes <= target && page.index_bytes + ibytes <= target
        });
        let page_index = if let Some(i) = existing {
            i
        } else {
            let i = plan.pages.len();
            plan.pages.push(PagePlan::default());
            *open = Some(i);
            i
        };
        let page = &mut plan.pages[page_index];
        let base_vertex = page.vertex_bytes / VERTEX_BYTES;
        if base_vertex > i32::MAX as u64 {
            return Err(limit("頂点の位置", base_vertex, i32::MAX as u64));
        }
        plan.prims.push(PrimSlot {
            page: page_index as u32,
            base_vertex: base_vertex as i32,
            first_index: (page.index_bytes / INDEX_BYTES) as u32,
            index_count: index_count as u32,
        });
        page.vertex_bytes += vbytes;
        page.index_bytes += ibytes;
        page.uv_bytes += uvbytes;
        plan.geometry_bytes += vbytes + ibytes + uvbytes;
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
