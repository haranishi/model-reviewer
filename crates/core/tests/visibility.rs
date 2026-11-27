//! 表示状態。親を隠すと子孫の実効表示だけが落ちる。

use glam::Affine3A;

use model_reviewer_core::math::Aabb;
use model_reviewer_core::scene::{Model, ModelStats, Node, NodeId};
use model_reviewer_core::visibility::Visibility;

fn node(name: &str, gltf_index: u32, parent: Option<NodeId>, end: u32) -> Node {
    Node {
        name: name.to_owned(),
        gltf_index,
        parent,
        subtree_end: end,
        depth: u32::from(parent.is_some()),
        local: Affine3A::IDENTITY,
        world: Affine3A::IDENTITY,
        draws: 0..0,
        world_aabb: Aabb::empty(),
    }
}

/// 0 根、1 子A、2 孫、3 子B。
fn family() -> Model {
    Model {
        nodes: vec![
            node("根", 0, None, 4),
            node("子A", 4, Some(NodeId(0)), 3),
            node("孫", 9, Some(NodeId(1)), 3),
            node("子B", 6, Some(NodeId(0)), 4),
        ],
        draws: Vec::new(),
        primitives: Vec::new(),
        bounds: Aabb::empty(),
        stats: ModelStats {
            nodes: 4,
            ..ModelStats::default()
        },
        warnings: Vec::new(),
    }
}

fn hidden(vis: &Visibility) -> Vec<u32> {
    vis.hidden_own().map(|id| id.0).collect()
}

#[test]
fn hiding_a_parent_hides_descendants_without_forgetting_their_own_flags() {
    let model = family();
    let root = NodeId(0);
    let child_a = NodeId(1);
    let grandchild = NodeId(2);
    let child_b = NodeId(3);
    let mut vis = Visibility::all_visible(model.nodes.len());

    vis.set(&model, root, false);
    for id in [root, child_a, grandchild, child_b] {
        assert!(!vis.effective(id), "node {} stays visible", id.0);
    }
    assert!(!vis.own(root));
    assert!(vis.own(child_a) && vis.own(grandchild) && vis.own(child_b));
    assert_eq!(hidden(&vis), vec![0]);

    vis.set(&model, root, true);
    for id in [root, child_a, grandchild, child_b] {
        assert!(vis.own(id) && vis.effective(id));
    }
    assert!(hidden(&vis).is_empty());

    vis.set(&model, child_a, false);
    vis.set(&model, root, false);
    vis.set(&model, root, true);
    assert!(vis.own(root) && vis.effective(root));
    assert!(!vis.own(child_a) && !vis.effective(child_a));
    assert!(vis.own(grandchild) && !vis.effective(grandchild));
    assert!(vis.own(child_b) && vis.effective(child_b));
    assert_eq!(hidden(&vis), vec![1]);

    vis.show_all();
    for id in [root, child_a, grandchild, child_b] {
        assert!(vis.own(id) && vis.effective(id));
    }
}

#[test]
fn out_of_range_ids_do_not_panic_or_change_flags() {
    let model = family();
    let mut vis = Visibility::all_visible(model.nodes.len());
    vis.set(&model, NodeId(1), false);
    vis.set(&model, NodeId(99), false);
    vis.set(&model, NodeId(u32::MAX), true);
    assert_eq!(hidden(&vis), vec![1]);
    assert!(!vis.own(NodeId(99)));
    assert!(!vis.effective(NodeId(u32::MAX)));
    assert!(vis.own(NodeId(0)));
}

/// 0 根、1 子A、2 孫、3 子B から子A（1..3）を切り取った形。
fn without_child_a() -> Model {
    Model {
        nodes: vec![node("根", 0, None, 2), node("子B", 6, Some(NodeId(0)), 2)],
        draws: Vec::new(),
        primitives: Vec::new(),
        bounds: Aabb::empty(),
        stats: ModelStats {
            nodes: 2,
            ..ModelStats::default()
        },
        warnings: Vec::new(),
    }
}

#[test]
fn cutting_and_splicing_a_subtree_restores_flags_exactly() {
    let model = family();
    let mut vis = Visibility::all_visible(4);
    vis.set(&model, NodeId(2), false);
    vis.set(&model, NodeId(0), false);
    vis.set(&model, NodeId(0), true);
    vis.set(&model, NodeId(1), false);
    let original = vis.clone();

    let own = vis.cut(1..3);
    assert_eq!(own, vec![false, false]);
    assert_eq!(vis.len(), 2);
    assert!(
        vis.own(NodeId(1)) && vis.effective(NodeId(1)),
        "子Bはそのまま"
    );
    let mut recomputed = vis.clone();
    recomputed.set_many(&without_child_a(), [], true);
    assert_eq!(
        recomputed, vis,
        "切り取った後の実効表示は、残りの表から計算したものと同じ"
    );

    vis.splice(&model, NodeId(1), &own);
    assert_eq!(vis, original);
    assert!(!vis.effective(NodeId(2)));
}

#[test]
fn spliced_parts_under_a_hidden_parent_are_hidden() {
    let model = family();
    let mut vis = Visibility::all_visible(4);
    vis.set(&model, NodeId(0), false);
    let own = vis.cut(1..3);
    vis.splice(&model, NodeId(1), &own);
    assert!(vis.own(NodeId(1)) && !vis.effective(NodeId(1)));
    assert!(vis.own(NodeId(2)) && !vis.effective(NodeId(2)));

    // 範囲外は末尾に足し、長さを超える切り取りは無視する。
    let mut short = Visibility::all_visible(2);
    short.splice(&without_child_a(), NodeId(99), &[false]);
    assert_eq!(short.len(), 3);
    assert_eq!(short.cut(2..10), vec![false]);
    assert!(short.cut(5..9).is_empty());
    assert_eq!(short.len(), 2);
}

#[test]
fn set_many_recomputes_once() {
    let model = family();
    let mut vis = Visibility::all_visible(4);
    vis.set_many(&model, [NodeId(1), NodeId(3), NodeId(42)], false);
    assert_eq!(hidden(&vis), vec![1, 3]);
    assert!(!vis.effective(NodeId(2)));
    vis.set_many(&model, [NodeId(1)], true);
    assert!(vis.effective(NodeId(2)));
    assert!(!vis.is_empty());
}
