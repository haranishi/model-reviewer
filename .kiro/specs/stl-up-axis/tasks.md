# Implementation Plan

作業パッケージは1本にまとめる。この文書と [requirements.md](requirements.md)・[ux.md](ux.md)・[design.md](design.md) だけを渡された実装者が進められるように書いた。

## 進め方の約束
- **着手前に読むもの**: 上の3つと、第3段階の [design-stl-units.md](../cad-inspect/design-stl-units.md)、[design-testing.md](../cad-inspect/design-testing.md) の「3つのOSで同じ結果にするための決まり」。
- **合格条件（共通）**: `cargo fmt --all --check`・`cargo test --workspace`・`cargo clippy --all-targets --workspace -- -D warnings`・`cargo build --release` が通る。既存の試験を消したり弱めたりしない。commit・push はしない。外部の依存を足さない（`Cargo.lock` が変わったら止めて進行役に伝える）。
- **担当ファイルの外を変えない。** 型に項目を足したせいで通らなくなる組み立て（構造体の手書き、`match`）は、足した分だけ直してよい。直したファイルを報告する。
- GPU と画面が要る確認（実機の自動操作、撮影、画像の目視）は進行役が行う。実装者は、実行していないものを「未確認」と報告する。design.md の「要確認」は、確かめた結果を報告に書く。
- 文書とコードに、個人名・メール・ホームの絶対パス・作業に使った道具の名前を書かない。画面の言葉は日本語にする。

- [ ] 1. WP-Z1 STL の上の向き
- [ ] 1.1 型と読み込み（`core`・`stl`）
  - `core::units::UpAxis`（既定は Z。`suffix`・`key`・`other`・`rotation`）。`StlOptions.up`。`assemble_model` が `local` に倍率と回転を入れる。回転は 0 と ±1 を直に並べた行列で作る（design.md）
  - 合格（`stl/tests/parse.rs`・`core` の単体試験）: `z_up_puts_file_z_on_world_y`（見本の箱の外形が 20×40×30）、`y_up_keeps_file_coordinates`（今までと同じ `local`）、`up_rotation_keeps_positions_and_winding`（形状の位置と番号のバイトがどちらの向きでも同じ。行列式が正）、`up_axis_keys_and_other`
  - _Requirements: 1.1, 1.2, 1.3, 1.4_
- [ ] 1.2 レビューJSON（`core`）
  - `ModelRef` と `ReviewModelRecord` に `stl_up`。STL のときだけ書く。無ければ `None`（復元では Y。`saved_stl_up`）。2つ以外は `InvalidValue`
  - 合格: `stl_up_is_written_for_stl_only`（GLB のレビューのバイト列が変わらない）、`stl_up_round_trips`、`unknown_stl_up_is_rejected`、`missing_stl_up_reads_as_none`
  - _Requirements: 3.4, 3.5_
- [ ] 1.3 つなぐ（`app`）
  - `LoadOptions.stl_up`、`SourceKind::Stl { unit, up }`、`Command::SetStlUp`、`ExportBase::Stl { unit, up, name }`。単位と向きの切り替えを1つの関数にまとめ、片方を替えてももう片方が残るようにする
  - 状態欄のボタン（`ids::STATUS_STL_UP = "status.stl_up"`。文は ux.md の表層）。幅の見積もり（`optional_items`）に足す。ヘルプの表1行と制限の文
  - 書き出し: 土台の作り直しに `up` を渡す。結果の一覧の1行（Z のときだけ足す）。レビュー: 保存・復元・照合（単位か向きが違えば開き直す）
  - 自動操作: `open_model` の `stl_up`、`dump_state` の `stl_up`
  - 合格（`app/tests/stl.rs` ほか。試験名の目安）:
    - `stl_opens_z_up_by_default`（外形 20×40×30 mm、回転の欄の X が -90、`source_kind` の向きが Z）
    - `changing_stl_up_reopens_after_confirm`（未保存があれば確認。取り消すと前のモデルと向きが残る）
    - `changing_unit_keeps_up_and_changing_up_keeps_unit`
    - `stl_review_saves_and_restores_with_up`（`"stl_up": "y"` を書く。別の向きで開いていれば開き直す。項目が無ければ Y）
    - `annotation_stays_on_the_same_surface_across_up`（同じ注釈の部品の中の座標が、どちらの向きでも同じ）
    - `z_up_stl_exports_rotation`（合成直後の土台と書き出した GLB の回転・倍率を照合し、外形も比べる）。`export_note_describes_initial_rotation_after_edit`（回転を編集しても、結果は開いたときの仮定を示す）
    - 既存の `scene_numbers_do_not_change_across_stl_export`・`exported_glb_from_stl_reopens_with_same_extent` が、既定（Z が上）のまま通る
    - `gltf/tests`: `synthesized_base_keeps_rotation_and_scale`（`load_glb(synthesize_base(m))` の `local` が相対 1e-6 以内）
    - 自動操作: `open_model_parses_stl_up`（`"z"`・`"y"`・省略・それ以外は誤り）、状態の書き出しの `stl_up`
    - 幅960で STL・未保存の印2つ・簡略表示が同時に出ても、状態欄の項目が重ならない。`exported_stl_status_fits_at_960` では、100文字の名前で GLB を書き出すと、修正前は「詳細」の右端が x=1374 にはみ出した。修正後は画面内に収まった
  - 既存の試験で、既定で開いて座標や外形を比べているものは、期待する値を Z が上の値へ直す（比べる項目は減らさない）。座標の意味を保ちたい試験は `stl_up: Some(UpAxis::Y)` を指定する。どちらにしたかを報告する
  - _Requirements: 1.5, 1.6, 2.1, 2.2, 2.3, 2.4, 2.5, 3.1, 3.2, 3.3, 3.5, 3.6, 4.1, 4.4_
