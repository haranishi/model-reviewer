# キーで動かす（向きの固定・数値・矢印）

要件11〜14（[requirements-2.md](requirements-2.md)）。2026-11-27 に足した。

## 方針
- **変形の仕組みは増やさない。** いまの変形は「ワールドでの差分の行列 D を見せ、確定で `local` に入れる」形で、ハンドルと数値の欄が同じ経路を通る。キーは3つ目の出どころになるだけで、確定（`CommitTransform`）・取り消し（`CancelTransform`）・履歴・描画（差分の行列だけを送る）は今のものを使う。
- **入力欄を使わない。** 打った文字は `Session` が持ち、状態欄に出す。入力の位置を持つ部品が無いので、「前面でないウィンドウでは入力の位置が偽」「日本語入力の変換」の罠を避けられる。キーは `app::shortcut_commands` が読む（キーを読む場所を1つに保つ）。
- **量から差分の行列を作る関数は `core` に置き、カメラを使わない。** ハンドルのドラッグは視線と軸が平行だと値が定まらないが、数値には向きの制約が無い。

## 型の約束
```rust
// core::gizmo（足す）
/// 軸に沿った量から、ワールドでの差分の行列を作る。`axes` と `pivot` は GizmoFrame と同じもの。
/// 移動は距離（シーンの単位）、回転は度、拡大は倍率。有限でない量と、0 以下の倍率は None。ScaleAll は None。
pub fn key_delta(handle: Handle, pivot: Vec3, axes: [Vec3; 3], value: f32) -> Option<DragState>;

// app::session::edit（変える）
pub enum TransformSource { Gizmo(Drag), Field, Keys(KeyEntry) }
pub struct KeyEntry { pub handle: Handle, pub text: String }     // text は「-12.5」など。空もある
// Transform::handle() は Keys でも Some を返す（ハンドルの強調と、履歴の名前「移動」「回転」「拡大」がそのまま働く）

// app::session::keymove（新規）
pub fn parse_amount(text: &str) -> Option<f64>;                  // 空と "-" は Some(0.0)。数字・小数点・負号以外は None
pub fn format_amount(value: f64) -> String;                      // 有効6桁。末尾の 0 と小数点を落とす
pub fn step_of(handle: Handle, grid: f32, fine: bool) -> f64;    // 移動は格子（シーンの単位）、回転 15・1、拡大 0.1・0.01
```
`Command` に足すのは3つ。どれも、確定するまで履歴と未保存の印に入らない。
| `Command` | 積む所 | `Session::apply` がすること |
|---|---|---|
| `BeginKeyTransform(Axis)` | X・Y・Z（道具が移動・回転・拡大で、選択がある） | いまの道具と軸から `Handle` を決める。変形が無ければ `Keys` の変形を始める（D は単位行列）。`Keys` の途中なら軸だけ替えて D を作り直す。ハンドルを掴んでいる途中なら何もしない |
| `KeyTransformInput(KeyInput)` | 数値の入力の間の数字・小数点・負号・Backspace・矢印 | `text` を直し、量を読んで D と `readout` を作り直す。読めなければ D を変えずに理由を状態欄へ |
| `Nudge { axis, sign, fine }` | 矢印（移動の道具で、選択があり、変形の途中でない） | `key_delta` で D を作り、今の確定の共通部（`commit_local_matrix`）で入れる。履歴の名前は「移動」 |
```rust
pub enum KeyInput { Char(char), Backspace, Step { up: bool, fine: bool } }   // Char は 0〜9・'.'・'-' だけ
```
- 負号は先頭だけ、小数点は1つだけ受ける（2つ目は無視する）。`text` は32文字まで。
- 移動の量は表示の単位で打つ。`Units::from_display` でシーンの値に直してから `key_delta` へ渡す。状態欄には `Units::format` と同じ文字で出す（表示が「ファイルと同じ」なら、単位の文字は今までどおり付かない）。
- 矢印の1歩（移動）は `grid_spacing(camera.distance)`（状態欄と描画が使っている関数）。`Step` は「今の量 ± 1歩」を `format_amount` で `text` に書き戻す（表示の単位で足す。格子が 0.1 m で表示が mm なら 100）。
- 拡大の量は倍率で、空のときの `Step` は 1 から始める。確定のときに 0 以下なら断る（今の「大きさが0になるため」の確かめも残る）。
- 確定は `CommitTransform`。量が空・0（拡大は 1）なら D は単位行列で、今の実装どおり何も積まずに終わる。

