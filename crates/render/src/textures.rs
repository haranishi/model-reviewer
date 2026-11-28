//! 表示中のモデルが持つ画像と、貼り方ごとのグループ1。

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use model_reviewer_core::scene::SceneRead;
use model_reviewer_core::texture::{
    IDENTITY_TRANSFORM, ImageId, Mip, MipChain, Sampling, TexId, UvMap, Wrap, box_sides,
};
use wgpu::util::DeviceExt;

use crate::image_upload::{ImageProgress, ImageUpload, descriptor};
use crate::instances::Batch;
use crate::{RenderError, targets::scoped};

pub(super) fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let entry = |binding, visibility, ty| wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty,
        count: None,
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("base color material"),
        entries: &[
            entry(
                0,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
            ),
            entry(
                1,
                wgpu::ShaderStages::FRAGMENT,
                wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            ),
            entry(
                2,
                wgpu::ShaderStages::VERTEX_FRAGMENT,
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(48),
                },
            ),
        ],
    })
}

fn sampler_desc(s: Sampling) -> wgpu::SamplerDescriptor<'static> {
    let wrap = |mode| match mode {
        Wrap::Repeat => wgpu::AddressMode::Repeat,
        Wrap::Mirror => wgpu::AddressMode::MirrorRepeat,
        Wrap::Clamp => wgpu::AddressMode::ClampToEdge,
    };
    let filter = |linear| {
        if linear {
            wgpu::FilterMode::Linear
        } else {
            wgpu::FilterMode::Nearest
        }
    };
    wgpu::SamplerDescriptor {
        address_mode_u: wrap(s.wrap[0]),
        address_mode_v: wrap(s.wrap[1]),
        mag_filter: filter(s.mag_linear),
        min_filter: filter(s.min_linear),
        mipmap_filter: match s.mip {
            Mip::None | Mip::Nearest => wgpu::MipmapFilterMode::Nearest,
            Mip::Linear => wgpu::MipmapFilterMode::Linear,
        },
        lod_max_clamp: if s.mip == Mip::None { 0.0 } else { 32.0 },
        ..Default::default()
    }
}

fn uniform(device: &wgpu::Device, map: UvMap, cutoff: Option<f32>) -> wgpu::Buffer {
    let (transform, [width, height], kind) = match map {
        UvMap::Vertex { transform, .. } => (transform, [1.0; 2], 0.0),
        UvMap::Box { size, aspect } => (IDENTITY_TRANSFORM, box_sides(size, aspect), 1.0),
    };
    let [a, b, tx, c, d, ty] = transform;
    let rows = [
        [a, b, tx, 0.0],
        [c, d, ty, 0.0],
        [cutoff.unwrap_or(-1.0), width, kind, height],
    ];
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("texture parameters"),
        contents: bytemuck::cast_slice(&rows),
        usage: wgpu::BufferUsages::UNIFORM,
    })
}

fn bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    uniform: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("base color material"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniform.as_entire_binding(),
            },
        ],
    })
}

struct Placeholder {
    view: wgpu::TextureView,
    // 届く前は、MASK の材質も基本色の面として見せる。
    uniform: wgpu::Buffer,
}

impl Placeholder {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let white = device.create_texture_with_data(
            queue,
            &descriptor([1, 1], 1),
            wgpu::util::TextureDataOrder::MipMajor,
            &[255; 4],
        );
        Self {
            view: white.create_view(&Default::default()),
            uniform: uniform(
                device,
                UvMap::Box {
                    size: 1.0,
                    aspect: 1.0,
                },
                None,
            ),
        }
    }
}

pub(super) struct Material {
    image: ImageId,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    pub map: UvMap,
    pub group: wgpu::BindGroup,
    pub ready: bool,
}

#[derive(Default)]
pub(super) struct GpuTextures {
    white: Option<Placeholder>,
    samplers: HashMap<Sampling, wgpu::Sampler>,
    pub materials: HashMap<TexId, Material>,
    images: HashMap<ImageId, (wgpu::TextureView, u64)>,
    pending: Option<ImageUpload>,
    pub bytes: u64,
}

impl GpuTextures {
    pub fn count(&self) -> u32 {
        self.images.len() as u32
    }

    /// 貼り方の番号は文書内で不変。編集中に足した番号も、その描画が初めて現れた回に用意する。
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        scene: &impl SceneRead,
        batches: &[Batch],
    ) -> Result<(), RenderError> {
        for id in batches.iter().filter_map(|b| b.tex) {
            if self.materials.contains_key(&id) {
                continue;
            }
            let Some(usage) = scene.tex_use(id) else {
                continue;
            };
            let Some(image) = usage.image else { continue };
            let sampler = match self.samplers.entry(usage.sampling) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => entry.insert(scoped(device, || {
                    device.create_sampler(&sampler_desc(usage.sampling))
                })?),
            };
            let resident = self.images.get(&image);
            if resident.is_none() && self.white.is_none() {
                self.white = Some(scoped(device, || Placeholder::new(device, queue))?);
            }
            let (uniform, group) = scoped(device, || {
                let uniform = uniform(device, usage.map, usage.cutoff);
                let (view, params) = match resident {
                    Some((view, _)) => (view, &uniform),
                    None => {
                        let white = self.white.as_ref().expect("未転送の画像の代用");
                        (&white.view, &white.uniform)
                    }
                };
                let group = bind(device, layout, view, sampler, params);
                (uniform, group)
            })?;
            self.materials.insert(
                id,
                Material {
                    image,
                    sampler: sampler.clone(),
                    uniform,
                    map: usage.map,
                    group,
                    ready: resident.is_some(),
                },
            );
        }
        Ok(())
    }

    pub fn begin(
        &mut self,
        device: &wgpu::Device,
        id: ImageId,
        chain: MipChain,
    ) -> Result<(), RenderError> {
        if self.pending.as_ref().is_some_and(|p| p.id != id) {
            return Err(RenderError::Device("別の画像を転送しています".into()));
        }
        self.pending = Some(ImageUpload::new(device, id, chain)?);
        Ok(())
    }

    pub fn step(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        budget: u64,
    ) -> ImageProgress {
        let Some(job) = &mut self.pending else {
            return ImageProgress::default();
        };
        let uploaded_bytes = job.step(queue, budget);
        if uploaded_bytes > 0 {
            queue.submit([]);
        }
        let completed = if job.done() {
            let job = self.pending.take().expect("転送済みの画像");
            let view = job.texture.create_view(&Default::default());
            for material in self.materials.values_mut().filter(|m| m.image == job.id) {
                material.group = bind(device, layout, &view, &material.sampler, &material.uniform);
                material.ready = true;
            }
            if let Some((_, old_bytes)) = self.images.insert(job.id, (view, job.bytes)) {
                self.bytes -= old_bytes;
            }
            self.bytes += job.bytes;
            Some(job.id)
        } else {
            None
        };
        ImageProgress {
            uploaded_bytes,
            completed,
        }
    }
}
