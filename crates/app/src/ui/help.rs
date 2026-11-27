//! 操作の説明。操作表と同じ文を出す。

use crate::command::Command;

use super::ids;
use super::reg;

pub fn toggle(ctx: &egui::Context) {
    let id = egui::Id::unique(ids::HELP_OPEN);
    ctx.data_mut(|data| {
        let open = data.get_temp::<bool>(id).unwrap_or(false);
        data.insert_temp(id, !open);
    });
}

pub fn show(ui: &mut egui::Ui, out: &mut Vec<Command>) {
    let _ = out;
    let ctx = ui.ctx().clone();
    let id = egui::Id::unique(ids::HELP_OPEN);
    let open = ctx.data(|data| data.get_temp::<bool>(id).unwrap_or(false));
    if !open {
        return;
    }
    let screen = ctx.content_rect();
    // 幅に余裕があれば広げ、語の途中で折り返さないようにする。
    let inner_w = (screen.width() - 48.0).clamp(280.0, 900.0);
    let scroll_h = (screen.height() - 120.0).max(80.0);
    let modal_id = egui::Id::unique("help.modal");
    let mut modal = egui::Modal::new(modal_id);
    modal.area = modal
        .area
        .fade_in(false)
        .movable(false)
        .constrain(true)
        .default_width(inner_w);
    modal.show(&ctx, |ui| {
        let width = inner_w.min(ui.available_width().max(1.0));
        ui.set_min_width(width);
        ui.set_max_width(width);
        ui.label("操作");
        egui::ScrollArea::vertical()
            .id_salt("help.scroll")
            .max_height(scroll_h)
            .auto_shrink([false, true])
            .show(ui, |ui| operations(ui, "help.operations"));
        ui.add_space(8.0);
        let close = ui.button("閉じる");
        reg(ids::HELP_CLOSE, &close);
        if close.clicked() {
            ctx.data_mut(|data| data.insert_temp(id, false));
        }
    });
}

/// 列幅の割合。960×600 の何も開いていない画面で、どの句も途中で折れない配分。
const COLUMN_SHARE: [f32; 4] = [0.21, 0.26, 0.29, 0.24];

/// 操作表。ヘルプと何も開いていない画面で同じ表を出す。列幅は使える幅を割合で分け、
/// 句の切れ目（`phrases`）でだけ折り返す。
pub fn operations(ui: &mut egui::Ui, id_salt: &str) {
    let gaps = ui.spacing().item_spacing.x * 3.0;
    let avail = ui.available_width() - gaps;
    let cols = COLUMN_SHARE.map(|share| (avail * share).max(48.0));
    egui::Grid::new(id_salt)
        .striped(true)
        .min_col_width(0.0)
        .show(ui, |ui| {
            let row = |ui: &mut egui::Ui, what: &str, mouse: &str, trackpad: &str, key: &str| {
                for (text, col) in [what, mouse, trackpad, key].into_iter().zip(cols) {
                    cell(ui, col, text);
                }
                ui.end_row();
            };
            row(ui, "したいこと", "マウス", "トラックパッド", "キー");
            row(ui, "回転", "左ドラッグ", "1本指で押してドラッグ", "");
            row(
                ui,
                "平行移動",
                "右ドラッグ、または Shift＋左ドラッグ",
                "Shift＋ドラッグ、または2本指で押してドラッグ",
                "",
            );
            row(ui, "ズーム", "ホイール", "2本指スクロール、ピンチ", "");
            row(ui, "選択", "左クリック", "クリック（タップ）", "Esc で解除");
            row(
                ui,
                "全体表示／選択を表示／初期視点",
                "上部のボタン",
                "同じ",
                "A ／ F ／ Home",
            );
            row(
                ui,
                "選択を非表示／すべて表示",
                "目の印、ボタン",
                "同じ",
                "H ／ Shift＋H",
            );
            row(
                ui,
                "注釈を追加",
                "「注釈を追加」→ 表面をクリック",
                "同じ",
                "N で切り替え、Esc で終了",
            );
            row(
                ui,
                "前・右・上から見る",
                "右上の立方体の面を押す（後・左・下も）",
                "同じ",
                "1 ／ 3 ／ 7",
            );
            row(
                ui,
                "透視と平行を切り替える",
                "3Dビューの上端のボタン",
                "同じ",
                "5",
            );
            row(
                ui,
                "面と線／透かし",
                "3Dビューの上端のボタン",
                "同じ",
                "Z ／ Shift＋Z",
            );
            row(ui, "床の格子と軸", "上端の「格子」", "同じ", "");
            row(
                ui,
                "道具を選ぶ",
                "左端の列を押す",
                "同じ",
                "V 選択／G 移動／R 回転／S 拡大／M 計測／N 注釈",
            );
            row(
                ui,
                "動かす・回す・拡大する",
                "ハンドルを押してドラッグ、離すと確定",
                "1本指で押してドラッグ",
                "ドラッグ中の Esc で取り消し",
            );
            row(
                ui,
                "数値で指定する",
                "右の「変形」の欄を押して入力、または欄を左右にドラッグ",
                "同じ",
                "Enter で確定、Esc で戻す",
            );
            row(
                ui,
                "取り消し／やり直し",
                "上部の「戻す」「やり直す」",
                "同じ",
                "Cmd＋Z ／ Shift＋Cmd＋Z",
            );
            row(ui, "図形を足す", "左端の「図形」→ 形を押す", "同じ", "");
            row(
                ui,
                "複製／削除",
                "部品を選んでからキー",
                "同じ",
                "Cmd＋D ／ Delete（Backspace）",
            );
            row(
                ui,
                "角や辺に合わせて測る",
                "左端の「計測」→ 印が付いた所でクリック",
                "同じ",
                "M、Esc で消す・終了",
            );
            row(
                ui,
                "吸着を入れる・切る",
                "道具の列の横の「吸着」",
                "同じ",
                "X",
            );
            row(ui, "切って中を見る", "上端の「断面」", "同じ", "C");
            row(
                ui,
                "切る軸を選ぶ・向きを反転する",
                "断面の行の X・Y・Z、「反転」",
                "同じ",
                "",
            );
            row(
                ui,
                "切る位置を動かす",
                "3Dビューの矢印をドラッグ、または位置の欄を左右にドラッグ・数値を入力",
                "同じ",
                "Enter で確定、Esc で戻す",
            );
            row(
                ui,
                "表示の単位を替える",
                "状態欄の「表示 …」を押す（ファイルと同じ → mm → cm → m）",
                "同じ",
                "",
            );
            row(
                ui,
                "ファイルの単位の前提を替える",
                "状態欄の「1単位 = …」「STL = …」を押す（STL は開き直す）",
                "同じ",
                "",
            );
            row(
                ui,
                "図形の寸法を決める",
                "「図形」で置く → 右の「寸法」に入力",
                "同じ",
                "Enter で確定、Esc で戻す",
            );
            row(
                ui,
                "STL を開く",
                "「開く」、またはファイルをドロップ（バイナリと ASCII）",
                "同じ",
                "",
            );
        });
    ui.add_space(8.0);
    ui.add(
        egui::Label::new(
            "中ボタンは使わない。数字は文字キーの段で、テンキーは使わない。1文字のキーと Cmd＋Z は、入力欄に文字を打っている間は働かない。Windows と Linux では Cmd の代わりに Ctrl。",
        )
        .wrap(),
    );
    ui.add(egui::Label::new(LIMITS).wrap());
}

