//! ASCII STL の読み取り。
//!
//! 行を `\r\n`・`\n`・`\r` で分け、キーワードは大文字小文字を区別しない。
//! `solid` / `endsolid` 行の名前は読み飛ばし、`facet normal` の 3 数は有限な数として読めることだけ確かめる。
//! 座標が有限でないか絶対値 `COORD_LIMIT` を超えたら `BadNumber`、構造の誤りは `Syntax`、
//! 面の途中や `endsolid` の前で終われば `Truncated`。複数の `solid` は 1 つの部品にまとめて警告にする。

use model_reviewer_core::scene::LoadWarning;

use crate::binary::Parsed;
use crate::detect::UTF8_BOM;
use crate::weld::Welder;
use crate::{COORD_LIMIT, StlError};

enum State {
    Outside,
    InSolid,
    InFacet,
    InLoop { verts: [[f32; 3]; 3], count: u8 },
    AfterLoop,
}

pub(crate) fn parse(bytes: &[u8]) -> Result<Parsed, StlError> {
    let bytes = bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes);
    let mut state = State::Outside;
    let mut welder = Welder::new();
    let mut solid_count = 0u32;

    for (line_no, raw_line, terminated) in Lines::new(bytes) {
        let trimmed = trim_ascii_whitespace(raw_line);
        if trimmed.is_empty() {
            continue;
        }
        match step(&mut state, &mut welder, &mut solid_count, line_no, trimmed) {
            Ok(()) => {}
            // 改行で終わらない最後の行の誤りは、途中で切れたファイルとみなす。
            Err(StlError::Syntax { .. }) if !terminated => return Err(StlError::Truncated),
            Err(error) => return Err(error),
        }
    }

    if !matches!(state, State::Outside) {
        return Err(StlError::Truncated);
    }
    if welder.triangle_count() == 0 {
        return Err(StlError::NoTriangles);
    }
    let mut warnings = Vec::new();
    if solid_count > 1 {
        warnings.push(LoadWarning::StlSolidsMerged { count: solid_count });
    }
    Ok(Parsed {
        primitive: welder.finish(),
        warnings,
    })
}

/// 1行を読んで状態を進める。
fn step(
    state: &mut State,
    welder: &mut Welder,
    solid_count: &mut u32,
    line_no: u64,
    trimmed: &[u8],
) -> Result<(), StlError> {
    match state {
        State::Outside => {
            if starts_with_keyword(trimmed, b"solid") {
                *solid_count = solid_count.checked_add(1).ok_or(StlError::TooLarge)?;
                *state = State::InSolid;
            } else {
                return Err(StlError::Syntax { line: line_no });
            }
        }
        State::InSolid => {
            if starts_with_keyword(trimmed, b"endsolid") {
                *state = State::Outside;
                return Ok(());
            }
            let text =
                std::str::from_utf8(trimmed).map_err(|_| StlError::Syntax { line: line_no })?;
            let mut words = text.split_ascii_whitespace();
            let (Some(k0), Some(k1), Some(nx), Some(ny), Some(nz), None) = (
                words.next(),
                words.next(),
                words.next(),
                words.next(),
                words.next(),
                words.next(),
            ) else {
                return Err(StlError::Syntax { line: line_no });
            };
            if !k0.eq_ignore_ascii_case("facet") || !k1.eq_ignore_ascii_case("normal") {
                return Err(StlError::Syntax { line: line_no });
            }
            // 法線は使わない。数として読めることだけ確かめる（`nan` を書くソフトもある）。
            for token in [nx, ny, nz] {
                if token.parse::<f32>().is_err() {
                    return Err(StlError::Syntax { line: line_no });
                }
            }
            *state = State::InFacet;
        }
        State::InFacet => {
            let text =
                std::str::from_utf8(trimmed).map_err(|_| StlError::Syntax { line: line_no })?;
            let mut words = text.split_ascii_whitespace();
            let (Some(k0), Some(k1), None) = (words.next(), words.next(), words.next()) else {
                return Err(StlError::Syntax { line: line_no });
            };
            if !k0.eq_ignore_ascii_case("outer") || !k1.eq_ignore_ascii_case("loop") {
                return Err(StlError::Syntax { line: line_no });
            }
            *state = State::InLoop {
                verts: [[0.0; 3]; 3],
                count: 0,
            };
        }
        State::InLoop { verts, count } => {
            let text =
                std::str::from_utf8(trimmed).map_err(|_| StlError::Syntax { line: line_no })?;
            let mut words = text.split_ascii_whitespace();
            let Some(head) = words.next() else {
                return Err(StlError::Syntax { line: line_no });
            };
            if head.eq_ignore_ascii_case("vertex") {
                let (Some(sx), Some(sy), Some(sz), None) =
                    (words.next(), words.next(), words.next(), words.next())
                else {
                    return Err(StlError::Syntax { line: line_no });
                };
                if *count >= 3 {
                    return Err(StlError::Syntax { line: line_no });
                }
                let triangle_no = welder.triangle_count() + 1;
                let x = parse_coord(sx, line_no, triangle_no)?;
                let y = parse_coord(sy, line_no, triangle_no)?;
                let z = parse_coord(sz, line_no, triangle_no)?;
                verts[*count as usize] = [x, y, z];
                *count += 1;
            } else if head.eq_ignore_ascii_case("endloop") {
                if words.next().is_some() || *count != 3 {
                    return Err(StlError::Syntax { line: line_no });
                }
                let corners = *verts;
                welder.push_triangle(corners)?;
                *state = State::AfterLoop;
            } else {
                return Err(StlError::Syntax { line: line_no });
            }
        }
        State::AfterLoop => {
            let text =
                std::str::from_utf8(trimmed).map_err(|_| StlError::Syntax { line: line_no })?;
            let mut words = text.split_ascii_whitespace();
            let (Some(head), None) = (words.next(), words.next()) else {
                return Err(StlError::Syntax { line: line_no });
            };
            if !head.eq_ignore_ascii_case("endfacet") {
                return Err(StlError::Syntax { line: line_no });
            }
            *state = State::InSolid;
        }
    }
    Ok(())
}

