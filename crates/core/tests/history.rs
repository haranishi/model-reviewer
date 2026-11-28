//! 履歴: 取り消し→やり直しで文書と `ReviewState` がビット単位で戻ること、番号の付け替え、
//! 上限、版の番号。部品10万個での時間の測定（`#[ignore]`、release で実行する）。

use std::sync::Arc;
use std::time::{Duration, Instant};

use glam::{Affine3A, Quat, Vec3};

use model_reviewer_core::camera::OrbitCamera;
use model_reviewer_core::doc::{DocError, Document, DrawLook, NodeKey};
use model_reviewer_core::history::{Edit, History, KeyedNote, Pushed, Step};
use model_reviewer_core::math::Aabb;
use model_reviewer_core::pick::Hit;
use model_reviewer_core::review::ReviewState;
use model_reviewer_core::scene::{
    Draw, Model, ModelStats, Node, NodeId, PrimId, Primitive, SceneRead,
};
use model_reviewer_core::texture::{
    ImageHash, ImageId, Mime, Sampling, TexId, TexUse, Textures, UvMap,
};

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
            gltf_index: index as u32,
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

/// 0 根(7) ├1 腕(4) │├2 手 │└3 指 ├4 車輪 └5 胴(7) │└6 首  7 二本目の根
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
        let local = Affine3A::from_scale_rotation_translation(
            Vec3::splat(0.5 + rng.unit()),
            Quat::from_rotation_z(rng.unit() * 3.0),
            Vec3::new(rng.unit() * 4.0 - 2.0, rng.unit(), rng.unit() - 0.5),
        );
        let shapes = (0..rng.below(3)).map(|_| rng.below(2) as u32).collect();
        specs.push((path.last().copied(), local, shapes));
        path.push(index);
    }
    build(&specs, vec![prim(-1.0, 1.0), prim(0.0, 0.5)])
}

/// 文書とレビューの中身をビット単位で比べるための文字列（`{:?}` の浮動小数は往復できる最短の表記）。
fn snapshot(doc: &Document, review: &ReviewState) -> String {
    let mut out = String::new();
    for (index, node) in doc.nodes().iter().enumerate() {
        let id = NodeId(index as u32);
        out.push_str(&format!(
            "{node:?} {:?} {:?}\n",
            doc.draws(id),
            doc.file_index(id)
        ));
    }
    out.push_str(&format!("{:?}\n{review:?}", doc.bounds()));
    out
}

fn hit(node: NodeId, x: f32) -> Hit {
    Hit {
        node,
        t: 1.0,
        world_pos: Vec3::ZERO,
        local_pos: Vec3::new(x, 0.25, -0.5),
        local_normal: Vec3::Y,
        prim: model_reviewer_core::scene::PrimId(0),
        triangle: 0,
    }
}

struct World {
    doc: Document,
    review: ReviewState,
    history: History,
}

impl World {
    fn new(model: Model) -> Self {
        let doc = Document::new(Arc::new(model));
        let review = ReviewState::new(&doc);
        Self {
            doc,
            review,
            history: History::default(),
        }
    }

    fn snapshot(&self) -> String {
        snapshot(&self.doc, &self.review)
    }

    fn execute(&mut self, label: &'static str, edits: Vec<Edit>) {
        let (_, pushed) = self
            .history
            .execute(
                Step::new(label, edits, None),
                &mut self.doc,
                &mut self.review,
            )
            .unwrap();
        assert_eq!(pushed, Pushed::Kept);
    }

    fn undo(&mut self) {
        self.history
            .undo(&mut self.doc, &mut self.review)
            .expect("取り消せる");
    }

    fn redo(&mut self) {
        self.history
            .redo(&mut self.doc, &mut self.review)
            .expect("やり直せる");
    }

    fn key(&self, id: u32) -> NodeKey {
        self.doc.key(NodeId(id)).unwrap()
    }

    /// 注釈を足し（自分で実行して `push` する経路）、番号を返す。
    fn add_note(&mut self, id: u32, text: &str) -> u64 {
        let note_id = self
            .review
            .add_annotation(&hit(NodeId(id), id as f32), text.to_owned())
            .unwrap();
        let note = self.review.annotations.last().unwrap().clone();
        let key = self.key(id);
        self.history.push(Step::new(
            "注釈を足す",
            vec![Edit::AddNote(KeyedNote { node: key, note })],
            None,
        ));
        note_id
    }

    fn add_view(&mut self, name: &str) -> u64 {
        let id = self
            .review
            .add_view(name.to_owned(), OrbitCamera::default())
            .unwrap();
        let view = self.review.views.last().unwrap().clone();
        self.history
            .push(Step::new("視点を足す", vec![Edit::AddView(view)], None));
        id
    }

