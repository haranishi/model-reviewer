# 操作とキーの表

## 操作の一覧（42個）と既定
「名前」はファイルに書く文字列で、一度出したら変えない。どの操作も、ポインタの場所によらず働く（要件2.5。2026-11-27 に「働く場所」の区別をやめた）。**太字は、この段階で足す既定。** 新規作成の Cmd+N は creation-basics が先に足している。ほかは第3段階と同じである。
| まとまり | 名前 | 画面の言葉 | 既定 | 積む `Command`（既存のボタンと同じ） |
|---|---|---|---|---|
| ファイル | `file.new` ／ `file.open` ／ `file.save_review` ／ `file.load_review` ／ `file.export` | 新規作成／開く／レビュー保存／レビュー読込／書き出し | Cmd+N／**Cmd+O**／**Cmd+S**／なし／**Cmd+E** | `NewDocument`／`OpenModelDialog`／`SaveReviewDialog`／`LoadReviewDialog`／`ExportGlbDialog` |
| 編集 | `edit.undo` ／ `edit.redo` ／ `edit.duplicate` | 戻す／やり直す／複製 | Cmd+Z／Shift+Cmd+Z／Cmd+D | `Undo`／`Redo`／`DuplicateSelected`（選択があるとき） |
| 道具 | `tool.select` `tool.move` `tool.rotate` `tool.scale` `tool.shape` `tool.measure` | 選択・移動・回転・拡大・図形・計測 | V・G・R・S・なし・M | `SetTool(..)`（いまの道具と違うとき） |
| 道具 | `tool.annotate` | 注釈（切り替え） | N | `SetAnnotateMode(!今)` |
| 道具 | `shape.cube` `shape.sphere` `shape.cylinder` `shape.plane` | 立方体・球・円柱・平面を置く | なし | `AddShape(..)` |
| 道具 | `measure.snap` ／ `measure.clear` | 吸着／計測を消す | X／なし | `ToggleSnap`／`ClearMeasure` |
| 見る | `view.fit_all` ／ `view.fit_selection` ／ `view.home` | 全体表示／選択を表示／初期視点 | A／F／Home | `FitAll`／`FitSelection`／`Home` |
| 見る | `view.front` `view.right` `view.top` ／ `view.back` `view.left` `view.bottom` | 前・右・上／後・左・下から見る | 1・3・7／なし | `SnapView(..)` |
| 見る | `view.projection` | 透視と平行 | 5 | `ToggleProjection` |
| 表示 | `display.faces_lines` ／ `display.xray` | 面と線／透かし（どちらも切り替え。外すと面） | Z／Shift+Z | `SetDisplay(..)`（今の `toggled_display`） |
| 表示 | `display.faces` ／ `display.lines` ／ `display.grid` ／ `display.section` | 面／線／格子／断面 | なし／なし／なし／C | `SetDisplay(..)`／同（線に対応するGPUのとき）／`SetGrid(!今)`／`ToggleSection`（モデルがあるとき） |
| 表示 | `parts.hide_selected` ／ `parts.show_all` ／ `units.display_next` | 選択を非表示／すべて表示／表示の単位 | H／Shift+H／なし | `HideSelected`／`ShowAll`／`SetDisplayUnit(次)` |
| その他 | `app.help` ／ `app.keys` | ヘルプ／ショートカットの設定 | **F1**／なし | ヘルプの開閉／`SetKeysOpen(true)` |
- ボタンが押せない状態（モデルが無い、読み込み中、書き出し中など）では、キーも何もしない。条件は、ボタンの側の条件をそのまま使う（別に書き写さない。使えるなら、ボタンと共有の関数にする）。
- 固定のキーは表に入れない: Esc（今の振り分けの順のまま）、Enter（欄の確定）、Delete・Backspace（`DeleteSelected`）、矢印と、場面つきの X・Y・Z・数字（キーで動かす。[design-keymove.md](design-keymove.md)）。

## 型の約束（`crates/app/src/keymap.rs` と `keymap/`）
```rust
pub enum Action { FileNew, FileOpen, /* … 42個 */ }
impl Action {
    pub const ALL: &'static [Action];
    pub fn id(self) -> &'static str;            pub fn from_id(id: &str) -> Option<Action>;
    pub fn label(self) -> &'static str;         pub fn group(self) -> Group;       // 画面の言葉とまとまり
    pub fn default_chord(self) -> Option<Chord>;
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Chord { pub key: egui::Key, pub shift: bool, pub command: bool }
impl Chord {
    pub fn from_event(key: egui::Key, modifiers: egui::Modifiers) -> Result<Chord, Refusal>;
    pub fn parse(text: &str) -> Result<Chord, Refusal>;   // ファイルの書き方（"Shift+Command+Z"）
    pub fn to_file(self) -> String;                        // parse の逆
    pub fn label(self, platform: Platform) -> String;      // 画面の文字（"Shift+Cmd+Z"・"Shift+Ctrl+Z"）
}
pub enum Refusal { Fixed, Navigation, TextEditing, System, KeyNotAllowed, ShiftDigit, Modifier, Syntax }  // message() が日本語の理由
pub enum Platform { Mac, Other }                           // Platform::current() は cfg! で決める
pub struct Keymap { /* 操作→Option<Chord>、知らない項目、版の番号（変わるたびに増える） */ }
impl Keymap {                                              // Default は既定の割り当て
    pub fn chord(&self, action: Action) -> Option<Chord>;  pub fn action_for(&self, chord: Chord) -> Option<Action>;
    pub fn is_changed(&self, action: Action) -> bool;      pub fn rev(&self) -> u64;
    pub fn assign(&mut self, action: Action, chord: Chord, replace: bool) -> Result<(), Action>; // Err は使っている相手
    pub fn clear(&mut self, action: Action);               pub fn reset_all(&mut self);
    pub fn labels(&self, platform: Platform) -> KeyLabels;
}
#[derive(Clone)] pub struct KeyLabels(/* Arc。操作→Option<文字> */);
impl KeyLabels { pub fn get(&self, action: Action) -> Option<&str>; pub fn with_key(&self, action: Action, name: &str) -> String; } // 「移動（G）」／「図形」
```
- 不変の条件: 2つの操作が同じ `Chord` を持たない。`assign(.., replace: false)` は重なると `Err(相手)` を返して何も変えない。`replace: true` は相手を未割り当てにする。「初期値に戻す」は `assign(action, default, ..)` である（専用の経路を作らない）。
- `Default` と `KeyLabels::default()`（空ではなく既定の文字）を持たせ、入力用の構造体を組み立てている既存の試験が、1行足すだけで通るようにする。

