# 設定の画面と保存

## 設定の画面の状態（`Session.keys`）
```rust
pub struct KeysState {
    pub open: bool,
    pub capture: Option<Action>,               // 取り込み中の行。同時に1つ
    pub conflict: Option<KeyConflict>,         // 重なりの確認を待っている
    pub refusal: Option<(Action, Refusal)>,    // 直前に断った理由（その行の下に出す）
    pub reset_all_armed: bool,                 // 「すべて初期値に戻す」の1回目を押した
}
pub struct KeyConflict { pub action: Action, pub chord: Chord, pub holder: Action }
```
検索の欄の文字は画面の部品の持ち物（egui の一時の記憶）で、`Session` には置かない。小窓を閉じる・ダイアログが開く・モデルを開く、のどれでも `capture`・`conflict`・`refusal`・`reset_all_armed` を消す。

## `Command` の追加分（どれも履歴と未保存の印に入らない）
| `Command` | 積む所 | `Session::apply` がすること |
|---|---|---|
| `SetKeysOpen(bool)` | ヘルプのボタン、操作 `app.keys`、[閉じる]、Esc | 開閉。開くときはヘルプを閉じる。閉じるときは上の4つを消す |
| `BeginKeyCapture(Action)` | 行のキーの部品 | `capture` を入れ、`conflict`・`refusal` を消す |
| `CancelKeyCapture` | 取り込み中の Esc | `capture` を消す。割り当ては変えない |
| `CaptureKey(Result<Chord, Refusal>)` | `app::shortcut_commands`（取り込み中に届いたキー） | `Err` なら `refusal` に入れて取り込みを続ける。`Ok` なら `assign(.., false)`。重なれば `conflict` に入れる。通れば `capture` を消して保存 |
| `ResolveKeyConflict { replace: bool }` | [置き換える]・[やめる] | `replace` なら `assign(.., true)` して保存。どちらでも `conflict` と `capture` を消す |
| `ClearKey(Action)` | [外す] | 未割り当てにして保存 |
| `ResetKey(Action)` | [初期値に戻す] | 既定を `assign(.., false)`。重なれば `conflict`。既定が「なし」なら未割り当てにする |
| `ArmResetAllKeys(bool)` | [すべて初期値に戻す] の1回目、ほかの所を押したとき | `reset_all_armed` を入れる・消す |
| `ResetAllKeys` | 2回目 | `reset_all()` して保存 |
- 同じキーを同じ操作へ割り当て直したとき（変わらないとき）は、保存しない。
- 保存に失敗したら、理由を状態欄の知らせに出す（**要確認**: 知らせを出す既存の入口の名前。第1段階の読み込み失敗と同じ場所を使う）。割り当ては `Session.keymap` に残り、その起動の間は効く。

## 画面の部品（`ui/keys.rs`）
- ヘルプと同じ出し方（`egui::Modal`。動かせない。幅は画面に合わせる）。入力用の構造体だけを受け取り、`Session` を読まない。
```rust
pub struct KeysInput { pub rows: Vec<KeyRow>, pub capture: Option<Action>, pub conflict: Option<KeyConflict>,
                       pub refusal: Option<(Action, Refusal)>, pub reset_all_armed: bool, pub store_note: Option<String>, pub platform: Platform }
pub struct KeyRow { pub action: Action, pub key: Option<String>, pub changed: bool, pub default_key: Option<String> }
pub fn show(ui: &mut egui::Ui, input: &KeysInput, out: &mut Vec<Command>);
pub fn matches(row: &KeyRow, filter: &str) -> bool;   // 名前かキーに含む。英字は大文字小文字を区別しない
```
- 自動操作の名前（`ui/ids.rs`）: `keys.filter`、`keys.row.<操作の名前>`（キーの部品）、`keys.clear.<…>`、`keys.reset.<…>`、`keys.conflict.replace`、`keys.conflict.cancel`、`keys.reset_all`、`keys.close`、`help.keys`、`help.guide_copy`。見えている行だけ登録する（既存の一覧と同じ）。
- 取り込みを始めるとき、検索の欄の入力の位置を手放す（入力中のままだと、振り分けが「入力中」で止まり、キーが取り込みに届かない）。
- `store_note` は、保存されない事情があるときだけ出す（「設定の場所が決められないため、変更は保存されません」「新しい版の設定のため、変更は保存されません」「自動操作のため、変更は保存されません」）。

