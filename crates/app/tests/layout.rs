//! 詳細パネルの大きさと位置。GPUは使わない。

use std::sync::Arc;

use egui_kittest::Harness;
use model_reviewer::automation::WidgetFrame;
use model_reviewer::session::Session;
use model_reviewer::ui::{details, ids};
use model_reviewer_core::review::ReviewState;
use model_reviewer_core::scene::NodeId;
use model_reviewer_core::units::{UnitLabel, UpAxis};

fn sample_session() -> Session {
    let bytes = model_reviewer_testdata::sample_glb();
    let model = Arc::new(model_reviewer_gltf::load_glb(&bytes).expect("見本のGLB"));
    let mut session = Session::default();
    session.review = Some(ReviewState::new(&model));
    session.set_model(model);
    session.selection = Some(NodeId(11));
    session.panel_details = true;
    session
}

fn stl_session_with_warning() -> Session {
    use model_reviewer::session::SourceKind;
    use model_reviewer_core::identity::{ModelIdentity, ModelRef};
    use model_reviewer_stl::{StlOptions, load_stl};

    let mut bytes = model_reviewer_testdata::stl_sample();
    bytes.push(0); // 末尾の余分な1バイトで簡略表示の知らせを出す。
    let options = StlOptions {
        name: "review-sample",
        unit: UnitLabel::Mm,
        up: UpAxis::Z,
        allow_trailing: true,
    };
    let model = Arc::new(load_stl(&bytes, &options).unwrap());
    let mut session = Session::default();
    session.review = Some(ReviewState::new(&model));
    session.set_model(model);
    session.source_kind = SourceKind::Stl {
        unit: options.unit,
        up: options.up,
    };
    session.model_ref = Some(ModelRef {
        file_name: "review-sample.stl".into(),
        path: "review-sample.stl".into(),
        relative_path: None,
        identity: ModelIdentity::from_bytes(&bytes),
        stl_unit: Some(options.unit),
        stl_up: Some(options.up),
    });
    session
}

fn harness(size: egui::Vec2) -> Harness<'static, Session> {
    Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_size(size)
        .with_context_setup(model_reviewer::fonts::install)
        .build_ui_state(
            |ui, session| {
                let mut commands = Vec::new();
                model_reviewer::ui::show(ui, session, &mut commands);
                for command in commands {
                    session.apply(command);
                }
            },
            sample_session(),
        )
}

fn widget_rect(harness: &Harness<Session>, name: &str) -> egui::Rect {
    let frame = harness
        .ctx
        .data(|data| data.get_temp::<WidgetFrame>(egui::Id::NULL))
        .unwrap_or_default();
    let entry = frame
        .entries()
        .iter()
        .find(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("部品が無い: {name}"));
    egui::Rect::from_min_max(
        egui::pos2(entry.min[0], entry.min[1]),
        egui::pos2(entry.max[0], entry.max[1]),
    )
}

#[test]
fn details_panel_has_a_fixed_height_and_stays_above_the_status_bar() {
    for size in [egui::vec2(1280.0, 800.0), egui::vec2(960.0, 600.0)] {
        let mut harness = harness(size);
        for _ in 0..6 {
            harness.step();
        }
        let details_rect = widget_rect(&harness, ids::PANEL_DETAILS);
        let status_rect = widget_rect(&harness, ids::PANEL_STATUS);
        assert!(
            details_rect.max.y <= status_rect.min.y + 0.5,
            "{size:?}: 詳細 {details_rect:?} が状態欄 {status_rect:?} に重なる"
        );
        assert!(
            status_rect.max.y <= size.y + 0.5,
            "{size:?}: 状態欄が画面の外"
        );
        let expected = details::panel_height(size.y);
        assert!(
            (details_rect.height() - expected).abs() <= 2.0,
            "{size:?}: 高さ {} が {expected} でない",
            details_rect.height()
        );
    }
    assert!((details::panel_height(800.0) - 280.0).abs() < 0.01);
    assert!((details::panel_height(600.0) - 210.0).abs() < 0.01);
}

