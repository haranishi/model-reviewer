//! 断面の操作の帯：軸（X・Y・Z）、位置の欄、反転。押すと `SetSection(Some(plane))` を積む。
//! 軸を替えたら、位置は新しい軸の中央にする。位置の欄は表示の単位で出し、
//! `Units::from_display` でシーンの値へ戻す。名前は `ids::SECTION_*`。
//! `Session` を読まず、入力用の構造体だけを受け取る。
//!
//! 断面は履歴に入れないので、ドラッグ中も確定も同じ `SetSection` を積む。位置の欄の確定の判定は
//! `ui::inspector` と同じく egui のメモリで行う（`has_focus` を使わない）。Esc はドラッグを
//! 掴んだときの位置へ戻す。

use model_reviewer_core::math::Aabb;
use model_reviewer_core::section::{Axis, SectionPlane};
use model_reviewer_core::units::Units;

use crate::command::Command;
use crate::theme;

use super::ids;
use super::reg;
use super::viewbar::{BandSide, Chip, chip};

const BAR_HEIGHT: f32 = 28.0;
const FIELD_WIDTH: f32 = 104.0;
const GROUP_GAP: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionBarInput {
    /// いまの平面。
    pub plane: SectionPlane,
    /// 位置の範囲（見えている全体の境界箱）。
    pub bounds: Aabb,
    pub units: Units,
}

/// 軸の文字。
pub fn axis_name(axis: Axis) -> &'static str {
    match axis {
        Axis::X => "X",
        Axis::Y => "Y",
        Axis::Z => "Z",
    }
}

/// 軸を替えた平面。位置は新しい軸の中央にし、向き（反転）は保つ。
pub fn with_axis(input: &SectionBarInput, axis: Axis) -> SectionPlane {
    SectionPlane {
        flipped: input.plane.flipped,
        ..SectionPlane::centered(axis, &input.bounds)
    }
}

/// 位置の欄の値（表示の単位）から平面を作る。境界箱へ収める。
pub fn with_offset(input: &SectionBarInput, shown_value: f64) -> SectionPlane {
    let offset = input.units.from_display(shown_value as f32);
    SectionPlane {
        offset,
        ..input.plane
    }
    .clamped(&input.bounds)
}

/// フレームをまたいで持つ値。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Memory {
    /// 欄の値（表示の単位）。
    value: f64,
    /// ドラッグを掴んだときの平面（Esc で戻す先）。
    start: Option<SectionPlane>,
    cancelled: bool,
    escaped: bool,
}

pub fn show(ui: &mut egui::Ui, input: &SectionBarInput, out: &mut Vec<Command>) {
    egui::Frame::new()
        .fill(theme::PANEL_BG.gamma_multiply(0.92))
        .stroke(egui::Stroke::new(1.0, theme::SEPARATOR))
        .corner_radius(6)
        .inner_margin(4)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("断面").color(theme::TEXT_WEAK));
                ui.add_space(4.0);
                axes(ui, input, out);
                ui.add_space(GROUP_GAP);
                ui.label("位置");
                offset_field(ui, input, out);
                ui.add_space(GROUP_GAP);
                let label = "反転";
                let response = chip(
                    ui,
                    &Chip {
                        label,
                        selected: input.plane.flipped,
                        enabled: true,
                        reason: None,
                        hint: "消す側を入れ替えます",
                        size: egui::vec2(52.0, BAR_HEIGHT),
                        band: BandSide::Bottom,
                    },
                    None,
                );
                reg(ids::SECTION_FLIP, &response);
                if response.clicked() {
                    out.push(Command::SetSection(Some(SectionPlane {
                        flipped: !input.plane.flipped,
                        ..input.plane
                    })));
                }
            });
        });
}

