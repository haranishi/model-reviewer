# Implementation Plan

作業は「作業パッケージ（WP）」に分ける。この文書と、各WPが挙げる設計の文書だけを渡された実装者が進められるように書いた。第4段階（[shortcuts-guide](../shortcuts-guide/tasks.md)）の作業と順が絡むので、**全体の順をここに書く**。

## 全体の順（2つの仕様を通して）
```
前提: 第3段階の WP-U1・L1・J2 が取り込まれている
 ① 新規作成      N1 土台（core・gltf）─▶ N2 つなぐ（app）
 ② ショートカット  shortcuts-guide の M1 ─▶ (P) S1・V1・K1 ─▶ J1        ← キーで動かす、ポインタの場所によらないキーを含む
 ③ 色と質感      (P) C1 文書と描画（core・render）・C2 書き出し（gltf）─▶ C3 つなぐ（app）
 ④ 手順書の仕上げ  shortcuts-guide の G1 ─▶ F1（進行役）＋ この仕様の F1（進行役）
```
- C1 と C2 は、N1 の後ならいつ始めてもよい（②と同時に進められる。担当ファイルが重ならない）。**C3 は、②の J1 の後**に行う（`command.rs`・`session/mod.rs`・`ui/mod.rs`・`ui/ids.rs`・`automation*` が重なるため）。
- N2 は、②の M1 より前に取り込む（M1 が、Cmd+N の分岐をキーの表へ移す）。

## 進め方の約束
- **着手前に読むもの**: [design.md](design.md)、[design-decisions.md](design-decisions.md)（落とし穴11件と要確認）、第1段階の [落とし穴13件](../model-reviewer/design-decisions.md)、第3段階の [design-testing.md](../cad-inspect/design-testing.md) の「3つのOSで同じ結果にするための決まり」、自分のWPに書かれた文書。
- **全WP共通の合格条件**: `cargo fmt --all --check`・`cargo test --workspace`・`cargo clippy --all-targets --workspace -- -D warnings`・`cargo build --release` が通る。既存の試験を消したり弱めたりしない。commit・push はしない。外部の依存を足さない（`Cargo.lock` が変わったら、作業を止めて進行役に伝える）。
- **担当ファイルの外を変えない。** 型に項目や場合を足したせいで通らなくなる組み立て（`match` の場合分け、構造体の手書き）は、足した分だけ直してよい。直したファイルを報告する。それ以外で外を変える必要が出たら、作業を止めて進行役に伝える。
- GPUと画面が要る確認（実機の自動操作、画像の目視、実キー、性能）は進行役が行う。実装者は、実行していないものを「未確認」と報告する。API は推測で書かず、依存の実物で確かめる。「要確認」の項目は、確かめた結果を報告に書く。
- 文書とコードに、個人名・メール・ホームの絶対パス・作業に使った道具の名前を書かない。画面の言葉は日本語にする。

- [ ] 1. 新規作成
- [ ] 1.1 WP-N1 空のモデルと空の土台（[design-new.md](design-new.md) の「型の約束」の `core` の分、[design-export.md](design-export.md) の「空の土台」）
  - `core::scene::Model::empty()`。`gltf::export::empty_base()`。土台を読む所（`export/base.rs`）が、ノード0個で `NoMesh` のときだけ空のモデルを土台にする（`load_glb` は変えない）。`build` が、土台が空で足す描画も無いとき `InvalidScene` を返す
  - 要確認を先に確かめる: BIN もバッファも無い GLB を、容器の読み取りと JSON の読み取りが受けるか。受けなければ、土台用の入口で扱い、報告する
  - 合格: `empty_model_makes_empty_document`、`empty_base_is_stable_bytes`、`empty_base_is_not_openable_as_model`、`exports_from_empty_base`（足した図形1つ。`load_glb` で開け、名前・行列・色・三角形の数が一致）、`same_document_same_bytes_from_empty_base`、`empty_scene_on_empty_base_is_refused`。既存の書き出しの試験が通る
  - _Boundary: `core/src/scene.rs`、`gltf/src/build.rs`（土台用の入口だけ）、`gltf/src/export/{mod,synth,base,patch}.rs`、`gltf/tests/`_ ／ _Requirements: 1.1, 2.1, 2.2, 2.5_