fn entries(harness: &Harness<Session>) -> Vec<(String, egui::Rect)> {
    let frame = harness
        .ctx
        .data(|data| data.get_temp::<WidgetFrame>(egui::Id::NULL))
        .unwrap_or_default();
    frame
        .entries()
        .iter()
        .map(|entry| {
            (
                entry.name.clone(),
                egui::Rect::from_min_max(
                    egui::pos2(entry.min[0], entry.min[1]),
                    egui::pos2(entry.max[0], entry.max[1]),
                ),
            )
        })
        .collect()
}

/// 上部の操作・3Dビュー上端のボタンと立方体・状態欄が、幅960でも互いに重ならず画面に収まる。
#[test]
fn top_controls_and_status_bar_fit_without_overlap_at_960() {
    for (size, details_open) in [
        (egui::vec2(960.0, 600.0), false),
        (egui::vec2(960.0, 600.0), true),
        (egui::vec2(1280.0, 800.0), false),
    ] {
        let mut session = sample_session();
        session.panel_details = details_open;
        let mut harness = Harness::builder()
            .allow_missing_glyphs()
            .with_accessibility_check(false)
            .with_size(size)
            .with_context_setup(model_reviewer::fonts::install)
            .build_ui_state(
                |ui, session| {
                    let mut commands = Vec::new();
                    let pixels = model_reviewer::ui::show(ui, session, &mut commands);
                    for command in commands {
                        session.apply(command);
                    }
                    session.set_viewport(pixels);
                },
                session,
            );
        for _ in 0..6 {
            harness.step();
        }
        let all = entries(&harness);
        let status = widget_rect(&harness, ids::PANEL_STATUS);
        let viewport = widget_rect(&harness, ids::VIEWPORT);
        assert!(
            viewport.max.y <= status.min.y + 0.5,
            "{size:?}: 3Dビューが状態欄に重なる"
        );
        let top: Vec<_> = all
            .iter()
            .filter(|(name, _)| name.starts_with("toolbar."))
            .collect();
        assert!(!top.is_empty());
        for (name, rect) in &top {
            assert!(
                rect.max.x <= size.x + 0.5 && rect.min.x >= -0.5,
                "{size:?}: {name} {rect:?} が画面の外"
            );
            assert!(
                rect.max.y <= viewport.min.y + 0.5 && rect.max.y <= status.min.y,
                "{size:?}: {name} {rect:?} が3Dビューか状態欄に重なる"
            );
        }
        for (name, rect) in all.iter().filter(|(name, _)| name.starts_with("status.")) {
            assert!(
                rect.max.x <= size.x + 0.5 && rect.min.y >= status.min.y - 0.5,
                "{size:?}: 状態欄の {name} {rect:?} がはみ出す"
            );
        }
        let bar: Vec<_> = all
            .iter()
            .filter(|(name, _)| name.starts_with("view."))
            .collect();
        let cube: Vec<_> = all
            .iter()
            .filter(|(name, _)| name.starts_with("viewcube."))
            .collect();
        assert_eq!(
            bar.len(),
            9,
            "{size:?}: 上端のボタンが9つ（画像と断面を含む）"
        );
        assert!(!cube.is_empty(), "{size:?}: 立方体の面が登録されていない");
        for (name, rect) in bar.iter().chain(cube.iter()) {
            assert!(
                viewport.contains_rect(*rect),
                "{size:?}: {name} {rect:?} が3Dビュー {viewport:?} の外"
            );
        }
        for (bar_name, bar_rect) in &bar {
            for (cube_name, cube_rect) in &cube {
                assert!(
                    !bar_rect.intersects(*cube_rect),
                    "{size:?}: {bar_name} と {cube_name} が重なる"
                );
            }
        }
        // 上部のボタン（戻す・やり直すを含む）は互いに重ならない。
        for (i, (a_name, a_rect)) in top.iter().enumerate() {
            for (b_name, b_rect) in &top[i + 1..] {
                assert!(
                    !a_rect.shrink(0.5).intersects(b_rect.shrink(0.5)),
                    "{size:?}: {a_name} と {b_name} が重なる"
                );
            }
        }
        assert!(top.iter().any(|(name, _)| name == ids::TOOLBAR_EXPORT));
        assert!(top.iter().any(|(name, _)| name == ids::TOOLBAR_UNDO));
        assert!(top.iter().any(|(name, _)| name == ids::TOOLBAR_REDO));
        // 道具の列は3Dビューの中の左端にあり、上端のボタンと立方体に重ならない。
        let rail: Vec<_> = all
            .iter()
            .filter(|(name, _)| name.starts_with("tool."))
            .collect();
        assert_eq!(rail.len(), 7, "{size:?}: 道具が7つ");
        for (name, rect) in &rail {
            assert!(
                viewport.contains_rect(*rect),
                "{size:?}: {name} {rect:?} が3Dビュー {viewport:?} の外"
            );
            assert!(
                rect.min.x < viewport.min.x + 80.0,
                "{size:?}: {name} が左端にない"
            );
            for (other, other_rect) in bar.iter().chain(cube.iter()) {
                assert!(
                    !rect.intersects(*other_rect),
                    "{size:?}: {name} と {other} が重なる"
                );
            }
        }
        // 「変形」の欄は右のパネルの中で、状態欄・詳細より上に収まる。
        let fields: Vec<_> = all
            .iter()
            .filter(|(name, _)| name.starts_with("inspector."))
            .collect();
        assert_eq!(fields.len(), 9, "{size:?}: 変形の欄が9つ");
        let bottom = if details_open {
            widget_rect(&harness, ids::PANEL_DETAILS).min.y
        } else {
            status.min.y
        };
        for (name, rect) in &fields {
            assert!(
                rect.min.x >= viewport.max.x - 0.5 && rect.max.x <= size.x + 0.5,
                "{size:?}: {name} {rect:?} が右のパネルの外"
            );
            assert!(
                rect.max.y <= bottom + 0.5,
                "{size:?}: {name} {rect:?} が下の領域 {bottom} に重なる"
            );
        }
    }
}

