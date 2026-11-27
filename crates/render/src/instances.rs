//! 表示中の部品のインスタンス。表示状態が変わったときだけ作り直す（非表示の部品は入れない）。
//!
//! 同じプリミティブを参照する部品はバッファの連続した範囲にまとめ、1回の描画命令で描く。
//! ノードを番号順に走査して詰めるので、各まとまりの中は `NodeId` 順に並ぶ。輪郭線のマスクは
//! この並びを二分探索して、選択範囲に入るインスタンスだけを描く。

use std::ops::Range;

use glam::{Affine3A, Mat3};
use model_reviewer_core::scene::{NodeId, SceneRead};

/// インスタンスのフラグ。法線を持たない形状は、シェーダーで面法線を導く。
pub const FLAG_DERIVE_NORMAL: u32 = 1;
/// インスタンスのフラグ。ワールド行列の行列式が負（鏡写し）の部品。表と裏の判定が逆になるので、
/// シェーダーが断面の切り口の色を決めるときに反転する。`instance` がワールド行列から毎回決め直す。
pub const FLAG_MIRRORED: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct InstanceRaw {
    pub world: [[f32; 4]; 4],
    pub normal: [[f32; 4]; 3],
    pub color: [f32; 4],
    pub ids: [u32; 4],
}

const _: () = assert!(std::mem::size_of::<InstanceRaw>() as u64 == crate::plan::INSTANCE_BYTES);

/// 同じプリミティブを描くインスタンスの範囲。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    pub prim: u32,
    pub instances: Range<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InstanceList {
    pub raw: Vec<InstanceRaw>,
    /// `raw` と同じ並びの NodeId。選択範囲の二分探索に使う。
    pub nodes: Vec<u32>,
    pub batches: Vec<Batch>,
    /// 部品ごとのインスタンスの位置の表の区切り（部品の数＋1）。部品 i の位置は
    /// `slots[node_starts[i]..node_starts[i + 1]]`。非表示の部品は空。確定した変形の部分更新に使う。
    pub node_starts: Vec<u32>,
    /// 部品の番号順に並べたインスタンスの位置。
    pub slots: Vec<u32>,
}

impl InstanceList {
    /// 各まとまりのうち、`selected` のノードに当たるインスタンスの範囲。空の範囲は含めない。
    pub fn selected_ranges(&self, selected: &Range<u32>) -> Vec<(u32, Range<u32>)> {
        let mut out = Vec::new();
        if selected.start >= selected.end {
            return out;
        }
        for batch in &self.batches {
            let slice = &self.nodes[batch.instances.start as usize..batch.instances.end as usize];
            let lo = slice.partition_point(|node| *node < selected.start) as u32;
            let hi = slice.partition_point(|node| *node < selected.end) as u32;
            if lo < hi {
                let base = batch.instances.start;
                out.push((batch.prim, base + lo..base + hi));
            }
        }
        out
    }

    /// `nodes`（`NodeId` の連続範囲）の部品のインスタンスの位置。範囲の外は切り詰める。
    pub fn node_slots(&self, nodes: &Range<u32>) -> &[u32] {
        let count = self.node_starts.len().saturating_sub(1) as u32;
        let start = nodes.start.min(count) as usize;
        let end = nodes.end.min(count).max(nodes.start.min(count)) as usize;
        match (self.node_starts.get(start), self.node_starts.get(end)) {
            (Some(&a), Some(&b)) => &self.slots[a as usize..b as usize],
            _ => &[],
        }
    }
}

