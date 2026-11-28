//! 文書: 開いた直後の内容、変形、挿入と削除、複製、図形、番号の付け替え。

use std::sync::Arc;

use glam::{Affine3A, Quat, Vec3};

use model_reviewer_core::doc::{DocError, Document, DrawLook, NodeKey, Place, Remap, Subtree};
use model_reviewer_core::math::Aabb;
use model_reviewer_core::scene::{
    Draw, Model, ModelStats, Node, NodeId, PrimId, Primitive, SceneRead,
};
use model_reviewer_core::shapes::{self, ShapeKind, ShapeSpec};
use model_reviewer_core::texture::{
    IDENTITY_TRANSFORM, ImageHash, ImageId, ImageInfo, ImageSource, Mime, Sampling, TexId, TexUse,
    Textures, UvMap,
};

/// 決まった種の乱数（xorshift64）。
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn unit(&mut self) -> f32 {
        (self.next() % 10_000) as f32 / 10_000.0
    }
}

fn prim(min: f32, max: f32) -> Primitive {
    Primitive {
        vertices: Vec::new(),
        indices: Vec::new(),
        local_aabb: Aabb {
            min: Vec3::splat(min),
            max: Vec3::splat(max),
        },
        has_normals: true,
    }
}

/// 行きがけ順の (親, local, 描く形状) から、読み込みと同じ計算で `Model` を作る。
fn build(specs: &[(Option<usize>, Affine3A, Vec<u32>)], prims: Vec<Primitive>) -> Model {
    let count = specs.len();
    let mut ends: Vec<u32> = (1..=count as u32).collect();
    for index in (0..count).rev() {
        if let Some(parent) = specs[index].0 {
            ends[parent] = ends[parent].max(ends[index]);
        }
    }
    let mut nodes: Vec<Node> = Vec::with_capacity(count);
    let mut draws = Vec::new();
    for (index, (parent, local, shapes)) in specs.iter().enumerate() {
        let (parent_world, depth) = match parent {
            Some(parent) => (nodes[*parent].world, nodes[*parent].depth + 1),
            None => (Affine3A::IDENTITY, 0),
        };
        let world = parent_world * *local;
        let start = draws.len() as u32;
        let mut aabb = Aabb::empty();
        for &shape in shapes {
            draws.push(Draw {
                prim: PrimId(shape),
                base_color: [0.5, 0.5, 0.5, 1.0],
                tex: None,
            });
            aabb = aabb.union(prims[shape as usize].local_aabb.transformed(&world));
        }
        nodes.push(Node {
            name: format!("部品{index}"),
            gltf_index: index as u32 * 3 + 1,
            parent: parent.map(|parent| NodeId(parent as u32)),
            subtree_end: ends[index],
            depth,
            local: *local,
            world,
            draws: start..draws.len() as u32,
            world_aabb: aabb,
        });
    }
    let bounds = nodes
        .iter()
        .fold(Aabb::empty(), |bounds, node| bounds.union(node.world_aabb));
    Model {
        textures: Textures::default(),
        stats: ModelStats {
            nodes: count as u32,
            ..ModelStats::default()
        },
        nodes,
        draws,
        primitives: prims,
        bounds,
        warnings: Vec::new(),
    }
}

fn shift(x: f32) -> Affine3A {
    Affine3A::from_translation(Vec3::new(x, 0.5 * x, -x))
}

/// 0 根(5) ├1 腕(3) │├2 手 │└3 指 ├4 車輪 └5 胴(7) │└6 首  7 二本目の根
fn family() -> Model {
    build(
        &[
            (None, shift(1.0), vec![0]),
            (Some(0), shift(2.0), vec![1]),
            (Some(1), Affine3A::from_rotation_y(0.3), vec![0, 1]),
            (Some(1), Affine3A::from_scale(Vec3::splat(2.0)), vec![]),
            (Some(0), shift(-1.0), vec![1]),
            (Some(0), shift(0.25), vec![0]),
            (Some(5), shift(0.1), vec![1]),
            (None, shift(4.0), vec![0]),
        ],
        vec![prim(-1.0, 1.0), prim(0.0, 0.5)],
    )
}