#[test]
fn texture_button_and_progress_fit_at_960() {
    use egui_kittest::kittest::Queryable;
    let mut h = harness(egui::vec2(960.0, 600.0));
    h.state_mut().textures.total = 24;
    h.state_mut().textures.done = 3;
    h.step();
    h.step();
    let button = widget_rect(&h, ids::VIEW_TEXTURES);
    let progress = h.get_by_label("テクスチャ 3/24").rect();
    assert!(widget_rect(&h, ids::VIEWPORT).contains_rect(button));
    assert!(widget_rect(&h, ids::PANEL_STATUS).contains_rect(progress));
    for (name, other) in entries(&h) {
        if name.starts_with("status.") && name != ids::STATUS_TEXTURES {
            assert!(!other.intersects(progress), "{name}");
        }
    }
    assert!(!button.intersects(progress));
}

/// 2つの未保存の印と長い状態文が同時に出た状態でも、幅960で状態欄がはみ出さず重ならない。
#[test]
fn status_bar_with_both_unsaved_badges_fits_at_960() {
    use egui_kittest::kittest::Queryable;
    use model_reviewer::command::Command;
    use model_reviewer_core::gizmo::decompose;

    let size = egui::vec2(960.0, 600.0);
    let mut session = sample_session();
    let arm = NodeId(9);
    let mut trs = decompose(&session.doc.as_ref().unwrap().node(arm).unwrap().local).unwrap();
    trs.translation.x += 0.5;
    session.apply(Command::Select(Some(arm)));
    session.apply(Command::CommitLocal(trs));
    session.apply(Command::SetVisible {
        node: NodeId(1),
        visible: false,
    });
    session.status_text =
        "書き出しました（作業の対象を long-exported-model-file-name.glb へ切り替えました）"
            .to_owned();
    assert!(session.doc_unsaved());
    assert!(session.review_unsaved());

    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_size(size)
        .with_context_setup(model_reviewer::fonts::install)
        .build_ui_state(
            |ui, session| {
                let mut commands = Vec::new();
                let pixels = model_reviewer::ui::show(ui, session, &mut commands);
                for command in commands {
                    session.apply(command);
                }
                session.set_viewport(pixels);
            },
            session,
        );
    for _ in 0..6 {
        harness.step();
    }
    let status = widget_rect(&harness, ids::PANEL_STATUS);
    let details_btn = widget_rect(&harness, ids::STATUS_DETAILS);
    assert!(
        details_btn.max.x <= size.x + 0.5 && details_btn.min.y >= status.min.y - 0.5,
        "詳細ボタンがはみ出す: {details_btn:?} status={status:?}"
    );
    let doc_badge = harness.get_by_label("●形の編集 未書き出し").rect();
    let review_badge = harness.get_by_label("●レビュー未保存").rect();
    assert!(
        doc_badge.max.x <= review_badge.min.x + 0.5,
        "2つの未保存の印が重なる: {doc_badge:?} / {review_badge:?}"
    );
    assert!(
        review_badge.max.x <= details_btn.min.x,
        "未保存の印が詳細ボタンに重なる: {review_badge:?} / {details_btn:?}"
    );
}

