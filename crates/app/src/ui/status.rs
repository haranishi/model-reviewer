//! 下部の状態。高さが限られるので、警告の一覧はポップアップに出す。

use model_reviewer_core::camera::grid_spacing;
use model_reviewer_core::gizmo::Tool;
use model_reviewer_core::units::{UnitLabel, format_distance};

use crate::command::{Command, PanelKind};
use crate::loader::LoadPhase;
use crate::session::Session;
use crate::theme;

use super::ids;
use super::reg;

pub fn show(ui: &mut egui::Ui, session: &Session, out: &mut Vec<Command>) {
    ui.horizontal(|ui| {
        ui.label(file_name(session));
        if session.doc_unsaved() {
            // 失うと困る状態なので、色と太字で目立たせる。
            ui.label(
                egui::RichText::new("●形の編集 未書き出し")
                    .color(theme::SELECTION)
                    .strong(),
            );
        }
        if session.review_unsaved() {
            ui.label(
                egui::RichText::new("●レビュー未保存")
                    .color(theme::SELECTION)
                    .strong(),
            );
        }
        // 右端の「詳細」と統計・格子・選択名を右から先に並べ、残りの幅で状態文を省略する。
        // 幅960で2つの未保存の印と長い知らせが同時に出ても重ならない。
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if session.panel_details {
                "詳細▴"
            } else {
                "詳細"
            };
            let details = ui.button(label);
            reg(ids::STATUS_DETAILS, &details);
            if details.clicked() {
                out.push(Command::SetPanel {
                    panel: PanelKind::Details,
                    open: !session.panel_details,
                });
            }
            if let Some(model) = &session.model {
                ui.label(format!(
                    "部品 {}・三角形 {}",
                    // どちらも文書から（追加・複製・削除で変わる）。
                    grouped(session.doc.as_ref().map_or(0, |doc| doc.nodes().len()) as u64),
                    grouped(session.placed_triangles()),
                ));
                if session.grid_visible {
                    let spacing = grid_spacing(session.camera.distance);
                    ui.label(format!("格子 {}", spacing_text(spacing)))
                        .on_hover_text("床の格子の1マスの長さ（モデルの単位）");
                }
                // 単位は前提の表示だけ。押すと m → cm → mm と替わり、数値は変えない。
                let unit = ui
                    .button(unit_text(session.unit))
                    .on_hover_text("長さの数値はファイルの値のまま。押すと添える単位を替えます");
                reg(ids::STATUS_UNIT, &unit);
                if unit.clicked() {
                    out.push(Command::SetUnit(session.unit.next()));
                }
                if let Some(distance) = crate::session::measure_distance(session) {
                    ui.label(
                        egui::RichText::new(format!(
                            "距離 {}",
                            format_distance(distance, session.unit)
                        ))
                        .color(theme::WARNING)
                        .strong(),
                    )
                    .on_hover_text("計測の2点の距離（保存されません）");
                }
                let count = model.warnings.len();
                if count > 0 {
                    let response = ui.add(egui::Button::new(
                        egui::RichText::new(format!("簡略表示 {count} 件")).color(theme::WARNING),
                    ));
                    reg(ids::STATUS_WARNINGS, &response);
                    egui::Popup::menu(&response).show(|ui| {
                        ui.set_min_width(320.0);
                        egui::ScrollArea::vertical()
                            .max_height(200.0)
                            .show(ui, |ui| {
                                for warning in &model.warnings {
                                    ui.label(warning.message_ja());
                                }
                            });
                    });
                }
            }
            if let Some(name) = selected_name(session) {
                ui.label(egui::RichText::new(name).strong())
                    .on_hover_text("選択中の部品");
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                show_middle(ui, session);
            });
        });
    });
}