    /// 無作為な操作を1つ行って積む。
    fn random_op(&mut self, rng: &mut Rng) {
        let count = self.doc.node_count();
        let id = rng.below(count) as u32;
        let key = self.key(id);
        match rng.below(12) {
            0 | 1 => {
                let before = self.doc.node(NodeId(id)).unwrap().local;
                let after = Affine3A::from_rotation_x(rng.unit()) * shift(rng.unit());
                self.execute("移動", vec![Edit::SetLocal { key, before, after }]);
            }
            2 if count > 10 => {
                let edit = Edit::remove_request(&self.doc, key).unwrap();
                self.execute("削除", vec![edit]);
            }
            3 => {
                let (at, piece) = self.doc.duplicate_subtree(key).unwrap();
                self.execute("複製", vec![Edit::Insert { at, piece }]);
            }
            4 => {
                let prim = self.doc.add_primitive(Arc::new(prim(-0.5, 0.5)));
                let draw = Draw {
                    prim,
                    base_color: [0.2, 0.4, 0.6, 1.0],
                    tex: None,
                };
                let (at, piece) = self
                    .doc
                    .shape_piece("立方体", shift(rng.unit()), draw)
                    .unwrap();
                self.execute("図形を足す", vec![Edit::Insert { at, piece }]);
            }
            5 | 6 => {
                let before = self.review.visibility.own(NodeId(id));
                self.execute(
                    "表示",
                    vec![Edit::SetVisible {
                        key,
                        before,
                        after: !before,
                    }],
                );
            }
            7 => {
                let hidden: Vec<NodeKey> = self
                    .review
                    .visibility
                    .hidden_own()
                    .map(|id| self.doc.key(id).unwrap())
                    .collect();
                if !hidden.is_empty() {
                    self.execute("すべて表示", vec![Edit::ShowAll { hidden }]);
                }
            }
            8 => {
                self.add_note(id, &format!("注釈{}", rng.next() % 100));
            }
            9 if !self.review.annotations.is_empty() => {
                let index = rng.below(self.review.annotations.len());
                let note = self.review.annotations[index].clone();
                if rng.below(2) == 0 {
                    self.execute(
                        "注釈を直す",
                        vec![Edit::EditNote {
                            id: note.id,
                            before: note.text.clone(),
                            after: format!("{}+", note.text),
                        }],
                    );
                } else {
                    let node = self.doc.key(note.node).unwrap();
                    self.execute(
                        "注釈を消す",
                        vec![Edit::RemoveNote {
                            index,
                            note: KeyedNote { node, note },
                        }],
                    );
                }
            }
            10 => {
                self.add_view("視点");
            }
            11 if !self.review.views.is_empty() => {
                let index = rng.below(self.review.views.len());
                let view = self.review.views[index].clone();
                self.execute("視点を消す", vec![Edit::RemoveView { index, view }]);
            }
            _ => {
                let before = self.doc.node(NodeId(id)).unwrap().local;
                self.execute(
                    "移動",
                    vec![Edit::SetLocal {
                        key,
                        before,
                        after: shift(2.0) * before,
                    }],
                );
            }
        }
    }
}

/// 実行→取り消し→やり直し→取り消しで、前後がビット単位で一致すること。
fn roundtrip(world: &mut World, label: &'static str, edits: Vec<Edit>) {
    let before = world.snapshot();
    world.execute(label, edits);
    let after = world.snapshot();
    assert_ne!(before, after, "{label} が何かを変える");
    world.undo();
    assert_eq!(world.snapshot(), before, "{label} の取り消し");
    world.redo();
    assert_eq!(world.snapshot(), after, "{label} のやり直し");
    world.undo();
    assert_eq!(world.snapshot(), before, "{label} の2回目の取り消し");
    world.redo();
}

