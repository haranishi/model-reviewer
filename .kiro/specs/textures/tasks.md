# Implementation Plan

作業は「作業パッケージ（WP）」に分ける。この仕様の文書だけを渡された実装者が進められるように書いた。段階ごとに取り込める（①だけで「画像つきの GLB が見える」になる）。

## 全体の順
```
前提: stl-up-axis と protect-originals が取り込まれている
 ① 表示    T0 型（core）─▶ (P) T1 読み込み（gltf）・T2 復号（texture）・T3 描画（render）・T4 素材（testdata）─▶ T5 つなぐ（app）
 ② 貼って書き出す  (P) P1 文書と履歴（core。T0 の後ならいつでも）・X1 見た目の写し（gltf。T1 の後）─▶ P2 貼り付けと書き出しをつなぐ（app。T5・P1・X1 の後）─▶ X2 文書
 ③ 仕上げ  F1（進行役）
```
- **T0 は1人で先に済ませる**（`Draw` と `Model` の手書き箇所を、ワークスペース全体で直すため）。T0 の後、(P) の4つと P1 は担当ファイルが重ならない。
- `app` の2つ（T5 → P2）は、`command.rs`・`session/mod.rs`・`ui/ids.rs`・`automation*` が重なるので、順に行う。
- **貼る操作は、書き出しと同じ取り込みで公開する。** 貼り付けだけを先に出すと、画像を書かない書き出しが成功して「形の編集 未書き出し」の印が消え、貼った画像を失う（書き出しの完了が、文書を書き出し済みにする）。そのため P2 は X1 の後に行い、`export_scene` の直しを P2 に入れた。

## 進め方の約束
- **着手前に読むもの**: [design.md](design.md)（方針・型・要確認）、自分の WP が挙げる文書、[design-testing.md](design-testing.md)、第3段階の「3つのOSで同じ結果にするための決まり」。
- **全 WP 共通の合格条件**: `cargo fmt --all --check`・`cargo clippy --all-targets --workspace --locked -- -D warnings`・`cargo test --workspace --locked`・`cargo build --release` が通る。既存の試験を消したり弱めたりしない（期待する値を直すときは、比べる項目を減らさず、直した試験を報告する）。commit・push はしない。
- **依存**: `Cargo.toml` を変えてよいのは、T1（`gltf` の機能1つ）、T2（`image` の機能と新しいクレート）、T4（`testdata` が `image` を使う）、T5（`app` が新しいクレートを使う）だけ。`Cargo.lock` の差分は、ワークスペースの中のクレートの項目と依存の行までを認め、**外部のパッケージの追加と版の変更は1つも認めない**（[design.md](design.md) の「依存」）。外れたら、止めて進行役に伝える。
- **担当ファイルの外を変えない。** 型に項目を足したせいで通らなくなる組み立て（構造体の手書き、`match`）は、足した分だけ直してよい。直したファイルを報告する。
- GPU と画面が要る確認は進行役が行う。実装者は、実行していないものを「未確認」と報告する。API は推測で書かず、依存の実物で確かめる。「要確認」は、確かめた結果を報告に書く。
- 文書とコードに、個人名・メール・ホームの絶対パス・作業に使った道具の名前を書かない。画面の言葉は日本語にする。未実装の機能を、動くように見えるボタンで置かない（WP の途中で取り込むときは、部品ごと出さない）。

- [x] 1. 表示
- [x] 1.1 WP-T0 型と純粋な関数（[design.md](design.md) の「型の約束」、[design-paste.md](design-paste.md) の式、[design-display.md](design-display.md) の `plan_levels`）
  - `core/src/texture.rs`（新規）。`Draw.tex`、`Model.textures`、`SceneRead` の3つの関数（既定の実装つき）、警告の種類 `LoadWarning::OtherTextures`・`ImagesSkipped`（文は ux.md の表層）。手書きの組み立てを全部直す（`tex: None`、`textures: Textures::default()`）
  - 合格: design-testing.md の `core`（単体）の11本。既存の試験が、値を変えずに通る
  - _Boundary: `core/src/{texture,scene,lib}.rs` と、通らなくなった組み立て（全クレート。機械的な直しだけ）_ ／ _Requirements: 1.9, 3.2, 3.3, 4.3_
