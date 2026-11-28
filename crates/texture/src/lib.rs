//! テクスチャの復号、縮小と貼り付け用の付帯情報除去。

mod jpeg;
mod mip;
mod png;

use std::{fmt, io::Cursor};

use image::{ImageError, ImageFormat, ImageReader, Limits};
use model_reviewer_core::texture::{Mime, MipChain, sniff};

const MAX_SIDE: u32 = 8192;
const MAX_PIXELS: u64 = 8192 * 4096;
const MAX_ALLOC: u64 = 160 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum TexError {
    Unsupported,
    TooLarge { size: [u32; 2] },
    Broken(String),
}

impl TexError {
    pub fn message_ja(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for TexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("PNG か JPEG の画像を選んでください"),
            Self::TooLarge { size: [w, h] } => {
                write!(f, "画像が大きすぎます（{w} × {h} 画素）")
            }
            Self::Broken(reason) => write!(f, "画像を読めません: {reason}"),
        }
    }
}

impl std::error::Error for TexError {}

/// 画素を展開せず、ヘッダーの寸法と画素数の上限を確かめる。
pub fn dimensions(bytes: &[u8]) -> Result<[u32; 2], TexError> {
    header(bytes).map(|(_, size)| size)
}

fn header(bytes: &[u8]) -> Result<(Mime, [u32; 2]), TexError> {
    let mime = sniff(bytes).ok_or(TexError::Unsupported)?;
    let size = match mime {
        Mime::Png => png::dimensions(bytes)?,
        Mime::Jpeg => jpeg::dimensions(bytes)?,
    };
    let [w, h] = size;
    if size.contains(&0) {
        return Err(TexError::Broken("幅か高さが 0 です".into()));
    }
    if w.max(h) > MAX_SIDE || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(TexError::TooLarge { size });
    }
    Ok((mime, size))
}

/// 元の画素順で RGBA8 に復号する。省く段数が多い場合も最後の 1×1 は残す。
pub fn decode(bytes: &[u8], skip: u8) -> Result<MipChain, TexError> {
    let (mime, size) = header(bytes)?;
    let (format, end) = match mime {
        Mime::Png => (ImageFormat::Png, png::visit(bytes, |_| {})?),
        Mime::Jpeg => (ImageFormat::Jpeg, jpeg::visit(bytes, |_| {})?),
    };
    let mut reader = ImageReader::with_format(Cursor::new(&bytes[..end]), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    let image = reader.decode().map_err(|error| match error {
        ImageError::Limits(_) => TexError::TooLarge { size },
        ImageError::Unsupported(_) => TexError::Broken("この画像の符号化方式は未対応です".into()),
        _ => TexError::Broken("画素のデータが破損しています".into()),
    })?;
    Ok(mip::chain(image, skip))
}

/// 貼り付け用の圧縮バイト列を返す。文書に加える前に返り値を `decode` で確かめる。
pub fn strip_metadata(bytes: &[u8], mime: Mime) -> Result<Vec<u8>, TexError> {
    if sniff(bytes) != Some(mime) {
        return Err(TexError::Unsupported);
    }
    let mut output = Vec::new();
    let keep = |part: &[u8]| output.extend_from_slice(part);
    match mime {
        Mime::Png => png::visit(bytes, keep)?,
        Mime::Jpeg => jpeg::visit(bytes, keep)?,
    };
    Ok(output)
}
