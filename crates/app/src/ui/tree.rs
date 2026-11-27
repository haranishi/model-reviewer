//! 右の部品一覧。見えている行だけを描く。開閉とスクロール位置は egui の一時メモリに置く。
//! 部品は文書（`Session::doc`）から読む。開閉の記憶は `NodeKey` で持つので、部品の追加や削除で
//! 並びがずれても、同じ部品の開閉が保たれる。

use std::collections::HashSet;
use std::ops::Range;

use model_reviewer_core::doc::{DocNode, NodeKey};
use model_reviewer_core::scene::NodeId;

use crate::command::Command;
use crate::session::Session;
use crate::theme;

use super::ids;
use super::reg;

const ROW_H: f32 = 22.0;
const BAND: f32 = 4.0;
const EYE_W: f32 = 18.0;
const TRI_W: f32 = 16.0;
const INDENT: f32 = 14.0;

#[derive(Clone, Debug, Default)]
struct TreeMem {
    model_ptr: usize,
    collapsed: HashSet<NodeKey>,
    selection_ready: bool,
    seen_selection: Option<NodeId>,
    scroll: f32,
    viewport_h: f32,
}

pub fn show(ui: &mut egui::Ui, session: &Session, out: &mut Vec<Command>) -> usize {
    ui.label("部品");
    let response = ui.add_enabled(!session.edits_locked(), egui::Button::new("すべて表示"));
    reg(ids::PARTS_SHOW_ALL, &response);
    if response.clicked() {
        out.push(Command::ShowAll);
    }
    let Some(model_ptr) = session
        .doc
        .as_ref()
        .map(|doc| std::sync::Arc::as_ptr(doc.base()) as usize)
    else {
        return 0;
    };
    let mem_id = egui::Id::unique("parts.tree");
    let mut mem = ui
        .data(|data| data.get_temp::<TreeMem>(mem_id))
        .unwrap_or_default();
    if mem.model_ptr != model_ptr {
        mem = TreeMem {
            model_ptr,
            selection_ready: false,
            ..TreeMem::default()
        };
    }
    let selection = session.selection;
    let (scroll_row, total) = {
        let nodes = session.doc.as_ref().map(|doc| doc.nodes()).unwrap_or(&[]);
        let scroll_row = sync_selection(ui, selection, nodes, &mut mem);
        let total = visible_len(nodes, &mem.collapsed);
        (scroll_row, total)
    };
    let stride = ROW_H + ui.spacing().item_spacing.y;
    let content_h = (stride * total as f32 - ui.spacing().item_spacing.y).max(0.0);
    // 一覧の高さは、このフレームの残りの高さで決まる（`auto_shrink` なし）。前のフレームの値を
    // 使うと、選ぶと同時に下の「寸法」が出て一覧が縮んだとき、選んだ行が隠れたままになる。
    let viewport_h = if ui.available_height() > 1.0 {
        ui.available_height()
    } else {
        mem.viewport_h
    };
    let mut area = egui::ScrollArea::vertical()
        .id_salt("parts.rows")
        .animated(false)
        .auto_shrink([false, false]);
    if let Some(row) = scroll_row
        && let Some(offset) = reveal_offset(mem.scroll, viewport_h, content_h, row, stride, ROW_H)
    {
        area = area.vertical_scroll_offset(offset);
    }
    let mut painted = 0;
    let output = area.show_rows(ui, ROW_H, total, |ui, range| {
        painted = paint_range(ui, session, &mut mem, range, out);
    });
    mem.scroll = output.state.clamped_offset().y;
    mem.viewport_h = output.inner_rect.height();
    ui.data_mut(|data| data.insert_temp(mem_id, mem));
    painted
}

/// 選択行がすでに見えていれば動かさない。見えないときだけ、内容の高さ以内で収まる位置へ送る。
fn reveal_offset(
    scroll: f32,
    viewport_h: f32,
    content_h: f32,
    row: usize,
    stride: f32,
    row_h: f32,
) -> Option<f32> {
    if viewport_h <= 1.0 || stride <= 0.0 {
        return None;
    }
    let max_scroll = (content_h - viewport_h).max(0.0);
    let row_top = row as f32 * stride;
    let row_bottom = row_top + row_h;
    let visible_bottom = scroll + viewport_h;
    let eps = 1.0;
    if row_top >= scroll - eps && row_bottom <= visible_bottom + eps {
        return None;
    }
    let target = if row_top < scroll {
        row_top
    } else {
        row_bottom - viewport_h
    };
    let target = target.clamp(0.0, max_scroll);
    if (target - scroll).abs() < 0.5 {
        None
    } else {
        Some(target)
    }
}

