# STL と単位：判定・読み込み・壊れた入力・換算・書き出し・レビュー

要件5〜8。

## 単位の考え方（要件7）
- **シーンの数値は変えない。** GLB はファイルの数値がそのままシーンの数値になる（第2段階のまま）。換算は、画面に出すときと、画面から受け取るときだけ行う。
- **STL は、メートルのシーンに倍率つきで置く。** STL の座標の値は形状にそのまま入れ、部品の `local` に一様な拡大（mm なら 0.001）を入れる。こうすると、STL から書き出した GLB が規格どおりメートルで読め、書き出しの前後でシーンの数値が変わらない（作業の対象が新しい GLB へ移っても、視点・断面・保存した視点を直さずに済む）。
```rust
// core::units（UnitLabel { M, Cm, Mm } と format_distance は残す）
impl UnitLabel { pub fn meters(self) -> f64; }                       // 1.0・0.01・0.001
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Units { pub source: UnitLabel, pub display: Option<UnitLabel> }   // display = None は「ファイルと同じ」
impl Units {
    pub fn shown(self) -> UnitLabel;                                 // display.unwrap_or(source)
    pub fn to_display(self, scene: f32) -> f32;                      // source と shown が同じなら、値をそのまま返す（掛け算をしない）
    pub fn from_display(self, shown: f32) -> f32;
    pub fn format(self, scene: f32, digits: u8) -> String;           // 換算して単位の文字を添える。有限でなければ「—」
    pub fn next_display(self) -> Option<UnitLabel>;                  // None → Mm → Cm → M → None
}
pub fn format_length(value: f32, unit: UnitLabel, digits: u8) -> String;     // digits = 4 は format_distance と同じ文字になる
```
- 換算は `f64` で比を作ってから `f32` へ戻す。`source` と `shown` が同じときに掛け算をしないのは、既定の状態で第2段階と1ビットも違わない数値を出すためである。
- `Session` は `units: Units` と `source_kind: SourceKind { Glb, Stl { unit: UnitLabel } }` を持つ。GLB を開くと `{ source: M, display: None }`。STL を開くと `{ source: M, display: Some(STL の単位) }`。
- 既存の `Command::SetUnit` は「ファイルの単位（前提）」を替える操作として残す（GLB だけ。数値は換算しないので、表示が「ファイルと同じ」なら第2段階と同じ振る舞い）。STL では同じ部品が `SetStlUnit` を積む。`SetDisplayUnit(Option<UnitLabel>)` を足す。

| 換算して単位を添えるもの | 換算しないもの |
|---|---|
| 計測の距離と差、寸法と外形、断面の位置、床の格子の間隔、移動のハンドルの変化量 | 「変形」の位置・大きさ、レビューJSONの中の数値、性能値、自動操作の `dump_state` の既存の項目 |

状態欄の文字: GLB は「1単位 = m」（今までどおり）。STL は「STL = mm（仮定）」。その右に「表示 ファイルと同じ」「表示 mm」。格子は、表示が `None` なら今までどおり「格子 0.1」、そうでなければ「格子 100 mm」。

## STL の入口（新しいクレート `model-reviewer-stl`。依存は `core` と `glam`・`thiserror` だけ）
```rust
pub enum StlKind { Binary, Ascii }
pub struct StlOptions<'a> { pub name: &'a str, pub unit: UnitLabel, pub allow_trailing: bool }
pub fn detect(bytes: &[u8], allow_trailing: bool) -> Option<StlKind>;
pub fn load_stl(bytes: &[u8], options: &StlOptions) -> Result<Model, StlError>;
pub enum StlError { NotStl, TooShort, CountMismatch { claimed: u64, fits: u64 }, BadNumber { triangle: u64 },
                    Syntax { line: u64 }, Truncated, NoTriangles, TooLarge }      // message_ja() を持つ
pub const STL_COLOR: [f32; 4] = [0.72, 0.72, 0.72, 1.0];   pub const COORD_LIMIT: f32 = 1.0e18;
```
### 判定の順（`detect`）
1. 先頭の BOM と空白を飛ばし、`solid` の次が空白か行末で、かつ先頭4,096バイトの中に `facet` か `endsolid` があれば ASCII。
2. そうでなく、84バイト以上で、80バイト目からの `u32`（リトルエンディアン）を `n` として `84 + 50 × n` がファイルの大きさと等しければバイナリ。小さくて `allow_trailing` が真ならバイナリ（余りは警告）。計算は `u64` で行う。
3. どちらでもなければ `None`。
`solid` で始まるバイナリ（実在する）は、1で `facet` が見つからないので2へ進む。`app::loader` は、`glTF` の印が無いとき、`detect` が `Some` か拡張子が `.stl` なら `load_stl` を呼ぶ（拡張子が `.stl` なら `allow_trailing` を真にし、失敗の理由を具体的に出す）。どちらでもなければ「GLB でも STL でもありません」。

