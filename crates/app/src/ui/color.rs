//! 右の「色」。入力途中は文字のまま持ち、見本を押すか、入力を確定したときにだけ色を渡す。

use model_reviewer_core::color::{SWATCHES, parse_hex, srgb8_to_linear, to_hex};

use super::{ids, reg};
use crate::command::Command;
use crate::session::{ColorState, ColorView};
use crate::theme;

const BAD_HEX: &str = "6桁の16進数を入れてください";
const BAD_RGB: &str = "0〜255 の整数を入れてください";
const FIELD_IDS: [&str; 4] = [ids::COLOR_HEX, ids::COLOR_R, ids::COLOR_G, ids::COLOR_B];

#[derive(Clone)]
struct Fields {
    selection_rev: (u64, u64),
    text: [String; 4],
    error: Option<&'static str>,
}

fn values(state: ColorState) -> [String; 4] {
    match state {
        ColorState::One(rgb) => [
            to_hex(rgb),
            rgb[0].to_string(),
            rgb[1].to_string(),
            rgb[2].to_string(),
        ],
        _ => Default::default(),
    }
}

impl Fields {
    fn commit(&mut self, index: usize, state: ColorState) -> Option<[u8; 3]> {
        let base = values(state);
        if self.text[index] == base[index] {
            return None;
        }
        self.error = None;
        if index == 0 {
            if let Some(rgb) = parse_hex(&self.text[0]) {
                return Some(rgb);
            }
            self.error = Some(BAD_HEX);
        } else if let Ok(value) = self.text[index].trim().parse::<u8>() {
            self.text[index] = value.to_string();
            // 複数色からの数値入力では、まだ空の成分を補わず、3つがそろってから確定する。
            return Some([
                self.text[1].parse().ok()?,
                self.text[2].parse().ok()?,
                self.text[3].parse().ok()?,
            ]);
        } else {
            self.error = Some(BAD_RGB);
        }
        self.text[index].clone_from(&base[index]);
        None
    }
}

fn field(ui: &mut egui::Ui, index: usize, text: &mut String) -> egui::Response {
    let (label, widest) = [("#", "DDDDDD"), ("R", "255"), ("G", "255"), ("B", "255")][index];
    let label_id = ui.label(label).id;
    // 一番長い値が欠けずに見える幅を、画面の字で測る（欄の余白とカーソルの分を足す）。
    let font = egui::TextStyle::Body.resolve(ui.style());
    let width = ui
        .painter()
        .layout_no_wrap(widest.to_owned(), font, egui::Color32::WHITE)
        .size()
        .x
        + 14.0;
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .desired_width(width)
            .id_salt(FIELD_IDS[index]),
    );
    let response = if index == 0 {
        response.accessible_name("16進の色")
    } else {
        response.labelled_by(label_id)
    };
    reg(FIELD_IDS[index], &response);
    response
}

fn current(ui: &mut egui::Ui, state: ColorState) {
    let response = match state {
        ColorState::One([r, g, b]) => {
            let (rect, response) =
                ui.allocate_exact_size(egui::Vec2::splat(18.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, 3.0, egui::Color32::from_rgb(r, g, b));
            ui.label("今の色");
            response.on_hover_text(format!("#{}", to_hex([r, g, b])))
        }
        ColorState::Mixed { count: 99.. } => ui.label("複数の色（99色以上）"),
        ColorState::Mixed { count } => ui.label(format!("複数の色（{count}色）")),
        ColorState::NoFaces => ui.label("この部品には、色を付ける面がありません"),
    };
    reg(ids::COLOR_CURRENT, &response);
}

fn swatches(ui: &mut egui::Ui, state: ColorState) -> Option<[u8; 3]> {
    let mut picked = None;
    for (row, colors) in SWATCHES.chunks(6).enumerate() {
        ui.horizontal(|ui| {
            for (col, &(name, rgb @ [r, g, b])) in colors.iter().enumerate() {
                let selected = state == ColorState::One(rgb);
                let (rect, response) =
                    ui.allocate_exact_size(egui::Vec2::splat(18.0), egui::Sense::click());
                let stroke = if selected {
                    egui::Stroke::new(2.0, theme::SELECTION)
                } else {
                    ui.style().interact(&response).bg_stroke
                };
                ui.painter().rect(
                    rect,
                    3.0,
                    egui::Color32::from_rgb(r, g, b),
                    stroke,
                    egui::StrokeKind::Inside,
                );
                let label = format!("{name} #{}", to_hex(rgb));
                let response = response.on_hover_text(&label);
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::Role::Button,
                        ui.is_enabled(),
                        selected,
                        &label,
                    )
                });
                reg(
                    &format!("{}.{index}", ids::COLOR_SWATCH, index = row * 6 + col),
                    &response,
                );
                if response.clicked() {
                    picked = Some(rgb);
                }
            }
        });
    }
    picked
}

pub fn show(ui: &mut egui::Ui, input: &ColorView, out: &mut Vec<Command>) {
    let enabled = !input.locked && input.state != ColorState::NoFaces;
    let id = ui.scope_id().with("color.fields");
    let mut fields = ui
        .data(|data| data.get_temp::<Fields>(id))
        .filter(|fields| enabled && fields.selection_rev == input.selection_rev)
        .unwrap_or_else(|| Fields {
            selection_rev: input.selection_rev,
            text: values(input.state),
            error: None,
        });

    ui.push_id(input.selection_rev.0, |ui| {
        let hex = ui
            .horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("色").strong());
                current(ui, input.state);
                ui.add_enabled_ui(enabled, |ui| field(ui, 0, &mut fields.text[0]))
                    .inner
            })
            .inner;
        ui.add_enabled_ui(enabled, |ui| {
            let channels = ui
                .horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    [1, 2, 3].map(|i| field(ui, i, &mut fields.text[i]))
                })
                .inner;
            let picked = swatches(ui, input.state);
            let mut typed = None;
            for (index, response) in std::iter::once(hex).chain(channels).enumerate() {
                if enabled && response.lost_focus() {
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        fields.text[index] = values(input.state)[index].clone();
                        fields.error = None;
                        crate::app::mark_text_escape(ui.ctx());
                    } else {
                        typed = fields.commit(index, input.state);
                    }
                }
            }
            if let Some(rgb) = picked.or(typed) {
                fields.text = values(ColorState::One(rgb));
                fields.error = None;
                // 欄を離れるクリックで別の部品を選ぶ場合も、入力した部品へ先に適用する。
                out.insert(0, Command::SetColor(srgb8_to_linear(rgb)));
            }
        });
    });
    if let Some(error) = fields.error {
        ui.colored_label(theme::ERROR, error);
    }
    ui.data_mut(|data| data.insert_temp(id, fields));
    if let Some(note) = input.note {
        ui.indent(ids::COLOR_NOTE, |ui| {
            reg(
                ids::COLOR_NOTE,
                &ui.label(egui::RichText::new(note).small().color(theme::TEXT_WEAK)),
            );
        });
    }
}
