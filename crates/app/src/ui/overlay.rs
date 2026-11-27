//! 注釈の印。番号つきの円で、絵文字は使わない。
//!
//! 印は3D表示の上に描くので、そのままだと手前の部品に隠れた点も手前の部品の印に見える。
//! 点を通る視線（透視は目から放射状、平行は視線の向きに平行）を撃ち、手前に別の面があれば
//! 中を塗らない輪と薄い番号で描く。変形中は、動かしている部品の印に差分の行列を掛ける。
//!
//! 断面が入っている間、消えた側にある印は不透明度40%で描き、押せる場所を置かない（要件4.3）。
//! 隠れの判定も断面で消えた側の面を外す（`session::ray_hit`）。

use model_reviewer_core::scene::NodeId;

use crate::command::Command;
use crate::session::Session;
use crate::theme;

use super::ids;
use super::reg;

const RADIUS: f32 = 11.0;
/// 下書き欄の幅と、折り返して見せる行数。
const DRAFT_WIDTH: f32 = 280.0;
const DRAFT_MIN_ROWS: usize = 2;
const DRAFT_MAX_ROWS: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkerPlace {
    pub id: u64,
    pub number: usize,
    pub node: NodeId,
    pub center: egui::Pos2,
    pub selected: bool,
    /// 目から点までの間に別の面がある。索引ができるまでは false。
    pub hidden: bool,
    /// 断面で消えた側にある（薄く描き、押せない）。
    pub faded: bool,
}

/// 消えた側の印の不透明度（計測の点と同じ）。
pub const FADED_OPACITY: f32 = super::measure_view::FADED_OPACITY;

pub fn placed_markers(session: &Session, rect: egui::Rect) -> Vec<MarkerPlace> {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return Vec::new();
    }
    let Some(doc) = session.doc.as_ref() else {
        return Vec::new();
    };
    let Some(review) = session.review.as_ref() else {
        return Vec::new();
    };
    let aspect = aspect_of(session, rect);
    let mut places = Vec::new();
    for (index, note) in review.annotations.iter().enumerate() {
        if !review.visibility.effective(note.node) {
            continue;
        }
        let Some(node) = doc.node(note.node) else {
            continue;
        };
        let (world, moving) = moved_point(
            session,
            note.node,
            node.world.transform_point3(note.local_pos),
        );
        let Some(ndc) = session.camera.project(world, aspect) else {
            continue;
        };
        places.push(MarkerPlace {
            id: note.id,
            number: index + 1,
            node: note.node,
            center: ndc_to_pos(ndc, rect),
            selected: session.selected_annotation == Some(note.id),
            // 動かしている間は、当たり判定の形（まだ動かす前）と見た目が食い違うので判定しない。
            hidden: !moving && is_hidden(session, world, ndc, aspect),
            faded: session.section.is_some_and(|plane| plane.cuts(world)),
        });
    }
    places
}

/// 変形中なら、動かしている範囲の点に差分の行列を掛ける。2つめは動かしている点か。
fn moved_point(session: &Session, node: NodeId, world: glam::Vec3) -> (glam::Vec3, bool) {
    match session.transform.as_ref() {
        Some(transform) if transform.nodes.contains(&node.0) => {
            (transform.delta.transform_point3(world), true)
        }
        _ => (world, false),
    }
}

/// 点を通る視線を1本だけ撃つ。点の面そのものに当たった分は、距離の小さな余裕で除く。
/// 視線は画面上の位置から作るので、透視（目から放射状）と平行（視線の向きに平行）の両方で
/// 描かれた見え方と同じになる。
pub fn is_hidden(session: &Session, world: glam::Vec3, ndc: glam::Vec2, aspect: f32) -> bool {
    if session.doc.is_none() || session.pick_index.is_none() || session.review.is_none() {
        return false;
    }
    let ray = session.camera.screen_ray(ndc, aspect);
    let distance = (world - ray.origin).dot(ray.dir);
    if !distance.is_finite() || distance <= 1.0e-6 {
        return false;
    }
    // 余裕は見ている大きさに比べる。平行の視線は注視点のずっと手前から出るので、出発点からの
    // 距離に比べると大きくなりすぎる（手前の部品を見落とす）。
    let scale = (world - session.camera.eye())
        .length()
        .max(session.camera.distance.abs());
    let margin = scale * 1.0e-3 + 1.0e-5;
    crate::session::ray_hit(session, &ray).is_some_and(|hit| hit.t < distance - margin)
}