### 読み方
- **バイナリ**: 三角形1つは50バイト。法線の12バイトを飛ばし、`f32` を9個読み、付属の2バイトを飛ばす。確保する量は「実際に収まる三角形の数」で決める（要件6.2）。
- **ASCII**: 行に分け（`\n`・`\r\n`・`\r`）、行を空白で語に分ける。キーワードは大文字小文字を区別しない。`solid` の行は名前ごと読み飛ばす。`facet normal` の3つの数は、数として読めることだけ確かめる。`outer loop` の中の `vertex` が3つでなければ `Syntax`。面の途中や `endsolid` の前でファイルが終われば `Truncated`。2つ目以降の `solid` は同じ部品に足し、数を警告にする。
- **数の検査**: 座標が有限でない、または絶対値が `COORD_LIMIT` を超えたら `BadNumber`。Rust の `f32` の文字列の読み取りは `nan`・`inf`・`infinity` と、大きすぎる数（無限大になる）を成功として返す。**読めた後に必ず有限かを見る**（小さなプログラムで確かめた。試験でも固定する）。上限を 1e18 にしたのは、長さの2乗が `f32` で無限大にならない範囲に収めるためである。
- **頂点をまとめる**（`weld.rs`）。各座標の `-0.0` を `0.0` にそろえ、3成分のビットを鍵にして同じ位置の頂点を1つにする。標準の `HashMap` は表ごとに `RandomState` の鍵を持つ。頂点番号は入力に初めて現れた順に振るので、同じSTLを読めば頂点と番号の並びも同じになる。頂点の法線は0で埋め、`has_normals = false` にする。描画では面ごとに法線を求める。面積0の三角形も残す。
- **できあがる `Model`**: 部品1つ（名前は `options.name`、`gltf_index` は 0、`local` は `unit.meters()` の一様な拡大）、描画1つ（色は `STL_COLOR`）、形状1つ。`LoadWarning` に `StlSolidsMerged { count: u32 }` と `StlTrailingBytes { bytes: u64 }` を足す。法線を導くことは STL では当たり前なので、`DerivedNormals` は出さない。

### メモリの見込み（測った値ではない）
三角形 T 個のバイナリ（ファイルは 50T バイト）で、頂点をまとめた後がおよそ T/2 個になる閉じた形の場合: 読んだバイト列 50T ＋ 形状（頂点 24 × T/2 ＋ 番号 12T ＝ 24T）＋ まとめる表（およそ 16 × 頂点の数 × 2）。最大でファイルの大きさの2倍弱、読み終えるとバイト列と表を解放して 24T が残る。まとめない場合（84T）の3分の1以下になる。ファイル全体を一度に読むのは GLB と同じで、ファイルより大きなメモリが要ることは変えていない。

## 書き出し（要件8.1〜8.3）
```rust
// gltf::export（synth.rs）
/// 部品1つ・描画1つ・形状1つの Model から、最小の GLB を作る。同じ入力からは同じバイト列になる。
pub fn synthesize_base(model: &Model) -> Result<Vec<u8>, ExportError>;      // 形が違えば InvalidScene、4GiB 超は TooLarge
// app::exporter
pub enum ExportBase { File, Stl { unit: UnitLabel, name: String } }         // ExportRequest に base_kind として足す（既定は File）
```
- 土台が STL のとき、書き出しのスレッドは「STL を読み直す → 大きさとハッシュを照合 → `load_stl` → `synthesize_base`」で土台のバイト列を作り、あとは第2段階の `build`・`write_container`・照合へ渡す。土台のバイト列を常駐させない方針（第2段階）を保つ。
- 合成する GLB: BIN は位置（`f32` × 3）と番号（`u32`）。JSON は `asset`（`version` と `generator: "model-reviewer"`）、シーン1つ、ノード1つ（名前、`mesh`、倍率が1でなければ `scale`）、メッシュ1つ（`POSITION` と `indices`、三角形リスト）、材質1つ（基本色と、足した図形と同じ `metallicFactor: 0`。ほかのソフトで金属に見えないようにする）、アクセサ2つ（位置には `min`・`max`）。`NORMAL` は書かない。
- **約束**: `load_glb(synthesize_base(m))` は、部品の数・名前・`local`（相対 1e-6 以内）・位置と番号のバイト・色が `m` と同じになる。これが崩れると、文書の部品と形状の番号が土台とずれる。
- 書き出しが終わると `source_kind` は `Glb` になり、次の書き出しは新しい GLB を土台にする。`units` は変えない（シーンは最初からメートルなので、数値は動かない）。結果の一覧に、`app` が1行足す:「STL から GLB を作りました。座標の値は元のまま（mm）で、部品の大きさ 0.001 でメートルに合わせています」。開き直すと「法線の無い形状 1 件」の簡略表示が出る（事実なので、そのままにする）。

## レビューJSON（要件8.4〜8.6。版は上げない）
- `ModelRef` に `stl_unit: Option<UnitLabel>` を足し、`model` の中へ `"stl_unit": "mm"`（`"cm"`・`"m"`）として書く。GLB では項目ごと書かないので、GLB のレビューは今までと同じバイト列になる。
- 版を上げない理由: 座標の意味（部品の中の座標）が変わらず、前の版のアプリは知らない項目を無視するからである。前の版のアプリが STL 用のレビューを読むと、STL を開けずに「GLBではありません」と出して止まる（誤った復元はしない）。
- 読むとき、`stl_unit` が無ければ mm、3つ以外の文字なら `InvalidValue`。復元でモデルを読み込むときは、`loader` へ `LoadOptions { stl_unit }` を渡す。
- STL の部品名はファイル名に由来する。ハッシュとバイト数が一致し、`stl_unit` があって部品が1つなら、改名後も番号0の部品に注釈と表示状態を戻す。
- 読み直しと保存先の照合には、開いたときか書き出したときの実パスを `PathBuf` で保持する。画面とレビューJSONに載せるときに文字列へ変換する。
- `SetStlUnit` は、同じパスを新しい単位で開き直す。未保存があれば、別のモデルを開くときと同じ確認を通る。開き直すと、履歴・断面・計測は消える（モデルを開いたときと同じ）。
