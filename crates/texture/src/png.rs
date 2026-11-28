use crate::TexError;

fn broken() -> TexError {
    TexError::Broken("PNG のチャンクが欠けているか、並びが不正です".into())
}

pub(super) fn dimensions(bytes: &[u8]) -> Result<[u32; 2], TexError> {
    let header = bytes[8..]
        .strip_prefix(b"\0\0\0\rIHDR")
        .ok_or_else(broken)?;
    let [width, height, ..] = header.as_chunks::<4>().0 else {
        return Err(broken());
    };
    Ok([u32::from_be_bytes(*width), u32::from_be_bytes(*height)])
}

/// 終端まで区画を検査し、残すチャンクは CRC ごと渡す。
pub(super) fn visit(bytes: &[u8], mut keep: impl FnMut(&[u8])) -> Result<usize, TexError> {
    let (signature, mut rest) = bytes.split_at(8);
    keep(signature);
    let mut first = true;
    let mut has_data = false;
    loop {
        let length = u32::from_be_bytes(*rest.first_chunk().ok_or_else(broken)?) as usize;
        let (chunk, next) = rest
            .split_at_checked(length.checked_add(12).ok_or_else(broken)?)
            .ok_or_else(broken)?;
        let kind = &chunk[4..8];
        if first && kind != b"IHDR" {
            return Err(broken());
        }
        match kind {
            b"IHDR" if first && length == 13 => keep(chunk),
            b"IDAT" => {
                has_data = true;
                keep(chunk);
            }
            b"IEND" if has_data && length == 0 => {
                keep(chunk);
                return Ok(bytes.len() - next.len());
            }
            b"PLTE" | b"tRNS" | b"gAMA" | b"cHRM" | b"sRGB" | b"iCCP" | b"sBIT" | b"pHYs" => {
                keep(chunk);
            }
            // 未知の必須チャンクは、取り除くと画像の解釈が変わり得る。
            _ if kind[0].is_ascii_lowercase() && kind.iter().all(u8::is_ascii_alphabetic) => {}
            _ => return Err(broken()),
        }
        rest = next;
        first = false;
    }
}
