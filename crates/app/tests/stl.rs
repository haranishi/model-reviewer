//! STL を開く・単位と上の向きを替える・レビューを保存して戻す・GLB へ書き出す。GPUは使わない。
//!
//! 見本は 20 × 30 × 40 mm の箱（原点の角から伸びる）。mm と仮定すると、部品の大きさが 0.001 になり、
//! Z が上ならシーン（メートル）での外形は 0.02 × 0.04 × 0.03 になる。

mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use glam::{Affine3A, Vec2, Vec3};
use model_reviewer::command::{Command, Handle, KeyInput, ShapeKind, Tool, ViewDir};
use model_reviewer::loader::{LoadOptions, LoadPhase, NEITHER_GLB_NOR_STL};
use model_reviewer::session::{Session, SourceKind, selection_dims, stl_export_note};
use model_reviewer_core::gizmo::decompose;
use model_reviewer_core::scene::{NodeId, PrimId, SceneRead};
use model_reviewer_core::section::{Axis, SectionPlane};
use model_reviewer_core::shapes::{Dims, scale_for};
use model_reviewer_core::units::{UnitLabel, Units, UpAxis};
use model_reviewer_testdata::stl::{
    STL_SAMPLE_SIZE, ascii_stl, box_triangles, stl_inputs, stl_sample,
};

const Z_OPENING_NOTE: &str = "開いたときは Z を上と仮定し、回転 X の初期値を -90° にしました";

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("stl-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_bytes(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn pump_load(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let _ = session.poll_loader();
        let settled = matches!(session.load, LoadPhase::Idle | LoadPhase::Failed(_));
        let pick_ready = session.model.is_none() || session.pick_index.is_some();
        if settled && pick_ready && session.loading_file_name.is_none() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "読み込みが終わりません status={} phase={:?}",
            session.status_text,
            session.load.label()
        );
        thread::sleep(Duration::from_millis(2));
    }
}

fn pump_export(session: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while session.is_exporting() {
        let _ = session.poll_exporter();
        assert!(Instant::now() < deadline, "書き出しが終わりません");
        thread::sleep(Duration::from_millis(2));
    }
}

fn open(path: &Path) -> Session {
    let mut session = Session::default();
    session.set_viewport([800, 800]);
    session.apply(Command::OpenModel(path.to_path_buf()));
    pump_load(&mut session);
    assert_eq!(session.status_text, "読み込み完了", "{:?}", session.dialog);
    session
}

fn near(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() <= 1.0e-7 + expected.abs() * 1.0e-5
}

fn close3(actual: Vec3, expected: Vec3) -> bool {
    (actual - expected).length() <= 1.0e-7 + expected.length() * 1.0e-5
}

fn extent(session: &Session) -> Vec3 {
    let bounds = session.doc.as_ref().unwrap().bounds();
    bounds.max - bounds.min
}

fn sample_extent_m(meters: f32) -> Vec3 {
    let [x, y, z] = STL_SAMPLE_SIZE;
    Vec3::new(x, z, y) * meters
}

fn failure_body(session: &Session) -> String {
    session
        .dialog
        .as_ref()
        .expect("理由のダイアログ")
        .body
        .clone()
}

/// 箱の上で注釈を1つ置く（注釈の道具 → 中央をクリック → 確定）。
fn annotate_center(session: &mut Session, text: &str) {
    session.apply(Command::SetAnnotateMode(true));
    session.apply(Command::ViewClick { ndc: Vec2::ZERO });
    assert!(session.draft.is_some(), "箱の中央に当たる");
    session.apply(Command::CommitDraft {
        text: text.to_owned(),
    });
    session.apply(Command::SetAnnotateMode(false));
    assert_eq!(session.review.as_ref().unwrap().annotations.len(), 1);
}

#[test]
fn opens_binary_and_ascii_stl() {
    let dir = scratch("open");
    let triangles = box_triangles(STL_SAMPLE_SIZE);
    let inputs = [
        write_bytes(&dir, "box.stl", &stl_sample()),
        write_bytes(
            &dir,
            "box-ascii.stl",
            &ascii_stl(&[("box", &triangles)], "\n"),
        ),
        write_bytes(
            &dir,
            "BOX-CRLF.STL",
            &ascii_stl(&[("box", &triangles)], "\r\n"),
        ),
        // 拡張子が違っても、中身が STL なら開く（判定は中身で行う）。
        write_bytes(&dir, "box.bin", &stl_sample()),
    ];
    for path in &inputs {
        let session = open(path);
        assert_eq!(
            session.source_kind,
            SourceKind::Stl {
                unit: UnitLabel::Mm,
                up: UpAxis::Z,
            }
        );
        assert_eq!(
            session.units,
            Units {
                source: UnitLabel::M,
                display: Some(UnitLabel::Mm)
            }
        );
        let doc = session.doc.as_ref().unwrap();
        assert_eq!(doc.node_count(), 1);
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        assert_eq!(doc.name(NodeId(0)), stem);
        let trs = decompose(&doc.node(NodeId(0)).unwrap().local).unwrap();
        assert!(close3(trs.scale, Vec3::splat(0.001)));
        assert!(close3(extent(&session), sample_extent_m(0.001)));
        let model = session.model.as_ref().unwrap();
        assert_eq!(model.primitives[0].indices.len(), 36);
        assert_eq!(model.primitives[0].vertices.len(), 8);
        assert_eq!(
            session.model_ref.as_ref().unwrap().stl_unit,
            Some(UnitLabel::Mm)
        );
        assert!(
            session
                .opened_files()
                .iter()
                .any(|p| p.ends_with(path.file_name().unwrap()))
        );
    }
    // 元のファイルは書き換えない。
    assert_eq!(std::fs::read(&inputs[0]).unwrap(), stl_sample());
}

