//! 描画先の画像。大きさが変わったときだけ作り直す。
//!
//! 解決先は Rgba8Unorm で作り、`view_formats` に sRGB を足す。描くときは sRGB のビューを使い、
//! eguiには Unorm のビューを渡す（`register_native_texture` は Rgba8Unorm を要求し、値をガンマ値として扱う）。
//! sRGB のビューが作れない環境では、Unorm へ直接描いてシェーダーの最後で変換する。

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use crate::RenderError;

/// 多重サンプルの数。確保に失敗したら1へ落とす。実測で見直す。
pub const MSAA_SAMPLES: u32 = 4;
/// 描画先の一辺の上限。アダプタの上限がこれより小さければそちらに合わせる。
pub const MAX_DIMENSION: u32 = 8192;
pub const RESOLVE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub const SRGB_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// eguiへ渡す画像の形式。
pub fn color_format() -> wgpu::TextureFormat {
    RESOLVE_FORMAT
}

/// 物理ピクセルの大きさを上限に収める。縦横比は保つ。どちらかが0なら `None`（描かない）。
pub fn fit_size(size_px: [u32; 2], max_dim: u32) -> Option<[u32; 2]> {
    let [w, h] = size_px;
    if w == 0 || h == 0 {
        return None;
    }
    let max_dim = max_dim.max(1);
    let longest = w.max(h);
    if longest <= max_dim {
        return Some([w, h]);
    }
    let scale = f64::from(max_dim) / f64::from(longest);
    let fit = |v: u32| ((f64::from(v) * scale).round() as u32).clamp(1, max_dim);
    Some([fit(w), fit(h)])
}

pub struct Targets {
    pub size: [u32; 2],
    pub samples: u32,
    /// 真なら sRGB のビューへ描く。偽ならシェーダーで変換する。
    pub srgb_view: bool,
    pub resolve: wgpu::Texture,
    /// eguiへ渡す Unorm のビュー。
    pub resolve_view: wgpu::TextureView,
    /// 描くときのビュー（sRGB か Unorm）。
    pub draw_view: wgpu::TextureView,
    pub msaa: Option<(wgpu::Texture, wgpu::TextureView)>,
    pub depth: wgpu::Texture,
    pub depth_view: wgpu::TextureView,
    pub mask: wgpu::Texture,
    pub mask_view: wgpu::TextureView,
}

impl Targets {
    pub fn draw_format(&self) -> wgpu::TextureFormat {
        if self.srgb_view {
            SRGB_FORMAT
        } else {
            RESOLVE_FORMAT
        }
    }

    pub fn create(
        device: &wgpu::Device,
        size: [u32; 2],
        samples: u32,
        srgb_view: bool,
    ) -> Result<Self, RenderError> {
        scoped(device, || {
            Self::create_unchecked(device, size, samples, srgb_view)
        })
    }

    fn create_unchecked(device: &wgpu::Device, size: [u32; 2], samples: u32, srgb: bool) -> Self {
        let extent = wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        };
        let view_formats: &[wgpu::TextureFormat] = if srgb { &[SRGB_FORMAT] } else { &[] };
        let resolve = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("scene resolve"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: RESOLVE_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats,
        });
        let resolve_view = resolve.create_view(&wgpu::TextureViewDescriptor::default());
        let draw_format = if srgb { SRGB_FORMAT } else { RESOLVE_FORMAT };
        let draw_view = resolve.create_view(&wgpu::TextureViewDescriptor {
            label: Some("scene draw view"),
            format: Some(draw_format),
            ..Default::default()
        });
        let msaa = (samples > 1).then(|| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("scene msaa color"),
                size: extent,
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format: draw_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            (texture, view)
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("scene depth"),
            size: extent,
            mip_level_count: 1,
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
        let mask = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("selection mask"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: MASK_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let mask_view = mask.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            size,
            samples,
            srgb_view: srgb,
            resolve,
            resolve_view,
            draw_view,
            msaa,
            depth,
            depth_view,
            mask,
            mask_view,
        }
    }

    pub fn destroy(&self) {
        self.resolve.destroy();
        if let Some((texture, _)) = &self.msaa {
            texture.destroy();
        }
        self.depth.destroy();
        self.mask.destroy();
    }
}

/// 検証エラーとメモリ不足を捕まえて `RenderError` にする。捕まえなかったエラーで落ちないようにする。
pub(crate) fn scoped<T>(device: &wgpu::Device, f: impl FnOnce() -> T) -> Result<T, RenderError> {
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let value = f();
    let validation_err = block_on(validation.pop()).flatten();
    let oom_err = block_on(oom.pop()).flatten();
    if oom_err.is_some() {
        return Err(RenderError::OutOfMemory);
    }
    if let Some(err) = validation_err {
        return Err(RenderError::Device(err.to_string()));
    }
    Ok(value)
}

/// ネイティブの wgpu では、エラーの範囲を閉じる future はすぐに終わる。終わらなければ諦めて `None`。
pub(crate) fn block_on<F: Future>(future: F) -> Option<F::Output> {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..10_000 {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return Some(value);
        }
        std::thread::yield_now();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::fit_size;

    #[test]
    fn fit_size_keeps_small_and_clamps_large() {
        assert_eq!(fit_size([0, 10], 8192), None);
        assert_eq!(fit_size([10, 0], 8192), None);
        assert_eq!(fit_size([3024, 1964], 8192), Some([3024, 1964]));
        assert_eq!(fit_size([16384, 8192], 8192), Some([8192, 4096]));
        assert_eq!(fit_size([100, 50], 10), Some([10, 5]));
    }
}
