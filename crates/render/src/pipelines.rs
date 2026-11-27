//! シェーダーとパイプライン。描画先の形式と多重サンプル数が変わったときだけ作り直す。

use crate::targets::{DEPTH_FORMAT, MASK_FORMAT};

// 描画の定数（`shaders/frame.wgsl` の `Frame`）は scene・mask・grid の先頭に付ける。
pub const SCENE_WGSL: &str = concat!(
    include_str!("shaders/frame.wgsl"),
    include_str!("shaders/scene.wgsl")
);
pub const MASK_WGSL: &str = concat!(
    include_str!("shaders/frame.wgsl"),
    include_str!("shaders/mask.wgsl")
);
pub const GRID_WGSL: &str = concat!(
    include_str!("shaders/frame.wgsl"),
    include_str!("shaders/grid.wgsl")
);
pub const OUTLINE_WGSL: &str = include_str!("shaders/outline.wgsl");

/// シェーダーの `Frame` と同じ並び（304バイト）。変形中も毎フレーム送るのはこれだけ。
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FrameUniform {
    pub view_proj: [[f32; 4]; 4],
    pub eye: [f32; 4],
    /// 選択範囲の始まりと終わり、sRGBへの変換の有無、表示（`display_code`）。
    pub selected: [u32; 4],
    pub inv_view_proj: [[f32; 4]; 4],
    /// 変形中の差分の行列。
    pub drag_delta: [[f32; 4]; 4],
    /// 差分の行列の法線用（左上3×3の逆転置）。列ごとに4要素目は詰め物。
    pub drag_normal: [[f32; 4]; 3],
    /// 変形中の範囲の始まりと終わり（始まり >= 終わりなら変形なし）、未使用2つ。
    pub drag: [u32; 4],
    /// x: 格子の間隔。y: 面の不透明度。z, w: 未使用。
    pub params: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<FrameUniform>() == 304);

/// 透かしの面の不透明度。
pub const XRAY_ALPHA: f32 = 0.35;

/// 「面と線」の線を面より手前に寄せる深度の偏り（反転Zなので正の値が手前）。実機（Metal）の画像で決めた。
/// 32ビット浮動小数の深度では一定分（`constant`）はほとんど効かず、傾きの分（`slope_scale`）が効く。
/// 傾きの分が0だと、斜めの面の上の線が約3分の1に途切れる（`tests/display.rs`）。
pub const LINE_DEPTH_BIAS: wgpu::DepthBiasState = wgpu::DepthBiasState {
    constant: 8,
    slope_scale: 1.0,
    clamp: 0.0,
};

/// シェーダーの `selected.w` に入れる表示の番号。
pub fn display_code(mode: crate::DisplayMode) -> u32 {
    match mode {
        crate::DisplayMode::Faces => 0,
        crate::DisplayMode::FacesAndLines => 1,
        crate::DisplayMode::Lines => 2,
        crate::DisplayMode::XRay => 3,
    }
}

/// シェーダーの `Outline` と同じ並び（48バイト）。
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct OutlineUniform {
    pub inner: [f32; 4],
    pub outer: [f32; 4],
    /// x: 暗い帯の太さ。y: sRGB変換。z: 輪郭全体の太さ。
    pub params: [f32; 4],
}

const VERTEX_ATTRS: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
const INSTANCE_ATTRS: [wgpu::VertexAttribute; 9] = wgpu::vertex_attr_array![
    2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4,
    6 => Float32x4, 7 => Float32x4, 8 => Float32x4,
    9 => Float32x4, 10 => Uint32x4,
];
/// マスクはワールド行列と NodeId だけを読む（NodeId は変形中の範囲の判定に使う）。
const MASK_INSTANCE_ATTRS: [wgpu::VertexAttribute; 5] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 0,
        shader_location: 2,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 16,
        shader_location: 3,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 32,
        shader_location: 4,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x4,
        offset: 48,
        shader_location: 5,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Uint32x4,
        offset: 128,
        shader_location: 10,
    },
];

fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: crate::plan::VERTEX_BYTES,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &VERTEX_ATTRS,
    }
}

fn instance_layout(
    attributes: &'static [wgpu::VertexAttribute],
) -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: crate::plan::INSTANCE_BYTES,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes,
    }
}

/// 形式に依存しない部品（束縛の配置とシェーダー）。
pub struct Layouts {
    pub frame: wgpu::BindGroupLayout,
    pub outline: wgpu::BindGroupLayout,
    scene_module: wgpu::ShaderModule,
    mask_module: wgpu::ShaderModule,
    grid_module: wgpu::ShaderModule,
    outline_module: wgpu::ShaderModule,
}

impl Layouts {
    pub fn new(device: &wgpu::Device) -> Self {
        let frame = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let outline = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("outline"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let module = |label: &str, source: &str| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            })
        };
        Self {
            frame,
            outline,
            scene_module: module("scene.wgsl", SCENE_WGSL),
            mask_module: module("mask.wgsl", MASK_WGSL),
            grid_module: module("grid.wgsl", GRID_WGSL),
            outline_module: module("outline.wgsl", OUTLINE_WGSL),
        }
    }
}