## キーの振り分け（`app::shortcut_commands`。順は design-keymap.md の「キーの照合」に入れた）
1. 数値の入力の途中（`session.transform` が `Keys`）: Esc → `CancelTransform`（今の `escape_command` の先頭がそのまま働く）。Enter → `CommitTransform`。数字・`Minus`・`Period`・Backspace → `KeyTransformInput`。矢印 → `Step`。X・Y・Z → `BeginKeyTransform`。**ほかのキーは捨てる**（表も、Delete も引かない）。
2. 途中でなく、道具が移動・回転・拡大で選択がある: 修飾なしの X・Y・Z → `BeginKeyTransform`（表より先）。道具が移動なら、矢印 → `Nudge`（←→ は X、↓↑ は Y。Shift で `fine`）。
3. それ以外は表を引く。
- 見るのは `pressed: true, repeat: false` のキーだけ（今と同じ。押し続けでは進まない）。数字は `Key::Num0`〜`Num9`、負号は `Key::Minus`、小数点は `Key::Period`。
- 入力欄でない部品が入力の位置を持っているとき（`ctx.memory(|m| m.focused())` が `Some` で、`text_edit_focused()` が偽）は、矢印・Enter・Backspace・Delete を捨てる（要件2.5）。数値の入力を始めるときは、その入力の位置を手放す。
- 3Dビューのクリックとハンドルを掴む操作は、今も先に `cancel_transform` を呼ぶ。`ViewClick` にも同じ1行を足し、数値の入力が残らないようにする。道具を替える・選択が変わる・モデルを開く・取り消しで構造が変わる、のどれでも変形は消える（今の決まり）。

## 画面
- 状態欄: 「移動 X: 12.5 mm ｜ Enter で確定・Esc でやめる・矢印で 100 mm ずつ」。回転は「回転 Y: 15°」、拡大は「拡大 Z: ×1.5」。矢印の移動の後は「移動 X: +100 mm」。
- ハンドル: 固定した軸のハンドルを、掴んでいるときと同じ色にする（`Transform::handle()` を見る今の描き方のまま）。ハンドルのそばの変化量の文字も、今の `readout` で出る。
- 道具の使い方の一文（状態欄）に足す: 移動は「移動: 矢印をドラッグ。X・Y・Z で向きを決めて数値、矢印で少しずつ」。回転と拡大も「X・Y・Z で向きを決めて数値」を添える。
- ヘルプの表に3行: 「X・Y・Z（移動・回転・拡大の道具で）｜向きを固定して数値を打つ」「矢印（移動の道具で）｜格子1つぶん動かす。Shift で10分の1」「Enter・Esc（数値の途中）｜確定・やめる」。固定のキーなので、文は手で書く。
- 設定の画面の「変えられないキー」に同じ3つを足し、修飾なしの X・Y・Z を持つ行に注意の1文を出す（`KeyRow` に `note: Option<&'static str>` を足す）。

## 既存の振る舞いが変わる所
- 移動・回転・拡大の道具で部品を選んでいる間、X（吸着）と Z（面と線）が働かない。Shift+Z（透かし）と Cmd+Z は修飾が違うので今までどおり。
- 矢印が、移動の道具で部品を動かす（今までは何もしなかった）。
- 既存の試験と自動操作のうち、この条件で X・Z を押しているものは期待が変わる。弱めずに、「道具が選択のとき（今までどおり）」と「移動のとき（向きの固定）」の両方を持たせる。該当があれば報告する（**要確認**。`app/tests/snap.rs`・`view.rs`、`e2e/inspect-1.json` が X・Z を押している。道具と選択の状態は読んで確かめる）。

## 要確認（使う前に、依存の実物か小さな試験で確かめる）
- `egui::Key::Minus`・`Key::Period` が、日本語配列と英語配列で同じ文字キーから届くか。テンキーの数字と小数点が同じキーとして届くか（届かなければ「テンキーは使えない」を手順書に書く）。
- 日本語入力が入のとき、数字キーがキーの出来事として届くか（要件8.2 と同じ未確認）。
- ボタンを押した直後に、そのボタンが入力の位置を持ち続けるか（持つなら、矢印が働かない場面が出る。押した後に手放す処理を入れるかを決める）。
- 自動操作の `key` が、矢印・Minus・Period・Enter の名前を受けるか（`egui::Key::from_name` を通るか）。
