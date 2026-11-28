//! 基本色の画像、貼り方、縮小版の容量と箱形の投影。

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use glam::Vec3;

use crate::scene::{Primitive, Vertex};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImageId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TexId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mime {
    Png,
    Jpeg,
}

pub fn sniff(bytes: &[u8]) -> Option<Mime> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Mime::Png)
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(Mime::Jpeg)
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ImageHash(pub [u8; 32]);

impl ImageHash {
    pub fn of(bytes: &[u8]) -> Self {
        Self(*blake3::hash(bytes).as_bytes())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageSource {
    /// 開いたファイルの BIN 内の範囲。長さに詰め物は含まない。
    File {
        offset: u64,
        len: u64,
    },
    Held(Arc<[u8]>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageInfo {
    pub hash: ImageHash,
    pub mime: Mime,
    pub source: ImageSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Wrap {
    Repeat,
    Mirror,
    Clamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mip {
    None,
    Nearest,
    Linear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Sampling {
    pub wrap: [Wrap; 2],
    pub mag_linear: bool,
    pub min_linear: bool,
    pub mip: Mip,
}

impl Default for Sampling {
    fn default() -> Self {
        Self {
            wrap: [Wrap::Repeat; 2],
            mag_linear: true,
            min_linear: true,
            mip: Mip::Linear,
        }
    }
}

pub const IDENTITY_TRANSFORM: [f32; 6] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UvMap {
    Vertex {
        /// 2×3 行列の上段、下段の順。
        transform: [f32; 6],
        tex_coord: u32,
        /// 書き出しで textureInfo を写す元の材質番号。
        info_from: Option<u32>,
    },
    Box {
        /// ローカル座標で測った画像1枚の辺の長さ。
        size: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TexUse {
    /// MASK の閾値だけを保つ材質では `None`。
    pub image: Option<ImageId>,
    pub sampling: Sampling,
    pub map: UvMap,
    pub cutoff: Option<f32>,
    /// 箱形の投影を頂点の UV に焼いたときの辺の長さ。
    pub baked_box: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Textures {
    pub images: Vec<ImageInfo>,
    pub uses: Vec<TexUse>,
    /// 形状番号順。UV が無い形状は空で、全形状に無ければ表自体が空。
    pub prim_uvs: Vec<Vec<[f32; 2]>>,
    /// スキンまたはモーフのあるノードのファイル内番号。昇順。
    pub no_box_nodes: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MipChain {
    pub size: [u32; 2],
    /// RGBA8（sRGB）。先頭が `size`、末尾が 1×1。
    pub levels: Vec<Vec<u8>>,
}

fn half_size(size: [u32; 2]) -> [u32; 2] {
    size.map(|side| (side / 2).max(1))
}

fn area([w, h]: [u32; 2]) -> u64 {
    u64::from(w) * u64::from(h)
}

fn chain_bytes_exact(mut size: [u32; 2]) -> u128 {
    if size.contains(&0) {
        return 0;
    }
    let mut bytes = 0;
    loop {
        bytes += u128::from(area(size)) * 4;
        if size == [1, 1] {
            return bytes;
        }
        size = half_size(size);
    }
}

/// RGBA8 の全段のバイト数。空なら 0、表せる範囲を超えれば `u64::MAX`。
pub fn chain_bytes(size: [u32; 2]) -> u64 {
    u64::try_from(chain_bytes_exact(size)).unwrap_or(u64::MAX)
}

fn smaller_for_display(size: [u32; 2]) -> Option<[u32; 2]> {
    let [w, h] = half_size(size);
    (w.max(h) >= 64).then_some([w, h])
}

fn fit_long_side(mut size: [u32; 2], max_side: u32) -> Option<([u32; 2], u8)> {
    if size.contains(&0) {
        return None;
    }
    let mut skip = 0;
    while size[0].max(size[1]) > max_side {
        size = smaller_for_display(size)?;
        skip += 1;
    }
    Some((size, skip))
}

/// 画像ごとに省く先頭の段数。縮小が不要なら元が 64 画素未満でもそのまま使う。
pub fn plan_levels(sizes: &[[u32; 2]], max_side: u32, budget: u64) -> Vec<Option<u8>> {
    let mut plan = vec![None; sizes.len()];
    let mut groups = BTreeMap::<u64, Vec<_>>::new();
    let mut total = 0;
    for (index, &size) in sizes.iter().enumerate() {
        if let Some((size, skip)) = fit_long_side(size, max_side) {
            total += chain_bytes_exact(size);
            plan[index] = Some(skip);
            groups
                .entry(area(size))
                .or_default()
                .push((index, size, skip));
        }
    }
    while total > u128::from(budget) {
        // 同面積の組は、途中で予算に収まっても全画像を同じ回に処理する。
        let Some((pixels, largest)) = groups.pop_last() else {
            break;
        };
        for (index, size, skip) in largest {
            if let Some(smaller) = smaller_for_display(size) {
                total -= u128::from(pixels) * 4;
                plan[index] = Some(skip + 1);
                groups
                    .entry(area(smaller))
                    .or_default()
                    .push((index, smaller, skip + 1));
            } else {
                total -= chain_bytes_exact(size);
                plan[index] = None;
            }
        }
    }
    plan
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Axis {
    X,
    Y,
    Z,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct BoxFace {
    axis: Axis,
    negative: bool,
}

impl BoxFace {
    fn from_normal(normal: Vec3) -> Self {
        let n = normal.abs();
        let (axis, component) = if n.x >= n.y && n.x >= n.z {
            (Axis::X, normal.x)
        } else if n.y >= n.z {
            (Axis::Y, normal.y)
        } else {
            (Axis::Z, normal.z)
        };
        Self {
            axis,
            negative: component < 0.0,
        }
    }

    fn uv(self, p: Vec3, size: f32) -> [f32; 2] {
        let sign = if self.negative { -1.0 } else { 1.0 };
        let (u, v) = match self.axis {
            Axis::X => (-sign * p.z, p.y),
            Axis::Y => (p.x, -sign * p.z),
            Axis::Z => (sign * p.x, p.y),
        };
        [u / size, -v / size]
    }
}

/// ローカル座標を、面の外から見た UV に写す。`size` は有限の正の長さ。
pub fn box_uv(p: Vec3, face_normal: Vec3, size: f32) -> [f32; 2] {
    BoxFace::from_normal(face_normal).uv(p, size)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Baked {
    pub vertices: Vec<Vertex>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// 検証済みの三角形を元の順に焼く。法線属性の有無は元の `prim.has_normals` に従う。
pub fn bake_box(prim: &Primitive, size: f32) -> Baked {
    let mut baked = Baked {
        vertices: Vec::new(),
        uvs: Vec::new(),
        indices: Vec::with_capacity(prim.indices.len()),
    };
    let mut by_face = HashMap::new();
    for &triangle in prim.indices.as_chunks::<3>().0 {
        let [a, b, c] = triangle.map(|index| Vec3::from(prim.vertices[index as usize].position));
        let face = BoxFace::from_normal((b - a).cross(c - a));
        for index in triangle {
            let baked_index = by_face.entry((index, face)).or_insert_with(|| {
                let vertex = prim.vertices[index as usize];
                let next = baked.vertices.len() as u32;
                baked.vertices.push(vertex);
                baked.uvs.push(face.uv(Vec3::from(vertex.position), size));
                next
            });
            baked.indices.push(*baked_index);
        }
    }
    baked
}

/// 各側の画像表で解決したハッシュを渡す。画像番号があるのに解決できなければ一致しない。
/// 外側の `None` は貼り方のない描画、ハッシュだけが `None` なら画像のない材質を表す。
pub fn same_look(
    left: Option<(&TexUse, Option<ImageHash>)>,
    right: Option<(&TexUse, Option<ImageHash>)>,
) -> bool {
    let ((a, a_hash), (b, b_hash)) = match (left, right) {
        (Some(a), Some(b)) => (a, b),
        (None, None) => return true,
        _ => return false,
    };
    if a_hash != b_hash
        || a.image.is_some() != a_hash.is_some()
        || b.image.is_some() != b_hash.is_some()
        || a.sampling != b.sampling
        || a.cutoff != b.cutoff
    {
        return false;
    }
    match (a.map, b.map) {
        (
            UvMap::Vertex {
                transform: x,
                tex_coord: i,
                ..
            },
            UvMap::Vertex {
                transform: y,
                tex_coord: j,
                ..
            },
        ) => i == j && x.map(f32::to_bits) == y.map(f32::to_bits),
        (UvMap::Box { size: x }, UvMap::Box { size: y }) => x.to_bits() == y.to_bits(),
        (UvMap::Box { size }, _) => matches_baked_box(b, size),
        (_, UvMap::Box { size }) => matches_baked_box(a, size),
    }
}

fn matches_baked_box(tex: &TexUse, size: f32) -> bool {
    matches!(tex.map, UvMap::Vertex { transform, tex_coord: 0, .. } if transform == IDENTITY_TRANSFORM)
        && tex.baked_box.map(f32::to_bits) == Some(size.to_bits())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Aabb;

    #[test]
    fn sniff_reads_magic_bytes() {
        let png = b"\x89PNG\r\n\x1a\n";
        let jpeg = [0xff, 0xd8, 0xff];
        for (magic, mime) in [(png.as_slice(), Mime::Png), (jpeg.as_slice(), Mime::Jpeg)] {
            for len in 0..magic.len() {
                assert_eq!(sniff(&magic[..len]), None);
            }
            assert_eq!(sniff(magic), Some(mime));
            assert_eq!(sniff(&[magic, &[0; 8]].concat()), Some(mime));
            assert_eq!(sniff(&[&[0][..], magic].concat()), None);
        }
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\0"), None);
        assert_eq!(sniff(&[0xff, 0xd8, 0x00]), None);
        assert_eq!(sniff(b"GIF89a"), None);
    }

    #[test]
    fn plan_levels_caps_long_side() {
        let sizes = [[4097, 2049], [256, 8192], [1024, 1], [7, 3], [0, 512]];
        assert_eq!(
            plan_levels(&sizes, 1024, u64::MAX),
            [Some(2), Some(3), Some(0), Some(0), None]
        );
        assert_eq!(plan_levels(&sizes, 0, u64::MAX), [None; 5]);
        assert!(plan_levels(&[], 4096, 0).is_empty());
    }

    #[test]
    fn plan_levels_shrinks_largest_first() {
        let sizes = [[512, 128], [256, 256], [1024, 16], [256, 128]];
        let full = sizes.into_iter().map(chain_bytes).sum::<u64>();
        assert_eq!(plan_levels(&sizes, 1024, full), [Some(0); 4]);
        // 1 バイトだけ足りなくても、最大面積の2枚をともに縮める。
        assert_eq!(
            plan_levels(&sizes, 1024, full - 1),
            [Some(1), Some(1), Some(0), Some(0)]
        );
        let after_pair = full - 2 * (512 * 128 * 4);
        assert_eq!(
            plan_levels(&sizes, 1024, after_pair - 1),
            [Some(1), Some(1), Some(0), Some(1)]
        );
        assert_eq!(plan_levels(&[[u32::MAX; 2]], u32::MAX, u64::MAX), [Some(2)]);
    }

    #[test]
    fn plan_levels_is_order_independent() {
        let sizes = [[512, 256], [1024, 128], [128, 256], [32, 64], [0, 512]];
        let order = [3, 0, 4, 1, 2];
        let reordered = order.map(|i| sizes[i]);
        for budget in [0, 125_000, u64::MAX] {
            let plan = plan_levels(&sizes, 512, budget);
            assert_eq!(plan_levels(&reordered, 512, budget), order.map(|i| plan[i]));
        }
    }

    #[test]
    fn plan_levels_drops_below_64() {
        let last_level = chain_bytes([64, 1]);
        assert_eq!(plan_levels(&[[128, 2]], 128, last_level), [Some(1)]);
        assert_eq!(plan_levels(&[[128, 2]], 128, last_level - 1), [None]);
        assert_eq!(plan_levels(&[[65, 1]], 63, u64::MAX), [None]);
        assert_eq!(plan_levels(&[[1, 1]], 64, 4), [Some(0)]);
        assert_eq!(plan_levels(&[[1, 1]], 64, 3), [None]);
        assert_eq!(plan_levels(&[[64, 1]; 2], 64, last_level), [None; 2]);
    }

    #[test]
    fn chain_bytes_matches_levels() {
        assert_eq!(chain_bytes([1, 1]), 4);
        assert_eq!(chain_bytes([3, 5]), (15 + 2 + 1) * 4);
        assert_eq!(chain_bytes([1, 8]), (8 + 4 + 2 + 1) * 4);
        assert_eq!(chain_bytes([7, 1]), (7 + 3 + 1) * 4);
        assert_eq!(chain_bytes([3, 0]), 0);
        assert_eq!(chain_bytes([u32::MAX; 2]), u64::MAX);
    }

    fn assert_uv(actual: [f32; 2], expected: [f32; 2]) {
        assert!(
            actual
                .into_iter()
                .zip(expected)
                .all(|(a, b)| (a - b).abs() <= 1e-6),
            "{actual:?} != {expected:?}"
        );
    }

    #[test]
    fn box_uv_axes_and_signs() {
        // 各面を外から見たときの、絵の右向きと下向き。
        for (normal, right, down) in [
            (Vec3::X, Vec3::NEG_Z, Vec3::NEG_Y),
            (Vec3::NEG_X, Vec3::Z, Vec3::NEG_Y),
            (Vec3::Y, Vec3::X, Vec3::Z),
            (Vec3::NEG_Y, Vec3::X, Vec3::NEG_Z),
            (Vec3::Z, Vec3::X, Vec3::NEG_Y),
            (Vec3::NEG_Z, Vec3::NEG_X, Vec3::NEG_Y),
        ] {
            for [u, v] in [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]] {
                let p = (normal + right * u + down * v) * 2.0;
                assert_uv(box_uv(p, normal, 2.0), [u, v]);
            }
        }
    }

    #[test]
    fn box_uv_tie_prefers_x_then_y() {
        let p = Vec3::new(2.0, 4.0, 6.0);
        for (normal, uv) in [
            (Vec3::new(-1.0, 1.0, 1.0), [3.0, -2.0]),
            (Vec3::new(1.0, 0.0, -1.0), [-3.0, -2.0]),
            (Vec3::new(0.0, -2.0, 2.0), [1.0, -3.0]),
            (Vec3::splat(-0.0), [-3.0, -2.0]),
        ] {
            assert_uv(box_uv(p, normal, 2.0), uv);
        }
    }

    fn folded_mesh() -> Primitive {
        let positions = [
            [-0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [2.0, 2.0, 0.0],
            [0.0, 0.0, 2.0],
        ];
        Primitive {
            vertices: positions
                .into_iter()
                .map(|position| Vertex {
                    position,
                    normal: [0.0, 1.0, -0.0],
                })
                .collect(),
            // +Z の四角、+X の側面、−Z の裏面、面積ゼロの三角形。
            indices: vec![2, 0, 1, 1, 3, 2, 0, 2, 4, 2, 1, 0, 0, 0, 0],
            local_aabb: Aabb {
                min: Vec3::ZERO,
                max: Vec3::splat(2.0),
            },
            has_normals: true,
        }
    }

    #[test]
    fn bake_box_keeps_triangle_positions() {
        let mut prim = folded_mesh();
        for has_normals in [true, false] {
            prim.has_normals = has_normals;
            if !has_normals {
                for v in &mut prim.vertices {
                    v.normal = [0.0; 3];
                }
            }
            let baked = bake_box(&prim, 2.0);
            assert_eq!(baked.indices.len(), prim.indices.len());
            assert_eq!(baked.uvs.len(), baked.vertices.len());
            let normals = [Vec3::Z, Vec3::Z, Vec3::X, Vec3::NEG_Z, Vec3::ZERO];
            for (i, &index) in baked.indices.iter().enumerate() {
                let original = prim.vertices[prim.indices[i] as usize];
                let vertex = baked.vertices[index as usize];
                assert_eq!(
                    vertex.position.map(f32::to_bits),
                    original.position.map(f32::to_bits)
                );
                assert_eq!(
                    vertex.normal.map(f32::to_bits),
                    original.normal.map(f32::to_bits)
                );
                assert_uv(
                    baked.uvs[index as usize],
                    box_uv(original.position.into(), normals[i / 3], 2.0),
                );
            }
        }
    }

    #[test]
    fn bake_box_splits_only_at_axis_changes() {
        let baked = bake_box(&folded_mesh(), 2.0);
        assert_eq!(baked.vertices.len(), 10);
        assert_eq!(baked.indices, [0, 1, 2, 2, 3, 0, 4, 5, 6, 7, 8, 9, 4, 4, 4]);
    }

    #[test]
    fn same_look_treats_baked_box_as_box() {
        let boxed = TexUse {
            image: Some(ImageId(1)),
            sampling: Sampling::default(),
            map: UvMap::Box { size: 2.0 },
            cutoff: Some(0.5),
            baked_box: None,
        };
        let vertex_map = |tex_coord, info_from, transform| UvMap::Vertex {
            tex_coord,
            info_from,
            transform,
        };
        let baked = TexUse {
            image: Some(ImageId(3)),
            baked_box: Some(2.0),
            map: vertex_map(0, Some(7), IDENTITY_TRANSFORM),
            ..boxed
        };
        let hash = ImageHash::of(b"image");
        let same = |a: &TexUse, b: &TexUse| same_look(Some((a, Some(hash))), Some((b, Some(hash))));
        assert!(same(&boxed, &baked));
        assert!(same(&baked, &boxed));
        assert!(same(
            &boxed,
            &TexUse {
                image: baked.image,
                ..boxed
            }
        ));
        assert!(!same(
            &boxed,
            &TexUse {
                map: UvMap::Box { size: 1.0 },
                ..boxed
            }
        ));
        assert!(same(
            &baked,
            &TexUse {
                map: vertex_map(0, None, IDENTITY_TRANSFORM),
                ..baked
            }
        ));
        let mut shifted = IDENTITY_TRANSFORM;
        shifted[2] = 0.5;
        for different in [
            TexUse {
                map: vertex_map(1, Some(7), IDENTITY_TRANSFORM),
                ..baked
            },
            TexUse {
                map: vertex_map(0, Some(7), shifted),
                ..baked
            },
            TexUse {
                baked_box: None,
                ..baked
            },
            TexUse {
                baked_box: Some(1.0),
                ..baked
            },
            TexUse {
                cutoff: None,
                ..baked
            },
            TexUse {
                sampling: Sampling {
                    mip: Mip::None,
                    ..baked.sampling
                },
                ..baked
            },
        ] {
            assert!(!same(&boxed, &different));
            assert!(!same(&different, &boxed));
        }
        let mut signed_zero = IDENTITY_TRANSFORM;
        signed_zero[2] = -0.0;
        assert!(!same(
            &baked,
            &TexUse {
                map: vertex_map(0, None, signed_zero),
                ..baked
            }
        ));
        let other_hash = ImageHash::of(b"other");
        assert!(!same_look(
            Some((&boxed, Some(hash))),
            Some((&boxed, Some(other_hash)))
        ));
        assert!(!same_look(Some((&boxed, None)), Some((&boxed, None))));
        let mask = TexUse {
            image: None,
            ..boxed
        };
        assert!(same_look(Some((&mask, None)), Some((&mask, None))));
        assert!(!same_look(Some((&mask, None)), None));
        assert!(same_look(None, None));
    }
}
