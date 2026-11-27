//! `@builtin(frag_depth)` を多重サンプルの描画先で使えるかの確かめ（格子の方式を決める前提）。
//!
//! 4サンプルの色と深度へ、まず深度0.5の赤を全面に書き、次に左半分0.3・右半分0.7の深度を
//! シェーダーが出す緑を「大きいほど手前」で重ねる。左は赤が残り、右は緑になれば、
//! シェーダーが書いた深度で前後が付いている。

mod common;

use common::device;

const PROBE_WGSL: &str = r"
struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> Out {
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    var out: Out;
    // 頂点の深度は使わない値にしておく（シェーダーの深度が効いているかを見るため）。
    out.pos = vec4<f32>(x, y, 0.9, 1.0);
    out.ndc = vec2<f32>(x, y);
    return out;
}

struct Frag {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

@fragment
fn fs_red(in: Out) -> Frag {
    var f: Frag;
    f.color = vec4<f32>(1.0, 0.0, 0.0, 1.0);
    f.depth = 0.5;
    return f;
}

@fragment
fn fs_green(in: Out) -> Frag {
    var f: Frag;
    f.color = vec4<f32>(0.0, 1.0, 0.0, 1.0);
    f.depth = select(0.7, 0.3, in.ndc.x < 0.0);
    return f;
}
";

#[test]
#[ignore = "GPUが必要"]
fn frag_depth_orders_fragments_on_a_multisampled_target() {
    let (device, queue) = device();
    let size = wgpu::Extent3d {
        width: 64,
        height: 32,
        depth_or_array_layers: 1,
    };
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = |label, samples, format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let msaa = texture(
        "probe msaa",
        4,
        format,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    );
    let resolve = texture(
        "probe resolve",
        1,
        format,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let depth = texture(
        "probe depth",
        4,
        wgpu::TextureFormat::Depth32Float,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    );
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("probe"),
        source: wgpu::ShaderSource::Wgsl(PROBE_WGSL.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[],
        immediate_size: 0,
    });
    let pipeline = |entry: &str, write: bool| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(entry),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(write),
                depth_compare: Some(wgpu::CompareFunction::Greater),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 4,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(entry),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        })
    };
    let red = pipeline("fs_red", true);
    let green = pipeline("fs_green", false);

    let msaa_view = msaa.create_view(&Default::default());
    let resolve_view = resolve.create_view(&Default::default());
    let depth_view = depth.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("probe"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &msaa_view,
                depth_slice: None,
                resolve_target: Some(&resolve_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
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
        pass.set_pipeline(&red);
        pass.draw(0..3, 0..1);
        pass.set_pipeline(&green);
        pass.draw(0..3, 0..1);
    }
    let row = size.width * 4;
    let padded =
        row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("probe readback"),
        size: u64::from(padded * size.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &resolve,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(size.height),
            },
        },
        size,
    );
    queue.submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| result.expect("読み戻せる"));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("待てる");
    let mapped = slice.get_mapped_range().expect("読める");
    let pixel = |x: u32, y: u32| {
        let i = (y * padded + x * 4) as usize;
        [mapped[i], mapped[i + 1], mapped[i + 2]]
    };
    for y in [2, size.height / 2, size.height - 3] {
        assert_eq!(pixel(4, y), [255, 0, 0], "左は奥の緑が隠れて赤が残る");
        assert_eq!(pixel(size.width - 5, y), [0, 255, 0], "右は手前の緑");
    }
}
