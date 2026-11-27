# Implementation Plan

作業は「作業パッケージ（WP）」に分ける。この文書だけを渡された実装者が進められるように書いた。接合（WP-J*）と検証は [tasks-2.md](tasks-2.md) に続く。

## 進め方の約束
- **着手前に読むもの**: [design.md](design.md)、[design-decisions.md](design-decisions.md)（確かめたAPIと未確認）、第1段階の [落とし穴13件](../model-reviewer/design-decisions.md)、自分のWPに書かれた文書。
- **全WP共通の合格条件**: `cargo fmt --all --check`・`cargo test --workspace`・`cargo clippy --all-targets --workspace -- -D warnings`・`cargo build --release` が通る。既存の試験を消したり弱めたりしない。commit・push はしない。新しい依存を足さない（要るなら作業を止めて進行役に伝える）。
- **担当ファイルの外を変えない。** 他のWPのファイルや、境界の型（`SceneRead`・`Command`・各WPの公開の型）を変える必要が出たら、作業を止めて進行役に伝える。
- **共有の大きなファイル**（`command.rs`・`session/mod.rs`・`app.rs`・`ui/mod.rs`・`ui/ids.rs`・`gpu_view.rs`・`automation*`・`metrics.rs`）に触れるのは、WP-E0 と接合（WP-J1〜J6）だけ。接合は1本ずつ順に行うので、同時に2人が触ることは無い。
- GPUと画面が要る確認（`--include-ignored` の試験、実機の自動操作）は進行役が行う。実装者は、確かめていない項目を「未確認」と報告する。APIは推測で書かず、依存の実物で確かめる。
- 文書とコードに、個人名・メール・ホームの絶対パス・作業に使った道具の名前を書かない。

## 並行の組と依存
```
WP-E0（単独・最初）
  ├─(P) A1 カメラ ─┐
  ├─(P) A2 描画：格子と表示 ──▶ B4 描画：部分更新と足した形状
  ├─(P) A3 立方体と上端のボタン ─┤
  ├─(P) B1 文書と履歴            ├─▶ J1 見る ─▶ J2 載せ替え ─▶ J3 動かす・戻す ─▶ J4 書き出す ─▶ J5 足す・消す ─▶ J6 計測 ─▶ V1
  ├─(P) B2 ハンドルの計算        │      区切り1                                    区切り2         区切り3        区切り4
  ├─(P) B3 書き出し              │
  ├─(P) B5 道具・数値入力・ハンドルの画面
  ├─(P) C1 図形   ├─(P) D1 計測と単位   └─(P) T1 重いモデルの階層
```
`(P)` の10本は担当ファイルが重ならず、E0 の後なら同時に進められる。B4 だけは A2 と同じファイル（`render/src/lib.rs`）を触るので、A2 の後に同じ担当が行う。

## 区切り（それぞれ単独で出せる）
| 区切り | 終わったWP | 利用者にできるようになること |
|---|---|---|
| 1 見る | J1 | 格子と軸、立方体、平行投影、表示の切り替え。編集はまだ無い |
| 2 動かして渡す | J4 | 移動・回転・拡大、数値入力、取り消し、GLBの書き出し、2つの未保存の印 |
| 3 足す・消す | J5 | 図形の追加、複製、削除 |
| 4 測る | J6、V1 | 距離の計測と単位の表示。性能の測定と文書 |
J2・J3 の後では出さない。書き出しが無いまま編集だけを出すと、利用者が編集を失う。

