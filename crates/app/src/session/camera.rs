//! 視点の操作。移動に補間は付けない。カメラだけでは未保存にしない。

use glam::Vec2;
use model_reviewer_core::camera::{DISTANCE_MAX_RATIO, DISTANCE_MIN_RATIO, OrbitCamera};
use model_reviewer_core::scene::SceneRead;

use super::Session;

pub fn orbit(session: &mut Session, delta: Vec2) {
    if session.model.is_none() || !delta.is_finite() || delta == Vec2::ZERO {
        return;
    }
    let before = session.camera;
    session.camera.orbit(delta);
    mark_camera(session, before);
}

pub fn pan(session: &mut Session, delta: Vec2) {
    if session.model.is_none() || !delta.is_finite() || delta == Vec2::ZERO {
        return;
    }
    let height = session.viewport_px[1] as f32;
    let before = session.camera;
    session.camera.pan(delta, height);
    mark_camera(session, before);
}

pub fn zoom(session: &mut Session, factor: f32, cursor_ndc: Option<Vec2>) {
    if session.model.is_none() || !factor.is_finite() || factor <= 0.0 {
        return;
    }
    let radius = session
        .doc
        .as_ref()
        .map(|doc| doc.bounds().radius())
        .filter(|radius| radius.is_finite() && *radius > 0.0)
        .unwrap_or(1.0);
    let limits = (radius * DISTANCE_MIN_RATIO, radius * DISTANCE_MAX_RATIO);
    let aspect = super::aspect_of(session.viewport_px);
    let cursor = cursor_ndc.filter(|ndc| ndc.is_finite());
    let before = session.camera;
    session.camera.zoom(factor, cursor, aspect, limits);
    mark_camera(session, before);
}

pub fn fit_all(session: &mut Session) {
    let Some(bounds) = session.doc.as_ref().map(|doc| doc.bounds()) else {
        return;
    };
    refit(session, bounds);
}

pub fn fit_selection(session: &mut Session) {
    let Some(id) = session.selection else {
        return;
    };
    let Some(doc) = session.doc.as_ref() else {
        return;
    };
    if doc.node(id).is_none() {
        return;
    }
    refit(session, doc.subtree_bounds(id));
}

pub fn home(session: &mut Session) {
    let Some(doc) = session.doc.as_ref() else {
        return;
    };
    let aspect = super::aspect_of(session.viewport_px);
    let mut next = OrbitCamera::home(&doc.bounds(), aspect);
    // 初期視点は向きと距離を戻すだけで、投影は利用者の選んだままにする。
    next.projection = session.camera.projection;
    let before = session.camera;
    session.camera = next;
    mark_camera(session, before);
}

fn refit(session: &mut Session, bounds: model_reviewer_core::math::Aabb) {
    if bounds.is_empty() {
        return;
    }
    let aspect = super::aspect_of(session.viewport_px);
    let before = session.camera;
    session.camera.fit(&bounds, aspect);
    mark_camera(session, before);
}

pub(super) fn mark_camera(session: &mut Session, before: OrbitCamera) {
    if session.camera != before {
        session.scene_dirty.camera = true;
    }
}
