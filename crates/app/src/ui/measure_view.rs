//! 計測の線・両端の点・距離の文字を egui で描く。`Command` は積まない（描くだけ）。
//!
//! 3Dの上に重ねるので、明るい線の下に暗い縁を敷いて、明るい面の上でも暗い背景の上でも見える
//! ようにする。距離の文字は線の中点から線と直交する向きへずらし、地の色を敷く。
//!
//! 吸着の候補（次のクリックで置く点）は、ポインタではなく候補の点に、種類ごとに形の違う印
//! （頂点は四角、辺は辺の強調と菱形、面は丸）と言葉で描く。色だけで見分けさせない（要件1.2）。
//! 断面で消えた側にある計測の点は、不透明度40%で描く（要件4.3）。候補と薄さは `show_with` で
//! 渡す（`show` は渡さない）。

use model_reviewer_core::snap::SnapKind;
use model_reviewer_core::units::{UnitLabel, format_distance};

use crate::theme;

use super::measure_panel::kind_name;

/// 線の太さ（縁はこれに足す）。
const LINE_WIDTH: f32 = 2.0;
const HALO: f32 = 2.0;
/// 両端の点の半径。
const POINT_RADIUS: f32 = 5.0;
/// 距離の文字の箱と線の間。
const LABEL_GAP: f32 = 6.0;
/// 候補の印の大きさ（半径）と、辺を強調する線の太さ。
const MARK_RADIUS: f32 = 6.0;
const EDGE_WIDTH: f32 = 4.0;
/// 消えた側の点の不透明度。
pub const FADED_OPACITY: f32 = 0.4;

/// 印の形。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkShape {
    Square,
    Diamond,
    Circle,
}

/// 吸着の種類ごとの印の形（頂点は四角、辺は菱形、面は丸）。
pub fn mark_shape(kind: SnapKind) -> MarkShape {
    match kind {
        SnapKind::Vertex => MarkShape::Square,
        SnapKind::Edge => MarkShape::Diamond,
        SnapKind::Face => MarkShape::Circle,
    }
}

/// 吸着の候補（画面の座標）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapCandidate {
    pub pos: egui::Pos2,
    pub kind: SnapKind,
    /// 辺のときの両端（強調して描く）。
    pub edge: Option<[egui::Pos2; 2]>,
}

/// 吸着の印と、点の薄さ。`point_kinds`・`faded` は `MeasureViewInput::points` と同じ順。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SnapMarks {
    pub candidate: Option<SnapCandidate>,
    /// 置いた点が何に吸着したか。
    pub point_kinds: [SnapKind; 2],
    /// 置いた点が断面で消えた側にあるか。
    pub faded: [bool; 2],
}

/// 候補の印の形と言葉。候補が無いか位置が有限でなければ `None`。
pub fn candidate_mark(marks: &SnapMarks) -> Option<(MarkShape, &'static str)> {
    let candidate = marks.candidate.filter(|c| c.pos.is_finite())?;
    Some((mark_shape(candidate.kind), kind_name(candidate.kind)))
}

