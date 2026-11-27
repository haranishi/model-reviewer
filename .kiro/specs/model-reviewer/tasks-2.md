# Implementation Plan（マイルストーン4〜6）

[tasks.md](tasks.md) の続き。共通の合格条件と進め方は tasks.md の冒頭にある。

- [x] 4. マイルストーン4：選択・注釈・視点
- [x] 4.1 (P) WP-C3 選択の強調（色の変化と輪郭線）（design-render.md）
  - 合格: 実機の画像で、選択した部品に輪郭線が出て、他の部品に隠れていても見える（進行役）。選択を変えてもバッファを書き換えない
  - _Boundary: crates/render/src/outline.rs、shaders/{mask,outline}.wgsl_
  - _Depends: 2.2_
  - _Requirements: 5.7_
- [x] 4.2 (P) WP-A3 レビュー状態・レビューJSON・識別（design-review.md）
  - 合格: 往復で情報が保たれる。別モデル・存在しないノード・新しい版・違う形式・非数を拒む
  - _Boundary: crates/core/src/{review,review_file,identity,error}.rs、crates/core/tests/review_roundtrip.rs_
  - _Depends: 3.1_
  - _Requirements: 6.2, 7.1, 7.3, 8.2, 8.3, 8.4, 8.7, 12.2_
- [x] 4.3 (P) WP-D3a 3Dでの選択と、注釈の印・下書き（design-review.md、ux.md）
  - クリックで選択、一覧との連動、注釈追加モード、外れたクリックの知らせ、印の投影と描かない条件、索引の準備中の扱い
  - 合格: 画面なしのテストで、クリックで選択、非表示の部品は選ばれない、外れたクリックで注釈ができない、背面と非表示の印は描かれない
  - _Boundary: crates/app/src/session/pick.rs、ui/overlay.rs、crates/app/tests/select.rs_
  - _Depends: 3.2, 3.3, 4.2_
  - _Requirements: 5.5, 5.6, 6.1, 6.3, 6.6, 6.7, 6.8_
- [x] 4.4 (P) WP-D3b 注釈と視点のパネル
  - 合格: 画面なしのテストで、注釈の編集と削除、部品名と「非表示の部品」の表示、視点の保存・適用（カメラが一致）・削除
  - _Boundary: crates/app/src/session/notes.rs、ui/{annotations,views}.rs、crates/app/tests/notes.rs_
  - _Depends: 4.2_
  - _Requirements: 6.4, 6.5, 6.7, 7.1, 7.2, 7.3_

- [x] 5. マイルストーン5：レビューの保存と復元
- [x] 5.1 WP-D4 保存・読み込み・指定し直し・未保存の確認（design-review.md）
  - 一時ファイル経由の書き出し、相対パスと絶対パス、探す順番、不一致と不在のダイアログ、未保存の印と保留中の操作、終了の確認
  - 合格: 画面なしのテストで、保存→作り直した `App` で復元→状態が等しい。別モデルでは復元されない。指定し直しで復元される。未保存で開くと確認が出る。GLBのバイト列が変わっていない
  - _Boundary: crates/app/src/session/persist.rs、crates/app/tests/persist.rs_
  - _Depends: 4.3, 4.4_
  - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 8.7, 8.8_

- [x] 6. マイルストーン6：性能値・エラー処理・検証・README
- [x] 6.1 (P) WP-D5a 性能値と詳細パネル（design-app.md の表）
  - 合格: 値が無いものは「未計測」。各値に説明が付く。`--log-metrics` で同じ値が出る
  - _Boundary: crates/app/src/metrics.rs、ui/details.rs_
  - _Requirements: 10.1, 10.2, 10.3, 10.4, 10.5, 10.6, 10.7, 11.4_
- [x] 6.2 (P) WP-B3 負荷用の生成と、壊れた入力の総当たり
  - `stress` の生成、500か所の書き換えと切り詰めのテスト、アプリに壊れたGLBを開かせるテスト
  - _Boundary: crates/testdata/src/stress.rs、crates/gltf/tests/malformed.rs、crates/app/tests/malformed.rs_
  - _Requirements: 1.5, 12.2_
- [x] 6.3 WP-E1 通しの検証（design-testing.md）
  - 画面なしの通しテスト、GPUが要るテスト（`ignore`）、自動操作の手順 `flow-1.json`・`flow-2.json`
  - 合格: 通しの流れが GPUなしで通る。実機で2つの手順が終了コード0（進行役）
  - _Boundary: crates/app/tests/{flow,gpu_flow}.rs、crates/render/tests/gpu.rs、crates/app/e2e/flow-*.json_
  - _Depends: 5.1, 4.1, 2.3_
  - _Requirements: 12.2, 12.3, 12.4, 12.5_
- [x] 6.4 (P) WP-E2 README・CONTRIBUTING と、未対応の一覧の照合
  - 対応範囲・制限・保存形式・操作・検証方法。宣伝の言葉を入れない。絶対パスがJSONに入る注意を書く
  - _Boundary: README.md、CONTRIBUTING.md、docs/_
  - _Requirements: 1.6, 10.8, 11.5, 13.1, 13.3_
- [x] 6.5 進行役による実機の確認と計測（実装者には渡さない）
  - release で全テスト（`--include-ignored`）と2つの手順を実行し、画像を目で確かめる。サンプルと `stress` の性能値を、測定条件とともに記録する。未確認の項目を分けて報告する
  - _Depends: 6.1, 6.2, 6.3, 6.4_
  - _Requirements: 10.8, 12.3, 12.4, 12.5_
