# Implementation Plan

作業は「作業パッケージ（WP）」に分ける。この文書と、各WPが挙げる設計の文書だけを渡された実装者が進められるように書いた。

## 進め方の約束
- **始める条件**: 第3段階（`../cad-inspect/`）の WP-U1・L1・J2 と、[creation-basics](../creation-basics/tasks.md) の WP-N1・N2（新規作成）が取り込まれていること。取り込まれていなければ、作業を止めて進行役に伝える。全体の順は creation-basics の tasks.md にある（新規作成 → この仕様の M1〜J1 → 色 → この仕様の G1・F1）。
- **着手前に読むもの**: [design.md](design.md)、[design-decisions.md](design-decisions.md)（落とし穴13件と要確認）、[design-testing.md](design-testing.md) の「利用者の設定に触れない仕組み」、第1段階の [落とし穴13件](../model-reviewer/design-decisions.md)、第3段階の [design-testing.md](../cad-inspect/design-testing.md) の「3つのOSで同じ結果にするための決まり」、自分のWPに書かれた文書。
- **全WP共通の合格条件**: `cargo fmt --all --check`・`cargo test --workspace`・`cargo clippy --all-targets --workspace -- -D warnings`・`cargo build --release` が通る。既存の試験を消したり弱めたりしない。commit・push はしない。外部の依存を足さない（`Cargo.lock` が変わったら、作業を止めて進行役に伝える）。
- **担当ファイルの外を変えない。** 境界の型（WP-M1 が置く型）や他のWPのファイルを変える必要が出たら、作業を止めて進行役に伝える。
- **共有の大きなファイル**（`command.rs`・`app.rs`・`main.rs`・`lib.rs`・`session/mod.rs`・`ui/mod.rs`・`ui/ids.rs`・`automation*`）に触れるのは、WP-M1 と WP-J1 だけ。
- **試験は、利用者の本物の設定の場所を読みも書きもしない。** ファイルを使う試験は `std::env::temp_dir()` の下に、試験ごとの名前で作る。OSごとの本物の場所を、試験の中で組み立てない。
- GPUと画面が要る確認（実機の自動操作、画像の目視、実キー、日本語入力）は進行役が行う。実装者は、実行していないものを「未確認」と報告する。API は推測で書かず、依存の実物（cargo のレジストリの下）で確かめる。「要確認」の項目は、確かめた結果を報告に書く。
- 文書とコードに、個人名・メール・ホームの絶対パス・作業に使った道具の名前・ほかの製品の名前を使ったキー配列を書かない。画面の言葉は日本語にする。

## 並行の組と依存
```
WP-M1 表と振り分け（単独・最初）
  ├─(P) S1 保存（設定ファイル）──────────┐
  ├─(P) V1 画面（設定の小窓とキーの文字）─┼─▶ J1 接合 ─▶（creation-basics の色）─▶ G1 手順書 ─▶ F1 確認（進行役）
  └─(P) K1 キーで動かす（量と状態）───────┘
```
`(P)` の3本は担当ファイルが重ならず、M1 の後なら同時に進められる。J1 は S1・V1・K1 の後。**G1 は、creation-basics の色（WP-C3）が取り込まれた後に行う**（手順書が、新規作成・キーで動かす・色をまとめて書くため。2026-11-27 に順を変えた）。

