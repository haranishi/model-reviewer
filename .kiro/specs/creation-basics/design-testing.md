# テストと自動操作

第1〜第4段階の方針（論理は GPU なしで試す、実機の確認は OS の入力を使わない自動操作、3つのOSで同じ結果にする決まり）を保つ。検証コマンドも同じ4つである。試験名は目安で、実装者が合う名前に直してよい。

## どこで何を確かめるか
| 対象 | 置き場所 | 確かめること（試験名の目安） |
|---|---|---|
| 空のモデル | `core/src/scene.rs` の中 | `Model::empty()` が部品・描画・形状0個で、`Document::new` に渡してもパニックしない。範囲は空（`empty_model_makes_empty_document`） |
| 色の換算 | `core/src/color.rs` の中 | 256通りの値で `linear_to_srgb8(srgb8_to_linear(c)) == c`。0・255・128 の値が sRGB の式と合う。有限でない値と範囲の外で落ちない。`parse_hex` が `#`・小文字・前後の空白を受け、5桁・7桁・16進でない文字を断る。見本12色が互いに違う |
| 文書の色 | `core/src/doc.rs` の中 | `set_subtree_colors` が選んだ部品と子孫だけを変え、同じ形状を使う別の部品を変えない（`color_does_not_leak_to_shared_shape`）。数が違えば何も変えない。複製が色を引き継ぐ |
| 履歴 | `core/src/history.rs` の中 | 描画ごとに違っていた色が、取り消しで元に戻る（`undo_restores_per_draw_colors`）。透明度が変わらない。文書の版が進み、レビューの版は進まない。`recolored` に範囲が入り、`touched` と `structure` は立たない |
| 部分更新 | `render/src/lib.rs`・`instances.rs` の中、`app/tests/gpu_flow.rs` | 位置の表が部品の順・描画の順であること（`slots_follow_node_then_draw_order`）。色を変えた文書で部分更新すると、範囲のインスタンスの色が新しい値になり、行列と番号は変わらない。非表示の部品では何も起きない。GPUが要る分は `#[ignore]` |
| 空の土台 | `gltf/src/export/` の中 | `empty_base()` が毎回同じバイト列で、土台として読める。足した図形1つで書き出すと `load_glb` で開け、名前・行列・色が一致する（`exports_from_empty_base`）。同じ文書から同じバイト列。描画の無い文書は `InvalidScene`。`load_glb(empty_base())` は今までどおり失敗する |
| 色の写し | `gltf/src/export/recolor.rs` の中、`gltf/tests/` | 2つのノードが同じメッシュを使うファイルで、片方だけ色を変える: アクセサ・バッファビュー・BIN の長さが元と同じ、元の材質とメッシュが同じ位置に同じ値、変えたノードだけが新しいメッシュを指す（`recolor_adds_material_and_mesh_only`）。同じ色の2部品が材質とメッシュを共有する。テクスチャの参照・拡張・名前が写る。材質の無いプリミティブ。点と線のプリミティブが混ざったメッシュで、対応がずれない（`recolor_maps_draws_past_skipped_primitives`）。複製の色だけを変える。元の色に戻すと JSON が元と同じ。書き出したファイルを土台にもう一度書き出すと、材質が増えない（`second_export_adds_nothing`） |
| 照合 | `gltf/src/export/verify.rs` の中 | 写しでない材質（`metallicFactor` が 0 でない、元と別の項目が違う）を足したバイト列を、照合が断る。メッシュの写しが `material` のほかを変えていたら断る。色が1ビット違えば断る |
| 新規作成 | `app/tests/new_doc.rs` | `NewDocument` で空の文書になり、`source_kind` が New、`model_ref` が無い。未保存があると確認が出て、［やめる］で今の文書が残る。書き出し中・読み込み中は断る。図形を置くと原点に大きさ 1。初期視点の距離と格子。全部消しても文書が残る。レビュー保存を断る文。書き出しで作業の対象が移り、`source_kind` が Glb になる（`new_document_exports_and_moves_target`）。空の文書の書き出しを断る。Cmd+N が、ポインタがパネルの上でも働き、入力中とダイアログ中は働かない |
| 色（つなぎ） | `app/tests/color.rs` | `SetColor` が履歴の1回分で、取り消しで戻る。未書き出しの印が付く。同じ色をもう一度選んでも履歴が増えない。読み込み中・書き出し中は変わらない。`pending_patches` に範囲が入り、インスタンスの作り直し（`scene_dirty.instances`）が立たない（`color_change_patches_instances_only`）。色を変えて書き出し、開き直すと同じ色（`color_survives_export_and_reopen`）。テクスチャの警告があるモデルで、注意の文と結果の一覧の行が出る |
| 色の部品 | `app/tests/color_panel.rs`（`egui_kittest`） | 入力用の構造体だけで動く。見本を押すと `SetColor`。16進と R・G・B の確定。読めない値で積まず、理由が出る。`Mixed` と `NoFaces` の表示。`locked` で押せない。日本語の文言で落ちない。幅 960 で見本が6列に収まる（`layout.rs` に足す） |
| GPU | `app/tests/gpu_flow.rs`（`#[ignore]`） | 空のモデルの転送が終わり、図形を足して描ける（`empty_model_uploads_and_accepts_shapes`）。色を変えた後の画像で、変えた部品の画素だけが変わる（`recolor_changes_only_that_part`） |
- 既存の試験を弱めない。`Applied`・`Incoming`・`ExportBase`・`SourceKind` に足した項目で通らなくなる組み立ては、足した分だけ直す。
- 3つのOSで同じにするための注意は第3段階と同じ（パスは `Path::join`、改行は `\n`、浮動小数の比較はビットか決めた誤差）。色の換算は `f64` で計算してから `f32` にし、OSごとの数学関数の差が結果に出ないことを、256通りの表で固定する（**要確認**: `powf` の結果が3つのOSで同じ `u8` に丸まること。CI で確かめる）。

