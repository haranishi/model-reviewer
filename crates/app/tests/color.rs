//! 色の確定から履歴・部分更新・書き出しまでを画面なしで確かめる。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use model_reviewer as app;
use model_reviewer::command::{Axis, Command, ShapeKind, Tool};
use model_reviewer::loader::LoadPhase;
use model_reviewer::session::{ColorState, Session, selection_color};
use model_reviewer_core::color::{SWATCHES, srgb8_to_linear};
use model_reviewer_core::history::History;
use model_reviewer_core::review::ReviewState;
use model_reviewer_core::scene::{LoadWarning, Model, NodeId, SceneRead};
use model_reviewer_gltf::load_glb;
use model_reviewer_testdata::{rich_glb, sample_glb};

#[path = "support/texture_stream.rs"]
mod texture_stream;

const ARM: NodeId = NodeId(9);
const WHEEL: NodeId = NodeId(3);

fn selected(model: Model, id: NodeId) -> Session {
    let mut session = Session::default();
    session.review = Some(ReviewState::new(&model));
    session.set_model(Arc::new(model));
    session.apply(Command::Select(Some(id)));
    session
}

fn colors(session: &Session, id: NodeId) -> Vec<[f32; 4]> {
    let doc = session.doc.as_ref().unwrap();
    doc.subtree_colors(doc.key(id).unwrap()).unwrap()
}

fn paint(index: usize) -> Command {
    Command::SetColor(srgb8_to_linear(SWATCHES[index].1))
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("color")
        .join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn open(session: &mut Session, path: &Path) {
    session.apply(Command::OpenModel(path.to_owned()));
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.is_loading() || session.pick_index.is_none() {
        session.poll_loader();
        assert!(Instant::now() < deadline, "{}", session.status_text);
        std::thread::yield_now();
    }
    assert_eq!(session.status_text, "読み込み完了");
}

fn finish_export(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.is_exporting() {
        session.poll_exporter();
        assert!(Instant::now() < deadline, "{}", session.status_text);
        std::thread::yield_now();
    }
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );
}

#[test]
fn set_color_is_one_history_step() {
    let mut model = load_glb(&sample_glb()).unwrap();
    for (i, draw) in model.draws.iter_mut().enumerate() {
        draw.base_color[3] = (i as f32 + 1.0) / 16.0;
    }
    let mut session = selected(model, ARM);
    let before = colors(&session, ARM);
    assert!(matches!(
        selection_color(&session).unwrap().state,
        ColorState::Mixed { .. }
    ));
    session.apply(Command::SetTool(Tool::Move));
    session.apply(Command::BeginKeyTransform(Axis::X));
    assert!(session.transform.is_some());
    session.apply(paint(4));
    assert!(session.transform.is_none());
    assert_eq!(session.history.len(), 1);
    assert_eq!(session.history.undo_label(), Some("色の変更"));
    assert_eq!(session.status_text, "色の変更を確定しました");
    let after = colors(&session, ARM);
    let [r, g, b] = srgb8_to_linear(SWATCHES[4].1);
    for (original, painted) in before.iter().zip(&after) {
        assert_eq!(
            painted.map(f32::to_bits),
            [r, g, b, original[3]].map(f32::to_bits)
        );
    }
    session.apply(Command::Undo);
    assert_eq!(colors(&session, ARM), before);
    assert!(matches!(
        selection_color(&session).unwrap().state,
        ColorState::Mixed { .. }
    ));
    session.apply(Command::Redo);
    assert_eq!(colors(&session, ARM), after);
    assert_eq!(
        selection_color(&session).unwrap().state,
        ColorState::One(SWATCHES[4].1)
    );
}

#[test]
fn same_color_adds_no_history() {
    let mut session = selected(load_glb(&sample_glb()).unwrap(), WHEEL);
    let [r, g, b, _] = colors(&session, WHEEL)[0];
    session.apply(Command::SetColor([r, g, b]));
    assert!(session.history.is_empty());
    // 表示の8ビットに丸めると同じ色でも、文書の1ビットの差は確定する。
    let next = Command::SetColor([f32::from_bits(r.to_bits() + 1), g, b]);
    session.apply(next.clone());
    let rev = session.history.doc_rev();
    assert_eq!(session.history.len(), 1);
    session.take_instance_patches();
    session.apply(next);
    assert_eq!(session.history.len(), 1);
    assert_eq!(session.history.doc_rev(), rev);
    assert!(session.take_instance_patches().is_empty());
}

