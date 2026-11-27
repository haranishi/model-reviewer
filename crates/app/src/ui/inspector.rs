//! 「変形」の数値の欄（右のパネルの部品一覧の下）。位置・回転（度のオイラー角、XYZの順）・大きさ。
//!
//! 約束: 欄は egui の `DragValue`。値は「選択中の部品と文書の版」を鍵にした一時メモリに置き、
//! `Session` へは `Command` でだけ伝える。ドラッグ中は `PreviewLocal`、離したら `CommitLocal`。
//! 文字で打ったときは欄を離れたフレームで `CommitLocal`。Esc は値を戻して `CancelTransform`。
//! 入力位置の判定に `Response::has_focus` を使わない（前面でないウィンドウで常に偽）。
//! 名前は `ids::INSPECTOR_*`。
//!
//! 文字の入力は `update_while_editing(false)` にして、欄を離れたフレーム（Enter・Tab・ほかの所の
//! クリック）でだけ値が変わるようにする。離れたかどうかは `DragValue` がメモリで判定する。
//! Esc は egui がそのフレームの始めに入力位置を外すので、「離れた」と「Esc が押された」の
//! 組で見分ける（`Response::lost_focus` はメモリだけで決まる）。

use model_reviewer_core::doc::NodeKey;
use model_reviewer_core::gizmo::{Trs, euler_deg, from_euler_deg};

use crate::command::Command;
use crate::theme;

use super::ids;
use super::reg;

/// 部品を選んでいないときの1行。
pub const NO_SELECTION: &str = "部品を選ぶと数値が出ます";

/// 欄の幅（トラックパッドで押しやすく、7桁ほどが入る幅）。
const FIELD_WIDTH: f32 = 64.0;
const FIELD_HEIGHT: f32 = 24.0;

#[derive(Clone, Debug, PartialEq)]
pub enum InspectorValue {
    Editable(Trs),
    /// 欄を無効にする理由（選択なし・傾いた拡大・編集できない状態など）。
    Disabled(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct InspectorInput {
    pub node: Option<NodeKey>,
    pub value: InspectorValue,
    /// 文書の版。版が変われば一時メモリの値を捨てて読み直す。
    pub doc_rev: u64,
}

/// 欄の並び（位置 XYZ・回転 XYZ・大きさ XYZ）。
const FIELDS: [[&str; 3]; 3] = [
    [
        ids::INSPECTOR_POS_X,
        ids::INSPECTOR_POS_Y,
        ids::INSPECTOR_POS_Z,
    ],
    [
        ids::INSPECTOR_ROT_X,
        ids::INSPECTOR_ROT_Y,
        ids::INSPECTOR_ROT_Z,
    ],
    [
        ids::INSPECTOR_SCALE_X,
        ids::INSPECTOR_SCALE_Y,
        ids::INSPECTOR_SCALE_Z,
    ],
];
const ROW_LABELS: [&str; 3] = ["位置", "回転", "大きさ"];

/// 一時メモリ。鍵が変われば捨てる。
#[derive(Clone, Debug, PartialEq)]
struct Memory {
    key: (Option<NodeKey>, u64),
    /// 欄の値（位置・回転の度・大きさ）。
    values: [f64; 9],
    /// ドラッグで `PreviewLocal` を積んだ（離したら `CommitLocal`）。
    previewing: bool,
    /// Esc で取り消したドラッグ（離すまで値を戻し続ける）。
    cancelled: bool,
    /// Esc で文字の入力をやめた欄。egui の `DragValue` は、Esc の次のフレームでも「離れた」と
    /// 判定して打ちかけの文字を当てはめてしまう（Esc のフレームに捨てない）。離れた判定が
    /// 続く間は、この欄の変化を捨てる。
    escaped: Option<usize>,
}

/// `Trs` を欄の値へ。回転は度のオイラー角（X→Y→Z の外部回転）。
pub fn to_fields(trs: &Trs) -> [f64; 9] {
    let deg = euler_deg(trs.rotation);
    [
        f64::from(trs.translation.x),
        f64::from(trs.translation.y),
        f64::from(trs.translation.z),
        clean_degrees(deg.x),
        clean_degrees(deg.y),
        clean_degrees(deg.z),
        f64::from(trs.scale.x),
        f64::from(trs.scale.y),
        f64::from(trs.scale.z),
    ]
}

/// 分けた誤差の小さな角度（「-0°」と出る）を 0 にそろえる。
fn clean_degrees(value: f32) -> f64 {
    if value.abs() < 1.0e-4 {
        0.0
    } else {
        f64::from(value)
    }
}

/// 欄の値を `Trs` へ。
pub fn from_fields(values: &[f64; 9]) -> Trs {
    let v = |i: usize| values[i] as f32;
    Trs {
        translation: glam::Vec3::new(v(0), v(1), v(2)),
        rotation: from_euler_deg(glam::Vec3::new(v(3), v(4), v(5))),
        scale: glam::Vec3::new(v(6), v(7), v(8)),
    }
}

pub fn show(ui: &mut egui::Ui, input: &InspectorInput, out: &mut Vec<Command>) {
    ui.label(egui::RichText::new("変形").strong());
    let trs = match &input.value {
        InspectorValue::Editable(trs) => *trs,
        InspectorValue::Disabled(reason) => {
            let text = if reason.is_empty() {
                NO_SELECTION
            } else {
                reason.as_str()
            };
            ui.label(egui::RichText::new(text).color(theme::TEXT_WEAK));
            return;
        }
    };
    let id = ui.scope_id().with("inspector");
    let key = (input.node, input.doc_rev);
    let base = to_fields(&trs);
    let mut memory = ui
        .data(|data| data.get_temp::<Memory>(id))
        .filter(|memory| memory.key == key)
        .unwrap_or(Memory {
            key,
            values: base,
            previewing: false,
            cancelled: false,
            escaped: None,
        });
    // 動かしていない間は、外の変更（ハンドルの確定など）に合わせる。
    if !memory.previewing && !memory.cancelled {
        memory.values = base;
    }
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));