/// 計測の距離・単位・格子・部品の数・未保存の印が同時に出ても、幅960で重ならない。単位を押すと
/// 添える文字だけが替わる。
#[test]
fn status_bar_with_distance_and_unit_fits_at_960_and_the_unit_cycles() {
    use egui_kittest::kittest::Queryable;
    use model_reviewer::command::{Command, Tool};
    use model_reviewer_core::doc::NodeKey;
    use model_reviewer_core::measure::MeasurePoint;
    use model_reviewer_core::units::UnitLabel;

    let size = egui::vec2(960.0, 600.0);
    let mut session = sample_session();
    session.apply(Command::SetTool(Tool::Measure));
    session.apply(Command::SetVisible {
        node: NodeId(1),
        visible: false,
    });
    for node in [1, 9] {
        session.measurement.push(MeasurePoint {
            node: NodeKey(node),
            local: glam::Vec3::ZERO,
        });
    }
    session.status_text = "計測: もう1点をクリック".to_owned();
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_size(size)
        .with_context_setup(model_reviewer::fonts::install)
        .build_ui_state(
            |ui, session| {
                let mut commands = Vec::new();
                let pixels = model_reviewer::ui::show(ui, session, &mut commands);
                for command in commands {
                    session.apply(command);
                }
                session.set_viewport(pixels);
            },
            session,
        );
    for _ in 0..6 {
        harness.step();
    }
    let distance = model_reviewer::session::measure_distance(harness.state()).unwrap();
    let label = format!(
        "距離 {}",
        model_reviewer_core::units::format_distance(distance, UnitLabel::M)
    );
    let status = widget_rect(&harness, ids::PANEL_STATUS);
    let mut rects = vec![
        harness.get_by_label(&label).rect(),
        widget_rect(&harness, ids::STATUS_UNIT),
        widget_rect(&harness, ids::STATUS_DETAILS),
        harness.get_by_label("●レビュー未保存").rect(),
    ];
    rects.push(harness.get_by_label_contains("計測は保存されません").rect());
    for (i, a) in rects.iter().enumerate() {
        assert!(
            a.max.x <= size.x + 0.5 && a.min.y >= status.min.y - 0.5,
            "{a:?}"
        );
        for b in &rects[i + 1..] {
            assert!(
                !a.shrink(0.5).intersects(b.shrink(0.5)),
                "重なる: {a:?} {b:?}"
            );
        }
    }
    // 単位: m → cm → mm → m。数値の部分は同じ。
    for unit in [UnitLabel::Cm, UnitLabel::Mm, UnitLabel::M] {
        let center = widget_rect(&harness, ids::STATUS_UNIT).center();
        harness.event(egui::Event::PointerMoved(center));
        harness.step();
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: center,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        harness.step();
        harness.step();
        assert_eq!(harness.state().units.source, unit);
        let text = format!(
            "距離 {}",
            model_reviewer_core::units::format_distance(distance, unit)
        );
        assert!(harness.query_by_label(&text).is_some(), "{text}");
        assert!(
            harness
                .query_by_label(&format!("1単位 = {}", unit.suffix()))
                .is_some()
        );
        assert!(!harness.state().doc_unsaved());
    }
}

