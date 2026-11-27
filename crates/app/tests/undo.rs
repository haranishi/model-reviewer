//! 変形・数値・取り消し・やり直しを `Command` の列で確かめる。GPUと画面は使わない。

use std::path::PathBuf;
use std::sync::Arc;

use glam::{Affine3A, Vec2, Vec3};
use model_reviewer::command::{Command, Handle, Tool, Trs};
use model_reviewer::loader::LoadPhase;
use model_reviewer::session::{GIZMO_SIZE, Session, gizmo_frame};
use model_reviewer::ui::gizmo_view::grab_points;
use model_reviewer_core::gizmo::decompose;
use model_reviewer_core::identity::{ModelIdentity, ModelRef};
use model_reviewer_core::pick::PickIndex;
use model_reviewer_core::review::ReviewState;
use model_reviewer_core::scene::{NodeId, SceneRead};

const ARM: NodeId = NodeId(9);
const ARM_TIP: NodeId = NodeId(10);
const VIEW: [u32; 2] = [1100, 720];

fn sample_session() -> Session {
    let bytes = model_reviewer_testdata::sample_glb();
    let model = Arc::new(model_reviewer_gltf::load_glb(&bytes).expect("見本のGLB"));
    let mut session = Session::default();
    session.review = Some(ReviewState::new(&model));
    session.model_ref = Some(ModelRef {
        file_name: "sample.glb".to_owned(),
        path: "sample.glb".into(),
        relative_path: None,
        identity: ModelIdentity::from_bytes(&bytes),
        stl_unit: None,
        stl_up: None,
    });
    session.pick_index = Some(PickIndex::build(&model));
    session.set_model(model);
    session.set_viewport(VIEW);
    let bounds = session.doc.as_ref().unwrap().bounds();
    session.camera =
        model_reviewer_core::camera::OrbitCamera::home(&bounds, VIEW[0] as f32 / VIEW[1] as f32);
    let _ = session.take_scene_dirty();
    session
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("undo")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn local(session: &Session, id: NodeId) -> Affine3A {
    session.doc.as_ref().unwrap().node(id).unwrap().local
}

fn world(session: &Session, id: NodeId) -> Affine3A {
    *session.doc.as_ref().unwrap().world(id)
}

/// ハンドルの掴める点（正規化座標）。
fn grab_ndc(session: &Session, handle: Handle) -> Vec2 {
    let size = [VIEW[0] as f32, VIEW[1] as f32];
    let frame = gizmo_frame(session, size, GIZMO_SIZE).expect("ハンドルの枠");
    let (_, px) = grab_points(session.tool, &frame)
        .into_iter()
        .find(|(h, _)| *h == handle)
        .expect("掴める点");
    Vec2::new(px.x / size[0] * 2.0 - 1.0, 1.0 - px.y / size[1] * 2.0)
}

/// arm を選んで移動の道具でX軸を掴み、右へ動かす（確定はしない）。
fn drag_arm_right(session: &mut Session) {
    session.apply(Command::Select(Some(ARM)));
    session.apply(Command::SetTool(Tool::Move));
    assert_eq!(session.tool, Tool::Move);
    let start = grab_ndc(session, Handle::MoveX);
    session.apply(Command::BeginTransform {
        handle: Handle::MoveX,
        ndc: start,
    });
    assert!(session.transform.is_some(), "掴めた");
    session.apply(Command::DragTransform {
        ndc: start + Vec2::new(0.15, 0.0),
    });
}

#[test]
fn transform_commits_once_and_undo_redo_restore_bits() {
    let mut session = sample_session();
    let local_before = local(&session, ARM);
    let tip_before = world(&session, ARM_TIP);
    drag_arm_right(&mut session);
    // ドラッグ中は文書も履歴も変わらない。見た目は差分の行列だけ。
    assert_eq!(local(&session, ARM), local_before);
    assert!(session.history.is_empty());
    let delta = session.transform.as_ref().unwrap().delta;
    assert_ne!(delta, Affine3A::IDENTITY);
    assert!(session.take_scene_dirty().drag);
    assert!(!session.unsaved());

    session.apply(Command::CommitTransform);
    assert!(session.transform.is_none());
    assert_eq!(session.history.len(), 1);
    assert!(session.unsaved());
    let local_after = local(&session, ARM);
    assert_ne!(local_after, local_before);
    // 子も親に付いて動く。動いたのはX方向だけ。
    let moved =
        Vec3::from(world(&session, ARM_TIP).translation) - Vec3::from(tip_before.translation);
    assert!(moved.x > 1.0e-3, "右へ動いた: {moved:?}");
    assert!(
        moved.y.abs() < 1.0e-5 && moved.z.abs() < 1.0e-5,
        "{moved:?}"
    );
    // 動かした範囲だけを部分更新に回す。構造は変わらない。
    let dirty = session.take_scene_dirty();
    assert!(!dirty.instances);
    assert_eq!(session.take_instance_patches(), vec![9..11]);
    assert!(session.metrics.edit_commit.is_some());
    assert_eq!(session.metrics.history_len, Some(1));

    session.apply(Command::Select(None));
    session.apply(Command::Undo);
    assert_eq!(local(&session, ARM), local_before);
    assert_eq!(world(&session, ARM_TIP), tip_before);
    assert_eq!(session.selection, Some(ARM), "戻した部品を選ぶ");
    assert!(!session.unsaved(), "開いた時点まで戻れば印が消える");
    assert!(session.history.can_redo());

    session.apply(Command::Redo);
    assert_eq!(local(&session, ARM), local_after);
    assert!(session.unsaved());
    assert_eq!(session.history.len(), 1);
    assert!(session.status_text.contains("やり直しました"));
}

#[test]
fn move_status_uses_the_display_unit() {
    use model_reviewer::ui::gizmo_view::readout_text_with;
    use model_reviewer_core::gizmo::Readout;
    use model_reviewer_core::units::UnitLabel;

    for (unit, factor, suffix) in [
        (None, 1.0, None),
        (Some(UnitLabel::Mm), 1000.0, Some("mm")),
        (Some(UnitLabel::Cm), 100.0, Some("cm")),
        (Some(UnitLabel::M), 1.0, Some("m")),
    ] {
        let mut session = sample_session();
        session.apply(Command::SetDisplayUnit(unit));
        drag_arm_right(&mut session);
        let Some(Readout::Distance(distance)) = session.transform.unwrap().readout else {
            panic!("移動の変化量がありません");
        };
        let expected = readout_text_with(Readout::Distance(distance * factor), suffix);
        assert_eq!(session.status_text, format!("移動 {expected}"));
    }
}

#[test]
fn cancelling_a_drag_leaves_document_and_history_untouched() {
    let mut session = sample_session();
    let before = local(&session, ARM);
    drag_arm_right(&mut session);
    session.apply(Command::CancelTransform);
    assert!(session.transform.is_none());
    assert_eq!(local(&session, ARM), before);
    assert!(session.history.is_empty());
    assert!(!session.unsaved());
    // 取り消したあとに離しても確定しない。
    session.apply(Command::CommitTransform);
    assert!(session.history.is_empty());
    // 掴んだまま別の操作が来たら、変形をやめてから行う。
    drag_arm_right(&mut session);
    session.apply(Command::HideSelected);
    assert!(session.transform.is_none());
    assert_eq!(local(&session, ARM), before);
    assert_eq!(session.history.len(), 1, "非表示の1回だけ");
}

#[test]
fn numeric_preview_and_commit() {
    let mut session = sample_session();
    session.apply(Command::Select(Some(ARM)));
    let before = local(&session, ARM);
    let mut trs = decompose(&before).expect("分けられる");
    trs.translation.x += 0.5;
    session.apply(Command::PreviewLocal(trs));
    assert_eq!(local(&session, ARM), before, "ドラッグ中は書き換えない");
    assert!(session.transform.is_some());
    assert!(session.history.is_empty());
    session.apply(Command::CommitLocal(trs));
    assert!(session.transform.is_none());
    assert_eq!(session.history.len(), 1);
    let after = decompose(&local(&session, ARM)).unwrap();
    assert!((after.translation.x - trs.translation.x).abs() < 1.0e-6);
    // 同じ値の確定は履歴を増やさない。
    session.apply(Command::CommitLocal(after));
    assert_eq!(session.history.len(), 1);
    // Esc（取り消し）で戻すと、確定前の見た目に戻る。
    let mut again = after;
    again.translation.y += 1.0;
    session.apply(Command::PreviewLocal(again));
    session.apply(Command::CancelTransform);
    assert!(session.transform.is_none());
    assert_eq!(session.history.len(), 1);
}

#[test]
fn non_finite_or_zero_results_are_not_committed() {
    let mut session = sample_session();
    session.apply(Command::Select(Some(ARM)));
    let before = local(&session, ARM);
    let mut nan = decompose(&before).unwrap();
    nan.translation.x = f32::NAN;
    session.apply(Command::CommitLocal(nan));
    assert_eq!(local(&session, ARM), before);
    assert!(session.history.is_empty());
    assert!(
        session.status_text.contains("有限"),
        "{}",
        session.status_text
    );

    let mut flat = decompose(&before).unwrap();
    flat.scale = Vec3::new(1.0, 0.0, 1.0);
    session.apply(Command::CommitLocal(flat));
    assert_eq!(local(&session, ARM), before);
    assert!(session.history.is_empty());
    assert!(
        session.status_text.contains("大きさ"),
        "{}",
        session.status_text
    );
    let infinite = Trs {
        scale: Vec3::splat(f32::INFINITY),
        ..Trs::default()
    };
    session.apply(Command::PreviewLocal(infinite));
    session.apply(Command::CommitLocal(infinite));
    assert_eq!(local(&session, ARM), before);
    assert!(session.history.is_empty());
}

#[test]
fn edits_are_refused_while_a_model_is_loading() {
    let mut session = sample_session();
    session.apply(Command::Select(Some(ARM)));
    session.apply(Command::SetTool(Tool::Move));
    session.apply(Command::HideSelected);
    assert_eq!(session.history.len(), 1);
    session.load = LoadPhase::Reading;
    assert!(session.edits_locked());
    let reason = session.edit_block_reason().unwrap().to_owned();
    let before = local(&session, ARM);
    let trs = decompose(&before).unwrap();
    for command in [
        Command::SetTool(Tool::Rotate),
        Command::BeginTransform {
            handle: Handle::MoveX,
            ndc: Vec2::ZERO,
        },
        Command::PreviewLocal(trs),
        Command::CommitLocal(Trs {
            translation: trs.translation + Vec3::X,
            ..trs
        }),
        Command::Undo,
        Command::Redo,
    ] {
        session.status_text.clear();
        session.apply(command.clone());
        assert_eq!(session.status_text, reason, "{command:?}");
    }
    assert_eq!(session.tool, Tool::Move);
    assert!(session.transform.is_none());
    assert_eq!(local(&session, ARM), before);
    assert_eq!(session.history.len(), 1);
    assert!(!session.history.can_redo());
}

#[test]
fn annotation_and_hiding_are_undoable_and_return_with_the_same_ids() {
    let mut session = sample_session();
    session.apply(Command::SetAnnotateMode(true));
    assert_eq!(session.tool, Tool::Annotate);
    session.apply(Command::ViewClick { ndc: Vec2::ZERO });
    assert!(session.draft.is_some(), "中央に面がある");
    session.apply(Command::CommitDraft {
        text: "ここ".to_owned(),
    });
    let note = session.review.as_ref().unwrap().annotations[0].clone();
    session.apply(Command::HideSelected);
    let hidden = note.node;
    assert!(!session.review.as_ref().unwrap().visibility.own(hidden));
    assert_eq!(session.history.len(), 2);
    assert!(session.take_scene_dirty().instances);

    session.apply(Command::Undo);
    assert!(session.review.as_ref().unwrap().visibility.own(hidden));
    assert!(session.take_scene_dirty().instances, "表示は全体を作り直す");
    session.apply(Command::Undo);
    assert!(session.review.as_ref().unwrap().annotations.is_empty());
    assert_eq!(session.selected_annotation, None);
    assert!(!session.unsaved());

    session.apply(Command::Redo);
    let back = &session.review.as_ref().unwrap().annotations;
    assert_eq!(back.len(), 1);
    assert_eq!(back[0], note, "番号も本文も同じ");

    // 本文の編集・削除・視点の追加と削除も戻せる。
    session.apply(Command::EditAnnotation {
        id: note.id,
        text: "直した".to_owned(),
    });
    session.apply(Command::DeleteAnnotation(note.id));
    session.apply(Command::SaveView {
        name: "横".to_owned(),
    });
    let view = session.review.as_ref().unwrap().views[0].id;
    session.apply(Command::DeleteView(view));
    assert!(session.review.as_ref().unwrap().views.is_empty());
    session.apply(Command::Undo);
    assert_eq!(session.review.as_ref().unwrap().views.len(), 1);
    session.apply(Command::Undo);
    assert!(session.review.as_ref().unwrap().views.is_empty());
    session.apply(Command::Undo);
    assert_eq!(
        session.review.as_ref().unwrap().annotations[0].text,
        "直した"
    );
    session.apply(Command::Undo);
    assert_eq!(session.review.as_ref().unwrap().annotations[0].text, "ここ");
}

#[test]
fn new_edit_after_undo_drops_redo_and_saving_moves_the_clean_point() {
    let dir = scratch("clean-point");
    let mut session = sample_session();
    session.apply(Command::Select(Some(ARM)));
    session.apply(Command::HideSelected);
    session.apply(Command::SaveReview(dir.join("review.json")));
    assert!(!session.unsaved(), "保存した時点");
    session.apply(Command::ShowAll);
    assert!(session.unsaved());
    session.apply(Command::Undo);
    assert!(!session.unsaved(), "保存した時点まで戻れば印が消える");
    session.apply(Command::Undo);
    assert!(session.unsaved(), "保存より前へ戻ると、また未保存");
    session.apply(Command::Redo);
    assert!(!session.unsaved());
    session.apply(Command::Undo);
    session.apply(Command::Select(Some(NodeId(1))));
    session.apply(Command::HideSelected);
    assert!(
        !session.history.can_redo(),
        "新しい操作でやり直しの分を捨てる"
    );

    // 形の編集はレビューの保存では消えない（書き出しまで残る）。
    session.apply(Command::Select(Some(ARM)));
    let mut trs = decompose(&local(&session, ARM)).unwrap();
    trs.translation.z += 0.25;
    session.apply(Command::CommitLocal(trs));
    session.apply(Command::SaveReview(dir.join("review.json")));
    assert!(session.unsaved(), "動かした形はレビューに入らない");
}

#[test]
fn escape_order_is_drag_draft_tool_then_selection() {
    let mut session = sample_session();
    drag_arm_right(&mut session);
    let ctx = egui::Context::default();
    let escape = |session: &Session| -> Vec<Command> {
        let mut out = Vec::new();
        let raw = egui::RawInput {
            events: [true, false]
                .map(|pressed| egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                })
                .to_vec(),
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            out = model_reviewer::shortcut_commands(ui.ctx(), session);
        });
        output.textures_delta.clear();
        out
    };
    assert_eq!(escape(&session), vec![Command::CancelTransform]);
    session.apply(Command::CancelTransform);
    assert_eq!(escape(&session), vec![Command::SetTool(Tool::Select)]);
    session.apply(Command::SetTool(Tool::Select));
    assert_eq!(escape(&session), vec![Command::Select(None)]);
    session.apply(Command::Select(None));
    assert!(escape(&session).is_empty());
}