fn sync_selection(
    ui: &egui::Ui,
    selection: Option<NodeId>,
    nodes: &[DocNode],
    mem: &mut TreeMem,
) -> Option<usize> {
    let skip_id = egui::Id::unique("parts.tree.skip");
    let skip = ui.data(|data| data.get_temp::<NodeId>(skip_id));
    ui.data_mut(|data| {
        data.remove::<NodeId>(skip_id);
    });
    // ツリー自身のクリックは、コマンド適用がこのフレームの後なので次のフレームで届く。
    let from_tree = selection.is_some() && skip == selection;
    if !mem.selection_ready {
        mem.selection_ready = true;
        mem.seen_selection = selection;
        return None;
    }
    if selection == mem.seen_selection {
        return None;
    }
    mem.seen_selection = selection;
    if from_tree {
        return None;
    }
    let id = selection?;
    let index = id.0 as usize;
    nodes.get(index)?;
    reveal(nodes, &mut mem.collapsed, id);
    row_of(nodes, &mem.collapsed, index)
}

fn paint_range(
    ui: &mut egui::Ui,
    session: &Session,
    mem: &mut TreeMem,
    range: Range<usize>,
    out: &mut Vec<Command>,
) -> usize {
    let indices = {
        let nodes = session.doc.as_ref().map(|doc| doc.nodes()).unwrap_or(&[]);
        indices_for(nodes, &mem.collapsed, range)
    };
    let count = indices.len();
    for index in indices {
        paint_row(ui, session, mem, index, out);
    }
    count
}

fn paint_row(
    ui: &mut egui::Ui,
    session: &Session,
    mem: &mut TreeMem,
    index: usize,
    out: &mut Vec<Command>,
) {
    let Some((name, depth, subtree_end, key)) = session.doc.as_ref().and_then(|doc| {
        doc.nodes()
            .get(index)
            .map(|node| (node.name.clone(), node.depth, node.subtree_end, node.key))
    }) else {
        return;
    };
    let id = NodeId(index as u32);
    let (own, effective) = visibility(session, id);
    let selected = session.selection == Some(id);
    let width = ui.available_width().max(1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, ROW_H), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    if selected {
        painter.rect_filled(
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + BAND, rect.max.y)),
            0.0,
            theme::SELECTION,
        );
    }
    // 目の印は字下げしない。左端（帯の分だけ）に固定する。
    let eye_rect = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + BAND, rect.min.y),
        egui::vec2(EYE_W, ROW_H),
    );
    let tri_x = eye_rect.max.x + depth as f32 * INDENT;
    let tri_rect =
        egui::Rect::from_min_size(egui::pos2(tri_x, rect.min.y), egui::vec2(TRI_W, ROW_H));
    let has_children = subtree_end as usize > index + 1;
    let locked = session.edits_locked();
    let eye = ui.interact(
        eye_rect,
        egui::Id::unique((ids::PARTS_EYE, id.0)),
        if locked {
            egui::Sense::hover()
        } else {
            egui::Sense::click()
        },
    );
    let expand = has_children.then(|| {
        ui.interact(
            tri_rect,
            egui::Id::unique((ids::PARTS_EXPAND, id.0)),
            egui::Sense::click(),
        )
    });
    let name_x = tri_rect.max.x;
    let name_rect = egui::Rect::from_min_max(egui::pos2(name_x, rect.min.y), rect.max);
    let row = ui.interact(
        name_rect,
        egui::Id::unique((ids::PARTS_ROW, id.0)),
        egui::Sense::click(),
    );

    let eye_label = if !own {
        format!("非表示 {name}")
    } else if !effective {
        format!("親により非表示 {name}")
    } else {
        format!("表示 {name}")
    };
    eye.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, !locked, &eye_label));
    let eye_name = format!("{}.{}", ids::PARTS_EYE, id.0);
    reg(&eye_name, &eye);
    if let Some(expand) = &expand {
        let label = format!("開閉 {name}");
        expand.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
        let expand_name = format!("{}.{}", ids::PARTS_EXPAND, id.0);
        reg(&expand_name, expand);
    }
    row.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::ListItem, true, selected, name.to_string())
    });
    let row_name = format!("{}.{}", ids::PARTS_ROW, id.0);
    reg(&row_name, &row);

    paint_eye(&painter, eye_rect.center(), own, effective, locked);
    if has_children {
        let collapsed = mem.collapsed.contains(&key);
        let icon = egui::Rect::from_center_size(tri_rect.center(), egui::vec2(9.0, 7.0));
        // 0 が下向き（開いている）。畳むと右向き。
        let rotation = if collapsed {
            -std::f32::consts::FRAC_PI_2
        } else {
            0.0
        };
        painter.add(egui::Shape::rotated_triangle(icon, rotation, theme::TEXT));
    }
    let name_color = if own && !effective {
        theme::TEXT_WEAK
    } else {
        theme::TEXT
    };
    let galley = painter.layout_no_wrap(
        name.to_string(),
        egui::FontId::proportional(theme::BODY_SIZE),
        name_color,
    );
    let text_pos = egui::pos2(name_x, rect.center().y - galley.size().y * 0.5);
    painter.galley(text_pos, galley.clone(), name_color);
    // `RichText::strong` は色が濃くなるだけなので、選択は帯に加えてもう一度ずらして太くする。
    if selected {
        painter.galley(text_pos + egui::vec2(0.7, 0.0), galley, name_color);
    }

    if !locked && eye.clicked() {
        out.push(Command::SetVisible {
            node: id,
            visible: !own,
        });
    } else if expand.as_ref().is_some_and(egui::Response::clicked) {
        if !mem.collapsed.remove(&key) {
            mem.collapsed.insert(key);
        }
    } else if row.clicked() {
        out.push(Command::Select(Some(id)));
        ui.data_mut(|data| data.insert_temp(egui::Id::unique("parts.tree.skip"), id));
    }
}

