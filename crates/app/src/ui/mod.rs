//! 5領域。部品は `Session` を読み、`Command` を積むだけ。
//! 第2段階で足した部品（`viewcube`・`viewbar`・`toolrail`・`inspector`・`gizmo_view`・
//! `measure_view`）は `Session` を読まず、自分のファイルに置いた入力用の構造体だけを受け取る。
//! 構造体を `Session` から組み立てて置くのは接合（WP-J*）の仕事。

use crate::automation::{WidgetEntry, WidgetFrame};
use crate::command::{Command, PanelKind};
use crate::session::Session;

pub mod annotations;
pub mod details;
pub mod dialogs;
pub mod empty;
pub mod gizmo_view;
pub mod help;
pub mod ids;
pub mod inspector;
pub mod measure_view;
pub mod overlay;
pub mod status;
pub mod toolbar;
pub mod toolrail;
pub mod tree;
pub mod viewbar;
pub mod viewcube;
pub mod viewport;
pub mod views;

pub fn show(ui: &mut egui::Ui, session: &Session, out: &mut Vec<Command>) -> [u32; 2] {
    begin_widgets(ui.ctx());
    let mut parts_open = session.panel_parts;
    let mut notes_open = session.panel_notes;
    let mut details_open = session.panel_details;

    let status = egui::Panel::bottom(ids::PANEL_STATUS)
        .exact_size(28.0)
        .show(ui, |ui| status::show(ui, session, out));
    reg(ids::PANEL_STATUS, &status.response);
    // 高さは固定で、中を縦にスクロールする。大きさを変えられると最後の行が状態欄に重なる。
    let details_h = details::panel_height(ui.ctx().content_rect().height());
    let details = egui::Panel::bottom(ids::PANEL_DETAILS)
        .resizable(false)
        .drag_to_open(false)
        .exact_size(details_h)
        .show_collapsible(ui, &mut details_open, |ui| {
            details::show(ui, session, out);
        });
    if let Some(details) = details {
        reg(ids::PANEL_DETAILS, &details.response);
    }
    egui::Panel::top(ids::PANEL_TOOLBAR)
        .exact_size(36.0)
        .show(ui, |ui| toolbar::show(ui, session, out));
    // 左は「指摘」（注釈と視点）、右は「モデル」（部品の一覧と、その下に変形）。パネルの名前と
    // 開閉の記憶は第1段階のまま（`PANEL_NOTES` が左、`PANEL_PARTS` が右）。
    egui::Panel::left(ids::PANEL_NOTES)
        .default_size(300.0)
        .min_size(200.0)
        .max_size(480.0)
        .show_collapsible(ui, &mut notes_open, |ui| {
            annotations::show(ui, session, out);
            views::show(ui, session, out);
        });
    egui::Panel::right(ids::PANEL_PARTS)
        .default_size(260.0)
        .min_size(180.0)
        .max_size(420.0)
        .show_collapsible(ui, &mut parts_open, |ui| parts_panel(ui, session, out));

    let mut pixels = [1, 1];
    egui::CentralPanel::default().show(ui, |ui| {
        pixels = pixel_size(ui.available_size(), ui.pixels_per_point());
        viewport::show(ui, session, out);
    });

    sync_panel(parts_open, session.panel_parts, PanelKind::Parts, out);
    sync_panel(notes_open, session.panel_notes, PanelKind::Notes, out);
    sync_panel(details_open, session.panel_details, PanelKind::Details, out);
    dialogs::show(ui, session, out);
    help::show(ui, out);
    pixels
}

/// 右のパネル。上に部品の一覧、下に「変形」。一覧は下の場所を除いた高さに収める。
fn parts_panel(ui: &mut egui::Ui, session: &Session, out: &mut Vec<Command>) {
    let below = transform_height(session);
    let list_h = (ui.available_height() - below).max(80.0);
    let width = ui.available_width();
    ui.allocate_ui(egui::vec2(width, list_h), |ui| {
        ui.set_min_height(list_h);
        tree::show(ui, session, out);
    });
    if session.model.is_some() {
        ui.separator();
        inspector::show(ui, &inspector_input(session), out);
    }
}

/// 一覧の下に空ける高さ（「変形」の見出しと、軸の名前・位置・回転・大きさの4行）。
fn transform_height(session: &Session) -> f32 {
    if session.model.is_some() { 172.0 } else { 0.0 }
}

/// 「変形」の欄の入力。選択が無い・読み込み中・分けられない行列なら、押せない理由を渡す。
pub fn inspector_input(session: &Session) -> inspector::InspectorInput {
    use model_reviewer_core::gizmo::decompose;
    use model_reviewer_core::scene::SceneRead;
    let doc_rev = session.history.doc_rev();
    let node = session
        .selection
        .zip(session.doc.as_ref())
        .filter(|(id, doc)| (id.0 as usize) < doc.node_count())
        .and_then(|(id, doc)| doc.node(id));
    let Some(node) = node else {
        return inspector::InspectorInput {
            node: None,
            value: inspector::InspectorValue::Disabled(String::new()),
            doc_rev,
        };
    };
    // 編集できない理由そのものは状態欄に出る。ここは欄が押せないことだけを短く伝える。
    let value = if session.is_exporting() {
        inspector::InspectorValue::Disabled(LOCKED_EXPORT.to_owned())
    } else if session.edits_locked() {
        inspector::InspectorValue::Disabled(LOCKED.to_owned())
    } else {
        match decompose(&node.local) {
            Some(trs) => inspector::InspectorValue::Editable(trs),
            None => inspector::InspectorValue::Disabled(SKEWED.to_owned()),
        }
    };
    inspector::InspectorInput {
        node: Some(node.key),
        value,
        doc_rev,
    }
}

/// 読み込み中に「変形」の欄を押せない理由。
pub const LOCKED: &str = "読み込みが終わると数値を変えられます";

/// 書き出し中に「変形」の欄を押せない理由。
pub const LOCKED_EXPORT: &str = "書き出しが終わると数値を変えられます";

/// 位置・回転・大きさに分けられない部品の理由（要件7.4）。
pub const SKEWED: &str = "傾いた拡大を含むため数値では変えられません。ハンドルでは動かせます";

pub fn reg(name: &str, response: &egui::Response) {
    let rect = response.rect;
    response.ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<WidgetFrame>(egui::Id::NULL)
            .push(WidgetEntry {
                name: name.to_owned(),
                min: [rect.min.x, rect.min.y],
                max: [rect.max.x, rect.max.y],
            });
    });
}

fn begin_widgets(ctx: &egui::Context) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::NULL, WidgetFrame::default()));
}

fn sync_panel(now: bool, was: bool, panel: PanelKind, out: &mut Vec<Command>) {
    if now != was {
        out.push(Command::SetPanel { panel, open: now });
    }
}

fn pixel_size(logical: egui::Vec2, pixels_per_point: f32) -> [u32; 2] {
    [
        to_px(logical.x, pixels_per_point),
        to_px(logical.y, pixels_per_point),
    ]
}

fn to_px(logical: f32, pixels_per_point: f32) -> u32 {
    let value = logical * pixels_per_point;
    if value.is_finite() && value > 0.0 {
        let rounded = value.round();
        if rounded >= u32::MAX as f32 {
            u32::MAX
        } else {
            rounded as u32
        }
    } else {
        1
    }
}
