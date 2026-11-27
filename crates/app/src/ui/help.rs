//! 操作の説明。操作表と同じ文を出す。表のキーの列は今の割り当て（`KeyLabels`）から作る。

use crate::command::Command;
use crate::keymap::{Action, KeyLabels, Platform};

use super::ids;
use super::reg;

pub fn toggle(ctx: &egui::Context) {
    let id = egui::Id::unique(ids::HELP_OPEN);
    ctx.data_mut(|data| {
        let open = data.get_temp::<bool>(id).unwrap_or(false);
        data.insert_temp(id, !open);
    });
}

/// 既定の文字で出す（接合の前の呼び方）。`show_with` と同じ。
pub fn show(ui: &mut egui::Ui, out: &mut Vec<Command>) {
    show_with(ui, &KeyLabels::default(), Platform::current(), out);
}

/// ヘルプの小窓。表のキーは `keys`（今の割り当て）から作る。表の下に [ショートカットの設定] と、
/// 手順書の場所と [場所を写す] を出す。[ショートカットの設定] はヘルプを閉じてから
/// `SetKeysOpen(true)` を積む（小窓を2枚重ねない）。
pub fn show_with(ui: &mut egui::Ui, keys: &KeyLabels, platform: Platform, out: &mut Vec<Command>) {
    let ctx = ui.ctx().clone();
    let id = egui::Id::unique(ids::HELP_OPEN);
    let open = ctx.data(|data| data.get_temp::<bool>(id).unwrap_or(false));
    if !open {
        return;
    }
    let screen = ctx.content_rect();
    // 幅に余裕があれば広げ、語の途中で折り返さないようにする。
    let inner_w = (screen.width() - 48.0).clamp(280.0, 900.0);
    // 表の下の2行（手順書の場所、閉じる）の分を残す。
    let scroll_h = (screen.height() - 160.0).max(80.0);
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
        // 下にまだ行があると分かるように、入りきらないときは棒を出したままにする。
        super::show_scroll_handle(ui);
        egui::ScrollArea::vertical()
            .id_salt("help.scroll")
            .max_height(scroll_h)
            .auto_shrink([false, true])
            .show(ui, |ui| operations(ui, "help.operations", keys, platform));
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            let mut settings = ui.button(KEYS_BUTTON);
            if let Some(key) = keys.get(Action::AppKeys) {
                settings = settings.on_hover_text(format!("{KEYS_BUTTON}（{key}）"));
            }
            reg(ids::HELP_KEYS, &settings);
            if settings.clicked() {
                ctx.data_mut(|data| data.insert_temp(id, false));
                out.push(Command::SetKeysOpen(true));
            }
            ui.add_space(12.0);
            ui.label(format!("手順書: {GUIDE_PATH}"));
            if let Some(url) = guide_url() {
                let copy = ui
                    .button(GUIDE_COPY)
                    .on_hover_text("手順書の場所をクリップボードへ写します");
                reg(ids::HELP_GUIDE_COPY, &copy);
                if copy.clicked() {
                    ctx.copy_text(url);
                }
            }
        });
        ui.add_space(4.0);
        let close = ui.button("閉じる");
        reg(ids::HELP_CLOSE, &close);
        if close.clicked() {
            ctx.data_mut(|data| data.insert_temp(id, false));
        }
    });
}

/// ヘルプの [ショートカットの設定] の言葉。
pub const KEYS_BUTTON: &str = "ショートカットの設定";
/// 手順書の場所を写すボタンの言葉。
pub const GUIDE_COPY: &str = "場所を写す";

/// 列幅の割合。960×600 の何も開いていない画面で、どの句も途中で折れない配分。
const COLUMN_SHARE: [f32; 4] = [0.21, 0.26, 0.29, 0.24];

/// 手順書の入口（リポジトリの中の場所）。
pub const GUIDE_PATH: &str = "docs/guide/README.md";

/// [場所を写す] で写す文字。リポジトリの場所（`Cargo.toml` の `repository`）が空なら `None`。
pub fn guide_url() -> Option<String> {
    let repository = env!("CARGO_PKG_REPOSITORY").trim_end_matches('/');
    (!repository.is_empty()).then(|| format!("{repository}/blob/HEAD/{GUIDE_PATH}"))
}

/// 操作のキー。未割り当ては「なし」（「A ／ F ／ Home」のように位置で対応する列で使う）。
fn key_or_none(keys: &KeyLabels, action: Action) -> String {
    keys.get(action).unwrap_or("なし").to_owned()
}

/// 「A ／ F ／ Home」。どれも未割り当てなら空。
fn keys_joined(keys: &KeyLabels, actions: &[Action]) -> String {
    if actions.iter().all(|&action| keys.get(action).is_none()) {
        return String::new();
    }
    actions
        .iter()
        .map(|&action| key_or_none(keys, action))
        .collect::<Vec<_>>()
        .join(" ／ ")
}

