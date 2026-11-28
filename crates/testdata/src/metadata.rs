//! 付帯情報の除去試験で好きな区画・走査の前後へ挿せる、画素を含まないバイト列。

// リトルエンディアンの TIFF。最初の IFD は Orientation = 1 だけを持つ。
const TIFF: &[u8] = b"II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x01\0\0\0\0\0\0\0";

/// 長さと CRC を含む tEXt・eXIf。生成した PNG では IHDR の直後（33バイト目）へ挿せる。
pub fn png_chunks() -> Vec<u8> {
    [
        b"\0\0\0\x0ctEXtNote\0fixture\x1b\x2e\xca\x0d".as_slice(),
        b"\0\0\0\x1aeXIf",
        TIFF,
        b"\xbd\x8d\x18\x30",
    ]
    .concat()
}

/// マーカーと長さを含む EXIF の APP1 と注釈の COM。
pub fn jpeg_segments() -> Vec<u8> {
    [
        b"\xff\xe1\0\x22Exif\0\0".as_slice(),
        TIFF,
        b"\xff\xfe\0\x09fixture",
    ]
    .concat()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Pattern, jpeg_8x8, pattern_png};
    use image::{ImageDecoder, ImageFormat, codecs::png::PngDecoder};
    use std::io::Cursor;

    #[test]
    fn metadata_can_be_inserted_without_changing_pixels() {
        let mut png = pattern_png(Pattern::Quadrants, 8, 0);
        let pixels = image::load_from_memory(&png).unwrap().into_rgba8();
        png.splice(33..33, png_chunks());
        let mut decoder = PngDecoder::new(Cursor::new(&png)).unwrap();
        assert_eq!(decoder.exif_metadata().unwrap().as_deref(), Some(TIFF));
        assert_eq!(image::load_from_memory(&png).unwrap().into_rgba8(), pixels);

        let original = jpeg_8x8();
        let mut jpeg = original.clone();
        jpeg.splice(2..2, jpeg_segments());
        let mut at = 2;
        for (marker, data) in [
            (0xe1, [b"Exif\0\0".as_slice(), TIFF].concat()),
            (0xfe, b"fixture".to_vec()),
        ] {
            assert_eq!(&jpeg[at..at + 2], &[0xff, marker]);
            let end = at + 2 + u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]) as usize;
            assert_eq!(&jpeg[at + 4..end], data);
            at = end;
        }
        assert_eq!(&jpeg[at..], &original[2..]);
        if ImageFormat::Jpeg.reading_enabled() {
            let pixels = image::load_from_memory(&jpeg).unwrap().into_luma8();
            assert_eq!(pixels.dimensions(), (8, 8));
            assert_eq!(pixels.as_raw(), &[128; 64]);
        }
    }
}
