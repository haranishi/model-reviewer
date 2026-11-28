//! GPUの要らない試験：ページ分割と上限、形状の共有、インスタンスの並び。

mod common;

use common::{cube, model, node, tetra, trs};
use glam::{Affine3A, Quat, Vec3};
use model_reviewer_core::scene::NodeId;
use model_reviewer_render::instances::{self, FLAG_DERIVE_NORMAL, FLAG_MIRRORED};
use model_reviewer_render::plan::{INSTANCE_BYTES, PAGE_TARGET_BYTES};
use model_reviewer_render::{RenderError, plan_buffers};

const GREY: [f32; 4] = [0.5, 0.5, 0.5, 1.0];

#[test]
fn uv_pages_are_separate() {
    let mut m = model((0..6).map(|_| tetra()).collect(), vec![]);
    m.textures.prim_uvs = (0..6)
        .map(|i| {
            if i % 2 == 0 {
                vec![[0.5; 2]; 4]
            } else {
                vec![]
            }
        })
        .collect();
    let limits = wgpu::Limits {
        max_buffer_size: 200,
        ..Default::default()
    };
    let plan = plan_buffers(&m, &limits).unwrap();
    let slots: Vec<_> = plan
        .prims
        .iter()
        .map(|s| (s.page, s.base_vertex, s.first_index))
        .collect();
    assert_eq!(
        slots,
        [
            (0, 0, 0),
            (1, 0, 0),
            (0, 4, 12),
            (1, 4, 12),
            (2, 0, 0),
            (3, 0, 0)
        ]
    );
    assert_eq!(
        plan.pages.iter().map(|p| p.uv_bytes).collect::<Vec<_>>(),
        [64, 0, 32, 0]
    );
    assert_eq!(plan.geometry_bytes, 6 * (96 + 48) + 3 * 32);
}

#[test]
fn plan_without_uvs_is_unchanged() {
    use model_reviewer_render::{PagePlan, PrimSlot};
    let m = model(
        vec![tetra(), cube(true)],
        vec![node(
            "両方",
            None,
            Affine3A::IDENTITY,
            vec![(1, GREY), (0, GREY)],
        )],
    );
    let p = plan_buffers(&m, &wgpu::Limits::default()).unwrap();
    assert_eq!(
        p.pages,
        [PagePlan {
            vertex_bytes: 672,
            index_bytes: 192,
            uv_bytes: 0
        }]
    );
    assert_eq!(
        p.prims,
        [
            PrimSlot {
                page: 0,
                base_vertex: 0,
                first_index: 0,
                index_count: 12
            },
            PrimSlot {
                page: 0,
                base_vertex: 4,
                first_index: 12,
                index_count: 36
            },
        ]
    );
    assert_eq!((p.geometry_bytes, p.placed_instances), (864, 2));
}

fn mixed_batches() -> instances::InstanceList {
    use model_reviewer_core::texture::{IDENTITY_TRANSFORM, ImageId, TexId, TexUse, UvMap};
    let draws = [
        vec![Some(0), Some(1)],
        vec![Some(2), None],
        vec![Some(0), Some(2), Some(1)],
        vec![None, Some(1)],
    ];
    let mut m = model(
        vec![tetra()],
        draws
            .iter()
            .map(|ids| node("部品", None, Affine3A::IDENTITY, vec![(0, GREY); ids.len()]))
            .collect(),
    );
    m.textures.prim_uvs = vec![vec![[0.0; 2]; 4]];
    let uv = UvMap::Vertex {
        transform: IDENTITY_TRANSFORM,
        tex_coord: 0,
        info_from: None,
    };
    m.textures.uses = [
        UvMap::Box {
            size: 1.0,
            aspect: 1.0,
        },
        uv,
        uv,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, map)| TexUse {
        image: Some(ImageId(i as u32)),
        sampling: Default::default(),
        map,
        cutoff: None,
        baked_box: None,
    })
    .collect();
    for (draw, id) in m.draws.iter_mut().zip(draws.into_iter().flatten()) {
        draw.tex = id.map(TexId);
    }
    instances::build(&m, &|_| true)
}

#[test]
fn batches_split_by_texture_and_keep_node_order() {
    let list = mixed_batches();
    let groups: Vec<_> = list
        .batches
        .iter()
        .map(|b| (b.prim, b.instances.clone()))
        .collect();
    assert_eq!(groups, [(0, 0..2), (0, 2..5), (0, 5..7), (0, 7..9)]);
    assert_eq!(list.nodes, [1, 3, 0, 2, 3, 1, 2, 0, 2]);
    // 部分更新で使う位置は、部品の中の描画順。
    assert_eq!(list.node_slots(&(2..3)), [8, 6, 3]);
}

#[test]
fn batch_order_is_plain_uv_box() {
    let ids: Vec<_> = mixed_batches()
        .batches
        .iter()
        .map(|b| b.tex.map(|id| id.0))
        .collect();
    assert_eq!(ids, [None, Some(1), Some(2), Some(0)]);
}

#[test]
fn selected_ranges_work_across_texture_batches() {
    assert_eq!(
        mixed_batches().selected_ranges(&(1..3)),
        [(0, 0..1), (0, 3..4), (0, 5..7), (0, 8..9)]
    );
}

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

#[test]
fn mirrored_parts_are_marked_when_the_determinant_is_negative() {
    let m = model(
        vec![cube(true), tetra()],
        vec![
            node("root", None, Affine3A::IDENTITY, vec![]),
            node(
                "plain",
                Some(0),
                trs(
                    Vec3::new(2.0, 1.0, 1.0),
                    Quat::from_rotation_y(0.4),
                    Vec3::X,
                ),
                vec![(0, GREY)],
            ),
            node(
                "mirrored",
                Some(0),
                trs(Vec3::new(-1.0, 1.0, 1.0), Quat::IDENTITY, Vec3::Y),
                vec![(0, GREY), (1, GREY)],
            ),
            node(
                "two_negative",
                Some(0),
                trs(Vec3::new(-1.0, -1.0, 1.0), Quat::IDENTITY, Vec3::Z),
                vec![(0, GREY)],
            ),
        ],
    );
    let list = instances::build(&m, &|_| true);
    // 立方体の並びは plain・mirrored・two_negative、四面体は mirrored。
    assert_eq!(list.nodes, vec![1, 2, 3, 2]);
    let flags: Vec<u32> = list.raw.iter().map(|raw| raw.ids[1]).collect();
    assert_eq!(flags[0] & FLAG_MIRRORED, 0);
    assert_eq!(flags[1] & FLAG_MIRRORED, FLAG_MIRRORED);
    assert_eq!(flags[2] & FLAG_MIRRORED, 0, "負が2つなら鏡写しではない");
    // 鏡写しの印と法線の印は、同じ場所に独立して立つ。
    assert_eq!(flags[3], FLAG_MIRRORED | FLAG_DERIVE_NORMAL);
    assert_eq!(flags[1] & FLAG_DERIVE_NORMAL, 0);
}