/// 目の形の印。表示中は開いた目、自分で隠すと濃い斜線、親に隠されると薄い目に薄い斜線。
/// 読み込み中で押せない間は、全体を薄くする。
fn paint_eye(
    painter: &egui::Painter,
    center: egui::Pos2,
    own: bool,
    effective: bool,
    locked: bool,
) {
    let faint = theme::TEXT.gamma_multiply(0.45);
    let color = if locked || (own && !effective) {
        faint
    } else {
        theme::TEXT
    };
    let radius = egui::vec2(6.5, 4.0);
    painter.add(egui::Shape::ellipse_stroke(
        center,
        radius,
        egui::Stroke::new(1.3, color),
    ));
    painter.circle_filled(center, 1.8, color);
    if !own || !effective {
        let delta = egui::vec2(6.0, 6.0);
        let width = if own { 1.5 } else { 2.0 };
        painter.line_segment(
            [center - delta, center + delta],
            egui::Stroke::new(width, color),
        );
    }
}

fn visibility(session: &Session, id: NodeId) -> (bool, bool) {
    match &session.review {
        Some(review) => (review.visibility.own(id), review.visibility.effective(id)),
        None => (true, true),
    }
}

fn reveal(nodes: &[DocNode], collapsed: &mut HashSet<NodeKey>, id: NodeId) {
    let mut parent = nodes.get(id.0 as usize).and_then(|node| node.parent);
    while let Some(current) = parent {
        let Some(node) = nodes.get(current.0 as usize) else {
            break;
        };
        collapsed.remove(&node.key);
        parent = node.parent;
    }
}

fn visible_len(nodes: &[DocNode], collapsed: &HashSet<NodeKey>) -> usize {
    if collapsed.is_empty() {
        return nodes.len();
    }
    let mut count = 0;
    let mut index = 0;
    while index < nodes.len() {
        count += 1;
        index = advance(nodes, collapsed, index);
    }
    count
}

fn row_of(nodes: &[DocNode], collapsed: &HashSet<NodeKey>, target: usize) -> Option<usize> {
    if collapsed.is_empty() {
        return (target < nodes.len()).then_some(target);
    }
    let mut row = 0;
    let mut index = 0;
    while index < nodes.len() {
        if index == target {
            return Some(row);
        }
        row += 1;
        index = advance(nodes, collapsed, index);
    }
    None
}

fn indices_for(nodes: &[DocNode], collapsed: &HashSet<NodeKey>, range: Range<usize>) -> Vec<usize> {
    if collapsed.is_empty() {
        return range.filter(|index| *index < nodes.len()).collect();
    }
    let mut found = Vec::with_capacity(range.end.saturating_sub(range.start));
    let mut index = 0;
    let mut row = 0;
    while index < nodes.len() && row < range.end {
        if row >= range.start {
            found.push(index);
        }
        row += 1;
        index = advance(nodes, collapsed, index);
    }
    found
}

fn advance(nodes: &[DocNode], collapsed: &HashSet<NodeKey>, index: usize) -> usize {
    let end = nodes[index].subtree_end as usize;
    if collapsed.contains(&nodes[index].key) && end > index {
        end
    } else {
        index + 1
    }
}

#[cfg(test)]
mod tests {
    use super::reveal_offset;

    #[test]
    fn fitted_list_does_not_move() {
        assert_eq!(reveal_offset(0.0, 200.0, 70.0, 2, 25.0, 22.0), None);
    }

    #[test]
    fn offscreen_row_moves_into_view_without_passing_the_end() {
        let offset = reveal_offset(0.0, 100.0, 1000.0, 10, 25.0, 22.0).expect("移動");
        assert!(offset < 250.0, "行を先頭へ送っています: {offset}");
        assert!(offset <= 900.0);
        assert!(250.0 >= offset - 1.0 && 272.0 <= offset + 100.0 + 1.0);

        let end = reveal_offset(0.0, 50.0, 120.0, 4, 25.0, 22.0).expect("末尾");
        assert!((end - 70.0).abs() < 0.01, "内容を超えています: {end}");

        assert_eq!(reveal_offset(50.0, 100.0, 1000.0, 3, 25.0, 22.0), None);
        let above = reveal_offset(200.0, 100.0, 1000.0, 1, 25.0, 22.0).expect("上");
        assert!((above - 25.0).abs() < 0.01);
    }
}
