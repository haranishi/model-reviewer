//! 下部の状態。高さが限られるので、警告の一覧はポップアップに出す。
//!
//! 単位は2つの部品で出す。左が前提（GLB は「1単位 = m」、STL は「STL = mm（仮定）」）、右が表示
//! （「表示 ファイルと同じ」「表示 mm」）。幅が足りないときは「格子」「表示の単位」「部品の数」の
//! 順に省く。前提の単位と未保存の印は省かない。

use model_reviewer_core::camera::grid_spacing;
use model_reviewer_core::gizmo::Tool;
use model_reviewer_core::units::{UnitLabel, Units};

use crate::command::{Command, PanelKind};
use crate::keymap::Action;
use crate::loader::LoadPhase;
use crate::session::{Session, SourceKind};
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
        let shown = optional_items(ui, session);
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
                if shown.parts {
                    ui.label(parts_text(session));
                }
                if shown.grid {
                    ui.label(grid_text(session))
                        .on_hover_text("床の格子の1マスの長さ");
                }
                if shown.display {
                    let display = ui
                        .button(display_unit_text(session.units))
                        .on_hover_text(session.key_labels.with_key(
                            Action::UnitsDisplayNext,
                            "長さの数値を出す単位。押すと ファイルと同じ → mm → cm → m と替わります",
                        ));
                    reg(ids::STATUS_DISPLAY_UNIT, &display);
                    if display.clicked() {
                        out.push(Command::SetDisplayUnit(session.units.next_display()));
                    }
                }
                // 前提の単位（ファイルの数値1が表す長さ）。数値は換算しない。
                let (text, hint, command) = match session.source_kind {
                    SourceKind::Glb | SourceKind::New => (
                        unit_text(session.units.source),
                        "ファイルの数値1が表す長さの前提です。押すと m → cm → mm と替わります（ファイルの数値は変えません）",
                        Command::SetUnit(session.units.source.next()),
                    ),
                    SourceKind::Stl { unit } => (
                        stl_unit_text(unit),
                        "STL には単位が無いため、この単位と仮定しています。押すと仮定を替えて開き直します",
                        Command::SetStlUnit(unit.next()),
                    ),
                };
                let unit = ui.button(text).on_hover_text(hint);
                reg(ids::STATUS_UNIT, &unit);
                if unit.clicked() {
                    out.push(command);
                }
                if let Some(distance) = crate::session::measure_distance(session) {
                    ui.label(
                        egui::RichText::new(format!(
                            "距離 {}",
                            session.units.format(distance, 4)
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
    let phase = session.load.label().or_else(|| {
        session
            .export_stage()
            .map(|stage| stage.label_ja().to_owned())
    });
    let (text, color) = if let Some(label) = phase {
        let reason = session
            .review_save_block_reason()
            .filter(|reason| *reason == session.status_text)
            .or_else(|| session.edit_block_reason());
        let joined = match reason {
            Some(reason) => format!("{label}  {reason}"),
            None => label,
        };
        (joined, None)
    } else if let Some(reason) = session.edit_block_reason() {
        (reason.to_owned(), None)
    } else if !session.status_text.is_empty() {
        let error = matches!(session.load, LoadPhase::Failed(_)) || session.export_failed();
        (session.status_text.clone(), error.then_some(theme::ERROR))
    } else if session.model.is_some() {
        let hint = if session.section.is_some() && session.tool == Tool::Select {
            SECTION_HINT
        } else {
            tool_hint(session.tool)
        };
        let hint = match (session.tool, session.key_labels.get(Action::MeasureSnap)) {
            (Tool::Measure, Some(key)) => format!("{hint}（{key} で吸着を切り替え）"),
            _ => hint.to_owned(),
        };
        (hint, Some(theme::TEXT_WEAK))
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

/// 幅に合わせて出すか決める部品（省く順は 格子 → 表示の単位 → 部品の数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Optional {
    pub grid: bool,
    pub display: bool,
    pub parts: bool,
}

/// 状態文に残したい最小の幅。
const MIDDLE_MIN: f32 = 160.0;
/// ボタンの左右の余白と部品の間の分。
const BUTTON_PAD: f32 = 16.0;

/// 使える幅に収まるように、省く部品を決める。ファイル名と未保存の印を並べた後に呼ぶ。
fn optional_items(ui: &egui::Ui, session: &Session) -> Optional {
    let mut shown = Optional {
        grid: session.grid_visible,
        display: true,
        parts: true,
    };
    if session.model.is_none() {
        return shown;
    }
    let gap = ui.spacing().item_spacing.x;
    let width = |text: &str, pad: f32| text_width(ui, text) + pad + gap;
    // ファイル名と未保存の印は、呼ぶ時点で並べ終えている（使える幅から引かれている）。
    let mut fixed = width("詳細▴", BUTTON_PAD);
    fixed += width(&unit_button_text(session), BUTTON_PAD);
    if let Some(distance) = crate::session::measure_distance(session) {
        fixed += width(&format!("距離 {}", session.units.format(distance, 4)), 0.0);
    }
    if session
        .model
        .as_ref()
        .is_some_and(|model| !model.warnings.is_empty())
    {
        fixed += width("簡略表示 000 件", BUTTON_PAD);
    }
    if let Some(name) = selected_name(session) {
        fixed += width(&name, 0.0);
    }
    let grid = width(&grid_text(session), 0.0);
    let display = width(&display_unit_text(session.units), BUTTON_PAD);
    let parts = width(&parts_text(session), 0.0);
    let available = ui.available_width() - MIDDLE_MIN;
    let mut total = fixed + if shown.grid { grid } else { 0.0 } + display + parts;
    if total > available && shown.grid {
        shown.grid = false;
        total -= grid;
    }
    if total > available {
        shown.display = false;
        total -= display;
    }
    if total > available {
        shown.parts = false;
    }
    shown
}

fn text_width(ui: &egui::Ui, text: &str) -> f32 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE)
        .size()
        .x
}

fn parts_text(session: &Session) -> String {
    format!(
        "部品 {}・三角形 {}",
        // どちらも文書から（追加・複製・削除で変わる）。
        grouped(session.doc.as_ref().map_or(0, |doc| doc.nodes().len()) as u64),
        grouped(session.placed_triangles()),
    )
}

/// 格子の文字。表示が「ファイルと同じ」なら今までどおり「格子 0.1」、そうでなければ
/// 「格子 100 mm」。
pub fn grid_text(session: &Session) -> String {
    let spacing = grid_spacing(session.camera.distance);
    grid_label(spacing, session.units)
}

/// 格子の間隔（シーンの単位）の文字。
pub fn grid_label(spacing: f32, units: Units) -> String {
    match units.display {
        None => format!("格子 {}", spacing_text(spacing)),
        Some(_) => format!(
            "格子 {} {}",
            spacing_text(units.to_display(spacing)),
            units.shown().suffix()
        ),
    }
}

/// 表示の単位の文字（「表示 ファイルと同じ」「表示 mm」）。
pub fn display_unit_text(units: Units) -> String {
    match units.display {
        None => "表示 ファイルと同じ".to_owned(),
        Some(unit) => format!("表示 {}", unit.suffix()),
    }
}

/// STL の単位の仮定の文字（「STL = mm（仮定）」）。
pub fn stl_unit_text(unit: UnitLabel) -> String {
    format!("STL = {}（仮定）", unit.suffix())
}

fn unit_button_text(session: &Session) -> String {
    match session.source_kind {
        SourceKind::Glb | SourceKind::New => unit_text(session.units.source),
        SourceKind::Stl { unit } => stl_unit_text(unit),
    }
}

/// 道具の使い方の一文（状態欄に知らせが無いときに出す）。
pub fn tool_hint(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "選択: クリックで部品を選ぶ",
        Tool::Move => {
            "移動: 矢印をドラッグ。X・Y・Z で向きを決めて数値を入力。矢印キーで少しずつ移動"
        }
        Tool::Rotate => "回転: 輪をドラッグ。X・Y・Z で向きを決めて角度を入力",
        Tool::Scale => "拡大: 箱をドラッグ。X・Y・Z で向きを決めて倍率を入力",
        Tool::Shape => "図形: 置く形を押す",
        Tool::Measure => "計測: 角や辺に近づけてクリック",
        Tool::Annotate => "注釈: 表面をクリック",
    }
}

/// 断面が入っている間の一文（道具の一文が選択のとき）。
pub const SECTION_HINT: &str = "断面: 矢印をドラッグ、または位置を入力";

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

/// 新規の文書（まだ書き出していない）の名前（要件1.9）。
pub const NEW_DOCUMENT_NAME: &str = "新規（未書き出し）";

fn file_name(session: &Session) -> &str {
    session
        .model_ref
        .as_ref()
        .map(|model| model.file_name.as_str())
        .or(session.loading_file_name.as_deref())
        .or(session.is_new_document().then_some(NEW_DOCUMENT_NAME))
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
