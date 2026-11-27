# アプリ層：状態・コマンド・再描画・フォント・性能値・自動操作

## 状態とコマンド
```rust
pub enum Command {
    OpenModelDialog, OpenModel(PathBuf), DroppedFiles(Vec<PathBuf>),
    SaveReviewDialog, SaveReview(PathBuf), LoadReviewDialog, LoadReview(PathBuf), RelinkModelDialog, RelinkModel(PathBuf),
    Orbit(Vec2), Pan(Vec2), Zoom { factor: f32, cursor_ndc: Option<Vec2> }, FitAll, FitSelection, Home,
    ViewClick { ndc: Vec2 },                       // 通常は選択、注釈追加モードでは下書きの作成
    Select(Option<NodeId>), SetVisible { node: NodeId, visible: bool }, HideSelected, ShowAll,
    SetAnnotateMode(bool), CommitDraft { text: String }, CancelDraft,
    EditAnnotation { id: u64, text: String }, DeleteAnnotation(u64), SelectAnnotation(Option<u64>),
    SaveView { name: String }, ApplyView(u64), DeleteView(u64),
    SetPanel { panel: PanelKind, open: bool }, DismissDialog, CancelRestore, ConfirmDiscard, CancelPending,
    SetDraftPending(bool), SetBodyPending(bool),
}
pub enum PanelKind { Parts, Notes, Details }   // 左の部品、右の注釈と視点、下部の詳細
pub struct SceneDirty { pub camera: bool, pub instances: bool, pub selection: bool, pub model: bool, pub size: bool }
impl SceneDirty { pub fn any(self) -> bool; }   // どれか一つでも立っていれば真
pub struct Session { /* モデル・ModelRef・ReviewState・OrbitCamera・選択・モード・下書き・PickIndex・読み込み状態・ダイアログ・未保存の印・描き直しの印・計測値 */ }
impl Session {
    pub fn apply(&mut self, cmd: Command);         // 状態を書き換える唯一の入口
    pub fn poll_loader(&mut self) -> bool;         // 読み込みスレッドからの知らせを取り込む。変化があれば true
    pub fn set_viewport(&mut self, size_px: [u32; 2]);
    pub fn take_scene_dirty(&mut self) -> SceneDirty;   // camera / instances / selection / model / size
}
// 画面の部品はすべて同じ形にする（並行して作れる）
pub fn show(ui: &mut egui::Ui, session: &Session, out: &mut Vec<Command>) -> [u32; 2]; // 中央ビューのピクセルサイズ。App が set_viewport へ渡す
```
- 画面の部品は `&Session` を読み、`Command` を積むだけ。1回の `ui` の最後に、積まれたコマンドを順に `apply` する。部品どうしは互いを知らない。
- `…Dialog` のコマンドだけは `App` が受け、`rfd::FileDialog` を開いて、選ばれたパスを持つコマンドへ変えて積み直す。ダイアログを通らないパスつきのコマンドがあるので、テストと自動操作はOSのダイアログに触れずに済む。
- 未保存の変更（下書き欄に未登録の文字がある状態と、注釈本文の未確定な編集を含む）があるときに失う操作（`OpenModel`・`LoadReview`・終了）が来たら、`Session` はその操作を保留して確認のダイアログを出す。`ConfirmDiscard` で続行、`CancelPending` で取り消す。終了は `close_requested` を見て `ViewportCommand::CancelClose` で止める。入力途中の有無は UI から変化があったときだけ `SetDraftPending`・`SetBodyPending` で伝え、同じ値の再送で再描画を増やさない。
- 未保存の印を消すのは、置き換えが成功した時点（新しいモデルやレビューが反映されたとき）である。続行した操作が失敗したら、前のレビューと入力途中は未保存のまま残る。続行中の確認スキップ（`discard_bypass`）は、完了・失敗・中止・GPUエラーのどの終わり方でも必ず戻す。
- 表示中のモデルを置き換える読み込み（読み取り・解析・転送）の間は、表示の切り替え・注釈・視点の追加/編集/削除を受け付けず、状態欄に理由を出して編集系のボタンを無効にする（カメラ操作は可）。失敗や中止で読み込みが終われば再び編集できる。
- Esc は `app.rs` の1か所で振り分ける。ダイアログ（確認・エラー・操作説明）表示中は `A・F・H・N・Home` と選択解除を働かせず、Esc はダイアログの「閉じる・やめる・キャンセル」に当たる場合だけ効かせる。入力欄で文字を打っている間の Esc は下書きの取り消しか入力欄を離れるだけにし、通常時は下書きの取り消し→注釈追加モードの終了→選択の解除の順で1回に1つだけ効かせる。
- 入力途中の文字（注釈の本文、視点の名前、編集中の本文）は egui の一時メモリ（`ui.data_mut`）に置き、コマンドで確定する。編集中の注釈本文の一時メモリIDにはモデル・レビュー切り替えの世代番号を含め、切り替え時に消去する。下書きの位置は `Session` が持つ。
- eframe 0.37 の `App` は `logic(ctx, frame)` と `ui(ui, frame)` に分かれている（`update` は無い）。`logic` で `poll_loader` とGPU転送の続きを行い、`ui` で画面を作って3Dを描く。パネルは `egui::Panel::left(..).show(ui, ..)`、折りたたみは `show_collapsible(ui, &mut 開閉, ..)` で置く（`show_inside` は非推奨）。

