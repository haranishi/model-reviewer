//! 頂点を位置でまとめる。`-0.0` は `+0.0` にそろえてから 3 成分のビットを鍵にし、
//! 最初に出た順で番号を振る。表には標準の `HashMap` の `RandomState` を使う。

use std::collections::HashMap;

use glam::Vec3;
use model_reviewer_core::math::Aabb;
use model_reviewer_core::scene::{Primitive, Vertex};

use crate::StlError;

/// 同じ位置の頂点を 1 つにまとめながら `Primitive` を組み立てる。
pub(crate) struct Welder {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    map: HashMap<[u32; 3], u32>,
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
            map: HashMap::with_capacity(vertex_cap),
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
        let key = position.map(f32::to_bits);
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
    fn weld_table_uses_random_state() {
        let welder = Welder::new();
        let hasher: &dyn std::any::Any = welder.map.hasher();
        assert!(hasher.is::<std::hash::RandomState>());
    }

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