#[test]
fn every_operation_undoes_and_redoes_bit_exactly() {
    let mut world = World::new(family());
    let note_a = world.add_note(2, "手の傷");
    let note_b = world.add_note(5, "胴");
    world.add_note(0, "根");
    let view = world.add_view("正面");
    world.add_view("上");
    let hidden = world.key(3);
    world.execute(
        "表示",
        vec![Edit::SetVisible {
            key: hidden,
            before: true,
            after: false,
        }],
    );

    let key = world.key(1);
    let before = world.doc.node(NodeId(1)).unwrap().local;
    let after = Affine3A::from_rotation_z(0.4) * shift(0.7);
    roundtrip(
        &mut world,
        "移動",
        vec![Edit::SetLocal { key, before, after }],
    );

    let key = world.key(4);
    roundtrip(
        &mut world,
        "表示",
        vec![Edit::SetVisible {
            key,
            before: true,
            after: false,
        }],
    );
    let hidden_keys: Vec<NodeKey> = world
        .review
        .visibility
        .hidden_own()
        .map(|id| world.doc.key(id).unwrap())
        .collect();
    roundtrip(
        &mut world,
        "すべて表示",
        vec![Edit::ShowAll {
            hidden: hidden_keys,
        }],
    );
    roundtrip(
        &mut world,
        "注釈を直す",
        vec![Edit::EditNote {
            id: note_b,
            before: "胴".to_owned(),
            after: "胴の継ぎ目".to_owned(),
        }],
    );
    let index = world
        .review
        .annotations
        .iter()
        .position(|note| note.id == note_a)
        .unwrap();
    let note = world.review.annotations[index].clone();
    let node = world.doc.key(note.node).unwrap();
    roundtrip(
        &mut world,
        "注釈を消す",
        vec![Edit::RemoveNote {
            index,
            note: KeyedNote { node, note },
        }],
    );
    let saved = world.review.views[0].clone();
    assert_eq!(saved.id, view);
    roundtrip(
        &mut world,
        "視点を消す",
        vec![Edit::RemoveView {
            index: 0,
            view: saved,
        }],
    );

    // 削除（注釈と隠した部品を含む部分木）。
    let edit = Edit::remove_request(&world.doc, world.key(1)).unwrap();
    roundtrip(&mut world, "削除", vec![edit]);

    // 削除をやり直した後なので、番号ではなく鍵で指す。
    let (at, piece) = world.doc.duplicate_subtree(NodeKey(5)).unwrap();
    roundtrip(&mut world, "複製", vec![Edit::Insert { at, piece }]);

    let prim = world.doc.add_primitive(Arc::new(prim(-0.5, 0.5)));
    let draw = Draw {
        prim,
        base_color: [1.0, 0.0, 0.0, 1.0],
        tex: None,
    };
    let (at, piece) = world.doc.shape_piece("球", shift(3.0), draw).unwrap();
    roundtrip(&mut world, "図形を足す", vec![Edit::Insert { at, piece }]);

    // 自分で実行して積んだ追加（注釈・視点）も、取り消すと番号の控えまで戻る。
    let before = world.snapshot();
    world.add_note(4, "車輪");
    let after = world.snapshot();
    world.undo();
    assert_eq!(world.snapshot(), before, "注釈の追加の取り消し");
    world.redo();
    assert_eq!(world.snapshot(), after, "注釈の追加のやり直し");
    let before = world.snapshot();
    world.add_view("右");
    world.undo();
    assert_eq!(world.snapshot(), before, "視点の追加の取り消し");
}

#[test]
fn random_sequences_undo_back_to_the_start() {
    for seed in [0x1234_5678_9abc_def1_u64, 42, 7_777_777] {
        let mut rng = Rng(seed);
        let mut world = World::new(random_model(&mut rng, 120));
        let start = world.snapshot();
        let mut ops = 0;
        for _ in 0..300 {
            if world.history.can_undo() && rng.below(6) == 0 {
                world.undo();
                if rng.below(2) == 0 {
                    world.redo();
                }
            } else {
                world.random_op(&mut rng);
                ops += 1;
            }
        }
        assert!(ops > 200);
        let end = world.snapshot();
        let mut undone = 0;
        while world.history.can_undo() {
            world.undo();
            undone += 1;
        }
        assert_eq!(
            world.snapshot(),
            start,
            "種 {seed}: 全部取り消すと最初に戻る"
        );
        for _ in 0..undone {
            world.redo();
        }
        assert_eq!(world.snapshot(), end, "種 {seed}: 全部やり直すと最後に戻る");
    }
}

