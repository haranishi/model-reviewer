# 色を変える

要件3・5。書き出しは [design-export.md](design-export.md)。

## 確かめた事実（この設計の土台）
- `core::scene::Draw` は `{ prim: PrimId, base_color: [f32; 4] }` で、材質の情報は基本色だけである（金属らしさ・粗さ・テクスチャの有無は持たない）。
- `Document` は部品ごとに描画の表を持つ。複製（`duplicate_subtree`）は描画の表の写しを持ち、形状（`PrimId`）だけを共有する。
- 描画はインスタンスごとに色を持つ（`InstanceRaw.color`）。シェーダーは、この色をリニアの値として照明に掛ける。透明度は使わず、常に 1 で描く。
- `SceneRenderer::update_instances(queue, model, nodes)` は、部品の範囲のインスタンスだけを書き直す（変形の確定で使っている）。**いまは行列だけを作り直し、色は前の値（`old.color`）を写している。** このままでは、色を変えても画面が変わらない。
- インスタンスの位置の表（`InstanceList.slots`）は、部品の順、その中は描画の順に並ぶ（`instances::build` が、その順で積んでいる）。部品1つの範囲を引くと、i 番目が i 番目の描画に当たる。
- 基本色は、読み込みが glTF の `baseColorFactor`（リニア）をそのまま入れている。テクスチャと頂点の色は読まず、モデル単位の警告（`LoadWarning::Textures`・`VertexColors`）だけを持つ。

## 方針
- **色は `Draw.base_color` を書き換える。** 新しい表も、材質の型も足さない。部品ごとに描画の表があるので、同じ形状を使う別の部品には及ばない。
- **扱うのは基本色の赤・緑・青だけ。** 透明度は元の値を保つ。金属らしさと粗さは扱わない（`Draw` に無く、描画にも出ない。足すなら、テクスチャの段階で描画と一緒に足す）。
- **`Draw` の形を変えない。** `Draw { .. }` を手で組み立てている箇所が29か所（試験を含む11ファイル）あり、この段階では項目を足す理由が無い。

## 型の約束（`core`）
```rust
// color.rs（新規）
pub fn srgb8_to_linear(rgb: [u8; 3]) -> [f32; 3];      // sRGB の式（0.04045 を境にした2つの式）。f64 で計算して f32 へ
pub fn linear_to_srgb8(rgb: [f32; 3]) -> [u8; 3];      // 0〜1 に収めてから逆の式、四捨五入。有限でなければ 0
pub fn parse_hex(text: &str) -> Option<[u8; 3]>;       // 6桁の16進。前後の空白と先頭の # は許す。大文字小文字を区別しない
pub fn to_hex(rgb: [u8; 3]) -> String;                 // 大文字の6桁（# は付けない）
pub const SWATCHES: [(&str, [u8; 3]); 12];             // 見本の名前と色（下の表）
// doc.rs
impl Document {
    /// 部品とその子孫の、描画の基本色（部品の順、描画の順）。
    pub fn subtree_colors(&self, key: NodeKey) -> Result<Vec<[f32; 4]>, DocError>;
    /// 同じ並びで基本色を入れる。数が合わなければ InvalidPiece、有限でない値があれば NonFinite。返すのは部品の番号の範囲。
    pub fn set_subtree_colors(&mut self, key: NodeKey, colors: &[[f32; 4]]) -> Result<Range<u32>, DocError>;
}
// history.rs
Edit::SetColors { key: NodeKey, before: Vec<[f32; 4]>, rgb: [f32; 3] }   // 進む: 各描画を [rgb, 元の透明度] に。戻る: before を入れる
pub struct Applied { /* … */ pub recolored: Vec<Range<u32>> }            // 色だけが変わった部品の範囲
```
- 約束: `linear_to_srgb8(srgb8_to_linear(c)) == c` が、256通りの値すべてで成り立つ（試験で固定する）。選んだ色が、次に欄へ出るときに同じ値で出る（要件3.11）。
- `Edit::SetColors` は `changes_document()` が真、`changes_review()` が偽。`bytes()` は `before` の長さ × 16 を足す。選んだ範囲が大きくて履歴の上限を超えたら、今の決まり（実行したうえで履歴を空にし、「これより前へは戻せません」）に従う。
- `before` と描画の並びの対応は、履歴が1列であることに頼る（取り消すときの文書は、実行した直後と同じ形をしている）。数が合わなければ `InvalidPiece` で何も変えない。