#[test]
fn stl_opens_z_up_by_default() {
    use egui_kittest::{Harness, kittest::Queryable};
    use model_reviewer::ui::{inspector, inspector_input};

    let stl = write_bytes(&scratch("default-up"), "box.stl", &stl_sample());
    let mut session = open(&stl);
    assert_eq!(session.model_ref.as_ref().unwrap().stl_up, Some(UpAxis::Z));
    session.apply(Command::Select(Some(NodeId(0))));
    let dims = selection_dims(&session).unwrap();
    let mm = dims
        .extent
        .unwrap()
        .map(|value| dims.units.to_display(value));
    assert!(close3(mm.into(), Vec3::new(20.0, 40.0, 30.0)));

    let input = inspector_input(&session);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .build_ui(move |ui| inspector::show(ui, &input, &mut Vec::new()));
    harness.step();
    let rotation_x = harness
        .get_all_by_role(egui::accesskit::Role::SpinButton)
        .nth(3)
        .unwrap();
    let text = rotation_x.value().unwrap();
    let degrees = text.trim_end_matches('°').replace('−', "-");
    // 表示の桁で -90 になり、分解時の端数が欄に出ない。
    assert_eq!(degrees.parse::<f64>().unwrap(), -90.0, "{text}");

    let local = session.doc.as_ref().unwrap().nodes()[0].local;
    let mut trs = decompose(&local).unwrap();
    trs.rotation = glam::Quat::IDENTITY;
    session.apply(Command::CommitLocal(trs));
    assert!(close3(
        extent(&session),
        Vec3::from(STL_SAMPLE_SIZE) * 0.001
    ));
    session.apply(Command::Undo);
    assert!(close3(extent(&session), sample_extent_m(0.001)));
}

#[test]
fn z_up_part_moves_along_world_y_with_keys_and_handle() {
    let stl = write_bytes(&scratch("up-move"), "box.stl", &stl_sample());
    let mut session = open(&stl);
    session.apply(Command::Select(Some(NodeId(0))));
    session.apply(Command::SetTool(Tool::Move));
    session.apply(Command::BeginKeyTransform(Axis::Y));
    session.apply(Command::KeyTransformInput(KeyInput::Char('5')));
    session.apply(Command::CommitTransform);
    let translation =
        |s: &Session| Vec3::from(s.doc.as_ref().unwrap().nodes()[0].world.translation);
    assert!(close3(translation(&session), Vec3::Y * 0.005));
    session.apply(Command::Undo);

    session.apply(Command::SnapView(ViewDir::Front));
    let pivot = session.doc.as_ref().unwrap().bounds().center();
    let start = session
        .camera
        .project(pivot + Vec3::Y * 0.002, 1.0)
        .unwrap();
    let end = session
        .camera
        .project(pivot + Vec3::Y * 0.007, 1.0)
        .unwrap();
    session.apply(Command::BeginTransform {
        handle: Handle::MoveY,
        ndc: start,
    });
    session.apply(Command::DragTransform { ndc: end });
    session.apply(Command::CommitTransform);
    assert!(close3(translation(&session), Vec3::Y * 0.005));
}

#[test]
fn neither_glb_nor_stl_message() {
    let dir = scratch("neither");
    for (name, bytes) in [
        ("notes.txt", b"hello, this is not a model".as_slice()),
        (
            "fake.glb",
            b"not a glb file at all, but long enough".as_slice(),
        ),
    ] {
        let path = write_bytes(&dir, name, bytes);
        let mut session = Session::default();
        session.apply(Command::OpenModel(path));
        pump_load(&mut session);
        assert!(matches!(session.load, LoadPhase::Failed(_)));
        assert!(session.model.is_none());
        assert_eq!(failure_body(&session), NEITHER_GLB_NOR_STL);
    }
    // 拡張子が `.stl` なら、STL として読んだ具体的な理由を出す。
    let path = write_bytes(&dir, "short.stl", b"abc");
    let mut session = Session::default();
    session.apply(Command::OpenModel(path));
    pump_load(&mut session);
    assert_eq!(failure_body(&session), "STL として短すぎます");
}

#[test]
fn broken_stl_keeps_the_open_model() {
    let dir = scratch("broken");
    let good = write_bytes(&dir, "good.stl", &stl_sample());
    let mut session = open(&good);
    annotate_center(&mut session, "角");
    let model = Arc::clone(session.model.as_ref().unwrap());
    let review = session.review.clone();
    for (name, bytes, expected) in [
        (
            "truncated.stl",
            stl_inputs::truncated_ascii(60),
            "途中で終わっています",
        ),
        ("count.stl", stl_inputs::count_one_too_many(), "三角形の数"),
        (
            "nan.stl",
            stl_inputs::binary_with_coordinate(f32::NAN),
            "座標",
        ),
    ] {
        let bad = write_bytes(&dir, name, &bytes);
        session.apply(Command::OpenModel(bad));
        if session.dialog.is_some() {
            // 未保存の注釈があるので確認が出る。破棄して続けても、読めなければ前のまま残る。
            session.apply(Command::ConfirmDiscard);
        }
        pump_load(&mut session);
        assert!(matches!(session.load, LoadPhase::Failed(_)), "{name}");
        let body = failure_body(&session);
        assert!(body.contains(expected), "{name}: {body}");
        assert!(Arc::ptr_eq(session.model.as_ref().unwrap(), &model));
        assert_eq!(session.review, review);
        assert_eq!(
            session.source_kind,
            SourceKind::Stl {
                unit: UnitLabel::Mm,
                up: UpAxis::Z,
            }
        );
        session.apply(Command::DismissDialog);
    }
}

