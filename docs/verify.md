# 検証と性能値の見方

## テスト

README の4コマンドは、GPUが無くても通る範囲です。CI（`.github/workflows/ci.yml`）は macOS・Linux・Windows で clippy とテストを、書式と依存のライセンス検査（`deny.toml`）を1回実行します。GPUがある環境では次も実行します。

```
cargo test --workspace -- --include-ignored
```

`#[ignore]` のテストは、通常の `cargo test` では ignored と数えられ、成功には数えません。内訳は次のとおりです。

- GPUの描画テスト（`crates/render/tests/gpu.rs`）: 中心が背景色でない、選択で画像が変わる、大きさ0では描かない、汚れの無いフレームでは描かない、転送に失敗しても表示中のモデルが残る。基準画像との1画素比較はしません。
- アプリの通し（`crates/app/tests/gpu_flow.rs`）と、画面の画像を書き出すテスト（`ui_shots.rs`。`UI_SHOT=<接頭辞>` で `target/ui-check/` に出力）。
- 数百万三角形の当たり判定の準備時間を測るテスト（`crates/core/tests/pick.rs`）。
- 外部で作ったGLBを読むテスト。`MR_EXTERNAL_GLB_DIR=<フォルダ> cargo test -p model-reviewer-gltf --test external -- --ignored --nocapture`。STL は `MR_EXTERNAL_STL_DIR` と `-p model-reviewer-stl`。変数が無ければ何もしません。

## 実機の自動操作

OSの入力は使わないので、本物のマウスは動きません。ウィンドウは開きますが、前面には出ません。作業場所の直下で実行します。

```
target/release/model-reviewer --script crates/app/e2e/<名前>.json --out target/e2e
```

| 手順 | 確かめること |
|---|---|
| `smoke` | 起動と画面撮影 |
| `flow-1` → `flow-2` | 開く・回転・選択・非表示・注釈・視点・保存。起動し直して同じ状態に戻る |
| `view-1` | 立方体・平行投影・4つの表示・格子・版2のレビュー保存 |
| `edit-1` → `edit-2` | ハンドルと数値の欄で動かす・取り消す・注釈・書き出し・レビュー保存。起動し直して一致 |
| `edit-3` | 図形を足す・複製・削除・取り消し・書き出し。開き直して部品の数と位置が一致 |
| `measure-1` | 2点の距離、部品を動かした後の距離、単位の切り替え、平行投影 |
| `inspect-1` | 角や辺に吸着する計測、断面（軸・位置・反転、消えた側の当たり）、表示の単位 |
| `stl-1` → `stl-2` | 見本の STL（20×30×40 mm の箱）を開く・角から角で 20 mm・注釈・円柱の寸法・GLB へ書き出し・レビュー保存。起動し直して一致 |
| `heavy-edit`・`heavy-inspect` | 下の重いモデルで性能を測る（先に生成する）。`heavy-inspect` は吸着の探索300回・断面の矢印のドラッグ・重い STL を開く |

終了コードは成功が0、失敗や時間切れが1です。`report.json` に各手順の結果と性能値が残ります。手順の `op` は `open_model`（STL の単位の仮定 `stl_unit` を省略できる）、`load_review`、`save_review`、`export_glb`、`wait_idle`、`drag_view`、`drag_widget`、`scroll_view`、`click_view`、`click_widget`、`hover_view`、`type_text`、`key`、`screenshot`、`dump_state`、`assert_state_equals`、`sample`、`quit` です。パスは相対だけです。`click_widget` と `drag_widget` は部品の位置が2回続けて同じになってから押し、`type_text` は入力欄に入力位置が来てから打ちます。`wait_idle` は読み込み・書き出しの失敗を手順の失敗にします。`sample`（`label`、待つフレーム数 `frames`）と、ドラッグの `sample_every` は、常駐メモリ・フレームのCPU時間・GPUへ送ったバイト数・描いた回数・履歴の大きさなどを `report.json` の `samples` に残します。

重いモデルは生成します（リポジトリには置きません）。`heavy-edit` と `heavy-inspect` は、下の2行目からの3つを使います。

```
cargo run --release -p model-reviewer-testdata -- stress 100000 200 target/stress.glb
cargo run --release -p model-reviewer-testdata -- stress-nested 100000 200 target/stress-nested.glb
cargo run --release -p model-reviewer-testdata -- stl-stress 5000000 binary target/stress.stl
cargo run --release -p model-reviewer-testdata -- stl-stress 1000000 ascii target/stress-ascii.stl
```