#[test]
fn everyone_holding_ids_points_at_the_same_parts_after_remove_and_undo() {
    let mut world = World::new(family());
    for id in [0, 2, 3, 5, 6, 7] {
        world.add_note(id, &format!("注釈{id}"));
    }
    for id in [3, 6] {
        let key = world.key(id);
        world.execute(
            "表示",
            vec![Edit::SetVisible {
                key,
                before: true,
                after: false,
            }],
        );
    }
    let note_keys: Vec<(u64, NodeKey)> = world
        .review
        .annotations
        .iter()
        .map(|note| (note.id, world.doc.key(note.node).unwrap()))
        .collect();
    let own_by_key: Vec<(NodeKey, bool, bool)> = (0..world.doc.node_count() as u32)
        .map(|id| {
            (
                world.key(id),
                world.review.visibility.own(NodeId(id)),
                world.review.visibility.effective(NodeId(id)),
            )
        })
        .collect();
    // 画面の側の持ち主（選択など）は、`Applied.remaps` で付け替える。
    let mut selection = [Some(NodeId(6)), Some(NodeId(2)), Some(NodeId(0))];
    let selection_keys: Vec<NodeKey> = selection
        .iter()
        .map(|id| world.key(id.unwrap().0))
        .collect();

    let edit = Edit::remove_request(&world.doc, world.key(1)).unwrap();
    let (applied, _) = world
        .history
        .execute(
            Step::new("削除", vec![edit], None),
            &mut world.doc,
            &mut world.review,
        )
        .unwrap();
    assert!(applied.structure);
    for remap in &applied.remaps {
        for held in &mut selection {
            *held = held.and_then(|id| remap.map(id));
        }
    }
    assert_eq!(selection, [Some(NodeId(3)), None, Some(NodeId(0))]);
    assert_eq!(
        world.review.annotations.len(),
        4,
        "部品と一緒に注釈も消える"
    );
    for note in &world.review.annotations {
        let (_, key) = note_keys.iter().find(|(id, _)| *id == note.id).unwrap();
        assert_eq!(world.doc.key(note.node), Some(*key), "残った注釈の付け替え");
    }

    let applied = world
        .history
        .undo(&mut world.doc, &mut world.review)
        .unwrap();
    for remap in &applied.remaps {
        for held in &mut selection {
            *held = held.and_then(|id| remap.map(id));
        }
    }
    assert_eq!(selection[0], Some(NodeId(6)));
    assert_eq!(
        world.doc.key(selection[0].unwrap()),
        Some(selection_keys[0])
    );
    assert_eq!(
        world.doc.key(selection[2].unwrap()),
        Some(selection_keys[2])
    );
    assert_eq!(world.review.annotations.len(), note_keys.len());
    for (note, (id, key)) in world.review.annotations.iter().zip(&note_keys) {
        assert_eq!(note.id, *id, "並びも戻る");
        assert_eq!(world.doc.key(note.node), Some(*key), "注釈 {id}");
    }
    for (index, (key, own, effective)) in own_by_key.iter().enumerate() {
        let id = NodeId(index as u32);
        assert_eq!(world.doc.key(id), Some(*key));
        assert_eq!(world.review.visibility.own(id), *own);
        assert_eq!(world.review.visibility.effective(id), *effective);
    }
}

#[test]
fn limits_drop_the_oldest_steps_and_the_rest_still_undo() {
    let mut world = World::new(family());
    world.history = History::with_limits(u64::MAX, 3);
    let key = world.key(4);
    let mut states = vec![world.snapshot()];
    for step in 0..5 {
        let before = world.doc.node(NodeId(4)).unwrap().local;
        world.execute(
            "移動",
            vec![Edit::SetLocal {
                key,
                before,
                after: shift(step as f32 + 1.0),
            }],
        );
        states.push(world.snapshot());
    }
    assert_eq!(world.history.len(), 3);
    for expected in states[2..5].iter().rev() {
        world.undo();
        assert_eq!(&world.snapshot(), expected);
    }
    assert!(!world.history.can_undo());
    assert!(
        world
            .history
            .undo(&mut world.doc, &mut world.review)
            .is_none()
    );

    // バイトの上限: 2回分だけ入る大きさにすると、3回目で最古が落ちる。
    let mut world = World::new(family());
    let one = Step::new(
        "移動",
        vec![Edit::SetLocal {
            key,
            before: Affine3A::IDENTITY,
            after: Affine3A::IDENTITY,
        }],
        None,
    )
    .bytes();
    world.history = History::with_limits(one * 2, 500);
    for step in 0..3 {
        let before = world.doc.node(NodeId(4)).unwrap().local;
        world.execute(
            "移動",
            vec![Edit::SetLocal {
                key,
                before,
                after: shift(step as f32),
            }],
        );
    }
    assert_eq!(world.history.len(), 2);
    assert_eq!(world.history.bytes(), one * 2);

    // 1回分だけで上限を超える操作は、実行したうえで履歴を空にする。
    world.history = History::with_limits(one * 2, 500);
    world.add_note(1, "x");
    let edit = Edit::remove_request(&world.doc, world.key(0)).unwrap();
    let (applied, pushed) = world
        .history
        .execute(
            Step::new("削除", vec![edit], None),
            &mut world.doc,
            &mut world.review,
        )
        .unwrap();
    assert_eq!(pushed, Pushed::ClearedTooLarge);
    assert!(applied.structure);
    assert_eq!(world.doc.node_count(), 1, "削除は実行された");
    assert!(!world.history.can_undo() && !world.history.can_redo());
    assert_eq!(world.history.bytes(), 0);
}

