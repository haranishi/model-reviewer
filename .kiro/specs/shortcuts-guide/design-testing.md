# テストと自動操作

第1〜第3段階の方針（論理はGPUなしで試す、実機の確認はOSの入力を使わない自動操作、3つのOSで同じ結果にする決まり）を保つ。検証コマンドも同じ4つである。

## 利用者の設定に触れない仕組み（要件7）
- 設定の場所を決める関数（`default_path`）は、環境を引数で受け取る。本物の環境変数を渡すのは `main.rs` だけである。
- `LaunchArgs.keymap` が `None` なら、`App::new` は `Store::none()`（読まない・書かない）を使う。既存の試験は `LaunchArgs` を手で組み立てているので、足した項目に `None` を書けば、今までどおり何も読まない。`Session::default()` も `Keymap::default()` と `Store::none()` である。
- 試験でファイルを使うときは、`std::env::temp_dir()` の下に、試験ごとに違う名前のフォルダを作って渡す。試験の中で、OSごとの本物の場所を組み立てない。
- `--script` で `--keymap` が無いとき、`main.rs` は場所を `None` にする。設定の画面には「自動操作のため、変更は保存されません」と出る。
- **確かめ方**: 試験 `resolve_path_prefers_flag_then_script_then_os`（引数から場所を決める関数 `keymap::store::resolve_path` を置き、`main.rs` はそれを呼ぶだけにする。関数は環境を引数で受ける）。進行役は、実機の確認の前後で、利用者の設定ファイルの有無と更新時刻が変わっていないことを見る。

## どこで何を確かめるか
| 対象 | 置き場所 | 確かめること（試験名の目安） |
|---|---|---|
| 操作の一覧 | `keymap/actions.rs` の中 | 名前が重ならず `from_id(id())` で戻る。41個。既定の組が互いに重ならず、どれも `Chord::from_event` を通る（`defaults_are_unique_and_allowed`）。既定が第3段階の表と同じ（`defaults_match_stage_three`。表を試験の中に書き写して固定する） |
| キーの組 | `keymap/chord.rs` の中 | `parse(to_file(c)) == c`。断る理由の表（design-keymap.md）の各行。修飾の完全一致。macOS の Control と Alt を断る。`label` が OS の別で「Cmd」「Ctrl」になる（`Platform` を引数で渡すので、どのOSでも両方を試せる） |
| 表の書き換え | `keymap.rs` の中 | 重なると `Err(相手)` で何も変わらない。`replace` で相手が未割り当て。`clear`・`reset_all`。変わると `rev` が増え、変わらなければ増えない |
| 場所 | `keymap/store.rs` の中 | OSごとの場所。変数が無い・空・相対で `None`。`XDG_CONFIG_HOME` が先（`default_path_per_os`） |
| 読み書き | `crates/app/tests/keymap.rs` | 保存して読むと同じ割り当て（`round_trips_changes_only`）。既定のままなら `bindings` が空。同じ割り当てから同じバイト列。知らない名前と最上位の項目が残る。読むときの決まりの表の各行（`broken_file_is_kept_as_broken_on_first_save`、`newer_version_is_never_overwritten`、`unreadable_file_is_not_overwritten`、`user_binding_beats_default_of_another_action`、`duplicate_user_bindings_keep_the_first`）。決まった種のランダムなバイト列1万通りで落ちない。保存先のフォルダが無ければ作る。保存に失敗（保存先がフォルダになっている）すると理由が返り、一時ファイルが残らない |
| 振り分け | `crates/app/tests/keys_flow.rs`、既存の `edit`・`select`・`view`・`undo`・`snap`・`section` | 既定で、第3段階のキーがすべて同じ `Command` になる（`default_keymap_dispatches_like_before`。キーと期待の表を1つ持つ）。G を T に変えると、T で移動になり G は何もしない。入力中・ダイアログ中・ヘルプ中・設定の画面の間は働かない。パネルの上では `Scope::View` が働かず、`Anywhere` は働く。Shift+Home が働かない（変えた所）。Cmd+O が `OpenModelDialog` を積む |
| 設定の画面 | `crates/app/tests/keys_window.rs`（`egui_kittest`） | 入力用の構造体だけで動く。行を押すと `BeginKeyCapture`。重なりの確認の2つのボタン。変えた行だけに印と [初期値に戻す]。検索で行が絞られる（名前とキー）。[すべて初期値に戻す] は2回目で `ResetAllKeys`。保存されない事情の文。日本語の文言と、キーの文字（F1・PageUp など）で落ちない。幅960×600で行が重ならない |
| 取り込み | `crates/app/tests/keys_flow.rs` | 取り込み中の Esc で変わらない。断るキーで理由が出て取り込みが続く。重なり → [置き換える] で相手が未割り当て。「コピー」の出来事で `TextEditing` の理由。取り込みの直後から新しいキーが働く。変えると一時フォルダのファイルが書かれ、新しい `Session` で読むと同じになる（`change_is_saved_and_restored`） |
| キーの文字 | 既存の `toolrail`・`viewbar`・`layout`、`ui/help.rs` の中 | 割り当てを変えると、説明の文・道具の列の1文字・ヘルプの表が変わる。未割り当てで括弧が出ない。既定のままなら、今の文字と同じ（`default_labels_keep_existing_text`。今の文を試験に固定してから直す） |
| 手順書 | `crates/app/tests/guide.rs` | 既定の表と変えられないキーの表が、アプリの表と一致。リンクと画像が実在。ページがそろい、100行以内（design-guide.md） |
- 既存の試験を弱めない。`shortcut_commands` を呼んでいる既存の試験は、手を入れずに通ること（表が既定だから）。入力用の構造体に `keys` を足した分だけ、組み立てを直す。
- 3つのOSで同じにするための注意: 画面の文字を比べる試験は `Platform` を明示する（実行したOSの `cfg!` に頼らない）。パスは `Path::join` で作る。ファイルの中身を比べるときは、改行を `\n` に決めて書く（`serde_json` の出力に `\n` を1つ足すだけにする）。

