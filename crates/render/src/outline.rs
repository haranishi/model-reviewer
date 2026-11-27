//! 選択の輪郭線。選択中のインスタンスだけを深度なしでマスクへ描き、マスクの縁を描き終えた画像へ重ねる。
//!
//! 深度を使わないので、他の部品に隠れていても輪郭が出る（要件5.7、ux.md の S4）。
//! 輪郭は部品の縁に沿った二重線で、外は暗く内は明るい。琥珀色だけでは橙の部品と区別できない。
//! 裏面を膨らませる方式は、法線の無いモデルや鋭い角で破綻するので採らない。
//! 選択を変えてもバッファは書き換えない。描く範囲は `InstanceList::selected_ranges` の二分探索で求め、
//! 選択かインスタンスが変わったときだけ計算し直す。

use std::ops::Range;

use crate::instances::InstanceList;
use crate::pipelines::{Layouts, OutlineUniform, Pipelines};
use crate::targets::Targets;
use crate::upload::GpuGeometry;

/// 輪郭の内側（明るい）と外縁（暗い）。一覧の帯の琥珀とは別の色。
pub const OUTLINE_INNER_SRGB: [u8; 3] = [0xF4, 0xF7, 0xFB];
pub const OUTLINE_OUTER_SRGB: [u8; 3] = [0x14, 0x16, 0x1C];

/// 選択範囲ごとの描く範囲の控え。
#[derive(Default)]
pub struct SelectionCache {
    key: Option<(Range<u32>, u64)>,
    ranges: Vec<(u32, Range<u32>)>,
}

impl SelectionCache {
    pub fn ranges(
        &mut self,
        selected: &Range<u32>,
        generation: u64,
        list: &InstanceList,
    ) -> &[(u32, Range<u32>)] {
        let key = (selected.clone(), generation);
        if self.key.as_ref() != Some(&key) {
            self.ranges = list.selected_ranges(selected);
            self.key = Some(key);
        }
        &self.ranges
    }

    pub fn clear(&mut self) {
        self.key = None;
        self.ranges.clear();
    }
}

pub struct OutlineResources {
    /// 束縛が参照する。持ち続けるだけで読まない。
    _uniform: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
}

impl OutlineResources {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &Layouts,
        targets: &Targets,
    ) -> Self {
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("outline uniform"),
            size: std::mem::size_of::<OutlineUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dark = if targets.size[1] > 1200 { 3.0 } else { 2.0 };
        let inner = OUTLINE_INNER_SRGB.map(srgb_to_linear);
        let outer = OUTLINE_OUTER_SRGB.map(srgb_to_linear);
        let data = OutlineUniform {
            inner: [inner[0], inner[1], inner[2], 1.0],
            outer: [outer[0], outer[1], outer[2], 1.0],
            params: [
                dark,
                if targets.srgb_view { 0.0 } else { 1.0 },
                dark * 2.0,
                0.0,
            ],
        };
        queue.write_buffer(&uniform, 0, bytemuck::bytes_of(&data));
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("outline"),
            layout: &layouts.outline,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&targets.mask_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        Self {
            _uniform: uniform,
            bind_group,
        }
    }
}

/// マスクを描いてから、縁を解決先へ重ねる。描くものが無ければ何もしない。
#[allow(clippy::too_many_arguments)]
pub fn encode(
    encoder: &mut wgpu::CommandEncoder,
    pipelines: &Pipelines,
    targets: &Targets,
    resources: &OutlineResources,
    frame_bind_group: &wgpu::BindGroup,
    geometry: &GpuGeometry,
    instance_buffer: &wgpu::Buffer,
    ranges: &[(u32, Range<u32>)],
) {
    if ranges.is_empty() {
        return;
    }
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("selection mask"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &targets.mask_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipelines.mask);
        pass.set_bind_group(0, frame_bind_group, &[]);
        pass.set_vertex_buffer(1, instance_buffer.slice(..));
        crate::draw_ranges(&mut pass, geometry, ranges);
    }
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("selection outline"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &targets.draw_view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(&pipelines.outline);
    pass.set_bind_group(0, &resources.bind_group, &[]);
    pass.draw(0..3, 0..1);
}

pub fn srgb_to_linear(value: u8) -> f32 {
    let c = f32::from(value) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}
