//! 計測の線・両端の点・距離の文字を egui で描く。`Command` は積まない（描くだけ）。
//!
//! 3Dの上に重ねるので、明るい線の下に暗い縁を敷いて、明るい面の上でも暗い背景の上でも見える
//! ようにする。距離の文字は線の中点から線と直交する向きへずらし、地の色を敷く。

use model_reviewer_core::units::{UnitLabel, format_distance};

use crate::theme;

/// 線の太さ（縁はこれに足す）。
const LINE_WIDTH: f32 = 2.0;
const HALO: f32 = 2.0;
/// 両端の点の半径。
const POINT_RADIUS: f32 = 5.0;
/// 距離の文字の箱と線の間。
const LABEL_GAP: f32 = 6.0;

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

/// 描く。点が無ければ何もしない。
pub fn show(ui: &mut egui::Ui, input: &MeasureViewInput) {
    let points: Vec<egui::Pos2> = input
        .points
        .iter()
        .copied()
        .filter(|p| p.is_finite())
        .take(2)
        .collect();
    if points.is_empty() {
        return;
    }
    let painter = ui.painter();
    let halo = egui::Color32::from_black_alpha(170);
    if let [a, b] = points[..] {
        painter.line_segment([a, b], egui::Stroke::new(LINE_WIDTH + HALO * 2.0, halo));
        painter.line_segment([a, b], egui::Stroke::new(LINE_WIDTH, theme::WARNING));
    }
    for p in &points {
        painter.circle(
            *p,
            POINT_RADIUS,
            theme::WARNING,
            egui::Stroke::new(HALO, halo),
        );
    }
    if let (Some(text), [a, b]) = (label_text(input), &points[..]) {
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
}