/// 第3段階の状態欄: 前提の単位・表示の単位・距離（mm）・格子・部品の数・2つの未保存の印・選択名が
/// 同時に出ても、幅960で重ならない。幅が足りなければ「格子」「表示の単位」「部品の数」の順に省き、
/// 前提の単位と未保存の印は省かない。広い幅ではすべて出る。
#[test]
fn status_bar_with_display_unit_fits_at_960() {
    use egui_kittest::kittest::Queryable;
    use model_reviewer::command::{Command, Tool};
    use model_reviewer_core::doc::NodeKey;
    use model_reviewer_core::measure::MeasurePoint;
    for (size, heavy, stl) in [
        (egui::vec2(960.0, 600.0), true, false),
        (egui::vec2(960.0, 600.0), false, false),
        (egui::vec2(1280.0, 800.0), true, false),
        (egui::vec2(960.0, 600.0), true, true),
    ] {
        let mut session = if stl {
            stl_session_with_warning()
        } else {
            sample_session()
        };
        session.panel_details = false;
        session.apply(Command::SetTool(Tool::Measure));
        session.apply(Command::SetDisplayUnit(Some(UnitLabel::Mm)));
        if heavy {
            session.apply(Command::SetVisible {
                node: NodeId(if stl { 0 } else { 1 }),
                visible: false,
            });
            session.apply(Command::AddShape(
                model_reviewer_core::shapes::ShapeKind::Cube,
            ));
            session.apply(Command::SetTool(Tool::Measure));
        } else {
            session.selection = None;
        }
        let points = if stl {
            [(0, glam::Vec3::ZERO), (0, glam::Vec3::X * 20.0)]
        } else {
            [(1, glam::Vec3::ZERO), (9, glam::Vec3::ZERO)]
        };
        for (node, local) in points {
            session.measurement.push(MeasurePoint {
                node: NodeKey(node),
                local,
            });
        }
        session.status_text = "計測: もう1点をクリック".to_owned();
        let mut harness = Harness::builder()
            .allow_missing_glyphs()
            .with_accessibility_check(false)
            .with_size(size)
            .with_context_setup(model_reviewer::fonts::install)
            .build_ui_state(
                |ui, session| {
                    let mut commands = Vec::new();
                    let pixels = model_reviewer::ui::show(ui, session, &mut commands);
                    for command in commands {
                        session.apply(command);
                    }
                    session.set_viewport(pixels);
                },
                session,
            );
        for _ in 0..6 {
            harness.step();
        }
        let session = harness.state();
        assert_eq!(session.doc_unsaved() && session.review_unsaved(), heavy);
        let distance = model_reviewer::session::measure_distance(session).unwrap();
        let label = format!("距離 {}", session.units.format(distance, 4));
        assert!(label.ends_with(" mm"), "{label}");
        let status = widget_rect(&harness, ids::PANEL_STATUS);
        let mut rects = vec![
            ("距離", harness.get_by_label(&label).rect()),
            ("前提", widget_rect(&harness, ids::STATUS_UNIT)),
            ("詳細", widget_rect(&harness, ids::STATUS_DETAILS)),
            (
                "知らせ",
                harness.get_by_label_contains("計測は保存されません").rect(),
            ),
        ];
        if heavy {
            rects.push(("形", harness.get_by_label("●形の編集 未書き出し").rect()));
            rects.push(("レビュー", harness.get_by_label("●レビュー未保存").rect()));
        }
        let all = entries(&harness);
        if stl {
            rects.push((
                "ファイル名",
                harness.get_by_label("review-sample.stl").rect(),
            ));
            rects.push(("向き", widget_rect(&harness, ids::STATUS_STL_UP)));
            rects.push(("簡略表示", widget_rect(&harness, ids::STATUS_WARNINGS)));
        } else {
            assert!(!all.iter().any(|(id, _)| id == ids::STATUS_STL_UP));
        }
        if let Some((_, rect)) = all.iter().find(|(n, _)| n == ids::STATUS_DISPLAY_UNIT) {
            rects.push(("表示", *rect));
        }
        if let Some(node) = harness.query_by_label_contains("格子 ") {
            rects.push(("格子", node.rect()));
        }
        if let Some(node) = harness.query_by_label_contains("部品 ") {
            rects.push(("部品", node.rect()));
        }
        for (i, (an, a)) in rects.iter().enumerate() {
            assert!(
                status.expand(0.5).contains_rect(*a),
                "{size:?}: {an} {a:?} がはみ出す"
            );
            for (bn, b) in &rects[i + 1..] {
                assert!(
                    !a.shrink(0.5).intersects(b.shrink(0.5)),
                    "{size:?}: {an} と {bn} が重なる: {a:?} {b:?}"
                );
            }
        }
        let names: Vec<_> = rects.iter().map(|(name, _)| *name).collect();
        if !heavy {
            assert!(
                names.contains(&"表示"),
                "{size:?}: 印が無ければ幅960でも表示の単位は出る {names:?}"
            );
        }
        if size.x >= 1280.0 {
            assert_eq!(rects.len(), 9, "{size:?}: 広い幅ではすべて出る");
            let grid = model_reviewer::ui::status::grid_text(harness.state());
            assert!(grid.ends_with(" mm"), "{grid}");
            assert!(harness.query_by_label(&grid).is_some(), "{grid}");
        }
        if stl {
            harness.get_by_label("上 = Z（仮定）").click();
            harness.run();
            assert!(
                harness.state().dialog.is_some(),
                "向きを替える前に未保存の確認が出る"
            );
        }
    }
}