pub fn show(ui: &mut egui::Ui, session: &Session, rect: egui::Rect, out: &mut Vec<Command>) {
    let places = placed_markers(session, rect);
    let painter = ui.painter();
    // 隠れた印を先に描き、見えている印がその上に来るようにする。
    let mut order: Vec<&MarkerPlace> = places.iter().collect();
    order.sort_by_key(|place| !place.hidden);
    for place in &order {
        let opacity = if place.faded { FADED_OPACITY } else { 1.0 };
        if place.hidden {
            paint_hidden_pin(painter, place.center, place.number, place.selected, opacity);
        } else {
            paint_pin(
                painter,
                place.center,
                Some(place.number),
                place.selected,
                opacity,
            );
        }
    }
    for place in places.iter().filter(|place| !place.faded) {
        let response = ui.interact(
            egui::Rect::from_center_size(place.center, egui::Vec2::splat(RADIUS * 2.0)),
            ui.scope_id().with(("annotation.pin", place.id)),
            egui::Sense::CLICK,
        );
        reg(&format!("annotation.pin.{}", place.id), &response);
        if response.clicked_by(egui::PointerButton::Primary) {
            out.push(Command::SelectAnnotation(Some(place.id)));
            out.push(Command::Select(Some(place.node)));
        }
    }
    show_draft(ui, session, rect, out);
}

/// 入力位置を移し終えた下書きの目印。置いた場所が変われば別の下書きとみなす。
type FocusedDraft = (u32, [u32; 3]);

fn show_draft(ui: &mut egui::Ui, session: &Session, rect: egui::Rect, out: &mut Vec<Command>) {
    let focus_id = egui::Id::unique("annotation.draft.focused");
    let Some(draft) = &session.draft else {
        // 同じ場所に置き直しても、もう一度入力位置が移るようにする。
        ui.data_mut(|data| data.remove::<FocusedDraft>(focus_id));
        if session.draft_pending() {
            out.push(Command::SetDraftPending(false));
        }
        return;
    };
    let Some(doc) = session.doc.as_ref() else {
        return;
    };
    let visible = session
        .review
        .as_ref()
        .is_some_and(|review| review.visibility.effective(draft.node));
    if !visible {
        return;
    }
    let Some(node) = doc.node(draft.node) else {
        return;
    };
    let (world, _) = moved_point(
        session,
        draft.node,
        node.world.transform_point3(draft.local_pos),
    );
    let Some(ndc) = session.camera.project(world, aspect_of(session, rect)) else {
        return;
    };
    let center = ndc_to_pos(ndc, rect);
    paint_pin(ui.painter(), center, None, false, 1.0);
    let text_id = egui::Id::unique(ids::DRAFT_TEXT);
    let key: FocusedDraft = (draft.node.0, draft.local_pos.to_array().map(f32::to_bits));
    // 枠と余白の分を引いても3D表示の中に収まる幅にする。
    let width = DRAFT_WIDTH.min(rect.width() - 32.0).max(80.0);
    egui::Area::new(egui::Id::unique("annotation.draft.popup"))
        .fixed_pos(center + egui::vec2(RADIUS + 6.0, -RADIUS))
        .constrain_to(rect)
        .order(egui::Order::Foreground)
        .fade_in(false)
        .movable(false)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(width);
                ui.vertical(|ui| {
                    let fresh =
                        ui.data(|data| data.get_temp::<FocusedDraft>(focus_id)) != Some(key);
                    // 新しい下書きは、前の本文を引き継がない。
                    let mut text = if fresh {
                        String::new()
                    } else {
                        ui.data(|data| data.get_temp::<String>(text_id).unwrap_or_default())
                    };
                    // どの部品に付くかを書く前に見せる。取り違えを防ぐ。
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("{} への指摘", node.name))
                                .size(theme::SMALL_SIZE)
                                .color(theme::TEXT_WEAK),
                        )
                        .truncate(),
                    );
                    // 折り返して先頭から全部見せる。4行を超えた分は欄の中でスクロールする。
                    let row_h = ui.text_style_height(&egui::TextStyle::Body);
                    let edit = egui::ScrollArea::vertical()
                        .id_salt("annotation.draft.scroll")
                        .max_height(row_h * DRAFT_MAX_ROWS + 8.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut text)
                                    .hint_text("指摘を入力")
                                    .desired_width(width)
                                    .desired_rows(DRAFT_MIN_ROWS)
                                    // Enter は登録に使う。改行は Shift＋Enter。
                                    .return_key(egui::KeyboardShortcut::new(
                                        egui::Modifiers::SHIFT,
                                        egui::Key::Enter,
                                    )),
                            )
                        })
                        .inner;
                    // 置いた直後に打ち始められるようにする。移さないと、打った文字が
                    // ショートカットとして働く。移すのは1回だけで、以後は奪い返さない。
                    if fresh {
                        edit.request_focus();
                        ui.data_mut(|data| data.insert_temp(focus_id, key));
                    }
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(text_id, text.clone()));
                    reg(ids::DRAFT_TEXT, &edit);
                    // `Response::has_focus` はウィンドウが前面にあることも条件にする。
                    // 自動操作のウィンドウは前面に出ないので、入力欄の位置だけで判定する。
                    let enter = ui.memory(|memory| memory.has_focus(edit.id))
                        && ui.input_mut(|input| {
                            // consume_key は Shift の有無を区別しないので、先に Shift を除く。
                            !input.modifiers.shift
                                && input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                        });
                    let commit = ui
                        .horizontal(|ui| {
                            let commit =
                                ui.add_enabled(!text.trim().is_empty(), egui::Button::new("登録"));
                            ui.label(
                                egui::RichText::new("Enter 登録／Shift＋Enter 改行／Esc 取消")
                                    .size(theme::SMALL_SIZE)
                                    .color(theme::TEXT_WEAK),
                            );
                            commit
                        })
                        .inner;
                    reg(ids::DRAFT_COMMIT, &commit);
                    let committed = (commit.clicked() || enter) && !text.trim().is_empty();
                    if committed {
                        out.push(Command::CommitDraft {
                            text: text.trim().to_owned(),
                        });
                        // 次の下書きが前の本文を引き継がない。Session 側の ctx が無い試験でも消す。
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(text_id, String::new()));
                    }
                    let pending = !committed && !text.trim().is_empty();
                    if pending != session.draft_pending() {
                        out.push(Command::SetDraftPending(pending));
                    }
                });
            });
        });
}