    egui::Grid::new(id.with("grid"))
        .num_columns(4)
        .spacing(egui::vec2(6.0, 6.0))
        .show(ui, |ui| {
            ui.label("");
            for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                // 軸の色に文字を添える。欄の中央にそろえる。
                ui.add_sized(
                    egui::vec2(FIELD_WIDTH, 18.0),
                    egui::Label::new(egui::RichText::new(name).color(theme::AXES[axis]).strong()),
                );
            }
            ui.end_row();
            for (row, names) in FIELDS.iter().enumerate() {
                ui.allocate_ui_with_layout(
                    egui::vec2(0.0, FIELD_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| ui.label(ROW_LABELS[row]),
                );
                for (axis, name) in names.iter().enumerate() {
                    let index = row * 3 + axis;
                    field(
                        ui,
                        &mut memory,
                        &base,
                        Field {
                            index,
                            name,
                            row,
                            axis,
                        },
                        escape,
                        out,
                    );
                }
                ui.end_row();
            }
        });
    ui.data_mut(|data| data.insert_temp(id, memory));
}

struct Field<'a> {
    index: usize,
    name: &'a str,
    row: usize,
    axis: usize,
}

fn field(
    ui: &mut egui::Ui,
    memory: &mut Memory,
    base: &[f64; 9],
    field: Field<'_>,
    escape: bool,
    out: &mut Vec<Command>,
) {
    let before = memory.values[field.index];
    let speed = match field.row {
        0 => 0.01,
        1 => 0.5,
        _ => 0.005,
    };
    let mut value = before;
    let axis_name = ["X", "Y", "Z"][field.axis];
    let row_name = ROW_LABELS[field.row];
    let suffix = if field.row == 1 { "°" } else { "" };
    let mut drag = egui::DragValue::new(&mut value)
        .speed(speed)
        .max_decimals(4)
        .suffix(suffix)
        .update_while_editing(false);
    if field.row == 2 {
        drag = drag.range(1.0e-4..=f64::MAX).clamp_existing_to_range(false);
    }
    let response = ui.add_sized(egui::vec2(FIELD_WIDTH, FIELD_HEIGHT), drag);
    let response = response.on_hover_text(format!(
        "{row_name} {axis_name}: 左右にドラッグで変える。押して打ち、Enter で確定、Esc で戻す"
    ));
    reg(field.name, &response);

    if memory.escaped == Some(field.index) {
        if response.lost_focus() {
            memory.values[field.index] = base[field.index];
            return;
        }
        memory.escaped = None;
    }
    if memory.cancelled {
        // Esc で取り消したドラッグは、離すまで値を戻し続ける。
        memory.values = *base;
        if !response.dragged() {
            memory.cancelled = false;
        }
        return;
    }
    let changed = value != before && value.is_finite();
    if response.dragged() {
        if escape {
            memory.values = *base;
            memory.cancelled = true;
            if memory.previewing {
                out.push(Command::CancelTransform);
            }
            memory.previewing = false;
            return;
        }
        if changed {
            memory.values[field.index] = value;
            memory.previewing = true;
            out.push(Command::PreviewLocal(from_fields(&memory.values)));
        }
        return;
    }
    if response.drag_stopped() {
        // egui は Esc でドラッグを打ち切る（離したのと同じに見える）。そのときは戻す。
        if escape {
            memory.values = *base;
            if memory.previewing {
                out.push(Command::CancelTransform);
            }
            memory.previewing = false;
            return;
        }
        if changed {
            memory.values[field.index] = value;
        }
        if memory.previewing {
            out.push(Command::CommitLocal(from_fields(&memory.values)));
        }
        memory.previewing = false;
        return;
    }
    if escape && response.lost_focus() {
        // 文字の入力を Esc でやめた。DragValue は値を変えない。この Esc は欄を戻すだけに使い、
        // アプリの Esc の順番（道具を選択へ戻す・選択を外す）へは回さない。
        memory.values = *base;
        memory.escaped = Some(field.index);
        crate::app::mark_text_escape(ui.ctx());
        out.push(Command::CancelTransform);
        return;
    }
    if changed {
        // 文字で打って欄を離れた（または矢印キー）。
        memory.values[field.index] = value;
        out.push(Command::CommitLocal(from_fields(&memory.values)));
    }
}