/// 断面の帯（軸・位置・反転）は、上端のボタンの下に出て、立方体・道具の列・上端のボタンと
/// 重ならず、3Dビューの中に収まる。
#[test]
fn section_bar_fits_under_the_view_buttons_at_960() {
    use model_reviewer::command::Command;
    for (size, parts, notes) in [
        (egui::vec2(960.0, 600.0), true, true),
        (egui::vec2(960.0, 600.0), false, false),
        (egui::vec2(1280.0, 800.0), true, true),
    ] {
        let mut session = sample_session();
        session.panel_details = false;
        session.panel_parts = parts;
        session.panel_notes = notes;
        session.apply(Command::ToggleSection);
        let mut harness = Harness::builder()
            .allow_missing_glyphs()
            .with_accessibility_check(false)
            .with_size(size)
            .with_context_setup(model_reviewer::fonts::install)
            .build_ui_state(
                |ui, session| {
                    let mut commands = Vec::new();
                    let pixels = model_reviewer::ui::show(ui, session, &mut commands);
                    for command in commands {
                        session.apply(command);
                    }
                    session.set_viewport(pixels);
                },
                session,
            );
        for _ in 0..6 {
            harness.step();
        }
        let all = entries(&harness);
        let viewport = widget_rect(&harness, ids::VIEWPORT);
        let band: Vec<_> = all
            .iter()
            .filter(|(name, _)| name.starts_with("section.") && name != ids::SECTION_HANDLE)
            .collect();
        assert_eq!(band.len(), 5, "{size:?}: 軸3つ・位置・反転");
        let others: Vec<_> = all
            .iter()
            .filter(|(name, _)| {
                name.starts_with("view.")
                    || name.starts_with("viewcube.")
                    || name.starts_with("tool.")
            })
            .collect();
        for (name, rect) in &band {
            assert!(
                viewport.contains_rect(*rect),
                "{size:?}: {name} {rect:?} が3Dビュー {viewport:?} の外"
            );
            for (other, other_rect) in &others {
                assert!(
                    !rect.shrink(0.5).intersects(other_rect.shrink(0.5)),
                    "{size:?}: {name} と {other} が重なる"
                );
            }
        }
        for (i, (a_name, a)) in band.iter().enumerate() {
            for (b_name, b) in &band[i + 1..] {
                assert!(
                    !a.shrink(0.5).intersects(b.shrink(0.5)),
                    "{size:?}: {a_name} と {b_name} が重なる"
                );
            }
        }
    }
}