/// 行きがけ順の無作為な木。
fn random_model(rng: &mut Rng, count: usize) -> Model {
    let mut specs: Vec<(Option<usize>, Affine3A, Vec<u32>)> = Vec::new();
    let mut path: Vec<usize> = Vec::new();
    for index in 0..count {
        let keep = if path.is_empty() {
            0
        } else {
            rng.below(path.len() + 1)
        };
        path.truncate(keep);
        let parent = path.last().copied();
        let local = Affine3A::from_scale_rotation_translation(
            Vec3::splat(0.5 + rng.unit()),
            Quat::from_rotation_z(rng.unit() * 3.0),
            Vec3::new(rng.unit() * 4.0 - 2.0, rng.unit(), rng.unit() - 0.5),
        );
        let shapes = (0..rng.below(3)).map(|_| rng.below(2) as u32).collect();
        specs.push((parent, local, shapes));
        path.push(index);
    }
    build(&specs, vec![prim(-1.0, 1.0), prim(0.0, 0.5)])
}

/// 部品の表の約束（行きがけ順・範囲・深さ・描画の連続・鍵の表）を確かめる。
fn check_invariants(doc: &Document) {
    let nodes = doc.nodes();
    let mut draw_cursor = 0;
    for (index, node) in nodes.iter().enumerate() {
        let id = NodeId(index as u32);
        assert!(node.subtree_end as usize > index && node.subtree_end as usize <= nodes.len());
        assert_eq!(doc.id_of(node.key), Some(id), "鍵の表");
        assert_eq!(node.draws.start, draw_cursor, "描画の連続");
        draw_cursor = node.draws.end;
        match node.parent {
            Some(parent) => {
                assert!((parent.0 as usize) < index);
                let parent_node = &nodes[parent.0 as usize];
                assert!(parent_node.subtree_end >= node.subtree_end);
                assert_eq!(node.depth, parent_node.depth + 1);
                // 親から見て、自分の前の兄弟の範囲は自分の手前で終わる。
                assert!(index as u32 > parent.0);
            }
            None => assert_eq!(node.depth, 0),
        }
        // 範囲の中の部品の親は、範囲の中にある。
        for child in &nodes[index + 1..node.subtree_end as usize] {
            let parent = child.parent.expect("範囲の中は子孫").0 as usize;
            assert!(parent >= index && parent < node.subtree_end as usize);
        }
    }
    let expected = nodes
        .iter()
        .fold(Aabb::empty(), |bounds, node| bounds.union(node.world_aabb));
    assert_eq!(format!("{:?}", doc.bounds()), format!("{expected:?}"));
}

/// 文書の中身をビット単位で比べるための文字列（`{:?}` の浮動小数は往復できる最短の表記）。
fn snapshot(doc: &Document) -> String {
    let mut out = String::new();
    for (index, node) in doc.nodes().iter().enumerate() {
        let id = NodeId(index as u32);
        out.push_str(&format!(
            "{node:?} {:?} {:?}\n",
            doc.draws(id),
            doc.file_index(id)
        ));
    }
    out.push_str(&format!("{:?}", doc.bounds()));
    out
}

fn bits(m: &Affine3A) -> Vec<u32> {
    m.to_cols_array().iter().map(|v| v.to_bits()).collect()
}

#[test]
fn a_new_document_reads_like_the_model() {
    let model = Arc::new(family());
    let doc = Document::new(Arc::clone(&model));
    assert_eq!(doc.node_count(), model.node_count());
    assert_eq!(doc.primitive_count(), model.primitive_count());
    assert_eq!(doc.bounds(), model.bounds());
    assert!(doc.shares_geometry_with(&model));
    for index in 0..model.nodes.len() {
        let id = NodeId(index as u32);
        assert_eq!(doc.key(id), Some(NodeKey(index as u64)));
        assert_eq!(doc.parent(id), model.parent(id));
        assert_eq!(doc.subtree_end(id), model.subtree_end(id));
        assert_eq!(doc.depth(id), model.depth(id));
        assert_eq!(doc.name(id), model.name(id));
        assert_eq!(bits(doc.world(id)), bits(model.world(id)));
        assert_eq!(doc.world_aabb(id), model.world_aabb(id));
        assert_eq!(doc.draws(id), model.draws(id));
        assert_eq!(doc.file_index(id), model.file_index(id));
        assert_eq!(doc.find_file_index(index as u32 * 3 + 1), Some(id));
        assert_eq!(doc.node(id).unwrap().local, model.nodes[index].local);
        assert_eq!(doc.subtree_bounds(id), model.subtree_bounds(id));
    }
    assert_eq!(doc.find_file_index(2), None);
    assert!(doc.node(NodeId(99)).is_none());
    check_invariants(&doc);
}

