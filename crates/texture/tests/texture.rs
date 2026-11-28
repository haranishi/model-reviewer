mod fixtures;

use fixtures::*;
use image::ExtendedColorType::{Rgba8, Rgba16};
use model_reviewer_core::texture::{Mime, chain_bytes};
use model_reviewer_texture::{TexError, decode, dimensions, strip_metadata};

#[test]
fn decodes_png_and_jpeg_to_rgba8() {
    let pixels = [1, 2, 3, 4, 80, 100, 120, 0];
    let bytes = png([2, 1], &pixels, Rgba8);
    assert_eq!(dimensions(&bytes), Ok([2, 1]));
    assert_eq!(decode(&bytes, 0).unwrap().levels[0], pixels);

    let wide: Vec<_> = [0x1111_u16, 0x8080, 0xfefe, 0x3030]
        .into_iter()
        .flat_map(u16::to_ne_bytes)
        .collect();
    assert_eq!(
        decode(&png([1, 1], &wide, Rgba16), 0).unwrap().levels[0],
        [17, 128, 254, 48]
    );

    for (jpeg, size, rgb) in [
        (rgb_jpeg(), [16, 8], [40, 100, 190]),
        (cmyk_jpeg(), [8, 8], [64; 3]),
    ] {
        assert_eq!(dimensions(&jpeg), Ok(size));
        let image = decode(&jpeg, 0).unwrap();
        assert_eq!(image.size, size);
        assert_eq!(image.levels[0].len(), (size[0] * size[1] * 4) as usize);
        for pixel in image.levels[0].as_chunks::<4>().0 {
            assert!(
                pixel[..3]
                    .iter()
                    .zip(rgb)
                    .all(|(&c, expected)| c.abs_diff(expected) <= 2),
                "{pixel:?}"
            );
            assert_eq!(pixel[3], 255);
        }
    }
}

#[test]
fn mips_average_in_linear() {
    let pixels = [
        0, 255, 47, 0, 255, 0, 47, 85, 255, 0, 47, 170, 0, 255, 47, 255,
    ];
    let image = decode(&png([2, 2], &pixels, Rgba8), 0).unwrap();
    let mean = &image.levels[1];
    assert!((187..=189).contains(&mean[0]));
    assert_eq!(mean[0], mean[1]);
    assert_eq!(&mean[2..], &[47, 128]);
}

#[test]
fn odd_sizes_end_at_1x1() {
    for (w, h) in [(5, 3), (5, 1), (1, 5)] {
        let mut pixels = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let value = if x == 4 || y == 4 || (h == 3 && y == 2) {
                    99 // 除かれる端の列・行
                } else if (w == 1 && y >= 2) || x >= 2 {
                    220
                } else {
                    20
                };
                pixels.extend([value; 4]);
            }
        }
        let chain = decode(&png([w, h], &pixels, Rgba8), 0).unwrap();
        assert_eq!(chain.levels.len(), 3);
        assert_eq!(chain.levels[1], [20, 20, 20, 20, 220, 220, 220, 220]);
        assert_eq!(chain.levels[2].len(), 4);
        assert_eq!(chain.levels[2][3], 120);
    }
}

#[test]
fn skip_drops_top_levels() {
    let pixels: Vec<_> = (0..9 * 5 * 4).map(|i| (i * 13) as u8).collect();
    let bytes = png([9, 5], &pixels, Rgba8);
    let full = decode(&bytes, 0).unwrap();
    for (skip, start, size) in [
        (0, 0, [9, 5]),
        (1, 1, [4, 2]),
        (2, 2, [2, 1]),
        (3, 3, [1, 1]),
        (255, 3, [1, 1]),
    ] {
        let chain = decode(&bytes, skip).unwrap();
        assert_eq!(chain.size, size);
        assert_eq!(chain.levels, full.levels[start..]);
        assert_eq!(
            chain.levels.iter().map(|p| p.len() as u64).sum::<u64>(),
            chain_bytes(size)
        );
    }
    for color in [
        image::ExtendedColorType::L16,
        image::ExtendedColorType::La16,
        image::ExtendedColorType::Rgb16,
        Rgba16,
    ] {
        let wide: Vec<_> = (0..9 * 5 * u16::from(color.channel_count()))
            .flat_map(|i| i.wrapping_mul(977).to_ne_bytes())
            .collect();
        let bytes = png([9, 5], &wide, color);
        assert_eq!(
            decode(&bytes, 1).unwrap().levels,
            decode(&bytes, 0).unwrap().levels[1..]
        );
    }
}