#[test]
fn stl_review_saves_and_restores_with_unit() {
    let dir = scratch("review");
    let stl = write_bytes(&dir, "box.stl", &stl_sample());
    let mut session = Session::default();
    session.set_viewport([800, 800]);
    session.open_model_with(
        stl.clone(),
        LoadOptions {
            stl_unit: Some(UnitLabel::Cm),
            ..LoadOptions::default()
        },
    );
    pump_load(&mut session);
    assert_eq!(
        session.source_kind,
        SourceKind::Stl {
            unit: UnitLabel::Cm,
            up: UpAxis::Z,
        }
    );
    assert!(close3(extent(&session), sample_extent_m(0.01)));
    annotate_center(&mut session, "仕切り");
    let note = session.review.as_ref().unwrap().annotations[0].clone();
    let json_path = dir.join("box-review.json");
    session.apply(Command::SaveReview(json_path.clone()));
    assert_eq!(session.status_text, "レビューを保存しました");
    let text = std::fs::read_to_string(&json_path).unwrap();
    assert!(text.contains("\"stl_unit\": \"cm\""), "{text}");

    // 起動し直して読む。レビューの単位（cm）で STL を開いて復元する。
    let mut restored = Session::default();
    restored.apply(Command::LoadReview(json_path.clone()));
    pump_load(&mut restored);
    assert_eq!(restored.status_text, "レビューを復元しました");
    assert_eq!(
        restored.source_kind,
        SourceKind::Stl {
            unit: UnitLabel::Cm,
            up: UpAxis::Z,
        }
    );
    assert_eq!(restored.units.display, Some(UnitLabel::Cm));
    assert!(close3(extent(&restored), sample_extent_m(0.01)));
    let back = &restored.review.as_ref().unwrap().annotations[0];
    assert_eq!(back.text, note.text);
    assert!(close3(back.local_pos, note.local_pos));

    // mm で開いている所へ cm のレビューを読むと、その場で当てはめず cm で開き直す。
    let mut other = open(&stl);
    other.apply(Command::LoadReview(json_path));
    pump_load(&mut other);
    assert_eq!(other.status_text, "レビューを復元しました");
    assert_eq!(
        other.source_kind,
        SourceKind::Stl {
            unit: UnitLabel::Cm,
            up: UpAxis::Z,
        }
    );
    assert!(close3(extent(&other), sample_extent_m(0.01)));
    assert_eq!(other.review.as_ref().unwrap().annotations.len(), 1);

    // `stl_unit` の無い（前の版の）レビューは mm として開く。
    let legacy = text.replace(",\n    \"stl_unit\": \"cm\"", "");
    assert!(!legacy.contains("stl_unit"), "{legacy}");
    let legacy_path = write_bytes(&dir, "legacy.json", legacy.as_bytes());
    let mut old = Session::default();
    old.apply(Command::LoadReview(legacy_path));
    pump_load(&mut old);
    assert_eq!(
        old.source_kind,
        SourceKind::Stl {
            unit: UnitLabel::Mm,
            up: UpAxis::Z,
        }
    );
    assert_eq!(std::fs::read(&stl).unwrap(), stl_sample());
}

#[test]
fn stl_review_saves_and_restores_with_up() {
    let dir = scratch("review-up");
    let stl = write_bytes(&dir, "original.stl", &stl_sample());
    let mut author = open(&stl);
    author.apply(Command::SetStlUp(UpAxis::Y));
    pump_load(&mut author);
    annotate_center(&mut author, "上の面");
    let note = author.review.as_ref().unwrap().annotations[0].clone();
    author.apply(Command::SaveView {
        name: "検査位置".into(),
    });
    let review = dir.join("review.json");
    author.apply(Command::SaveReview(review.clone()));
    let mut json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&review).unwrap()).unwrap();
    assert_eq!(json["model"]["stl_up"], "y");

    // 同じ内容を既定の Z で開いている。改名後でも現在の実パスで Y に戻せる。
    let renamed = dir.join("renamed.stl");
    std::fs::rename(&stl, &renamed).unwrap();
    let mut session = open(&renamed);
    session.apply(Command::LoadReview(review.clone()));
    pump_load(&mut session);
    assert_eq!(
        session.source_kind,
        SourceKind::Stl {
            unit: UnitLabel::Mm,
            up: UpAxis::Y
        }
    );
    assert!(close3(
        extent(&session),
        Vec3::from(STL_SAMPLE_SIZE) * 0.001
    ));
    let restored = session.review.as_ref().unwrap();
    assert_eq!(restored.annotations[0].node, note.node);
    assert!(close3(restored.annotations[0].local_pos, note.local_pos));
    assert!(close3(
        restored.views[0].camera.target,
        author.camera.target
    ));
    assert!(near(
        restored.views[0].camera.distance,
        author.camera.distance
    ));

    // "z" と書いたレビューは、Y で開いている同じ STL を Z で開き直す。
    json["model"]["stl_up"] = "z".into();
    std::fs::write(&review, json.to_string()).unwrap();
    session.apply(Command::LoadReview(review.clone()));
    pump_load(&mut session);
    assert_eq!(
        session.source_kind,
        SourceKind::Stl {
            unit: UnitLabel::Mm,
            up: UpAxis::Z
        }
    );
    assert!(close3(extent(&session), sample_extent_m(0.001)));
    assert!(close3(
        session.review.as_ref().unwrap().annotations[0].local_pos,
        note.local_pos
    ));

    // 向きの記録が無いレビュー（向きを書かない版で保存したもの）は、Y が上として戻す。
    // 保存した視点は、その版の表示（Y が上）のままで合う。
    json["model"].as_object_mut().unwrap().remove("stl_up");
    std::fs::write(&review, json.to_string()).unwrap();
    session.apply(Command::LoadReview(review));
    pump_load(&mut session);
    assert_eq!(
        session.source_kind,
        SourceKind::Stl {
            unit: UnitLabel::Mm,
            up: UpAxis::Y
        }
    );
    assert!(close3(
        extent(&session),
        Vec3::from(STL_SAMPLE_SIZE) * 0.001
    ));
    let restored = session.review.as_ref().unwrap();
    assert!(close3(restored.annotations[0].local_pos, note.local_pos));
    assert!(close3(
        restored.views[0].camera.target,
        author.camera.target
    ));
}