## 再描画の方針（要件9.5・9.6）
- eframe は入力があったときだけ描き直す。**無条件の `request_repaint` はどこにも書かない。** 呼ぶのは次のときだけ: 読み込みスレッドが知らせを送ったとき（スレッド側から）、GPU転送が残っているとき、自動操作の手順が残っているとき。
- 3Dの画像は `SceneDirty` のどれかが立ったフレームだけ描き直す。それ以外のフレームは、前の画像を貼るだけにする。
- カメラの移動に補間（アニメーション）は付けない。視点の適用も全体表示も1フレームで切り替える。連続再描画を持ち込まず、テストの結果も決まる。動きの演出は後続の範囲とする。
- 確認の方法: 何もしないフレームで `take_scene_dirty` が空であること（GPUなし）と、`RenderStats` の描画回数が増えないこと（GPUあり）をテストで確かめる。

## 日本語フォント
- egui の同梱フォントに日本語は無い。eframe 0.37 の `system_fonts` 機能は `egui_system_font_provider::SystemFontProvider` を登録し、足りない文字だけOSのフォントから読む（フォントファイルはメモリマップで、複製しない）。
- ソースを読んだ結果: 漢字のフォントは `locale` で決まり、既定は**OSの言語設定**である。日本語以外の設定のMacでは、漢字が中国語の字形のフォントになりうる。そこで eframe の自動登録を止め（`NativeOptions::system_font_fallback = false`、eframe の `system_fonts` 機能も切る）、`SystemFontProvider::new().with_locale(Some("ja"))` を自分で `ctx.add_font_provider` に渡す。
- **残る不確かさ**: 句読点・かっこ・記号は文字体系が「共通」なので、`locale` を見ない別の経路（汎用のフォント族を順に試す）で選ばれる。漢字・かなとは別のフォントになる可能性がある。実機の画像で「、。「」（）・ー」の字形と位置を確かめる作業を、最初の画面の作業パッケージの合格条件に入れる。おかしければ、日本語フォントを名前で探して最優先に登録する処理を `fonts.rs` に足す（フォントの同梱はしない）。
- フォントを同梱しないので、配布物は小さく、フォントのライセンス表記も要らない。日本語フォントの無い環境では文字が出ないので、その場合は起動時にログへ警告を出す。
- 設定は `fonts::install(ctx)` の1か所に置き、アプリとテストの両方から呼ぶ。

