# Implementation Plan（接合と確認）

[tasks.md](tasks.md) の続き。進め方の約束は同じである。接合は、並行の部品を `Session` と画面につなぐ作業で、共有の大きなファイルに触れる。1本ずつ順に行う。

- [ ] 2. 接合
- [x] 2.1 WP-J1 単位・吸着・断面をつなぐ（区切りA。K1・R1・V1 の後）
  - **単位**: `Session::units` を、計測の線の文字・状態欄・移動のハンドルの変化量・格子の文字に通す。`SetUnit`（GLB の前提）と `SetDisplayUnit` を `session::measure` に実装する。状態欄に「表示 …」を足し、幅が足りないときの省く順を入れる（design.md の「画面」）。表示が「ファイルと同じ」の間、既存の文字と数値が変わらないことを試験で固定する
  - **吸着**: `ui::viewport` が `ViewHover` を積む条件（design-snap.md の「つなぎ方」）。`session::snap::hover` と、計測のクリックでの `snap`。8ミリ秒を超えたときの休み。`SetSnap`・`ToggleSnap` とキー X。`measure_view`・`measure_panel` の入力の組み立て。性能値「吸着の探索」を `metrics` に足す
  - **断面**: `ToggleSection`・`SetSection` とキー C。`gpu_view` が `FrameParams.section` を渡し、`scene_dirty.section` で描き直す。選択・注釈の下書き・計測のクリックを `pick_clipped` に替える。`section_bar`・`section_view` を3Dビューに置き、入力の振り分けに足す。`ui::overlay` と `measure_view` で、消えた側の印を薄くして押せなくする。モデルを開く・閉じるで断面と候補を消す
  - **自動操作**: `hover_view`、`dump_state` の `measure.kinds`・`measure.delta`・`snap`・`section`・`units`。`e2e/inspect-1.json`。ヘルプの表と「できないこと」
  - 合格: `app/tests/{snap,section,units}.rs`（試験名の目安: `measure_click_snaps_to_vertex`、`click_does_not_use_stale_preview`、`x_toggles_snap`、`slow_search_backs_off`、`c_toggles_section_and_remembers_plane`、`cut_side_is_not_selectable`、`cut_side_notes_are_dimmed_and_inert`、`display_same_as_file_changes_nothing`、`display_mm_converts_readouts_only`）。`layout.rs` に幅960の状態欄。既存の `measure`・`measure_view`・`select`・`notes` の試験が通る。X・C が入力中とダイアログ中に働かない
  - 既存の `measure-1.json` が手順を変えずに通るかを報告する（通らなければ、理由と、足した1行を報告する）
  - _Boundary: `app/src/{app,gpu_view,metrics,automation}.rs`、`app/src/automation/`、`app/src/session/{mod,snap,section,measure,pick,camera}.rs`、`app/src/ui/{mod,ids,viewport,overlay,status,help}.rs`、`app/tests/{snap,section,units,layout}.rs` と既存の試験、`app/e2e/inspect-1.json`_ ／ _Depends: 1.1, 1.4, 1.5_ ／ _Requirements: 1.1, 1.3, 1.5, 1.8, 2.3, 2.4, 2.5, 3.1, 3.2, 3.3, 3.6, 3.8, 4.1, 4.3, 4.4, 7.1, 7.2, 7.3, 7.4, 7.5, 11.2, 11.4, 12.2, 12.3, 12.5_