fn assert_too_large(header: &[u8], size: [u32; 2]) {
    assert_eq!(dimensions(header), Err(TexError::TooLarge { size }));
    assert_eq!(decode(header, 8), Err(TexError::TooLarge { size }));
}

#[test]
fn too_large_is_refused_before_decoding() {
    for size in [[8193, 1], [1, 8193]] {
        assert_too_large(&png_header(size, 8), size);
        assert_too_large(&jpeg_frame(0xc0, size.map(|n| n as u16), &[1]), size);
    }
    assert_too_large(&png_header([u32::MAX; 2], 8), [u32::MAX; 2]);
    assert!(matches!(
        dimensions(&png_header([0, 1], 8)),
        Err(TexError::Broken(_))
    ));
}

#[test]
fn oversized_pixel_count_is_refused_from_header() {
    for size in [[8192, 4097], [4097, 8192], [6000, 6000]] {
        assert_too_large(&png_header(size, 8), size);
        assert_too_large(&jpeg_frame(0xc0, size.map(|n| n as u16), &[1]), size);
    }
    for size in [[8192, 4096], [4096, 8192]] {
        assert_eq!(dimensions(&png_header(size, 8)), Ok(size));
        assert_eq!(
            dimensions(&jpeg_frame(0xc0, size.map(|n| n as u16), &[1])),
            Ok(size)
        );
    }
    // 画素数が範囲内でも、16 ビットの元のバッファ（256 MiB）は復号の上限を超える。
    let size = [8192, 4096];
    let mut bytes = png_header(size, 16);
    bytes.extend(&png([1, 1], &[0; 8], Rgba16)[33..]);
    assert_eq!(decode(&bytes, 0), Err(TexError::TooLarge { size }));
}

#[test]
fn broken_image_is_an_error() {
    assert_eq!(decode(b"GIF89a", 0), Err(TexError::Unsupported));
    let png = png([1, 1], &[1; 4], Rgba8);
    let jpeg = rgb_jpeg();
    assert_eq!(strip_metadata(&jpeg, Mime::Png), Err(TexError::Unsupported));
    for (bytes, mime) in [(&png, Mime::Png), (&jpeg, Mime::Jpeg)] {
        for len in 0..bytes.len() {
            assert!(decode(&bytes[..len], 0).is_err(), "{mime:?} {len}");
            assert!(
                strip_metadata(&bytes[..len], mime).is_err(),
                "{mime:?} {len}"
            );
        }
    }
    for bad in [
        [0xff, 0x02, 0, 2], // 未知の印
        [0xff, 0xe1, 0, 1], // 長さ欄自体より短い
        [0xff, 0xe1, 0xff, 0xff],
        [0xff, 0xd0, 0, 0], // 走査の外の再開マーカー
        [0xff, 0xdb, 0, 2], // 表が入っていない DQT
    ] {
        let bytes = [&jpeg[..2], &bad, &jpeg[2..]].concat();
        assert!(matches!(
            strip_metadata(&bytes, Mime::Jpeg),
            Err(TexError::Broken(_))
        ));
    }
    for chunk in [png_chunk(b"WHAT", &[]), vec![0xff; 8]] {
        let bytes = [&png[..33], &chunk, &png[33..]].concat();
        assert!(matches!(
            strip_metadata(&bytes, Mime::Png),
            Err(TexError::Broken(_))
        ));
    }
    let bad_data = [
        &png[..33],
        &png_chunk(b"IDAT", &[0; 4]),
        &png[png.len() - 12..],
    ]
    .concat();
    assert!(matches!(decode(&bad_data, 0), Err(TexError::Broken(_))));
}

// TIFF の向きは右に 90 度。付帯情報があっても画素を回転せず、貼るときは区画ごと除く。
const EXIF: &[u8] = b"II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";

