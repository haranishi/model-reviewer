//! 部品の選択と表示。表示が変わったときだけインスタンスを汚し、選択では未保存にしない。

use model_reviewer_core::doc::NodeKey;
use model_reviewer_core::history::{Edit, Step};
use model_reviewer_core::scene::{NodeId, SceneRead};

use super::Session;

pub fn select(session: &mut Session, id: Option<NodeId>) {
    let valid = match id {
        Some(id) => session
            .doc
            .as_ref()
            .is_some_and(|doc| (id.0 as usize) < doc.node_count()),
        None => session.doc.is_some(),
    };
    if !valid || session.selection == id {
        return;
    }
    session.selection = id;
    session.selected_annotation = None;
    session.scene_dirty.selection = true;
}

pub fn set_visible(session: &mut Session, node: NodeId, visible: bool) {
    let Some(doc) = session.doc.as_ref() else {
        return;
    };
    let Some(key) = doc.key(node) else {
        return;
    };
    let Some(before) = session
        .review
        .as_ref()
        .map(|review| review.visibility.own(node))
    else {
        return;
    };
    if before == visible {
        return;
    }
    let label = if visible { "表示" } else { "非表示" };
    let step = Step::new(
        label,
        vec![Edit::SetVisible {
            key,
            before,
            after: visible,
        }],
        Some(key),
    );
    super::edit::execute(session, step);
}

pub fn hide_selected(session: &mut Session) {
    if let Some(id) = session.selection {
        set_visible(session, id, false);
    }
}

pub fn show_all(session: &mut Session) {
    let (Some(doc), Some(review)) = (session.doc.as_ref(), session.review.as_ref()) else {
        return;
    };
    let hidden: Vec<NodeKey> = review
        .visibility
        .hidden_own()
        .filter_map(|id| doc.key(id))
        .collect();
    if hidden.is_empty() {
        return;
    }
    let step = Step::new("すべて表示", vec![Edit::ShowAll { hidden }], None);
    super::edit::execute(session, step);
}