/// 表示されている部品のインスタンスを作る。2回の走査で数えてから詰め、部品ごとの確保をしない。
pub fn build(model: &impl SceneRead, visible: &dyn Fn(NodeId) -> bool) -> InstanceList {
    let prim_count = model.primitive_count();
    let mut counts = vec![0u32; prim_count];
    let each_draw = |f: &mut dyn FnMut(usize, &model_reviewer_core::scene::Draw)| {
        for index in 0..model.node_count() {
            let id = NodeId(index as u32);
            if !visible(id) {
                continue;
            }
            for draw in model.draws(id) {
                if (draw.prim.0 as usize) < prim_count {
                    f(index, draw);
                }
            }
        }
    };
    each_draw(&mut |_, draw| counts[draw.prim.0 as usize] += 1);

    let mut starts = vec![0u32; prim_count];
    let mut batches = Vec::new();
    let mut total = 0u32;
    for (prim, count) in counts.iter().enumerate() {
        starts[prim] = total;
        if *count > 0 {
            batches.push(Batch {
                prim: prim as u32,
                instances: total..total + count,
            });
        }
        total += count;
    }

    let zero = InstanceRaw {
        world: [[0.0; 4]; 4],
        normal: [[0.0; 4]; 3],
        color: [0.0; 4],
        ids: [0; 4],
    };
    let mut raw = vec![zero; total as usize];
    let mut nodes = vec![0u32; total as usize];
    let mut cursor = starts;
    let mut per_node = vec![0u32; model.node_count()];
    let mut slots = Vec::with_capacity(total as usize);
    each_draw(&mut |index, draw| {
        let id = NodeId(index as u32);
        let has_normals = model
            .primitive(draw.prim)
            .is_none_or(|prim| prim.has_normals);
        let slot = &mut cursor[draw.prim.0 as usize];
        let flags = if has_normals { 0 } else { FLAG_DERIVE_NORMAL };
        raw[*slot as usize] = instance(model.world(id), draw.base_color, index as u32, flags);
        nodes[*slot as usize] = index as u32;
        slots.push(*slot);
        per_node[index] += 1;
        *slot += 1;
    });
    let mut node_starts = Vec::with_capacity(per_node.len() + 1);
    let mut acc = 0u32;
    node_starts.push(0);
    for count in per_node {
        acc += count;
        node_starts.push(acc);
    }
    InstanceList {
        raw,
        nodes,
        batches,
        node_starts,
        slots,
    }
}

/// 1件のインスタンス。確定した変形の部分更新でも、作り直しと同じ値になるようにここだけで作る。
/// 鏡写しの印（`FLAG_MIRRORED`）は、渡された `flags` の値を捨てて `world` から決め直す。
pub(crate) fn instance(world: &Affine3A, color: [f32; 4], node: u32, flags: u32) -> InstanceRaw {
    let flags = (flags & !FLAG_MIRRORED) | if is_mirrored(world) { FLAG_MIRRORED } else { 0 };
    let m = glam::Mat4::from(*world).to_cols_array_2d();
    let n = normal_matrix(world);
    let cols = n.to_cols_array_2d();
    InstanceRaw {
        world: m,
        normal: [
            [cols[0][0], cols[0][1], cols[0][2], 0.0],
            [cols[1][0], cols[1][1], cols[1][2], 0.0],
            [cols[2][0], cols[2][1], cols[2][2], 0.0],
        ],
        color: [color[0], color[1], color[2], 1.0],
        ids: [node, flags, 0, 0],
    }
}

/// ワールド行列の左上3×3の行列式が負か（鏡写し）。0 と有限でない値は鏡写しとしない。
pub fn is_mirrored(world: &Affine3A) -> bool {
    let det = world.matrix3.determinant();
    det.is_finite() && det < 0.0
}

/// 左上3×3の逆転置。逆行列が有限でない（拡大率0など）ときは単位行列。
pub fn normal_matrix(world: &Affine3A) -> Mat3 {
    let linear = Mat3::from(world.matrix3);
    let det = linear.determinant();
    if !det.is_finite() || det.abs() < f32::MIN_POSITIVE {
        return Mat3::IDENTITY;
    }
    let n = linear.inverse().transpose();
    if n.is_finite() { n } else { Mat3::IDENTITY }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn mirrored_flag_follows_the_world_not_the_given_flags() {
        let plain = Affine3A::from_scale(Vec3::new(2.0, 1.0, 1.0));
        let mirrored = Affine3A::from_scale(Vec3::new(-2.0, 1.0, 1.0));
        let color = [0.5; 4];
        let a = instance(&mirrored, color, 3, FLAG_DERIVE_NORMAL);
        assert_eq!(a.ids[1], FLAG_DERIVE_NORMAL | FLAG_MIRRORED);
        // 部分更新は前の印を渡す。鏡写しでなくなったら印が外れ、法線の印は残る。
        let b = instance(&plain, color, 3, a.ids[1]);
        assert_eq!(b.ids[1], FLAG_DERIVE_NORMAL);
        let c = instance(&mirrored, color, 3, 0);
        assert_eq!(c.ids[1], FLAG_MIRRORED);
        let flat = Affine3A::from_scale(Vec3::new(0.0, 1.0, 1.0));
        assert_eq!(instance(&flat, color, 3, FLAG_MIRRORED).ids[1], 0);
        let nan = Affine3A::from_scale(Vec3::new(f32::NAN, 1.0, 1.0));
        assert!(!is_mirrored(&nan));
    }
}
