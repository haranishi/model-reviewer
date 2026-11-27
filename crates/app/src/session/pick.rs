//! クリックの選択（計測の道具では計測の点）。印の当たりは画面側が先に見る。ここは面だけ。
//!
//! 約束: 視線の当たり判定の入口はこのファイルの `ray_hit` だけにする。断面を渡すのはここだけで、
//! 選択・注釈の下書き・計測（吸着）・注釈の印の隠れの判定が、同じ消えた側を外す。

use glam::Vec2;
use model_reviewer_core::gizmo::Tool;
use model_reviewer_core::math::Ray;
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
    let Some(bounds) = session.doc.as_ref().map(|doc| doc.bounds()) else {
        return;
    };
    if session.pick_index.is_none() {
        return;
    }
    if session.tool == Tool::Measure {
        // 候補の控え（`snap_preview`）は使わず、その場で探す（ポインタの移動より先にクリックが
        // 届くことがある）。
        let target = super::snap::search(session, ndc, true);
        super::measure::click(session, target);
        return;
    }
    let aspect = super::aspect_of(session.viewport_px);
    let ray = session.camera.screen_ray_in(ndc, aspect, &bounds);
    let hit = ray_hit(session, &ray);

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

/// 視線の当たり（断面で消えた側を外す）。索引か文書が無ければ `None`。
pub fn ray_hit(session: &Session, ray: &Ray) -> Option<Hit> {
    let doc = session.doc.as_ref()?;
    let index = session.pick_index.as_ref()?;
    let clip = session.section.as_ref();
    match session.review.as_ref() {
        Some(review) => pick::pick_clipped(doc, index, &review.visibility, ray, clip),
        None => {
            let visibility = Visibility::all_visible(doc.node_count());
            pick::pick_clipped(doc, index, &visibility, ray, clip)
        }
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