#[test]
fn moving_a_part_moves_its_descendants_and_nothing_else() {
    let model = Arc::new(family());
    let mut doc = Document::new(Arc::clone(&model));
    let before = snapshot(&doc);
    let moved = Affine3A::from_rotation_x(0.7) * shift(3.0);
    let range = doc.set_local(NodeKey(1), moved).unwrap();
    assert_eq!(range, 1..4);
    for index in 0..doc.node_count() {
        let id = NodeId(index as u32);
        let node = doc.node(id).unwrap();
        if range.contains(&(index as u32)) {
            let parent_world = *doc.world(node.parent.unwrap());
            assert_eq!(bits(doc.world(id)), bits(&(parent_world * node.local)));
            let mut aabb = Aabb::empty();
            for draw in doc.draws(id) {
                let shape = doc.primitive(draw.prim).unwrap();
                aabb = aabb.union(shape.local_aabb.transformed(doc.world(id)));
            }
            assert_eq!(doc.world_aabb(id), aabb);
        } else {
            assert_eq!(
                bits(doc.world(id)),
                bits(model.world(id)),
                "触れていない部品"
            );
            assert_eq!(doc.world_aabb(id), model.world_aabb(id));
        }
    }
    assert_ne!(doc.world(NodeId(3)), model.world(NodeId(3)));
    check_invariants(&doc);

    let mut broken = Affine3A::IDENTITY;
    broken.translation.x = f32::NAN;
    let moved_snapshot = snapshot(&doc);
    assert_eq!(doc.set_local(NodeKey(1), broken), Err(DocError::NonFinite));
    assert_eq!(
        doc.set_local(NodeKey(99), Affine3A::IDENTITY),
        Err(DocError::UnknownKey(NodeKey(99)))
    );
    assert_eq!(snapshot(&doc), moved_snapshot);

    doc.set_local(NodeKey(1), model.nodes[1].local).unwrap();
    assert_eq!(snapshot(&doc), before, "元の行列へ戻すとビット単位で戻る");
}

/// 構造の変更の前後で、`Remap` が全部の古い番号を、同じ鍵の新しい番号へ移すこと。
fn check_remap(before: &[NodeKey], doc: &Document, remap: Remap) {
    for (old, key) in before.iter().enumerate() {
        assert_eq!(remap.map(NodeId(old as u32)), doc.id_of(*key), "番号 {old}");
    }
}

fn keys(doc: &Document) -> Vec<NodeKey> {
    doc.nodes().iter().map(|node| node.key).collect()
}

#[test]
fn removing_and_inserting_back_restores_the_document() {
    let model = Arc::new(family());
    let mut doc = Document::new(model);
    let before = snapshot(&doc);
    let old_keys = keys(&doc);

    let removed = doc.remove_subtree(NodeKey(1)).unwrap();
    assert_eq!(removed.remap, Remap::Removed { at: 1, len: 3 });
    assert_eq!(
        removed.place,
        Place {
            parent: Some(NodeKey(0)),
            next_sibling: Some(NodeKey(4)),
        }
    );
    assert_eq!(removed.piece.nodes.len(), 3);
    assert_eq!(removed.piece.nodes[0].parent, None);
    assert_eq!(removed.piece.nodes[2].parent, Some(NodeId(0)));
    assert_eq!(removed.piece.draws.len(), 3);
    assert!(removed.piece.bytes() > 0);
    check_invariants(&doc);
    check_remap(&old_keys, &doc, removed.remap);
    assert_eq!(doc.subtree_end(NodeId(0)), 4);
    assert_eq!(doc.id_of(NodeKey(2)), None);
    assert_eq!(doc.find_file_index(2 * 3 + 1), None);
    assert_eq!(doc.find_file_index(4 * 3 + 1), Some(NodeId(1)));

    let middle_keys = keys(&doc);
    let remap = doc.insert_subtree(removed.place, removed.piece).unwrap();
    assert_eq!(remap, Remap::Inserted { at: 1, len: 3 });
    check_remap(&middle_keys, &doc, remap);
    check_invariants(&doc);
    assert_eq!(snapshot(&doc), before);

    // 末尾の根、最後の子、根の並びの先頭も同じように往復する。
    for key in [7, 6, 4, 0] {
        let removed = doc.remove_subtree(NodeKey(key)).unwrap();
        check_invariants(&doc);
        doc.insert_subtree(removed.place, removed.piece).unwrap();
        assert_eq!(snapshot(&doc), before, "鍵 {key}");
    }
}

