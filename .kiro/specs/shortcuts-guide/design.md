# Design Document（入口）

第4段階「ショートカットの設定と、公式の手順書」の設計。要件は [requirements.md](requirements.md)、体験の設計は [ux.md](ux.md)、作業の割り振りは [tasks.md](tasks.md)。第1〜第3段階の設計は有効で、この文書群は変える所と足す所だけを書く。判断と理由、境界の約束を書き、処理の逐語訳は書かない。

## Overview
3つを足す。①操作とキーの対応を1つの表（`Keymap`）にし、キーの振り分けと画面のキーの文字を、どちらもこの表から作る。②表を画面から変えられるようにし、既定との差だけを利用者ごとのファイルに保存する。③使い方の手順書を `docs/guide/` に置き、画像は自動操作で撮り直せるようにする。新しい外部の依存は無い（JSON の読み書きは、すでに使っている `serde_json`。場所は環境変数から決める）。

### Goals
- 既定のままなら、第3段階とキーの働きが同じ（変わる所は要件2.7 の1つと、足した4つのキー）。既存の自動操作の手順が、手順を変えずに通る。
- 画面に出るキーの文字が、どこでも今の割り当てと一致する。
- 設定ファイルがどんな内容でも起動でき、利用者の割り当てを黙って失わない。
- キーの表・ファイルの読み書き・設定の画面を、GPUと実機の画面なしで、3つのOSで試験できる。試験は利用者の本物の設定に触れない。

### Non-Goals
操作を探して実行する小窓、Alt（Option）・記号キー・テンキーの割り当て、1つの操作に複数のキー、2打鍵の割り当て、設定の書き出しと読み込みの画面、ほかの設定（単位、ウィンドウ、配色）の記憶、ファイルの変更の見張り、手順書の英語版、アプリの中に手順書の本文を出すこと。保留の理由は [design-decisions.md](design-decisions.md)。

## 文書の構成
| 文書 | 内容 |
|---|---|
| [design-keymap.md](design-keymap.md) | 操作の一覧と既定、型の約束、キーの照合、割り当てられないキー、画面のキーの文字 |
| [design-settings.md](design-settings.md) | 設定の画面の状態と `Command`、設定ファイルの場所・形式・壊れた内容の扱い・書き込み |
| [design-guide.md](design-guide.md) | 手順書のページ構成、画像の撮り直し、入口、アプリの文との照合 |
| [design-testing.md](design-testing.md) | 試験の置き場所、自動操作に足すもの、利用者の設定に触れない仕組み |
| [design-decisions.md](design-decisions.md) | 判断の一覧、捨てた案、落とし穴、確かめた事実と要確認、保留、依頼者に聞く分かれ道 |

## Boundary Commitments
- **この仕様が持つもの**: `app::keymap` の公開の型（`Action`・`Chord`・`Keymap`・`KeyLabels`・`Refusal`）、操作の名前（ファイルに書く文字列。**一度出したら変えない**）、設定ファイルの形式と版、`LaunchArgs.keymap` と引数 `--keymap`、`Command` の追加分、自動操作の状態に足す `keys`、`docs/guide/` の構成。
- **持たないもの**: 各操作の中身（積む `Command` は既存のもの）、Esc の振り分けの順、入力欄の中の編集キー（egui の持ち物）、OSのメニュー。
- **依存の向き**: 変えない。`keymap` は `app` の中に置き、`core` には入れない（egui のキーの型を使うため）。`keymap` は `Session` を読まない。`keymap::store` は画面も `Session` も読まない。
- **見直しが必要になる変更**: 操作の名前の付け替え、ファイルの版、修飾の持ち方（Alt を足すなど）、「自動操作は既定を使う」という約束。

