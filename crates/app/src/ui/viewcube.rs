//! 向きの立方体（3Dビューの右上）。8つの角をカメラの回転だけで画面に移し、手前を向いた面
//! （最大3つ）を多角形として描く。面の文字は「前・後・左・右・上・下」。押すと `SnapView`、
//! ドラッグは `Orbit`。見えている面ごとに `ids::VIEWCUBE_*` の名前で小さな矩形を登録する。
//!
//! 画面の向きは `OrbitCamera::axes` と同じ（右・上）。モデルの正面は +Z なので「前」は +Z の面。
//! 角は中心から √3 倍まで離れるので、どの向きでも矩形からはみ出さない大きさにする。

use model_reviewer_core::camera::{OrbitCamera, ViewDir};

use crate::command::Command;
use crate::theme;

use super::ids;
use super::reg;

/// 面を手前向きとみなす下限（法線と視線の向きの内積）。真横の面は細い線になるので出さない。
const FACING_MIN: f32 = 0.05;
/// 立方体の半分の大きさの、矩形の半分に対する割合（角が √3 倍に伸びても収まる）。
const HALF_RATIO: f32 = 0.95 / 1.732_050_8;
/// 自動操作に登録する矩形の大きさ（面の中心まわり）。
const MARKER: f32 = 8.0;
/// 文字を出す面の最小の幅（細い面に文字を詰めない）。
const LABEL_MIN_SPAN: f32 = 14.0;

/// 立方体の入力。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewCubeInput {
    /// カメラの方位と仰角（ラジアン。`OrbitCamera` と同じ）。
    pub yaw: f32,
    pub pitch: f32,
    /// 置く矩形（画面の座標）。
    pub rect: egui::Rect,
}

/// 画面に見えている面1つ。
#[derive(Clone, Debug, PartialEq)]
pub struct CubeFace {
    pub dir: ViewDir,
    pub label: &'static str,
    /// 画面上の多角形（4点）。
    pub polygon: Vec<egui::Pos2>,
}

struct FaceDef {
    dir: ViewDir,
    label: &'static str,
    id: &'static str,
    normal: glam::Vec3,
    /// 面の上の2つの向き（法線と直交）。角は `normal ± u ± v`。
    u: glam::Vec3,
    v: glam::Vec3,
}

const FACES: [FaceDef; 6] = [
    FaceDef {
        dir: ViewDir::Front,
        label: "前",
        id: ids::VIEWCUBE_FRONT,
        normal: glam::Vec3::Z,
        u: glam::Vec3::X,
        v: glam::Vec3::Y,
    },
    FaceDef {
        dir: ViewDir::Back,
        label: "後",
        id: ids::VIEWCUBE_BACK,
        normal: glam::Vec3::NEG_Z,
        u: glam::Vec3::X,
        v: glam::Vec3::Y,
    },
    FaceDef {
        dir: ViewDir::Right,
        label: "右",
        id: ids::VIEWCUBE_RIGHT,
        normal: glam::Vec3::X,
        u: glam::Vec3::Z,
        v: glam::Vec3::Y,
    },
    FaceDef {
        dir: ViewDir::Left,
        label: "左",
        id: ids::VIEWCUBE_LEFT,
        normal: glam::Vec3::NEG_X,
        u: glam::Vec3::Z,
        v: glam::Vec3::Y,
    },
    FaceDef {
        dir: ViewDir::Top,
        label: "上",
        id: ids::VIEWCUBE_TOP,
        normal: glam::Vec3::Y,
        u: glam::Vec3::X,
        v: glam::Vec3::Z,
    },
    FaceDef {
        dir: ViewDir::Bottom,
        label: "下",
        id: ids::VIEWCUBE_BOTTOM,
        normal: glam::Vec3::NEG_Y,
        u: glam::Vec3::X,
        v: glam::Vec3::Z,
    },
];

fn face_def(dir: ViewDir) -> &'static FaceDef {
    FACES
        .iter()
        .find(|face| face.dir == dir)
        .unwrap_or(&FACES[0])
}

/// 手前を向いた面を、奥から手前の順に返す。
pub fn visible_faces(yaw: f32, pitch: f32, rect: egui::Rect) -> Vec<CubeFace> {
    visible_with_facing(yaw, pitch, rect)
        .into_iter()
        .map(|(face, _)| face)
        .collect()
}

/// 面と、手前を向いている度合い（0〜1）。奥から手前の順。
fn visible_with_facing(yaw: f32, pitch: f32, rect: egui::Rect) -> Vec<(CubeFace, f32)> {
    if !rect.is_finite() || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return Vec::new();
    }
    let camera = OrbitCamera {
        yaw,
        pitch,
        ..OrbitCamera::default()
    };
    let (right, up, forward) = camera.axes();
    let toward_eye = -forward;
    let half = rect.width().min(rect.height()) * 0.5 * HALF_RATIO;
    let center = rect.center();
    let to_screen =
        |p: glam::Vec3| egui::pos2(center.x + p.dot(right) * half, center.y - p.dot(up) * half);
    let mut faces: Vec<(CubeFace, f32)> = FACES
        .iter()
        .filter_map(|def| {
            let facing = def.normal.dot(toward_eye);
            if !facing.is_finite() || facing < FACING_MIN {
                return None;
            }
            let n = def.normal;
            let corners = [
                n - def.u - def.v,
                n + def.u - def.v,
                n + def.u + def.v,
                n - def.u + def.v,
            ];
            let polygon: Vec<egui::Pos2> = corners.into_iter().map(to_screen).collect();
            if !polygon.iter().all(|p| p.is_finite()) {
                return None;
            }
            let face = CubeFace {
                dir: def.dir,
                label: def.label,
                polygon,
            };
            Some((face, facing))
        })
        .collect();
    faces.sort_by(|a, b| a.1.total_cmp(&b.1));
    faces
}