#[test]
fn bad_places_and_pieces_are_refused_without_changes() {
    let mut doc = Document::new(Arc::new(family()));
    let removed = doc.remove_subtree(NodeKey(1)).unwrap();
    let before = snapshot(&doc);
    let missing_parent = Place {
        parent: Some(NodeKey(2)),
        next_sibling: None,
    };
    assert_eq!(
        doc.insert_subtree(missing_parent, removed.piece.clone()),
        Err(DocError::InvalidPlace)
    );
    let wrong_sibling = Place {
        parent: Some(NodeKey(0)),
        next_sibling: Some(NodeKey(7)),
    };
    assert_eq!(
        doc.insert_subtree(wrong_sibling, removed.piece.clone()),
        Err(DocError::InvalidPlace)
    );
    let mut taken_key = removed.piece.clone();
    taken_key.nodes[1].key = NodeKey(0);
    assert_eq!(
        doc.insert_subtree(removed.place, taken_key),
        Err(DocError::InvalidPiece)
    );
    let mut broken = removed.piece.clone();
    broken.nodes[0].subtree_end = 9;
    assert_eq!(
        doc.insert_subtree(removed.place, broken),
        Err(DocError::InvalidPiece)
    );
    assert_eq!(
        doc.insert_subtree(removed.place, Subtree::default()),
        Err(DocError::InvalidPiece)
    );
    assert_eq!(snapshot(&doc), before);
    assert_eq!(
        doc.remove_subtree(NodeKey(2)).unwrap_err(),
        DocError::UnknownKey(NodeKey(2))
    );
}

#[test]
fn duplicating_places_a_copy_right_after_the_original() {
    let mut doc = Document::new(Arc::new(family()));
    let old_keys = keys(&doc);
    let (place, piece) = doc.duplicate_subtree(NodeKey(1)).unwrap();
    assert_eq!(
        place,
        Place {
            parent: Some(NodeKey(0)),
            next_sibling: Some(NodeKey(4)),
        }
    );
    assert_eq!(&*piece.nodes[0].name, "部品1 コピー");
    assert_eq!(&*piece.nodes[1].name, "部品2");
    assert_eq!(piece.nodes[0].template, Some(4));
    assert_eq!(piece.nodes[2].template, Some(10));
    let new_keys: Vec<NodeKey> = piece.nodes.iter().map(|node| node.key).collect();
    assert!(new_keys.iter().all(|key| key.0 >= 8));
    let remap = doc.insert_subtree(place, piece).unwrap();
    assert_eq!(remap, Remap::Inserted { at: 4, len: 3 });
    check_invariants(&doc);
    check_remap(&old_keys, &doc, remap);
    assert_eq!(doc.id_of(new_keys[0]), Some(NodeId(4)));
    assert_eq!(doc.subtree_end(NodeId(0)), 10);
    for offset in 0..3 {
        let original = NodeId(1 + offset);
        let copy = NodeId(4 + offset);
        assert_eq!(bits(doc.world(copy)), bits(doc.world(original)));
        assert_eq!(doc.draws(copy), doc.draws(original));
        assert_eq!(doc.file_index(copy), None, "まだファイルに無い");
    }

    // 複製の複製は、元のファイルの番号を写す。
    let (place, piece) = doc.duplicate_subtree(new_keys[0]).unwrap();
    assert_eq!(piece.nodes[0].template, Some(4));
    doc.insert_subtree(place, piece).unwrap();
    check_invariants(&doc);

    // 書き出しの後の番号を入れると、複製もファイルの番号を持つ。
    let assigned: Vec<u32> = (0..doc.node_count() as u32).map(|i| 100 + i).collect();
    doc.set_file_indices(&assigned);
    assert!(doc.nodes().iter().all(|node| node.template.is_none()));
    assert_eq!(doc.file_index(NodeId(4)), Some(104));
    assert_eq!(doc.find_file_index(104), Some(NodeId(4)));
    doc.set_file_indices(&[1, 2]);
    assert_eq!(
        doc.file_index(NodeId(4)),
        Some(104),
        "長さが違えば何もしない"
    );
}

