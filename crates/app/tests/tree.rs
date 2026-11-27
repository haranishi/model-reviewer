//! 部品ツリー。GPUは使わない。

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use egui_kittest::kittest::Queryable;
use glam::Affine3A;
use model_reviewer::command::Command;
use model_reviewer::session::Session;
use model_reviewer_core::math::Aabb;
use model_reviewer_core::review::ReviewState;
use model_reviewer_core::scene::{Model, ModelStats, Node, NodeId};

fn node(name: &str, parent: Option<NodeId>, subtree_end: u32, depth: u32) -> Node {
    Node {
        name: name.to_owned(),
        gltf_index: subtree_end,
        parent,
        subtree_end,
        depth,
        local: Affine3A::IDENTITY,
        world: Affine3A::IDENTITY,
        draws: 0..0,
        world_aabb: Aabb::empty(),
    }
}

fn install(session: &mut Session, nodes: Vec<Node>) {
    let count = nodes.len() as u32;
    let model = Arc::new(Model {
        nodes,
        draws: Vec::new(),
        primitives: Vec::new(),
        bounds: Aabb::empty(),
        stats: ModelStats {
            nodes: count,
            ..ModelStats::default()
        },
        warnings: Vec::new(),
    });
    session.review = Some(ReviewState::new(&model));
    session.set_model(model);
}

fn family() -> Session {
    let mut session = Session::default();
    install(
        &mut session,
        vec![
            node("assembly", None, 3, 0),
            node("arm", Some(NodeId(0)), 3, 1),
            node("arm_tip", Some(NodeId(1)), 3, 2),
        ],
    );
    session
}

fn harness(session: Session) -> egui_kittest::Harness<'static, Session> {
    egui_kittest::Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_size(egui::vec2(320.0, 360.0))
        .with_context_setup(|ctx| {
            model_reviewer::fonts::install(ctx);
            model_reviewer::theme::install(ctx);
        })
        .build_ui_state(
            |ui, session| {
                let mut commands = Vec::new();
                model_reviewer::ui::tree::show(ui, session, &mut commands);
                for command in commands {
                    session.apply(command);
                }
            },
            session,
        )
}

#[test]
fn hiding_a_parent_keeps_each_child_flag_until_show_all() {
    let mut session = family();
    session.apply(Command::SetVisible {
        node: NodeId(0),
        visible: false,
    });
    let visibility = &session.review.as_ref().unwrap().visibility;
    assert!(!visibility.effective(NodeId(0)));
    assert!(!visibility.effective(NodeId(1)));
    assert!(!visibility.effective(NodeId(2)));
    assert!(visibility.own(NodeId(1)));
    assert!(visibility.own(NodeId(2)));
    assert!(session.unsaved());
    let dirty = session.take_scene_dirty();
    assert!(dirty.instances);
    assert!(!dirty.selection);

    session.mark_saved();
    session.apply(Command::SetVisible {
        node: NodeId(2),
        visible: false,
    });
    assert!(!session.review.as_ref().unwrap().visibility.own(NodeId(2)));
    session.take_scene_dirty();

    session.apply(Command::SetVisible {
        node: NodeId(0),
        visible: true,
    });
    let visibility = &session.review.as_ref().unwrap().visibility;
    assert!(visibility.own(NodeId(0)));
    assert!(visibility.effective(NodeId(1)));
    assert!(!visibility.own(NodeId(2)));
    assert!(!visibility.effective(NodeId(2)));

    session.mark_saved();
    session.apply(Command::ShowAll);
    let visibility = &session.review.as_ref().unwrap().visibility;
    assert!(visibility.own(NodeId(2)) && visibility.effective(NodeId(2)));
    assert!(session.unsaved());
    assert!(session.take_scene_dirty().instances);

    session.mark_saved();
    session.apply(Command::ShowAll);
    assert!(!session.unsaved());
    assert!(!session.take_scene_dirty().any());

    session.apply(Command::SetVisible {
        node: NodeId(1),
        visible: false,
    });
    session.take_scene_dirty();
    session.mark_saved();
    session.apply(Command::SetVisible {
        node: NodeId(1),
        visible: false,
    });
    assert!(!session.unsaved());
    assert!(!session.take_scene_dirty().any());

    session.apply(Command::HideSelected);
    assert!(!session.unsaved());

    session.apply(Command::Select(Some(NodeId(1))));
    assert!(!session.unsaved());
    let dirty = session.take_scene_dirty();
    assert!(dirty.selection);
    assert!(!dirty.instances);
    session.apply(Command::Select(Some(NodeId(1))));
    assert!(!session.take_scene_dirty().any());

    session.apply(Command::SetVisible {
        node: NodeId(9),
        visible: false,
    });
    assert!(!session.take_scene_dirty().any());
}