## キーの照合
- 見るのは `egui::Event::Key { pressed: true, repeat: false, .. }` だけ（今と同じ）。`key` と `modifiers` から `Chord::from_event` を作り、`Keymap::action_for` を引く。**取り込みと照合は同じ関数を通す**（取り込めた組は必ず働く）。
- `command` は macOS の Cmd、ほかの OS の Ctrl を表す。Alt と macOS の Control は断る。macOS で Control と Cmd を一緒に押しても同じ扱いになる。
- 修飾は完全一致。`Chord { key: Z, shift: false, command: false }` は、Cmd+Z では働かない。
- 1フレームに複数のキーが届いたら、届いた順に操作を積む（今は、コードに書いた順）。同じフレームで2つのキーを押す場合だけ順が変わりうる。
- 振り分けの入口の順: ダイアログ → ヘルプ → **設定の画面（足す）** → Esc → 入力中なら終わり → **数値の入力の途中ならそのキーだけ（足す）** → **向きの固定と矢印（足す。場面つき）** → 固定のキー（Delete・Backspace）→ 表。
- Enter で入力を確定したフレームは、入力欄を離れた後も削除や割り当てた操作へキーを渡さない。道具を替えたときは本文のある注釈の下書きを残し、そのまま登録できる。
- **「パネルの上」の判定（`over_panel`）は消す。** 代わりに、入力欄でない部品が入力の位置を持っている間だけ、Delete・Backspace・矢印・Enter を捨てる（一覧やボタンを Tab で移っている人の操作を奪わない）。英字・数字・修飾つきの組は、その間も働く。
- 公開の関数 `shortcut_commands(ctx, session)` の形は変えない（既存の試験が呼んでいる）。表は `session.keymap` から読む。

## 割り当てられないキー（`Chord::from_event` と `parse` が返す理由）
| 理由 | 対象 | 画面の文（目安） |
|---|---|---|
| `Fixed` | Esc・Enter・Delete・Backspace | 「…は役目が決まっているため変えられません」 |
| `Navigation` | Tab・Space・矢印 | 「画面の部品の移動に使うため使えません」 |
| `TextEditing` | Cmd+C・X・V・A | 「入力欄の切り取り・コピー・貼り付け・全選択に使うため使えません」 |
| `System` | Cmd+Q・W・H・M | 「終了やウィンドウの操作に使うため使えません」 |
| `ShiftDigit` | Shift+0〜9 | 「Shift と数字の組は、配列によって別の記号になるため使えません」 |
| `Modifier` | Alt（Option）、macOS の Control、修飾キーだけ | 「使える修飾は Shift と Cmd（Ctrl）です」 |
| `KeyNotAllowed` | 上のどれでもなく、使えるキー（要件4.1）でもないもの | 「このキーは割り当てに使えません（英字・数字・F1〜F12・Home など）」 |
`TextEditing` と `System` は、どのOSでも同じに断る（設定ファイルを別のOSへ持っていっても意味が変わらない）。Cmd+C・X・V は、egui がキーではなく「コピー」などの出来事に変えて渡す。取り込みの間は、その出来事を見て `TextEditing` の理由を出す。

## 画面のキーの文字
- 文字は `Chord::label` で作る。順は Shift → Cmd（Ctrl）→ キー、つなぎは半角の「+」。キーの名前は `egui::Key::name()`（**要確認**: 数字は「1」、ページ送りは「PageUp」と出ること）。
- `Session` は `key_labels: KeyLabels` を持ち、`keymap.rev()` が変わったときだけ作り直す。画面の部品は、入力用の構造体の `keys: KeyLabels` で受け取る（`ToolRailInput`・`ViewBarInput`。`toolbar`・`help`・`empty`・`status` は `Session` から読む）。
- 説明の文は、今の「移動（G）: 矢印を…」の括弧の中だけを差し替える。未割り当てなら「図形: 立方体・…」のように括弧を出さない。表示の文（「Z で面と線を切り替え」など）も、キーを埋める形に直す。
- ヘルプの表（`ui/help.rs` の `operations`）は、キーの列を `KeyLabels` から組み立てる。固定のキー（Esc・Enter・Delete）と、キーの無い行は今の文のまま。表の下の注意書きの「Cmd の代わりに Ctrl」は、今のOSの書き方に合わせて1文にする。
- 道具の列の1文字は、`Chord` が修飾なしで、文字が1文字のときだけ出す。