#[test]
fn strip_removes_exif_and_text_keeps_pixels() {
    let png = png([2, 1], &[10, 20, 30, 0, 70, 80, 90, 255], Rgba8);
    let phys = png_chunk(b"pHYs", &[0, 0, 1, 44, 0, 0, 1, 44, 1]);
    let clean = [&png[..33], &phys, &png[33..]].concat();
    let mut marked = png[..33].to_vec();
    for (kind, data) in [
        (b"tEXt", b"Note\0memo".as_slice()),
        (b"eXIf", EXIF),
        (b"iTXt", b"Note\0\0\0\0\0memo"),
        (b"zTXt", b"Note\0\0\x78\x01\x01\0\0\xff\xff\0\0\0\x01"),
        (b"tIME", &[7, 230, 1, 1, 8, 0, 0]),
        (b"vpAg", b"memo"),
    ] {
        marked.extend(png_chunk(kind, data));
    }
    marked.extend(&phys);
    marked.extend(&png[33..]);
    assert_eq!(strip_metadata(&marked, Mime::Png).unwrap(), clean);
    assert_eq!(decode(&marked, 0), decode(&clean, 0));
    assert_eq!(decode(&clean, 0).unwrap().size, [2, 1]);

    // インデックスの画素では、パレットと透明度のチャンクも画素情報の一部になる。
    let gray = fixtures::png([2, 1], &[0, 1], image::ExtendedColorType::L8);
    let mut ihdr = gray[16..29].to_vec();
    ihdr[9] = 3;
    let indexed = [
        &gray[..8],
        &png_chunk(b"IHDR", &ihdr),
        &png_chunk(b"PLTE", &[120, 0, 0, 0, 0, 160]),
        &png_chunk(b"tRNS", &[64, 0]),
        &gray[33..],
    ]
    .concat();
    let marked = [
        &indexed[..33],
        &png_chunk(b"tEXt", b"Note\0memo"),
        &indexed[33..],
    ]
    .concat();
    assert_eq!(strip_metadata(&marked, Mime::Png).unwrap(), indexed);
    assert_eq!(
        decode(&indexed, 0).unwrap().levels[0],
        [120, 0, 0, 64, 0, 0, 160, 0]
    );

    let jpeg = rgb_jpeg();
    let mut clean = jpeg[..2].to_vec();
    segment(&mut clean, 0xe2, b"ICC_PROFILE\0\x01\x01");
    let mut marked = clean.clone();
    segment(&mut marked, 0xe1, &[b"Exif\0\0".as_slice(), EXIF].concat());
    segment(&mut marked, 0xe0, b"JFXX\0\x10thumbnail");
    segment(&mut marked, 0xe2, b"MPF\0index");
    for code in (0xe3..=0xed).chain([0xef, 0xfe]) {
        segment(&mut marked, code, b"note\xff\xd9");
    }
    clean.extend(&jpeg[2..]);
    marked.extend(&jpeg[2..]);
    assert_eq!(strip_metadata(&marked, Mime::Jpeg).unwrap(), clean);
    assert_eq!(decode(&marked, 0).unwrap(), decode(&clean, 0).unwrap());
}

#[test]
fn strip_removes_segments_between_scans() {
    let (clean, second_scan) = progressive_jpeg();
    let mut marked = clean[..second_scan].to_vec();
    segment(&mut marked, 0xe1, &[b"Exif\0\0".as_slice(), EXIF].concat());
    segment(&mut marked, 0xfe, b"note\xff\x00\xff\xda");
    marked.extend(&clean[second_scan..clean.len() - 2]);
    segment(&mut marked, 0xfe, b"last note");
    marked.extend([0xff, 0xd9]);
    assert_eq!(strip_metadata(&marked, Mime::Jpeg).unwrap(), clean);
    let image = decode(&clean, 0).unwrap();
    assert_eq!(image.size, [16, 8]);
    assert!(
        image.levels[0]
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[0].abs_diff(160) <= 1 && p[3] == 255)
    );
    assert_eq!(decode(&marked, 0).unwrap(), image);
}

#[test]
fn strip_drops_bytes_after_eoi() {
    for (bytes, mime) in [
        (rgb_jpeg(), Mime::Jpeg),
        (png([1, 1], &[42; 4], Rgba8), Mime::Png),
    ] {
        let with_trailer = [bytes.as_slice(), b"extra\xff\xd8\xff\xd9"].concat();
        assert_eq!(strip_metadata(&with_trailer, mime).unwrap(), bytes);
    }
}

#[test]
fn strip_is_idempotent() {
    let png = png([1, 1], &[42; 4], Rgba8);
    let marked_png = [&png[..33], &png_chunk(b"tEXt", b"Note\0memo"), &png[33..]].concat();
    let jpeg = cmyk_jpeg();
    let mut marked_jpeg = jpeg[..2].to_vec();
    segment(&mut marked_jpeg, 0xfe, b"memo");
    marked_jpeg.extend(&jpeg[2..]);
    for (bytes, clean, mime) in [
        (marked_png, png, Mime::Png),
        (marked_jpeg, jpeg, Mime::Jpeg),
    ] {
        let once = strip_metadata(&bytes, mime).unwrap();
        assert_eq!(once, clean);
        assert_eq!(strip_metadata(&once, mime).unwrap(), once);
    }
}
