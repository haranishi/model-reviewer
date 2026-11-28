//! シェーダーとパイプライン。描画先の形式と多重サンプル数が変わったときだけ作り直す。

use crate::targets::{DEPTH_FORMAT, MASK_FORMAT};

// 描画の定数（`shaders/frame.wgsl` の `Frame`）は scene・mask・grid の先頭に付ける。
pub const SCENE_WGSL: &str = concat!(
    include_str!("shaders/frame.wgsl"),
    include_str!("shaders/scene.wgsl"),
    include_str!("shaders/texture.wgsl")
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
    /// x: 格子の間隔。y: 面の不透明度。z: 断面の位置。w: 断面の軸と向き（`SectionPlane::code`、断面なしは 0）。
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
const UV_ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![11 => Float32x2];
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
    pub texture: wgpu::BindGroupLayout,
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
            texture: crate::textures::layout(device),
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
    pub scene_uv: wgpu::RenderPipeline,
    pub scene_box: wgpu::RenderPipeline,
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
        let texture_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("textured scene layout"),
            bind_group_layouts: &[Some(&layouts.frame), Some(&layouts.texture)],
            immediate_size: 0,
        });
        let multisample = wgpu::MultisampleState {
            count: samples,
            mask: !0,
            alpha_to_coverage_enabled: false,
        };
        let buffers = [
            Some(vertex_layout()),
            Some(instance_layout(&INSTANCE_ATTRS)),
            Some(wgpu::VertexBufferLayout {
                array_stride: crate::plan::UV_BYTES,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &UV_ATTRS,
            }),
        ];
        let shape = |v: ShapeVariant, vertex, layout, buffer_count| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(v.label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &layouts.scene_module,
                    entry_point: Some(vertex),
                    buffers: &buffers[..buffer_count],
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
        let plain = |v| shape(v, "vs_main", &frame_layout, 2);
        let faces = |label, fragment| ShapeVariant {
            label,
            fragment,
            polygon_mode: wgpu::PolygonMode::Fill,
            depth_write: true,
            depth_compare: wgpu::CompareFunction::Greater,
            bias: wgpu::DepthBiasState::default(),
            blend: None,
        };
        let scene = plain(faces("scene", "fs_main"));
        let scene_uv = shape(faces("scene_uv", "scene_uv"), "vs_uv", &texture_layout, 3);
        let scene_box = shape(
            faces("scene_box", "scene_box"),
            "vs_box",
            &texture_layout,
            2,
        );
        let xray = plain(ShapeVariant {
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
                Some(plain(ShapeVariant {
                    label: "lines over faces",
                    fragment: "fs_line",
                    polygon_mode: wgpu::PolygonMode::Line,
                    depth_write: false,
                    depth_compare: wgpu::CompareFunction::GreaterEqual,
                    bias: LINE_DEPTH_BIAS,
                    blend: None,
                })),
                Some(plain(ShapeVariant {
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
            scene_uv,
            scene_box,
            xray,
            line_overlay,
            line_only,
            grid,
            mask,
            outline,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SCENE_WGSL;

    #[test]
    fn textured_scene_shaders_validate() {
        use wgpu::naga::{ShaderStage, front::wgsl, valid};
        let module = wgsl::parse_str(SCENE_WGSL)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(SCENE_WGSL)));
        valid::Validator::new(valid::ValidationFlags::all(), valid::Capabilities::empty())
            .validate(&module)
            .expect("シェーダーを検証できる");
        for (stage, name) in [
            (ShaderStage::Vertex, "vs_uv"),
            (ShaderStage::Vertex, "vs_box"),
            (ShaderStage::Fragment, "scene_uv"),
            (ShaderStage::Fragment, "scene_box"),
        ] {
            assert!(
                module
                    .entry_points
                    .iter()
                    .any(|p| p.name == name && p.stage == stage),
                "{name}"
            );
        }
    }

    #[test]
    fn box_derivative_sign_matches_projected_winding() {
        use glam::{Vec2, Vec3, camera::rh};
        use wgpu::naga::{Expression, Literal, front::wgsl};

        let module = wgsl::parse_str(SCENE_WGSL).unwrap();
        let (_, constant) = module
            .constants
            .iter()
            .find(|(_, c)| c.name.as_deref() == Some("FRONT_DERIVATIVE_SIGN"))
            .unwrap();
        let Expression::Literal(Literal::F32(sign)) = module.global_expressions[constant.init]
        else {
            panic!("面の向きの符号");
        };
        assert_eq!(wgpu::FrontFace::default(), wgpu::FrontFace::Ccw);
        let eye = Vec3::new(3.0, 2.0, 4.0);
        let view = rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y);
        let projections = [
            (
                rh::proj::directx::perspective_infinite_reverse(0.7, 1.0, 0.05),
                true,
            ),
            (
                rh::proj::directx::orthographic(-1.0, 1.0, -1.0, 1.0, 20.0, 0.05),
                false,
            ),
        ];
        for (proj, perspective) in projections {
            for scale_x in [-1.0, 1.0] {
                for n in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z] {
                    let u = if n.x == 0.0 { Vec3::X } else { Vec3::Y };
                    let v = n.cross(u);
                    let world = [n, n + u, n + v].map(|p| p * Vec3::new(scale_x, 1.0, 1.0));
                    let [a, b, c] = world;
                    let to_eye = if perspective { eye - a } else { eye };
                    let front = (b - a).cross(c - a).dot(to_eye) > 0.0;
                    let [a, b, c] = world.map(|p| {
                        let ndc = (proj * view).project_point3(p);
                        Vec2::new(ndc.x, -ndc.y)
                    });
                    // ローカルの辺を画面 x,y の微分に直すと、外積に画面の符号付き面積の逆数が掛かる。
                    let area = (b - a).perp_dot(c - a);
                    assert!(
                        area * (if front { sign } else { -sign }) > 0.0,
                        "面 {n}、X倍率 {scale_x}"
                    );
                }
            }
        }
    }

    fn srgb_to_linear(c: u8) -> f32 {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    /// シェーダーの切り口の色が、sRGB の #D9822B をリニアにした値と合う（許容差つき）。
    #[test]
    fn cut_color_is_d9822b_in_linear() {
        let line = SCENE_WGSL
            .lines()
            .find(|line| line.starts_with("const CUT_COLOR"))
            .expect("切り口の色の定数がある");
        let inside = line
            .split("vec3<f32>(")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .expect("3つの数");
        let values: Vec<f32> = inside
            .split(',')
            .map(|v| v.trim().parse().expect("数"))
            .collect();
        assert_eq!(values.len(), 3);
        for (value, srgb) in values.iter().zip([0xD9u8, 0x82, 0x2B]) {
            let expected = srgb_to_linear(srgb);
            assert!((value - expected).abs() <= 1.0e-4, "{value} {expected}");
        }
    }

    /// 断面で捨てるのは、微分を求めた後（落とし穴：微分は一様な制御の流れの中でしか使えない）。
    #[test]
    fn scene_discards_after_derivatives() {
        let start = SCENE_WGSL.find("fn fs_main").expect("fs_main");
        let body = &SCENE_WGSL[start..];
        let derivative = body.find("dpdy(").expect("微分");
        let discard = body.find("discard").expect("捨てる");
        assert!(derivative < discard);
    }
}