- [ ] 1.2 WP-N2 新規作成をつなぐ（[design-new.md](design-new.md) の全部。N1 の後）
  - `Command::NewDocument`、`SourceKind::New`、`Pending::New`、`Incoming.model_ref` を `Option` に。`session::open` に `new_guarded`・`new_document`。`commit_pending` の分岐（復元を呼ばない、単位、初期視点、状態欄、性能値）。`model_ref` が無いときに失敗や空になる箇所を洗い出して直す（状態欄の名前、詳細パネル、状態の書き出し）
  - 書き出し: `ExportBase::Empty`（読む工程と識別の照合を飛ばす）。描画を持つ部品が無ければ断る。保存先の既定の名前「無題.glb」。終わったら `source_kind` を Glb に。レビュー保存を断る文
  - 画面: 何も開いていない画面の［新規作成］（`empty.new`）、Cmd+N（`app.rs`。「パネルの上」の確かめの前）、ヘルプの1行、図形を置けないときの文
  - 自動操作: `dump_state` の `source_kind: "new"`。`e2e/new-1.json`。`docs/verify.md` の手順の表、`docs/glb.md` に「新規の文書から書き出した GLB」の節、`docs/roadmap.md`
  - 合格: [design-testing.md](design-testing.md) の「新規作成」の行（`new_document_starts_empty`、`new_document_asks_before_discarding`、`new_document_is_refused_while_busy`、`first_shape_is_one_meter_at_origin`、`deleting_everything_keeps_the_document`、`review_save_is_refused_until_export`、`new_document_exports_and_moves_target`、`empty_document_export_is_refused`、`cmd_n_works_over_panels_but_not_while_typing`）。GPUの試験 `empty_model_uploads_and_accepts_shapes`（`#[ignore]`。実行は進行役）。既存の試験が通る
  - _Boundary: `app/src/{command,app,exporter}.rs`、`app/src/session/{mod,open,export,persist,edit}.rs`、`app/src/ui/{empty,help,status,details,ids}.rs`、`app/src/automation/`、`app/tests/{new_doc,gpu_flow}.rs`、`app/e2e/new-1.json`、`docs/{glb,verify,roadmap}.md`_ ／ _Depends: 1.1_ ／ _Requirements: 1.1〜1.10, 2.3, 2.4, 2.6, 2.7, 6.1〜6.4_

- [ ] 2. 色と質感
- [ ] 2.1 (P) WP-C1 文書・履歴・部分更新（[design-color.md](design-color.md) の「型の約束」と、`render::update_instances` の直し）
  - `core/src/color.rs`（換算、16進、見本12色）。`Document::subtree_colors`・`set_subtree_colors`。`Edit::SetColors` と `Applied::recolored`（`History` の実行・取り消し・やり直し、`bytes`、版）。`render::SceneRenderer::update_instances` が、行列と一緒に色も文書から読む
  - 合格: design-testing.md の「色の換算」「文書の色」「履歴」「部分更新」の行（`srgb_round_trips_all_256`、`hex_parse_table`、`swatches_are_distinct`、`color_does_not_leak_to_shared_shape`、`wrong_count_changes_nothing`、`duplicate_inherits_color`、`undo_restores_per_draw_colors`、`set_colors_keeps_alpha`、`recolor_bumps_document_rev_only`、`slots_follow_node_then_draw_order`、`patch_rewrites_color_and_keeps_ids`）。変形の部分更新の既存の試験が通る
  - _Boundary: `core/src/{color,doc,history,lib}.rs`、`render/src/{lib,instances}.rs`_ ／ _Requirements: 3.3, 3.4, 3.7, 3.10〜3.12_
- [ ] 2.2 (P) WP-C2 色の書き出し（[design-export.md](design-export.md) の「色の変わった部品」「照合」。N1 の後）
  - 読み込みの組み立てが「描画ごとの（メッシュ、プリミティブ）」を内部の結果に残し、土台が持つ。`export/recolor.rs`（材質の写し、メッシュの写し、まとめる鍵）。`patch::build` で、ファイルにある部品と複製のノードの `mesh` を替える。`ExportNote::Recolored`。`verify` の4つの直し（design-export.md の表）
  - `ExportNote` に場合が増えるので、`app/src/session/export.rs` の文を1つ足す（design-export.md の「結果の一覧」の1行目。テクスチャの注意の行は C3）
  - 合格: design-testing.md の「色の写し」「照合」の行（`recolor_adds_material_and_mesh_only`、`recolor_shares_material_and_mesh`、`recolor_keeps_texture_refs_and_extensions`、`recolor_without_material`、`recolor_maps_draws_past_skipped_primitives`、`recolor_of_duplicate_only`、`restoring_color_restores_json`、`second_export_adds_nothing`、`verify_rejects_non_copy_material`、`verify_rejects_mesh_copy_with_other_changes`、`verify_rejects_one_bit_color_difference`）。既存の書き出しと照合の試験が通る
  - _Boundary: `gltf/src/build.rs`、`gltf/src/export/{mod,base,patch,append,verify,recolor}.rs`、`gltf/tests/`、`app/src/session/export.rs`（文を1つ）_ ／ _Depends: 1.1_ ／ _Requirements: 4.1〜4.8_