/// キーがあれば「{キー}{後ろ}」、無ければ `without`。
fn key_then(keys: &KeyLabels, action: Action, after: &str, without: &str) -> String {
    match keys.get(action) {
        Some(key) => format!("{key}{after}"),
        None => without.to_owned(),
    }
}

/// 操作表の行（したいこと・マウス・トラックパッド・キー）。キーの列は今の割り当てから作る。
/// 1行目は見出し。固定のキー（Esc・Enter・Delete）とキーの無い行は、文のまま。
pub fn operation_rows(keys: &KeyLabels) -> Vec<[String; 4]> {
    let tools = [
        (Action::ToolSelect, "選択"),
        (Action::ToolMove, "移動"),
        (Action::ToolRotate, "回転"),
        (Action::ToolScale, "拡大"),
        (Action::ToolShape, "図形"),
        (Action::ToolMeasure, "計測"),
        (Action::ToolAnnotate, "注釈"),
    ]
    .iter()
    .filter_map(|&(action, name)| keys.get(action).map(|key| format!("{key} {name}")))
    .collect::<Vec<_>>()
    .join("／");
    let duplicate = format!(
        "{} ／ Delete（Backspace）",
        key_or_none(keys, Action::EditDuplicate)
    );
    let rows: Vec<(&str, &str, &str, String)> = vec![
        ("したいこと", "マウス", "トラックパッド", "キー".to_owned()),
        (
            "新規作成（空の状態から作り始める）",
            "何も開いていない画面の［新規作成］",
            "同じ",
            keys_joined(keys, &[Action::FileNew]),
        ),
        ("回転", "左ドラッグ", "1本指で押してドラッグ", String::new()),
        (
            "平行移動",
            "右ドラッグ、または Shift＋左ドラッグ",
            "Shift＋ドラッグ、または2本指で押してドラッグ",
            String::new(),
        ),
        (
            "ズーム",
            "ホイール",
            "2本指スクロール、ピンチ",
            String::new(),
        ),
        (
            "選択",
            "左クリック",
            "クリック（タップ）",
            "Esc で解除".to_owned(),
        ),
        (
            "全体表示／選択を表示／初期視点",
            "上部のボタン",
            "同じ",
            keys_joined(
                keys,
                &[
                    Action::ViewFitAll,
                    Action::ViewFitSelection,
                    Action::ViewHome,
                ],
            ),
        ),
        (
            "選択を非表示／すべて表示",
            "目の印、ボタン",
            "同じ",
            keys_joined(keys, &[Action::PartsHideSelected, Action::PartsShowAll]),
        ),
        (
            "注釈を追加",
            "「注釈を追加」→ 表面をクリック",
            "同じ",
            key_then(
                keys,
                Action::ToolAnnotate,
                " で切り替え、Esc で終了",
                "Esc で終了",
            ),
        ),
        (
            "前・右・上から見る",
            "右上の立方体の面を押す（後・左・下も）",
            "同じ",
            keys_joined(
                keys,
                &[Action::ViewFront, Action::ViewRight, Action::ViewTop],
            ),
        ),
        (
            "透視と平行を切り替える",
            "3Dビューの上端のボタン",
            "同じ",
            keys_joined(keys, &[Action::ViewProjection]),
        ),
        (
            "面と線／透かし",
            "3Dビューの上端のボタン",
            "同じ",
            keys_joined(keys, &[Action::DisplayFacesLines, Action::DisplayXray]),
        ),
        (
            "床の格子と軸",
            "上端の「格子」",
            "同じ",
            keys_joined(keys, &[Action::DisplayGrid]),
        ),
        ("道具を選ぶ", "左端の列を押す", "同じ", tools),
        (
            "動かす・回す・拡大する",
            "ハンドルを押してドラッグ、離すと確定",
            "1本指で押してドラッグ",
            "ドラッグ中の Esc で取り消し".to_owned(),
        ),
        (
            "数値で指定する",
            "右の「変形」の欄を押して入力、または欄を左右にドラッグ",
            "同じ",
            "Enter で確定、Esc で戻す".to_owned(),
        ),
        (
            "向きを固定して数値を打つ",
            "移動・回転・拡大の道具で部品を選ぶ",
            "同じ",
            "X・Y・Z で軸を選び、数値を入力".to_owned(),
        ),
        (
            "格子1つぶん動かす",
            "移動の道具で部品を選ぶ",
            "同じ",
            "矢印で移動、Shift で10分の1".to_owned(),
        ),
        (
            "打った量を確定する／やめる",
            "数値の入力中",
            "同じ",
            "Enter で確定、Esc でやめる".to_owned(),
        ),
        (
            "取り消し／やり直し",
            "上部の「戻す」「やり直す」",
            "同じ",
            keys_joined(keys, &[Action::EditUndo, Action::EditRedo]),
        ),
        (
            "図形を足す",
            "左端の「図形」→ 形を押す",
            "同じ",
            keys_joined(keys, &[Action::ToolShape]),
        ),
        ("複製／削除", "部品を選んでからキー", "同じ", duplicate),
        (
            "角や辺に合わせて測る",
            "左端の「計測」→ 印が付いた所でクリック",
            "同じ",
            key_then(
                keys,
                Action::ToolMeasure,
                "、Esc で消す・終了",
                "Esc で消す・終了",
            ),
        ),
        (
            "吸着を入れる・切る",
            "道具の列の横の「吸着」",
            "同じ",
            keys_joined(keys, &[Action::MeasureSnap]),
        ),
        (
            "切って中を見る",
            "上端の「断面」",
            "同じ",
            keys_joined(keys, &[Action::DisplaySection]),
        ),
        (
            "切る軸を選ぶ・向きを反転する",
            "断面の行の X・Y・Z、「反転」",
            "同じ",
            String::new(),
        ),
        (
            "切る位置を動かす",
            "3Dビューの矢印をドラッグ、または位置の欄を左右にドラッグ・数値を入力",
            "同じ",
            "Enter で確定、Esc で戻す".to_owned(),
        ),
        (
            "表示の単位を替える",
            "状態欄の「表示 …」を押す（ファイルと同じ → mm → cm → m）",
            "同じ",
            keys_joined(keys, &[Action::UnitsDisplayNext]),
        ),
        (
            "ファイルの単位の前提を替える",
            "状態欄の「1単位 = …」「STL = …」を押す（STL は開き直す）",
            "同じ",
            String::new(),
        ),
        (
            "STL の上の向きを替える",
            "状態欄の「上 = …」を押す（開き直す）",
            "同じ",
            String::new(),
        ),
        (
            "図形の寸法を決める",
            "「図形」で置く → 右の「寸法」に入力",
            "同じ",
            "Enter で確定、Esc で戻す".to_owned(),
        ),
        (
            "STL を開く",
            "「開く」、またはファイルをドロップ（バイナリと ASCII）",
            "同じ",
            String::new(),
        ),
    ];
    rows.into_iter()
        .map(|(what, mouse, trackpad, key)| {
            [what.to_owned(), mouse.to_owned(), trackpad.to_owned(), key]
        })
        .collect()
}