#[test]
fn revisions_track_document_and_review_separately() {
    let mut world = World::new(family());
    let saved_doc = world.history.doc_rev();
    let saved_review = world.history.review_rev();
    let key = world.key(1);
    world.execute(
        "移動",
        vec![Edit::SetLocal {
            key,
            before: world.doc.node(NodeId(1)).unwrap().local,
            after: shift(9.0),
        }],
    );
    let moved = world.history.doc_rev();
    assert_ne!(moved, saved_doc);
    assert_eq!(
        world.history.review_rev(),
        saved_review,
        "変形はレビューを変えない"
    );

    world.add_note(2, "注釈");
    assert_eq!(world.history.doc_rev(), moved, "注釈は文書を変えない");
    let noted = world.history.review_rev();
    assert_ne!(noted, saved_review);

    let edit = Edit::remove_request(&world.doc, world.key(4)).unwrap();
    world.execute("削除", vec![edit]);
    assert_ne!(world.history.doc_rev(), moved, "削除は両方を変える");
    assert_ne!(world.history.review_rev(), noted);

    world.undo();
    assert_eq!(
        (world.history.doc_rev(), world.history.review_rev()),
        (moved, noted)
    );
    world.undo();
    world.undo();
    assert_eq!(
        world.history.doc_rev(),
        saved_doc,
        "書き出した時点まで戻ると未保存が消える"
    );
    assert_eq!(world.history.review_rev(), saved_review);
    let applied = world
        .history
        .redo(&mut world.doc, &mut world.review)
        .unwrap();
    assert_eq!(applied.doc_rev, moved);
    assert_eq!(applied.label, "移動");
    assert_eq!(applied.touched, vec![1..4]);

    // 新しい操作の版は、取り消した版と取り違えない。
    let key = world.key(7);
    world.execute(
        "移動",
        vec![Edit::SetLocal {
            key,
            before: world.doc.node(NodeId(7)).unwrap().local,
            after: shift(1.5),
        }],
    );
    let fresh = world.history.doc_rev();
    assert!(fresh != moved && fresh != saved_doc);
    assert!(!world.history.can_redo());

    // 上限で捨てた版へは戻れない。
    world.history = History::with_limits(u64::MAX, 1);
    world.execute(
        "移動",
        vec![Edit::SetLocal {
            key,
            before: world.doc.node(NodeId(7)).unwrap().local,
            after: shift(2.5),
        }],
    );
    let saved = world.history.doc_rev();
    for x in [3.5, 4.5] {
        world.execute(
            "移動",
            vec![Edit::SetLocal {
                key,
                before: world.doc.node(NodeId(7)).unwrap().local,
                after: shift(x),
            }],
        );
    }
    while world.history.can_undo() {
        world.undo();
    }
    assert_ne!(world.history.doc_rev(), saved);
}

#[test]
fn a_refused_step_changes_nothing() {
    let mut world = World::new(family());
    world.add_note(1, "注釈");
    let before = world.snapshot();
    let len = world.history.len();
    let key = world.key(1);
    let local = world.doc.node(NodeId(1)).unwrap().local;
    let result = world.history.execute(
        Step::new(
            "移動",
            vec![
                Edit::SetLocal {
                    key,
                    before: local,
                    after: shift(5.0),
                },
                Edit::remove_request(&world.doc, world.key(0)).unwrap(),
                Edit::SetLocal {
                    key: NodeKey(999),
                    before: Affine3A::IDENTITY,
                    after: Affine3A::IDENTITY,
                },
            ],
            None,
        ),
        &mut world.doc,
        &mut world.review,
    );
    assert_eq!(result.unwrap_err(), DocError::UnknownKey(NodeKey(999)));
    assert_eq!(world.snapshot(), before);
    assert_eq!(world.history.len(), len);
    assert_eq!(
        world.history.push(Step::new("空", Vec::new(), None)),
        Pushed::Kept
    );
    assert_eq!(world.history.len(), len);
}

