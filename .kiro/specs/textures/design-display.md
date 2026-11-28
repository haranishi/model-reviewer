# 表示（読み込み・復号・GPU）

要件1〜3。型は [design.md](design.md) の「型の約束」。

## 読み込み（`gltf::build`・`gltf::inspect`）
- **どの画像を出すか**: 描画になるプリミティブの材質が `pbrMetallicRoughness.baseColorTexture` を持ち、その `textures[i].source` が、`bufferView` を持ち `mimeType` が `image/png` か `image/jpeg` の画像を指すとき。範囲は BIN の中に収まっていること（外れていれば、今の「データが破損しています」）。先頭のバイトが `mimeType` と合わなければ、先頭のバイトを信じる。
- **出さない画像**（モデルは開く。警告にする）: `source` が無い（KTX2・WebP の拡張だけを持つ）、`uri` を持つ、PNG でも JPEG でもない。件数を `LoadWarning::ImagesSkipped { count, reason }` にする（`reason` は `Format`・`External`・`NoUv`）。
- **基本色以外の画像**: 材質の `normalTexture`・`metallicRoughnessTexture`・`occlusionTexture`・`emissiveTexture` を数え、`LoadWarning::OtherTextures { count }` にする。**今の `LoadWarning::Textures` は、出す画像が1枚も無いファイルでは出さなくなる**（文も「読まず」から変わる）。画像の付いた GLB を開いたときの警告を確かめている既存の試験は、新しい警告へ直す（比べる項目は減らさない）。
- **`TexUse` の作り方**: `texCoord`（`KHR_texture_transform` の `texCoord` があればそちら）の番号 n を決め、プリミティブの `TEXCOORD_n` を読む。アクセサが無ければ、その描画は画像なし（`ImagesSkipped` の `reason: NoUv`）。サンプラは `Sampling` へ写す（無指定は繰り返しと線形・縮小版つき）。変換は `offset`・`rotation`・`scale` から 2×3 の行列にする。`alphaMode` が MASK なら `cutoff` に `alphaCutoff`（既定 0.5）。同じ値の `TexUse` は1つにまとめる。
- **UV**: 小数・正規化した u8・u16 を `[f32; 2]` に直す。`byteStride` を守る。数が位置と違う、有限でない値がある場合は、その形状の UV を捨てて画像なしにする（破損とはしない）。`prim_uvs[形状の番号]` に入れる。
- **形状の共有の鍵に UV のアクセサを足す**（`ShareKey.uv: Option<u32>`。画像を出す描画だけ `Some`）。画像の無いファイルでは鍵が今と同じなので、形状の数も番号も変わらない。同じ形状を、画像つきと画像なしの材質で使うファイルでは、形状が2つになる（まれ。メモリは増える）。
- **画像の表**: 出す画像ごとに `ImageInfo { hash, mime, source: File { offset, len } }`。ハッシュは `ImageHash::of`（画像のバイト列だけを読む）。画素数はここでは読まない（`gltf` は画像の形式を知らない）。
- 書き出した材質の印（`extras.model_reviewer.box_size`）があれば `baked_box` に入れる（[design-export.md](design-export.md)）。

## 復号と縮小（`texture` クレート）
```rust
pub fn dimensions(bytes: &[u8]) -> Result<[u32; 2], TexError>;            // 頭だけ読む
pub fn decode(bytes: &[u8], skip: u8) -> Result<MipChain, TexError>;      // 復号 → skip 段縮める → 縮小版の列
pub fn strip_metadata(bytes: &[u8], mime: Mime) -> Result<Vec<u8>, TexError>;   // design-paste.md
pub enum TexError { Unsupported, TooLarge { size: [u32; 2] }, Broken(String) }  // message_ja() を持つ
```
- 上限は `image` の `Limits` で掛ける（幅と高さ 8192 まで、確保するメモリ 320 MiB まで）。超えたら `TooLarge`。
- 縮小版は 2×2 の平均を、sRGB → リニア → sRGB の表（256個と、逆向きの表）を通して作る。透明度はそのまま平均する。奇数の辺は端の1列を捨てる。最後は 1×1。縮める（`skip`）のは、列の先頭を捨てることと同じである。
- `plan_levels`（`core`）: 各画像の長辺が `max_side` 以下になるまで段を足す。合計（`chain_bytes` の和）が `budget` を超える間、面積が最大の画像すべてに1段ずつ足す。長辺が 64 を下回る画像は、それ以上縮めず `None`（出さない）にする。入力の順に依存しない（同じ入力なら同じ答え）。
- 復号は sRGB 以外の色の設定（ICC など）を見ない。画像の向きの情報も見ない。