#[test]
fn added_shapes_get_new_primitive_ids_and_go_to_the_end() {
    let model = Arc::new(family());
    let mut doc = Document::new(Arc::clone(&model));
    let id = doc.add_primitive(Arc::new(prim(-2.0, 2.0)));
    assert_eq!(id, PrimId(2));
    assert_eq!(doc.primitive_count(), 3);
    assert_eq!(doc.added_primitives().len(), 1);
    assert_eq!(doc.primitive(id).unwrap().local_aabb.max, Vec3::splat(2.0));
    let local = shift(10.0);
    let draw = Draw {
        prim: id,
        base_color: [1.0, 0.0, 0.0, 1.0],
        tex: None,
    };
    let (place, piece) = doc.shape_piece("立方体", local, draw).unwrap();
    assert_eq!(
        place,
        Place {
            parent: None,
            next_sibling: None,
        }
    );
    let key = piece.nodes[0].key;
    let remap = doc.insert_subtree(place, piece).unwrap();
    assert_eq!(remap, Remap::Inserted { at: 8, len: 1 });
    let new = doc.id_of(key).unwrap();
    assert_eq!(doc.name(new), "立方体");
    assert_eq!(doc.draws(new), &[draw]);
    assert_eq!(
        doc.world_aabb(new),
        prim(-2.0, 2.0).local_aabb.transformed(&local)
    );
    assert!(doc.bounds().max.x >= 12.0);
    check_invariants(&doc);

    let unknown = Draw {
        prim: PrimId(9),
        base_color: [1.0; 4],
        tex: None,
    };
    assert_eq!(
        doc.shape_piece("x", local, unknown).unwrap_err(),
        DocError::InvalidPiece
    );
    // 取り除いても形状は残る。
    doc.remove_subtree(key).unwrap();
    assert_eq!(doc.primitive_count(), 3);
    assert_eq!(model.primitives.len(), 2, "土台は変わらない");
}

#[test]
fn remap_moves_every_index() {
    let inserted = Remap::Inserted { at: 3, len: 2 };
    assert_eq!(inserted.map(NodeId(0)), Some(NodeId(0)));
    assert_eq!(inserted.map(NodeId(2)), Some(NodeId(2)));
    assert_eq!(inserted.map(NodeId(3)), Some(NodeId(5)));
    assert_eq!(inserted.map(NodeId(u32::MAX)), None);
    let removed = Remap::Removed { at: 3, len: 2 };
    assert_eq!(removed.map(NodeId(2)), Some(NodeId(2)));
    assert_eq!(removed.map(NodeId(3)), None);
    assert_eq!(removed.map(NodeId(4)), None);
    assert_eq!(removed.map(NodeId(5)), Some(NodeId(3)));
    assert_eq!(removed.map(NodeId(u32::MAX)), Some(NodeId(u32::MAX - 2)));
}

#[test]
fn random_structure_changes_keep_the_table_consistent() {
    let mut rng = Rng(0x5eed_1234_abcd);
    let model = Arc::new(random_model(&mut rng, 300));
    let mut doc = Document::new(model);
    let start = snapshot(&doc);
    let mut undo: Vec<(Place, Subtree, NodeKey)> = Vec::new();
    for _ in 0..200 {
        let before = keys(&doc);
        let pick = doc.nodes()[rng.below(doc.node_count())].key;
        match rng.below(3) {
            0 if doc.node_count() > 20 => {
                let removed = doc.remove_subtree(pick).unwrap();
                check_remap(&before, &doc, removed.remap);
                let root = removed.piece.nodes[0].key;
                undo.push((removed.place, removed.piece, root));
            }
            1 => {
                let (place, piece) = doc.duplicate_subtree(pick).unwrap();
                let root = piece.nodes[0].key;
                let remap = doc.insert_subtree(place, piece).unwrap();
                check_remap(&before, &doc, remap);
                // 取り消しは、入れた部分木を取り除くこと。
                undo.push((place, Subtree::default(), root));
            }
            _ => {
                let local = Affine3A::from_translation(Vec3::splat(rng.unit()));
                doc.set_local(pick, local).unwrap();
                continue;
            }
        }
        check_invariants(&doc);
    }
    // 構造の変更だけを逆順に戻す（変形は戻さないので、構造と鍵だけを比べる）。
    while let Some((place, piece, root)) = undo.pop() {
        if piece.nodes.is_empty() {
            doc.remove_subtree(root).unwrap();
        } else {
            doc.insert_subtree(place, piece).unwrap();
        }
        check_invariants(&doc);
    }
    let structure = |doc: &Document| -> Vec<(NodeKey, Option<NodeId>, u32, u32)> {
        doc.nodes()
            .iter()
            .map(|node| (node.key, node.parent, node.subtree_end, node.depth))
            .collect()
    };
    let fresh = Document::new(Arc::new(random_model(&mut Rng(0x5eed_1234_abcd), 300)));
    assert_eq!(structure(&doc), structure(&fresh));
    assert_ne!(snapshot(&doc), start, "変形は戻していない");
}

