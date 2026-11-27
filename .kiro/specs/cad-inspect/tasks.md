# Implementation Plan

作業は「作業パッケージ（WP）」に分ける。この文書と、各WPが挙げる設計の文書だけを渡された実装者が進められるように書いた。接合（WP-J1・J2）と最後の確認（WP-F1）は [tasks-2.md](tasks-2.md) に続く。

## 進め方の約束
- **着手前に読むもの**: [design.md](design.md)、[design-decisions.md](design-decisions.md)（落とし穴14件と未確認）、[design-testing.md](design-testing.md) の「3つのOSで同じ結果にするための決まり」、第1段階の [落とし穴13件](../model-reviewer/design-decisions.md)、自分のWPに書かれた文書。
- **全WP共通の合格条件**: `cargo fmt --all --check`・`cargo test --workspace`・`cargo clippy --all-targets --workspace -- -D warnings`・`cargo build --release` が通る。既存の試験を消したり弱めたりしない。commit・push はしない。外部の依存を足さない（要るなら作業を止めて進行役に伝える）。
- **担当ファイルの外を変えない。** 他のWPのファイルや、境界の型（下の WP-C0 が置く型）を変える必要が出たら、作業を止めて進行役に伝える。
- **共有の大きなファイル**（`command.rs`・`session/mod.rs`・`app.rs`・`ui/mod.rs`・`ui/ids.rs`・`gpu_view.rs`・`loader.rs`・`exporter.rs`・`automation*`・`metrics.rs`）に触れるのは、WP-C0 と接合（WP-J1・J2）だけ。接合は1本ずつ順に行う。
- GPUと画面が要る確認（`--include-ignored` の試験、実機の自動操作、画像の目視、性能）は進行役が行う。実装者は GPU の試験を書くが、実行していないものは「未確認」と報告する。API は推測で書かず、依存の実物（cargo のレジストリの下）で確かめる。
- 換算・角度・投影を通った値を `assert_eq!` で比べない。OS 固有のパスを書かない。
- 文書とコードに、個人名・メール・ホームの絶対パス・作業に使った道具の名前を書かない。画面の言葉は日本語にする。

## 並行の組と依存
```
WP-C0（単独・最初）
  ├─(P) K1 コアの幾何（吸着・切った当たり・断面の計算）─┐
  ├─(P) U1 寸法とレビューの項目                        ├─▶ J1 単位・吸着・断面 ─▶ J2 STL・寸法 ─▶ F1 確認
  ├─(P) L1 STL の読み込みと土台の合成                  │        区切りA              区切りB
  ├─(P) R1 描画：断面                                  │
  └─(P) V1 画面の部品 ────────────────────────────────┘
```
`(P)` の5本は担当ファイルが重ならず、C0 の後なら同時に進められる。J1 は K1・R1・V1 の後、J2 は J1・U1・L1 の後。

## 区切り（それぞれ単独で出せる）
| 区切り | 終わったWP | 利用者にできるようになること |
|---|---|---|
| A 測る・切る | J1 | 表示の単位、頂点と辺に吸着する計測、軸ごとの差、断面 |
| B STL と寸法 | J2、F1 | STL を開く・測る・注釈する・GLB として書き出す。寸法を指定した図形と、外形の表示 |
A の中の順は、単位 → 吸着 → 断面。B の中の順は、STL → 寸法（依頼の順のとおり）。