/// 置いた点の不透明度（消えた側なら40%）。
pub fn point_opacity(marks: &SnapMarks, index: usize) -> f32 {
    if marks.faded.get(index).copied().unwrap_or(false) {
        FADED_OPACITY
    } else {
        1.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeasureViewInput {
    /// 計測の点の画面上の位置（0〜2個。画面の外や視点の後ろの点は呼び出し側が除く）。
    pub points: Vec<egui::Pos2>,
    /// 2点がそろっていれば距離（ファイルの値のまま）。
    pub distance: Option<f32>,
    pub unit: UnitLabel,
}

/// 距離の文字。2点と距離がそろっていなければ `None`。
pub fn label_text(input: &MeasureViewInput) -> Option<String> {
    if input.points.len() < 2 {
        return None;
    }
    input
        .distance
        .map(|distance| format_distance(distance, input.unit))
}

/// 距離の文字を置く位置（中心）。線の中点から、線と直交する向き（上側。縦の線なら右側）へ、
/// 文字の箱 `size` が線に重ならない距離だけずらす。
pub fn label_anchor(a: egui::Pos2, b: egui::Pos2, size: egui::Vec2) -> egui::Pos2 {
    let mid = a + (b - a) * 0.5;
    let along = b - a;
    let length = along.length();
    let normal = if !length.is_finite() || length < 1.0e-3 {
        egui::vec2(0.0, -1.0)
    } else {
        let n = along.rot90() / length;
        if n.y > 0.0 || (n.y == 0.0 && n.x < 0.0) {
            -n
        } else {
            n
        }
    };
    let clear = normal.x.abs() * size.x * 0.5 + normal.y.abs() * size.y * 0.5;
    mid + normal * (clear + LABEL_GAP)
}

/// 描く。点が無ければ何もしない。吸着の印は描かない。
pub fn show(ui: &mut egui::Ui, input: &MeasureViewInput) {
    show_with(ui, input, &SnapMarks::default());
}

/// 計測の線と点に加え、吸着の候補の印を描く。消えた側の点は薄く描く。
pub fn show_with(ui: &mut egui::Ui, input: &MeasureViewInput, marks: &SnapMarks) {
    let points: Vec<(usize, egui::Pos2)> = input
        .points
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, p)| p.is_finite())
        .take(2)
        .collect();
    let painter = ui.painter();
    let halo = egui::Color32::from_black_alpha(170);
    if let [(ia, a), (ib, b)] = points[..] {
        let opacity = point_opacity(marks, ia).min(point_opacity(marks, ib));
        painter.line_segment(
            [a, b],
            egui::Stroke::new(LINE_WIDTH + HALO * 2.0, halo.gamma_multiply(opacity)),
        );
        painter.line_segment(
            [a, b],
            egui::Stroke::new(LINE_WIDTH, theme::WARNING.gamma_multiply(opacity)),
        );
    }
    for (index, p) in &points {
        let opacity = point_opacity(marks, *index);
        let kind = marks.point_kinds.get(*index).copied().unwrap_or_default();
        paint_mark(
            painter,
            *p,
            mark_shape(kind),
            POINT_RADIUS,
            theme::WARNING.gamma_multiply(opacity),
            egui::Stroke::new(HALO, halo.gamma_multiply(opacity)),
        );
    }
    if let (Some(text), [(_, a), (_, b)]) = (label_text(input), &points[..]) {
        let galley = painter.layout_no_wrap(
            text,
            egui::FontId::proportional(theme::BODY_SIZE),
            theme::TEXT,
        );
        let size = galley.size() + egui::vec2(12.0, 6.0);
        let rect = egui::Rect::from_center_size(label_anchor(*a, *b, size), size);
        painter.rect(
            rect,
            4.0,
            theme::PANEL_BG.gamma_multiply(0.94),
            egui::Stroke::new(1.0, theme::WARNING),
            egui::StrokeKind::Inside,
        );
        painter.galley(rect.center() - galley.size() * 0.5, galley, theme::TEXT);
    }
    paint_candidate(painter, marks);
}

fn paint_candidate(painter: &egui::Painter, marks: &SnapMarks) {
    let (Some(candidate), Some((shape, name))) = (marks.candidate, candidate_mark(marks)) else {
        return;
    };
    let halo = egui::Color32::from_black_alpha(170);
    if let Some([a, b]) = candidate
        .edge
        .filter(|[a, b]| a.is_finite() && b.is_finite())
    {
        painter.line_segment([a, b], egui::Stroke::new(EDGE_WIDTH + HALO * 2.0, halo));
        painter.line_segment([a, b], egui::Stroke::new(EDGE_WIDTH, theme::SELECTION));
    }
    paint_mark(
        painter,
        candidate.pos,
        shape,
        MARK_RADIUS,
        egui::Color32::TRANSPARENT,
        egui::Stroke::new(2.0, theme::SELECTION),
    );
    // 暗い縁（明るい面の上でも見えるように、印の外側に重ねる）。
    paint_mark(
        painter,
        candidate.pos,
        shape,
        MARK_RADIUS + 2.0,
        egui::Color32::TRANSPARENT,
        egui::Stroke::new(1.0, halo),
    );
    let galley = painter.layout_no_wrap(
        name.to_owned(),
        egui::FontId::proportional(theme::SMALL_SIZE),
        theme::TEXT,
    );
    let size = galley.size() + egui::vec2(8.0, 4.0);
    let rect = egui::Rect::from_min_size(
        candidate.pos + egui::vec2(MARK_RADIUS + 6.0, -size.y - MARK_RADIUS),
        size,
    );
    painter.rect(
        rect,
        3.0,
        theme::PANEL_BG.gamma_multiply(0.94),
        egui::Stroke::new(1.0, theme::SELECTION),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.center() - galley.size() * 0.5, galley, theme::TEXT);
}

fn paint_mark(
    painter: &egui::Painter,
    center: egui::Pos2,
    shape: MarkShape,
    radius: f32,
    fill: egui::Color32,
    stroke: egui::Stroke,
) {
    match shape {
        MarkShape::Circle => {
            painter.circle(center, radius, fill, stroke);
        }
        MarkShape::Square => {
            painter.rect(
                egui::Rect::from_center_size(center, egui::Vec2::splat(radius * 1.8)),
                0.0,
                fill,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        MarkShape::Diamond => {
            let r = radius * 1.2;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    center + egui::vec2(0.0, -r),
                    center + egui::vec2(r, 0.0),
                    center + egui::vec2(0.0, r),
                    center + egui::vec2(-r, 0.0),
                ],
                fill,
                stroke,
            ));
        }
    }
}