#[test]
fn adopting_file_locals_recomputes_like_loading_and_ignores_bad_input() {
    let model = Arc::new(family());
    let mut doc = Document::new(Arc::clone(&model));
    let locals: Vec<Affine3A> = doc.nodes().iter().map(|node| node.local).collect();
    assert!(!doc.adopt_file_locals(&locals), "同じ値なら何もしない");
    assert!(
        !doc.adopt_file_locals(&locals[1..]),
        "長さが違えば何もしない"
    );
    let mut bad = locals.clone();
    bad[0] = Affine3A::from_translation(Vec3::splat(f32::NAN));
    assert!(!doc.adopt_file_locals(&bad));
    let before = snapshot(&doc);

    // 根の最後の桁だけを変える。子の world も読み込みと同じ式で作り直される。
    let mut nudged = locals.clone();
    nudged[0] = Affine3A::from_translation(Vec3::new(1.0e-7, 0.0, 0.0)) * locals[0];
    assert!(doc.adopt_file_locals(&nudged));
    assert_ne!(snapshot(&doc), before);
    check_invariants(&doc);
    for (index, node) in doc.nodes().iter().enumerate() {
        let parent = node
            .parent
            .map_or(Affine3A::IDENTITY, |p| doc.nodes()[p.0 as usize].world);
        assert_eq!(bits(&node.world), bits(&(parent * nudged[index])));
    }
}

#[test]
fn shape_spec_survives_duplicate() {
    let model = Arc::new(family());
    let mut doc = Document::new(Arc::clone(&model));
    let spec = ShapeSpec {
        kind: ShapeKind::Cylinder,
        size: 0.5,
    };
    let plain = doc.add_primitive(Arc::new(prim(-1.0, 1.0)));
    let id = doc.add_shape_primitive(Arc::new(shapes::build(spec.kind, spec.size)), spec);
    assert_eq!(id, PrimId(3));
    assert_eq!(doc.shape_spec(id), Some(spec));
    assert_eq!(doc.shape_spec(plain), None, "記録の無い形状");
    assert_eq!(doc.shape_spec(PrimId(0)), None, "読み込んだ形状");
    assert_eq!(doc.shape_spec(PrimId(1)), None, "読み込んだ形状");
    assert_eq!(doc.shape_spec(PrimId(99)), None, "無い番号");

    let draw = Draw {
        prim: id,
        base_color: shapes::SHAPE_COLOR,
        tex: None,
    };
    let (place, piece) = doc.shape_piece("円柱", shift(3.0), draw).unwrap();
    let key = piece.nodes[0].key;
    doc.insert_subtree(place, piece).unwrap();
    let (place, piece) = doc.duplicate_subtree(key).unwrap();
    let copy_key = piece.nodes[0].key;
    doc.insert_subtree(place, piece).unwrap();
    check_invariants(&doc);
    let copy = doc.id_of(copy_key).unwrap();
    let copy_prim = doc.draws(copy)[0].prim;
    assert_eq!(copy_prim, id, "複製は同じ形状を指す");
    assert_eq!(doc.shape_spec(copy_prim), Some(spec));

    // 元を消しても、形状と記録は残る。
    doc.remove_subtree(key).unwrap();
    assert_eq!(doc.shape_spec(id), Some(spec));

    // 文書を作り直すと記録は消える。
    let fresh = Document::new(model);
    assert_eq!(fresh.shape_spec(id), None);
}