- [ ] 0. 土台
- [ ] 0.1 WP-C0 契約の先置き（単独で最初に行う。**振る舞いを変えない**）
  - 型と関数の宣言を置く。**置くだけの関数は、パニックさせず、`None`・空・既定値を返す**（並行のWPの試験が落ちないようにする）
  - `core`: `snap.rs`・`section.rs` を新規に作り `lib.rs` に足す（型は design-snap.md・design-section.md）。`TriHit.triangle`、`Hit.prim`・`Hit.triangle`、`pick_clipped`（中身は `clip` を無視して既存の `pick` と同じ）、`PickIndex::bvh`、`Bvh::triangles_near`（空で `true`）、`Measurement.kinds`・`push_with`・`delta`、`ShapeSpec`・`Dims`・`dims_of`・`scale_for`、`Document::add_shape_primitive`・`shape_spec`、`ModelRef.stl_unit`（既定 `None`。読み書きは U1）、`LoadWarning::{StlSolidsMerged, StlTrailingBytes}` と日本語の文
  - **C0 が中身まで書くもの**（複数のWPが使う小さな関数）: `UnitLabel::meters`、`Units` の全部と `format_length`（design-stl-units.md）、`SectionPlane::{centered, normal, cuts, clamped, code}`
  - `crates/stl`: クレートを作り、`Cargo.toml` の `members` に足す（`license`・`edition`・`rust-version`・`version` はワークスペースから継ぐ）。`detect` は `None`、`load_stl` は `Err(StlError::NotStl)`。`gltf::export::synthesize_base` は `Err(ExportError::NotImplemented)`
  - `render`: `FrameParams.section: Option<SectionPlane>`（既定 `None`。まだ使わない）。`app`: `SceneDirty.section`、`Command` の7つ（design-shapes.md。`Session::apply` の分岐は空の関数を呼ぶだけ）、`session/{snap,section,dims}.rs`（`DimsView`・`ShapeDims` の宣言を含む）、`ui/{section_bar,section_view,dims,measure_panel}.rs`（入力用の構造体と、何も描かない `show`）、`ui/ids.rs` の名前、`Session` の `units`・`source_kind`・`snap`・`snap_preview`・`section`・`section_memory`、`ExportBase`、`LoadOptions`
  - 項目を足した構造体を直接組み立てている箇所（試験を含む）を、すべて直す
  - 合格: 既存の試験がすべて通り、件数が減っていない。`cargo deny check licenses sources bans` が通る（進行役）。`smoke`・`flow-1`・`measure-1`・`edit-1` が通り、画像が変わらない（進行役）
  - _Boundary: 上に挙げたファイル、ルートの `Cargo.toml`・`Cargo.lock`、各クレートの既存の試験（構造体の組み立ての修正だけ）_ ／ _Requirements: 13.1, 13.3_

- [ ] 1. 並行して進める部品（C0 の後）
- [ ] 1.1 (P) WP-K1 コアの幾何（design-snap.md、design-section.md の「型の約束」「当たり判定」）
  - `Bvh::triangles_near`、`closest` が三角形の番号を返す、`pick_clipped`、`SectionPlane::{ray_range, frame_corners}`、`drag_offset`、`snap`（手順1〜8と辺の判定）、`Measurement` の種類と差
  - 合格: design-testing.md の「近くの三角形」「切った当たり判定」「断面の計算」「吸着」「計測」の行。試験名の目安: `near_matches_brute_force`、`clipped_ray_hits_the_far_wall`、`clip_none_equals_pick`、`snaps_to_corner_edge_and_face`、`flat_diagonal_is_not_an_edge`、`open_border_is_an_edge`、`snaps_from_just_outside_the_silhouette`、`cut_vertices_are_not_candidates`、`snap_local_equals_vertex_under_scale_and_rotation`
  - _Boundary: `core/src/{bvh,pick,section,snap,measure}.rs`、`core/tests/{pick,snap}.rs`_ ／ _Requirements: 1.1, 1.3, 1.4, 1.6, 1.7, 2.1, 2.2, 4.1, 4.2, 11.1_
- [ ] 1.2 (P) WP-U1 寸法とレビューの項目（design-shapes.md の「型の約束」、design-stl-units.md の「レビューJSON」）
  - `dims_of`・`scale_for`、`Document` の形状の記録（複製で読める。文書を作り直すと消える）、レビューJSONの `model.stl_unit` の読み書き
  - 合格: design-testing.md の「寸法」「レビュー」の行。試験名の目安: `dims_round_trip_per_kind`、`uneven_sphere_has_no_dims`、`out_of_range_dims_are_rejected`、`shape_spec_survives_duplicate`、`stl_unit_round_trips`、`glb_review_bytes_are_unchanged`
  - _Boundary: `core/src/{shapes,doc,review_file,identity}.rs`、`core/tests/{doc,review_roundtrip}.rs`_ ／ _Requirements: 8.4, 8.5, 9.2, 9.5, 9.6, 9.7_