#[test]
fn opening_a_model_clears_the_history() {
    let mut session = sample_session();
    session.apply(Command::Select(Some(ARM)));
    session.apply(Command::HideSelected);
    assert!(session.history.can_undo());
    let bytes = model_reviewer_testdata::sample_glb();
    let model = Arc::new(model_reviewer_gltf::load_glb(&bytes).unwrap());
    let dir = scratch("reopen");
    let path = dir.join("sample.glb");
    std::fs::write(&path, &bytes).unwrap();
    session.mark_saved();
    session.apply(Command::OpenModel(path));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while session.history.can_undo() {
        session.poll_loader();
        assert!(std::time::Instant::now() < deadline, "読み込みが終わらない");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(!session.history.can_redo());
    assert!(!session.unsaved());
    drop(model);
}

const LAMP: NodeId = NodeId(11);

/// 部品を鍵ではなく名前で並べた写し。選択・注釈・表示が「同じ部品」を指すかを比べる。
#[derive(Debug, PartialEq)]
struct Snapshot {
    names: Vec<String>,
    hidden: Vec<String>,
    notes: Vec<(u64, String, String)>,
    selection: Option<String>,
    selected_note: Option<u64>,
}

fn snapshot(session: &Session) -> Snapshot {
    let doc = session.doc.as_ref().unwrap();
    let review = session.review.as_ref().unwrap();
    let name = |id: NodeId| doc.name(id).to_owned();
    assert_eq!(
        review.visibility.len(),
        doc.node_count(),
        "表示の配列の長さ"
    );
    Snapshot {
        names: (0..doc.node_count() as u32)
            .map(|i| name(NodeId(i)))
            .collect(),
        hidden: review.visibility.hidden_own().map(name).collect(),
        notes: review
            .annotations
            .iter()
            .map(|note| (note.id, name(note.node), note.text.clone()))
            .collect(),
        selection: session.selection.map(name),
        selected_note: session.selected_annotation,
    }
}

fn add_note(session: &mut Session, node: NodeId, text: &str) -> u64 {
    session.draft = Some(model_reviewer::session::Draft {
        node,
        local_pos: Vec3::ZERO,
        local_normal: Vec3::Y,
        text: String::new(),
    });
    session.apply(Command::CommitDraft {
        text: text.to_owned(),
    });
    session
        .review
        .as_ref()
        .unwrap()
        .annotations
        .last()
        .unwrap()
        .id
}

#[test]
fn add_duplicate_delete_and_undo_keep_every_owner_on_the_same_part() {
    let mut session = sample_session();
    let base_prims = session.doc.as_ref().unwrap().primitive_count();
    let triangles = session.placed_triangles();
    session.apply(Command::SetVisible {
        node: LAMP,
        visible: false,
    });
    add_note(&mut session, ARM_TIP, "先端");
    let lamp_note = add_note(&mut session, LAMP, "灯り");
    let start = snapshot(&session);
    let undo_start = session.history.len();

    // 図形: 何も無い所を見て置き、その場で選べる（当たり判定の索引に足してある）。
    session.camera.target = Vec3::new(8.0, 0.0, 8.0);
    session.apply(Command::AddShape(
        model_reviewer_core::shapes::ShapeKind::Cube,
    ));
    let doc = session.doc.as_ref().unwrap();
    let cube = NodeId(doc.node_count() as u32 - 1);
    assert_eq!(doc.name(cube), "立方体");
    assert_eq!(session.selection, Some(cube));
    assert_eq!(session.tool, Tool::Move, "置いたら移動の道具");
    assert_eq!(doc.primitive_count(), base_prims + 1);
    assert_eq!(doc.draws(cube)[0].prim.0 as usize, base_prims);
    assert_eq!(
        doc.draws(cube)[0].base_color,
        model_reviewer_core::shapes::SHAPE_COLOR
    );
    assert_eq!(doc.world(cube).translation, Vec3::new(8.0, 0.0, 8.0).into());
    assert!(
        session.placed_triangles() > triangles,
        "三角形の数も文書から"
    );
    assert!(session.take_scene_dirty().instances);
    session.apply(Command::SetTool(Tool::Select));
    session.apply(Command::Select(None));
    session.apply(Command::ViewClick { ndc: Vec2::ZERO });
    assert_eq!(session.selection, Some(cube), "足した図形を選べる");

    // 複製: arm と子を元の直後へ。後ろの部品（lamp）の注釈と非表示が付いていく。
    session.apply(Command::Select(Some(ARM)));
    session.apply(Command::DuplicateSelected);
    let copy = NodeId(ARM_TIP.0 + 1);
    let doc = session.doc.as_ref().unwrap();
    assert_eq!(doc.name(copy), "arm コピー");
    assert_eq!(doc.name(NodeId(copy.0 + 1)), "arm_tip");
    assert_eq!(doc.parent(copy), doc.parent(ARM), "同じ親の下");
    assert_eq!(
        doc.draws(copy)[0].prim,
        doc.draws(ARM)[0].prim,
        "形状を共有"
    );
    assert_eq!(session.selection, Some(copy));
    let after_dup = snapshot(&session);
    assert_eq!(after_dup.hidden, vec!["lamp".to_owned()]);
    assert!(
        after_dup
            .notes
            .contains(&(lamp_note, "lamp".to_owned(), "灯り".to_owned()))
    );

    // 削除: 元の arm と子、付いていた注釈（先端）も一緒に消える。
    session.apply(Command::Select(Some(ARM)));
    session.apply(Command::DeleteSelected);
    assert_eq!(session.status_text, "部品 2 個と注釈 1 件を削除しました");
    assert_eq!(session.selection, None);
    let after_delete = snapshot(&session);
    assert!(!after_delete.names.contains(&"arm".to_owned()));
    assert_eq!(after_delete.notes.len(), 1);
    assert_eq!(after_delete.hidden, vec!["lamp".to_owned()]);
    assert!(session.doc_unsaved() && session.review_unsaved());

    // 削除の取り消し: 部品・表示・注釈が番号も含めて戻り、戻した部品が選ばれる。
    session.apply(Command::Undo);
    let mut expected = after_dup;
    expected.selection = Some("arm".to_owned());
    assert_eq!(snapshot(&session), expected);
    session.apply(Command::Undo);
    session.apply(Command::Undo);
    let mut back = snapshot(&session);
    back.selection = None;
    let mut start_unselected = start;
    start_unselected.selection = None;
    start_unselected.selected_note = None;
    assert_eq!(back, start_unselected, "最初と同じ部品を指す");
    assert_eq!(session.history.len(), undo_start);
    assert_eq!(session.placed_triangles(), triangles);
    // 取り消しても形状は残る（番号を使い回さない）。
    assert_eq!(
        session.doc.as_ref().unwrap().primitive_count(),
        base_prims + 1
    );

    // やり直しで削除の後へ戻る。
    session.apply(Command::Redo);
    session.apply(Command::Redo);
    session.apply(Command::Redo);
    let mut redone = snapshot(&session);
    redone.selection = None;
    let mut expected = after_delete;
    expected.selection = None;
    assert_eq!(redone, expected);
}

#[test]
fn a_part_with_a_draft_is_not_deleted() {
    let mut session = sample_session();
    session.draft = Some(model_reviewer::session::Draft {
        node: ARM_TIP,
        local_pos: Vec3::ZERO,
        local_normal: Vec3::Y,
        text: String::new(),
    });
    session.apply(Command::Select(Some(ARM)));
    session.apply(Command::DeleteSelected);
    assert!(
        session.status_text.contains("書きかけの注釈"),
        "{}",
        session.status_text
    );
    assert_eq!(session.doc.as_ref().unwrap().node_count(), 12);
    assert!(session.history.is_empty());
    assert_eq!(session.draft.as_ref().unwrap().node, ARM_TIP);
    // 下書きの無い部品は消せる。下書きは付け替えられて残る。
    session.apply(Command::Select(Some(NodeId(1))));
    session.apply(Command::DeleteSelected);
    assert_eq!(session.doc.as_ref().unwrap().node_count(), 11);
    assert_eq!(session.draft.as_ref().unwrap().node, NodeId(ARM_TIP.0 - 1));
}

#[test]
fn shapes_need_a_model_and_structure_edits_wait_for_loading() {
    let mut empty = Session::default();
    empty.key_labels = empty.keymap.labels(model_reviewer::keymap::Platform::Mac);
    empty.apply(Command::AddShape(
        model_reviewer_core::shapes::ShapeKind::Sphere,
    ));
    assert!(empty.doc.is_none());
    assert_eq!(
        empty.status_text,
        "［新規作成］（Cmd+N）か、モデルを開くと図形を置けます"
    );
    empty.apply(Command::DuplicateSelected);
    empty.apply(Command::DeleteSelected);
    assert!(empty.history.is_empty());

    let mut session = sample_session();
    session.apply(Command::Select(Some(ARM)));
    session.load = LoadPhase::Reading;
    let reason = session.edit_block_reason().unwrap().to_owned();
    for command in [
        Command::AddShape(model_reviewer_core::shapes::ShapeKind::Plane),
        Command::DuplicateSelected,
        Command::DeleteSelected,
    ] {
        session.status_text.clear();
        session.apply(command.clone());
        assert_eq!(session.status_text, reason, "{command:?}");
    }
    assert_eq!(session.doc.as_ref().unwrap().node_count(), 12);
    assert!(session.history.is_empty());
}

#[test]
fn stale_ids_after_a_structure_change_are_ignored() {
    // 構造が変わった同じフレームに、古い番号の選択が届いても落ちない。
    let mut session = sample_session();
    session.apply(Command::Select(Some(LAMP)));
    session.apply(Command::DeleteSelected);
    session.apply(Command::Select(Some(LAMP)));
    assert_eq!(session.selection, None, "範囲の外の番号は無視");
    session.apply(Command::SetVisible {
        node: LAMP,
        visible: false,
    });
    session.apply(Command::DeleteSelected);
    assert_eq!(session.doc.as_ref().unwrap().node_count(), 11);
}

#[test]
fn shape_size_follows_the_model_and_a_copy_of_a_shape_shares_it() {
    let mut session = sample_session();
    session.apply(Command::AddShape(
        model_reviewer_core::shapes::ShapeKind::Cube,
    ));
    let doc = session.doc.as_ref().unwrap();
    let cube = session.selection.unwrap();
    let radius = doc.base().bounds.radius();
    let size = model_reviewer_core::shapes::size_for(radius);
    let aabb = doc.world_aabb(cube);
    assert!(
        ((aabb.max.x - aabb.min.x) - size).abs() < 1.0e-5,
        "{aabb:?} {size}"
    );
    let prims = doc.primitive_count();
    session.apply(Command::DuplicateSelected);
    let doc = session.doc.as_ref().unwrap();
    let copy = session.selection.unwrap();
    assert_eq!(doc.name(copy), "立方体 コピー");
    assert_eq!(doc.primitive_count(), prims, "複製は形状を足さない");
    assert_eq!(doc.draws(copy)[0].prim, doc.draws(cube)[0].prim);
}

#[test]
fn structure_edits_during_a_drag_cancel_the_drag_first() {
    let mut session = sample_session();
    drag_arm_right(&mut session);
    let before = local(&session, ARM);
    session.apply(Command::DuplicateSelected);
    assert!(session.transform.is_none(), "変形をやめてから複製");
    assert_eq!(local(&session, ARM), before, "変形は確定しない");
    assert_eq!(session.history.len(), 1, "複製だけ");
    session.apply(Command::Select(Some(ARM)));
    session.apply(Command::SetTool(Tool::Move));
    drag_arm_right(&mut session);
    session.apply(Command::DeleteSelected);
    assert!(session.transform.is_none());
    assert_eq!(session.history.len(), 2);
    assert!(session.take_scene_dirty().drag, "変形の見た目を消す");
    session.apply(Command::Undo);
    assert_eq!(local(&session, ARM), before);
}

#[test]
fn loading_a_review_over_unexported_shapes_keeps_shape_numbers_aligned() {
    // 書き出していない図形があるままレビューを開くと、文書は元の形へ戻る。足した形状は
    // GPUと当たり判定の索引に送ってあるので、次に足す図形の番号が既に送った物と重ならないこと。
    let dir = scratch("review-over-shapes");
    let mut session = sample_session();
    let base = session.doc.as_ref().unwrap().primitive_count();
    session.apply(Command::SetVisible {
        node: ARM,
        visible: false,
    });
    let review = dir.join("review.json");
    session.apply(Command::SaveReview(review.clone()));
    assert!(!session.has_unsaved(), "{}", session.status_text);
    session.apply(Command::AddShape(
        model_reviewer_core::shapes::ShapeKind::Cube,
    ));
    assert_eq!(session.doc.as_ref().unwrap().primitive_count(), base + 1);
    session.apply(Command::LoadReview(review));
    assert!(
        session.dialog.is_some(),
        "書き出していない形の編集を失う確認"
    );
    session.apply(Command::ConfirmDiscard);
    assert_eq!(session.status_text, "レビューを復元しました");
    let doc = session.doc.as_ref().unwrap();
    assert_eq!(doc.node_count(), 12, "図形は消える");
    assert_eq!(doc.primitive_count(), base + 1, "形状の番号は引き継ぐ");
    assert!(!session.review.as_ref().unwrap().visibility.own(ARM));
    session.apply(Command::AddShape(
        model_reviewer_core::shapes::ShapeKind::Sphere,
    ));
    let doc = session.doc.as_ref().unwrap();
    let sphere = session.selection.unwrap();
    assert_eq!(doc.draws(sphere)[0].prim.0 as usize, base + 1, "続きの番号");
    assert_eq!(doc.name(sphere), "球");
}

#[test]
fn undo_or_redo_that_would_remove_a_part_with_a_draft_is_refused() {
    let mut session = sample_session();
    // 足した図形に下書きを置いてから「戻す」と、図形と一緒に書きかけの注釈が消えてしまう。
    session.apply(Command::AddShape(
        model_reviewer_core::shapes::ShapeKind::Cube,
    ));
    let cube = NodeId(session.doc.as_ref().unwrap().node_count() as u32 - 1);
    session.draft = Some(model_reviewer::session::Draft {
        node: cube,
        local_pos: Vec3::ZERO,
        local_normal: Vec3::Y,
        text: "書きかけ".to_owned(),
    });
    session.apply(Command::Undo);
    assert!(
        session.status_text.contains("書きかけの注釈"),
        "{}",
        session.status_text
    );
    assert_eq!(session.doc.as_ref().unwrap().node_count(), 13);
    assert_eq!(session.draft.as_ref().unwrap().node, cube);
    assert_eq!(session.draft.as_ref().unwrap().text, "書きかけ");
    assert!(session.history.can_undo(), "履歴も残る");

    // やり直しで部品が消える場合も同じ。
    session.draft = None;
    session.apply(Command::Select(Some(ARM)));
    session.apply(Command::DeleteSelected);
    session.apply(Command::Undo);
    assert_eq!(session.doc.as_ref().unwrap().name(ARM_TIP), "arm_tip");
    session.draft = Some(model_reviewer::session::Draft {
        node: ARM_TIP,
        local_pos: Vec3::ZERO,
        local_normal: Vec3::Y,
        text: String::new(),
    });
    session.apply(Command::Redo);
    assert!(session.status_text.contains("書きかけの注釈"));
    assert_eq!(session.doc.as_ref().unwrap().node_count(), 13);
    assert_eq!(session.draft.as_ref().unwrap().node, ARM_TIP);
    assert!(session.history.can_redo());
    // 下書きを取り消せば、やり直せる。
    session.apply(Command::CancelDraft);
    session.apply(Command::Redo);
    assert_eq!(session.doc.as_ref().unwrap().node_count(), 11);
}

#[test]
fn large_counts_in_the_status_use_digit_groups() {
    assert_eq!(model_reviewer::ui::status::grouped(50001), "50,001");
    assert_eq!(model_reviewer::ui::status::grouped(999), "999");
}