fn textured_family() -> Model {
    let plane = shapes::build(ShapeKind::Plane, 1.0);
    let uvs = vec![[0.25, 0.5]; plane.vertices.len()];
    let mut model = build(
        &[
            (None, Affine3A::IDENTITY, vec![0]),
            (Some(0), shift(1.0), vec![0, 0]),
            (None, shift(2.0), vec![0]),
        ],
        vec![plane],
    );
    model.textures.images.push(ImageInfo {
        hash: ImageHash::of(b"base"),
        mime: Mime::Png,
        source: ImageSource::File { offset: 8, len: 4 },
    });
    model.textures.uses.push(TexUse {
        image: Some(ImageId(0)),
        sampling: Sampling::default(),
        map: UvMap::Vertex {
            transform: IDENTITY_TRANSFORM,
            tex_coord: 1,
            info_from: Some(2),
        },
        cutoff: Some(0.375),
        baked_box: None,
    });
    model.textures.prim_uvs.push(uvs);
    model.draws[0].tex = Some(TexId(0));
    model.draws[1].base_color[3] = 0.25;
    model.draws[2].base_color[3] = 0.75;
    model
}

#[test]
fn add_image_dedups_by_hash() {
    let model = Arc::new(textured_family());
    let mut doc = Document::new(model.clone());
    assert_eq!(
        doc.add_image(Arc::from(&b"base"[..]), Mime::Png),
        ImageId(0)
    );
    assert_eq!(doc.image(ImageId(0)), model.image(ImageId(0)));
    assert_eq!(doc.held_image_bytes(), 0);

    let bytes: Arc<[u8]> = Arc::from(&b"pasted"[..]);
    let id = doc.add_image(bytes.clone(), Mime::Jpeg);
    assert_eq!(id, ImageId(1));
    assert_eq!(
        doc.try_add_image(Arc::from(&b"pasted"[..]), Mime::Jpeg),
        Ok(id)
    );
    let image = doc.image(id).unwrap();
    assert_eq!(image.hash, ImageHash::of(&bytes));
    assert_eq!(image.mime, Mime::Jpeg);
    assert!(matches!(&image.source, ImageSource::Held(held) if Arc::ptr_eq(held, &bytes)));
    assert_eq!(doc.images().count(), 2);
    assert!(doc.image(ImageId(2)).is_none());
    assert_eq!(doc.held_image_bytes(), bytes.len() as u64);
}

#[test]
fn set_looks_round_trips() {
    let model = Arc::new(textured_family());
    let mut doc = Document::new(model.clone());
    let tex = *doc.tex_use(TexId(0)).unwrap();
    assert_eq!(doc.add_tex_use(tex), Ok(TexId(0)));
    let mask = TexUse { image: None, ..tex };
    assert_eq!(doc.add_tex_use(mask), Ok(TexId(1)));
    assert_eq!(doc.add_tex_use(mask), Ok(TexId(1)));
    assert_eq!(doc.tex_use(TexId(1)), Some(&mask));
    assert_eq!(
        doc.add_tex_use(TexUse {
            image: Some(ImageId(1)),
            ..tex
        }),
        Err(DocError::InvalidPiece)
    );
    assert!(doc.tex_use(TexId(2)).is_none());
    let mut other_material = tex;
    let UvMap::Vertex { info_from, .. } = &mut other_material.map else {
        unreachable!()
    };
    *info_from = Some(3);
    assert_eq!(doc.add_tex_use(other_material), Ok(TexId(2)));
    assert_eq!(doc.tex_use(TexId(0)), Some(&tex));
    assert_eq!(doc.prim_uvs(PrimId(0)), model.prim_uvs(PrimId(0)));
    assert!(!doc.prim_uvs(PrimId(0)).is_empty());
    let added = doc.add_primitive(Arc::new(prim(0.0, 1.0)));
    assert!(doc.prim_uvs(added).is_empty());

    let key = NodeKey(0);
    let before = doc.subtree_looks(key).unwrap();
    let after = [
        DrawLook {
            base_color: [1.0; 4],
            tex: None,
        },
        DrawLook {
            base_color: [1.0, 1.0, 1.0, 0.25],
            tex: Some(TexId(0)),
        },
        DrawLook {
            base_color: [0.0, 0.0, 0.0, 0.75],
            tex: Some(TexId(1)),
        },
    ];
    assert_eq!(doc.set_subtree_looks(key, &after), Ok(0..2));
    assert_eq!(doc.subtree_looks(key).unwrap(), after);
    assert_eq!(doc.subtree_looks(NodeKey(1)).unwrap(), after[1..]);
    assert_eq!(doc.draws(NodeId(2)), model.draws(NodeId(2)));
    assert_eq!(doc.set_subtree_looks(key, &before), Ok(0..2));
    for id in [NodeId(0), NodeId(1)] {
        assert_eq!(doc.draws(id), model.draws(id));
    }

    assert_eq!(
        doc.set_subtree_looks(key, &after[1..]),
        Err(DocError::InvalidPiece)
    );
    let mut bad = after;
    bad[2].tex = Some(TexId(99));
    assert_eq!(
        doc.set_subtree_looks(key, &bad),
        Err(DocError::InvalidPiece)
    );
    let mut bad = after;
    bad[2].base_color[3] = f32::NAN;
    assert_eq!(doc.set_subtree_looks(key, &bad), Err(DocError::NonFinite));
    assert_eq!(doc.subtree_looks(key).unwrap(), before);
    let missing = NodeKey(99);
    assert_eq!(
        doc.subtree_looks(missing),
        Err(DocError::UnknownKey(missing))
    );
    assert_eq!(
        doc.set_subtree_looks(missing, &[]),
        Err(DocError::UnknownKey(missing))
    );
}