#[inline]
fn parse_coord(token: &str, line: u64, triangle: u64) -> Result<f32, StlError> {
    let value = token
        .parse::<f32>()
        .map_err(|_| StlError::Syntax { line })?;
    // `f32::from_str` は "nan"・"inf"・"infinity"・"1e999" などを成功として返すため、必ず有限性を確かめる。
    if !value.is_finite() || value.abs() > COORD_LIMIT {
        return Err(StlError::BadNumber { triangle });
    }
    Ok(value)
}

fn trim_ascii_whitespace(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let rest = &bytes[start..];
    let end = rest
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(0, |index| index + 1);
    &rest[..end]
}

fn starts_with_keyword(trimmed: &[u8], keyword: &[u8]) -> bool {
    let Some(head) = trimmed.get(..keyword.len()) else {
        return false;
    };
    if !head.eq_ignore_ascii_case(keyword) {
        return false;
    }
    match trimmed.get(keyword.len()) {
        None => true,
        Some(next) => next.is_ascii_whitespace(),
    }
}

/// `\r\n`・`\n`・`\r` のすべてを行の区切りとして、1 始まりの行番号と行のバイト列を返す。
struct Lines<'a> {
    bytes: &'a [u8],
    pos: usize,
    line_no: u64,
}

impl<'a> Lines<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            pos: 0,
            line_no: 0,
        }
    }
}

impl<'a> Iterator for Lines<'a> {
    /// 行番号、行のバイト列、改行で終わったか。
    type Item = (u64, &'a [u8], bool);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.bytes.len() {
            return None;
        }
        self.line_no += 1;
        let start = self.pos;
        while self.pos < self.bytes.len()
            && self.bytes[self.pos] != b'\n'
            && self.bytes[self.pos] != b'\r'
        {
            self.pos += 1;
        }
        let line = &self.bytes[start..self.pos];
        let terminated = self.pos < self.bytes.len();
        if terminated {
            if self.bytes[self.pos] == b'\r' && self.bytes.get(self.pos + 1) == Some(&b'\n') {
                self.pos += 2;
            } else {
                self.pos += 1;
            }
        }
        Some((self.line_no, line, terminated))
    }
}