#[test]
fn annotation_stays_on_the_same_surface_across_up() {
    let stl = write_bytes(&scratch("annotation-up"), "box.stl", &stl_sample());
    let file_point = Vec3::new(20.0, 8.0, 26.0);
    // 両方で +X の面の同じ場所を押す。画面での位置は上の向きに合わせて変わる。
    for (up, point_mm) in [
        (UpAxis::Y, file_point),
        (UpAxis::Z, Vec3::new(20.0, 26.0, -8.0)),
    ] {
        let mut session = open(&stl);
        session.apply(Command::SetStlUp(up));
        pump_load(&mut session);
        session.apply(Command::SnapView(ViewDir::Right));
        let ndc = session.camera.project(point_mm * 0.001, 1.0).unwrap();
        session.apply(Command::SetAnnotateMode(true));
        session.apply(Command::ViewClick { ndc });
        session.apply(Command::CommitDraft {
            text: "右面".into(),
        });
        let note = &session.review.as_ref().unwrap().annotations[0];
        assert_eq!(note.node, NodeId(0));
        assert!(
            close3(note.local_pos, file_point),
            "{up:?}: {:?}",
            note.local_pos
        );
        assert!(close3(note.local_normal, Vec3::X));
    }
}

#[test]
fn renamed_stl_restores_the_annotation_and_hidden_part_after_relink() {
    use model_reviewer_core::{error::ReviewError, review_file};

    let dir = scratch("renamed");
    let path = write_bytes(&dir, "before.stl", &stl_sample());
    let mut session = open(&path);
    annotate_center(&mut session, "角");
    session.apply(Command::SetVisible {
        node: NodeId(0),
        visible: false,
    });
    let review_path = dir.join("review.json");
    session.apply(Command::SaveReview(review_path.clone()));
    assert_eq!(session.status_text, "レビューを保存しました");
    let mut file = review_file::parse(&std::fs::read_to_string(&review_path).unwrap()).unwrap();
    let renamed = dir.join("after.stl");
    std::fs::rename(path, &renamed).unwrap();

    let mut restored = Session::default();
    restored.apply(Command::LoadReview(review_path));
    assert_eq!(restored.status_text, "モデルが見つかりません");
    restored.apply(Command::RelinkModel(renamed));
    pump_load(&mut restored);
    assert_eq!(restored.status_text, "レビューを復元しました");
    let doc = restored.doc.as_ref().unwrap();
    assert_eq!(doc.name(NodeId(0)), "after");
    let review = restored.review.unwrap();
    assert_eq!(review.annotations, session.review.unwrap().annotations);
    assert!(!review.visibility.effective(NodeId(0)));

    let identity = &restored.model_ref.unwrap().identity;
    file.hidden_nodes.clear();
    file.model.stl_unit = None;
    assert_eq!(
        review_file::restore(&file, doc, identity).unwrap_err(),
        ReviewError::UnknownNode { gltf_index: 0 }
    );
    file.model.stl_unit = Some(UnitLabel::Mm);
    file.annotations[0].node.gltf_index = 1;
    assert_eq!(
        review_file::restore(&file, doc, identity).unwrap_err(),
        ReviewError::UnknownNode { gltf_index: 1 }
    );
}

#[test]
fn changing_stl_up_reopens_after_confirm() {
    let stl = write_bytes(&scratch("confirm-up"), "box.stl", &stl_sample());
    let mut session = open(&stl);
    annotate_center(&mut session, "面");
    let before = Arc::clone(session.model.as_ref().unwrap());
    session.apply(Command::SetStlUp(UpAxis::Y));
    assert!(session.dialog.is_some() && session.loading_file_name.is_none());
    session.apply(Command::CancelPending);
    assert!(Arc::ptr_eq(&before, session.model.as_ref().unwrap()));
    assert_eq!(
        session.source_kind,
        SourceKind::Stl {
            unit: UnitLabel::Mm,
            up: UpAxis::Z
        }
    );
    assert_eq!(session.review.as_ref().unwrap().annotations.len(), 1);
    session.apply(Command::SetStlUp(UpAxis::Y));
    session.apply(Command::ConfirmDiscard);
    pump_load(&mut session);
    assert_eq!(
        session.source_kind,
        SourceKind::Stl {
            unit: UnitLabel::Mm,
            up: UpAxis::Y
        }
    );
    assert!(session.review.as_ref().unwrap().annotations.is_empty());
    assert!(session.history.is_empty());
}

