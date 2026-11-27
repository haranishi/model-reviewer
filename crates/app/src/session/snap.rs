//! 計測の点の吸着（`core::snap` をつなぐ）。
//!
//! 約束: 探索は、計測の道具でポインタが動いたフレーム（`ViewHover`）とクリックだけで行う。
//! 結果は `Session::snap_preview` に置く。クリックは控えを使わず、その場で探す。吸着の入・切は
//! 履歴にも未保存の印にも入らない。
//!
//! 移動に合わせた探索が8ミリ秒を超えたら、かかった時間の4倍だけ休む（要件1.8）。休んでいる間は
//! 候補を消す（古い候補を見せない）。クリックは休まない。

use std::time::{Duration, Instant};

use glam::Vec2;
use model_reviewer_core::gizmo::Tool;
use model_reviewer_core::scene::SceneRead;
use model_reviewer_core::snap::{SnapQuery, SnapSettings, SnapTarget, snap};
use model_reviewer_core::visibility::Visibility;

use super::Session;

/// 吸着の半径（ポイント。要件1.1）。探索のときに `pixels_per_point` を掛けて物理画素にする。
pub const SNAP_RADIUS_POINTS: f32 = 12.0;
/// これを超えた探索の後は休む。
pub const SLOW_SEARCH: Duration = Duration::from_millis(8);
/// 休む長さは、かかった時間のこの倍。
pub const BACKOFF_FACTOR: u32 = 4;

/// ポインタが3Dビューの上を動いた（`Some`）か、出た（`None`）。
pub(super) fn hover(session: &mut Session, ndc: Option<Vec2>) {
    let Some(ndc) = ndc.filter(|ndc| ndc.is_finite()) else {
        set_preview(session, None);
        return;
    };
    if session.tool != Tool::Measure || session.draft.is_some() {
        set_preview(session, None);
        return;
    }
    if let Some(until) = session.snap_rest_until {
        if Instant::now() < until {
            set_preview(session, None);
            return;
        }
        session.snap_rest_until = None;
    }
    let target = search(session, ndc, false);
    set_preview(session, target);
}

/// その場で吸着の先を探す（クリックと移動で同じ規則）。かかった時間を性能値に入れる。
/// `click` が偽（ポインタの移動）なら、遅かったときに休みを入れる。
pub(super) fn search(session: &mut Session, ndc: Vec2, click: bool) -> Option<SnapTarget> {
    let started = Instant::now();
    let target = find(session, ndc);
    let elapsed = started.elapsed();
    session.metrics.snap_last = Some(elapsed);
    session.metrics.snap_max = Some(session.metrics.snap_max.map_or(elapsed, |m| m.max(elapsed)));
    if !click && let Some(rest) = rest_after(elapsed) {
        session.snap_rest_until = Some(Instant::now() + rest);
    }
    target
}

/// 探索にかかった時間から、移動に合わせた探索を休む長さを求める（8ミリ秒以下なら休まない）。
pub fn rest_after(elapsed: Duration) -> Option<Duration> {
    (elapsed > SLOW_SEARCH).then(|| elapsed * BACKOFF_FACTOR)
}

/// 吸着の先（休みも性能値も見ない）。索引か文書が無ければ `None`。
pub fn find(session: &Session, ndc: Vec2) -> Option<SnapTarget> {
    let doc = session.doc.as_ref()?;
    let index = session.pick_index.as_ref()?;
    if !ndc.is_finite() {
        return None;
    }
    let [w, h] = session.viewport_px;
    let query = SnapQuery {
        camera: &session.camera,
        aspect: super::aspect_of(session.viewport_px),
        viewport_px: [w as f32, h as f32],
        ndc,
        settings: settings(session),
        clip: session.section,
    };
    match session.review.as_ref() {
        Some(review) => snap(doc, index, &review.visibility, &query),
        None => snap(
            doc,
            index,
            &Visibility::all_visible(doc.node_count()),
            &query,
        ),
    }
}

/// 探索に渡す設定（半径を物理画素にする）。
fn settings(session: &Session) -> SnapSettings {
    let scale = if session.pixels_per_point.is_finite() && session.pixels_per_point > 0.0 {
        session.pixels_per_point
    } else {
        1.0
    };
    SnapSettings {
        enabled: session.snap.enabled,
        radius_px: session.snap.radius_px * scale,
    }
}

/// 候補を置き換える。変わったときだけ画面の描き直しを求める（印は egui が描く）。
pub(super) fn set_preview(session: &mut Session, target: Option<SnapTarget>) {
    if session.snap_preview != target {
        session.snap_preview = target;
        if let Some(ctx) = &session.egui_ctx {
            ctx.request_repaint();
        }
    }
}

/// 吸着を入れるか。
pub(super) fn set_snap(session: &mut Session, on: bool) {
    if session.snap.enabled != on {
        session.snap.enabled = on;
        set_preview(session, None);
    }
    session.status_text = if on { "吸着: 入" } else { "吸着: 切" }.to_owned();
}

/// 吸着の入・切。
pub(super) fn toggle_snap(session: &mut Session) {
    let next = !session.snap.enabled;
    set_snap(session, next);
}