- [x] 1. 土台
- [x] 1.1 WP-M1 表と振り分け（[design-keymap.md](design-keymap.md)。単独で最初に行う）
  - `keymap.rs`・`keymap/{actions,chord}.rs` を、**中身まで**書く（`Action` の42個。`file.new` は既にある Cmd+N の分岐を表へ移す。`Chord`、`Refusal`、`Platform`、`Keymap`、`KeyLabels`。型は design-keymap.md。`Scope` は作らない）
  - `app::shortcut_commands` を、表を引く形に直す。形 `(ctx, session)` は変えない。固定のキーと Esc の順は今のまま。足す既定の4つと、修飾の完全一致（Shift+Home）は、ここで入る。**`over_panel` の判定を消し**、入力の位置を持つ部品があるときの Delete・Backspace・矢印・Enter だけを捨てる（要件2.5）。「パネルの上では働かない」を確かめている既存の試験は、期待を直して試験名を報告する（消さない）。操作ごとの「ボタンが押せないときは何もしない」は、既存の分岐の条件を写す
  - **置くだけにするもの**（パニックさせず、既定・空・何もしないを返す）: `keymap/store.rs`（`Os`、`default_path` は `None`、`Store::none`・`open`・`save`・状態の型）、`Session` の `keymap`・`key_labels`・`keys`・`store`、`Command` の9つ（design-settings.md。`apply` は空の関数を呼ぶだけ。`session/keys.rs` を新規に置く）、`ui/keys.rs`（`KeysInput`・`KeyRow` と、何も描かない `show`、`matches`）、`ui/ids.rs` の名前、`ToolRailInput`・`ViewBarInput` の `keys: KeyLabels`（まだ使わない）、`LaunchArgs.keymap` と引数 `--keymap`（`usage` の文も直す）、キーで動かす分（[design-keymove.md](design-keymove.md) の `Command` 3つと `KeyInput`、`TransformSource::Keys`・`KeyEntry`、`session/keymove.rs` の3つの関数の形、`core::gizmo::key_delta` は `None` を返す形）
  - 項目を足した構造体を手で組み立てている箇所（試験を含む）を、すべて直す
  - 合格: 既存の試験がすべて通り、件数が減っていない。`defaults_are_unique_and_allowed`、`defaults_match_stage_three`、`chord_round_trips_through_file_text`、`refusals_cover_the_table`、`assign_reports_the_holder`、`default_keymap_dispatches_like_before`、`shift_home_no_longer_fits`、`keys_work_over_panels`、`focused_widget_keeps_delete_and_arrows`、`keymap_flag_is_parsed`。既存の自動操作の手順が通る（進行役）
  - _Boundary: `core/src/gizmo.rs`（置くだけ）、`app/src/{keymap.rs,keymap/,command.rs,app.rs,lib.rs}`、`app/src/session/{mod,keys,keymove,edit}.rs`（`edit.rs` は型だけ）、`app/src/ui/{ids,keys}.rs`、`ui/{toolrail,viewbar}.rs`（項目を足すだけ）、既存の試験（構造体の組み立てだけ）、`app/tests/keys_flow.rs`（振り分けの分）_ ／ _Requirements: 2.1〜2.8, 4.1〜4.4, 7.3, 7.5, 8.1_
  - 注意: 置くだけにした `Command` は、`apply` で何もしない。X・Y・Z と矢印の振り分けは J1 で入れる（M1 の時点では、X は吸着、Z は面と線のまま）

- [x] 2. 並行して進める部品（M1 の後）
- [x] 2.1 (P) WP-S1 保存（[design-settings.md](design-settings.md) の「設定ファイルの場所」以降）
  - `default_path`、`resolve_path`（引数と自動操作の有無から場所を決める。環境は引数で受ける）、`Store::open`（読むときの決まりの表）、`Store::save`（一時ファイルからの付け替え、`.broken` を残す）、知らない項目の書き戻し、日本語の知らせの文
  - 合格: design-testing.md の「場所」「読み書き」の行（`default_path_per_os`、`resolve_path_prefers_flag_then_script_then_os`、`round_trips_changes_only`、`same_keymap_same_bytes`、`unknown_entries_survive`、`broken_file_is_kept_as_broken_on_first_save`、`newer_version_is_never_overwritten`、`unreadable_file_is_not_overwritten`、`user_binding_beats_default_of_another_action`、`duplicate_user_bindings_keep_the_first`、`random_bytes_never_panic`、`failed_save_leaves_no_temp_file`、`opening_writes_nothing`）
  - _Boundary: `app/src/keymap/store.rs`、`app/tests/keymap.rs`_ ／ _Requirements: 5.1〜5.7, 6.1〜6.6_
- [x] 2.2 (P) WP-V1 画面（[design-settings.md](design-settings.md) の「画面の部品」、[design-keymap.md](design-keymap.md) の「画面のキーの文字」、[ux.md](ux.md)）
  - `ui/keys.rs`（設定の小窓。入力用の構造体だけで動く。「変えられないキー」の欄に場面つきの固定のキーを並べ、`KeyRow.note` を行の下に出す）。説明の文・道具の列の1文字・上端と上部のボタンの説明・ヘルプと「何も開いていない画面」の表を、`KeyLabels` から作る。ヘルプに [ショートカットの設定] と手順書の場所・[場所を写す]
  - 直す前に、今の説明の文とヘルプのキーの列を試験に固定し、既定のままなら同じ文字になることを確かめる（つなぎの「＋」→「+」だけが変わる）
  - 合格: design-testing.md の「設定の画面」「キーの文字」の行（`row_click_begins_capture`、`conflict_offers_replace_and_cancel`、`only_changed_rows_show_reset`、`filter_matches_name_and_key`、`reset_all_needs_two_clicks`、`store_note_is_shown`、`key_names_render_without_missing_glyphs`、`rows_do_not_overlap_at_960`、`default_labels_keep_existing_text`、`rebinding_changes_hint_badge_and_help`、`unassigned_shows_no_parentheses`）
  - _Boundary: `app/src/ui/{keys,toolrail,viewbar,toolbar,help,empty}.rs`、`app/tests/{keys_window,toolrail,viewbar,layout}.rs`_ ／ _Requirements: 1.1〜1.6, 3.1, 3.2, 3.7〜3.9, 13.1, 13.2_