#[test]
fn changing_unit_keeps_up_and_changing_up_keeps_unit() {
    let stl = write_bytes(&scratch("unit-and-up"), "box.stl", &stl_sample());
    let mut session = open(&stl);
    session.apply(Command::SetStlUp(UpAxis::Y));
    session.apply(Command::SetStlUnit(UnitLabel::M));
    assert!(session.status_text.contains("読み込み中"));
    pump_load(&mut session);
    assert_eq!(
        session.model_ref.as_ref().unwrap().stl_unit,
        Some(UnitLabel::Mm)
    );
    session.apply(Command::SetStlUnit(UnitLabel::Cm));
    pump_load(&mut session);
    assert_eq!(
        session.source_kind,
        SourceKind::Stl {
            unit: UnitLabel::Cm,
            up: UpAxis::Y
        }
    );
    assert!(close3(extent(&session), Vec3::from(STL_SAMPLE_SIZE) * 0.01));
    session.apply(Command::SetStlUp(UpAxis::Z));
    pump_load(&mut session);
    assert_eq!(
        session.source_kind,
        SourceKind::Stl {
            unit: UnitLabel::Cm,
            up: UpAxis::Z
        }
    );
    assert!(close3(extent(&session), sample_extent_m(0.01)));
}

#[test]
fn changing_stl_unit_reopens_after_confirm() {
    let dir = scratch("unit");
    let stl = write_bytes(&dir, "box.stl", &stl_sample());
    let mut session = open(&stl);
    // 未保存が無ければ、そのまま開き直す。
    session.apply(Command::SetStlUnit(UnitLabel::Cm));
    assert!(session.dialog.is_none());
    pump_load(&mut session);
    assert_eq!(
        session.source_kind,
        SourceKind::Stl {
            unit: UnitLabel::Cm,
            up: UpAxis::Z,
        }
    );
    assert_eq!(session.units.display, Some(UnitLabel::Cm));
    assert!(close3(extent(&session), sample_extent_m(0.01)));

    // 未保存（足した図形と注釈）があれば、先に確認する。やめれば何も変わらない。
    annotate_center(&mut session, "角");
    session.apply(Command::AddShape(ShapeKind::Cube));
    let nodes = session.doc.as_ref().unwrap().node_count();
    session.apply(Command::SetStlUnit(UnitLabel::M));
    let dialog = session.dialog.clone().expect("未保存の確認");
    assert!(
        dialog.body.contains("書き出していない形の編集"),
        "{}",
        dialog.body
    );
    session.apply(Command::CancelPending);
    pump_load(&mut session);
    assert_eq!(
        session.source_kind,
        SourceKind::Stl {
            unit: UnitLabel::Cm,
            up: UpAxis::Z,
        }
    );
    assert_eq!(session.doc.as_ref().unwrap().node_count(), nodes);
    assert_eq!(session.review.as_ref().unwrap().annotations.len(), 1);

    // 開き直しに失敗したら（ファイルが壊れていた）、前のモデルと編集が残る。
    std::fs::write(&stl, stl_inputs::truncated_ascii(60)).unwrap();
    session.apply(Command::SetStlUnit(UnitLabel::M));
    session.apply(Command::ConfirmDiscard);
    pump_load(&mut session);
    assert!(matches!(session.load, LoadPhase::Failed(_)));
    assert_eq!(session.doc.as_ref().unwrap().node_count(), nodes);
    assert_eq!(session.review.as_ref().unwrap().annotations.len(), 1);
    assert_eq!(
        session.source_kind,
        SourceKind::Stl {
            unit: UnitLabel::Cm,
            up: UpAxis::Z,
        }
    );
    session.apply(Command::DismissDialog);

    // 確かめて破棄すると、新しい単位で開き直す。履歴・計測・断面は消える。
    std::fs::write(&stl, stl_sample()).unwrap();
    session.apply(Command::ToggleSection);
    session.apply(Command::SetStlUnit(UnitLabel::M));
    assert!(session.dialog.is_some());
    session.apply(Command::ConfirmDiscard);
    pump_load(&mut session);
    assert_eq!(
        session.source_kind,
        SourceKind::Stl {
            unit: UnitLabel::M,
            up: UpAxis::Z
        }
    );
    assert_eq!(session.units.display, Some(UnitLabel::M));
    assert!(close3(extent(&session), sample_extent_m(1.0)));
    assert_eq!(session.doc.as_ref().unwrap().node_count(), 1);
    assert!(session.history.is_empty());
    assert!(session.section.is_none());
    assert!(!session.has_unsaved());

    // GLB では何もしない。書き出し中も開き直さない。
    let mut glb = Session::default();
    glb.apply(Command::SetStlUnit(UnitLabel::Cm));
    assert!(glb.dialog.is_none() && glb.loading_file_name.is_none());
}

/// STL を開き、円柱を足して寸法を決め、計測と断面を置いた状態。
fn edited_stl(dir: &Path) -> (Session, PathBuf) {
    let stl = write_bytes(dir, "part.stl", &stl_sample());
    let mut session = open(&stl);
    session.apply(Command::AddShape(ShapeKind::Cylinder));
    let shape = session.selection.expect("足した円柱");
    let view = selection_dims(&session).unwrap().shape.unwrap();
    let scale = scale_for(
        view.spec,
        Dims::Cylinder { d: 0.01, h: 0.015 },
        view.trs.scale.y,
    )
    .unwrap();
    session.apply(Command::CommitLocal(model_reviewer::command::Trs {
        scale,
        ..view.trs
    }));
    let Some(Dims::Cylinder { d, h }) = selection_dims(&session).unwrap().shape.unwrap().dims
    else {
        panic!("円柱の寸法");
    };
    assert!(near(d, 0.01) && near(h, 0.015), "{d} {h}");
    assert_eq!(session.selection, Some(shape));
    session.apply(Command::Select(Some(NodeId(0))));
    session.apply(Command::SetSection(Some(SectionPlane {
        axis: Axis::X,
        offset: 0.01,
        flipped: false,
    })));
    (session, stl)
}

