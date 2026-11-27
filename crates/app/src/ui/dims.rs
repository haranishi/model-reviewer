//! 右のパネルの「寸法」：外形（選択と子孫の境界箱の大きさ）と、寸法を指定できる図形の欄。
//! 欄の値は表示の単位で出し、`Units::from_display` と `scale_for` で大きさに戻して、ドラッグ中は
//! `PreviewLocal`、確定で `CommitLocal`、Esc で `CancelTransform` を積む。新しい `Command` は
//! 足さない。名前は `ids::DIMS_*`。`Session` を読まず、`DimsView` だけを受け取る。
//!
//! 欄の持ち方は `ui::inspector` と同じ（文書の版を鍵にした一時メモリ、文字の入力は欄を離れた
//! フレームで確定、`has_focus` を使わない）。受け付けない値は欄を元へ戻し、欄の下に理由を出す。

use model_reviewer_core::gizmo::Trs;
use model_reviewer_core::shapes::{DIM_MAX, DIM_MIN, Dims, ShapeKind, ShapeSpec, scale_for};
use model_reviewer_core::units::Units;

use crate::command::Command;
use crate::session::DimsView;
use crate::theme;

use super::ids;
use super::reg;

/// 右のパネルの有効数字（要件2.3）。
pub const PANEL_DIGITS: u8 = 6;
/// 読み込んだ部品に添える1行（要件10.2）。
pub const LOADED_PART: &str = "読み込んだ部品の寸法は直せません。大きさは『変形』で変えられます";
/// 拡大や回転で寸法として表せない図形の理由（`ShapeDims::blocked` が無いときに使う）。
pub const NOT_DIMENSIONAL: &str = "この図形は拡大や回転で形が変わっているため、寸法では直せません";
/// 受け付けない値の理由（要件9.5）。
pub const NOT_POSITIVE: &str = "0より大きい数を入れてください";
pub const TOO_LARGE: &str = "大きすぎます";
pub const TOO_SMALL: &str = "小さすぎます";
/// 範囲の中なのに大きさへ戻せなかったとき。
pub const CANNOT_APPLY: &str = "この寸法にはできません";

const FIELD_WIDTH: f32 = 96.0;
const FIELD_HEIGHT: f32 = 24.0;

/// 図形の種類ごとの欄（名前・見出し）。
pub fn fields_of(kind: ShapeKind) -> &'static [(&'static str, &'static str)] {
    match kind {
        ShapeKind::Cube => &[
            (ids::DIMS_X, "幅 X"),
            (ids::DIMS_Y, "高さ Y"),
            (ids::DIMS_Z, "奥行 Z"),
        ],
        ShapeKind::Sphere => &[(ids::DIMS_D, "直径")],
        ShapeKind::Cylinder => &[(ids::DIMS_D, "直径"), (ids::DIMS_H, "高さ")],
        ShapeKind::Plane => &[(ids::DIMS_X, "幅 X"), (ids::DIMS_Z, "奥行 Z")],
    }
}

/// 寸法を欄の並びの値へ（シーンの単位）。
fn dims_values(dims: Dims) -> Vec<f32> {
    match dims {
        Dims::Cube { x, y, z } => vec![x, y, z],
        Dims::Sphere { d } => vec![d],
        Dims::Cylinder { d, h } => vec![d, h],
        Dims::Plane { x, z } => vec![x, z],
    }
}

/// 欄の並びの値（シーンの単位）から寸法へ。種類と数が合わなければ `None`。
fn dims_from(kind: ShapeKind, values: &[f32]) -> Option<Dims> {
    Some(match (kind, values) {
        (ShapeKind::Cube, [x, y, z]) => Dims::Cube {
            x: *x,
            y: *y,
            z: *z,
        },
        (ShapeKind::Sphere, [d]) => Dims::Sphere { d: *d },
        (ShapeKind::Cylinder, [d, h]) => Dims::Cylinder { d: *d, h: *h },
        (ShapeKind::Plane, [x, z]) => Dims::Plane { x: *x, z: *z },
        _ => return None,
    })
}

