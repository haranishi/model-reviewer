use std::sync::LazyLock;

use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
use model_reviewer_core::{
    color::{linear_to_srgb8, srgb8_to_linear},
    texture::MipChain,
};

// リニア値は 16 ビット。4 画素の合計は u32 に収まり、画素ごとのべき乗が要らない。
struct ColorTables {
    linear: [u32; 256],
    srgb: Box<[u8]>,
}

static TABLES: LazyLock<ColorTables> = LazyLock::new(|| {
    let scale = f32::from(u16::MAX);
    ColorTables {
        linear: std::array::from_fn(|i| (srgb8_to_linear([i as u8; 3])[0] * scale).round() as u32),
        srgb: (0..=u16::MAX)
            .map(|i| linear_to_srgb8([f32::from(i) / scale; 3])[0])
            .collect(),
    }
});

fn half(src: &impl GenericImageView<Pixel = Rgba<u8>>) -> RgbaImage {
    let (w, h) = src.dimensions();
    let tables = &*TABLES;
    RgbaImage::from_fn((w / 2).max(1), (h / 2).max(1), |x, y| {
        let (left, top) = (2 * x, 2 * y);
        let (right, bottom) = ((left + 1).min(w - 1), (top + 1).min(h - 1));
        let samples = [
            src.get_pixel(left, top),
            src.get_pixel(right, top),
            src.get_pixel(left, bottom),
            src.get_pixel(right, bottom),
        ];
        let color = |channel: usize| {
            let sum: u32 = samples
                .iter()
                .map(|p| tables.linear[usize::from(p[channel])])
                .sum();
            tables.srgb[((sum + 2) / 4) as usize]
        };
        let alpha: u16 = samples.iter().map(|p| u16::from(p[3])).sum();
        Rgba([color(0), color(1), color(2), ((alpha + 2) / 4) as u8])
    })
}

pub(super) fn chain(image: DynamicImage, skip: u8) -> MipChain {
    // 元の段を省く場合、RGB や 16 ビットの原寸画像と同時に原寸 RGBA8 を確保しない。
    let mut pixels = if skip == 0 || image.dimensions() == (1, 1) {
        image.into_rgba8()
    } else {
        let first = half(&image);
        drop(image);
        first
    };
    for _ in 1..skip {
        if pixels.dimensions() == (1, 1) {
            break;
        }
        pixels = half(&pixels);
    }
    let size = [pixels.width(), pixels.height()];
    let mut levels = Vec::new();
    while pixels.dimensions() != (1, 1) {
        let smaller = half(&pixels);
        levels.push(pixels.into_raw());
        pixels = smaller;
    }
    levels.push(pixels.into_raw());
    MipChain { size, levels }
}
