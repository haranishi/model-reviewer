# Technical Design（テクスチャの表示と貼り付け）

要件は [requirements.md](requirements.md)、体験は [ux.md](ux.md)。細部は、表示が [design-display.md](design-display.md)、貼り付けが [design-paste.md](design-paste.md)、書き出しが [design-export.md](design-export.md)、試験と計測が [design-testing.md](design-testing.md)。第5段階の [見立て](../creation-basics/design-texture-outlook.md) の「決めるべきこと」に、ここで答えを出した。

## 確かめた事実（コードを読んだ結果。2026-11-28）
- `core::scene::Vertex` は位置と法線の24バイト。`Draw` は `{ prim, base_color }`。`Model` と `Document` は画像も UV も持たない。
- 読み込み（`gltf::build`）は、形状を（位置・法線・番号・種類）のアクセサの組で共有する。テクスチャは件数を数えて警告にするだけである。
- 描画は、形状ごとに1回の描画命令でインスタンスをまとめる（`instances::Batch`）。束ねるものは定数1つだけで、形状のパイプラインは、面・線（重ねる線と線だけの2本）・透かしである。インスタンスは144バイト。
- 読み込みのスレッドは、ファイルのバイト列を持ったまま、モデルを渡し、当たり判定の索引を作って終わる。
- 書き出しは、色を変えた部品に材質とメッシュの写しを足す（`export/recolor.rs`）。照合は、最上位の8つの配列以外が元と等しいことと、開き直した形状が文書と同じことを確かめる。書き出した後、文書は読み直さず、次の書き出しで新しいファイルを土台に読む。
- `image` は 0.25.10 で、有効な機能は `png` だけ。JPEG の機能は `zune-jpeg`（^0.5.5）と `zune-core`（^0.5.0）を要る。`Cargo.lock` には `zune-jpeg` 0.5.15 と `zune-core` 0.5.3 が載っているが、いまのビルドには入っていない（`cargo tree -i zune-jpeg` が空）。どちらも MIT OR Apache-2.0 OR Zlib、`rust-version` は 1.75。`image` は 1.88。
- 構造体を手で組み立てている箇所は、`Draw` が42（15ファイル）、`Model` が44、`Primitive` が39（17ファイル）ある。

## 方針（7つ）
1. **型の変更は1回で済ませる。** `Draw` に `tex: Option<TexId>`、`Model` に `textures: Textures` を足す。`Vertex` と `Primitive` は変えない（UV は形状の番号に並ぶ別の表に持つ）。
2. **UV は、画面に画像が出る形状だけが持つ。** テクスチャの無いモデルでは、メモリも GPU の転送も今と同じになる（要件1.9）。
3. **今の描画の道は変えない。** 画像つきの描画は、別のパイプライン2本（UV で貼る・箱形で貼る）で描く。画像の無い描画・「線」「透かし」・表示を切ったときは、今のパイプラインをそのまま使う。
4. **形を先に、画像は後から。** 復号は読み込みのスレッドが1枚ずつ行い、1枚ぶんだけ待ち行列に置く。GPU へは予算つきで送る。届くまでは 1×1 の白で代用し、基本色だけの見た目にする。
5. **箱形の投影は、画面ではシェーダーが計算する。** 形状を増やさない。書き出すときだけ、同じ式で UV 付きの形状を作る。
6. **書き出しは足すだけ。** 色の写しの仕組みを「見た目の写し」に広げる。画像は中身のハッシュで見分け、同じものは1回だけ書く。
7. **利用者の持ち物を残さない。** 画像の名前と場所は持たない。付帯情報は貼るときに落とす。

