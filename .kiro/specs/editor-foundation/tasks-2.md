# Implementation Plan（接合と検証）

[tasks.md](tasks.md) の続き。約束と並行の組はそちらに書いた。接合（WP-J*）は**1本ずつ順に**行い、共有の大きなファイルに触れてよい。並行のWPが作った部品を `Session`・画面・GPU・自動操作へつなぐのが仕事で、部品の中身を書き換える必要が出たら、作業を止めて進行役に伝える。各接合の合格条件のうち「実機」と書いたものは進行役が行う。

- [x] 2. 区切り1：見る
- [x] 2.1 WP-J1 接合：格子・立方体・投影・表示
  - `Session` に表示・格子の状態を持たせ、`session/view.rs` に `SetDisplay`・`SetGrid`・`SetProjection`・`ToggleProjection`・`SnapView` の処理を書く。`gpu_view` が `FrameParams` に渡す。`main.rs` で、アダプタが持っていれば `POLYGON_MODE_LINE` を要求する
  - 画面の配置を入れ替える（ux.md の骨格）: 部品の一覧を右のパネルへ、注釈と視点を左のパネルへ移す。パネルの名前（`ids`）・開閉の記憶・各部品の名前は変えず、開閉の矢印の向きだけ合わせる。右のパネルは上に一覧、下に「変形」の場所を空けられる形にする
  - 3Dビューの上端にボタン、右上に立方体を置く。キー 1・3・7・5・Z・Shift+Z を `app.rs` に足す。ヘルプと、何も開いていない画面の操作表を更新する。状態欄に格子の間隔を出す。`dump_state` に投影・表示・格子を足す
  - 合格（試験）: `app/tests/view.rs` で、`Command` の列により視点・投影・表示が変わる。何もしないフレームで描き直しの印が立たない。入力欄に文字を打っている間は 1・3・7・5・Z が働かない。幅960で上部と状態欄が重ならない（`layout.rs`）
  - 合格（実機）: `view-1.json` が通る。画像で、格子が部品に隠れること、4つの表示、平行投影、真上の視点、立方体の文字、足した文言と句読点の字形を確かめる。既存の `smoke`・`flow-1`・`flow-2` が通る
  - _Boundary: `app/src/{app,gpu_view,main}.rs`、`session/{mod,view,camera}.rs`、`ui/{mod,viewport,help,status,empty,tree,annotations,views}.rs`、`automation/state.rs`、`app/e2e/view-1.json`、`app/tests/{view,layout,ui_shots,tree}.rs`_
  - _Depends: 1.1, 1.2, 1.3_ ／ _Requirements: 1.2, 1.3, 2.3, 3.1, 4.2, 4.4, 5.4, 14.2, 14.3, 14.4, 15.1_

- [x] 3. 区切り2：動かして渡す
- [x] 3.1 WP-J2 接合：文書への載せ替え（**利用者から見た振る舞いを変えない**）
  - モデルの表示が始まるときに `Document` を作って `Session` に持たせ、部品を読む箇所（選択、表示の切り替え、当たり判定、注釈、レビューの保存と復元、部品の一覧、状態欄、詳細、インスタンスの作成、`dump_state`）をすべて文書へ向ける。部品一覧の開閉の記憶を `NodeKey` に替える
  - 合格（試験）: 既存の試験がすべて通る。`crates/app/src` と `crates/render/src` に、`Model` の `nodes` を直接読む箇所が残っていない（文書を作る所と読み込みの統計を除く。`grep` の結果を報告する）
  - 合格（実機）: `smoke`・`flow-1`・`flow-2`・`view-1` が通る。重いモデルを開いたときの常駐メモリを、載せ替えの前後で記録する
  - _Boundary: `app/src/{gpu_view,loader}.rs`、`session/` の全ファイル、`ui/{tree,overlay,annotations,views,status,details}.rs`、`automation/state.rs`、`app/tests/`_
  - _Depends: 2.1, 1.4_ ／ _Requirements: 16.1_