fn axes(ui: &mut egui::Ui, input: &SectionBarInput, out: &mut Vec<Command>) {
    let list = [
        (Axis::X, ids::SECTION_AXIS_X),
        (Axis::Y, ids::SECTION_AXIS_Y),
        (Axis::Z, ids::SECTION_AXIS_Z),
    ];
    for (axis, id) in list {
        let label = axis_name(axis);
        let hint = format!("{label} 軸に垂直な面で切ります（位置は中央へ）");
        let response = chip(
            ui,
            &Chip {
                label,
                selected: input.plane.axis == axis,
                enabled: true,
                reason: None,
                hint: &hint,
                size: egui::vec2(36.0, BAR_HEIGHT),
                band: BandSide::Bottom,
            },
            None,
        );
        reg(id, &response);
        if response.clicked() && input.plane.axis != axis {
            out.push(Command::SetSection(Some(with_axis(input, axis))));
        }
    }
}

fn offset_field(ui: &mut egui::Ui, input: &SectionBarInput, out: &mut Vec<Command>) {
    let id = ui.scope_id().with("section.offset");
    let base = f64::from(input.units.to_display(input.plane.offset));
    let mut memory = ui
        .data(|data| data.get_temp::<Memory>(id))
        .unwrap_or(Memory {
            value: base,
            start: None,
            cancelled: false,
            escaped: false,
        });
    if memory.start.is_none() && !memory.cancelled {
        memory.value = base;
    }
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let axis = input.plane.axis.index();
    let (lo, hi) = (input.bounds.min[axis], input.bounds.max[axis]);
    let span = if !input.bounds.is_empty() && (hi - lo).is_finite() {
        f64::from(input.units.to_display(hi - lo)).abs()
    } else {
        1.0
    };
    let before = memory.value;
    let mut value = before;
    let suffix = format!(" {}", input.units.shown().suffix());
    let drag = egui::DragValue::new(&mut value)
        .speed((span * 0.002).max(1.0e-6))
        .max_decimals(4)
        .suffix(suffix)
        .update_while_editing(false);
    let response = ui
        .add_sized(egui::vec2(FIELD_WIDTH, BAR_HEIGHT), drag)
        .on_hover_text("切る位置: 左右にドラッグで動かす。押して打ち、Enter で確定、Esc で戻す");
    reg(ids::SECTION_OFFSET, &response);
    handle_field(&response, input, &mut memory, value, escape, ui, out);
    ui.data_mut(|data| data.insert_temp(id, memory));
}

fn handle_field(
    response: &egui::Response,
    input: &SectionBarInput,
    memory: &mut Memory,
    value: f64,
    escape: bool,
    ui: &egui::Ui,
    out: &mut Vec<Command>,
) {
    let base = f64::from(input.units.to_display(input.plane.offset));
    if memory.escaped {
        if response.lost_focus() {
            memory.value = base;
            return;
        }
        memory.escaped = false;
    }
    if memory.cancelled {
        memory.value = base;
        if !response.dragged() {
            memory.cancelled = false;
        }
        return;
    }
    let changed = value != memory.value && value.is_finite();
    if response.dragged() {
        let start = *memory.start.get_or_insert(input.plane);
        if escape {
            out.push(Command::SetSection(Some(start)));
            memory.start = None;
            memory.cancelled = true;
            memory.value = f64::from(input.units.to_display(start.offset));
            return;
        }
        if changed {
            memory.value = value;
            out.push(Command::SetSection(Some(with_offset(input, value))));
        }
        return;
    }
    if response.drag_stopped() {
        if let (true, Some(start)) = (escape, memory.start) {
            out.push(Command::SetSection(Some(start)));
        }
        memory.start = None;
        return;
    }
    if escape && response.lost_focus() {
        // 文字の入力を Esc でやめた。値は変えない。
        memory.value = base;
        memory.escaped = true;
        crate::app::mark_text_escape(ui.ctx());
        return;
    }
    if changed {
        memory.value = value;
        out.push(Command::SetSection(Some(with_offset(input, value))));
    }
}
