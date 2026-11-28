//! 1枚の RGBA8 画像を、縮小版も含めて予算内の長方形に分けて送る。

use model_reviewer_core::texture::{ImageId, MipChain, chain_bytes};

use crate::{RenderError, plan, targets::scoped};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImageProgress {
    /// この呼び出しで送ったバイト数。
    pub uploaded_bytes: u64,
    /// 全段を送り終え、描画に使えるようになった画像。
    pub completed: Option<ImageId>,
}

pub(super) fn descriptor(size: [u32; 2], levels: u32) -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: Some("base color image"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    }
}

fn level_size(size: [u32; 2], level: u32) -> [u32; 2] {
    size.map(|side| (side >> level).max(1))
}

fn validate(chain: &MipChain, device_max_side: u32) -> Result<(), RenderError> {
    let [w, h] = chain.size;
    let max_side = device_max_side.min(4096);
    if w.max(h) > max_side {
        return Err(plan::limit(
            "画像の長辺",
            u64::from(w.max(h)),
            u64::from(max_side),
        ));
    }
    if w == 0 || h == 0 || chain.levels.len() != (w.max(h).ilog2() + 1) as usize {
        return Err(RenderError::Device(
            "画像の寸法または縮小版の段数が不正です".into(),
        ));
    }
    for (level, pixels) in chain.levels.iter().enumerate() {
        let [w, h] = level_size(chain.size, level as u32);
        if pixels.len() != (w * h * 4) as usize {
            return Err(RenderError::Device(format!(
                "縮小版の第{level}段の画素数が一致しません"
            )));
        }
    }
    Ok(())
}

/// 行頭からは複数行をまとめる。予算が1行に満たないときも1画素単位で進める。
fn region([w, h]: [u32; 2], texel: u32, budget: u64) -> Option<(wgpu::Origin3d, wgpu::Extent3d)> {
    let available = (budget / 4).min(u64::from(w * h - texel)) as u32;
    if available == 0 {
        return None;
    }
    let x = texel % w;
    let (width, height) = if x == 0 && available >= w {
        (w, available / w)
    } else {
        ((w - x).min(available), 1)
    };
    Some((
        wgpu::Origin3d {
            x,
            y: texel / w,
            z: 0,
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    ))
}

pub(super) struct ImageUpload {
    pub id: ImageId,
    pub texture: wgpu::Texture,
    pub bytes: u64,
    chain: MipChain,
    level: u32,
    texel: u32,
}

impl ImageUpload {
    pub fn new(device: &wgpu::Device, id: ImageId, chain: MipChain) -> Result<Self, RenderError> {
        validate(&chain, device.limits().max_texture_dimension_2d)?;
        let texture = scoped(device, || {
            device.create_texture(&descriptor(chain.size, chain.levels.len() as u32))
        })?;
        Ok(Self {
            id,
            texture,
            bytes: chain_bytes(chain.size),
            chain,
            level: 0,
            texel: 0,
        })
    }

    pub fn done(&self) -> bool {
        self.level as usize == self.chain.levels.len()
    }

    pub fn step(&mut self, queue: &wgpu::Queue, budget: u64) -> u64 {
        let mut sent = 0;
        while !self.done() {
            let size = level_size(self.chain.size, self.level);
            let Some((origin, extent)) = region(size, self.texel, budget - sent) else {
                break;
            };
            let count = extent.width * extent.height;
            let start = self.texel as usize * 4;
            let end = start + count as usize * 4;
            let pixels = &mut self.chain.levels[self.level as usize];
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: self.level,
                    origin,
                    aspect: wgpu::TextureAspect::All,
                },
                &pixels[start..end],
                wgpu::TexelCopyBufferLayout {
                    bytes_per_row: Some(extent.width * 4),
                    ..Default::default()
                },
                extent,
            );
            self.texel += count;
            sent += u64::from(count) * 4;
            if self.texel == size[0] * size[1] {
                *pixels = Vec::new();
                self.texel = 0;
                self.level += 1;
            }
        }
        sent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_regions_cover_each_texel_within_budget() {
        for size @ [w, h] in [[1, 1], [1, 9], [9, 1], [7, 5]] {
            for budget in [4, 7, 15, 28, 65, u64::MAX] {
                assert!(region(size, 0, 3).is_none());
                let mut next = 0;
                while let Some((origin, extent)) = region(size, next, budget) {
                    assert!(u64::from(extent.width * extent.height) * 4 <= budget);
                    assert!(origin.x + extent.width <= w && origin.y + extent.height <= h);
                    for y in origin.y..origin.y + extent.height {
                        for x in origin.x..origin.x + extent.width {
                            assert_eq!(y * w + x, next);
                            next += 1;
                        }
                    }
                }
                assert_eq!(next, w * h);
            }
        }
    }

    #[test]
    fn incomplete_or_oversized_mips_are_rejected_before_allocation() {
        let mut chain = MipChain {
            size: [5, 2],
            levels: vec![vec![0; 40], vec![0; 8], vec![0; 4]],
        };
        assert!(validate(&chain, 5).is_ok());
        assert!(validate(&chain, 4).is_err());
        chain.levels[1].pop();
        assert!(validate(&chain, 5).is_err());
        chain.levels.pop();
        assert!(validate(&chain, 5).is_err());
        chain.size = [0, 2];
        assert!(validate(&chain, 5).is_err());
    }
}