- [x] 1.2 WP-T1 読み込み（[design-display.md](design-display.md) の「読み込み」）(P)
  - 材質 → `TexUse`、UV、`ShareKey.uv`、画像の表とハッシュ、警告を出す条件（`OtherTextures`・`ImagesSkipped`・今の `Textures` の扱い）、印（`extras.model_reviewer.box_size`）の読み取り
  - 型へ直す前の JSON から、印と `source` の無いテクスチャを読む。`gltf` の機能 `allow_empty_texture` を足す（要確認を先に確かめる）。MASK だけの材質の `TexUse`、`tex_coord`・`info_from`、`no_box_nodes`
  - 合格: `gltf/tests/texture.rs` の17本。`untextured_file_keeps_primitive_numbers` を最初に書く
  - _Boundary: `gltf/src/{build,inspect,error}.rs`、`gltf/tests/`、ルートの `Cargo.toml`（`gltf` の機能の行だけ）_ ／ _Requirements: 1.1, 1.3, 1.4, 1.5, 2.1, 2.2, 2.3_
- [x] 1.3 WP-T2 復号・縮小版・付帯情報の除去（[design-display.md](design-display.md) の「復号と縮小」、[design-paste.md](design-paste.md) の「付帯情報の除去」）(P)
  - 新しいクレート `crates/texture`。ワークスペースの `image` に `jpeg` を足す。**依存の確かめ（design.md）を先に行い、結果を報告する**
  - 合格: `texture/tests/` の11本。`cargo deny check` が通る。`cargo tree -i zune-jpeg --locked` の結果を報告に貼る
  - _Boundary: `crates/texture/`、ルートの `Cargo.toml`・`Cargo.lock`_ ／ _Requirements: 1.2, 1.6, 3.2, 5.2, 9.2_
- [x] 1.4 WP-T3 描画（[design-display.md](design-display.md) の「GPU」、[design-paste.md](design-paste.md) の式）(P)
  - UV のページ、画像の転送（予算つき）、束ねる組と白の代用、`Batch.tex`、`scene_uv`・`scene_box`、`set_textures_shown`、`RenderStats` の3項目
  - 合格: `render/tests/plan.rs` ほかの6本（GPU なし）と、`render/tests/texture.rs` の9本（`--include-ignored`。GPU のある環境で実行し、無ければ「未確認」）。箱形の符号の定数は、6面の試験が通る値にする
  - _Boundary: `render/src/`、`render/tests/`_ ／ _Requirements: 1.1〜1.9, 3.5_
- [x] 1.5 WP-T4 試験用の素材（[design-testing.md](design-testing.md) の「試験用の素材」）(P)
  - `pattern_png`、`textured_quad_glb`、`textured_scene_glb`、コマンドラインの `textured`
  - 合格: `patterns_are_deterministic`、`textured_quad_opens`（T1 の後に通す。先に済んだら `#[ignore]` にせず、T1 と合わせて取り込む）、`textured_scene_has_requested_counts`
  - _Boundary: `testdata/src/`、`testdata/Cargo.toml`_ ／ _Requirements: 7.4_
- [x] 1.6 WP-T5 表示をつなぐ（[design-display.md](design-display.md) の「読み込みの順」「画面」）
  - 読み込みのスレッドの続き（圧縮したままの画像を引き継ぐ）と容量1の待ち行列、形状とは別の段の画像の転送と描き直し、`TextureProgress`、状態欄と「簡略表示」、表示の切り替え（部品・`Command`・キー）、起動の引数2つ、性能値2項目、`wait_idle`、`dump_state` の `textures`、色の欄の注意（部品単位）、ヘルプ
  - 合格: `app/tests/texture.rs` の表示の分（`opening_textured_glb_reports_progress`・`textures_can_be_hidden_and_shown`・`color_note_is_per_part`・`no_textures_flag_skips_decoding`）、幅960の試験、キーの表の試験、自動操作の `dump_state_has_textures` と `wait_idle_*` の3本、`textures_arrive_after_geometry_upload_is_done`。既存の自動操作の手順が、手順を変えずに通る
  - _Boundary: `app/src/{loader,gpu_view,command,metrics,app}.rs`、`app/src/session/{mod,open,color}.rs`、`app/src/ui/{status,viewbar,color,help,ids,details}.rs`、`app/src/keymap/actions.rs`、`app/src/automation*`、`app/tests/`、`app/Cargo.toml`（`texture` クレートを足すだけ）_ ／ _Requirements: 1.7, 1.8, 2.4, 3.1, 3.4, 3.6, 3.7, 3.8, 7.2_