## 自動操作に足すもの
- **手順は足さない。** 設定の画面は、既存の `click_widget`・`type_text`・`key` で操作できる。取り込みは、`key` の出来事を `shortcut_commands` が読むので、前面でないウィンドウでも働く。
- **引数**: `--keymap <ファイル>`。相対の場所は、ほかの引数と同じく作業場所から読む。
- **状態の書き出し**（`dump_state`）に `keys` を足す: `source`（`"default"`・`"file"`）、`saved`（保存されるか）、`changed`（既定と違う操作の名前 → ファイルの書き方か `null`）、`window`（`open`、`capture`、`conflict` の相手）。既存の項目は変えない。`keys.source` と `keys.saved` は、起動し直した後の一致の比較（`assert_state_equals`）から外す項目に足す。
| 手順 | 内容 |
|---|---|
| `keys-1.json` | `--keymap` に `--out` の下のファイルを渡して起動 → 見本を開く → ヘルプ → [ショートカットの設定] → 検索に「移動」→ 行を押す → `key` T → 状態で `tool.move` が T → 閉じる → `key` T で道具が移動、`key` G では変わらない → 「回転」を T にしようとして重なりの確認 → [やめる] → 画像 → 状態を書き出す |
| `keys-2.json` | 同じ `--keymap` で起動し直す → 状態の `keys.changed` が `keys-1` の最後と同じ → [すべて初期値に戻す] を2回 → `changed` が空 → `key` G で移動 |
| `guide-*.json` | 手順書の画像（design-guide.md）。`--keymap` は付けない（既定になる） |
- 既存の手順（第3段階までの全部）は、**手順を変えずに**通ること。`--script` では既定を使うので、進行役の機械の設定に左右されない。これを確かめるために、進行役は一度、自分の設定で G を別のキーに変えた状態で `edit-1` を実行し、通ることを見る。

## 進行役が実機で確かめること（実装者は「未確認」と報告する）
実キーで: 既定のキー、足した Cmd+O・Cmd+S・Cmd+E・F1、取り込み、Cmd+C を取り込もうとしたときの理由、Cmd+Q の未保存の確認が今までどおり出ること。日本語入力を入にしたまま、入力欄の外で G・1・Cmd+Z が働くか（要件8.2）。日本語配列のキーボードで、英字と数字が同じに働くか。設定を変えて起動し直し、残っていること。設定ファイルを手で壊して起動し、既定で立ち上がり、次の変更で `.broken` が残ること。画像の目視: 設定の画面（既定、取り込み中、重なり、断った理由）、道具の列の1文字、幅960。