pub struct Pipelines {
    pub format: wgpu::TextureFormat,
    pub samples: u32,
    /// 線のパイプラインを作ったか（`POLYGON_MODE_LINE` を持つ装置だけ）。
    pub lines: bool,
    pub scene: wgpu::RenderPipeline,
    /// 透かし。半透明で重ね、深度は比べず書かない。
    pub xray: wgpu::RenderPipeline,
    /// 「面と線」の線。深度は「同じか手前」で比べ、偏りで面より手前に寄せる。書かない。
    pub line_overlay: Option<wgpu::RenderPipeline>,
    /// 「線」だけの表示。奥の線も見えるように深度を比べない。
    pub line_only: Option<wgpu::RenderPipeline>,
    pub grid: wgpu::RenderPipeline,
    pub mask: wgpu::RenderPipeline,
    pub outline: wgpu::RenderPipeline,
}

/// 形状を描くパイプラインの違いの部分。
struct ShapeVariant {
    label: &'static str,
    fragment: &'static str,
    polygon_mode: wgpu::PolygonMode,
    depth_write: bool,
    depth_compare: wgpu::CompareFunction,
    bias: wgpu::DepthBiasState,
    blend: Option<wgpu::BlendState>,
}

impl Pipelines {
    /// `lines` が偽なら線のパイプラインを作らない（`POLYGON_MODE_LINE` の無い装置で作ると失敗する）。
    pub fn new(
        device: &wgpu::Device,
        layouts: &Layouts,
        format: wgpu::TextureFormat,
        samples: u32,
        lines: bool,
    ) -> Self {
        let frame_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene layout"),
            bind_group_layouts: &[Some(&layouts.frame)],
            immediate_size: 0,
        });
        let outline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("outline layout"),
            bind_group_layouts: &[Some(&layouts.outline)],
            immediate_size: 0,
        });
        let multisample = wgpu::MultisampleState {
            count: samples,
            mask: !0,
            alpha_to_coverage_enabled: false,
        };
        let shape = |v: ShapeVariant| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(v.label),
                layout: Some(&frame_layout),
                vertex: wgpu::VertexState {
                    module: &layouts.scene_module,
                    entry_point: Some("vs_main"),
                    buffers: &[
                        Some(vertex_layout()),
                        Some(instance_layout(&INSTANCE_ATTRS)),
                    ],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                // 面の向きによる間引きはしない（鏡像の変換や裏表の不揃いなモデルで面が消えないように）。
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    polygon_mode: v.polygon_mode,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(v.depth_write),
                    depth_compare: Some(v.depth_compare),
                    stencil: wgpu::StencilState::default(),
                    bias: v.bias,
                }),
                multisample,
                fragment: Some(wgpu::FragmentState {
                    module: &layouts.scene_module,
                    entry_point: Some(v.fragment),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: v.blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // 反転Z：クリア値0、手前ほど大きい。
        let scene = shape(ShapeVariant {
            label: "scene",
            fragment: "fs_main",
            polygon_mode: wgpu::PolygonMode::Fill,
            depth_write: true,
            depth_compare: wgpu::CompareFunction::Greater,
            bias: wgpu::DepthBiasState::default(),
            blend: None,
        });
        let xray = shape(ShapeVariant {
            label: "scene xray",
            fragment: "fs_main",
            polygon_mode: wgpu::PolygonMode::Fill,
            depth_write: false,
            depth_compare: wgpu::CompareFunction::Always,
            bias: wgpu::DepthBiasState::default(),
            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
        });
        let (line_overlay, line_only) = if lines {
            (
                Some(shape(ShapeVariant {
                    label: "lines over faces",
                    fragment: "fs_line",
                    polygon_mode: wgpu::PolygonMode::Line,
                    depth_write: false,
                    depth_compare: wgpu::CompareFunction::GreaterEqual,
                    bias: LINE_DEPTH_BIAS,
                    blend: None,
                })),
                Some(shape(ShapeVariant {
                    label: "lines only",
                    fragment: "fs_line",
                    polygon_mode: wgpu::PolygonMode::Line,
                    depth_write: false,
                    depth_compare: wgpu::CompareFunction::Always,
                    bias: wgpu::DepthBiasState::default(),
                    blend: None,
                })),
            )
        } else {
            (None, None)
        };
        // 格子は形状の後に同じパスで描く。深度は比べるが書かない（深度はシェーダーが交点から出す）。
        let grid = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("grid"),
            layout: Some(&frame_layout),
            vertex: wgpu::VertexState {
                module: &layouts.grid_module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Greater),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample,
            fragment: Some(wgpu::FragmentState {
                module: &layouts.grid_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let primitive = wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        };
        let mask = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("selection mask"),
            layout: Some(&frame_layout),
            vertex: wgpu::VertexState {
                module: &layouts.mask_module,
                entry_point: Some("vs_main"),
                buffers: &[
                    Some(vertex_layout()),
                    Some(instance_layout(&MASK_INSTANCE_ATTRS)),
                ],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive,
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &layouts.mask_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: MASK_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let outline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("selection outline"),
            layout: Some(&outline_layout),
            vertex: wgpu::VertexState {
                module: &layouts.outline_module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &layouts.outline_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            format,
            samples,
            lines,
            scene,
            xray,
            line_overlay,
            line_only,
            grid,
            mask,
            outline,
        }
    }
}
