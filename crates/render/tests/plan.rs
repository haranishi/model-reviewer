//! GPUの要らない試験：ページ分割と上限、形状の共有、インスタンスの並び。

mod common;

use common::{cube, model, node, tetra, trs};
use glam::{Affine3A, Quat, Vec3};
use model_reviewer_core::scene::NodeId;
use model_reviewer_render::instances::{self, FLAG_DERIVE_NORMAL};
use model_reviewer_render::plan::{INSTANCE_BYTES, PAGE_TARGET_BYTES};
use model_reviewer_render::{RenderError, plan_buffers};

const GREY: [f32; 4] = [0.5, 0.5, 0.5, 1.0];

fn wheels() -> model_reviewer_core::scene::Model {
    model(
        vec![cube(true)],
        vec![
            node("root", None, Affine3A::IDENTITY, vec![]),
            node(
                "a",
                Some(0),
                Affine3A::from_translation(Vec3::X),
                vec![(0, GREY)],
            ),
            node(
                "b",
                Some(0),
                Affine3A::from_translation(Vec3::Y),
                vec![(0, GREY)],
            ),
            node(
                "c",
                Some(0),
                Affine3A::from_translation(Vec3::Z),
                vec![(0, GREY)],
            ),
            node(
                "d",
                Some(0),
                Affine3A::from_translation(-Vec3::X),
                vec![(0, GREY)],
            ),
        ],
    )
}

#[test]
fn shared_mesh_is_packed_once() {
    let m = wheels();
    let plan = plan_buffers(&m, &wgpu::Limits::default()).expect("計画できる");
    let prim = &m.primitives[0];
    let one = (prim.vertices.len() * 24 + prim.indices.len() * 4) as u64;
    assert_eq!(plan.prims.len(), 1);
    assert_eq!(plan.pages.len(), 1);
    assert_eq!(plan.geometry_bytes, one, "4部品が共有しても形状は1回分");
    assert_eq!(plan.placed_instances, 4);
}

#[test]
fn small_limit_is_reported_before_allocation() {
    let m = wheels();
    let limits = wgpu::Limits {
        max_buffer_size: 100,
        ..wgpu::Limits::default()
    };
    match plan_buffers(&m, &limits) {
        Err(RenderError::LimitExceeded {
            what,
            needed,
            limit,
        }) => {
            assert_eq!(what, "頂点バッファ");
            assert_eq!(needed, 24 * 24);
            assert_eq!(limit, 100);
        }
        other => panic!("上限超過になるはず: {other:?}"),
    }
}

#[test]
fn instance_buffer_limit_is_checked() {
    let mut m = wheels();
    m.primitives[0] = tetra();
    // 形状（頂点96バイト）は収まるが、インスタンス4件（576バイト）は収まらない。
    let limits = wgpu::Limits {
        max_buffer_size: 4 * INSTANCE_BYTES - 1,
        ..wgpu::Limits::default()
    };
    match plan_buffers(&m, &limits) {
        Err(RenderError::LimitExceeded { what, .. }) => assert_eq!(what, "インスタンスバッファ"),
        other => panic!("インスタンスの上限超過になるはず: {other:?}"),
    }
}

#[test]
fn pages_split_at_target_and_offsets_are_consistent() {
    let prims = (0..10).map(|_| cube(true)).collect::<Vec<_>>();
    let specs = (0..10)
        .map(|i| node("p", None, Affine3A::IDENTITY, vec![(i, GREY)]))
        .collect();
    let m = model(prims, specs);
    // 立方体1つは頂点576バイト・インデックス144バイト。上限1500バイトなら1ページに2つまで。
    let limits = wgpu::Limits {
        max_buffer_size: 1500,
        ..wgpu::Limits::default()
    };
    let plan = plan_buffers(&m, &limits).expect("計画できる");
    assert_eq!(plan.pages.len(), 5);
    for page in &plan.pages {
        assert!(page.vertex_bytes <= 1500 && page.index_bytes <= 1500);
    }
    assert_eq!(plan.prims[0].page, 0);
    assert_eq!(plan.prims[1].page, 0);
    assert_eq!(plan.prims[1].base_vertex, 24);
    assert_eq!(plan.prims[1].first_index, 36);
    assert_eq!(plan.prims[2].page, 1);
    assert_eq!(plan.prims[2].base_vertex, 0);
    const { assert!(PAGE_TARGET_BYTES >= 64 * 1024 * 1024) };
}