/// できないこと（要件1.7・3.5・9.8）。
pub const LIMITS: &str = "できないこと: 吸着は当たった部品の中の頂点と辺だけ（別の部品の頂点には付かない）。切り口にふたと輪郭線は無く、見えるのは奥の面の裏側（閉じていない形状や裏返った面では、切っていない所にも切り口の色が出る）。円柱と球は32分割で、大きくすると角が目立つ。STL の単位は仮定（既定は mm）で、STL としては保存できない（書き出すと GLB になる）。";

/// 句を詰めて並べ、収まらない句は改行して次の行へ送る。
/// 列より長い句だけは、やむを得ず句の中で折り返す。
fn cell(ui: &mut egui::Ui, col: f32, text: &str) {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let measure = |s: &str| {
        ui.painter()
            .layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE)
            .size()
            .x
    };
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut fits = true;
    for phrase in phrases(text) {
        fits &= measure(phrase.trim_end()) <= col;
        let joined = format!("{line}{phrase}");
        if line.is_empty() || measure(joined.trim_end()) <= col {
            line = joined;
        } else {
            lines.push(line.trim_end().to_owned());
            line = phrase.trim_start().to_owned();
        }
    }
    if !line.is_empty() {
        lines.push(line.trim_end().to_owned());
    }
    let shown = lines.join("\n");
    let response = if fits {
        ui.add(egui::Label::new(shown).extend())
    } else {
        // 列より長い句があるときだけ、列の幅で折り返す。
        ui.scope(|ui| {
            ui.set_max_width(col);
            ui.add(egui::Label::new(shown).wrap())
        })
        .inner
    };
    // 読み上げと試験には、改行を入れる前の文をそのまま渡す。
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
}

/// 「／」「、」「→」「て」「または」と長い語の後の空白の直後、「（」と「または」の直前で区切る。
pub fn phrases(text: &str) -> Vec<&str> {
    const OR: &str = "または";
    let mut out = Vec::new();
    let mut start = 0;
    for (index, ch) in text.char_indices() {
        if (ch == '（' || text[index..].starts_with(OR)) && index > start {
            out.push(&text[start..index]);
            start = index;
        }
        // 「N で」「Esc で」のような短いキー名の後の空白では区切らない。
        let long_word = text[start..index].trim().chars().count() > 4;
        if text[..index + ch.len_utf8()].ends_with(OR)
            || matches!(ch, '／' | '、' | '→' | 'て')
            || (ch == ' ' && long_word)
        {
            let end = index + ch.len_utf8();
            out.push(&text[start..end]);
            start = end;
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::phrases;

    #[test]
    fn phrases_split_only_between_words() {
        assert_eq!(
            phrases("全体表示／選択を表示／初期視点"),
            ["全体表示／", "選択を表示／", "初期視点"]
        );
        assert_eq!(
            phrases("2本指スクロール、ピンチ"),
            ["2本指スクロール、", "ピンチ"]
        );
        assert_eq!(
            phrases("1本指で押してドラッグ"),
            ["1本指で押して", "ドラッグ"]
        );
        assert_eq!(phrases("クリック（タップ）"), ["クリック", "（タップ）"]);
        assert_eq!(
            phrases("ドラッグ、または2本指"),
            ["ドラッグ、", "または", "2本指"]
        );
        assert!(phrases("").is_empty());
    }
}