#[test]
fn duplicate_shares_image() {
    let mut doc = Document::new(Arc::new(textured_family()));
    let image = doc.add_image(Arc::from(&b"copy"[..]), Mime::Png);
    let tex = doc
        .add_tex_use(TexUse {
            image: Some(image),
            ..*doc.tex_use(TexId(0)).unwrap()
        })
        .unwrap();
    let key = NodeKey(0);
    let mut pasted = doc.subtree_looks(key).unwrap();
    for look in &mut pasted {
        look.tex = Some(tex);
    }
    doc.set_subtree_looks(key, &pasted).unwrap();
    let (place, piece) = doc.duplicate_subtree(key).unwrap();
    let copy = piece.root_key().unwrap();
    doc.insert_subtree(place, piece).unwrap();
    assert_eq!(doc.subtree_looks(copy).unwrap(), pasted);
    assert_eq!(doc.tex_use(tex).unwrap().image, Some(image));
    assert_eq!(doc.images().count(), 2);
    assert_eq!(doc.held_image_bytes(), 4);
    let mut detached = pasted.clone();
    detached[0].tex = None;
    doc.set_subtree_looks(copy, &detached).unwrap();
    assert_eq!(doc.subtree_looks(key).unwrap(), pasted);

    let (place, mut bad) = doc.duplicate_subtree(key).unwrap();
    bad.draws[0].tex = Some(TexId(99));
    assert_eq!(
        doc.shape_piece("図形", Affine3A::IDENTITY, bad.draws[0]),
        Err(DocError::InvalidPiece)
    );
    assert_eq!(doc.insert_subtree(place, bad), Err(DocError::InvalidPiece));
}

#[test]
fn untextured_document_reads_as_before() {
    let model = Arc::new(family());
    let doc = Document::new(model.clone());
    assert_eq!(doc.images().count(), 0);
    assert_eq!(doc.tex_uses().count(), 0);
    assert_eq!(doc.held_image_bytes(), 0);
    assert!(doc.image(ImageId(0)).is_none());
    assert!(doc.tex_use(TexId(0)).is_none());
    for (index, node) in doc.nodes().iter().enumerate() {
        let id = NodeId(index as u32);
        assert_eq!(doc.draws(id), model.draws(id));
        let expected: Vec<_> = model
            .subtree(id)
            .flat_map(|n| model.draws(NodeId(n)))
            .map(|draw| DrawLook {
                base_color: draw.base_color,
                tex: None,
            })
            .collect();
        assert_eq!(doc.subtree_looks(node.key).unwrap(), expected);
    }
    for p in 0..doc.primitive_count() {
        assert!(doc.prim_uvs(PrimId(p as u32)).is_empty());
    }
}