/// 寸法として受け付けない理由（シーンの単位の値で判定する）。受け付けるなら `None`。
pub fn reject_reason(scene_value: f32) -> Option<&'static str> {
    if !scene_value.is_finite() || scene_value <= 0.0 {
        Some(NOT_POSITIVE)
    } else if scene_value > DIM_MAX {
        Some(TOO_LARGE)
    } else if scene_value < DIM_MIN {
        Some(TOO_SMALL)
    } else {
        None
    }
}

/// 一時メモリ。鍵が変われば捨てる。
#[derive(Clone, Debug, PartialEq)]
struct Memory {
    key: (u64, u8, u32),
    /// 欄の値（表示の単位）。
    values: Vec<f64>,
    previewing: bool,
    cancelled: bool,
    escaped: Option<usize>,
    /// 受け付けなかった値の理由。次に受け付けるまで出す。
    error: Option<&'static str>,
}

fn kind_code(kind: ShapeKind) -> u8 {
    match kind {
        ShapeKind::Cube => 0,
        ShapeKind::Sphere => 1,
        ShapeKind::Cylinder => 2,
        ShapeKind::Plane => 3,
    }
}

pub fn show(ui: &mut egui::Ui, view: &DimsView, out: &mut Vec<Command>) {
    ui.label(egui::RichText::new("寸法").strong());
    let units = view.units;
    egui::Grid::new(ui.scope_id().with("dims.extent"))
        .num_columns(2)
        .spacing(egui::vec2(10.0, 4.0))
        .show(ui, |ui| {
            for (axis, name) in ["幅 X", "高さ Y", "奥行 Z"].into_iter().enumerate() {
                ui.label(egui::RichText::new(name).color(theme::AXES[axis]));
                let value = view
                    .extent
                    .map(|e| e[axis])
                    .filter(|v| v.is_finite())
                    .unwrap_or(f32::NAN);
                ui.label(units.format(value, PANEL_DIGITS));
                ui.end_row();
            }
        });
    let Some(shape) = view.shape else {
        ui.label(egui::RichText::new(LOADED_PART).color(theme::TEXT_WEAK));
        return;
    };
    let dims = match (shape.dims, shape.blocked) {
        (Some(dims), None) => dims,
        (_, reason) => {
            ui.label(
                egui::RichText::new(reason.unwrap_or(NOT_DIMENSIONAL)).color(theme::TEXT_WEAK),
            );
            return;
        }
    };
    let scene_values = dims_values(dims);
    let base: Vec<f64> = scene_values
        .iter()
        .map(|v| f64::from(units.to_display(*v)))
        .collect();
    let id = ui.scope_id().with("dims");
    let key = (
        view.doc_rev,
        kind_code(shape.spec.kind),
        shape.spec.size.to_bits(),
    );
    let mut memory = ui
        .data(|data| data.get_temp::<Memory>(id))
        .filter(|memory| memory.key == key && memory.values.len() == base.len())
        .unwrap_or(Memory {
            key,
            values: base.clone(),
            previewing: false,
            cancelled: false,
            escaped: None,
            error: None,
        });
    if !memory.previewing && !memory.cancelled {
        memory.values = base.clone();
    }
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let context = Context {
        spec: shape.spec,
        trs: shape.trs,
        units,
        base: &base,
        escape,
    };
    egui::Grid::new(id.with("grid"))
        .num_columns(2)
        .spacing(egui::vec2(10.0, 6.0))
        .show(ui, |ui| {
            for (index, (name, label)) in fields_of(shape.spec.kind).iter().enumerate() {
                ui.label(*label);
                field(ui, &mut memory, &context, index, name, label, out);
                ui.end_row();
            }
        });
    if let Some(error) = memory.error {
        ui.label(egui::RichText::new(error).color(theme::ERROR));
    }
    ui.data_mut(|data| data.insert_temp(id, memory));
}