## 取り込みの振り分け（`app::shortcut_commands` の中。設定の画面が開いているとき）
1. ダイアログが開いていれば、今までどおりダイアログが先（設定の画面のキーは何も働かない）。
2. `capture` があるとき: Esc → `CancelKeyCapture`。「コピー・切り取り・貼り付け」の出来事 → `CaptureKey(Err(TextEditing))`。それ以外のキー（修飾キーだけの出来事は egui からは届かない）→ `CaptureKey(Chord::from_event(..))`。1フレームに1つだけ取る。
3. `capture` が無いとき: Esc は、`conflict` があれば `ResolveKeyConflict { replace: false }`、無ければ検索の欄の入力をやめるか `SetKeysOpen(false)`（ヘルプの Esc と同じ順）。
4. どちらの場合も、割り当てた操作は働かせない。

## 設定ファイルの場所（`keymap::store::default_path`）
| OS | 場所 | 決められないとき |
|---|---|---|
| macOS | `$HOME/Library/Application Support/model-reviewer/keymap.json` | `HOME` が無い・空・相対 → `None` |
| Windows | `%APPDATA%\model-reviewer\keymap.json` | `APPDATA` が無い・空・相対 → `None` |
| そのほか | `$XDG_CONFIG_HOME/model-reviewer/keymap.json`。`XDG_CONFIG_HOME` が無い・空・相対なら `$HOME/.config/model-reviewer/keymap.json` | どちらも無ければ `None` |
```rust
pub enum Os { Mac, Windows, Other }
pub fn default_path(os: Os, env: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf>;   // 環境を直接読まない（試験で差し替える）
```
`main.rs` だけが、本物の環境変数を渡して `resolve_path(launch.keymap, 自動操作か, os, env)` を呼ぶ。順は「`--keymap` があればそれ ＞ `--script` があれば `None` ＞ `default_path`」。フォルダは、初めて保存するときに作る（起動しただけでは何も書かない）。

## 形式（版1）
```json
{ "format": "model-reviewer-keymap", "version": 1,
  "bindings": { "tool.move": "T", "tool.rotate": null, "edit.redo": "Shift+Command+Y" } }
```
- `bindings` には、既定と違う操作だけを書く。値は `Chord::to_file` の文字（修飾は `Shift`・`Command` の順、キーは `egui::Key::name()`）か、未割り当ての `null`。`Command` は「macOS は Cmd、ほかは Ctrl」を表す。
- 書くときの並びは名前の順（`serde_json` の既定の並び）。同じ割り当てからは同じバイト列になる。末尾に改行を1つ。
- 知らない操作の名前の項目と、知らない最上位の項目は、読んだ値のまま持ち、書き戻す（新しい版のアプリで足した割り当てを、古い版が消さない）。

## 読むときの決まり（`Store::open`）
| ファイルの様子 | 割り当て | 保存 | 知らせ |
|---|---|---|---|
| 無い | 既定 | できる | なし |
| 読めない（権限など）／通常のファイルでない | 既定 | **しない** | 理由 |
| 64 KiB を超える／JSON でない／`format` が違う／`version` や `bindings` の型が違う | 既定 | できる。初めて保存する直前に、元を `keymap.json.broken` へ名前を替えて残す（すでにあれば置き換える） | 「設定を読めなかったため、初期値で起動しました」 |
| `version` が 2 以上 | 既定 | **しない** | 「新しい版の設定です。このアプリでは変更を保存しません」 |
| 版1 | 下の順で作る | できる | 捨てた件数があれば「割り当て N 件を読めませんでした」 |
割り当ての作り方: ①名前の順に、利用者の項目を1件ずつ当てる。書き方が違う、割り当てられない組、すでに別の利用者の項目が使っている組は、その1件を捨てる。②残りの操作に既定を当てる。既定の組が利用者の項目で使われていれば、その操作は未割り当てにする（利用者の割り当てを優先する）。捨てた項目は、保存のときに書き戻さない（次に保存した時点で消える）。

## 書き込み（`Store::save`）
同じフォルダの `.keymap.json.tmp` に書き、装置へ書き切ってから（`sync_all`）、`fs::rename` で付け替える。失敗したら一時ファイルを消して理由を返す。レビューの保存（`session/persist.rs` の `write_atomic`）と同じ手順だが、文言がレビュー用なので、`store.rs` の中に同じ手順を書く（共有のファイルを変えない）。2つのウィンドウが同時に保存したら、後に書いたほうが残る。ファイルの変更は見張らない。
