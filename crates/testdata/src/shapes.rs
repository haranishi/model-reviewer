//! 自作の形状。外部のモデルは使わない。円柱の頂点はリテラルで、`sin` / `cos` を呼ばない。

#[derive(Clone, Debug)]
pub struct Geometry {
    pub positions: Vec<[f32; 3]>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub indices: Vec<u32>,
}

impl Geometry {
    /// テストが公開する三角形数。本体の生成はインデックス長を直接使う。
    #[allow(dead_code)]
    pub fn triangles(&self) -> u32 {
        (self.indices.len() / 3) as u32
    }
}

/// 1 辺が 1 の立方体。面ごとの法線を持つ。
pub fn box_mesh() -> Geometry {
    box_extents([0.5, 0.5, 0.5])
}

/// 中心が原点の直方体。`half` は各軸の半分の長さ。面ごとの法線を持つ。
pub fn box_extents(half: [f32; 3]) -> Geometry {
    let [x, y, z] = half;
    let mut positions = Vec::with_capacity(24);
    let mut normals = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [1.0, 0.0, 0.0],
            [[x, -y, z], [x, -y, -z], [x, y, -z], [x, y, z]],
        ),
        (
            [-1.0, 0.0, 0.0],
            [[-x, -y, -z], [-x, -y, z], [-x, y, z], [-x, y, -z]],
        ),
        (
            [0.0, 1.0, 0.0],
            [[-x, y, z], [x, y, z], [x, y, -z], [-x, y, -z]],
        ),
        (
            [0.0, -1.0, 0.0],
            [[-x, -y, -z], [x, -y, -z], [x, -y, z], [-x, -y, z]],
        ),
        (
            [0.0, 0.0, 1.0],
            [[-x, -y, z], [x, -y, z], [x, y, z], [-x, y, z]],
        ),
        (
            [0.0, 0.0, -1.0],
            [[x, -y, -z], [-x, -y, -z], [-x, y, -z], [x, y, -z]],
        ),
    ];
    for (normal, corners) in faces {
        let base = positions.len() as u32;
        for corner in corners {
            positions.push(corner);
            normals.push(normal);
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    Geometry {
        positions,
        normals: Some(normals),
        indices,
    }
}

/// 24 角の円柱。半径 0.32、厚さ 0.12、軸は Y。三角形は 96。頂点はリテラル。
pub fn cylinder_mesh() -> Geometry {
    #[allow(clippy::approx_constant, clippy::excessive_precision)]
    const C15: f32 = 0.9659258262890683;
    #[allow(clippy::approx_constant, clippy::excessive_precision)]
    const S15: f32 = 0.25881904510252074;
    #[allow(clippy::approx_constant, clippy::excessive_precision)]
    const C30: f32 = 0.8660254037844386;
    const S30: f32 = 0.5;
    #[allow(clippy::approx_constant, clippy::excessive_precision)]
    const C45: f32 = 0.7071067811865476;
    const R: f32 = 0.32;
    const H: f32 = 0.06;
    const RING: [[f32; 2]; 24] = [
        [1.0, 0.0],
        [C15, S15],
        [C30, S30],
        [C45, C45],
        [S30, C30],
        [S15, C15],
        [0.0, 1.0],
        [-S15, C15],
        [-S30, C30],
        [-C45, C45],
        [-C30, S30],
        [-C15, S15],
        [-1.0, 0.0],
        [-C15, -S15],
        [-C30, -S30],
        [-C45, -C45],
        [-S30, -C30],
        [-S15, -C15],
        [0.0, -1.0],
        [S15, -C15],
        [S30, -C30],
        [C45, -C45],
        [C30, -S30],
        [C15, -S15],
    ];
    let n = RING.len();
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    for i in 0..n {
        let j = (i + 1) % n;
        let base = positions.len() as u32;
        for k in [i, j] {
            let x = RING[k][0];
            let z = RING[k][1];
            let normal = [x, 0.0, z];
            positions.push([R * x, -H, R * z]);
            normals.push(normal);
            positions.push([R * x, H, R * z]);
            normals.push(normal);
        }
        // 外から見て反時計回り。旧順は側面が内向きだった。
        indices.extend([base, base + 3, base + 2, base, base + 1, base + 3]);
    }
    let top = positions.len() as u32;
    positions.push([0.0, H, 0.0]);
    normals.push([0.0, 1.0, 0.0]);
    let top_rim = positions.len() as u32;
    for point in RING {
        positions.push([R * point[0], H, R * point[1]]);
        normals.push([0.0, 1.0, 0.0]);
    }
    for k in 0..n {
        let a = top_rim + k as u32;
        let b = top_rim + ((k + 1) % n) as u32;
        indices.extend([top, b, a]);
    }
    let bottom = positions.len() as u32;
    positions.push([0.0, -H, 0.0]);
    normals.push([0.0, -1.0, 0.0]);
    let bottom_rim = positions.len() as u32;
    for point in RING {
        positions.push([R * point[0], -H, R * point[1]]);
        normals.push([0.0, -1.0, 0.0]);
    }
    for k in 0..n {
        let a = bottom_rim + k as u32;
        let b = bottom_rim + ((k + 1) % n) as u32;
        indices.extend([bottom, a, b]);
    }
    Geometry {
        positions,
        normals: Some(normals),
        indices,
    }
}

/// 法線の無い四面体。三角形は 4。
pub fn tetrahedron() -> Geometry {
    Geometry {
        positions: vec![
            [0.0, 0.5, 0.0],
            [0.5, -0.25, 0.4330127],
            [-0.5, -0.25, 0.4330127],
            [0.0, -0.25, -0.5],
        ],
        normals: None,
        indices: vec![0, 2, 1, 0, 3, 2, 0, 1, 3, 1, 2, 3],
    }
}

/// 同じ四面体に、面ごとの法線を付けたもの。ランプの四面体とはアクセサが別になる。
pub fn tetrahedron_with_normals() -> Geometry {
    let bare = tetrahedron();
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    let face_normals = [
        [0.0, 0.5, 0.8660254],
        [-0.84071, 0.30036, -0.45054],
        [0.84071, 0.30036, -0.45054],
        [0.0, -1.0, 0.0],
    ];
    for (face, normal) in bare.indices.chunks(3).zip(face_normals) {
        let base = positions.len() as u32;
        for index in face {
            positions.push(bare.positions[*index as usize]);
            normals.push(normal);
        }
        indices.extend([base, base + 1, base + 2]);
    }
    Geometry {
        positions,
        normals: Some(normals),
        indices,
    }
}

/// 頂点を一様に拡大する。法線の向きは変えない。
pub fn scale_uniform(geometry: &Geometry, factor: f32) -> Geometry {
    let mut scaled = geometry.clone();
    for position in &mut scaled.positions {
        position[0] *= factor;
        position[1] *= factor;
        position[2] *= factor;
    }
    scaled
}

/// 頂点を平行移動する。法線とインデックスはそのまま。
pub fn translate(geometry: &Geometry, offset: [f32; 3]) -> Geometry {
    let mut moved = geometry.clone();
    for position in &mut moved.positions {
        position[0] += offset[0];
        position[1] += offset[1];
        position[2] += offset[2];
    }
    moved
}

/// `triangles` 個の三角形。各三角形は独立した 3 頂点を持つ。
pub fn triangle_grid(triangles: u32) -> Geometry {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    for i in 0..triangles {
        let z = i as f32;
        let base = positions.len() as u32;
        positions.push([0.0, 0.0, z]);
        positions.push([1.0, 0.0, z]);
        positions.push([0.0, 1.0, z]);
        normals.extend([[0.0, 0.0, 1.0]; 3]);
        indices.extend([base, base + 1, base + 2]);
    }
    Geometry {
        positions,
        normals: Some(normals),
        indices,
    }
}

#[cfg(test)]
mod tests {
    use super::{box_mesh, cylinder_mesh, tetrahedron, tetrahedron_with_normals};

    #[test]
    fn published_triangle_counts() {
        assert_eq!(box_mesh().triangles(), 12);
        assert_eq!(cylinder_mesh().triangles(), 96);
        let cyl = cylinder_mesh();
        let max_r = cyl
            .positions
            .iter()
            .map(|p| (p[0] * p[0] + p[2] * p[2]).sqrt())
            .fold(0.0f32, f32::max);
        let max_y = cyl
            .positions
            .iter()
            .map(|p| p[1].abs())
            .fold(0.0f32, f32::max);
        assert!((max_r - 0.32).abs() < 1e-5, "{max_r}");
        assert!((max_y - 0.06).abs() < 1e-6, "{max_y}");
        assert_eq!(tetrahedron().triangles(), 4);
        assert_eq!(tetrahedron_with_normals().triangles(), 4);
        assert!(tetrahedron().normals.is_none());
        assert!(box_mesh().normals.is_some());
        assert_eq!(
            tetrahedron().indices,
            vec![0, 2, 1, 0, 3, 2, 0, 1, 3, 1, 2, 3]
        );
    }

    #[test]
    fn triangles_face_outward() {
        assert_outward(&cylinder_mesh());
        assert_outward(&tetrahedron_with_normals());
        assert_outward(&box_mesh());
    }

    fn assert_outward(geometry: &super::Geometry) {
        let normals = geometry.normals.as_ref().expect("normals");
        for face in geometry.indices.chunks(3) {
            let tri = [
                geometry.positions[face[0] as usize],
                geometry.positions[face[1] as usize],
                geometry.positions[face[2] as usize],
            ];
            let geometric = face_normal(tri);
            for index in face {
                let vertex = normals[*index as usize];
                let dot =
                    geometric[0] * vertex[0] + geometric[1] * vertex[1] + geometric[2] * vertex[2];
                assert!(dot > 0.0, "inward face {tri:?} normal {vertex:?} dot {dot}");
            }
        }
    }

    fn face_normal(tri: [[f32; 3]; 3]) -> [f32; 3] {
        let e1 = [
            tri[1][0] - tri[0][0],
            tri[1][1] - tri[0][1],
            tri[1][2] - tri[0][2],
        ];
        let e2 = [
            tri[2][0] - tri[0][0],
            tri[2][1] - tri[0][1],
            tri[2][2] - tri[0][2],
        ];
        [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ]
    }
}