#[test]
fn tree_paints_only_the_rows_on_screen_and_scrolls_to_a_selection() {
    const MANY: u32 = 100_000;
    let mut nodes = Vec::with_capacity(MANY as usize);
    for index in 0..MANY {
        let name = if index == 80_000 { "target-row" } else { "n" };
        nodes.push(node(name, None, index + 1, 0));
    }
    let mut session = Session::default();
    install(&mut session, nodes);
    let painted = Rc::new(Cell::new(0usize));
    let painted_ui = Rc::clone(&painted);
    let mut harness = egui_kittest::Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_size(egui::vec2(280.0, 240.0))
        .with_context_setup(|ctx| {
            model_reviewer::fonts::install(ctx);
            model_reviewer::theme::install(ctx);
        })
        .build_ui_state(
            move |ui, session| {
                let mut commands = Vec::new();
                painted_ui.set(model_reviewer::ui::tree::show(ui, session, &mut commands));
                for command in commands {
                    session.apply(command);
                }
            },
            session,
        );
    let rows = painted.get();
    assert!(rows > 0, "行が描かれていません");
    assert!(
        rows.saturating_mul(100) < MANY as usize,
        "画面分を超えて {rows} 行描いています"
    );
    assert_eq!(harness.query_all_by_label("n").count(), rows);
    assert!(harness.query_by_label("target-row").is_none());

    harness.state_mut().selection = Some(NodeId(80_000));
    harness.step();
    harness.get_by_label("target-row");
    let rows = painted.get();
    assert!(rows > 0 && rows.saturating_mul(100) < MANY as usize);
}

#[test]
fn eye_hides_descendants_and_show_all_restores_them() {
    let mut harness = harness(family());
    harness.get_by_label("表示 assembly").click();
    harness.step();
    harness.step();
    harness.get_by_label("親により非表示 arm");
    harness.get_by_label("親により非表示 arm_tip");
    assert!(harness.state().selection.is_none());
    assert!(harness.state().unsaved());

    harness.get_by_label("すべて表示").click();
    harness.step();
    harness.step();
    harness.get_by_label("表示 arm");
    harness.get_by_label("表示 arm_tip");
    assert!(
        harness
            .state()
            .review
            .as_ref()
            .unwrap()
            .visibility
            .effective(NodeId(2))
    );
}

#[test]
fn a_self_hidden_row_stays_distinct_when_its_parent_is_hidden() {
    let mut session = family();
    session.apply(Command::SetVisible {
        node: NodeId(2),
        visible: false,
    });
    session.apply(Command::SetVisible {
        node: NodeId(0),
        visible: false,
    });
    let harness = harness(session);
    harness.get_by_label("非表示 arm_tip");
    harness.get_by_label("親により非表示 arm");
    assert!(harness.query_by_label("親により非表示 arm_tip").is_none());
}

#[test]
fn choosing_a_row_selects_it_and_collapsing_hides_its_children() {
    let mut harness = harness(family());
    harness.get_by_label("arm").click();
    harness.step();
    assert_eq!(harness.state().selection, Some(NodeId(1)));
    assert!(!harness.state().unsaved());

    harness.get_by_label("開閉 assembly").click();
    harness.step();
    harness.step();
    assert!(harness.query_by_label("arm").is_none());
    harness.get_by_label("assembly");
}

#[test]
fn fitted_list_keeps_the_top_row_when_the_last_row_is_selected() {
    let mut harness = harness(family());
    harness.get_by_label("assembly");
    harness.state_mut().selection = Some(NodeId(2));
    harness.step();
    let top = harness.get_by_label("assembly").rect().top();
    let selected = harness.get_by_label("arm_tip").rect().top();
    assert!(selected > top, "収まる一覧なのに先頭がずれています");
}