## 型の約束（`core::texture`。新規。GPU にも画像の復号にも依存しない）
```rust
pub struct ImageId(pub u32);  pub struct TexId(pub u32);
pub enum Mime { Png, Jpeg }
pub fn sniff(bytes: &[u8]) -> Option<Mime>;                 // 先頭のバイトで判定（PNG の8バイト、JPEG の FF D8 FF）
pub struct ImageHash(pub [u8; 32]);                         // blake3。impl ImageHash { pub fn of(bytes: &[u8]) -> Self }
pub enum ImageSource { File { offset: u64, len: u64 }, Held(Arc<[u8]>) }   // File は開いたファイルの BIN の中の範囲
pub struct ImageInfo { pub hash: ImageHash, pub mime: Mime, pub source: ImageSource }
pub enum Wrap { Repeat, Mirror, Clamp }
pub struct Sampling { pub wrap: [Wrap; 2], pub mag_linear: bool, pub min_linear: bool, pub mip: Mip }  // Mip { None, Nearest, Linear }
pub enum UvMap {
    Unresolved { material: u32 },        // 表示を省いた基本色の画像。取り消しで元の参照を戻す
    Vertex { transform: [f32; 6],        // 2×3 の行列（m00 m01 tx m10 m11 ty）。無変換は単位行列
             tex_coord: u32, info_from: Option<u32> },  // UV の番号と、textureInfo を読んだ材質の番号（書き出しが写す）
    Box { size: f32, aspect: f32 },      // 形状のローカルの長さで画像の長い辺と、画像の幅 / 高さ
}
pub struct TexUse { pub image: Option<ImageId>, pub sampling: Sampling, pub map: UvMap,   // 表示する画像が無ければ None
                    pub cutoff: Option<f32>,          // 材質が MASK のとき。画像の有無によらず保つ
                    pub baked_box: Option<f32> }      // 箱形を UV に焼いて書いた材質の印（design-export.md）
pub struct Textures { pub images: Vec<ImageInfo>, pub uses: Vec<TexUse>,
                      pub prim_uvs: Vec<Vec<[f32; 2]>>,    // 形状の番号順。空なら UV なし。表が空なら全部なし
                      pub no_box_nodes: Vec<u32>,          // スキンかモーフを持つノード（ファイルの番号。昇順）
                      pub source_meshes: HashMap<u32, u32> } // ノード → 開いた時点のメッシュ（取り消しの書き出し）
pub struct MipChain { pub size: [u32; 2], pub levels: Vec<Vec<u8>> }       // RGBA8（sRGB）。levels[0] が size
pub fn plan_levels(sizes: &[[u32; 2]], max_side: u32, budget: u64) -> Vec<Option<u8>>;  // 何段縮めるか。None は出さない
pub fn chain_bytes(size: [u32; 2]) -> u64;                                 // 縮小版を含む RGBA8 のバイト数
pub fn box_uv(p: Vec3, n: Vec3, center: Vec3, sides: [f32; 2]) -> [f32; 2]; // 式は design-paste.md
pub fn bake_box(prim: &Primitive, sides: [f32; 2]) -> Baked;  // 分けた頂点の元の番号も返す
```
- `SceneRead` に `tex_use(TexId)`・`image(ImageId)`・`prim_uvs(PrimId)` を、既定の実装つき（`None`・空）で足す。`Document` は、土台の表の後ろに、貼った分を足した番号で返す（形状と同じ数え方）。
- 2つの `TexUse` が「同じ見た目」かは、番号ではなく値で比べる（画像はハッシュ。`info_from` は比べない）。`same_look(a, b)` を `core::texture` に置き、書き出しと照合が使う。

## クレートの境界
| クレート | 足すもの |
|---|---|
| `core` | 上の型と純粋な関数。`Document` の画像の表、`Edit::SetLooks`（[design-paste.md](design-paste.md)） |
| `gltf` | 材質 → `TexUse`、UV の読み取り、警告。書き出しの「見た目の写し」と照合。**画像は復号しない**（バイト列とハッシュだけ扱う） |
| `texture`（新規 `model-reviewer-texture`） | 復号（RGBA8）、縮小版の列、付帯情報の除去。依存は `core` と `image` だけ |
| `render` | UV のバッファ、画像の転送、束ねる組、パイプライン2本。`MipChain` を受け取るだけで、復号は知らない |
| `app` | 読み込みのスレッドの続き、貼り付けの仕事、画面、キー、自動操作、計測 |
新しいクレートに分けたのは、`core` に画像の形式のクレートを入れない決まり（[ロードマップ](../../../docs/roadmap.md)）を保ち、`gltf` と `render` を GPU なし・復号なしで試験できるままにするためである。

## 依存（要件9.2）
- 足すのは、ワークスペースの `image` の機能に `"jpeg"` を1つ（`features = ["png", "jpeg"]`）。新しいクレートは `image` と `core` を使う。`testdata` は試験用の PNG を作るために `image` を使う。ほかは足さない。
- 理由: GLB の画像は PNG か JPEG と決まっていて、JPEG を読めないと実物のモデルの多くで絵が出ない。自前の JPEG の復号は書かない。
- 確かめ方（WP-T2 が行い、結果を報告する）: `cargo tree -i zune-jpeg --locked` が `image` 経由だけを示す。`cargo deny check` が通る（`deny.toml` の許可は MIT・Apache-2.0・Zlib を含む）。`Cargo.lock` の差分は、新しいクレートの項目（`model-reviewer-texture`）、それを使う `model-reviewer`・`model-reviewer-testdata` の依存の行、`image` の依存の行（`zune-*`）までを認める。**外部のパッケージの追加と版の変更が1つも無いこと**を条件にする。`cargo build --release` が 1.95 で通る。条件を外れたら、止めて進行役に伝える。
- 捨てた案: `zune-jpeg` を直に使う（`image` の PNG の道と2本になる）、`jpeg-decoder`（`image` が使わなくなった）、WebP・KTX2（依存が増え、圧縮テクスチャは装置ごとの対応が要る）。

