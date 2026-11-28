use crate::TexError;

const SOS: u8 = 0xda;
const EOI: u8 = 0xd9;

fn broken() -> TexError {
    TexError::Broken("JPEG の区画が欠けているか、印や長さが不正です".into())
}

fn is_frame(code: u8) -> bool {
    matches!(code, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf)
}

/// FF の詰め物を含めて区画の印を消費する。
fn marker(rest: &mut &[u8]) -> Result<u8, TexError> {
    if rest.first() != Some(&0xff) {
        return Err(broken());
    }
    let index = rest
        .iter()
        .position(|&byte| byte != 0xff)
        .ok_or_else(broken)?;
    let code = rest[index];
    *rest = &rest[index + 1..];
    Ok(code)
}

struct Segment<'a> {
    code: u8,
    data: &'a [u8],
    raw: &'a [u8],
}

fn segment<'a>(rest: &mut &'a [u8]) -> Result<Segment<'a>, TexError> {
    let start = *rest;
    let code = marker(rest)?;
    let data = match code {
        EOI => &[][..],
        // 枠、表、走査、APPn、注釈だけが長さを持つ区画として使える。
        0xc0..=0xc7 | 0xc9..=0xcf | 0xda..=0xdd | 0xe0..=0xef | 0xfe => {
            let length = usize::from(u16::from_be_bytes(*rest.first_chunk().ok_or_else(broken)?));
            let (body, next) = rest.split_at_checked(length).ok_or_else(broken)?;
            *rest = next;
            body.get(2..).ok_or_else(broken)?
        }
        _ => return Err(broken()),
    };
    Ok(Segment {
        code,
        data,
        raw: &start[..start.len() - rest.len()],
    })
}

fn frame_size(data: &[u8]) -> Result<[u32; 2], TexError> {
    let &[_, h1, h2, w1, w2, count] = data.first_chunk().ok_or_else(broken)?;
    if count == 0 || data.len() != 6 + usize::from(count) * 3 {
        return Err(broken());
    }
    Ok([
        u16::from_be_bytes([w1, w2]).into(),
        u16::from_be_bytes([h1, h2]).into(),
    ])
}

// DQT と DHT は一つの区画に複数の表を持てる。各表が区画の内側で終わることを確かめる。
fn check_tables(code: u8, mut data: &[u8]) -> Result<(), TexError> {
    if data.is_empty() {
        return Err(broken());
    }
    while let Some((&info, values)) = data.split_first() {
        let length = if code == 0xdb {
            match info >> 4 {
                0 => 64,
                1 => 128,
                _ => return Err(broken()),
            }
        } else {
            let counts = values.first_chunk::<16>().ok_or_else(broken)?;
            16 + counts.iter().map(|&n| usize::from(n)).sum::<usize>()
        };
        data = values.get(length..).ok_or_else(broken)?;
    }
    Ok(())
}

pub(super) fn dimensions(bytes: &[u8]) -> Result<[u32; 2], TexError> {
    let mut rest = &bytes[2..];
    loop {
        let part = segment(&mut rest)?;
        if is_frame(part.code) {
            return frame_size(part.data);
        }
        if matches!(part.code, SOS | EOI) {
            return Err(broken());
        }
    }
}

/// FF 00 は圧縮データの FF。RST0〜7 も走査を終えない。
fn scan_data<'a>(rest: &mut &'a [u8]) -> Result<&'a [u8], TexError> {
    let start = *rest;
    let mut cursor = start;
    loop {
        let offset = cursor
            .iter()
            .position(|&byte| byte == 0xff)
            .ok_or_else(broken)?;
        let boundary = &cursor[offset..];
        cursor = boundary;
        if !matches!(marker(&mut cursor)?, 0x00 | 0xd0..=0xd7) {
            *rest = boundary;
            return Ok(&start[..start.len() - boundary.len()]);
        }
    }
}

/// SOS ごとに圧縮データを写し、その次の区画から再び付帯情報を選別する。
pub(super) fn visit(bytes: &[u8], mut keep: impl FnMut(&[u8])) -> Result<usize, TexError> {
    let (soi, mut rest) = bytes.split_at(2);
    keep(soi);
    let mut has_frame = false;
    let mut has_scan = false;
    loop {
        let part = segment(&mut rest)?;
        match part.code {
            code if is_frame(code) => {
                if has_frame {
                    return Err(broken());
                }
                frame_size(part.data)?;
                has_frame = true;
            }
            SOS => {
                let &count = part.data.first().ok_or_else(broken)?;
                if !has_frame || count == 0 || part.data.len() != 4 + usize::from(count) * 2 {
                    return Err(broken());
                }
                let scan = scan_data(&mut rest)?;
                if scan.is_empty() {
                    return Err(broken());
                }
                has_scan = true;
                keep(part.raw);
                keep(scan);
                continue;
            }
            EOI => {
                if !has_scan {
                    return Err(broken());
                }
                keep(part.raw);
                return Ok(bytes.len() - rest.len());
            }
            // APP0 の別画像や APP2 の複数画像の索引には、元の撮影情報が入り得る。
            0xe0 if !part.data.starts_with(b"JFIF\0") => continue,
            0xe2 if !part.data.starts_with(b"ICC_PROFILE\0") => continue,
            0xe1 | 0xe3..=0xed | 0xef | 0xfe => continue,
            0xdb | 0xc4 => check_tables(part.code, part.data)?,
            0xcc if part.data.is_empty() || !part.data.len().is_multiple_of(2) => {
                return Err(broken());
            }
            0xdc | 0xdd if part.data.len() != 2 => return Err(broken()),
            _ => {}
        }
        keep(part.raw);
    }
}
