# 試験と計測

要件7・8。第3段階の [3つのOSで同じ結果にするための決まり](../cad-inspect/design-testing.md) を守る（時刻・乱数・ホームのフォルダに頼らない、浮動小数は式を固定する）。

## 試験用の素材（外部の素材を置かない）
`testdata` に足す。画像はすべて式で作り、PNG に直す（乱数は、種を固定した自前の xorshift）。
```rust
pub fn pattern_png(kind: Pattern, size: u32, seed: u32) -> Vec<u8>;   // Checker・Stripes・Noise・Quadrants（4色。向きの確かめ用）
pub fn textured_quad_glb(options: &TexturedOptions) -> Vec<u8>;        // 板1枚。UV の型・texCoord・サンプラ・変換・MASK を選べる
pub fn textured_scene_glb(images: u32, size: u32, parts: u32) -> Vec<u8>;   // 計測用。画像ごとに材質1つ、部品は立方体と板
```
- コマンドライン: `cargo run --release -p model-reviewer-testdata -- textured <画像の枚数> <辺の画素数> <部品の数> <出力.glb>`。計測用のモデルはリポジトリに置かず、`target/` に作る。
- JPEG は、`image` の機能で書けるならそれで作る。書けなければ、自分で作った 8×8 の JPEG のバイト列を定数で持つ（[design.md](design.md) の要確認）。
- 付帯情報つきの画像は、試験の中で区画を手で足して作る（PNG に `tEXt` と `eXIf`、JPEG に `APP1` と `COM`）。

## GPU と画面なしの試験（3つの OS の CI で通す。名前は目安）
| 置き場所 | 試験 |
|---|---|
| `core`（単体） | `sniff_reads_magic_bytes`、`plan_levels_caps_long_side`、`plan_levels_shrinks_largest_first`、`plan_levels_is_order_independent`、`plan_levels_drops_below_64`、`chain_bytes_matches_levels`、`box_uv_axes_and_signs`（6面の4隅。裏返らない）、`box_uv_tie_prefers_x_then_y`、`bake_box_keeps_triangle_positions`（位置のビットと三角形の順）、`bake_box_splits_only_at_axis_changes`、`same_look_treats_baked_box_as_box` |
| `core/tests/doc.rs`・`history.rs` | `add_image_dedups_by_hash`、`set_looks_round_trips`、`paste_undo_redo_restores_looks`、`duplicate_shares_image`、`removed_part_comes_back_with_texture`、`tables_never_shrink`、`untextured_document_reads_as_before` |
| `gltf/tests/texture.rs` | `base_color_texture_becomes_tex_use`、`texcoord_1_is_read`、`normalized_u8_and_u16_uvs`、`sampler_defaults_and_values`、`texture_transform_applies_scale_rotation_offset`（拡大 (2, 1)・回転 90 度・ずらし (0.5, 0) を UV (1, 0) に掛けた値を、拡張の文書の行列から手で計算した値と比べる。逆の順で掛けると (0, 1.5) 側の別の値になり、落ちる。回転の向きは拡張の文書の行列で確かめる）、`mask_without_texture_keeps_cutoff`、`sourceless_texture_opens_with_warning`、`box_mark_is_read_from_extras`、`skinned_and_morphed_nodes_are_listed`、`mask_sets_cutoff_blend_does_not`、`other_textures_are_counted_not_read`、`external_and_unknown_images_are_skipped`、`missing_uv_drops_texture_only`、`share_key_separates_different_uvs`、`untextured_file_keeps_primitive_numbers`、`image_hash_covers_exact_byte_range` |
| `texture/tests/` | `decodes_png_and_jpeg_to_rgba8`、`mips_average_in_linear`（黒と白の市松が、sRGB の 188 付近になる）、`odd_sizes_end_at_1x1`、`skip_drops_top_levels`、`too_large_is_refused_before_decoding`、`broken_image_is_an_error`、`strip_removes_exif_and_text_keeps_pixels`（復号した画素が前後で同じ）、`strip_removes_segments_between_scans`（走査が2つある JPEG の、走査の間に置いた `APP1` と `COM`）、`strip_drops_bytes_after_eoi`、`strip_is_idempotent`、`oversized_pixel_count_is_refused_from_header` |
| `render/tests/plan.rs` ほか | `uv_pages_are_separate`、`plan_without_uvs_is_unchanged`（今の計画と同じ値）、`batches_split_by_texture_and_keep_node_order`、`batch_order_is_plain_uv_box`、`selected_ranges_work_across_texture_batches`、シェーダーの検査（`scene_uv`・`scene_box` を naga で通す。今の検査と同じ形） |
| `gltf/tests/export_texture.rs` | `pasted_image_is_embedded_once`、`same_image_as_base_is_reused`、`uv_part_swaps_texture_index_only`、`removed_texture_drops_the_field`、`box_part_gets_baked_mesh`、`baked_mesh_keeps_triangles`、`duplicate_with_other_image_keeps_original`、`exporting_twice_adds_nothing`、`remove_export_undo_export_restores_original`（元の画像の番号・`texCoord: 1`・UV の変換に戻り、画像を足さない）、`unreferenced_base_image_is_reused`、`box_paste_keeps_alpha_mode_and_cutoff`、`skinned_or_morphed_box_is_refused`、`same_document_same_bytes_with_images`、`no_name_no_uri_in_images`、`untouched_parts_keep_json`、`verify_rejects_changed_image_bytes`、`verify_rejects_dropped_material_fields`。既存の `export.rs`・`recolor.rs` の試験がそのまま通る |
| `app/tests/texture.rs` | `opening_textured_glb_reports_progress`、`textures_can_be_hidden_and_shown`、`paste_applies_to_selection_and_descendants`、`paste_sets_rgb_white_keeps_alpha`、`paste_without_selection_is_refused`、`paste_targets_the_part_selected_at_request`、`paste_is_dropped_when_another_model_opens_meanwhile`（鍵が同じ別のモデル）、`paste_is_dropped_when_document_changes_meanwhile`、`box_paste_on_skinned_part_is_refused`、`box_paste_keeps_mask_cutoff`、`remove_texture_keeps_color`、`size_field_changes_box_size`、`stl_paste_uses_scale_for_size`、`dropped_image_pastes_dropped_glb_opens`、`pasted_file_is_protected_from_export_and_review`、`renamed_pasted_file_is_still_protected`（付帯情報つきの画像を貼り、別名へ写して元を消し、そこへの書き出しとレビュー保存が断られる）、`no_file_name_in_state_or_glb`、`color_note_is_per_part`、`export_reopen_matches_looks`、`no_textures_flag_skips_decoding` |
| `app/tests/{layout,keymap,keys_window}.rs` | 幅960で「画像」のボタンと状態欄の進み具合が重ならない。キーの表が45行で、既定に重複が無い。Cmd+T と Shift+T が入力中に働かない |
| `app/src/automation/tests.rs` | `paste_texture_parses_path`、`dump_state_has_textures`、`wait_idle_waits_for_textures`、`wait_idle_ends_without_plan`（画像0枚・`--no-textures`）、`wait_idle_ends_when_images_are_skipped_or_fail`、`textures_arrive_after_geometry_upload_is_done`（形状の転送が済んだ後のフレームでも、画像が進み、描き直しが掛かる） |
アプリの試験は、復号と貼り付けのスレッドを、今の読み込みと同じく「終わるまで回す」補助で待つ。GPU が無い環境では、画像の転送を「届いたら捨てて数だけ数える」受け口で代える（進み具合と状態は GPU なしで確かめられる）。