#[test]
fn stl_exports_to_glb_and_moves_target() {
    let dir = scratch("export");
    let (mut session, stl) = edited_stl(&dir);
    let target = dir.join("part.glb");
    session.apply(Command::ExportGlb(target.clone()));
    pump_export(&mut session);
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );
    assert_eq!(session.source_kind, SourceKind::Glb);
    let model_ref = session.model_ref.as_ref().unwrap();
    assert!(model_ref.path.ends_with("part.glb"));
    assert_eq!(model_ref.stl_unit, None);
    assert_eq!(model_ref.stl_up, None);
    assert!(!session.doc_unsaved());
    let body = failure_body(&session);
    assert!(
        body.contains(&stl_export_note(UnitLabel::Mm, UpAxis::Z)),
        "{body}"
    );
    assert!(body.contains("0.001"), "{body}");
    // 単位は変えない（シーンはメートルのまま、表示は mm）。
    assert_eq!(
        session.units,
        Units {
            source: UnitLabel::M,
            display: Some(UnitLabel::Mm)
        }
    );
    assert_eq!(&std::fs::read(&target).unwrap()[..4], b"glTF");
    assert_eq!(std::fs::read(&stl).unwrap(), stl_sample());
    // 書き出した後も、同じ作業の間は足した図形の寸法を直せる（要件9.6）。
    session.apply(Command::Select(Some(NodeId(1))));
    let shape = selection_dims(&session).unwrap().shape.expect("寸法の欄");
    assert!(shape.dims.is_some() && shape.blocked.is_none());
    // 次の書き出しは新しい GLB が土台。STL の単位を替える操作は、もう働かない。
    session.apply(Command::SetStlUnit(UnitLabel::Cm));
    session.apply(Command::SetStlUp(UpAxis::Y));
    assert!(session.loading_file_name.is_none());
    let mut trs = shape.trs;
    trs.translation.x += 0.005;
    session.apply(Command::CommitLocal(trs));
    let second = dir.join("part-2.glb");
    session.apply(Command::ExportGlb(second.clone()));
    pump_export(&mut session);
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );
    assert!(!failure_body_or_empty(&session).contains("STL から"));
    // 開いた STL へは書き出さない。
    session.apply(Command::ExportGlb(stl.clone()));
    pump_export(&mut session);
    assert!(session.export_error().is_some());
    assert_eq!(std::fs::read(&stl).unwrap(), stl_sample());
}

fn failure_body_or_empty(session: &Session) -> String {
    session
        .dialog
        .as_ref()
        .map(|dialog| dialog.body.clone())
        .unwrap_or_default()
}

