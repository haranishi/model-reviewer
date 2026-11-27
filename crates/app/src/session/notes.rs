//! 注釈と視点。本文・表示・視点の追加と削除だけ未保存にする。選択とカメラの適用では立てない。

use glam::Vec3;
use model_reviewer_core::history::{Edit, KeyedNote, Step};
use model_reviewer_core::pick::Hit;
use model_reviewer_core::scene::NodeId;

use crate::ui::ids;

use super::Session;

#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub node: NodeId,
    pub local_pos: Vec3,
    pub local_normal: Vec3,
    pub text: String,
}

pub fn set_mode(session: &mut Session, on: bool) {
    if session.annotate_mode == on || (on && session.model.is_none()) {
        return;
    }
    session.annotate_mode = on;
    if !on && !session.draft_pending() {
        cancel(session);
    }
}

pub fn commit(session: &mut Session, text: String) {
    let text = text.trim().to_owned();
    if text.is_empty() {
        return;
    }
    let Some(draft) = session.draft.clone() else {
        return;
    };
    let hit = Hit {
        node: draft.node,
        t: 0.0,
        world_pos: Vec3::ZERO,
        local_pos: draft.local_pos,
        local_normal: draft.local_normal,
        // 注釈は部品と部品の中の座標だけを使う。形状と三角形の番号は読まない。
        prim: model_reviewer_core::scene::PrimId(0),
        triangle: 0,
    };
    let Some(key) = session.doc.as_ref().and_then(|doc| doc.key(draft.node)) else {
        return;
    };
    let Some(review) = session.review.as_mut() else {
        return;
    };
    let Some(id) = review.add_annotation(&hit, text) else {
        session.status_text = "注釈をこれ以上追加できません".to_owned();
        return;
    };
    let note = review
        .annotations
        .iter()
        .find(|note| note.id == id)
        .cloned();
    session.draft = None;
    session.draft_pending = false;
    session.selected_annotation = Some(id);
    session.selection = Some(draft.node);
    session.status_text.clear();
    session.scene_dirty.selection = true;
    if let Some(note) = note {
        let step = Step::new(
            "注釈の追加",
            vec![Edit::AddNote(KeyedNote { node: key, note })],
            Some(key),
        );
        super::edit::record(session, step);
    }
    clear_draft_text(session);
}

pub fn cancel(session: &mut Session) {
    session.draft_pending = false;
    if session.draft.take().is_some() {
        session.scene_dirty.selection = true;
        clear_draft_text(session);
    }
}

pub fn edit(session: &mut Session, id: u64, text: String) {
    let Some(note) = session
        .review
        .as_ref()
        .and_then(|review| review.annotations.iter().find(|note| note.id == id))
    else {
        return;
    };
    if note.text == text {
        return;
    }
    let before = note.text.clone();
    let key = session.doc.as_ref().and_then(|doc| doc.key(note.node));
    let step = Step::new(
        "注釈の編集",
        vec![Edit::EditNote {
            id,
            before,
            after: text,
        }],
        key,
    );
    super::edit::execute(session, step);
}

pub fn delete_annotation(session: &mut Session, id: u64) {
    let Some((index, note)) = session.review.as_ref().and_then(|review| {
        review
            .annotations
            .iter()
            .enumerate()
            .find(|(_, note)| note.id == id)
            .map(|(index, note)| (index, note.clone()))
    }) else {
        return;
    };
    let Some(key) = session.doc.as_ref().and_then(|doc| doc.key(note.node)) else {
        return;
    };
    if session.selected_annotation == Some(id) {
        session.selected_annotation = None;
    }
    let step = Step::new(
        "注釈の削除",
        vec![Edit::RemoveNote {
            index,
            note: KeyedNote { node: key, note },
        }],
        Some(key),
    );
    if super::edit::execute(session, step).is_some() {
        clear_body_edit(session, id);
    }
}

pub fn select_annotation(session: &mut Session, id: Option<u64>) {
    let Some(id) = id else {
        if session.selected_annotation.take().is_some() {
            session.scene_dirty.selection = true;
        }
        return;
    };
    let node = session.review.as_ref().and_then(|review| {
        review
            .annotations
            .iter()
            .find(|note| note.id == id)
            .map(|note| note.node)
    });
    let Some(node) = node else {
        return;
    };
    if session.selected_annotation == Some(id) && session.selection == Some(node) {
        return;
    }
    session.selected_annotation = Some(id);
    session.selection = Some(node);
    session.scene_dirty.selection = true;
}

pub(super) fn clear_body_edits(session: &Session) {
    let Some(ctx) = &session.egui_ctx else {
        return;
    };
    let generation = session.review_generation();
    let Some(review) = session.review.as_ref() else {
        return;
    };
    let ids: Vec<u64> = review.annotations.iter().map(|note| note.id).collect();
    ctx.data_mut(|data| {
        for id in ids {
            let buf = crate::ui::annotations::body_temp_id(generation, id);
            data.remove_temp::<String>(buf);
            data.remove_temp::<egui::text_edit::TextEditState>(buf);
            data.remove::<egui::text_edit::TextEditState>(buf);
        }
    });
}

fn clear_body_edit(session: &Session, id: u64) {
    let Some(ctx) = &session.egui_ctx else {
        return;
    };
    let buf = crate::ui::annotations::body_temp_id(session.review_generation(), id);
    ctx.data_mut(|data| {
        data.remove_temp::<String>(buf);
        data.remove_temp::<egui::text_edit::TextEditState>(buf);
        data.remove::<egui::text_edit::TextEditState>(buf);
    });
}

fn clear_draft_text(session: &Session) {
    let Some(ctx) = &session.egui_ctx else {
        return;
    };
    ctx.data_mut(|data| data.insert_temp(egui::Id::unique(ids::DRAFT_TEXT), String::new()));
}

pub fn save_view(session: &mut Session, name: String) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    let camera = session.camera;
    let Some(review) = session.review.as_mut() else {
        return;
    };
    let Some(id) = review.add_view(name.to_owned(), camera) else {
        session.status_text = "視点をこれ以上追加できません".to_owned();
        return;
    };
    let Some(view) = review.views.iter().find(|view| view.id == id).cloned() else {
        return;
    };
    super::edit::record(
        session,
        Step::new("視点の保存", vec![Edit::AddView(view)], None),
    );
}

pub fn apply_view(session: &mut Session, id: u64) {
    let Some(camera) = session.review.as_ref().and_then(|review| {
        review
            .views
            .iter()
            .find(|view| view.id == id)
            .map(|view| view.camera)
    }) else {
        return;
    };
    if session.camera == camera {
        return;
    }
    session.camera = camera;
    session.scene_dirty.camera = true;
}

pub fn delete_view(session: &mut Session, id: u64) {
    let Some((index, view)) = session.review.as_ref().and_then(|review| {
        review
            .views
            .iter()
            .enumerate()
            .find(|(_, view)| view.id == id)
            .map(|(index, view)| (index, view.clone()))
    }) else {
        return;
    };
    super::edit::execute(
        session,
        Step::new("視点の削除", vec![Edit::RemoveView { index, view }], None),
    );
}