fn show_middle(ui: &mut egui::Ui, session: &Session) {
    let (text, color) = if let Some(label) = session.load.label() {
        let joined = match session.edit_block_reason() {
            Some(reason) => format!("{label}  {reason}"),
            None => label,
        };
        (joined, None)
    } else if let Some(stage) = session.export_stage() {
        let label = stage.label_ja();
        let joined = match session.edit_block_reason() {
            Some(reason) => format!("{label}  {reason}"),
            None => label.to_owned(),
        };
        (joined, None)
    } else if let Some(reason) = session.edit_block_reason() {
        (reason.to_owned(), None)
    } else if !session.status_text.is_empty() {
        let error = matches!(session.load, LoadPhase::Failed(_)) || session.export_failed();
        (session.status_text.clone(), error.then_some(theme::ERROR))
    } else if session.model.is_some() {
        (tool_hint(session.tool).to_owned(), Some(theme::TEXT_WEAK))
    } else {
        (String::new(), None)
    };
    let mut full = if session.notice.is_empty() {
        text
    } else if text.is_empty() {
        session.notice.clone()
    } else {
        format!("{text}  {}", session.notice)
    };
    if session.tool == Tool::Measure && session.model.is_some() {
        full.push_str("  計測は保存されません");
    }
    if full.is_empty() {
        return;
    }
    let rich = match color {
        Some(color) => egui::RichText::new(&full).color(color),
        None => egui::RichText::new(&full),
    };
    ui.add(egui::Label::new(rich).truncate())
        .on_hover_text(&full);
}

/// 道具の使い方の一文（状態欄に知らせが無いときに出す）。
pub fn tool_hint(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "選択: クリックで部品を選ぶ",
        Tool::Move => "移動: 矢印をドラッグ",
        Tool::Rotate => "回転: 輪をドラッグ",
        Tool::Scale => "拡大: 箱をドラッグ",
        Tool::Shape => "図形: 置く形を押す",
        Tool::Measure => "計測: 表面を2回クリック",
        Tool::Annotate => "注釈: 表面をクリック",
    }
}

/// 単位の表示（「1単位 = m」）。
pub fn unit_text(unit: UnitLabel) -> String {
    format!("1単位 = {}", unit.suffix())
}

/// 選択中の部品の名前。長ければ省略する。
fn selected_name(session: &Session) -> Option<String> {
    use model_reviewer_core::scene::SceneRead;
    const MAX_CHARS: usize = 14;
    let id = session.selection?;
    let doc = session.doc.as_ref()?;
    if id.0 as usize >= doc.node_count() {
        return None;
    }
    let name = doc.name(id);
    let name = if name.is_empty() {
        "（名前なし）"
    } else {
        name
    };
    Some(if name.chars().count() > MAX_CHARS {
        let head: String = name.chars().take(MAX_CHARS - 1).collect();
        format!("{head}…")
    } else {
        name.to_owned()
    })
}

fn file_name(session: &Session) -> &str {
    session
        .model_ref
        .as_ref()
        .map(|model| model.file_name.as_str())
        .or(session.loading_file_name.as_deref())
        .unwrap_or("ファイルなし")
}

/// 格子の間隔（10の累乗）を、桁を丸めた文字にする。`0.1`、`10`、とても小さい・大きい値は `1e-7`。
pub fn spacing_text(spacing: f32) -> String {
    if !spacing.is_finite() || spacing <= 0.0 {
        return "-".to_owned();
    }
    let exponent = spacing.log10().round() as i32;
    if !(-6..=9).contains(&exponent) {
        format!("1e{exponent}")
    } else if exponent < 0 {
        format!("{:.*}", exponent.unsigned_abs() as usize, spacing)
    } else {
        format!("{spacing:.0}")
    }
}

/// 3桁ごとに「,」を入れる（状態欄の部品・三角形の数と同じ書き方）。
pub fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index.is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::spacing_text;

    #[test]
    fn grid_spacing_is_written_without_float_noise() {
        assert_eq!(spacing_text(0.1), "0.1");
        assert_eq!(spacing_text(10.0_f32.powi(-3)), "0.001");
        assert_eq!(spacing_text(1.0), "1");
        assert_eq!(spacing_text(100.0), "100");
        assert_eq!(spacing_text(1.0e-9), "1e-9");
        assert_eq!(spacing_text(1.0e12), "1e12");
        assert_eq!(spacing_text(f32::NAN), "-");
    }
}