#[test]
fn offscreen_row_scrolls_into_view_without_jumping_to_the_top() {
    const MANY: u32 = 40;
    let mut nodes = Vec::new();
    for index in 0..MANY {
        nodes.push(node(&format!("row-{index}"), None, index + 1, 0));
    }
    let mut session = Session::default();
    install(&mut session, nodes);
    let mut harness = egui_kittest::Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_size(egui::vec2(280.0, 280.0))
        .with_context_setup(|ctx| {
            model_reviewer::fonts::install(ctx);
            model_reviewer::theme::install(ctx);
        })
        .build_ui_state(
            |ui, session| {
                let mut commands = Vec::new();
                model_reviewer::ui::tree::show(ui, session, &mut commands);
                for command in commands {
                    session.apply(command);
                }
            },
            session,
        );
    harness.get_by_label("row-0");
    let hidden = (0..MANY)
        .find(|index| {
            harness
                .query_by_label(format!("row-{index}").as_str())
                .is_none()
        })
        .expect("画面外の行がありません");
    assert!(hidden >= 3, "比較できる手前の行がありません");
    harness.state_mut().selection = Some(NodeId(hidden));
    harness.step();
    let target_label = format!("row-{hidden}");
    let target = harness.get_by_label(target_label.as_str()).rect().top();
    let earlier = harness.get_by_label("row-2").rect().top();
    assert!(target > earlier, "選択行を先頭へ送っています");
}

/// 開閉の記憶は部品の鍵で持つ。前に部品が入って並びがずれても、畳んだ部品が畳まれたまま。
#[test]
fn collapsed_rows_follow_the_part_when_the_order_shifts() {
    use model_reviewer_core::doc::Place;

    let mut harness = harness(family());
    harness.get_by_label("開閉 arm").click();
    harness.step();
    harness.step();
    assert!(harness.query_by_label("arm_tip").is_none());

    let session = harness.state_mut();
    let doc = session.doc.as_mut().expect("文書");
    let root = doc.key(NodeId(0)).unwrap();
    let arm = doc.key(NodeId(1)).unwrap();
    let (_, piece) = doc.duplicate_subtree(arm).unwrap();
    let before_arm = Place {
        parent: Some(root),
        next_sibling: Some(arm),
    };
    doc.insert_subtree(before_arm, piece).unwrap();
    let review = ReviewState::new(&*doc);
    session.review = Some(review);
    harness.step();
    harness.step();

    // 並び: assembly, arm コピー, arm_tip（コピーの子・開いている）, arm（畳んだまま）。
    let tip = harness.get_by_label("arm_tip").rect().center().y;
    let original = harness.get_by_label("arm").rect().center().y;
    let copy = harness.get_by_label("arm コピー").rect().center().y;
    assert!(
        copy < tip && tip < original,
        "畳んだ部品が並びの番号に付いていった: コピー {copy} 子 {tip} 元 {original}"
    );
}

#[test]
fn list_follows_duplicate_delete_and_undo_through_commands() {
    let mut harness = harness(family());
    // arm を畳む。複製（元の直後）と削除と取り消しの後も、畳んだのは同じ arm のまま。
    harness.get_by_label("開閉 arm").click();
    harness.step();
    harness.step();
    harness.state_mut().apply(Command::Select(Some(NodeId(1))));
    harness.state_mut().apply(Command::DuplicateSelected);
    harness.step();
    harness.step();
    assert_eq!(harness.state().selection, Some(NodeId(3)), "複製が選ばれる");
    assert!(harness.query_by_label("arm コピー").is_some());
    assert_eq!(
        harness.query_all_by_label("arm_tip").count(),
        1,
        "元は畳んだまま・複製の子は開いている"
    );

    // 元の arm を消すと、行も消える。
    harness.state_mut().apply(Command::Select(Some(NodeId(1))));
    harness.state_mut().apply(Command::DeleteSelected);
    harness.step();
    harness.step();
    assert!(harness.query_by_label("arm").is_none());
    assert!(harness.query_by_label("arm コピー").is_some());

    // 取り消すと戻り、畳んだ状態も戻る。
    harness.state_mut().apply(Command::Undo);
    harness.step();
    harness.step();
    assert_eq!(harness.state().selection, Some(NodeId(1)));
    let original = harness.get_by_label("arm").rect().center().y;
    let copy = harness.get_by_label("arm コピー").rect().center().y;
    assert!(original < copy, "元が先、複製が後");
    assert_eq!(harness.query_all_by_label("arm_tip").count(), 1);
    harness.state_mut().apply(Command::Undo);
    harness.step();
    harness.step();
    assert!(harness.query_by_label("arm コピー").is_none());
    assert_eq!(harness.state().doc.as_ref().unwrap().nodes().len(), 3);
}