- [x] 2.3 (P) WP-K1 キーで動かす: 量と状態（[design-keymove.md](design-keymove.md) の「型の約束」。振り分けは J1）
  - `core::gizmo::key_delta` の中身。`session/keymove.rs`: `parse_amount`・`format_amount`・`step_of` と、`BeginKeyTransform`・`KeyTransformInput`・`Nudge` の中身（`Session` を受ける関数。`apply` からの呼び出しは M1 が置いた形のまま）。`session/edit.rs`: `Keys` のときの `handle()` と、確定の名前。`ui/gizmo_view.rs`: `Keys` のときも固定した軸を掴んだ色にする
  - 試験は `Session::apply` に `Command` を直接渡して書く（キーの振り分けを通さない）
  - 合格: `key_delta_matches_drag_result`、`key_delta_works_when_view_is_parallel`、`key_delta_refuses_bad_amounts`、`amount_parse_and_format_table`、`step_follows_grid_and_fine`、`typed_amount_previews_without_touching_document`、`enter_commits_one_history_step`、`axis_switch_keeps_amount`、`move_amount_is_read_in_display_unit`、`zero_or_empty_amount_changes_nothing`、`nudge_moves_one_grid_step`、`nudge_does_not_snap_to_grid`
  - _Boundary: `core/src/gizmo.rs`、`app/src/session/{keymove,edit}.rs`、`app/src/ui/gizmo_view.rs`、`app/tests/keymove.rs`、`app/tests/gizmo_view.rs`_ ／ _Requirements: 11.1, 11.3〜11.5, 11.7, 11.10, 11.11, 12.1〜12.5_
- [ ] 2.4 WP-G1 手順書（[design-guide.md](design-guide.md)。**creation-basics の WP-C3 の後**）
  - `docs/guide/README.md` と11ページ（画像の参照は、名前を決めて書く。画像そのものは F1 で入る）。`crates/app/tests/guide.rs`（既定の表と変えられないキーの表の照合、リンク、ページの数と行数。**画像の実在だけは、F1 まで `#[ignore]` にし、理由を書く**）。`crates/app/e2e/guide-*.json`（既存の手順と同じ書き方。実行は進行役）。README（日本語・英語）の入口と、キーのつなぎの文字
  - 画面の言葉は、コード（`ui/*.rs` の文字列）から写す。確かめていないことを、確かめたように書かない
  - 合格: `guide_default_table_matches_registry`、`guide_fixed_table_matches_registry`、`guide_links_resolve`、`guide_has_all_pages_within_limit`。手順の JSON が JSON として読め、`op` が `docs/verify.md` の一覧にある名前だけであること（`guide_scripts_use_known_ops`。手順を読む関数は公開されていないので、実行での確認は進行役）
  - _Boundary: `docs/guide/`、`app/tests/guide.rs`、`app/e2e/guide-*.json`、`README.ja.md`、`README.md`_ ／ _Requirements: 9.1〜9.6, 10.2, 13.3_