- [x] 3.2 WP-J3 接合：道具・変形・数値入力・取り消し
  - `session/edit.rs` に、変形の開始・途中・確定・取り消し、数値の確定、`Undo`・`Redo` を書く。履歴に積むのは確定のときだけ。第1段階の操作（表示の切り替え、注釈と視点の追加・編集・削除）も履歴へ積むように直す。`Applied.remaps` で選択・下書き・選択中の注釈を付け替える
  - 道具の列を3Dビューの左端に、「変形」を右のパネルの部品一覧の下に置く。ハンドルを重ねる（入力の振り分けは design-ui.md の順）。上部に「戻す」「やり直す」。キー V・G・R・S、Cmd+Z、Shift+Cmd+Z、Esc の順番（要件5.5）。変形中は注釈の印にも差分の行列を掛ける
  - `gpu_view` が、変形中は `DragPreview` を渡し、確定したら動かした範囲だけを書き直す。性能値に「編集の確定」と「履歴の使用量・件数」を足す。自動操作に `drag_widget` を足し、`dump_state` に道具・各部品の `local`・履歴の件数を足す
  - 合格（試験）: `app/tests/{edit,undo}.rs` で、変形→取り消し→やり直しの後の状態が期待どおり。Esc でドラッグを取り消すと履歴が増えない。読み込み中は編集が断られる。注釈の追加と部品の非表示が取り消せる。入力欄に文字を打っている間は道具のキーと Cmd+Z が働かない。変形の結果が有限でなければ確定しない
  - 合格（実機）: ハンドルのドラッグで部品と輪郭と注釈が一緒に動く。前面に出ていないウィンドウで、数値の欄へ打って Enter で確定する。`gpu_flow.rs` に「何も開いていない状態から32MiB超のモデルを開き、転送の直後に変形と取り消しを行う」試験を足して通す
  - _Boundary: `app/src/{app,gpu_view,metrics}.rs`、`session/{mod,edit,parts,notes,pick}.rs`、`ui/{mod,viewport,overlay,toolbar,status,details,help,tree}.rs`、`automation*`、`app/tests/{edit,undo,gpu_flow,layout}.rs`_
  - _Depends: 3.1, 1.5, 1.7, 1.8_ ／ _Requirements: 5.1〜5.6, 6.1〜6.7, 7.1〜7.5, 8.1〜8.8, 15.3_
- [x] 3.3 WP-J4 接合：書き出しと未保存の扱い
  - `exporter.rs`（別スレッド。読み直し、ハッシュの照合、`build`、一時ファイルへの書き込み、`verify`、名前の付け替え、工程の通知）。`session/export.rs` に、文書から `ExportScene` を作る処理、終わった後の作業対象の切り替え（design-export.md の「書き出した後」）、失敗の扱いを書く
  - 「開く」で開いたファイルを覚え、書き出し先にできないようにする。未保存の印を2つに分け、確認のダイアログで失うものを挙げる。書き出していない編集があるときの「レビュー保存」は、書き出しを選ばせる。上部に「書き出し」、結果のダイアログ、状態欄の2つの印。性能値に工程ごとの時間。自動操作に `export_glb`
  - 合格（試験）: `app/tests/export.rs` で、書き出し→作業対象が新しいファイル→レビュー保存→開き直して一致。保存先が開いたファイルと同じ（別名のリンク経由を含む）なら断る。元のファイルが変わっていたら断る。失敗しても文書・履歴・レビューが残る。取り消しで書き出した時点へ戻ると印が消える。確認の後の操作が失敗したら未保存のまま
  - 合格（実機）: `edit-1.json` と `edit-2.json` が通る。元のGLBのハッシュが変わっていない。外部で作ったGLB（テクスチャつき・アニメーションつき）を動かして書き出し、開き直せる
  - _Boundary: `app/src/{app,exporter,metrics}.rs`、`session/{mod,export,persist,open}.rs`、`ui/{toolbar,status,dialogs,details}.rs`、`automation*`、`app/e2e/edit-{1,2}.json`、`app/tests/{export,persist}.rs`_
  - _Depends: 3.2, 1.6_ ／ _Requirements: 11.1〜11.10, 12.1〜12.5, 13.1〜13.4_