## 検討して捨てた案
| 案 | 捨てた理由 |
|---|---|
| 頂点に UV を足す（24 → 32 バイト） | テクスチャの無いモデルと STL でも形状のメモリが3割増える。`Vertex` の手書き箇所も直すことになる |
| 画像の番号をインスタンスに入れ、配列テクスチャ1枚で描く | 大きさの違う画像を1つの配列に置けない。そろえるには全部を拡大か縮小することになる |
| UV の変換をインスタンスに入れる | 全モデルでインスタンスが 144 → 168 バイトになる。束ねる組の小さな定数に置けば、画像つきの描画だけが払う |
| 足した図形に UV を持たせる（立方体は面ごと、球は経度と緯度） | 今の図形の頂点データと、書き出すバイト列が変わる（「同じ文書から同じバイト列」の試験と、既存の書き出しに響く）。STL と UV の無い GLB には別の貼り方が要り、仕組みが2つになる |
| 貼るたびに UV 付きの形状を作って差し替える | 大きな STL に貼れない（足せる形状は1回32 MiB まで）。当たり判定の索引も作り直しになる |
| ファイルの UV を、画像の無い材質でも読んでおく | 貼るときに UV を使えるが、UV を持つすべてのモデルで形状のメモリが増える（頂点あたり8バイト）。下の「分かれ道」 |
| 貼った画像を PNG に書き直して埋め込む | 付帯情報は確実に落ちるが、写真の JPEG が数倍に膨らむ。区画を落とすだけにした |
| 繰り返しを、拡張（`KHR_texture_transform`）で書く | 対応しないソフトで絵の大きさが変わる。箱形は UV に焼くので拡張は要らない |
| 縮小版を GPU で作る | 描画先にできる形式と手順が要り、GPU なしで試験できない。別スレッドの CPU で作る |

## やらないこと（範囲外と理由）
- **金属らしさ・粗さ・法線・発光の絵**: 今の照明は基本色に空と地面の色と1つの光を掛けるだけで、出しても見分けがつかない。照明を作り直す段階で入れる。
- **頂点の色**: 頂点のデータがもう1本要る。画像より使う人が少ない。警告は残す。
- **BLEND**: 奥から順に描く並べ替えが要り、インスタンスのまとめ描きと両立しない。不透明で描く。
- **UV のある部品での繰り返し回数、回転、位置ずらし**: 読んだ値は表示するが、変える部品は置かない（次の段階。書くなら `KHR_texture_transform`）。
- **貼った画像の入れ替えで GPU のメモリを詰め直すこと**: 取り消して使われなくなった画像も、文書を閉じるまで GPU と文書に残る。件数と大きさは詳細パネルに出す。

## 分かれ道（依頼者に聞くもの。既定で進める）
1. **画像の合計の上限**: 既定は 512 MiB（確認用の場面: 2048 画素 × 24 枚 ≈ 510 MiB が縮まずに出る値）。メモリの少ない機械を優先するなら 256 MiB。**計測（要件8）の後に見直す**前提で、512 で進める。
2. **UV を持つが画像の無い GLB の部品に貼るとき**: 既定は箱形の投影（UV を読まないので軽い）。モデルの UV で貼りたいなら、全モデルで UV を読むことになる。既定で進め、要望が出たら「貼るときに、その形状の UV だけをファイルから読む」形で足す。

## 要確認（実装で確かめ、結果を報告する）
- `image` の `jpeg` の機能だけで JPEG の書き出し（試験用の画像の生成）ができるか。できなければ、試験は自分で作った小さな JPEG のバイト列を定数で持つ。
- `image` が、CMYK の JPEG と16ビットの PNG を RGBA8 に直せるか。直せない形式は「読めない」として断る。
- `gltf` クレートの機能 `allow_empty_texture` を足した後、`source` の無いテクスチャを持つ GLB が開き、既存の壊れた GLB の試験が今と同じ理由で断られること（[design-display.md](design-display.md) の「読み込み」）。
- wgpu 30 で、同じ画像を行の範囲に分けて `write_texture` できること、`Rgba8UnormSrgb` が線形補間できること（どちらも見込み。使う前に依存の実物で確かめる）。
- 箱形の投影の、シェーダーの式と `box_uv` の一致（鏡写しの部品、平行投影を含む）。[design-testing.md](design-testing.md) の GPU の試験で決める。
