//! クリックの選択（計測の道具では計測の点）。印の当たりは画面側が先に見る。ここは面だけ。

use glam::Vec2;
use model_reviewer_core::gizmo::Tool;
use model_reviewer_core::pick::{self, Hit};
use model_reviewer_core::scene::SceneRead;
use model_reviewer_core::visibility::Visibility;

use super::Session;
use super::notes::Draft;

const MISS: &str = "表面をクリックしてください";

pub fn view_click(session: &mut Session, ndc: Vec2) {
    if session.draft.is_some() || !ndc.is_finite() {
        return;
    }
    let Some(doc) = session.doc.as_ref() else {
        return;
    };
    let aspect = super::aspect_of(session.viewport_px);
    let ray = session.camera.screen_ray_in(ndc, aspect, &doc.bounds());
    let hit = match (session.pick_index.as_ref(), session.review.as_ref()) {
        (Some(index), Some(review)) => pick::pick(doc, index, &review.visibility, &ray),
        (Some(index), None) => {
            let visibility = Visibility::all_visible(doc.node_count());
            pick::pick(doc, index, &visibility, &ray)
        }
        (None, _) => return,
    };

    if session.tool == Tool::Measure {
        super::measure::click(session, hit);
        return;
    }

    if session.annotate_mode {
        if let Some(reason) = session.edit_block_reason() {
            session.status_text = reason.to_owned();
            return;
        }
        if session.review.is_none() {
            session.status_text = MISS.to_owned();
            return;
        }
        if let Some(hit) = hit {
            clear_draft_text(session);
            session.draft = Some(draft_from(hit));
            session.status_text.clear();
        } else {
            session.status_text = MISS.to_owned();
        }
        return;
    }

    if let Some(hit) = hit {
        super::parts::select(session, Some(hit.node));
        session.status_text.clear();
    } else {
        super::parts::select(session, None);
        session.status_text = MISS.to_owned();
    }
}

fn draft_from(hit: Hit) -> Draft {
    Draft {
        node: hit.node,
        local_pos: hit.local_pos,
        local_normal: hit.local_normal,
        text: String::new(),
    }
}

fn clear_draft_text(session: &Session) {
    let Some(ctx) = &session.egui_ctx else {
        return;
    };
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::unique(crate::ui::ids::DRAFT_TEXT), String::new());
    });
}