- [x] 2. 貼り付けと書き出し（まとめて公開する）
- [x] 2.1 WP-P1 文書と履歴（[design-paste.md](design-paste.md) の「文書と履歴」）(P)
  - `DrawLook`、画像と貼り方の表（足すだけ）、`subtree_looks`・`set_subtree_looks`、`Edit::SetLooks`、`Applied.retextured`
  - 合格: `core/tests/doc.rs`・`history.rs` の7本
  - _Boundary: `core/src/{doc,history}.rs`、`core/tests/{doc,history}.rs`_ ／ _Requirements: 4.5, 4.6, 4.7, 4.8_
- [x] 2.2 WP-X1 見た目の写し（[design-export.md](design-export.md) の全部）(P)
  - `ExportScene.textures`、判定（`same_look`）、土台の全画像の索引、画像・サンプラ・テクスチャ・材質・メッシュの追記、`info_from` からの textureInfo の写し、箱形を焼く（スキンとモーフは断る）、`Appender` の画像つきの材質、照合の6行、`ExportNote::Retextured`
  - 合格: `gltf/tests/export_texture.rs` の17本。既存の `export.rs`・`recolor.rs`・`empty_base.rs` が通る。`exporting_twice_adds_nothing` と `remove_export_undo_export_restores_original` を最初に書く
  - _Boundary: `gltf/src/export/`、`gltf/tests/`_ ／ _Requirements: 6.1〜6.7_
- [x] 2.3 WP-P2 貼り付けと書き出しをつなぐ（[design-paste.md](design-paste.md) の「大きさの決め方」「つなぎ方」、[design-export.md](design-export.md) の「画面と結果の一覧」、[ux.md](ux.md) の骨格と表層。P1・X1・T5 の後）
  - `session/texture.rs` と `ui/texture.rs`（新規）、`Command` 4つ、貼り付けの仕事のスレッド（世代と版の照合）、ドロップの振り分け、保護（パスと、ファイルそのままの識別）、キー2つ、自動操作の `paste_texture` と `selection_texture`、`export_scene` が表を写す、結果の一覧の文
  - 合格: `app/tests/texture.rs` の貼り付けと書き出しの分（design-testing.md の表の `paste_*`・`box_paste_*`・`remove_*`・`size_*`・`stl_*`・`dropped_*`・`pasted_*`・`renamed_*`・`no_file_name_*`・`export_reopen_matches_looks`）、幅960、`paste_texture_parses_path`。既存の `app/tests/export.rs` が通る
  - _Boundary: `app/src/session/{texture,mod,open,export}.rs`、`app/src/ui/{texture,mod,ids,help}.rs`、`app/src/{command,gpu_view,exporter}.rs`、`app/src/exporter/protection.rs`、`app/src/keymap/actions.rs`、`app/src/automation*`、`app/tests/`_ ／ _Requirements: 4.1〜4.13, 5.1〜5.4, 6.8, 7.1, 7.2_
- [x] 2.4 WP-X2 文書（P2 の後）
  - `docs/glb.md`・`docs/verify.md`・`docs/roadmap.md`・`README.ja.md`・`README.md` の直し（どの文書も100行以内。越える文書は分ける）。開けないテクスチャの拡張、焼いた形状が写さない項目、未確認の項目を書く
  - _Boundary: `docs/`、README_ ／ _Requirements: 2.4, 9.3_

- [x] 3. 仕上げ（進行役）
  - 実機の自動操作 `tex-1`〜`tex-3` と既存の全手順を通し、撮った画像を目で確かめる。GPU の試験を `--include-ignored` で通す
  - 計測（[design-testing.md](design-testing.md) の表）。上限の既定を決め直す。書き出した GLB をほかのソフトで開く
  - 手順書に「画像を貼る」の章を足す。`verification.md` を足し、`spec.json` の `phase` を進める
  - _Requirements: 7.3, 8.1, 8.2, 9.1〜9.4_

## 壊れやすい所（先に読む。各設計の文書の同じ節に詳しく書いた）
1. **箱形の投影の向き**（T3）。画面の式と `box_uv` が食い違うと、画面で見た絵と書き出した絵がずれる。6面と鏡写しの GPU の試験で止める。
2. **形状の共有の鍵**（T1）。画像の無いファイルで形状の番号を変えない。
3. **書き出すたびに増える・元へ戻らない**（X1）。印、ハッシュの範囲、外して書き出した後の取り消し。
4. **貼る先のずれと、履歴の番号**（P1・P2）。表は足すだけ。貼る先は押したときの文書（世代と版）の部品。
5. **待ち行列の容量と世代**（T5）。CPU 側に全画像を積まない。前のモデルの画像を次へ入れない。
6. **既存の絵と数を変えない**（T0・T3）。テクスチャの無いモデルで、計画・転送のバイト数・描いた絵が今と同じ。