#[cfg(unix)]
fn check_native_model_path(dir: &Path, name: &[u8], bytes: &[u8]) {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let path = dir.join(OsStr::from_bytes(name));
    if let Err(err) = std::fs::write(&path, bytes) {
        // macOS の保存先によっては名前自体を拒む（EILSEQ は Darwin で 92）。
        #[cfg(target_os = "macos")]
        if matches!(
            err.kind(),
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::InvalidInput
        ) || err.raw_os_error() == Some(92)
        {
            eprintln!("非UTF-8名の作成が拒まれたため、表示名との取り違えを検査します（{err}）");
            check_missing_native_path(dir, &path, bytes);
            return;
        }
        panic!("モデルを用意できません（{err}）");
    }
    let mut session = open(&path);
    if matches!(session.source_kind, SourceKind::Stl { .. }) {
        session.apply(Command::SetStlUnit(UnitLabel::Cm));
        pump_load(&mut session);
        assert_eq!(session.status_text, "読み込み完了");
        assert!(close3(extent(&session), sample_extent_m(0.01)));
    }
    let review_path = dir.join("review.json");
    session.apply(Command::SaveReview(review_path.clone()));
    assert_eq!(session.status_text, "レビューを保存しました");
    let text = std::fs::read_to_string(review_path).unwrap();
    let file = model_reviewer_core::review_file::parse(&text).unwrap();
    assert_eq!(file.model.file_name, String::from_utf8_lossy(name));

    let exported = dir.join(OsStr::from_bytes(b"export-\xfd.glb"));
    session.apply(Command::ExportGlb(exported.clone()));
    pump_export(&mut session);
    assert_eq!(session.export_error(), None);
    let glb = std::fs::read(&exported).unwrap();
    assert!(model_reviewer_gltf::load_glb(&glb).is_ok());
    session.apply(Command::SaveReview(exported.clone()));
    assert!(failure_body(&session).contains("モデルのファイルへはレビューを保存できません"));
    assert_eq!(std::fs::read(exported).unwrap(), glb);

    let second = dir.join("second.glb");
    session.apply(Command::ExportGlb(second.clone()));
    pump_export(&mut session);
    assert_eq!(session.export_error(), None);
    assert!(model_reviewer_gltf::load_glb(&std::fs::read(second).unwrap()).is_ok());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

/// 元の実パスを読み直せなくても、同じ表示名の別ファイルを代わりに読んではいけない。
#[cfg(target_os = "macos")]
fn check_missing_native_path(dir: &Path, path: &Path, bytes: &[u8]) {
    let printed = dir.join(path.file_name().unwrap().to_string_lossy().as_ref());
    std::fs::write(&printed, bytes).unwrap();
    let mut session = open(&printed);
    session.model_ref.as_mut().unwrap().path = path.to_owned();
    session.apply(Command::SaveReview(path.to_owned()));
    assert!(failure_body(&session).contains("モデルのファイルへはレビューを保存できません"));
    session.apply(Command::ExportGlb(dir.join("export.glb")));
    pump_export(&mut session);
    assert!(
        session
            .export_error()
            .unwrap()
            .contains("元のファイルを読めません")
    );
    if matches!(session.source_kind, SourceKind::Stl { .. }) {
        session.apply(Command::SetStlUnit(UnitLabel::Cm));
        pump_load(&mut session);
        assert!(matches!(session.load, LoadPhase::Failed(_)));
    }
    assert_eq!(std::fs::read(printed).unwrap(), bytes);
}

#[cfg(unix)]
#[test]
fn non_utf8_stl_name_is_used_for_reopening() {
    check_native_model_path(&scratch("non-utf8-stl"), b"box-\xff.stl", &stl_sample());
}

#[cfg(unix)]
#[test]
fn non_utf8_glb_name_is_used_for_reopening() {
    check_native_model_path(
        &scratch("non-utf8-glb"),
        b"mesh-\xfe.glb",
        &model_reviewer_testdata::sample_glb(),
    );
}

#[test]
fn stl_export_refuses_a_changed_stl() {
    let dir = scratch("changed");
    let (mut session, stl) = edited_stl(&dir);
    let changed = ascii_stl(&[("box", &box_triangles([1.0, 2.0, 3.0]))], "\n");
    std::fs::write(&stl, &changed).unwrap();
    let target = dir.join("part.glb");
    session.apply(Command::ExportGlb(target.clone()));
    pump_export(&mut session);
    assert!(session.export_error().unwrap().contains("変わっている"));
    assert!(!target.exists());
    assert_eq!(
        session.source_kind,
        SourceKind::Stl {
            unit: UnitLabel::Mm,
            up: UpAxis::Z,
        }
    );
    assert!(session.doc_unsaved());
    assert_eq!(std::fs::read(&stl).unwrap(), changed);
}

#[test]
fn scene_numbers_do_not_change_across_stl_export() {
    let dir = scratch("numbers");
    let (mut session, _) = edited_stl(&dir);
    // 計測（部品0の角から角）を置く。
    session.apply(Command::SetTool(model_reviewer::command::Tool::Measure));
    session.apply(Command::SetSnap(false));
    let doc = session.doc.as_ref().unwrap();
    let locals: Vec<Affine3A> = doc.nodes().iter().map(|node| node.local).collect();
    let worlds: Vec<Affine3A> = doc.nodes().iter().map(|node| node.world).collect();
    let bounds = doc.bounds();
    let camera = session.camera;
    let section = session.section;
    let views_before = session.review.as_ref().unwrap().views.len();
    session.apply(Command::ViewClick { ndc: Vec2::ZERO });
    session.apply(Command::ViewClick {
        ndc: Vec2::new(0.05, 0.05),
    });
    let distance = model_reviewer::session::measure_distance(&session).expect("距離");

    session.apply(Command::ExportGlb(dir.join("numbers.glb")));
    pump_export(&mut session);
    assert!(
        session.export_error().is_none(),
        "{:?}",
        session.export_error()
    );

    let doc = session.doc.as_ref().unwrap();
    // 文書の部品0・描画0・形状0が、合成した土台の番号と合う。
    assert_eq!(doc.file_index(NodeId(0)), Some(0));
    assert_eq!(doc.draws(NodeId(0))[0].prim, PrimId(0));
    assert_eq!(doc.base().primitives.len(), 1);
    for (index, node) in doc.nodes().iter().enumerate() {
        let (a, b) = (node.local.to_cols_array(), locals[index].to_cols_array());
        for (x, y) in a.iter().zip(b) {
            assert!(near(*x, y), "部品 {index} の local: {x} と {y}");
        }
        let (a, b) = (node.world.to_cols_array(), worlds[index].to_cols_array());
        for (x, y) in a.iter().zip(b) {
            assert!(near(*x, y), "部品 {index} の world: {x} と {y}");
        }
    }
    // STL の部品の倍率（0.001）が残る。
    let trs = decompose(&doc.node(NodeId(0)).unwrap().local).unwrap();
    assert!(close3(trs.scale, Vec3::splat(0.001)));
    assert!(close3(doc.bounds().min, bounds.min) && close3(doc.bounds().max, bounds.max));
    assert_eq!(session.camera, camera);
    assert_eq!(session.section, section);
    let after = model_reviewer::session::measure_distance(&session).expect("距離");
    assert!(near(after, distance), "{after} と {distance}");
    assert_eq!(session.review.as_ref().unwrap().views.len(), views_before);
}

#[test]
fn z_up_stl_exports_rotation() {
    let dir = scratch("export-up");
    let stl = write_bytes(&dir, "box.stl", &stl_sample());
    for up in [UpAxis::Z, UpAxis::Y] {
        let mut session = open(&stl);
        session.apply(Command::SetStlUp(up));
        pump_load(&mut session);
        let before = extent(&session);
        let shown = |s: &Session| {
            extent(s)
                .to_array()
                .map(|v| s.units.format(v, model_reviewer::ui::dims::PANEL_DIGITS))
        };
        let labels = shown(&session);
        let (base_tx, base_rx) = std::sync::mpsc::channel();
        session.set_export_hooks(model_reviewer::exporter::ExportHooks {
            after_stl_base: Some(Arc::new(move |bytes| base_tx.send(bytes.to_vec()).unwrap())),
            ..Default::default()
        });
        let target = dir.join(format!("{}.glb", up.key()));
        session.apply(Command::ExportGlb(target.clone()));
        session.apply(Command::SetStlUp(up.other()));
        assert!(session.status_text.contains("書き出し中"));
        assert!(session.loading_file_name.is_none());
        pump_export(&mut session);
        assert_eq!(session.export_error(), None);
        assert_eq!(
            failure_body(&session).contains(Z_OPENING_NOTE),
            up == UpAxis::Z
        );
        let base =
            model_reviewer_gltf::load_glb(&base_rx.try_recv().expect("合成直後の土台")).unwrap();
        let scale = 0.001;
        let expected = Affine3A::from_mat3(up.rotation() * scale);
        let local = base.nodes[0].local;
        assert!(
            local.abs_diff_eq(expected, scale * 1.0e-6),
            "{up:?} が上の土台の回転と倍率: {local:?}"
        );
        let bytes = std::fs::read(&target).unwrap();
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let json: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
        let node = &json["nodes"][0];
        assert!(node.get("matrix").is_none());
        assert_eq!(node.get("rotation").is_some(), up == UpAxis::Z);
        assert!(node["scale"].is_array());
        let mut reopened = open(&target);
        let local = reopened.doc.as_ref().unwrap().nodes()[0].local;
        let z = if up == UpAxis::Z { Vec3::Y } else { Vec3::Z };
        assert!(close3(local.transform_vector3(Vec3::Z), z * 0.001));
        assert!(close3(extent(&reopened), before));
        reopened.apply(Command::SetDisplayUnit(Some(UnitLabel::Mm)));
        assert_eq!(shown(&reopened), labels);
    }
}

#[test]
fn export_note_describes_initial_rotation_after_edit() {
    let dir = scratch("edited-rotation");
    let stl = write_bytes(&dir, "box.stl", &stl_sample());
    let mut session = open(&stl);
    session.apply(Command::Select(Some(NodeId(0))));
    let mut trs = decompose(&session.doc.as_ref().unwrap().nodes()[0].local).unwrap();
    trs.rotation = glam::Quat::IDENTITY;
    session.apply(Command::CommitLocal(trs));

    let target = dir.join("edited.glb");
    session.apply(Command::ExportGlb(target.clone()));
    pump_export(&mut session);
    assert_eq!(session.export_error(), None);
    let body = failure_body(&session);
    assert!(body.contains(Z_OPENING_NOTE), "{body}");
    let model = model_reviewer_gltf::load_glb(&std::fs::read(target).unwrap()).unwrap();
    let expected = Affine3A::from_scale(Vec3::splat(0.001));
    assert!(model.nodes[0].local.abs_diff_eq(expected, 0.001 * 1.0e-6));
}

#[test]
fn exported_glb_from_stl_reopens_with_same_extent() {
    let dir = scratch("reopen");
    let (mut session, _) = edited_stl(&dir);
    let target = dir.join("reopen.glb");
    session.apply(Command::ExportGlb(target.clone()));
    pump_export(&mut session);
    assert!(session.export_error().is_none());
    let reopened = open(&target);
    assert_eq!(reopened.source_kind, SourceKind::Glb);
    let (a, b) = (
        session.doc.as_ref().unwrap(),
        reopened.doc.as_ref().unwrap(),
    );
    assert_eq!(a.node_count(), b.node_count());
    for index in 0..a.node_count() {
        let id = NodeId(index as u32);
        assert_eq!(a.name(id), b.name(id));
        let (pa, pb) = (
            a.node(id).unwrap().world_aabb,
            b.node(id).unwrap().world_aabb,
        );
        assert!(
            close3(pa.min, pb.min) && close3(pa.max, pb.max),
            "部品 {index}"
        );
    }
    assert!(close3(extent(&reopened), extent(&session)));
    let trs = decompose(&b.node(NodeId(0)).unwrap().local).unwrap();
    assert!(close3(trs.scale, Vec3::splat(0.001)));
    // 合成した土台を開き直すと「法線の無い形状」の簡略表示が出る（事実なので、そのまま）。
    assert!(!reopened.model.as_ref().unwrap().warnings.is_empty());
}

#[test]
fn exported_stl_status_fits_at_960() {
    use model_reviewer::ui::ids;

    let dir = scratch("export-status");
    let name = "W".repeat(100);
    let stl = write_bytes(&dir, &format!("{name}.stl"), &stl_sample());
    let mut session = open(&stl);
    session.apply(Command::Select(Some(NodeId(0))));
    session.apply(Command::ExportGlb(stl.with_extension("glb")));
    pump_export(&mut session);
    assert_eq!(session.source_kind, SourceKind::Glb);
    session.apply(Command::DismissDialog);

    let size = egui::vec2(960.0, 600.0);
    let (mut harness, _) = support::harness(session, size);
    harness.run_steps(4);
    let details = support::widget_rect(&harness, ids::STATUS_DETAILS).expect("詳細ボタン");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    assert!(screen.contains_rect(details), "詳細が画面の外: {details:?}");
}

#[test]
fn export_dialog_suggests_stl_name_with_glb_extension() {
    let dir = scratch("name");
    let stl = write_bytes(&dir, "bracket.stl", &stl_sample());
    let session = open(&stl);
    assert_eq!(
        model_reviewer::export_file_name(&session).as_deref(),
        Some("bracket.glb")
    );
    let glb = Session::default();
    assert_eq!(model_reviewer::export_file_name(&glb), None);
}