- [ ] 1.4 手順と文書
  - `e2e/stl-1.json` と `heavy-inspect.json` の STL に `"stl_up": "y"`。`guide-05-stl.json` は変えない。`guide-03-measure.json` にも、いまは `"stl_up": "y"` を足して通す（仮の扱い。進行役が2点を取り直して外す）
  - 画面を使わずに `stl-1` の STL 保存と `stl-3` の復元を別のセッションで確認した。レビューに `"stl_up": "y"` が入り、保存直後の状態と一致した。レビューの向きだけを Z に書き換えると比較は失敗した
  - `docs/stl.md`・`docs/review-json.md`・`docs/verify.md`・`docs/roadmap.md`・`docs/guide/05-stl.md`（中身は design.md の「文書の直し」）。どの文書も100行以内
  - _Requirements: 4.2, 4.3_
- _Boundary: `core/src/{units,review_file}.rs` と `ModelRef` のあるファイル、`stl/src/lib.rs`、`stl/tests/`、`gltf/tests/`、`app/src/{loader,exporter,command,automation}.rs`、`app/src/automation/`、`app/src/session/{mod,open,persist,export}.rs`、`app/src/ui/{status,ids,help}.rs`、`app/tests/{stl,layout,ui_shots}.rs` と通らなくなった既存の試験、`app/e2e/{stl-1,stl-3,heavy-inspect,guide-03-measure}.json`、`docs/`_
- _Depends: 第3段階の WP-J2、creation-basics の WP-N2（`SourceKind::New` の分岐がある）_

- [ ] 2. 仕上げ（進行役）
  - 実機の自動操作を全部通す。同じ出力先で `stl-1` の後、別々に起動して `stl-2` で GLB、`stl-3` で Y が上の STL の状態が一致すること、見本の GLB と STL のハッシュが前後で同じことを確かめる
  - `guide-03-measure.json` から `"stl_up": "y"` を外して2点を取り直し（「頂点」の印が出る位置。距離が 20.00 mm）、手順書 03 と 05 の絵を撮り直して目で確かめる（状態欄に「上 = Z（仮定）」、箱が立っている、寸法が 20・40・30）
  - 書き出した GLB を別のソフトで開き、向きと大きさを確かめる。使ったソフトと版を `verification.md` に書く
  - 仕様の記録: `verification.md` を足し、`spec.json` の `phase` を進める。第3段階の `verification.md` の「上の向きを選ぶ機能は未決」に、この仕様への案内を足す

## 壊れやすい所（先に読む）
1. **書き出しの土台に向きを渡し忘れる。** その後に文書の変形を重ねると、土台の部品0だけの誤りは最終の GLB からは見えない。合成時だけ Z と Y を入れ替えると `z_up_stl_exports_rotation` の土台の照合が失敗し、元に戻すと合格した。
2. **単位を替えると向きが既定へ戻る**（その逆も）。開き直しの指定を1か所で作る。
3. **レビューの照合が単位だけを見る。** 向きの違う STL に、その場でレビューを当てはめると、保存した視点がずれる。
4. **回転を角度から作る。** 外形に小さな余りが乗り、試験の許容差や表示の桁の境目で揺れる。
5. **状態欄の幅。** ボタンが1つ増える。幅の見積もりに足し忘れると、幅960で重なる。