## 読み込みの順（`app::loader`・`app::session`）
1. 今までどおり、モデルを渡し（`Loaded`）、当たり判定の索引を作る（`Pick`）。
2. `--no-textures` でなく、画像が1枚以上あれば、同じスレッドで続ける。全画像の `dimensions` を読み、`plan_levels` を呼び、計画を `LoadEvent::TexturePlan { generation, total, shrunk, skipped }` で送る。
3. 1枚ずつ `decode` し、**容量1の別の待ち行列**（`sync_channel(1)`）へ `TextureReady { generation, image, chain }` を送る。受け手が取るまで次を復号しない（CPU 側に持つのは、復号中の1枚と待ちの1枚まで）。失敗は `TextureFailed { image, error }`。
4. 受け手（`gpu_view`）は、フレームごとに予算の残りで `SceneRenderer::upload_image` を進め、1枚を送り終えたら次を取る。受け口を捨てる（別のモデルを開く・閉じる）と、送り手は送信の失敗で終わる。中止の旗も今までどおり見る。
- `Session` は `TextureProgress { total, done, shrunk, failed: Vec<(ImageId, String)>, bytes }` を持つ。状態欄は `done < total` の間「テクスチャ {done}/{total}」。「簡略表示」の一覧は、モデルの警告の後ろに、縮めた・読めなかったの行を足す（件数は一覧の数に入れる）。
- 予算は、形状の転送と1つの値を分け合う（1フレームの合計が 32 MiB を超えない）。形状が先。
- 自動操作の `wait_idle` は、`done + failed < total` の間は待つ。`report.json` と `--log-metrics` に「テクスチャ表示まで」（開く操作から最後の1枚を送り終えるまで）と「読み込み中の最大の常駐メモリ」（画像を送っている間、フレームごとに測った最大）を足す。

## GPU（`render`）
```rust
impl SceneRenderer {
    /// 画像を置く場所を作り、転送を始める。すでに同じ番号があれば置き換える。
    pub fn begin_image(&mut self, device: &wgpu::Device, id: ImageId, chain: MipChain) -> Result<(), RenderError>;
    /// 予算まで送り、送ったバイト数と、送り終えた画像の番号を返す。
    pub fn continue_images(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, budget_bytes: u64) -> ImageProgress;
    pub fn set_textures_shown(&mut self, shown: bool);
}
pub struct RenderStats { /* … */ pub texture_bytes: u64, pub texture_count: u32, pub textured_draws: u64 }
```
- **UV のバッファ**: `plan_buffers` は、UV を持つ形状と持たない形状を別のページに詰める（開いているページを2つ持つ）。UV のページだけが3本目のバッファ（頂点あたり8バイト）を持つ。`UploadJob` は「頂点 → 番号 → UV」の順に送る。UV の無いモデルの計画とバイト数は今と同じ（試験で固定）。
- **画像**: `Rgba8UnormSrgb`、縮小版つき。`write_texture` は、1回が予算を超えないよう行の範囲に分ける。置いたバイト数を `texture_bytes` に足す（計算値。GPU の実使用量ではない）。モデルを閉じると一緒に捨てる。
- **束ねる組（グループ1）**: 画像のビュー、サンプラ、定数48バイト（UV の行列、`cutoff`、箱形の `size`、種類）。`TexUse` ごとに1つ。サンプラは `Sampling` を鍵に使い回す。画像がまだ届いていない `TexUse` は、1×1 の白を指す組で作り、画像を送り終えた回に組だけ作り直す（インスタンスは作り直さない）。
- **まとまり**: `Batch` に `tex: Option<TexId>` を足し、鍵を（形状、`tex`）にする。まとまりの中は今までどおり部品の番号順（選択の二分探索はそのまま使える）。並びは「画像なし → UV → 箱形」にし、パイプラインの切り替えを最大2回にする。
- **パイプライン**: `scene_uv`（頂点の入力に UV を足す。位置11）と `scene_box`（入力は今と同じ。ローカルの位置をフラグメントへ渡す）。どちらも `scene.wgsl` の照明と断面と選択の強調をそのまま使い、色に `画像の色 × 基本色` を入れる。`cutoff` があれば、透明度が小さい画素を捨てる。「面と線」の線、「線」「透かし」、輪郭線、表示を切ったときは、今のパイプラインで描く。
- **色の変更の部分更新**（`update_instances`）は今のまま使える。画像を貼る・外すと、まとまりの鍵が変わるので、インスタンスを作り直す（`set_instances`）。
- 装置の `max_texture_dimension_2d` を `plan_levels` の `max_side` に渡す（4096 と小さいほう）。

## 画面（`app`）
- 表示の並びに［画像］の切り替え（`ids::VIEW_TEXTURES = "view.textures"`）。`Command::SetTexturesShown(bool)`、キーの操作 `Action::DisplayTextures`（名前 `display.textures`、既定 Shift+T）。切っても GPU の画像は捨てない（すぐ戻せる）。
- 詳細パネル: 画像の枚数、送ったバイト数、縮めた枚数、テクスチャ表示までの時間。測っていない値は「未計測」。
- ヘルプの表に「画像の表示を切り替える」を足す。`docs/glb.md` の「開いて簡略表示にするもの」を直す（テクスチャの行を、出す条件と出さない条件に分ける）。

## 壊れやすい所
1. **形状の共有の鍵**。UV を鍵に入れ忘れると、同じ位置で UV の違う2つのプリミティブが1つの形状になり、片方の絵がずれる。入れすぎると、画像の無いファイルで形状の番号が変わり、既存の試験と書き出しの照合に響く。
2. **白の代用の組**。画像が届いたのに組を作り直さないと、いつまでも基本色のままになる。逆に、届くたびにインスタンスを作り直すと、画像の枚数だけ作り直しが走る。
3. **待ち行列の容量**。容量を外すと、復号した全画像が CPU 側に積まれ、2048 画素 × 24 枚で約 380 MiB を一時に持つ。
4. **世代**。前のモデルの画像が、次のモデルの番号へ入らないよう、`generation` を必ず照合する。
