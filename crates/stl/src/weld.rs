//! 頂点を位置でまとめる。`-0.0` は `+0.0` にそろえてから 3 成分のビットを鍵にし、
//! 最初に出た順で番号を振る（結果が決まる）。外部の依存は使わず、標準の `HashMap` に
//! 自前の軽いハッシュ関数を付ける。

use std::collections::HashMap;
use std::hash::{BuildHasher, Hash, Hasher};

use glam::Vec3;
use model_reviewer_core::math::Aabb;
use model_reviewer_core::scene::{Primitive, Vertex};

use crate::StlError;

/// `-0.0` を `+0.0` にそろえた 3 成分のビット。
#[derive(Clone, Copy, PartialEq, Eq)]
struct PosKey([u32; 3]);

impl Hash for PosKey {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        let lo = u64::from(self.0[0]) | (u64::from(self.0[1]) << 32);
        state.write_u64(lo);
        state.write_u32(self.0[2]);
    }
}

#[derive(Clone, Copy, Default)]
struct FastBuildHasher;

impl BuildHasher for FastBuildHasher {
    type Hasher = FastHasher;

    #[inline]
    fn build_hasher(&self) -> Self::Hasher {
        FastHasher(0)
    }
}

struct FastHasher(u64);

impl FastHasher {
    #[inline]
    fn mix(&mut self, value: u64) {
        const K: u64 = 0x517c_c1b7_2722_0a95;
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(K);
    }
}

impl Hasher for FastHasher {
    #[inline]
    fn finish(&self) -> u64 {
        let mut h = self.0;
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
        h ^ (h >> 33)
    }

    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.mix(u64::from(byte));
        }
    }

    #[inline]
    fn write_u32(&mut self, value: u32) {
        self.mix(u64::from(value));
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.mix(value);
    }
}

/// 同じ位置の頂点を 1 つにまとめながら `Primitive` を組み立てる。
pub(crate) struct Welder {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    map: HashMap<PosKey, u32, FastBuildHasher>,
    local_aabb: Aabb,
    max_vertices: usize,
    max_indices: usize,
}

impl Welder {
    /// 初期容量を指定せずに作る（ASCII 用）。
    pub(crate) fn new() -> Self {
        Self::with_limits(0, 0, u32::MAX as usize, u32::MAX as usize)
    }

    /// 実際のファイル長から確認済みの三角形数に合わせた容量で作る（バイナリ用）。
    pub(crate) fn for_triangles(triangles: usize) -> Result<Self, StlError> {
        let index_cap = triangles.checked_mul(3).ok_or(StlError::TooLarge)?;
        if index_cap > u32::MAX as usize {
            return Err(StlError::TooLarge);
        }
        // 閉じた形では頂点数が三角形数の約半分になる見込み。
        let vertex_cap = (triangles / 2).saturating_add(2);
        Ok(Self::with_limits(
            vertex_cap,
            index_cap,
            u32::MAX as usize,
            u32::MAX as usize,
        ))
    }

    fn with_limits(
        vertex_cap: usize,
        index_cap: usize,
        max_vertices: usize,
        max_indices: usize,
    ) -> Self {
        Self {
            vertices: Vec::with_capacity(vertex_cap),
            indices: Vec::with_capacity(index_cap),
            map: HashMap::with_capacity_and_hasher(vertex_cap, FastBuildHasher),
            local_aabb: Aabb::empty(),
            max_vertices,
            max_indices,
        }
    }

    /// 三角形 1 つを足す。面積が 0 でも捨てない。
    #[inline]
    pub(crate) fn push_triangle(&mut self, corners: [[f32; 3]; 3]) -> Result<(), StlError> {
        if self.indices.len() > self.max_indices.saturating_sub(3) {
            return Err(StlError::TooLarge);
        }
        let a = self.intern(corners[0])?;
        let b = self.intern(corners[1])?;
        let c = self.intern(corners[2])?;
        self.indices.push(a);
        self.indices.push(b);
        self.indices.push(c);
        Ok(())
    }

    #[inline]
    fn intern(&mut self, raw: [f32; 3]) -> Result<u32, StlError> {
        let position = [
            canonical_zero(raw[0]),
            canonical_zero(raw[1]),
            canonical_zero(raw[2]),
        ];
        let key = PosKey([
            position[0].to_bits(),
            position[1].to_bits(),
            position[2].to_bits(),
        ]);
        if let Some(&index) = self.map.get(&key) {
            return Ok(index);
        }
        if self.vertices.len() >= self.max_vertices {
            return Err(StlError::TooLarge);
        }
        let index = u32::try_from(self.vertices.len()).map_err(|_| StlError::TooLarge)?;
        self.vertices.push(Vertex {
            position,
            normal: [0.0, 0.0, 0.0],
        });
        self.map.insert(key, index);
        let point = Vec3::from(position);
        self.local_aabb.min = self.local_aabb.min.min(point);
        self.local_aabb.max = self.local_aabb.max.max(point);
        Ok(index)
    }

    pub(crate) fn triangle_count(&self) -> u64 {
        (self.indices.len() / 3) as u64
    }

    /// まとめる表を解放し、`Primitive` を返す。
    pub(crate) fn finish(self) -> Primitive {
        Primitive {
            vertices: self.vertices,
            indices: self.indices,
            local_aabb: self.local_aabb,
            has_normals: false,
        }
    }
}

#[inline]
fn canonical_zero(value: f32) -> f32 {
    if value == 0.0 { 0.0 } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_negative_zero_with_positive_zero_in_first_seen_order() {
        let mut welder = Welder::new();
        welder
            .push_triangle([[-0.0, 0.0, -0.0], [1.0, -0.0, 0.0], [0.0, 1.0, 0.0]])
            .unwrap();
        welder
            .push_triangle([[0.0, -0.0, 0.0], [0.0, 1.0, -0.0], [1.0, 0.0, 0.0]])
            .unwrap();
        let prim = welder.finish();
        assert_eq!(prim.vertices.len(), 3);
        assert_eq!(prim.indices, vec![0, 1, 2, 0, 2, 1]);
        assert!(!prim.has_normals);
        for vertex in &prim.vertices {
            assert_eq!(vertex.normal, [0.0, 0.0, 0.0]);
            for component in vertex.position {
                if component == 0.0 {
                    assert_eq!(component.to_bits(), 0.0_f32.to_bits());
                }
            }
        }
    }

    #[test]
    fn vertex_limit_overflow_returns_too_large() {
        let mut welder = Welder::with_limits(0, 0, 3, 12);
        welder
            .push_triangle([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])
            .unwrap();
        let err = welder
            .push_triangle([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]])
            .unwrap_err();
        assert_eq!(err, StlError::TooLarge);
    }

    #[test]
    fn index_limit_overflow_returns_too_large() {
        let mut welder = Welder::with_limits(0, 0, 10, 3);
        welder
            .push_triangle([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])
            .unwrap();
        let err = welder
            .push_triangle([[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]])
            .unwrap_err();
        assert_eq!(err, StlError::TooLarge);
    }
}