## Architecture
```
 実行ファイルの入口（main.rs）── 設定の場所を決める（--keymap ＞ 自動操作なら無し ＞ OSごとの場所）
        │ LaunchArgs.keymap: Option<PathBuf>
        ▼
 App::new ─ keymap::store::Store::open ─▶ Session.keymap（Keymap）・Session.keys（設定の画面の状態）
                                                │
 入力(egui) ─▶ app::shortcut_commands ─ 固定のキー（Esc ほか。今のまま）
        │            └ それ以外: 押されたキー → Chord → Keymap::action_for → 操作 → 既存の Command
        │      ui/*（KeyLabels を入力用の構造体で受け取り、説明・道具の列・ヘルプの表に出す）
        │      ui/keys（設定の画面。入力用の構造体だけを受け取り、Command を積む）
        ▼ Command（AssignKey・ClearKey・ResetKey・ResetAllKeys ほか）
 Session::apply ─ Keymap を書き換える ─▶ Store::save（一時ファイル → 付け替え）─ 失敗は状態欄の知らせ
```
- **表は1つ。** キーの振り分け（`app.rs`）、説明の文、道具の列の1文字、ヘルプの表、設定の画面、手順書の既定の表の試験は、すべて `Keymap` と `Action` の一覧を読む。キーを文字列で手書きした箇所を残さない。
- **設定の場所を決めるのは、実行ファイルの入口だけ。** ライブラリ側（`App::new`・`Session`）は、渡された場所しか読み書きしない。試験は場所を渡さないか、一時フォルダを渡すので、利用者のファイルに届かない。
- **状態の変更は `Command` と `Session::apply` を通る**（第1段階のまま）。割り当ての変更は、履歴にも未保存の印にも入れない（モデルやレビューの変更ではない）。
- **キーの取り込みは `app::shortcut_commands` が行う。** キーの出来事を読む場所を1つに保つ。画面の部品の `has_focus` は使わない（前面でないウィンドウで常に偽）。

## File Structure Plan（足すファイルと、変えるファイル）
```
crates/app/src/     keymap.rs  keymap/{actions,chord,store}.rs  ui/keys.rs（新規）
                    command.rs app.rs main.rs lib.rs session/mod.rs automation.rs automation/{state,script}.rs
                    ui/{mod,ids,help,toolrail,viewbar,toolbar,status,empty}.rs（変更）
crates/app/tests/   keymap.rs  keys_window.rs  keys_flow.rs  guide.rs（新規）  既存の試験（LaunchArgs と入力用の構造体の組み立てだけ）
crates/app/e2e/     keys-1.json  keys-2.json  guide-*.json（新規）
docs/guide/         README.md  01-start.md … 10-troubleshooting.md  images/（新規）
docs/verify.md  README.ja.md  README.md（変更）
```

## Requirements Traceability
| 要件 | 実現する場所 | 詳細 |
|---|---|---|
| 1 キーが分かる | keymap（`KeyLabels`）、ui::{toolrail,viewbar,toolbar,help,empty} | design-keymap.md |
| 2 操作とキーの表 | keymap::{actions,chord}、app::shortcut_commands | design-keymap.md |
| 3 自分で設定する | ui::keys、session（`keys`）、`Command` の追加分 | design-settings.md |
| 4 割り当てられないキー | keymap::chord（`Refusal`） | design-keymap.md |
| 5 保存 ／ 6 壊れた内容 | keymap::store | design-settings.md |
| 7 自動操作と試験 | main.rs、command（`--keymap`）、automation | design-testing.md |
| 8 日本語配列と入力 | keymap::chord、app::shortcut_commands、手順書 | design-keymap.md、design-decisions.md |
| 9 手順書 | docs/guide、e2e/guide-*.json、tests/guide.rs | design-guide.md |
| 10 検証と体裁 | 各試験、docs/verify.md | design-testing.md |

## 実装者への注意
- 第1段階の落とし穴13件、第2・第3段階の「確かめたAPI」「落とし穴」は、そのまま当てはまる。先に読むこと。
- この段階の落とし穴は [design-decisions.md](design-decisions.md) にある。「要確認」と書いた項目は、使う前に依存の実物か小さな試験で確かめ、結果を報告する。
- 第3段階の区切りBで、`ui/help.rs`・`ui/status.rs`・`automation*`・README が変わる。**取り込んだ後の内容を土台にする**（この文書の行番号や文言は、区切りAの時点のもの）。
