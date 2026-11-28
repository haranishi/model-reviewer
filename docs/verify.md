# 検証と性能値の見方

## テスト

[ビルド手順](build.md)の4コマンドはGPUが無くても実行できます。通常の CI（`.github/workflows/ci.yml`）は macOS・Linux・Windows で clippy とテストを、書式と依存のライセンス検査（`deny.toml`）を1回実行します。配布用 CI は展開後の実行ファイルで `smoke` を試します。次の試験は通常 ignored と数えます。GPUがある環境では `cargo test --workspace -- --include-ignored` で実行します。

- GPUの描画テスト（`crates/render/tests/gpu.rs`）: 中心が背景色でない、選択で画像が変わる、大きさ0では描かない、汚れの無いフレームでは描かない、転送に失敗しても表示中のモデルが残る。基準画像との1画素比較はしません。
- `crates/render/tests/texture.rs` は、基本色との掛け合わせ、MASK の抜け方、箱形の投影の向きなどを描画した画素で確かめます。
- アプリの通し（`crates/app/tests/gpu_flow.rs`）と、画面の画像を書き出すテスト（`ui_shots.rs`。`UI_SHOT=<接頭辞>` で `target/ui-check/` に出力）。
- 数百万三角形の当たり判定の準備時間を測るテスト（`crates/core/tests/pick.rs`）。
- 外部で作ったGLBを読むテスト。`MR_EXTERNAL_GLB_DIR=<フォルダ> cargo test -p model-reviewer-gltf --test external -- --ignored --nocapture`。STL は `MR_EXTERNAL_STL_DIR` と `-p model-reviewer-stl`。変数が無ければ何もしません。

## 実機の自動操作

作業場所の直下で実行します。本物のマウスは動きません。macOS では、画面を透明にしてアプリを前面にせず、Dock にも出しません。`--script` では既定のキーを使い、割り当ての変更はその起動中だけ有効です。`--keymap <ファイル>` を渡すと、そのファイルから読み込み、変更を保存します。

実行例は `target/release/model-reviewer --script crates/app/e2e/<名前>.json --out target/e2e` です。`open_model`、`load_review`、`save_review`、`export_glb`、`paste_texture` は相対パスを受け取ります。ファイル選択画面を呼ぶ手順は、その場で理由を記録して終了します。

| 手順 | 確かめること |
|---|---|
| `smoke` | 起動と画面撮影 |
| `flow-1` → `flow-2` | 開く・回転・選択・非表示・注釈・視点・保存。起動し直して同じ状態に戻る |
| `view-1` | 立方体・平行投影・4つの表示・格子・版2のレビュー保存 |
| `edit-1` → `edit-2` | ハンドルと数値の欄で動かす・取り消す・注釈・書き出し・レビュー保存。起動し直して一致 |
| `edit-3` | 図形を足す・複製・削除・取り消し・書き出し。開き直して部品の数と位置が一致 |
| `measure-1` | 2点の距離、部品を動かした後の距離、単位の切り替え、平行投影 |
| `inspect-1` | 角や辺に吸着する計測、断面（軸・位置・反転、消えた側の当たり）、表示の単位 |
| `stl-1` → `stl-2`・`stl-3` | 同じ `--out` で各手順を別々に起動する。1 は Y が上の STL のレビュー（`model.stl_up` は `"y"`）と状態を保存する。角から角の 20 mm・注釈・円柱の寸法を確かめ、書き出した GLB のレビューと状態も別名で保存する。2 は GLB のレビューを復元して照合する。3 は見本を既定の Z で開き、STL のレビューを復元して Y で保存した状態と照合する |
| `new-1` | 何も開かずに［新規作成］・原点に大きさ 1 の立方体・レビュー保存が断られる・GLB へ書き出すと作業の対象が移る・Cmd+N で空に戻る |
| `color-1` | 見本の車輪を赤にし、取り消しとやり直しの状態を照合。16進で青にし、複製だけ黄に変えて移動。書き出した GLB を開き直し、元と複製の色をそれぞれ照合する。状態の `selection_color` に sRGB の16進と最大8件のリニア RGBA が入る |
| `desk-1` | 新規作成から寸法を cm で入力し、複製と軸キーで机1台（5部品）と椅子4脚（各6部品）を組む。`desk-1-assembled.json` の `nodes` は29件。`report.json` の `samples` で新規作成直後・組み立て後・書き出し後の性能値を比べる |
| `keys-1` → `keys-2` | 移動を T に変更し、回転との重複を取り消す。再起動して割り当てを比較し、初期化後に G で移動する。両方とも同じ新規の検証用フォルダを指定する（例 `--out target/e2e-keys --keymap target/e2e-keys/keymap.json`） |
| `keymove-1` | X へ 12.5 動かして数値欄で指定した位置と比較し、矢印3回を取り消す。Y に90度回し、Z の拡大を Esc でやめる |
| `tex-1` | 縞模様の GLB を開き、形状の初回描画と画像の転送完了の順を確認。「画像」の入・切を `tex-1-shown.png` と `tex-1-hidden.png` に撮り、表示を戻して状態を照合 |
| `tex-2` | 新規の立方体に貼り、1枚の大きさを 1 m から 0.25 m に変更。取り消し・やり直し・複製・書き出し後、元と複製を開き直して画像と基本色を照合 |
| `tex-3` | STL の箱に貼って GLB に書き出し、開き直した部品の見た目と、操作前後の STL・PNG の SHA-256 を照合 |
| `heavy-edit`・`heavy-inspect` | 下の重いモデルで性能を測る（先に生成する）。`heavy-inspect` は吸着の探索300回・断面の矢印のドラッグ・重い STL を開く |

