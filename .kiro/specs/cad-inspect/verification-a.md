# 検証の記録（区切りA: 吸着する計測・断面・表示の単位。2026-11-27、macOS・Apple M4 Pro）

区切りAの時点の記録。下の「残り」の後の状態は [verification.md](verification.md)。

## 検査（取り込んだ状態）
| 検査 | 結果 |
|---|---|
| `cargo fmt --all --check` | 成功 |
| `cargo clippy --all-targets --workspace -- -D warnings` | 警告0 |
| `cargo test --workspace` | 422件成功、失敗0、ignored 51件 |
| `cargo test --workspace -- --include-ignored` | 473件成功、失敗0 |
| `cargo build --release` | 成功 |
| `cargo deny check licenses sources bans` | 成功 |

## 実機の自動操作（OSの入力は使わない。ウィンドウは前面に出さない）
- `smoke`・`flow-1`・`flow-2`・`view-1`・`edit-1`・`edit-2`・`edit-3`・`measure-1`・`inspect-1`・`heavy-edit` の10本がすべて成功（終了コード0、`report.json` の `ok`）。見本GLBのハッシュは実行の前後で同じ（`44ce48ac…`）。
- `inspect-1` では、角から角が2点とも頂点で距離 2.33238 m、X で吸着を切ると面、軸 Y・位置 0.6 では手前の緑の円柱を押すと奥の base が選ばれ、mm 表示で「600 mm」「格子 1000 mm」になることを確かめた。画像で候補の印と言葉、断面の枠と矢印、切り口の色、状態欄の2つの単位を目で見た。
- `measure-1` は手順を変えずに通った。ただし吸着が既定で入るため、1点目が辺に吸着し、値は第2段階と違う（距離 0.7246 m、種類は辺と面）。
- 参考: 部品10万個の生成モデルでポインタを40回動かしたときの吸着の探索は直近 4.5 ms・最大 4.7 ms。設計の300回の測定ではない。

## 残り
- STL の読み込みと寸法を指定した図形（区切りB）。
- 断面の位置の範囲は、非表示の部品も含む境界箱から求めている（要件3.1・3.3 は見えている全体）。区切りBで直す。
- design-testing.md の性能の表、外部のツールで作った STL の確認は未実施。

## 未確認
実キーの X・C、実機のトラックパッド、日本語入力、Windows と Linux の画面（ビルドと自動テストは CI で確かめる）、GPUの処理時間。