#[test]
fn right_panel_dims_and_measure_fit_above_the_status_bar() {
    use model_reviewer::command::{Command, ShapeKind, Tool};
    for size in [egui::vec2(1280.0, 800.0), egui::vec2(960.0, 720.0)] {
        let mut harness = harness(size);
        let session = harness.state_mut();
        session.panel_details = false;
        session.apply(Command::AddShape(ShapeKind::Cube));
        session.apply(Command::SetTool(Tool::Measure));
        harness.run_steps(4);
        let status = widget_rect(&harness, ids::PANEL_STATUS);
        let scale = widget_rect(&harness, ids::INSPECTOR_SCALE_Z);
        let fields =
            [ids::DIMS_X, ids::DIMS_Y, ids::DIMS_Z].map(|name| widget_rect(&harness, name));
        let clear = widget_rect(&harness, ids::MEASURE_CLEAR);
        // 部品 → 変形 → 寸法 → 計測 の順に、重ならずに並ぶ。
        assert!(scale.bottom() <= fields[0].top(), "{size:?}");
        for pair in fields.windows(2) {
            assert!(pair[0].bottom() <= pair[1].top(), "{size:?}");
        }
        assert!(fields[2].bottom() <= clear.top(), "{size:?}");
        assert!(
            clear.bottom() <= status.top(),
            "{size:?}: {clear:?} {status:?}"
        );
        // 選んだ図形の行が一覧に見えている。
        let row = harness.state().selection.unwrap();
        let entries = entries(&harness);
        assert!(
            entries
                .iter()
                .any(|(name, _)| *name == format!("parts.row.{}", row.0)),
            "{size:?}"
        );
    }
}

#[test]
fn color_panel_fits_at_960() {
    let mut h = harness(egui::vec2(960.0, 600.0));
    h.state_mut().panel_details = false;
    h.run_steps(6);
    let view = widget_rect(&h, ids::VIEWPORT);
    let bottom = widget_rect(&h, ids::PANEL_STATUS).top();
    let swatches: Vec<_> = (0..12)
        .map(|i| widget_rect(&h, &format!("{}.{i}", ids::COLOR_SWATCH)))
        .collect();
    let hex = widget_rect(&h, ids::COLOR_HEX);
    assert!(hex.bottom() < swatches[0].top());
    for (column, upper) in swatches[..6].iter().enumerate() {
        let lower = swatches[column + 6];
        assert_eq!(upper.size(), egui::Vec2::splat(18.0));
        assert!((upper.left() - lower.left()).abs() < 0.5);
        assert!((upper.top() - swatches[0].top()).abs() < 0.5);
        assert!((lower.top() - swatches[6].top()).abs() < 0.5);
        assert!(
            upper.bottom() <= lower.top() && lower.bottom() <= bottom,
            "見本 {upper:?}・{lower:?}、16進 {hex:?}、変形 {:?}、状態欄の上端 {bottom}",
            widget_rect(&h, ids::INSPECTOR_POS_X)
        );
        assert!(upper.left() >= view.right() && lower.right() <= 960.0);
        if column > 0 {
            assert!(upper.left() >= swatches[column - 1].right());
        }
    }
}

