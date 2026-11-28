use std::io::Cursor;

use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ExtendedColorType, GrayImage, ImageEncoder, ImageFormat, Luma, Rgba, RgbaImage};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pattern {
    Checker,
    Stripes,
    Noise,
    /// 左上から右へ赤・緑、下の段が青・黄。
    Quadrants,
}

/// 辺が size 画素の RGBA8。市松は白黒の1画素交互、縦縞の色とノイズは種で決まる。
pub fn pattern_png(kind: Pattern, size: u32, seed: u32) -> Vec<u8> {
    encode_pattern(kind, size, seed, false)
}

pub(crate) fn encode_pattern(kind: Pattern, size: u32, seed: u32, mask: bool) -> Vec<u8> {
    assert!(size > 0, "画像の辺は1画素以上です");
    let half = size.div_ceil(2);
    let stripe_width = (size / 8).max(1);
    let [r, g, b, _] = seed.wrapping_mul(0x9e37_79b1).to_le_bytes();
    // xorshift32 は 0 から進まないため、種 0 にも非ゼロの初期状態を割り当てる。
    let mut state = if seed == 0 { 0x1234_5678 } else { seed };
    let pixels = RgbaImage::from_fn(size, size, |x, y| {
        let [r, g, b] = match kind {
            Pattern::Checker if (x ^ y) & 1 == 0 => [0; 3],
            Pattern::Checker => [255; 3],
            Pattern::Stripes if (x / stripe_width).is_multiple_of(2) => [r, g, b],
            Pattern::Stripes => [!r, !g, !b],
            Pattern::Noise => {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let [r, g, b, _] = state.to_le_bytes();
                [r, g, b]
            }
            Pattern::Quadrants => match (x < half, y < half) {
                (true, true) => [255, 0, 0],
                (false, true) => [0, 255, 0],
                (true, false) => [0, 0, 255],
                (false, false) => [255, 255, 0],
            },
        };
        Rgba([r, g, b, if mask && x >= half { 0 } else { 255 }])
    });
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::Up)
        .write_image(&pixels, size, size, ExtendedColorType::Rgba8)
        .expect("PNG");
    png
}

/// 明度128の8×8画像。JPEG の機能が有効なら同じ画素をその場で符号化する。
pub fn jpeg_8x8() -> Vec<u8> {
    if ImageFormat::Jpeg.writing_enabled() {
        let mut bytes = Cursor::new(Vec::new());
        GrayImage::from_pixel(8, 8, Luma([128]))
            .write_to(&mut bytes, ImageFormat::Jpeg)
            .expect("JPEG");
        return bytes.into_inner();
    }
    GRAY_JPEG.to_vec()
}

// 8×8 の灰色は中心化後の全係数が 0。DC の差 0 と AC の EOB に各1ビットの符号 0 を割り当てた。
const GRAY_JPEG: &[u8] = &[
    0xff, 0xd8, 0xff, 0xdb, 0, 67, 0, // DQT
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    0xff, 0xc0, 0, 11, 8, 0, 8, 0, 8, 1, 1, 0x11, 0, // SOF0
    0xff, 0xc4, 0, 38, // DHT
    0x00, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0xff, 0xda, 0, 8, 1, 1, 0, 0, 63, 0, // SOS
    0x3f, 0xff, 0xd9,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_are_deterministic() {
        for kind in [
            Pattern::Checker,
            Pattern::Stripes,
            Pattern::Noise,
            Pattern::Quadrants,
        ] {
            let bytes = pattern_png(kind, 9, 0);
            assert_eq!(bytes, pattern_png(kind, 9, 0));
            let image = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
                .unwrap()
                .into_rgba8();
            assert_eq!(image.dimensions(), (9, 9));
            let rgb = |x, y| image.get_pixel(x, y).0;
            match kind {
                Pattern::Checker => {
                    assert_eq!(rgb(0, 0), [0, 0, 0, 255]);
                    assert_eq!(rgb(1, 0), [255; 4]);
                    assert_eq!(rgb(1, 1), rgb(0, 0));
                }
                Pattern::Stripes => {
                    assert_ne!(rgb(1, 0), rgb(0, 0));
                    assert_eq!(rgb(1, 1), rgb(1, 0));
                }
                Pattern::Noise => {
                    assert_eq!(rgb(0, 0), [165, 90, 152, 255]);
                    assert_eq!(rgb(1, 0), [163, 36, 91, 255]);
                    assert_ne!(bytes, pattern_png(kind, 9, 1));
                }
                Pattern::Quadrants => {
                    assert_eq!(rgb(0, 0), [255, 0, 0, 255]);
                    assert_eq!(rgb(8, 0), [0, 255, 0, 255]);
                    assert_eq!(rgb(0, 8), [0, 0, 255, 255]);
                    assert_eq!(rgb(8, 8), [255, 255, 0, 255]);
                }
            }
        }
    }
}
