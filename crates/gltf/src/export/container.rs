//! GLBの容器（12バイトのヘッダと、JSON・BINの2つのチャンク）を読む・長さを数える。
//! 読み込み側（`gltf::binary::Glb`）と別に持つのは、BINの範囲を元のバイト列の中の
//! スライスとして扱い、写しを作らずに書き出すため。

use super::ExportError;

const MAGIC: &[u8; 4] = b"glTF";
const JSON_KIND: &[u8; 4] = b"JSON";
const BIN_KIND: &[u8; 4] = b"BIN\0";
const HEADER: u64 = 12;
const CHUNK_HEADER: u64 = 8;

/// 元のファイルの中のJSONとBINの範囲。
#[derive(Clone, Copy, Debug)]
pub(crate) struct Container<'a> {
    pub json: &'a [u8],
    pub bin: Option<&'a [u8]>,
}

/// 4の倍数へ切り上げる。上限を超えるときは `u64::MAX`（長さの検査で必ず断られる値）。
pub(crate) fn align4(value: u64) -> u64 {
    value.checked_next_multiple_of(4).unwrap_or(u64::MAX)
}

/// 容器を読む。宣言された長さとファイルの長さが違う、2つ目のチャンクがBINでない、
/// BINの後ろにまだデータがある、のどれも断る（書き出すと、その部分を落としてしまうため）。
pub(crate) fn parse(bytes: &[u8]) -> Result<Container<'_>, String> {
    if bytes.len() < HEADER as usize || &bytes[..4] != MAGIC {
        return Err("GLBではありません".to_owned());
    }
    if read_u32(bytes, 4)? != 2 {
        return Err("GLBの版が2ではありません".to_owned());
    }
    let declared = u64::from(read_u32(bytes, 8)?);
    if declared != bytes.len() as u64 {
        return Err(format!(
            "ヘッダの長さ {declared} とファイルの長さ {} が違います",
            bytes.len()
        ));
    }
    let (kind, json, next) = chunk(bytes, HEADER)?;
    if kind != JSON_KIND {
        return Err("最初のチャンクがJSONではありません".to_owned());
    }
    if next == declared {
        return Ok(Container { json, bin: None });
    }
    let (kind, bin, end) = chunk(bytes, next)?;
    if kind != BIN_KIND {
        return Err("2つ目のチャンクがBINではありません".to_owned());
    }
    if end != declared {
        return Err("BINチャンクの後ろに知らないデータがあります".to_owned());
    }
    Ok(Container {
        json,
        bin: Some(bin),
    })
}

fn chunk(bytes: &[u8], at: u64) -> Result<(&[u8], &[u8], u64), String> {
    let at_us = usize::try_from(at).map_err(|_| "チャンクの位置".to_owned())?;
    let len = u64::from(read_u32(bytes, at_us)?);
    let kind = bytes
        .get(at_us + 4..at_us + 8)
        .ok_or_else(|| "チャンクの種類がありません".to_owned())?;
    let start = at + CHUNK_HEADER;
    let end = start + len;
    let data = usize::try_from(start)
        .ok()
        .zip(usize::try_from(end).ok())
        .and_then(|(start, end)| bytes.get(start..end))
        .ok_or_else(|| "チャンクの長さがファイルを超えています".to_owned())?;
    Ok((kind, data, end))
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32, String> {
    let chunk = bytes
        .get(at..at + 4)
        .ok_or_else(|| "容器が途中で切れています".to_owned())?;
    let mut buf = [0u8; 4];
    buf.copy_from_slice(chunk);
    Ok(u32::from_le_bytes(buf))
}

/// 書き出すBINチャンクの長さ（4バイト境界にそろえた後）。足す物が無ければ元のBINの長さ
/// （無ければ `None`）。足す物があれば、元のBINを4バイト境界まで0で詰めた後ろに足す。
/// `appended` の長さは4の倍数（各ビューの末尾を0で詰める）。
pub(crate) fn bin_chunk_len(base_bin: Option<&[u8]>, appended: &[u8]) -> Option<u64> {
    let base = base_bin.map(|bin| align4(bin.len() as u64));
    if appended.is_empty() {
        return base;
    }
    Some(base.unwrap_or(0) + align4(appended.len() as u64))
}

/// GLB全体の長さを数える。ヘッダの長さの欄は32ビットなので、超えるなら `TooLarge`。
/// `json_len` は詰め物の前、`bin_len` はBINチャンクの中身の長さ（詰め物を含む）。
pub fn glb_length(json_len: u64, bin_len: Option<u64>) -> Result<u64, ExportError> {
    let json = align4(json_len);
    let bin = bin_len.map_or(0, |len| CHUNK_HEADER.saturating_add(align4(len)));
    let total = HEADER
        .saturating_add(CHUNK_HEADER)
        .saturating_add(json)
        .saturating_add(bin);
    if total > u64::from(u32::MAX) {
        return Err(ExportError::TooLarge { bytes: total });
    }
    Ok(total)
}