#[test]
fn color_marks_document_unexported() {
    let mut session = selected(load_glb(&sample_glb()).unwrap(), WHEEL);
    session.apply(paint(4));
    assert!(session.doc_unsaved());
    assert!(!session.review_unsaved());
    assert!(session.on_close_requested());
    session.apply(Command::CancelPending);
    session.apply(Command::Undo);
    assert!(!session.has_unsaved());

    session.history = History::with_limits(1, 1);
    session.apply(paint(9));
    assert!(session.doc_unsaved());
    assert!(session.history.is_empty());
    assert!(
        session.status_text.contains("戻せません"),
        "{}",
        session.status_text
    );
}

#[test]
fn color_is_refused_while_busy() {
    let mut session = Session::default();
    session.apply(Command::NewDocument);
    session.apply(Command::AddShape(ShapeKind::Cube));
    let before = colors(&session, NodeId(0));
    let rev = session.history.doc_rev();
    assert!(!selection_color(&session).unwrap().locked);
    session.load = LoadPhase::Reading;
    session.apply(paint(4));
    assert!(selection_color(&session).unwrap().locked);
    assert!(session.status_text.contains("読み込み中"));
    session.load = LoadPhase::Idle;
    session.apply(Command::ExportGlb(scratch("locked").join("cube.glb")));
    session.apply(paint(9));
    assert!(selection_color(&session).unwrap().locked);
    assert!(session.status_text.contains("書き出し中"));
    finish_export(&mut session);
    assert!(!selection_color(&session).unwrap().locked);
    assert_eq!(colors(&session, NodeId(0)), before);
    assert_eq!(session.history.doc_rev(), rev);
}

#[test]
fn color_change_patches_instances_only() {
    let mut session = selected(load_glb(&sample_glb()).unwrap(), ARM);
    session.take_scene_dirty();
    let triangles = session.placed_triangles();
    let bounds = session.doc.as_ref().unwrap().bounds();
    for cmd in [paint(4), Command::Undo, Command::Redo] {
        session.apply(cmd);
        assert_eq!(session.take_instance_patches(), vec![9..11]);
        let dirty = session.take_scene_dirty();
        assert!(dirty.any() && !dirty.instances && !dirty.model);
        assert_eq!(session.placed_triangles(), triangles);
        assert_eq!(session.doc.as_ref().unwrap().bounds(), bounds);
    }
}

#[test]
fn restored_wheel_can_be_recolored_after_exporting_its_deletion() {
    let dir = scratch("restored-wheel");
    let source = dir.join("source.glb");
    std::fs::write(&source, sample_glb()).unwrap();
    let mut session = Session::default();
    open(&mut session, &source);
    session.apply(Command::Select(Some(WHEEL)));
    let other = colors(&session, NodeId(4));
    session.apply(Command::DeleteSelected);
    session.apply(Command::ExportGlb(dir.join("deleted.glb")));
    finish_export(&mut session);
    session.apply(Command::DismissDialog);
    session.apply(Command::Undo);
    session.apply(Command::Select(Some(WHEEL)));
    session.apply(paint(4));
    let output = dir.join("restored.glb");
    session.apply(Command::ExportGlb(output.clone()));
    finish_export(&mut session);
    let model = load_glb(&std::fs::read(output).unwrap()).unwrap();
    let [r, g, b] = srgb8_to_linear(SWATCHES[4].1);
    assert_eq!(model.draws(WHEEL)[0].base_color, [r, g, b, 1.0]);
    assert_eq!(model.draws(NodeId(4))[0].base_color, other[0]);
}