struct Context<'a> {
    spec: ShapeSpec,
    trs: Trs,
    units: Units,
    base: &'a [f64],
    escape: bool,
}

impl Context<'_> {
    /// 欄の値（表示の単位）から、`scale` だけを差し替えた変形を作る。受け付けなければ理由。
    fn trs_for(&self, values: &[f64]) -> Result<Trs, &'static str> {
        let mut scene = Vec::with_capacity(values.len());
        for value in values {
            let scene_value = self.units.from_display(*value as f32);
            if let Some(reason) = reject_reason(scene_value) {
                return Err(reason);
            }
            scene.push(scene_value);
        }
        let dims = dims_from(self.spec.kind, &scene).ok_or(CANNOT_APPLY)?;
        let scale = scale_for(self.spec, dims, self.trs.scale.y).ok_or(CANNOT_APPLY)?;
        Ok(Trs { scale, ..self.trs })
    }
}

fn field(
    ui: &mut egui::Ui,
    memory: &mut Memory,
    context: &Context<'_>,
    index: usize,
    name: &str,
    label: &str,
    out: &mut Vec<Command>,
) {
    let before = memory.values[index];
    let mut value = before;
    let speed = (before.abs().max(1.0e-3)) * 0.005;
    let suffix = format!(" {}", context.units.shown().suffix());
    let drag = egui::DragValue::new(&mut value)
        .speed(speed)
        .max_decimals(6)
        .suffix(suffix)
        .update_while_editing(false);
    let response = ui
        .add_sized(egui::vec2(FIELD_WIDTH, FIELD_HEIGHT), drag)
        .on_hover_text(format!(
            "{label}: 左右にドラッグで変える。押して打ち、Enter で確定、Esc で戻す"
        ));
    reg(name, &response);

    if memory.escaped == Some(index) {
        if response.lost_focus() {
            memory.values[index] = context.base[index];
            return;
        }
        memory.escaped = None;
    }
    if memory.cancelled {
        memory.values = context.base.to_vec();
        if !response.dragged() {
            memory.cancelled = false;
        }
        return;
    }
    let changed = value != before;
    if response.dragged() {
        if context.escape {
            memory.values = context.base.to_vec();
            memory.cancelled = true;
            if memory.previewing {
                out.push(Command::CancelTransform);
            }
            memory.previewing = false;
            return;
        }
        if changed {
            let mut next = memory.values.clone();
            next[index] = value;
            // ドラッグで受け付けない値（0以下など）へ行ったときは、そこで止める。
            if let Ok(trs) = context.trs_for(&next) {
                memory.values = next;
                memory.previewing = true;
                memory.error = None;
                out.push(Command::PreviewLocal(trs));
            }
        }
        return;
    }
    if response.drag_stopped() {
        if context.escape {
            memory.values = context.base.to_vec();
            if memory.previewing {
                out.push(Command::CancelTransform);
            }
            memory.previewing = false;
            return;
        }
        if memory.previewing {
            match context.trs_for(&memory.values) {
                Ok(trs) => out.push(Command::CommitLocal(trs)),
                Err(_) => out.push(Command::CancelTransform),
            }
        }
        memory.previewing = false;
        return;
    }
    if context.escape && response.lost_focus() {
        memory.values = context.base.to_vec();
        memory.escaped = Some(index);
        crate::app::mark_text_escape(ui.ctx());
        out.push(Command::CancelTransform);
        return;
    }
    if changed {
        // 文字で打って欄を離れた（または矢印キー）。
        let mut next = memory.values.clone();
        next[index] = value;
        match context.trs_for(&next) {
            Ok(trs) => {
                memory.values = next;
                memory.error = None;
                out.push(Command::CommitLocal(trs));
            }
            Err(reason) => {
                memory.values[index] = context.base[index];
                memory.error = Some(reason);
            }
        }
    }
}