## つなぎ方（`session::color`、`Command::SetColor`）
```rust
Command::SetColor([f32; 3])              // リニアの赤・緑・青。選んだ部品とその子孫が対象。履歴に入る
// session::color
pub fn set_color(session: &mut Session, rgb: [f32; 3]);
pub fn selection_color(session: &Session) -> Option<ColorView>;     // 右のパネルの入力。選択が無ければ None
pub struct ColorView { pub state: ColorState, pub note: Option<&'static str>, pub locked: bool, pub selection_rev: (u64, u64) }
pub enum ColorState { NoFaces, One([u8; 3]), Mixed { count: u32 } }  // count は 99 で打ち切る（「99色以上」）
```
- `set_color`: 読み込み中・書き出し中は今の `block_edit_if_locked` で断る。変形の途中なら先にやめる。`subtree_colors` を読み、すべての描画がすでにその色（ビット単位）なら何もしない。そうでなければ `Step::new("色の変更", vec![Edit::SetColors { .. }], Some(key))` を今の `execute` に渡す。状態欄は「色の変更を確定しました」。
- `after_applied`: `applied.recolored` を `pending_patches` に足し、描き直しの合図を立てる（変形の `touched` と同じ扱い。構造か表示状態が変わっていれば、今までどおりインスタンスを作り直すので何もしない）。`gpu_view` は、今の部分更新（`update_instances`）で範囲のインスタンスを書き直す。**形状は送らない。当たり判定の表・範囲・三角形の数は変えない。**
- **`render::update_instances` を直す**: 部品ごとに、位置の表の i 番目へ `model.draws(id)[i].base_color` を入れる（行列と色の両方を作り直す）。描画の数と位置の数が合わない部品は、色を前の値のままにして警告を記録する（非表示の部品は位置を持たないので、何もしない。表示に戻すときにインスタンスを作り直すので、新しい色で出る）。関数の形（引数）は変えない。
- `selection_color` は、色の数を数えるのに選んだ範囲の描画をすべて読む。毎フレーム数えないよう、`(選択の鍵, 文書の版)` を鍵にした控えを `Session` に1つ持つ。
- 注意の文（要件5.3）は、`session.model` の警告に `Textures` か `VertexColors` があるときに入れる。モデル単位の判定で、選んだ部品にテクスチャがあるかまでは見ない（`Draw` が持っていないため。部品ごとの判定は、テクスチャの段階で入れる）。

## 画面の部品（`ui/color.rs`）
```rust
pub fn show(ui: &mut egui::Ui, input: &ColorView, out: &mut Vec<Command>);   // 入力用の構造体だけを受け、Session を読まない
```
| 見本（案。sRGB の16進） | | | | | |
|---|---|---|---|---|---|
| 白 `F2F2F2` | 薄い灰 `C8C8C8` | 濃い灰 `6E6E6E` | 黒 `222222` | 赤 `D64545` | 橙 `E08A2E` |
| 黄 `E3C63B` | 緑 `4FA35B` | 青緑 `2FA39A` | 青 `3D7BD9` | 紫 `8B5FC7` | 茶 `8A5A3C` |
- 見本を押すと、その場で `SetColor` を積む。16進と R・G・B は、Enter か欄を離れたときに積む。欄の持ち方（選択と文書の版を鍵にした一時の記憶、確定の判定）は、第2段階の `ui::inspector` と同じにする。
- 読めない値（要件3.6）は積まず、欄を元へ戻し、欄の下に理由を出す（「6桁の16進数を入れてください」「0〜255 の整数を入れてください」）。
- `ColorState::NoFaces`（選んだ範囲に描画が無い）は、欄と見本を押せなくして「この部品には、色を付ける面がありません」と出す。`Mixed` は「複数の色（3色）」と出し、欄を空にする。
- egui の色の部品（つまみと面で選ぶもの）は使わない。動かす間の値が連続して届き、試し塗りと確定を分ける状態が要るためである（保留）。
- 部品の名前（`ui/ids.rs`）: `color.current`、`color.hex`、`color.r`・`color.g`・`color.b`、`color.swatch.0`〜`color.swatch.11`、`color.note`。
- 置き場所は右のパネルの「寸法」の下、「計測」の上。幅が狭いとき（画面の幅 960）は、見本が6列のまま収まる大きさにする（`layout.rs` に1本足す）。

## 見た目に出るもの・出ないもの
| 変えると画面に出る | 画面に出ない（この段階では変えさせない） |
|---|---|
| 面の色、「線」の表示の線の色、「透かし」の色 | テクスチャ、頂点の色、金属らしさ、粗さ、透明度、発光、法線の絵 |
- 選んでいる部品は、選択の強調で少し明るく描かれる（シェーダーが色に 1.35 を掛ける。今の決まり）。**色の欄の見本が、書き出される色である。** 選択を外すと、その色で見える。手順書に1文で書く。
- ファイルに元から入っている色は、利用者が変えるまで読み込んだ値のまま持つ。欄に出す16進は、その値を丸めたものである（出すだけでは書き換えない）。
