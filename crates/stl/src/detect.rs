//! STL の形式判定（ASCII とバイナリ）。
//!
//! 判定の順:
//! 1. 先頭の UTF-8 BOM と ASCII 空白を飛ばし、`solid` の次が空白か行末で、かつ先頭 4,096 バイトの
//!    2行目以降に `facet` か `endsolid` で始まる行があれば ASCII。
//! 2. そうでなく、84 バイト以上で、80 バイト目からの `u32`（リトルエンディアン）を `n` として
//!    `84 + 50 * n` がファイルの大きさと等しければバイナリ。小さくて `allow_trailing` が真なら
//!    バイナリ（余りは警告）。計算は `u64` で行う。
//! 3. どちらでもなければ `None`。

use crate::StlKind;

pub(crate) const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
pub(crate) const ASCII_SCAN_LIMIT: usize = 4096;
pub(crate) const BINARY_HEADER_BYTES: usize = 80;
pub(crate) const BINARY_MIN_BYTES: usize = 84;
pub(crate) const BINARY_TRIANGLE_BYTES: u64 = 50;

/// STL かどうかと、その種類。どちらでもなければ `None`。
pub fn detect(bytes: &[u8], allow_trailing: bool) -> Option<StlKind> {
    if is_ascii(bytes) {
        return Some(StlKind::Ascii);
    }
    if is_binary(bytes, allow_trailing) {
        return Some(StlKind::Binary);
    }
    None
}

/// 先頭 4,096 バイトを見て ASCII STL の印があるか。
fn is_ascii(bytes: &[u8]) -> bool {
    let window = &bytes[..bytes.len().min(ASCII_SCAN_LIMIT)];
    let Some(after_solid) = find_solid_end(window) else {
        return false;
    };
    let rest = &window[after_solid..];
    let Some(eol) = rest.iter().position(|&b| b == b'\n' || b == b'\r') else {
        return false;
    };
    // `solid` 行に NUL が含まれるものは、0 埋めされたバイナリのヘッダとみなす。
    if rest[..eol].contains(&0) {
        return false;
    }
    let mut pos = eol;
    while pos < rest.len() {
        pos = skip_line_break(rest, pos);
        let line_start = pos;
        while pos < rest.len() && rest[pos] != b'\n' && rest[pos] != b'\r' {
            pos += 1;
        }
        let line = &rest[line_start..pos];
        if let Some(word) = first_word(line)
            && (word.eq_ignore_ascii_case(b"facet") || word.eq_ignore_ascii_case(b"endsolid"))
        {
            return true;
        }
    }
    false
}

/// 84 バイト以上で、申告された三角形数がファイルサイズと合う（または `allow_trailing` で収まる）か。
fn is_binary(bytes: &[u8], allow_trailing: bool) -> bool {
    let Some(claimed) = binary_claimed_count(bytes) else {
        return false;
    };
    let expected = BINARY_MIN_BYTES as u64 + BINARY_TRIANGLE_BYTES * claimed;
    let actual = bytes.len() as u64;
    expected == actual || (allow_trailing && expected < actual)
}

/// 84 バイト以上なら 80..84 の三角形数（`u64`）を返す。
pub(crate) fn binary_claimed_count(bytes: &[u8]) -> Option<u64> {
    let count_bytes: [u8; 4] = bytes
        .get(BINARY_HEADER_BYTES..BINARY_MIN_BYTES)?
        .try_into()
        .ok()?;
    Some(u64::from(u32::from_le_bytes(count_bytes)))
}

/// `detect` が `None` を返したときに、壊れた ASCII として扱うべきか。
/// `solid` で始まり NUL を含まず、84 バイト未満かヘッダ範囲内に改行がある UTF-8 テキストなら真。
pub(crate) fn looks_like_broken_ascii(bytes: &[u8]) -> bool {
    let window = &bytes[..bytes.len().min(ASCII_SCAN_LIMIT)];
    let Some(after_solid) = find_solid_end(window) else {
        return false;
    };
    if window.contains(&0) {
        return false;
    }
    if bytes.len() < BINARY_MIN_BYTES {
        return std::str::from_utf8(window).is_ok();
    }
    let first_eol = window[after_solid..]
        .iter()
        .position(|&b| b == b'\n' || b == b'\r');
    first_eol.is_some_and(|offset| after_solid + offset < BINARY_HEADER_BYTES)
        && std::str::from_utf8(window).is_ok()
}

/// 先頭の UTF-8 BOM と ASCII 空白を飛ばし、`solid` +（空白か末尾）なら `solid` 直後の位置を返す。
fn find_solid_end(bytes: &[u8]) -> Option<usize> {
    let mut pos = if bytes.starts_with(UTF8_BOM) {
        UTF8_BOM.len()
    } else {
        0
    };
    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    let word = bytes.get(pos..pos + 5)?;
    if !word.eq_ignore_ascii_case(b"solid") {
        return None;
    }
    let end = pos + 5;
    match bytes.get(end) {
        None => Some(end),
        Some(next) if next.is_ascii_whitespace() => Some(end),
        Some(_) => None,
    }
}

fn skip_line_break(bytes: &[u8], pos: usize) -> usize {
    if bytes.get(pos) == Some(&b'\r') && bytes.get(pos + 1) == Some(&b'\n') {
        pos + 2
    } else if matches!(bytes.get(pos), Some(b'\n' | b'\r')) {
        pos + 1
    } else {
        pos
    }
}

fn first_word(line: &[u8]) -> Option<&[u8]> {
    let start = line.iter().position(|b| !b.is_ascii_whitespace())?;
    let rest = &line[start..];
    let len = rest
        .iter()
        .position(|b| b.is_ascii_whitespace())
        .unwrap_or(rest.len());
    Some(&rest[..len])
}