手順の `op` は `open_model`、`load_review`、`save_review`、`export_glb`、`paste_texture`、`wait_idle`、`drag_view`、`drag_widget`、`scroll_view`、`click_view`、`click_widget`、`hover_view`、`type_text`、`key`、`screenshot`、`dump_state`、`assert_state_equals`、`sample`、`quit` です。

`open_model` で STL を開くときの既定は `stl_unit: "mm"`、`stl_up: "z"` です。`dump_state` の `stl_up` は `"z"`・`"y"`、STL 以外は `null` です。`click_widget` と `drag_widget` は部品の位置が2回続けて同じになってから押し、`type_text` は入力欄に入力位置が来てから打ちます。

終了コードは成功が 0、失敗・時間切れが 1 です。`report.json` に各手順の結果と性能値が残ります。`sample`（`label` と待つフレーム数 `frames`）やドラッグの `sample_every` は、常駐メモリ・フレームの CPU 時間・転送バイト数・描画回数・履歴の大きさなどを `samples` に記録します。

### テクスチャの試験素材と照合

作業場所の直下で次の3つを生成します。`textured` の数は画像の枚数・正方形の辺の画素数・部品数です。`pattern` は向きと繰り返しを見分けるための 128 × 128 画素の4色の PNG を作ります。

```sh
cargo run --release --locked -p model-reviewer-testdata -- textured 4 256 4 target/tex/scene.glb
cargo run --release --locked -p model-reviewer-testdata -- pattern target/tex/quadrants.png
cargo run --release --locked -p model-reviewer-testdata -- stl-sample target/tex/box.stl
```

`cargo build --release --locked` の後、GPU と画面のある環境で `python3 crates/app/e2e/check-textures.py` を実行します。3つの JSON を個別に起動し、`target/e2e/tex-1` ～ `tex-3` に結果を残します。箱形を UV に変換した前後では画像のハッシュと基本色を比べます。STL と PNG は手順3の直前と直後にファイル全体を照合します。

- `{"op":"paste_texture","path":"../../../target/tex/quadrants.png"}` は選んだ部品とその子に貼ります。貼る画像のパスは手順 JSON のフォルダから指定します。`"relative_to":"out"` を付けると `--out` が基準になります。直後の `wait_idle` は貼り付けと画像転送の完了まで待ちます。モデルの読み込み・貼り付け・書き出しが失敗すると手順も失敗します。
- `dump_state` の `textures` は `total`（省いた参照も含む枚数）、`displayed`（転送済み）、`shrunk`（縮小）、`failed`（省略・失敗）、`bytes`（縮小版を含む転送バイト数）、`shown`（表示の入・切）です。表示を切っても転送済みの枚数とバイト数は保たれます。
- `selection_texture` は未選択なら `null` です。`total` は選択と子孫の描画数、`draws` は先頭8件まで。各描画には画像の BLAKE3 の先頭16桁 `hash`、貼り方 `map`（`uv`／`box`）、`size`（箱形の画像の長い辺を部品内の座標で表した長さ、UV は `null`）が入り、画像のない描画は `null` です。