#[test]
fn draw_to_missing_primitive_is_an_error() {
    let mut m = wheels();
    m.draws[0].prim = model_reviewer_core::scene::PrimId(9);
    assert!(matches!(
        plan_buffers(&m, &wgpu::Limits::default()),
        Err(RenderError::Device(_))
    ));
}

#[test]
fn instances_are_batched_by_primitive_in_node_order() {
    let m = model(
        vec![cube(true), tetra()],
        vec![
            node("root", None, Affine3A::IDENTITY, vec![]),
            node("t1", Some(0), Affine3A::IDENTITY, vec![(1, GREY)]),
            node("c1", Some(0), Affine3A::IDENTITY, vec![(0, GREY)]),
            node(
                "both",
                Some(0),
                Affine3A::IDENTITY,
                vec![(0, GREY), (1, GREY)],
            ),
            node("c2", Some(0), Affine3A::IDENTITY, vec![(0, GREY)]),
        ],
    );
    let list = instances::build(&m, &|_| true);
    assert_eq!(list.batches.len(), 2);
    assert_eq!(list.batches[0].prim, 0);
    assert_eq!(list.batches[0].instances, 0..3);
    assert_eq!(list.batches[1].prim, 1);
    assert_eq!(list.batches[1].instances, 3..5);
    assert_eq!(list.nodes, vec![2, 3, 4, 1, 3]);
    for batch in &list.batches {
        let ids = &list.nodes[batch.instances.start as usize..batch.instances.end as usize];
        assert!(
            ids.windows(2).all(|w| w[0] <= w[1]),
            "まとまりの中はNodeId順"
        );
    }
    // 法線なしの四面体には面法線の印が付く。
    assert_eq!(list.raw[3].ids[1] & FLAG_DERIVE_NORMAL, FLAG_DERIVE_NORMAL);
    assert_eq!(list.raw[0].ids[1] & FLAG_DERIVE_NORMAL, 0);

    // 選択範囲 [3, 5) は、立方体の 1..3 と四面体の 4..5。
    let ranges = list.selected_ranges(&(3..5));
    assert_eq!(ranges, vec![(0, 1..3), (1, 4..5)]);
    assert!(list.selected_ranges(&(0..1)).is_empty());
}

#[test]
fn hidden_parts_are_not_in_the_instance_buffer() {
    let m = wheels();
    let list = instances::build(&m, &|id: NodeId| id.0 != 2);
    assert_eq!(list.raw.len(), 3);
    assert_eq!(list.nodes, vec![1, 3, 4]);
    let none = instances::build(&m, &|_| false);
    assert!(none.raw.is_empty() && none.batches.is_empty());
}

#[test]
fn normal_matrix_handles_non_uniform_and_zero_scale() {
    let world = trs(
        Vec3::new(2.0, 0.5, 1.0),
        Quat::from_rotation_y(0.3),
        Vec3::ONE,
    );
    let n = instances::normal_matrix(&world);
    // 拡大した面の法線は、逆転置で変換すると面に垂直なまま。
    let tangent = world.transform_vector3(Vec3::new(1.0, 1.0, 0.0).normalize());
    let normal = n * Vec3::new(-1.0, 1.0, 0.0).normalize();
    assert!(tangent.dot(normal).abs() < 1e-5);
    let flat = trs(Vec3::new(1.0, 0.0, 1.0), Quat::IDENTITY, Vec3::ZERO);
    assert_eq!(instances::normal_matrix(&flat), glam::Mat3::IDENTITY);
}