- [ ] 1.3 (P) WP-L1 STL の読み込みと土台の合成（design-stl-units.md の「STL の入口」「書き出し」）
  - `crates/stl`: `detect`、バイナリと ASCII の読み取り、数の検査、頂点をまとめる処理、`Model` の組み立て、`StlError` の日本語の文。`testdata`: `stl.rs`（箱の見本、重い STL、壊れた STL の生成器）と、`main.rs` の `stl-sample`・`stl-stress`。`assets/samples/review-sample.stl` を生成して置く
  - `gltf::export::synthesize_base`（`synth.rs`）。`gltf` は `stl` に依存させない。試験の入力は、手で組んだ「部品1つ・形状1つ」の `Model` を使う
  - 合格: design-testing.md の「STL の読み込み」「壊れた STL」「土台の合成」の行。試験名の目安: `binary_and_ascii_give_the_same_model`、`binary_starting_with_solid_is_binary`、`claimed_count_never_drives_allocation`、`non_finite_and_huge_coordinates_are_rejected`、`truncated_ascii_is_an_error`、`random_bytes_never_panic`、`unit_becomes_node_scale`、`synth_base_round_trips_through_load_glb`、`synth_base_is_deterministic`、`export_on_synth_base_verifies`
  - _Boundary: `crates/stl/`、`testdata/src/{stl,main,lib}.rs`、`assets/samples/review-sample.stl`、`gltf/src/export/{synth,mod}.rs`、`gltf/tests/export.rs`_ ／ _Requirements: 5.1, 5.3, 5.4, 5.5, 6.1, 6.2, 6.3, 6.5, 8.1, 8.2_
- [ ] 1.4 (P) WP-R1 描画：断面（design-section.md の「描画」）
  - 定数の `params.z`・`params.w`、`section_cut`、`scene.wgsl`・`mask.wgsl` の `discard`、切り口の色、`FLAG_MIRRORED`（`instances::build` と部分更新の両方）、`SceneRenderer` が `section` の変化で描き直すこと
  - 合格: design-testing.md の「描画」の行（GPUの試験を書く。実行は進行役）。GPUなしで通る試験として、`params` の詰め方と、行列式が負のときの印を確かめる。`FrameUniform` の大きさの検査（304バイト）を変えない
  - _Boundary: `render/src/{plan,lib,instances,pipelines}.rs`、`render/src/shaders/{frame,scene,mask}.wgsl`、`render/tests/`_ ／ _Requirements: 3.1, 3.4, 3.7, 3.9, 11.3_
- [ ] 1.5 (P) WP-V1 画面の部品（design-shapes.md の「画面の部品」、design-section.md の「画面の部品」、ux.md）
  - `section_bar`（軸・位置・反転）、`section_view`（枠と矢印。当たりは第2段階の `gizmo_view` と同じやり方）、`dims`（外形と寸法の欄）、`measure_panel`（距離・差・種類）、`measure_view` に候補の印と薄い描き方、`toolrail` に [吸着]、`viewbar` に [断面]
  - 入力用の構造体だけを受け取る形で書く。`Session` を読まない。コアの関数がまだ置いただけでも通る試験にする（押すと積まれる `Command`、無効のときの理由、文言）
  - 合格: design-testing.md の「画面の部品」の行（`layout.rs` の幅960は J1）。試験名の目安: `section_bar_emits_set_section`、`axis_change_recenters`、`dims_commit_emits_commit_local`、`invalid_dim_restores_and_explains`、`loaded_part_shows_extent_only`、`snap_marker_names_the_kind`、`snap_button_only_in_measure_tool`
  - _Boundary: `app/src/ui/{section_bar,section_view,dims,measure_panel,measure_view,toolrail,viewbar}.rs`、`app/tests/{section_bar,section_view,dims,measure_panel,measure_view,toolrail,viewbar}.rs`_ ／ _Requirements: 1.2, 2.2, 2.3, 3.2, 3.6, 9.2, 9.4, 9.5, 10.1, 10.2, 12.1, 12.4_