## GPU の試験（`--include-ignored`。CI では数えるだけ）
`render/tests/texture.rs`: `textured_quad_shows_image_not_factor`（4色の画像の4か所の画素）、`factor_tints_texture`、`mask_cuts_out_pixels`、`mip_is_used_far_away`、`hidden_textures_match_plain_render`（切った絵が、画像の無い描画と同じ）、`box_projection_matches_box_uv_on_six_faces`（透視と平行）、`box_projection_on_mirrored_part`、`placeholder_then_image`（届く前は基本色、届いた後は画像）、`untextured_model_uploads_same_bytes`（今の値と同じ）。基準画像との1画素比較はしない。色は許容差つきで比べる。

## 実機の自動操作（進行役。画面は透明）
| 手順 | 確かめること |
|---|---|
| `tex-1` | 生成した画像つきの GLB を開く。形が先に出る。画像がすべて出るまで待ち、撮る。表示を切って撮る。状態の `textures` を照合 |
| `tex-2` | 新規作成 → 立方体 → `paste_texture` → 大きさを変える → 取り消し・やり直し → 複製 → 書き出し → 開き直して、見た目の状態が一致 |
| `tex-3` | STL に貼る → 書き出し → 開き直す。元の STL と画像のファイルのハッシュが前後で同じ |
| `heavy-tex` | 下の計測。`sample` と `sample_every` で値を残す |
撮った画像は進行役が目で確かめる（絵が裏返っていない、継ぎ目の位置、縮小のちらつき）。書き出した GLB は、ほかのソフト1つで開いて見え方を確かめ、ソフトと版を記録する。

## 計測（要件8）
- **条件を書く**: 機械、OS、リリースビルド、ウィンドウの大きさ、電源、ほかのアプリ。各3回の中央値。ファイルは OS のキャッシュにある状態。
- **モデル**: `textured` で作る。画像 0・24・96 枚 × 辺 512・2048 画素、部品は 200 個で固定（形状の差を消す）。加えて、8192 画素 × 4 枚（縮小が働く）と、第5段階の確認用の場面（200万三角形・24枚。あれば）。
- **起動の引数**: 既定、`--no-textures`、`--texture-budget-mib 128`。
| 測る項目 | 出どころ |
|---|---|
| 初回表示まで | 今の定義のまま（画像を待たない） |
| テクスチャ表示まで | 開く操作から、最後の画像を送り終えるまで（新しい項目） |
| 常駐メモリ | 画像がすべて出た後、120フレーム待ってからの `physical_mem` |
| 読み込み中の最大の常駐メモリ | 画像を送っている間、フレームごとに測った最大（新しい項目） |
| 視点を回す間のフレームの CPU 時間（平均・最大） | `drag_view` の `sample_every`。画像あり・表示を切る・`--no-textures` の3通り |
| 送った画像のバイト数・枚数・縮めた枚数 | `RenderStats` と `TextureProgress`（計算値） |
| 貼り付けの確定、取り消し | 今の「編集の確定」「取り消し・やり直し」 |
| 書き出し（読み取り・組み立て・書き込み・確かめ）と最大のメモリ | 今の項目。画像 24 枚を貼った文書で測る |
- **書かないもの**: GPU のメモリの実使用量と GPU の処理時間（測る手段を持っていない。「未計測」）。ほかのソフトとの比較。フレームの CPU 時間を「描画の速さ」と呼ぶこと。
- 結果は `verification.md` と `docs/verify.md` の表に入れる。上限の既定（512 MiB）は、この表を見て決め直す。