/// ヘルプの下の [ショートカットの設定] と手順書の場所（第4段階の要件1.6）。幅960×600で表の下に
/// 収まり、押すとヘルプを閉じて設定の画面を開く。[場所を写す] は手順書の場所を写す。
#[test]
fn help_offers_keys_and_guide_location_at_960() {
    use egui_kittest::kittest::Queryable;
    use model_reviewer::command::Command;
    use model_reviewer::keymap::{Keymap, Platform};
    use model_reviewer::ui::help;

    struct Probe {
        out: Vec<Command>,
    }
    let keys = Keymap::default().labels(Platform::Mac);
    let mut harness = Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_size(egui::vec2(960.0, 600.0))
        .with_context_setup(|ctx| {
            model_reviewer::fonts::install(ctx);
            help::toggle(ctx);
        })
        .build_ui_state(
            move |ui, probe: &mut Probe| {
                ui.ctx()
                    .data_mut(|data| data.insert_temp(egui::Id::NULL, WidgetFrame::default()));
                help::show_with(ui, &keys, Platform::Mac, &mut probe.out);
            },
            Probe { out: Vec::new() },
        );
    let entries = |harness: &Harness<'static, Probe>| -> Vec<(String, egui::Rect)> {
        harness
            .ctx
            .data(|data| data.get_temp::<WidgetFrame>(egui::Id::NULL))
            .unwrap_or_default()
            .entries()
            .iter()
            .map(|e| {
                (
                    e.name.clone(),
                    egui::Rect::from_min_max(e.min.into(), e.max.into()),
                )
            })
            .collect()
    };
    let mut last = Vec::new();
    for _ in 0..300 {
        harness.step();
        let now = entries(&harness);
        if now == last {
            break;
        }
        last = now;
    }
    let rect = |name: &str| {
        last.iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("部品が無い: {name}"))
            .1
    };
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960.0, 600.0));
    let settings = rect(ids::HELP_KEYS);
    let copy = rect(ids::HELP_GUIDE_COPY);
    let close = rect(ids::HELP_CLOSE);
    for r in [settings, copy, close] {
        assert!(screen.contains_rect(r), "{r:?}");
    }
    assert!(settings.right() < copy.left(), "{settings:?} {copy:?}");
    assert!(copy.bottom() <= close.top(), "{copy:?} {close:?}");
    harness.get_by_label(&format!("手順書: {}", help::GUIDE_PATH));
    // 表のキーは今の割り当てから。
    harness.get_by_label("Cmd+Z ／ Shift+Cmd+Z");

    let press = |harness: &mut Harness<'static, Probe>, at: egui::Pos2| {
        harness.event(egui::Event::PointerMoved(at));
        harness.step();
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            harness.step();
        }
    };
    press(&mut harness, copy.center());
    let url = help::guide_url().expect("Cargo.toml に repository がある");
    assert!(url.ends_with("/blob/HEAD/docs/guide/README.md"), "{url}");
    assert!(url.starts_with("https://"), "{url}");
    // 写した文字は、押したフレーム（離したフレーム）の出力にある。
    assert!(
        harness
            .output()
            .platform_output
            .commands
            .contains(&egui::OutputCommand::CopyText(url.clone())),
        "{:?}",
        harness.output().platform_output.commands
    );
    assert!(
        harness.state().out.is_empty(),
        "写すだけで Command を積まない"
    );

    press(&mut harness, settings.center());
    assert_eq!(harness.state().out, vec![Command::SetKeysOpen(true)]);
    harness.step();
    let open = harness.ctx.data(|data| {
        data.get_temp::<bool>(egui::Id::unique(ids::HELP_OPEN))
            .unwrap_or(false)
    });
    assert!(!open, "設定の画面を開くときはヘルプを閉じる");
}