## 自動操作に足すもの
- **手順は足さない。** `click_widget`（`empty.new`、`color.swatch.N`）、`type_text`（`color.hex`）、`key` で操作できる。
- **状態の書き出し**（`dump_state`）: `source_kind` に `"new"`。`selection.color` に `{ "state": "one" | "mixed" | "none", "srgb": "D64545", "linear": [[r, g, b, a], …] }`（`linear` は最大8件）。既存の項目は変えない。
| 手順 | 内容 |
|---|---|
| `new-1.json` | 何も開かずに起動 → `empty.new` → 状態で `source_kind` が new、部品0 → 立方体を置く → 状態で位置が原点、大きさ1 → レビュー保存を試して断られる → 書き出し（`--out` の下）→ 状態で `source_kind` が glb → 画像 |
| `color-1.json` | 見本を開く → 部品を選ぶ → `color.swatch.4`（赤）→ 状態で色 → 取り消し → 元の色 → やり直し → `color.hex` に `3D7BD9` → 複製 → 複製だけ別の色 → 書き出し → 開き直して、2つの色が違うまま → 画像 |
| `desk-1.json` | 新規作成から、机の一式（29部品）を、図形・寸法・複製・キーで動かす・色で作る → 書き出し → 性能値を書き出す。第4段階の接合の後に書く（キーで動かす手順を使うため） |
- 既存の手順（第4段階までの全部）は、手順を変えずに通ること。

## 進行役が実機で確かめること（実装者は「未確認」と報告する）
- 実キーの Cmd+N（OSやメニューに取られずに届くか）。何も開いていない画面のボタン。
- `new-1`・`color-1`・`desk-1` と、既存の手順の全部。見本のファイルのハッシュが、実行の前後で同じこと。
- 画像の目視: 何も開いていない画面、空の文書（格子と原点）、右のパネルの「色」（1色・複数の色・注意の文つき）、幅 960 と広い幅。色を変えた部品だけが変わっていること。
- **軽さ**（要件7.3）: `desk-1` の、開いた後・29部品を置いた後・書き出した後の常駐メモリと時間。値・機械・条件を `verification.md` に書く。比べる相手は、同じ机を「土台の GLB を開いて作った」ときの値（brief の作り方）である。
- ほかのソフト1つで、書き出した GLB を開く: 新規から作ったもの、色を変えたもの、テクスチャ付きのモデルの色を変えたもの。色が画面と合うか、同じ名前の材質がまとめられないか（design-export.md の要確認）。使ったソフトと版を書く。
- 公開の前の点検（第4段階と同じ）: ライセンスと依存、最小の対応版、`Cargo.lock` に外部の項目が増えていないこと、文書とコードに個人名・メール・ホームの絶対パス・道具の名前が無いこと。