#[test]
fn color_survives_export_and_reopen() {
    let dir = scratch("roundtrip");
    let original = sample_glb();
    let source = dir.join("source.glb");
    let output = dir.join("painted.glb");
    std::fs::write(&source, &original).unwrap();
    let mut session = Session::default();
    open(&mut session, &source);
    session.apply(Command::Select(Some(WHEEL)));
    let shared = colors(&session, NodeId(4));
    session.apply(paint(9));
    session.apply(Command::DuplicateSelected);
    let copy = session.selection.unwrap();
    assert_eq!(colors(&session, copy), colors(&session, WHEEL));
    session.apply(paint(4));
    session.apply(Command::ExportGlb(output.clone()));
    finish_export(&mut session);
    assert!(!session.doc_unsaved());
    session.apply(Command::DismissDialog);
    open(&mut session, &output);
    let doc = session.doc.as_ref().unwrap();
    let by_name = |name: &str| {
        NodeId(
            doc.nodes()
                .iter()
                .position(|n| n.name.as_ref() == name)
                .unwrap() as u32,
        )
    };
    for (name, index) in [("wheel_FL", 9), ("wheel_FL コピー", 4)] {
        let [r, g, b] = srgb8_to_linear(SWATCHES[index].1);
        assert_eq!(
            colors(&session, by_name(name))[0].map(f32::to_bits),
            [r, g, b, 1.0].map(f32::to_bits)
        );
    }
    assert_eq!(colors(&session, by_name("wheel_FR")), shared);
    assert_eq!(std::fs::read(source).unwrap(), original);
}

#[test]
fn texture_note_is_shown_for_textured_model() {
    use egui_kittest::{Harness, kittest::Queryable};

    let dir = scratch("texture-note");
    let quad = dir.join("quad.glb");
    std::fs::write(
        &quad,
        model_reviewer_testdata::textured_quad_glb(&Default::default()),
    )
    .unwrap();
    let source = dir.join("textured.glb");
    std::fs::write(&source, rich_glb()).unwrap();
    let mut session = Session::default();
    open(&mut session, &quad);
    texture_stream::CountingImages::default().finish(&mut session);
    session.apply(Command::Select(Some(NodeId(0))));
    let view = selection_color(&session).unwrap();
    let note = view.note.unwrap();
    assert_eq!(note, "色は画像に掛け合わされて見えます");
    let panel = Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .build_ui(|ui| model_reviewer::ui::color::show(ui, &view, &mut Vec::new()));
    panel.get_by_label(note);
    // こちらの埋め込み PNG は壊れている。画面には画像が出ず、書き出しには元の材質が残る。
    open(&mut session, &source);
    texture_stream::CountingImages::default().finish(&mut session);
    session.apply(Command::Select(Some(NodeId(1))));
    assert_eq!(session.textures.failed.len(), 1);
    assert_eq!(selection_color(&session).unwrap().note, None);
    session.apply(paint(4));
    session.apply(Command::ExportGlb(dir.join("recolored.glb")));
    finish_export(&mut session);
    let result = &session.dialog.as_ref().unwrap().body;
    assert_eq!(result.lines().filter(|line| line.contains(note)).count(), 1);
    assert!(result.contains("色を変えた部品 2 個"));
    session.apply(Command::ExportGlb(dir.join("unchanged.glb")));
    finish_export(&mut session);
    assert!(
        session
            .dialog
            .as_ref()
            .is_none_or(|d| !d.body.contains(note))
    );

    let mut vertices = load_glb(&sample_glb()).unwrap();
    vertices.warnings = vec![LoadWarning::VertexColors { count: 1 }];
    let session = selected(vertices, WHEEL);
    assert!(
        selection_color(&session)
            .unwrap()
            .note
            .unwrap()
            .contains("頂点の色")
    );
}

#[test]
fn count_uses_rgb_bits_caps_at_99_and_resets_for_another_document() {
    let mut model = load_glb(&sample_glb()).unwrap();
    let mut draw = model.draws[0];
    model.nodes.truncate(1);
    model.nodes[0].subtree_end = 1;
    model.nodes[0].draws = 0..100;
    model.draws = (0..100)
        .map(|i| {
            draw.base_color = [f32::from_bits(0.5_f32.to_bits() + i), 0.0, 0.0, 1.0];
            draw
        })
        .collect();
    let mut session = selected(model.clone(), NodeId(0));
    assert_eq!(
        selection_color(&session).unwrap().state,
        ColorState::Mixed { count: 99 }
    );
    for (i, draw) in model.draws.iter_mut().enumerate() {
        draw.base_color = [1.0, 0.0, 0.0, i as f32 / 128.0];
    }
    session.set_model(Arc::new(model));
    assert_eq!(
        selection_color(&session).unwrap().state,
        ColorState::One([255, 0, 0])
    );
}