- [x] 0. 土台
- [x] 0.1 WP-E0 契約の先置き（単独で最初に行う。**振る舞いを変えない**）
  - `SceneRead` を `core::scene` に置き、`Model` に実装する。`pick`・`Visibility`・`ReviewState::new`・`review_file`・`render::instances::build`・`SceneRenderer::set_instances*` の引数を `&Model` から `&impl SceneRead` に替える（design-data.md）
  - `Node` に `local` を足し、読み込みがファイルの値から作った行列を入れる。`Node` を直接組み立てている試験を直す
  - 新しいファイルを、型の宣言だけ入れて作る（中身は各WPが書く）: `core/src/{doc,history,gizmo,shapes,measure,units}.rs`、`gltf/src/export/mod.rs`、`render/src/{grid,display}.rs`、`app/src/exporter.rs`、`app/src/session/{view,edit,export,measure}.rs`、`app/src/ui/{viewcube,viewbar,toolrail,inspector,gizmo_view,measure_view}.rs`、`testdata/src/rich.rs`。各 `lib.rs`・`mod.rs` に宣言を足す
  - 複数のファイルにまたがる項目を、すべてここで足す: `OrbitCamera.projection`（既定は透視）、`FrameParams` の `display`・`grid`・`drag`（既定は第1段階と同じ絵になる値）、`SceneDirty` の `display`・`drag`、`Command` の追加分と `Tool` などの型（design-ui.md）、`ui/ids.rs` の名前。`Session::apply` の新しい分岐は、`session/{view,edit,export,measure}.rs` の空の関数を呼ぶだけにする（画面からはまだ届かない）
  - 合格: 既存の試験がすべて通る（件数が減っていない）。`smoke`・`flow-1`・`flow-2` が通り、画像が第1段階と変わらない（進行役）
  - _Boundary: 上に挙げたファイル、`core/src/{scene,lib,pick,visibility,review,review_file,camera}.rs` の署名、`gltf/src/{build,lib}.rs`、`render/src/{lib,instances}.rs` の署名、`app/src/{command,lib}.rs`、`session/mod.rs`、`ui/{mod,ids}.rs`、各クレートの既存の試験_
  - _Requirements: 16.1_

- [x] 1. 並行して進める部品（E0 の後）
- [x] 1.1 (P) WP-A1 カメラ：投影・仰角90°・決まった向き・格子の間隔（design-render.md）
  - 合格: design-testing.md の「カメラ」の行。版2の読み書き（平行投影があれば版2、無ければ版1。版1のファイルは今までどおり読める）
  - _Boundary: `core/src/{camera,review_file}.rs`、`core/tests/{camera,review_roundtrip}.rs`_ ／ _Requirements: 2.2, 2.4, 3.1, 3.2, 3.3, 12.4_
- [x] 1.2 (P) WP-A2 描画：格子と軸、表示の切り替え、変形中の行列（design-render.md）
  - 最初に `frag_depth` と多重サンプルの組み合わせを小さな試験で確かめ、結果を報告する。`supports_lines()` を実装する
  - 合格: design-testing.md の「描画」の行のうち、格子・線・変形中の行列・何もしないフレーム。既存のGPU試験が通る（進行役）
  - _Boundary: `render/src/{grid,display,pipelines,lib}.rs`、`render/src/shaders/`、`render/tests/`_ ／ _Requirements: 1.1, 1.4, 4.1, 4.3, 6.6, 15.1, 15.2_
- [x] 1.3 (P) WP-A3 向きの立方体と上端のボタン（design-ui.md）
  - 合格: 面の見え方と当たり判定の単体テスト。押すと `SnapView`・`SetDisplay`・`SetProjection`・`SetGrid` が積まれる。線が使えないときに理由が出る。部品の名前が登録される
  - _Boundary: `app/src/ui/{viewcube,viewbar}.rs`、`app/tests/{viewcube,viewbar}.rs`_ ／ _Requirements: 2.1, 2.2, 2.5, 4.1, 4.2_