### 重いモデルの素材

重いモデルは生成します（リポジトリには置きません）。`heavy-edit` と `heavy-inspect` は、下の2行目からの3つを使います。

```
cargo run --release -p model-reviewer-testdata -- stress 100000 200 target/stress.glb
cargo run --release -p model-reviewer-testdata -- stress-nested 100000 200 target/stress-nested.glb
cargo run --release -p model-reviewer-testdata -- stl-stress 5000000 binary target/stress.stl
cargo run --release -p model-reviewer-testdata -- stl-stress 1000000 ascii target/stress-ascii.stl
```

## 手順書の画像

各手順（`guide-*.json`）を `--out target/guide` で実行します。`guide-09-shortcuts.json` だけは `--keymap target/guide/keymap.json`（まだ無いファイル）を付けます。省略すると設定画面に「自動操作のため、変更は保存されません」と出ます。出力画像を開いて確認し、256色に落として（`ffmpeg` の `palettegen`・`paletteuse`）`docs/guide/images/` に置きます。第11章までの画像は 2026-11-27 に macOS 27.2.1（Apple Silicon）、ウィンドウ 1280×800（倍率 1）で撮りました。

第12章は上の素材を生成してから `target/release/model-reviewer --script crates/app/e2e/guide-12-texture.json --out target/guide` で撮ります。`texture-view.png` は GLB の縞模様と「モデルの UV」、`texture-paste.png` は STL に貼った4色の模様と「1枚の大きさ」10 mm を確認します。この2枚は 2026-11-28 に macOS 27.2.1（Apple Silicon）、ウィンドウ 1280×800（倍率 2）で撮り、2560×1600 を 1280×800 に縮めてから256色に落としました。

## 性能値の定義

詳細パネルと `--log-metrics` は同じ値を出します。測っていない値には「未計測」と出ます。フレームの CPU 時間は直近120回の値です。操作中の値は `sample_every` か操作直後の `sample` で調べます。[読み込み・編集・画像の測定値と測定環境](measurements.md)

| 名前 | 何を測ったか |
|---|---|
| ファイルサイズ | 読み取ったバイト数 |
| ノード数／メッシュ数／三角形数 | ノード、メッシュ、形状として持つ三角形。配置後は共有形状を部品の数だけ数える |
| 読み取り | `fs::read` の経過時間 |
| 識別の計算 | blake3 の経過時間 |
| 解析 | 容器と JSON の解析から Model 完成までの経過時間 |
| GPU転送（CPU側） | `continue_upload` の合計と、呼んだ回数。GPUの処理時間ではない |
| 初回表示まで | 開く操作から、最初の描画命令を送り終えるまで。画面に出た瞬間は含まない |
| テクスチャ表示まで／読み込み中の最大の常駐メモリ | 開く操作から最後の画像を送り終えるまでの時間／画像読み込み中に観測した常駐メモリの最大値 |
| 当たり判定の準備 | `PickIndex::build` の経過時間。表示とは並行 |
| フレームのCPU時間 | `App::ui` 1回の経過。直近120回の平均と最大。GPUが追いつかないときは前の描画を待つ時間を含む。GPUの処理時間そのものではない |
| フレーム間隔 | ui の開始から次の開始まで。直近120回の平均と最大。100ms 以上の空きは除く。画面に出ている間は更新周期で頭打ちになる |
| GPUの処理時間 | 初版では計らない。常に「未計測」 |
| 常駐メモリ | `memory_stats` の `physical_mem` が取れたときだけ。プロセスの常駐メモリで、GPU に置いた画像などは含まないことがある（[画像の測定](measurements.md)） |
| 編集の確定 | 直近の変形・数値の確定・色の変更・追加・複製・削除で、文書と履歴を書き換えた時間 |
| 取り消し・やり直し | 直近の取り消し・やり直しで、文書とレビューを書き換えた時間 |
| 履歴 | 履歴が持つ操作の大きさの見積もり（バイト）と、取り消せる回数 |
| 書き出し | 読み取り・組み立て・書き込み・確かめの各時間と、書き出し中の常駐メモリの最大 |