#[test]
fn a_transform_step_is_small() {
    let step = Step::new(
        "移動",
        vec![Edit::SetLocal {
            key: NodeKey(1),
            before: Affine3A::IDENTITY,
            after: Affine3A::IDENTITY,
        }],
        None,
    );
    let bytes = step.bytes();
    eprintln!(
        "変形1回の見積もり: {bytes} バイト（Edit {} + Step {}）",
        std::mem::size_of::<Edit>(),
        std::mem::size_of::<Step>()
    );
    // 行列は SIMD の並びで1つ64バイト。設計の目安（200バイト）を少し超える。
    assert!(bytes <= 256, "{bytes}");
}

// ---- 性能（release で `cargo test --release -p model-reviewer-core --test history -- --ignored --nocapture`）

type Specs = Vec<(Option<usize>, Affine3A, Vec<u32>)>;

/// 根の下に、子孫 `descendants` 個の組（10個ずつの小さな組の集まり）を足し、先頭の番号を返す。
fn group(specs: &mut Specs, descendants: usize) -> usize {
    let head = specs.len();
    specs.push((Some(0), shift(0.01 * head as f32), vec![0]));
    let mut left = descendants;
    while left > 0 {
        let sub = specs.len();
        specs.push((Some(head), shift(0.1), vec![0]));
        left -= 1;
        let leaves = 9.min(left);
        for leaf in 0..leaves {
            specs.push((Some(sub), shift(0.01 * leaf as f32), vec![0]));
        }
        left -= leaves;
    }
    head
}

/// 部品10万個。根の下に、子孫5万個のまとめ役・子孫9,999個のかたまり・残りの小さな組を置く。
fn heavy() -> Model {
    const TOTAL: usize = 100_000;
    let mut specs: Specs = Vec::with_capacity(TOTAL);
    specs.push((None, Affine3A::IDENTITY, vec![]));
    let aggregator = group(&mut specs, 50_000);
    let block = group(&mut specs, 9_999);
    while specs.len() < TOTAL {
        let left = TOTAL - specs.len() - 1;
        group(&mut specs, left.min(9));
    }
    assert_eq!(specs.len(), TOTAL);
    assert_eq!((aggregator, block), (1, 50_002));
    build(&specs, vec![prim(-0.5, 0.5)])
}

fn time<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let value = f();
    (value, start.elapsed())
}

#[test]
#[ignore = "時間の測定。release で実行する"]
fn heavy_document_timings() {
    let (model, build_time) = time(heavy);
    let model = Arc::new(model);
    let (doc, open_time) = time(|| Document::new(Arc::clone(&model)));
    let review = ReviewState::new(&doc);
    let mut world = World {
        doc,
        review,
        history: History::default(),
    };
    eprintln!("モデル作成 {build_time:?}、文書を開く {open_time:?}");
    let aggregator = world.key(1);
    assert_eq!(world.doc.subtree(NodeId(1)).len(), 50_001);
    let block = world.key(50_002);
    assert_eq!(world.doc.subtree(NodeId(50_002)).len(), 10_000);

    let before = world.doc.node(NodeId(1)).unwrap().local;
    let (_, commit) = time(|| {
        world.execute(
            "移動",
            vec![Edit::SetLocal {
                key: aggregator,
                before,
                after: shift(1.0) * before,
            }],
        )
    });
    let (_, commit_undo) = time(|| world.undo());
    eprintln!("変形の確定（子孫5万個） {commit:?}、取り消し {commit_undo:?}");

    let prim = world.doc.add_primitive(Arc::new(prim(-1.0, 1.0)));
    let draw = Draw {
        prim,
        base_color: [1.0; 4],
        tex: None,
    };
    let (_, add) = time(|| {
        let (at, piece) = world.doc.shape_piece("立方体", shift(2.0), draw).unwrap();
        world.execute("図形を足す", vec![Edit::Insert { at, piece }]);
    });
    let (_, add_undo) = time(|| world.undo());
    eprintln!("1個の追加 {add:?}、取り消し {add_undo:?}");

    let (_, duplicate) = time(|| {
        let (at, piece) = world.doc.duplicate_subtree(block).unwrap();
        world.execute("複製", vec![Edit::Insert { at, piece }]);
    });
    let (_, duplicate_undo) = time(|| world.undo());
    eprintln!("1万個の複製 {duplicate:?}、取り消し {duplicate_undo:?}");

    let (_, remove) = time(|| {
        let edit = Edit::remove_request(&world.doc, block).unwrap();
        world.execute("削除", vec![edit]);
    });
    let (_, remove_undo) = time(|| world.undo());
    let (_, remove_redo) = time(|| world.redo());
    eprintln!("1万個の削除 {remove:?}、取り消し {remove_undo:?}、やり直し {remove_redo:?}");

    world.undo();
    world.history.clear();
    let key = world.key(5);
    for step in 0..100 {
        let before = world.doc.node(NodeId(5)).unwrap().local;
        world.execute(
            "移動",
            vec![Edit::SetLocal {
                key,
                before,
                after: shift(step as f32),
            }],
        );
    }
    let transforms = world.history.bytes();
    let edit = Edit::remove_request(&world.doc, block).unwrap();
    world.execute("削除", vec![edit]);
    eprintln!(
        "履歴: 変形100回で {transforms} バイト、続けて1万個の削除で {} バイト（{} 件）",
        world.history.bytes(),
        world.history.len()
    );
}