fn paint_pin(
    painter: &egui::Painter,
    center: egui::Pos2,
    number: Option<usize>,
    selected: bool,
    opacity: f32,
) {
    let white = egui::Color32::WHITE.gamma_multiply(opacity);
    painter.circle_filled(center, RADIUS, theme::ANNOTATION.gamma_multiply(opacity));
    painter.circle_stroke(center, RADIUS, egui::Stroke::new(1.5, white));
    if let Some(number) = number {
        painter.text(
            center,
            egui::Align2::CENTER_CENTER,
            number.to_string(),
            egui::FontId::proportional(theme::SMALL_SIZE),
            white,
        );
    }
    if selected {
        painter.circle_stroke(
            center,
            RADIUS + 3.5,
            egui::Stroke::new(2.0, theme::SELECTION.gamma_multiply(opacity)),
        );
    }
}

/// 隠れた点の印。中を塗らない破線の輪と薄い番号で、色に頼らず形で見分けられるようにする。
fn paint_hidden_pin(
    painter: &egui::Painter,
    center: egui::Pos2,
    number: usize,
    selected: bool,
    opacity: f32,
) {
    let color = theme::ANNOTATION.gamma_multiply(0.8 * opacity);
    let segments = 12;
    for i in (0..segments).step_by(2) {
        let a0 = i as f32 / segments as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / segments as f32 * std::f32::consts::TAU;
        let points: Vec<egui::Pos2> = (0..=4)
            .map(|k| {
                let a = a0 + (a1 - a0) * k as f32 / 4.0;
                center + egui::vec2(a.cos(), a.sin()) * RADIUS
            })
            .collect();
        painter.add(egui::Shape::line(points, egui::Stroke::new(2.0, color)));
    }
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        number.to_string(),
        egui::FontId::proportional(theme::SMALL_SIZE),
        theme::TEXT_WEAK.gamma_multiply(0.85 * opacity),
    );
    if selected {
        painter.circle_stroke(
            center,
            RADIUS + 3.5,
            egui::Stroke::new(2.0, theme::SELECTION.gamma_multiply(opacity)),
        );
    }
}

fn aspect_of(session: &Session, rect: egui::Rect) -> f32 {
    let px = session.viewport_px;
    if px[0] > 0 && px[1] > 0 {
        px[0] as f32 / px[1] as f32
    } else if rect.height() > 0.0 {
        rect.width() / rect.height()
    } else {
        1.0
    }
}

fn ndc_to_pos(ndc: glam::Vec2, rect: egui::Rect) -> egui::Pos2 {
    egui::pos2(
        rect.center().x + ndc.x * rect.width() * 0.5,
        rect.center().y - ndc.y * rect.height() * 0.5,
    )
}
