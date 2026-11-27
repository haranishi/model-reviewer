# 依存の選定・ライセンス・捨てた案・落とし穴

## 依存と選定理由
版は 2026-11-26 に crates.io から解決し、取得できたもの。`Cargo.lock` に固定する。
| クレート | 理由と使い方 |
|---|---|
| eframe / egui / egui-wgpu 0.37.0 | 依頼の指定。eframe は `default-features = false` で `wgpu`・`accesskit`・`default_fonts`・`wayland`・`x11` だけを有効にする。`glow`・`persistence`・`system_fonts` は使わない |
| wgpu 30.0.2 | `render` クレートが直接依存する（egui に依存させないため）。egui-wgpu が要求する範囲と同じ版なので、依存木に1つしか入らない。版がずれると、`app` が `eframe::wgpu::Device` を `render` に渡す箇所でコンパイルが通らなくなる。型の混在は、ビルドが通る限り起きない |
| egui_system_font_provider 0.37.0 | 日本語フォントをOSから読む。eframe に任せず自分で登録する理由は [design-app.md](design-app.md) |
| gltf 1.4.1 | `default-features = false`、`utils`・`names` と、見た目だけに関わる拡張の機能を有効にする。`import` を切るので、画像の復号と base64 の依存が入らない |
| glam 0.34.3 | egui・wgpu の周辺で広く使われ、`Affine3A` と視点・投影の関数がある |
| blake3 1.8.7 | モデルの識別。ARMのNEONで速く、暗号学的ハッシュなので偶然の一致を考えなくてよい |
| serde / serde_json | 依頼の指定 |
| rfd 0.18.0 | ネイティブのファイルダイアログ。同期の `FileDialog` を使う（OSのモーダルなので、その間アプリが止まるのは通常の振る舞い） |
| memory-stats 1.2.0 | プロセスの常駐メモリ。取れなければ `None` |
| bytemuck 1.25 / thiserror 2 / log 0.4 | 頂点をバイト列として渡す／エラー型／ログ。どれも依存木にすでに入っている。ログの出力先は自前の数十行で、追加のクレートは入れない |
| image 0.25 | `default-features = false`、`png` のみ。自動操作の画像保存に使う |
| egui_kittest 0.37.0（開発用） | 画面なしのテスト |
入れないもの: 引数解析のクレート（引数は4種類）、非同期ランタイム、スレッドプール、BVHのクレート、設定ファイルのクレート、ネットワークを使うクレート。

## ライセンス：MIT OR Apache-2.0（推奨案）
- Rust のクレートで最も一般的な組み合わせで、egui・wgpu・glam・gltf と同じである。上流とコードをやり取りするときに、条件の食い違いが起きない。
- Apache-2.0 は特許の許諾を含み、MIT は短く、GPLv2 のソフトにも取り込める。利用者が都合のよいほうを選べる。
- 貢献は「提出したものは同じ二重ライセンスで提供する」と `CONTRIBUTING.md` に書く（署名つきの同意書は求めない）。
- 著作権表記は「model-reviewer contributors」。依存のライセンスはすべてこれと両立する（rfd は MIT のみ、blake3 は CC0 または Apache-2.0、bytemuck は Zlib・Apache-2.0・MIT）。
- 実行ファイルに含まれる依存は寛容なライセンスだけである（2026-11-26 に `cargo metadata` で確認）。egui の内蔵フォント（`epaint_default_fonts`: OFL-1.1・Ubuntu Font Licence）は、実行ファイルを配るときに表記が要る。MPL-2.0 の `colored` はテスト用の依存（egui_kittest 経由）だけで、実行ファイルに入らない。
- 外部の素材とフォントは同梱しない。サンプルGLBは自作で、同じライセンスにする。