#[test]
fn next_removed_roots_names_what_undo_or_redo_would_take_out() {
    let model = std::sync::Arc::new(family());
    let mut doc = Document::new(std::sync::Arc::clone(&model));
    let mut review = ReviewState::new(&model);
    let mut history = History::default();
    assert!(history.next_removed_roots(true).is_empty());
    let key = doc.key(NodeId(1)).unwrap();
    let (place, piece) = doc.duplicate_subtree(key).unwrap();
    let copy = piece.root_key().unwrap();
    history
        .execute(
            Step::new("複製", vec![Edit::Insert { at: place, piece }], Some(copy)),
            &mut doc,
            &mut review,
        )
        .unwrap();
    // 取り消すと複製が外れる。やり直しの列は空。
    assert_eq!(history.next_removed_roots(true), vec![copy]);
    assert!(history.next_removed_roots(false).is_empty());
    history.undo(&mut doc, &mut review).unwrap();
    // 取り消した挿入をやり直しても、何も外れない。
    assert!(history.next_removed_roots(false).is_empty());

    let request = Edit::remove_request(&doc, key).unwrap();
    history
        .execute(
            Step::new("削除", vec![request], Some(key)),
            &mut doc,
            &mut review,
        )
        .unwrap();
    assert!(
        history.next_removed_roots(true).is_empty(),
        "削除の取り消しは戻すだけ"
    );
    history.undo(&mut doc, &mut review).unwrap();
    assert_eq!(history.next_removed_roots(false), vec![key]);
}

fn box_use(image: Option<ImageId>, size: f32) -> TexUse {
    TexUse {
        image,
        map: UvMap::Box { size },
        sampling: Sampling::default(),
        cutoff: Some(0.375),
        baked_box: None,
    }
}

fn paste_edit(doc: &mut Document, key: NodeKey, bytes: &[u8]) -> Edit {
    let image = doc.add_image(Arc::from(bytes), Mime::Png);
    let tex = doc.add_tex_use(box_use(Some(image), 1.0)).unwrap();
    let before = doc.subtree_looks(key).unwrap();
    let after = before
        .iter()
        .map(|look| DrawLook {
            base_color: [1.0, 1.0, 1.0, look.base_color[3]],
            tex: Some(tex),
        })
        .collect();
    Edit::SetLooks { key, before, after }
}

#[test]
fn paste_undo_redo_restores_looks() {
    let mut model = family();
    model.draws[1].base_color[3] = 0.25;
    model.draws[2].base_color[3] = 0.75;
    model.textures.uses.push(box_use(None, 1.0));
    model.draws[2].tex = Some(TexId(0));
    let mut world = World::new(model);
    let key = world.key(1);
    let before = world.doc.subtree_looks(key).unwrap();
    let edit = paste_edit(&mut world.doc, key, b"pasted");
    assert!(edit.changes_document());
    assert!(!edit.changes_review());
    assert_eq!(edit.bytes(), std::mem::size_of::<Edit>() as u64 + 6 * 24);
    let (applied, pushed) = world
        .history
        .execute(
            Step::new("画像の貼り付け", vec![edit], Some(key)),
            &mut world.doc,
            &mut world.review,
        )
        .unwrap();
    assert_eq!(pushed, Pushed::Kept);
    assert_eq!(world.history.len(), 1);
    assert!(applied.retextured);
    assert!(applied.recolored.is_empty());
    assert!(!applied.structure);
    assert!(applied.doc_rev > 0);
    assert_eq!(applied.review_rev, 0);
    let pasted = world.doc.subtree_looks(key).unwrap();
    for (look, alpha) in pasted.iter().zip([0.25, 0.75, 1.0]) {
        assert_eq!(look.base_color, [1.0, 1.0, 1.0, alpha]);
        assert_eq!(look.tex, Some(TexId(1)));
    }
    let undone = world
        .history
        .undo(&mut world.doc, &mut world.review)
        .unwrap();
    assert!(undone.retextured);
    assert_eq!((undone.doc_rev, undone.review_rev), (0, 0));
    assert_eq!(world.doc.subtree_looks(key).unwrap(), before);
    assert_eq!(world.doc.tex_use(TexId(0)), Some(&box_use(None, 1.0)));

    let stale = Edit::SetLooks {
        key,
        before: pasted.clone(),
        after: before.clone(),
    };
    assert_eq!(
        world.history.execute(
            Step::new("画像を外す", vec![stale], None),
            &mut world.doc,
            &mut world.review
        ),
        Err(DocError::InvalidPiece)
    );
    let redone = world
        .history
        .redo(&mut world.doc, &mut world.review)
        .unwrap();
    assert!(redone.retextured);
    assert_eq!((redone.doc_rev, redone.review_rev), (applied.doc_rev, 0));
    assert_eq!(world.doc.subtree_looks(key).unwrap(), pasted);

    let mut tinted = pasted.clone();
    tinted[0].base_color[0] = 0.5;
    let color = Edit::SetLooks {
        key,
        before: pasted.clone(),
        after: tinted,
    };
    let (changed, _) = world
        .history
        .execute(
            Step::new("色の変更", vec![color], None),
            &mut world.doc,
            &mut world.review,
        )
        .unwrap();
    assert!(!changed.retextured);
    assert_eq!(changed.recolored, vec![1..4]);
    world.undo();
    assert_eq!(world.doc.subtree_looks(key).unwrap(), pasted);
}

