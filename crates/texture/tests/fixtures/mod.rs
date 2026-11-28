use image::{ExtendedColorType, ImageEncoder, codecs::jpeg::JpegEncoder, codecs::png::PngEncoder};

pub fn png(size: [u32; 2], pixels: &[u8], color: ExtendedColorType) -> Vec<u8> {
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(pixels, size[0], size[1], color)
        .unwrap();
    bytes
}

pub fn rgb_jpeg() -> Vec<u8> {
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 100)
        .encode(
            &[40, 100, 190].repeat(16 * 8),
            16,
            8,
            ExtendedColorType::Rgb8,
        )
        .unwrap();
    bytes
}

pub fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
    chunk.extend(kind);
    chunk.extend(data);
    let mut crc = u32::MAX;
    for byte in &chunk[4..] {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    chunk.extend((!crc).to_be_bytes());
    chunk
}

pub fn png_header([width, height]: [u32; 2], depth: u8) -> Vec<u8> {
    let mut ihdr = width.to_be_bytes().to_vec();
    ihdr.extend(height.to_be_bytes());
    ihdr.extend([depth, 6, 0, 0, 0]);
    [b"\x89PNG\r\n\x1a\n".as_slice(), &png_chunk(b"IHDR", &ihdr)].concat()
}

pub fn segment(out: &mut Vec<u8>, code: u8, data: &[u8]) {
    out.extend([0xff, code]);
    out.extend((2 + data.len() as u16).to_be_bytes());
    out.extend(data);
}

pub fn jpeg_frame(code: u8, size: [u16; 2], channels: &[u8]) -> Vec<u8> {
    let mut frame = vec![8];
    frame.extend(size[1].to_be_bytes());
    frame.extend(size[0].to_be_bytes());
    frame.push(channels.len() as u8);
    for &id in channels {
        frame.extend([id, 0x11, 0]);
    }
    let mut bytes = vec![0xff, 0xd8];
    segment(&mut bytes, code, &frame);
    bytes
}

// 量子化の係数はすべて 1。DC・AC の表にはそれぞれ長さ 1 の符号を一つだけ置く。
fn tables(bytes: &mut Vec<u8>, dc_bits: u8) {
    let mut quant = [1; 65];
    quant[0] = 0;
    segment(bytes, 0xdb, &quant);
    for (id, symbol) in [(0, dc_bits), (0x10, 0)] {
        let mut table = [0; 18];
        table[0] = id;
        table[1] = 1;
        table[17] = symbol;
        segment(bytes, 0xc4, &table);
    }
}

pub fn cmyk_jpeg() -> Vec<u8> {
    let mut bytes = jpeg_frame(0xc0, [8, 8], b"CMYK");
    segment(&mut bytes, 0xee, b"Adobe\0\x64\0\0\0\0\0");
    tables(&mut bytes, 0);
    segment(
        &mut bytes,
        0xda,
        &[4, b'C', 0, b'M', 0, b'Y', 0, b'K', 0, 0, 63, 0],
    );
    bytes.extend([0, 0xff, 0xd9]); // 各成分が DC 差分 0 と EOB の 2 ビット。全成分の値は 128。
    bytes
}

pub fn progressive_jpeg() -> (Vec<u8>, usize) {
    let mut bytes = jpeg_frame(0xc2, [16, 8], &[1]);
    tables(&mut bytes, 8);
    segment(&mut bytes, 0xdd, &[0, 1]); // 8×8 のブロックごとに再開する。
    segment(&mut bytes, 0xda, &[1, 1, 0, 0, 0, 0]);
    // DC の差分は 255。詰めた後の FF は FF 00 として書く。
    bytes.extend([0x7f, 0xff, 0, 0xff, 0xd0, 0x7f, 0xff, 0]);
    let second_scan = bytes.len();
    segment(&mut bytes, 0xda, &[1, 1, 0, 1, 63, 0]);
    bytes.extend([0x7f, 0xff, 0xd0, 0x7f, 0xff, 0xd9]);
    (bytes, second_scan)
}