## 捨てた案と理由
| 案 | 捨てた理由 |
|---|---|
| eguiの描画パスへコールバックで直接描く（`egui_wgpu::CallbackTrait`） | 深度を eframe の設定に頼ることになり、`egui_kittest` のパスには深度が無い。3Dを描き直さないフレームも作れない |
| GPUで部品番号を描いて読み戻す当たり判定 | 読み戻しが非同期で1〜2フレーム遅れる。GPUの無い環境で選択を試験できない。注釈に要るローカル座標を別に求める必要がある |
| BVHに `bvh` 0.13 を使う | nalgebra に依存し、glam との間で型の変換が要る |
| BVHに `obvhs` 0.4 を使う | glam 0.34 に対応していて有力だが、初版の用途（クリック1回につき光線1本）には過剰。自前の実装を `PickIndex` の内側に隠し、実測で遅ければ差し替える（目安は [design-review.md](design-review.md)） |
| `gltf::Gltf::from_slice` / `gltf::import` | 未対応の理由を伝えられない。BINを複製する。画像を復号する |
| 法線の無い形状で、頂点を三角形ごとに複製する | メモリと転送量が増える。シェーダーの微分で足りる |
| 日本語フォントの同梱 | 配布物が数MB増え、フォントのライセンス表記が要る。OSのフォントで足りない場合の逃げ道は用意する |
| コアを1クレートにまとめ、gltf と描画もその中に置く | コアが wgpu と gltf に引きずられ、形状編集や別形式の読み込みを足すときに分け直しになる |
| 部品・操作・描画を差し替えられる拡張の仕組み（プラグイン） | 初版には要らない。境界は「クレートの依存の向き」だけにとどめる |
| 取り消し／やり直し | 初版の範囲外。ただし変更の入口を `Command` に絞ってあるので、後から履歴を足せる |
| ワールド座標で注釈を持つ | 部品を動かす機能が入ると全注釈の変換が要る |
| モデル不一致でも注釈を当てはめる選択肢 | 誤った場所を指す注釈を、気づかれないまま作る |
| 視点の切り替えのアニメーション | 連続再描画が要る。テストの結果が時間に左右される |
| ファイルのメモリマップ読み込み | `unsafe` が要り、読み込み中にファイルが書き換わると未定義の動きになる。初版は `fs::read` にし、巨大なファイルで必要になったら検討する |
| 他形式（OBJ・STL・STEP） | 依頼の範囲外。`gltf` クレートと同じ形の読み込みクレートを足せば入る構成にしてある |

## 落とし穴（実物のソースで確かめたもの）
記憶や古い資料どおりに書くと通らない箇所。括弧内は確かめたファイル。
1. **eframe 0.37 の `App` に `update` は無い。** `logic(&mut self, ctx, frame)` と `ui(&mut self, ui, frame)`（`eframe/src/epi.rs`）。パネルは `egui::Panel::left / right / top / bottom` と `show(ui, ..)`・`show_collapsible`。`SidePanel` や `show_inside` は古い名前で、非推奨の警告が出る（`egui/src/containers/panel.rs`）。
2. **glam 0.34 に `Mat4::perspective_*`・`look_at_*` は無い。** `glam::camera::rh::{view, proj::directx}` を使う（`glam/CHANGELOG.md`、`src/camera/`）。
3. **`register_native_texture` に渡す画像は Rgba8Unorm でなければならない**（`egui-wgpu/src/renderer.rs`）。sRGB形式の画像を渡すと色が二重に変換される。
4. **wgpu 30 の `DepthStencilState` は `depth_write_enabled` と `depth_compare` が `Option`。** ほかにも細かな違いがあるので、`egui-wgpu/src/renderer.rs` の書き方を手本にする。
5. **`egui_kittest` は、描けない文字があるとパニックする**（`src/builder.rs` の `allow_missing_glyphs`）。日本語の画面をそのままテストにかけると落ちる。
6. **`egui_kittest` の描画パスに深度は無い**（`src/wgpu.rs`）。既定のアダプタ選びはCPU描画を優先し、無ければ実GPUを使う。
7. **`gltf` の検証は、有効にしていない必須拡張をすべて「Unsupported extension」にする**（`gltf-json/src/root.rs`）。`Document::validate` は非公開で、`from_json` か `from_json_without_validation` の二択。
8. **wgpu は、捕まえなかったエラーで既定ではパニックする。** `Device::on_uncaptured_error` を起動時に登録する。`ErrorScopeGuard` はスレッドをまたげない（`wgpu/src/api/device.rs`）。
9. **eframe の既定の上限は `max_buffer_size` 256MiB・`max_texture_dimension_2d` 8192**（`egui-wgpu/src/setup.rs`）。
10. **画像の取得は `ctx.request_screenshot(クロージャ)`。** eframe の `__screenshot` 機能と環境変数は wgpu では未実装（`eframe/src/native/wgpu_integration.rs`）。
11. **ドロップされたファイルは `Arc<dyn DroppedFile>` で、パスは `path()` で取る**（`egui/src/data/input/dropped_file.rs`）。構造体のフィールドではない。
12. **release のプロファイルで `panic = "abort"` にしない。** 読み込みスレッドの `catch_unwind` が効かなくなる。
13. Rust は `PATH` に無い。`export PATH="$HOME/.cargo/bin:$PATH"` を先に実行する。ネットワークの無い環境では `cargo build --offline` を使い、`Cargo.lock` に無い依存を足さない。足す必要が出たら、作業を止めて進行役に伝える。