## 性能値の定義（要件10）
詳細パネルと、起動引数 `--log-metrics` を付けたときの標準エラー出力に、同じ値を出す。各値の横に、下の「測ったもの」を短く添える。
| 表示名 | 測ったもの |
|---|---|
| ファイルサイズ | 読み取ったバイト数 |
| ノード数／メッシュ数／三角形数 | GLBのノード、メッシュ、形状として持つ三角形。別に「配置後の三角形数」（共有された形状を部品の数だけ数えた値） |
| 読み取り | `fs::read` の経過時間 |
| 識別の計算 | blake3 の経過時間 |
| 解析 | 容器とJSONの解析から `Model` の完成までの経過時間 |
| GPU転送（CPU側） | `continue_upload` の呼び出しにかかった時間の合計と、かかったフレーム数。**GPUの処理時間ではない** |
| 初回表示まで | 開く操作を受けた時刻から、そのモデルを最初に描く命令を送り終えた時刻まで。画面に出た瞬間（提示）そのものは測れないので、そう明記する |
| 当たり判定の準備 | `PickIndex::build` の経過時間（表示とは並行） |
| フレームのCPU時間 | `App::ui` 1回の経過時間（UIの構築と3Dの描画命令の作成・送信。GPUが追いつかないときは前の描画を待つ時間を含む）。直近120回の平均と最大 |
| フレーム間隔 | 連続して描いているときの、`ui` の開始から次の開始まで。直近120回の平均と最大。100ms以上空いたものは「連続」とみなさず除く。画面に出ている間は更新周期で頭打ちになる値であることを添える |
| GPUの処理時間 | **未計測**と表示する（初版では計らない） |
| メモリ | `memory_stats::memory_stats()` の `physical_mem`（プロセスの常駐メモリ）。取得できないOSでは行を出さない |
時間は `std::time::Instant` で測る。値が無いものは `Option` の `None` のまま持ち、0で埋めない。

## 自動操作（人のマウスを動かさない検証）
- `model-reviewer --script <手順.json> --out <出力フォルダ>` で、本物のウィンドウと本物のGPUのまま、手順を自動で実行する。
- 入力は `App::raw_input_hook` で `egui::RawInput` に `egui::Event` を足して注入する。OSの入力は使わないので、本物のカーソルは動かない。3Dビューの操作もボタンの押下も、人の入力と同じ egui の経路を通る。実行中は本物のポインタとキーの入力を捨て、人がウィンドウに触れても結果が変わらないようにする。ウィンドウは `with_active(false)` で開き、手前に出て入力を奪うことを避ける。
- 手順の種類: `open_model`・`load_review`・`save_review`（ダイアログの代わりにパスを渡す）、`wait_idle`、`drag_view`・`scroll_view`・`click_view`（ビュー内の0〜1の座標）、`click_widget`（名前で指定）、`type_text`、`key`、`screenshot`、`dump_state`、`assert_state_equals`、`quit`。
- `click_widget` のために、押せる部品は描くたびに名前と矩形を `WidgetRegistry` へ登録する（`ui::reg("toolbar.open", &response)`）。名前は `ui/ids.rs` に定数で置く。出たばかりの部品は位置が定まらないので、2回続けて同じ矩形になってから押す。
- `type_text` は、入力欄に入力位置があることを2回続けて確かめてから打つ。egui の `Response::has_focus` はウィンドウが前面にあることも条件にするので、アプリ側のキー判定は入力位置（メモリ）で行う。
- 画像は `ctx.request_screenshot(..)`（中身は `ViewportCommand::Screenshot`）で受け取り、PNGで保存する。状態は `dump_state` でJSONに書き出し（選択・非表示・注釈・視点・カメラ）、再起動後の実行で `assert_state_equals` により比べる。
- ウィンドウが最小化されたり完全に隠れたりすると、eframe は `ui` を呼ばず `logic` だけを呼ぶ。手順の実行中は毎回 `ViewportCommand::RequestPaintWhileHidden` を送る（効くのは1フレーム）。`ui` が呼ばれなかった回の入力は持ち越されるので、注入は前の分を `ui` が受け取った後にだけ行う。
- 終了コードは、全手順が成功で0、失敗や時間切れ（既定30秒）で1。`report.json` に各手順の結果と性能値を残す。`quit` は未保存の確認を出さずに終了する。手順ファイルのパスは相対で書く。
- 起動引数は `[GLBまたはレビューJSON]`・`--script`・`--out`・`--log-metrics` だけなので、解析は標準ライブラリで書く。
