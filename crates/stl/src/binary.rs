//! バイナリ STL の読み取り。
//!
//! 80 バイトのヘッダと 4 バイトの三角形数（`u32` リトルエンディアン）の後ろに、
//! 三角形 1 つ 50 バイト（法線 12 バイト + 頂点 3 個 36 バイト + 付属 2 バイト）が並ぶ。
//! 確保する量は「実際にファイルに収まる三角形数」で決め、申告数だけを根拠に確保しない。

use model_reviewer_core::scene::{LoadWarning, Primitive};

use crate::detect::{BINARY_MIN_BYTES, BINARY_TRIANGLE_BYTES, binary_claimed_count};
use crate::weld::Welder;
use crate::{COORD_LIMIT, StlError, StlOptions};

pub(crate) struct Parsed {
    pub(crate) primitive: Primitive,
    pub(crate) warnings: Vec<LoadWarning>,
}

pub(crate) fn parse(bytes: &[u8], options: &StlOptions<'_>) -> Result<Parsed, StlError> {
    if bytes.len() < BINARY_MIN_BYTES {
        return Err(StlError::TooShort);
    }
    let claimed = binary_claimed_count(bytes).ok_or(StlError::TooShort)?;
    let actual = bytes.len() as u64;
    let fits = (actual - BINARY_MIN_BYTES as u64) / BINARY_TRIANGLE_BYTES;
    if claimed > fits {
        return Err(StlError::CountMismatch { claimed, fits });
    }
    let expected = BINARY_MIN_BYTES as u64 + BINARY_TRIANGLE_BYTES * claimed;
    let trailing = actual - expected;
    if trailing > 0 && !options.allow_trailing {
        return Err(StlError::NotStl);
    }
    if claimed == 0 {
        return Err(StlError::NoTriangles);
    }
    // `claimed <= fits` を確認済みなので、実際のファイルサイズに比例する量しか確保しない。
    let tri_count = usize::try_from(claimed.min(fits)).map_err(|_| StlError::TooLarge)?;
    let mut welder = Welder::for_triangles(tri_count)?;
    for index in 0..tri_count {
        let base = BINARY_MIN_BYTES + index * (BINARY_TRIANGLE_BYTES as usize);
        let triangle_no = (index as u64) + 1;
        // 法線 12 バイト（base..base+12）と付属 2 バイト（base+48..base+50）は使わない。
        let v0 = read_vertex(bytes, base + 12, triangle_no)?;
        let v1 = read_vertex(bytes, base + 24, triangle_no)?;
        let v2 = read_vertex(bytes, base + 36, triangle_no)?;
        welder.push_triangle([v0, v1, v2])?;
    }
    let mut warnings = Vec::new();
    if trailing > 0 {
        warnings.push(LoadWarning::StlTrailingBytes { bytes: trailing });
    }
    Ok(Parsed {
        primitive: welder.finish(),
        warnings,
    })
}

#[inline]
fn read_vertex(bytes: &[u8], offset: usize, triangle: u64) -> Result<[f32; 3], StlError> {
    let x = read_coord(bytes, offset, triangle)?;
    let y = read_coord(bytes, offset + 4, triangle)?;
    let z = read_coord(bytes, offset + 8, triangle)?;
    Ok([x, y, z])
}

#[inline]
fn read_coord(bytes: &[u8], offset: usize, triangle: u64) -> Result<f32, StlError> {
    let chunk: [u8; 4] = bytes[offset..offset + 4]
        .try_into()
        .map_err(|_| StlError::Truncated)?;
    let value = f32::from_le_bytes(chunk);
    if !value.is_finite() || value.abs() > COORD_LIMIT {
        return Err(StlError::BadNumber { triangle });
    }
    Ok(value)
}