- [ ] 3. 接合（S1・V1・K1 の後。共有の大きなファイルに触れる）
- [ ] 3.1 WP-J1 つなぐ（[design-settings.md](design-settings.md) の `Command` の表と「取り込みの振り分け」、[design-testing.md](design-testing.md)）
  - `session/keys.rs`: `Command` の9つの中身と、変わったときの保存、失敗の知らせ、`key_labels` の作り直し。`app.rs`: 設定の画面が開いている間の振り分けと取り込み。`main.rs`: 本物の環境変数を渡して `resolve_path` を呼ぶだけ（`--keymap` ＞ 自動操作なら無し ＞ OSごとの場所）。`App::new`: `Store::open` と、起動時の知らせ
  - キーで動かす: `app.rs` に、数値の入力の途中の振り分けと、向きの固定・矢印の振り分け（design-keymove.md の順）。`ViewClick` で変形をやめる1行。状態欄の文、ヘルプの3行、`KeyRow.note` の組み立て、`dump_state` の `transform.key_entry`、`e2e/keymove-1.json`
  - `ui/mod.rs`: 設定の小窓を出す、`KeysInput` と各部品の `keys` を `Session` から組み立てる。ヘルプと小窓が同時に開かないこと
  - 自動操作: `dump_state` の `keys`、比較から外す項目、`e2e/keys-1.json`・`keys-2.json`。`docs/verify.md` に、手順の表と `--keymap`、「手順書の画像」の節
  - 合格: design-testing.md の「取り込み」の行と、「振り分け」の残り（`capture_takes_the_next_key`、`escape_cancels_capture`、`refused_key_keeps_capturing`、`clipboard_event_explains_itself`、`conflict_replace_unbinds_the_holder`、`rebound_key_works_immediately`、`change_is_saved_and_restored`、`no_keys_fire_while_window_is_open`、`dialog_takes_priority_over_window`、`script_without_keymap_uses_defaults`、`failed_save_keeps_binding_and_notifies`、`xyz_lock_axis_only_with_edit_tool_and_selection`、`x_still_toggles_snap_with_select_tool`、`backspace_edits_amount_not_part`、`other_keys_are_ignored_while_typing_amount`、`key_repeat_does_not_nudge`、`view_click_cancels_amount`）。既存の試験が通る（期待が変わった試験は、名前と理由を報告する）
  - _Boundary: `app/src/{app,main,lib,command}.rs`、`app/src/session/{mod,keys,pick}.rs`、`app/src/ui/{mod,ids,status,help}.rs`、`app/src/automation.rs`、`app/src/automation/`、`app/tests/keys_flow.rs`、`app/tests/keymove.rs`（振り分けの分）、`app/e2e/{keys-*,keymove-1}.json`、`docs/verify.md`_ ／ _Depends: 2.1, 2.2, 2.3_ ／ _Requirements: 1.4, 2.6, 3.3〜3.6, 3.10, 5.4, 5.5, 7.1, 7.2, 7.4, 8.3, 11.2, 11.6, 11.8, 11.9, 12.6, 12.7, 13.3, 13.4_

- [ ] 4. 最後の確認（進行役が行う。実機・画像の目視・実キー）
- [ ] 4.1 WP-F1 確認と記録
  - 実機の自動操作: 第3段階までの全部（手順を変えずに）、`keys-1` → `keys-2`、`keymove-1`、`guide-*`。自分の設定で G を別のキーに変えた状態で `edit-1` が通ること。実行の前後で、利用者の設定ファイルの有無と更新時刻が変わらないこと
  - 実キーと日本語入力（design-testing.md の最後の節。キーで動かす分は要件14）。結果で要件8.2 と手順書の「困ったとき」を直す
  - 画像の目視: 設定の小窓（既定、取り込み中、重なり、断った理由）、道具の列の1文字、ヘルプの下端、幅960と広い幅。重なりと、日本語の字形
  - 手順書の画像: `guide-*` の出力を1枚ずつ開いて見てから `docs/guide/images/` に置き、`guide.rs` の `#[ignore]` を外す。撮った条件を `docs/verify.md` に書く。手順書を頭から読み、画面の言葉と食い違いが無いことを確かめる
  - 公開の前の点検: `cargo deny check licenses sources bans`、最小の対応版での `cargo check --workspace --all-targets`、`Cargo.lock` に外部の項目が増えていないこと、文書とコードに個人名・メール・ホームの絶対パス・道具の名前が無いこと。CI の3つのOSの結果を記録する
  - 記録（`verification.md`）に「未確認」を残す: Windows と Linux の画面と設定の場所、確かめられなかった実キー。`spec.json` の `phase` を `implemented` にする。`docs/roadmap.md` に、この段階を書く
  - _Requirements: 7.3, 7.5, 8.2, 9.3, 9.4, 10.1〜10.3, 14.1_

## 接合で気をつける所（壊れやすい順）
1. **既定のままで、キーの働きが変わらないこと。** 振り分けを書き直すので、条件の写し忘れ（モデルが無いときの C、選択が無いときの Cmd+D、いまの道具と同じ道具）が起きやすい。`default_keymap_dispatches_like_before` に、キーと状態と期待の表を持たせる。
2. **取り込みにキーが届かない。** 検索の欄の入力の位置、ヘルプの小窓、ダイアログが先に返す。振り分けの入口の順（design-keymap.md）を、試験で1つずつ固定する。
3. **利用者の設定に触れる経路を作らない。** `resolve_path` と `default_path` に本物の環境変数を渡すのは `main.rs` だけ。ライブラリの中から本物の環境変数で呼ぶ箇所が無いことを、進行役が検索で確かめる。
4. **保存の失敗で割り当てが巻き戻らないこと。** 表を変えてから保存する。失敗しても表は変えたままにし、知らせを出す。
5. **数値の途中のキーが、表や削除へ漏れること。** 途中は、決めたキー以外を捨てる。Backspace の順を試験で固定する（`backspace_edits_amount_not_part`）。