/// 点が入っている面（手前のものを優先）。
pub fn hit_face(faces: &[CubeFace], pos: egui::Pos2) -> Option<ViewDir> {
    faces
        .iter()
        .rev()
        .find(|face| inside(&face.polygon, pos))
        .map(|face| face.dir)
}

fn inside(polygon: &[egui::Pos2], pos: egui::Pos2) -> bool {
    if polygon.len() < 3 || !pos.is_finite() {
        return false;
    }
    let mut inside = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let (a, b) = (polygon[i], polygon[j]);
        if (a.y > pos.y) != (b.y > pos.y) {
            let x = (b.x - a.x) * (pos.y - a.y) / (b.y - a.y) + a.x;
            if pos.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

fn centroid(polygon: &[egui::Pos2]) -> egui::Pos2 {
    let sum = polygon
        .iter()
        .fold(egui::Vec2::ZERO, |acc, p| acc + p.to_vec2());
    (sum / polygon.len().max(1) as f32).to_pos2()
}

/// 多角形の短い方の幅（向かい合う辺の中点どうしの距離の小さい方）。
fn min_span(polygon: &[egui::Pos2]) -> f32 {
    if polygon.len() != 4 {
        return 0.0;
    }
    let mid = |a: egui::Pos2, b: egui::Pos2| a + (b - a) * 0.5;
    let a = mid(polygon[0], polygon[1]).distance(mid(polygon[2], polygon[3]));
    let b = mid(polygon[1], polygon[2]).distance(mid(polygon[3], polygon[0]));
    a.min(b)
}

/// 描いて、押されたら `Command` を積む。
pub fn show(ui: &mut egui::Ui, input: &ViewCubeInput, out: &mut Vec<Command>) {
    let rect = input.rect;
    let faces = visible_with_facing(input.yaw, input.pitch, rect);
    if faces.is_empty() {
        return;
    }
    let id = ui.scope_id().with("viewcube");
    let response = ui
        .interact(rect, id, egui::Sense::click_and_drag())
        .on_hover_text("面を押すと、その向きから見ます。ドラッグで回します");
    let plain: Vec<CubeFace> = faces.iter().map(|(face, _)| face.clone()).collect();
    let hovered = response
        .hover_pos()
        .filter(|_| !response.dragged())
        .and_then(|pos| hit_face(&plain, pos));

    let painter = ui.painter_at(rect.expand(2.0));
    for (face, facing) in &faces {
        let lit = 0.55 + 0.45 * facing.clamp(0.0, 1.0);
        let base = theme::PANEL_BG;
        let mut fill = egui::Color32::from_rgb(
            scale_channel(base.r(), lit * 1.6),
            scale_channel(base.g(), lit * 1.6),
            scale_channel(base.b(), lit * 1.6),
        );
        let mut stroke = egui::Stroke::new(1.0, theme::TEXT_WEAK);
        if hovered == Some(face.dir) {
            fill = theme::MODE_ACTIVE;
            stroke = egui::Stroke::new(1.5, theme::SELECTION);
        }
        painter.add(egui::Shape::convex_polygon(
            face.polygon.clone(),
            fill,
            stroke,
        ));
        if min_span(&face.polygon) >= LABEL_MIN_SPAN {
            let color = if hovered == Some(face.dir) {
                theme::TEXT
            } else {
                theme::TEXT.gamma_multiply(0.55 + 0.45 * facing.clamp(0.0, 1.0))
            };
            painter.text(
                centroid(&face.polygon),
                egui::Align2::CENTER_CENTER,
                face.label,
                egui::FontId::proportional(theme::SMALL_SIZE),
                color,
            );
        }
    }
    for (face, _) in &faces {
        let center = centroid(&face.polygon);
        let marker = egui::Rect::from_center_size(center, egui::vec2(MARKER, MARKER));
        reg(
            face_def(face.dir).id,
            &response.clone().with_new_rect(marker),
        );
    }

    if response.dragged() {
        let delta = response.drag_delta();
        if delta != egui::Vec2::ZERO && delta.is_finite() {
            out.push(Command::Orbit(glam::Vec2::new(delta.x, -delta.y)));
        }
    } else if response.clicked()
        && let Some(pos) = response.interact_pointer_pos()
        && let Some(dir) = hit_face(&plain, pos)
    {
        out.push(Command::SnapView(dir));
    }
}

fn scale_channel(value: u8, factor: f32) -> u8 {
    (f32::from(value) * factor).round().clamp(0.0, 255.0) as u8
}