- [x] 4. 区切り3：足す・消す
- [x] 4.1 WP-J5 接合：図形の追加・複製・削除
  - `session/edit.rs` に `AddShape`・`DuplicateSelected`・`DeleteSelected` を書く（文書の操作、履歴、付け替え、足した形状の当たり判定の索引）。`gpu_view` が足した形状を転送し、インスタンスを作り直す。図形のボタンを道具の横に出す。キー Cmd+D・Delete・Backspace。削除の結果を状態欄に出す
  - 合格（試験）: 追加→複製→削除→取り消しを繰り返した後、選択・注釈・表示の設定・部品の一覧が正しい部品を指す。下書きのある部品は削除されない。モデルが無いときは図形を足せない
  - 合格（実機）: `edit-3.json` が通る。足した図形が選択でき、動かせ、書き出して開き直すと同じ場所にある。テクスチャつきの部品を複製して書き出したGLBが、開き直せる
  - _Boundary: `app/src/{app,gpu_view}.rs`、`session/{mod,edit,export}.rs`、`ui/{viewport,tree,status,help}.rs`、`automation/state.rs`、`app/e2e/edit-3.json`、`app/tests/{edit,undo,tree}.rs`_
  - _Depends: 3.3, 1.9_ ／ _Requirements: 9.1〜9.8, 11.7_

- [x] 5. 区切り4：測る
- [x] 5.1 WP-J6 接合：計測と単位
  - `session/measure.rs`、クリックの振り分け（計測の道具のとき）、線と数値の表示、状態欄の距離と「1単位 = m」の切り替え、キー M、ヘルプ
  - 合格（試験）: 2点で距離が出る。部品を動かすと変わる。部品を削除すると消える。未保存の印が付かない。取り消しの履歴が増えない
  - 合格（実機）: `measure-1.json` が通る。平行投影でも同じ距離になる
  - _Boundary: `app/src/app.rs`、`session/{mod,measure,pick}.rs`、`ui/{viewport,status,help}.rs`、`automation/state.rs`、`app/e2e/measure-1.json`、`app/tests/measure.rs`_
  - _Depends: 4.1, 1.10_ ／ _Requirements: 10.1〜10.5, 14.1_
- [x] 5.2 WP-V1 検証・測定・文書
  - `heavy-edit.json` を書き、design-testing.md の「性能」の表を、生成した重いモデル（WP-T1）で測る。結果を、モデル・機械・条件とともに `verification.md`（この仕様のフォルダに新しく作る）へ書く。届かなかった目標は、値と対応を書く
  - design-testing.md の「罠と試験の対応」の6行を、1行ずつ実行した結果つきで記録する。design-decisions.md の「未確認」を、確かめた結果か、未確認のままかで更新する
  - 利用者向けの文書を直す: README（できること、操作表、制限）、`docs/glb.md`（書き出しの対応範囲と、削除した部品のデータが残ること）、`docs/review-json.md`（版2）、`docs/verify.md`（足した手順と性能値）。宣伝の言葉と、測っていない比較を書かない
  - 合格: 4つの検証コマンドと `--include-ignored` が通る。実機の自動操作が全部通る。記録に、実行していない確認を成功として書いていない
  - _Boundary: `app/e2e/heavy-edit.json`、`.kiro/specs/editor-foundation/{verification.md,design-decisions.md,tasks*.md}`、`README.md`、`docs/`_
  - _Depends: 5.1, 1.11_ ／ _Requirements: 15.3, 15.4, 16.2, 16.3, 16.4_

## 接合で間違えやすい所
- 変形中は文書を書き換えない。書き換えるのは確定のときだけで、そのとき初めて履歴に積む。数値の欄のドラッグも同じ。
- 構造が変わった直後のフレームで、古い `NodeId` を持ったままの `Command`（同じフレームに積まれた選択など）が届くことがある。範囲の外の番号は無視する。
- `request_repaint` を無条件に呼ばない。書き出しのスレッドは、工程が進んだときにだけ画面を起こす（読み込みと同じ）。
- 出たばかりのダイアログを自動操作で押すときは、既存の「位置が2回そろうまで待つ」を通す。自前の待ちを足さない。
- 入力位置の判定に `Response::has_focus` を使わない。`ctx.text_edit_focused()` と egui のメモリを使う。
- 画面の文言は、「保存」をレビュー、「書き出し」をGLBの意味でだけ使う。