## 性能値の定義

詳細パネルと `--log-metrics` は同じ値を出します。測っていない値は「未計測」で、0で埋めません。CPU側の時間を GPU の処理時間とは書きません。フレームの値は直近120回なので、操作の途中で測るときは `sample_every` か、操作の直後の `sample` を使います。

| 名前 | 何を測ったか |
|---|---|
| ファイルサイズ | 読み取ったバイト数 |
| ノード数／メッシュ数／三角形数 | ノード、メッシュ、形状として持つ三角形。配置後は共有形状を部品の数だけ数える |
| 読み取り | `fs::read` の経過時間 |
| 識別の計算 | blake3 の経過時間 |
| 解析 | 容器と JSON の解析から Model 完成までの経過時間 |
| GPU転送（CPU側） | `continue_upload` の合計と、呼んだ回数。GPUの処理時間ではない |
| 初回表示まで | 開く操作から、最初の描画命令を送り終えるまで。画面に出た瞬間は含まない |
| 当たり判定の準備 | `PickIndex::build` の経過時間。表示とは並行 |
| フレームのCPU時間 | `App::ui` 1回の経過。直近120回の平均と最大。GPUが追いつかないときは前の描画を待つ時間を含む。GPUの処理時間そのものではない |
| フレーム間隔 | ui の開始から次の開始まで。直近120回の平均と最大。100ms 以上の空きは除く。画面に出ている間は更新周期で頭打ちになる |
| GPUの処理時間 | 初版では計らない。常に「未計測」 |
| メモリ | `memory_stats` の `physical_mem` が取れたときだけ。プロセスの常駐メモリ |
| 編集の確定 | 直近の変形・数値の確定・追加・複製・削除で、文書と履歴を書き換えた時間 |
| 取り消し・やり直し | 直近の取り消し・やり直しで、文書とレビューを書き換えた時間 |
| 履歴 | 履歴が持つ操作の大きさの見積もり（バイト）と、取り消せる回数 |
| 書き出し | 読み取り・組み立て・書き込み・確かめの各時間と、書き出し中の常駐メモリの最大 |

## 測定値

条件: MacBook Pro（Apple M4 Pro・GPU 20コア・メモリ48GB）、macOS 27.2.1、リリースビルド、ウィンドウは 2560×1600 ピクセルで前面に出さない自動操作、電源接続、ほかのアプリも起動中。他のソフトとの比較はしていません。

読み込み（第1段階で測定。各3回の中央値、ファイルはOSのキャッシュにある状態）:

| | 見本 | 重いモデル | 部品10万個 |
|---|---|---|---|
| ファイル／部品／三角形（配置後） | 11.7 KB／12／524 | 156 MB／3,433／約565万 | 6.9 MB／100,000／2,000万 |
| 読み取り／識別／解析 | 0.1／0.0／0.1 ms | 15／80／89 ms | 0.8／3.3／65 ms |
| 初回表示まで | 20 ms | 219 ms | 74 ms |
| 回転中の1コマ（平均／最大） | 0.1／0.6 ms | 4.8／6.0 ms | 11.1／11.8 ms |
| メモリ（常駐） | 155 MB | 707 MB | 331 MB |

重いモデルは Blender で書き出した確認用で、リポジトリには含みません。編集の値（`heavy-edit`、部品10万個・子孫5万個のまとめ役を動かす、1回の実行）:

| | 値 |
|---|---|
| ドラッグ中の1コマ（平均）／同じモデルで回転中 | 16.1 ms／11.3 ms |
| ドラッグ中にGPUへ送った形状とインスタンス | 0 バイト（定数だけ1コマ304バイト） |
| 変形の確定（子孫5万個） | 1.1 ms |
| 図形の追加／5万個の削除／5万個の複製 | 1.6／3.3／10.8 ms |
| 取り消し（変形／追加／削除／複製） | 1.2／1.2／6.1／4.4 ms |
| 履歴（変形1回／変形100回／5万個の削除の後） | 224 バイト／22,400 バイト／約12 MB |
| 書き出し（読み取り／組み立て／書き込み／確かめ） | 3.4／143／5.4／324 ms |
| 書き出し中の常駐メモリの最大 | 840 MB（書き出す前は421 MB） |

ドラッグ中の1コマは、前の描画を待つ時間を含み、画面の更新周期（約16.7 ms）で頭打ちになります。記録の詳細と、届かなかった目標は `.kiro/specs/editor-foundation/verification.md` にあります。