- [ ] 2.2 WP-J2 STL と寸法をつなぐ（区切りB。J1・U1・L1 の後）
  - **開く**: `app::loader` の振り分け（`glTF` の印 → GLB、そうでなければ `detect` か拡張子で STL、どちらでもなければ新しい知らせ）。`LoadOptions { stl_unit }`。ファイルを選ぶ画面の絞り込みに STL を足す。ドロップ。開いたら `source_kind` と `units` を決める（design-stl-units.md）。`SetStlUnit` は、未保存の確認を通って開き直す
  - **書き出し**: `ExportRequest.base_kind`。土台が STL のとき、読み直し → 照合 → `load_stl` → `synthesize_base` を「読み取り中」の工程で行う。終わったら `source_kind` を `Glb` にし、結果の一覧に1行足す。保存先の既定の名前は「<元の名前>.glb」
  - **レビュー**: 保存のとき `ModelRef.stl_unit` を入れる。復元でモデルを読むとき、`stl_unit` を `LoadOptions` へ渡す。書き出していない編集があるときの案内は、第2段階のまま
  - **寸法**: `add_shape` が `add_shape_primitive` を呼ぶ。`session::dims::selection_dims`。右のパネルに「寸法」を置く（部品 → 変形 → 寸法 → 計測）。寸法の確定が履歴の1回分になり、取り消せること
  - **自動操作と文書**: `open_model` の `stl_unit`、`dump_state` の `source_kind`・`selection.dims`。`e2e/stl-1.json`・`stl-2.json`・`heavy-inspect.json`。`docs/stl.md`（新規。対応範囲、開かないもの、単位の仮定、書き出すとどうなるか）、`docs/{glb,review-json,verify,roadmap}.md` と README の該当箇所
  - 合格: `app/tests/{stl,dims}.rs`（試験名の目安: `opens_binary_and_ascii_stl`、`neither_glb_nor_stl_message`、`broken_stl_keeps_the_open_model`、`stl_review_saves_and_restores_with_unit`、`changing_stl_unit_reopens_after_confirm`、`stl_exports_to_glb_and_moves_target`、`scene_numbers_do_not_change_across_stl_export`、`exported_glb_from_stl_reopens_with_same_extent`、`dims_edit_is_one_undo_step`、`dims_of_loaded_part_are_read_only`）。既存の `open`・`malformed`・`export`・`persist` の試験が通る（知らせの文が変わる試験だけを直す）
  - _Boundary: `app/src/{loader,exporter,app}.rs`、`app/src/automation/`、`app/src/session/{mod,open,persist,export,edit,dims}.rs`、`app/src/ui/{mod,status,dialogs,help,inspector}.rs`、`app/tests/{stl,dims}.rs` と既存の試験、`app/e2e/{stl-1,stl-2,heavy-inspect}.json`、`app/Cargo.toml`（`model-reviewer-stl` を足す）、`docs/`、README_ ／ _Depends: 2.1, 1.2, 1.3_ ／ _Requirements: 5.1, 5.2, 5.6, 6.3, 6.4, 7.6, 7.7, 7.8, 7.9, 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 8.7, 9.1, 9.3, 9.6, 10.1, 10.3, 13.4, 13.5_

- [ ] 3. 最後の確認（進行役が行う。GPU・実機・画像の目視・性能）
- [ ] 3.1 WP-F1 確認と記録
  - `cargo test --workspace -- --include-ignored`（GPUの試験）。断面の画像を開いて見る: 消す側が消えている、切り口の色が裏の面だけに出ている、選択の輪郭と線が同じ平面で切れている、格子は切れていない、断面なしの絵が第2段階と同じ
  - 実機の自動操作: 既存の9本と、`inspect-1`・`stl-1`・`stl-2`・`heavy-inspect`。見本の GLB と STL のハッシュが、実行の前後で同じ
  - 画像の目視（`ui_shots`、幅960と広い幅）: 候補の印と言葉、断面の行と枠と矢印、右のパネルの「寸法」「計測」、状態欄の2つの単位。重なりと、日本語と「、。（）・」の字形
  - 性能（design-testing.md の表）: 生成モデルと生成 STL で測り、値・機械・条件を `verification.md` に書く。届かない項目は、値と、直すか目標を見直すかを書く
  - 外部のツールで作った STL（バイナリと ASCII）での確認と、書き出した GLB を別のソフトで開いた大きさの確認（design-testing.md）。使ったソフトと版を書く
  - 公開の前の点検: `cargo deny check licenses sources bans`、Rust 1.95 の `cargo check --workspace --all-targets`、文書とコードに個人名・メール・ホームの絶対パス・道具の名前が無いこと。CI の3つのOSの結果を記録する
  - 記録に「未確認」を残す: 実キーの X・C、実機のトラックパッド、ネイティブのダイアログの絞り込み、Windows と Linux の画面、GPUの処理時間
  - `spec.json` の `phase` を `implemented` にし、`documents` に `verification.md` を足す。`docs/roadmap.md` に、この段階を「済んだ段階」として書く
  - _Requirements: 3.5, 11.5, 13.1, 13.2, 13.4, 13.5_

## 接合で気をつける所（壊れやすい順）
1. **STL から書き出した直後。** 文書の部品0・形状0が、合成した土台の番号と合っていること、`set_file_indices` と `adopt_file_locals`（第2段階の仕組み）を通った後も `local` の倍率が残ること、シーンの数値が動かないことを、`scene_numbers_do_not_change_across_stl_export` で固定する。
2. **計測のクリックの結果が変わる。** 既定で吸着が入るので、既存の計測の試験と `measure-1` の期待値が変わりうる。変わった試験は、弱めずに「吸着を切った場合」と「入れた場合」の両方を持たせる。
3. **`pick` を `pick_clipped` に替える3か所**（選択・注釈・計測）。1か所でも替え忘れると、見えていない面に注釈が付く。`session::pick` の入口を1つにして、そこでだけ断面を渡す。
4. **表示の単位を通す場所**。通し忘れると、同じ画面に m と mm が混ざる。換算する場所の一覧（design-stl-units.md の表）を、試験 `display_mm_converts_readouts_only` の項目と1対1にする。
5. **開き直し（`SetStlUnit`）** は、別のモデルを開くときと同じ経路（未保存の確認、読み込み中の表示、失敗しても前のモデルが残る）を通す。専用の近道を作らない。
