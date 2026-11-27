//! 見る操作：決まった向き・表示・投影・格子。
//!
//! 約束: 視点・表示・格子の変更は `scene_dirty`（`camera`・`display`）を立てるだけで、履歴にも
//! 未保存の印にも入らない。値が変わらない操作は何も立てない（何もしない間は描かない）。

use model_reviewer_core::camera::{Projection, ViewDir};
use model_reviewer_render::DisplayMode;

use super::Session;
use super::camera::mark_camera;

/// 線の表示が要る表示か。
pub(super) fn needs_lines(mode: DisplayMode) -> bool {
    matches!(mode, DisplayMode::FacesAndLines | DisplayMode::Lines)
}

/// 方位と仰角だけを置き換える。モデルが無ければ何もしない（回転と同じ）。
pub(super) fn snap_view(session: &mut Session, dir: ViewDir) {
    if session.model.is_none() {
        return;
    }
    let before = session.camera;
    session.camera.snap(dir);
    mark_camera(session, before);
}

pub(super) fn set_display(session: &mut Session, mode: DisplayMode) {
    if needs_lines(mode) && !session.lines_supported {
        session.status_text = crate::ui::viewbar::NO_LINES_REASON.to_owned();
        return;
    }
    if session.display != mode {
        session.display = mode;
        session.scene_dirty.display = true;
    }
}

pub(super) fn set_projection(session: &mut Session, projection: Projection) {
    let before = session.camera;
    session.camera.projection = projection;
    mark_camera(session, before);
}

pub(super) fn toggle_projection(session: &mut Session) {
    let next = match session.camera.projection {
        Projection::Perspective => Projection::Orthographic,
        Projection::Orthographic => Projection::Perspective,
    };
    set_projection(session, next);
}

pub(super) fn set_grid(session: &mut Session, visible: bool) {
    if session.grid_visible != visible {
        session.grid_visible = visible;
        session.scene_dirty.display = true;
    }
}