/// 表の下の注意書き。修飾キーの書き方は今のOSに合わせる。
pub fn key_note(keys: &KeyLabels, platform: Platform) -> String {
    let typing = match keys.get(Action::EditUndo) {
        Some(undo) => format!("1文字のキーと {undo} は、入力欄に文字を打っている間は働かない。"),
        None => "1文字のキーは、入力欄に文字を打っている間は働かない。".to_owned(),
    };
    let other = match platform {
        Platform::Mac => "Windows と Linux では Cmd の代わりに Ctrl。",
        Platform::Other => "macOS では Ctrl の代わりに Cmd。",
    };
    format!("中ボタンは使わない。数字は文字キーの段で、テンキーは使わない。{typing}{other}")
}

/// 操作表。ヘルプと何も開いていない画面で同じ表を出す。列幅は使える幅を割合で分け、
/// 句の切れ目（`phrases`）でだけ折り返す。
pub fn operations(ui: &mut egui::Ui, id_salt: &str, keys: &KeyLabels, platform: Platform) {
    let gaps = ui.spacing().item_spacing.x * 3.0;
    let avail = ui.available_width() - gaps;
    let cols = COLUMN_SHARE.map(|share| (avail * share).max(48.0));
    let rows = operation_rows(keys);
    egui::Grid::new(id_salt)
        .striped(true)
        .min_col_width(0.0)
        .show(ui, |ui| {
            for row in &rows {
                for (text, col) in row.iter().zip(cols) {
                    cell(ui, col, text);
                }
                ui.end_row();
            }
        });
    ui.add_space(8.0);
    ui.add(egui::Label::new(key_note(keys, platform)).wrap());
    ui.add(egui::Label::new(LIMITS).wrap());
}

/// できないこと（要件1.7・3.5・9.8）。
pub const LIMITS: &str = "できないこと: 吸着は当たった部品の中の頂点と辺だけ（別の部品の頂点には付かない）。切り口にふたと輪郭線は無く、見えるのは奥の面の裏側（閉じていない形状や裏返った面では、切っていない所にも切り口の色が出る）。円柱と球は32分割で、大きくすると角が目立つ。STL の単位と上の向きは仮定（既定は mm・Z が上）で、STL としては保存できない（書き出すと GLB になる）。色の欄で変えられるのは基本色の RGB です。テクスチャや金属らしさは表示・編集できません。";

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
