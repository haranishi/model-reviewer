# 第4段階のレビュー指摘と回帰試験

11件のうち10件を再現して修正した。追加した16試験のうち15件は修正前に失敗した。4番の試験は最初から通り、終了判定は従来どおりとした。下表のパスは `crates/` からの相対パス。

| 番号 | 修正前の試験結果 | 修正と確認内容 | 実装箇所 | 追加した試験 |
|---|---|---|---|---|
| 1 | 4件失敗。モデルと自動操作の手順が設定に置き換わり、後から現れたデータにも保存できた | 起動引数と以後に開く文書を別名のリンクまで照合する。GLB・レビュー・非JSON・64 KiB超を検出したら理由を表示する。原本と控えの名前・内容を検査した | `app/src/keymap/store.rs:202`、同318 | `app/tests/keymap.rs:294`、同318、`app/tests/keys_flow.rs:246`、同276 |
| 2 | 1件失敗。Gで注釈の下書きが消えた | 道具を替えても入力済みの本文を残す。右パネルの余白を押してGで移動に切り替え、同じ本文を登録できた | `app/src/session/notes.rs:20` | `app/tests/select.rs:526` |
| 3 | 1件失敗。R欄の確定で部品が消え、道具も替わった | 入力欄で確定したフレームのキーを、削除や割り当てた操作に渡さない。R欄と位置X欄でBackspace・G・Enterを一緒に送り、編集だけが反映された | `app/src/app.rs:172`、同211 | `app/tests/color_panel.rs:235` |
| 4 | 1件成功。色と変形の終了判定に差はなかった | 両方の欄に「25」と打ち、確定前に終了を要求した。どちらも確認は出ず、履歴は0件。終了判定の修正は不要 | `app/src/session/mod.rs:816` | `app/tests/color_panel.rs:225` |
| 5 | 1件失敗。起動後に現れた新しい版を上書きした | 置換直前に版を読み直して保存を止める。起動時のファイルが未作成・不正な構造のJSON・現行版の3通りで版2の原本と理由を確認した | `app/src/keymap/store.rs:289` | `app/tests/keymap.rs:339` |
| 6 | 1件失敗。点の材質番号を差し替えた結果が照合を通った | 三角形以外は元の材質番号を要求する。同じ材質を使う三角形・点・線で、点と線を個別に差し替えて照合が拒むことを確かめた | `gltf/src/export/verify.rs:418` | `gltf/tests/recolor.rs:582` |
| 7 | 2件失敗。復元した部品の色が違うため再書き出しが失敗した | シーンから外れた元ノードも読み、描画とプリミティブの対応を復元する。削除を書き出して取り消し、着色して再出力した。共有先の部品の色と、単独メッシュの各描画の色も検査した | `gltf/src/build.rs:97`、`gltf/src/export/recolor.rs:48` | `app/tests/color.rs:192`、`gltf/tests/recolor.rs:549` |
| 8 | 1件失敗。保存後の再読込で既定値に戻った | 未知の項目を含む出力を改行まで数える。空白を詰めて収まれば再読込でき、超える場合は理由を返して元のバイト列を保つ | `app/src/keymap/store.rs:485` | `app/tests/keymap.rs:364` |
| 9 | 2件失敗。極端な倍率で直前の値を確定するか、入力が終了した | 確定時に行列を再計算する。差分や適用先の行列が無効なら履歴に積まず、値を直して確定できる | `app/src/session/edit.rs:268`、`app/src/session/keymove.rs:228` | `app/tests/keymove.rs:324`、同339 |
| 10 | 1件失敗。`meshes` の欠落で範囲外参照が起きた | メッシュの番号を参照前に検査する。図形のメッシュ・材質・アクセサ・バッファビュー・バッファの5配列を個別に除いた結果は、すべて照合エラーになった | `gltf/src/export/verify.rs:319` | `gltf/tests/empty_base.rs:211` |
| 11 | 1件失敗。Control＋Command＋Zが「取り消し」になった | macOSのControlはCommandとの同時押しでも拒む。Command＋Z単独と他OSのCtrl＋Zは使える | `app/src/keymap/chord.rs:132` | `app/src/keymap/chord.rs:365` |

## 変更したファイル

| ディレクトリ | ファイル |
|---|---|
| `crates/app/src/` | `app.rs`、`ui/mod.rs`、`keymap/chord.rs`、`keymap/store.rs` |
| `crates/app/src/session/` | `edit.rs`、`keymove.rs`、`keys.rs`、`mod.rs`、`notes.rs`、`open.rs` |
| `crates/app/tests/` | `color.rs`、`color_panel.rs`、`keymap.rs`、`keymove.rs`、`keys_flow.rs`、`select.rs` |
| `crates/core/src/` | `review_file.rs` |
| `crates/gltf/src/` | `build.rs`、`export/base.rs`、`export/patch.rs`、`export/recolor.rs`、`export/synth.rs`、`export/verify.rs` |
| `crates/gltf/tests/` | `empty_base.rs`、`recolor.rs` |
| `.kiro/specs/shortcuts-guide/` | `requirements.md`、`design-settings.md`、`design-keymap.md`、`design-keymove.md`、`design-decisions.md`、`design-testing.md`、`ux.md`、本記録 |
| `.kiro/specs/creation-basics/` | `design-export.md` |

設定の要件6.1・6.2・6.5に、元の名前の保護、置換直前の版確認、保存時の容量上限を明記した。設計にも、入力確定フレームのキー、復元時の材質照合を反映した。 [設定の要件](requirements.md#6-壊れた知らない内容)・[キーの振り分け](design-keymap.md)・[変形の確定](design-keymove.md)・[書き出しの照合](../creation-basics/design-export.md)

## 検査結果と未確認の範囲

macOSで整形検査に合格し、ワークスペースの試験は662件成功、失敗0件、既定の除外54件だった。全ターゲットの静的検査は警告0件。リリースビルドも成功した。

アプリの起動、実キー、GPU、Windows・Linuxでの実行は未確認。設定の競合試験は、起動後に保存先を更新してから保存する状況を再現した。最終確認とOSの置換処理との間に別プロセスが更新する場合は未確認。