- [ ] 2.3 WP-C3 色をつなぐ（[design-color.md](design-color.md) の「つなぎ方」「画面の部品」「見た目に出るもの・出ないもの」。C1・C2 と、第4段階の J1 の後）
  - `Command::SetColor`、`session/color.rs`（`set_color`、`selection_color` と控え）、`after_applied` で `recolored` を部分更新へ。`ui/color.rs` と、右のパネルへの配置（寸法の下、計測の上）。部品の名前。注意の文（モデルにテクスチャか頂点の色があるとき）と、結果の一覧の注意の行
  - 自動操作: `dump_state` の `selection.color`。`e2e/color-1.json`・`desk-1.json`（机の一式。第4段階のキーで動かす手順を使う。実行は進行役）。`docs/glb.md` に「色を変えたときに書かれるもの・残るもの・ほかのソフトでの見え方」、ヘルプの「できないこと」に、テクスチャと金属らしさは出ない・変えられないこと
  - 合格: design-testing.md の「色（つなぎ）」「色の部品」の行（`set_color_is_one_history_step`、`same_color_adds_no_history`、`color_marks_document_unexported`、`color_is_refused_while_busy`、`color_change_patches_instances_only`、`color_survives_export_and_reopen`、`texture_note_is_shown_for_textured_model`、`swatch_click_sets_color`、`hex_and_rgb_commit_on_enter`、`bad_hex_is_refused_with_reason`、`mixed_and_no_faces_states`、`color_panel_fits_at_960`）。GPUの試験 `recolor_changes_only_that_part`（`#[ignore]`）。既存の試験が通る
  - _Boundary: `app/src/command.rs`、`app/src/session/{mod,color,edit,export}.rs`、`app/src/ui/{mod,ids,color,inspector,help}.rs`、`app/src/automation/`、`app/tests/{color,color_panel,layout,gpu_flow}.rs`、`app/e2e/{color-1,desk-1}.json`、`docs/{glb,verify}.md`_ ／ _Depends: 2.1, 2.2_ ／ _Requirements: 3.1, 3.2, 3.5, 3.6, 3.8, 3.9, 5.1〜5.5, 6.1〜6.3_

- [ ] 3. 最後の確認（進行役が行う。第4段階の F1 と一緒に行ってよい）
- [ ] 3.1 WP-F1 確認と記録
  - `cargo test --workspace -- --include-ignored`。実機の自動操作: 既存の全部と、`new-1`・`color-1`・`desk-1`。見本のハッシュが前後で同じ
  - 実キーの Cmd+N。画像の目視、軽さの測定、ほかのソフトでの確認（[design-testing.md](design-testing.md) の最後の節）。結果で、design-export.md の要確認（同じ名前の材質）を決める
  - 記録（`verification.md`）に、測った値・機械・条件と「未確認」を書く。`spec.json` の `phase` を `implemented` にし、`documents` に `verification.md` を足す。`docs/roadmap.md` に、この段階と、次の段階（テクスチャ）の入口を書く
  - _Requirements: 6.4, 7.1〜7.4_

## 接合で気をつける所（壊れやすい順）
1. **照合をゆるめる所（C2）。** 「BIN なしでメッシュと材質が増える」「ノードの `mesh` が変わる」を通すようにするとき、写しでないものまで通すと、壊れたファイルを書いても気づけなくなる。断る試験3本（`verify_rejects_*`）を、通す試験より先に書く。
2. **描画とプリミティブの対応（C2）。** 点や線が混ざったメッシュで1つずれると、別の面の色が変わる。対応は読み込みの結果から取り、試験 `recolor_maps_draws_past_skipped_primitives` で固定する。
3. **空のモデル（N2）。** 今まで「文書がある ⇒ 部品が1つ以上ある・`model_ref` がある」に頼っていた箇所が、パニックや誤った文になる。GPUの試験と、`model_ref` を読む箇所の洗い出しを報告に含める。
4. **色の往復（C1・C2）。** 書いた色が読み直した色と1ビットでも違うと、書き出すたびに材質が増える。`second_export_adds_nothing` で固定する。
5. **部分更新（C1）。** 今は色を前の値のまま写している。直した後も、変形の確定で色が変わらないこと（文書の色と同じ値が入るだけ）を既存の試験で確かめる。