#[test]
fn removed_part_comes_back_with_texture() {
    let mut world = World::new(family());
    let key = world.key(1);
    let paste = paste_edit(&mut world.doc, key, b"part");
    world.execute("画像の貼り付け", vec![paste]);
    let before = world.snapshot();
    let remove = Edit::remove_request(&world.doc, key).unwrap();
    world.execute("削除", vec![remove]);
    assert!(world.doc.id_of(key).is_none());
    assert_eq!(world.doc.held_image_bytes(), 4);
    world.undo();
    assert_eq!(world.snapshot(), before);
    assert_eq!(world.doc.tex_use(TexId(0)).unwrap().image, Some(ImageId(0)));
    assert_eq!(
        world.doc.image(ImageId(0)).unwrap().hash,
        ImageHash::of(b"part")
    );
}

#[test]
fn tables_never_shrink() {
    let mut world = World::new(family());
    let key = world.key(4);
    let first = paste_edit(&mut world.doc, key, b"first");
    world.execute("画像の貼り付け", vec![first]);
    world.undo();
    let second = paste_edit(&mut world.doc, key, b"second");
    world.execute("画像の貼り付け", vec![second]);
    assert!(!world.history.can_redo());

    let before = world.doc.subtree_looks(key).unwrap();
    assert_eq!(before[0].tex, Some(TexId(1)));
    let resized = box_use(Some(ImageId(1)), 2.0);
    let id = world.doc.add_tex_use(resized).unwrap();
    assert_eq!(id, TexId(2));
    let after = vec![DrawLook {
        tex: Some(id),
        ..before[0]
    }];
    let resize = Edit::SetLooks {
        key,
        before: before.clone(),
        after: after.clone(),
    };
    let (resized_edit, _) = world
        .history
        .execute(
            Step::new("画像の大きさ", vec![resize], None),
            &mut world.doc,
            &mut world.review,
        )
        .unwrap();
    assert!(resized_edit.retextured, "同じ画像でも貼り方の番号が変わる");
    let remove = Edit::SetLooks {
        key,
        before: after.clone(),
        after: vec![DrawLook {
            tex: None,
            ..after[0]
        }],
    };
    world.execute("画像を外す", vec![remove]);
    assert_eq!(
        world.doc.subtree_looks(key).unwrap()[0],
        DrawLook {
            tex: None,
            ..after[0]
        }
    );
    world.undo();
    assert_eq!(world.doc.subtree_looks(key).unwrap(), after);
    world.undo();
    assert_eq!(world.doc.subtree_looks(key).unwrap(), before);
    world.history.clear();
    assert_eq!(
        world.doc.tex_uses().copied().collect::<Vec<_>>(),
        [
            box_use(Some(ImageId(0)), 1.0),
            box_use(Some(ImageId(1)), 1.0),
            resized,
        ]
    );
    assert_eq!(
        world.doc.images().map(|i| i.hash).collect::<Vec<_>>(),
        [ImageHash::of(b"first"), ImageHash::of(b"second"),]
    );
    assert_eq!(world.doc.held_image_bytes(), 11);
}