- [x] 1.4 (P) WP-B1 文書と履歴（design-data.md）
  - `Document`（作成、`set_local`、範囲の挿入と削除、部分木の複製、形状の追加、鍵とファイル内の番号の表、`SceneRead` の実装）、`Remap`、`History` と `Edit`（レビューの操作を含む）、`Visibility` の継ぎ足しと切り取り、`ReviewState::apply_remap` と番号つきの復元
  - 合格: design-testing.md の「文書」「履歴」の行
  - _Boundary: `core/src/{doc,history,visibility,review}.rs`、`core/tests/{doc,history,visibility}.rs`_ ／ _Requirements: 8.1, 8.4, 8.5, 9.3, 9.4, 9.5, 16.1_
- [x] 1.5 (P) WP-B2 ハンドルの計算（design-ui.md の「ハンドル」）
  - 合格: design-testing.md の「ハンドル」の行
  - _Boundary: `core/src/gizmo.rs`、`core/tests/gizmo.rs`_ ／ _Requirements: 6.1, 6.2, 6.7, 7.1, 7.4_
- [x] 1.6 (P) WP-B3 書き出し（design-export.md）
  - `build`・`write_container`・`verify`。試験用に、テクスチャ・アニメーション・スキン・知らない任意の拡張・`extras` を構造として含むGLBの生成器を `testdata/src/rich.rs` に書く
  - 合格: design-testing.md の「書き出しの試験」1〜8のうち、ファイルを扱わない分（8の「保存先が同じ」は J4）
  - _Boundary: `gltf/src/export/`、`gltf/tests/export.rs`、`testdata/src/rich.rs`_ ／ _Requirements: 11.2, 11.3, 11.6, 11.7, 16.1_
- [x] 1.7 WP-B4 描画：範囲だけの書き直しと、足した形状の転送（A2 の後。design-render.md）
  - 合格: 部分更新の後の画像が全体を作り直した画像と一致する。送ったバイト数が動かした範囲に比例する。足した図形が出る。モデルを閉じると足した分も解放される（進行役）
  - _Boundary: `render/src/{instances,upload,plan,lib}.rs`、`render/tests/`_ ／ _Depends: 1.2_ ／ _Requirements: 6.6, 9.8, 15.2_
- [x] 1.8 (P) WP-B5 道具の列・数値入力・ハンドルの画面（design-ui.md）
  - 入力用の構造体だけを受け取る形で書く。ハンドルの当たりは WP-B2 の関数を呼ぶ（まだ無ければ、E0 が置いた宣言に合わせる）
  - 合格: design-testing.md の「画面の部品」の行。数値の欄は、ドラッグ中は `PreviewLocal`、離すと `CommitLocal`、Esc で `CancelTransform`
  - _Boundary: `app/src/ui/{toolrail,inspector,gizmo_view}.rs`、`app/tests/{toolrail,inspector,gizmo_view}.rs`_ ／ _Requirements: 5.1, 6.1, 6.2, 7.1, 7.2, 7.3, 7.4, 7.5, 9.7_
- [x] 1.9 (P) WP-C1 図形（design-ui.md の「図形の追加」）
  - 合格: design-testing.md の「図形」の行
  - _Boundary: `core/src/shapes.rs`_ ／ _Requirements: 9.1, 9.2_
- [x] 1.10 (P) WP-D1 計測と単位（design-ui.md の「計測と単位」）
  - 合格: 2点の距離、部品を動かした後の距離、単位を替えても数値が変わらないこと。線と文字を描く部品が、入力用の構造体だけで動く
  - _Boundary: `core/src/{measure,units}.rs`、`app/src/ui/measure_view.rs`、`app/tests/measure_view.rs`_ ／ _Requirements: 10.1, 10.2, 10.4, 10.5_
- [x] 1.11 (P) WP-T1 重いモデルに階層を足す
  - `stress` の生成に、根の下へ「子孫を半分持つまとめ役」を置く形を足す（既存の形は残す）。出力は今までどおり `target/` の下だけ
  - 合格: 生成が決定的。部品10万個で、まとめ役の子孫が5万個
  - _Boundary: `testdata/src/{stress,main}.rs`_ ／ _Requirements: 15.4_
