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

/// 部分更新で書き直した結果。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Patched {
    /// 書き直したインスタンスの位置（小さい順）。
    pub slots: Vec<u32>,
    /// 描画の数と位置の数が合わず、色を前の値のままにした部品の数。
    pub mismatched: u32,
}

/// `nodes`（`NodeId` の連続範囲）の部品のインスタンスを、文書の今の値で書き直す（GPUには送らない）。
/// 行列と色の両方を作り直す。位置の表は部品の順・描画の順なので、部品の i 番目の位置が
/// i 番目の描画に当たる。描画の数と位置の数が合わない部品は、色を前の値のままにして数える
/// （非表示の部品は位置を持たないので何もしない）。番号と法線の印は前の値を渡す。
/// 部品の数が `list` と違えば何もしない。
pub(crate) fn patch(
    list: &mut InstanceList,
    model: &impl SceneRead,
    nodes: &Range<u32>,
) -> Patched {
    let mut out = Patched::default();
    if list.node_starts.len() != model.node_count() + 1 {
        return out;
    }
    let count = model.node_count() as u32;
    for node in nodes.start.min(count)..nodes.end.min(count) {
        let index = node as usize;
        let at = list.node_starts[index] as usize..list.node_starts[index + 1] as usize;
        if at.is_empty() {
            continue;
        }
        let id = NodeId(node);
        let draws = model.draws(id);
        let matched = draws.len() == at.len();
        if !matched {
            out.mismatched += 1;
        }
        let world = model.world(id);
        for (i, &slot) in list.slots[at].iter().enumerate() {
            let old = list.raw[slot as usize];
            let color = if matched {
                draws[i].base_color
            } else {
                old.color
            };
            list.raw[slot as usize] = instance(world, color, node, old.ids[1]);
            out.slots.push(slot);
        }
    }
    out.slots.sort_unstable();
    out
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
    use model_reviewer_core::doc::{Document, NodeKey};
    use model_reviewer_core::math::Aabb;
    use model_reviewer_core::scene::{Draw, Model, ModelStats, Node, PrimId, Primitive};
    use std::sync::Arc;

    fn prim() -> Primitive {
        Primitive {
            vertices: Vec::new(),
            indices: Vec::new(),
            local_aabb: Aabb {
                min: Vec3::splat(-0.5),
                max: Vec3::splat(0.5),
            },
            has_normals: true,
        }
    }

    fn shade(node: u32, draw: u32) -> [f32; 4] {
        [node as f32 * 0.1, draw as f32 * 0.1, 0.5, 0.75]
    }

    /// 根の並びに部品4個。部品 n は形状 (n + k) % 2 の描画を k = 0..(n % 3 + 1) 個持つので、
    /// 1つの部品のインスタンスが2つのまとまりに散らばる。
    fn scattered() -> Arc<Model> {
        let mut nodes = Vec::new();
        let mut draws = Vec::new();
        for n in 0..4u32 {
            let start = draws.len() as u32;
            for k in 0..(n % 3 + 1) {
                draws.push(Draw {
                    prim: PrimId((n + k) % 2),
                    base_color: shade(n, k),
                });
            }
            let local = Affine3A::from_translation(Vec3::new(n as f32, 0.0, 0.0));
            nodes.push(Node {
                name: format!("部品{n}"),
                gltf_index: n,
                parent: None,
                subtree_end: n + 1,
                depth: 0,
                local,
                world: local,
                draws: start..draws.len() as u32,
                world_aabb: Aabb::empty(),
            });
        }
        Arc::new(Model {
            nodes,
            draws,
            primitives: vec![prim(), prim()],
            bounds: Aabb::empty(),
            stats: ModelStats::default(),
            warnings: Vec::new(),
        })
    }

    #[test]
    fn slots_follow_node_then_draw_order() {
        let model = scattered();
        let list = build(&*model, &|_| true);
        for n in 0..4u32 {
            let slots = list.node_slots(&(n..n + 1));
            let draws = model.draws(NodeId(n));
            assert_eq!(slots.len(), draws.len());
            for (i, &slot) in slots.iter().enumerate() {
                let raw = list.raw[slot as usize];
                assert_eq!(list.nodes[slot as usize], n);
                assert_eq!(raw.ids[0], n);
                let expected = draws[i].base_color;
                assert_eq!(&raw.color[..3], &expected[..3], "部品{n} 描画{i}");
            }
        }
        // 部品1は2つのまとまりにまたがる（位置が連続しない）。
        let one = list.node_slots(&(1..2));
        assert_eq!(one.len(), 2);
        assert_ne!(one[0] + 1, one[1]);
    }

    #[test]
    fn patch_rewrites_color_and_keeps_ids() {
        let model = scattered();
        let mut doc = Document::new(Arc::clone(&model));
        let mut list = build(&doc, &|_| true);
        let before = list.clone();
        let paint = [0.2, 0.4, 0.6, 0.75];
        let count = doc.subtree_colors(NodeKey(2)).unwrap().len();
        let range = doc
            .set_subtree_colors(NodeKey(2), &vec![paint; count])
            .unwrap();
        assert_eq!(range, 2..3);

        let patched = patch(&mut list, &doc, &range);
        assert_eq!(patched.mismatched, 0);
        let mut expected_slots = before.node_slots(&range).to_vec();
        expected_slots.sort_unstable();
        assert_eq!(patched.slots, expected_slots);
        // 範囲の部品だけが新しい色で、透明度は描画でも 1。行列と番号と印は変わらない。
        for slot in 0..list.raw.len() {
            let (new, old) = (list.raw[slot], before.raw[slot]);
            assert_eq!(new.world, old.world);
            assert_eq!(new.normal, old.normal);
            assert_eq!(new.ids, old.ids);
            if expected_slots.contains(&(slot as u32)) {
                assert_eq!(new.color, [0.2, 0.4, 0.6, 1.0]);
            } else {
                assert_eq!(new.color, old.color);
            }
        }
        assert_eq!(list.nodes, before.nodes);
        assert_eq!(list.batches, before.batches);
        assert_eq!(list.slots, before.slots);
        // 作り直したときと同じ値になる。
        assert_eq!(list, build(&doc, &|_| true));
    }

    #[test]
    fn patch_moves_and_keeps_color_when_only_the_matrix_changed() {
        let model = scattered();
        let mut doc = Document::new(Arc::clone(&model));
        let mut list = build(&doc, &|_| true);
        let moved = Affine3A::from_translation(Vec3::new(0.0, 3.0, 0.0));
        let range = doc.set_local(NodeKey(1), moved).unwrap();
        let patched = patch(&mut list, &doc, &range);
        assert_eq!(patched.slots.len(), 2);
        assert_eq!(list, build(&doc, &|_| true));
    }

    #[test]
    fn patch_skips_hidden_and_keeps_color_on_mismatch() {
        let model = scattered();
        let mut doc = Document::new(Arc::clone(&model));
        // 部品1を隠して作ったインスタンス。部品1の色を変えても、位置が無いので何も起きない。
        let mut list = build(&doc, &|id| id.0 != 1);
        let before = list.clone();
        doc.set_subtree_colors(NodeKey(1), &[[0.9, 0.9, 0.9, 1.0]; 2])
            .unwrap();
        let patched = patch(&mut list, &doc, &(1..2));
        assert!(patched.slots.is_empty());
        assert_eq!(patched.mismatched, 0);
        assert_eq!(list, before);

        // 描画の数と位置の数が合わない部品（ここでは別のモデルの部品0）は、色を前の値のまま。
        let mut list = build(&doc, &|_| true);
        let mut other = (*model).clone();
        other.draws[0].base_color = [0.0, 1.0, 0.0, 1.0];
        other.draws.insert(
            0,
            Draw {
                prim: PrimId(1),
                base_color: [1.0, 0.0, 0.0, 1.0],
            },
        );
        for node in &mut other.nodes {
            node.draws = if node.draws.start == 0 {
                0..node.draws.end + 1
            } else {
                node.draws.start + 1..node.draws.end + 1
            };
        }
        let old0 = list.raw[list.node_slots(&(0..1))[0] as usize];
        let patched = patch(&mut list, &other, &(0..1));
        assert_eq!(patched.mismatched, 1);
        let new0 = list.raw[list.node_slots(&(0..1))[0] as usize];
        assert_eq!(new0.color, old0.color);
        // 部品の数が違う相手では何もしない。
        let mut fewer = (*model).clone();
        fewer.nodes.pop();
        let before = list.clone();
        assert_eq!(patch(&mut list, &fewer, &(0..4)), Patched::default());
        assert_eq!(list, before);
    }

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
