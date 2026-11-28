# 貼り付け（箱形の投影・文書・履歴・画面）

要件4・5。型は [design.md](design.md)、GPU は [design-display.md](design-display.md)。

## 箱形の投影の式（`core::texture::box_uv`。シェーダーと書き出しが同じ式を使う）
形状のローカルの位置を `p`、三角形の面の法線（頂点の並びが反時計回りに見える側。`cross(b - a, c - a)`）を `n`、形状のローカルの外形の中心を `c`、画像1枚の幅と高さを `(w, h) = box_sides(size, aspect)`（長い辺が `size`。`aspect` は画像の幅 / 高さ）とする。`p` は `p - c` に置き換えて使う。
1. 軸を選ぶ。`|n.x| >= |n.y|` かつ `|n.x| >= |n.z|` なら X。そうでなく `|n.y| >= |n.z|` なら Y。残りは Z。`s` はその成分の符号（0 と、長さ0の法線は正）。
2. X: `(u, v) = (-s * p.z, p.y)`。Y: `(u, v) = (p.x, -s * p.z)`。Z: `(u, v) = (s * p.x, p.y)`。
3. `uv = (0.5 + u / w, 0.5 - v / h)`。サンプラは繰り返し。
- 符号 `s` を掛けるのは、どの面を外から見ても絵が裏返らないようにするためである（文字やラベルを貼る使い方がある）。外形の中心が、どの面でも絵の中心に当たる（原点が中心に無い形状でも絵が割れない）。画面は箱形の描画のインスタンスの法線行列の各行の w に `c` を持つ。
- 画面では `scene_box` のフラグメントが、ローカルの位置の微分から面の法線を求めて同じ式を使う。微分から出る法線の向きは、画面上の回り方と、鏡写しの部品（`FLAG_MIRRORED`）で反転する。**どちらへ直すかの定数は、GPU の試験（[design-testing.md](design-testing.md)）で決める。** 推測で書かない。
- `bake_box` は、三角形ごとに（軸、符号）を決め、（元の頂点の番号、軸、符号）ごとに頂点を1つ作る。位置と法線は元の値をビット単位で写す。三角形の順と、各三角形の頂点の位置は変えない。法線の無い形状は、法線を持たないまま作る。

## 大きさの決め方
- 利用者が決めるのはワールドの長さ `W`（表示の単位で入力し、メートルで持つ）。描画ごとに、部品のワールド行列の拡大の大きさ `k`（3×3 の行列式の絶対値の立方根）で割り、`size = W / k` を `UvMap::Box` に入れる。`k` が 0 か有限でなければ、その描画には貼らない。
- 既定の `W` は、選んだ範囲の外形（`Document::subtree_bounds`）の最も長い辺。0 なら 1。
- 部品を後から拡大すると、絵も一緒に伸びる（`size` はローカルの長さなので）。軸ごとに違う拡大では、絵が軸ごとに伸びる。「1枚の大きさ」の欄は、いまの `size × k` を出す。描画ごとに値が違えば欄を空にし、入れると全部がその値になる。

## 文書と履歴（`core::doc`・`core::history`）
```rust
pub struct DrawLook { pub base_color: [f32; 4], pub tex: Option<TexId> }
impl Document {
    /// 画像を足す。同じハッシュがあれば（土台の画像も含めて）その番号を返す。
    pub fn add_image(&mut self, bytes: Arc<[u8]>, mime: Mime) -> ImageId;
    /// 貼り方を足す。同じ値があればその番号を返す。画像の番号が無ければ InvalidPiece。
    pub fn add_tex_use(&mut self, tex: TexUse) -> Result<TexId, DocError>;
    pub fn subtree_looks(&self, key: NodeKey) -> Result<Vec<DrawLook>, DocError>;     // 部品の順、描画の順
    pub fn set_subtree_looks(&mut self, key: NodeKey, looks: &[DrawLook]) -> Result<Range<u32>, DocError>;
    pub fn held_image_bytes(&self) -> u64;                                            // 貼った画像のバイト数の合計
}
Edit::SetLooks { key: NodeKey, before: Vec<DrawLook>, after: Vec<DrawLook> }
pub struct Applied { /* … */ pub retextured: bool }     // 画像の番号が変わった描画がある（インスタンスを作り直す）
```
- 画像の表と貼り方の表は、足すだけで減らさない。取り消しは、描画の `tex` と色を前の値へ戻すだけである（表の番号は取り消しの前後で変わらないので、履歴が番号を持てる）。使われなくなった画像は、文書を閉じるまで残る。
- `Edit::SetLooks` は `changes_document()` が真。`bytes()` は（`before` と `after` の長さ）× 24。色の変更（`Edit::SetColors`）はそのまま残す。
- 複製は描画の表を写すので、`tex` も写る（画像は共有）。削除と取り消しは、今の部品の出し入れのままで戻る。
- 貼った画像のバイト数の合計は 256 MiB まで。超えるときは貼らずに断る（[ux.md](ux.md) の「上限に空きがありません」と同じ扱い）。

## 付帯情報の除去（`texture::strip_metadata`）
- **JPEG**（区画の並びは ITU-T T.81 の付属書 B に従う）: 始まり（`SOI`）から終わり（`EOI`）まで、区画を1つずつ読む。写すのは、`APP0`（JFIF）、`APP2`（色の設定）、`APP14`、表（`DQT`・`DHT`・`DAC`・`DRI`）、枠（`SOF`）、`DNL`、走査（`SOS` と、それに続く圧縮データ。`FF 00` と `RST` を含み、次の区画の印の手前まで）、`EOI`。落とすのは、`APP1`（EXIF・XMP）、`APP3`〜`APP13`、`APP15`、`COM`。**走査と走査の間にある区画も同じ決まりで選ぶ**（段階表示の JPEG は走査が複数あり、その間にも注釈を置ける）。`EOI` の後ろのバイトは捨てる。知らない印や長さの合わない区画があれば、写さずに断る。
- **PNG**: 必須のチャンクと、`tRNS`・`gAMA`・`cHRM`・`sRGB`・`iCCP`・`sBIT`・`pHYs` を残す。`tEXt`・`zTXt`・`iTXt`・`eXIf`・`tIME` と、知らない補助チャンクを落とす。チャンクの中身と CRC は写すだけで、作り直さない。
- 落とした後のバイト列が `decode` で開けることを、貼る前に確かめる。開けなければ断る。文書が持つのも、ハッシュを取るのも、落とした後のバイト列である。

## つなぎ方（`app`）
```rust
Command::PasteTextureDialog            // ファイル選択の画面を開く（PNG・JPEG）
Command::PasteTexture(PathBuf)         // 選んだ部品へ貼る。自動操作の paste_texture も同じ
Command::RemoveTexture
Command::SetTextureSize(f32)           // ワールドの長さ（メートル）
// session::texture（新規）
pub fn paste(session: &mut Session, path: PathBuf);
pub fn texture_view(session: &Session) -> Option<TextureView>;   // 右のパネルの入力。選択が無ければ None
```
1. `paste`: 選択・描画の有無・読み込み中と書き出し中を確かめ、だめなら理由を状態欄に出して終わる。箱形になる描画を持つ部品が `no_box_nodes`（自分のノード、複製なら写す元）に当たれば断る。選んだ部品の鍵（`NodeKey`）と、**表示中の文書の世代（`shown_generation`）と文書の版（`doc_rev`）**を控え、仕事のスレッド（`texture-paste`）を起こす。
2. スレッド: ファイルの大きさ（64 MiB まで）→ 読む → ファイルそのままの識別（長さと BLAKE3。`ModelIdentity::from_bytes`）→ `sniff` → `strip_metadata` → `dimensions` → 残りの上限に収まる段数（`plan_levels` を1枚で呼ぶ。上限は「合計の上限 − 置いたバイト数」）→ `decode`。結果を1回だけ送る。失敗は `TexError::message_ja`。
3. 受け取ったとき、**世代と版が控えと同じ**で、読み込み中でも書き出し中でもなければ、`add_image` → 描画ごとに `TexUse` を作って `add_tex_use` → `Step::new("画像の貼り付け", vec![Edit::SetLooks { .. }], Some(key))` を実行する。違っていたら「貼る間に文書が変わったため、貼りませんでした」（部品の鍵は文書ごとに 0 から振るので、鍵だけでは別のモデルの部品に当たる。版まで見るのは、取り消しややり直しで同じ鍵の中身が替わるため）。画像は `begin_image` で GPU へ送る（予算つき）。
4. 描画ごとの `TexUse`: いまの `tex` が `UvMap::Vertex` で、その形状が UV を持つなら、その `TexUse` の `image` だけを替えた写し。それ以外は `{ image: Some(..), sampling: 繰り返し・線形・縮小版つき, map: Box { size, aspect }, cutoff: いまの TexUse の cutoff（無ければ None）, baked_box: None }`。色は赤・緑・青を 1 に、透明度はそのまま。
5. `RemoveTexture`: 描画の `tex` を、`cutoff` があれば `image: None` の `TexUse`、無ければ `None` にする（色はそのまま）。`SetTextureSize`: 箱形の描画だけ、`size` を替えた `TexUse` へ差し替える（`aspect` は保つ）。貼り替えは、いまの箱形の `size` を保ち、`aspect` を新しい画像の幅 / 高さにする。どちらも `SetLooks` の1回分（名前は「画像を外す」「画像の大きさ」）。すべての描画がすでにその値なら何もしない。
6. `after_applied`: `retextured` なら `scene_dirty.instances` を立てる。色だけが変わった範囲は、今の `recolored` の部分更新に乗せる。
- **保護**: 貼り付けが成功したら、モデルを開いたときと同じ2つを登録する。①画像のファイルのパス（選んだパスと、リンクを解いたパス）を `opened_files` に、②**ファイルそのままの識別（付帯情報を取り除く前の長さと BLAKE3）を `FileHistory::remember_opened` に**。②があるので、貼った後に画像を別名にしたり移したりしても、書き出しとレビュー保存の保存先にできない（[protect-originals](../protect-originals/design.md) の内容の照合）。文書が持つ「取り除いた後」のハッシュは、この照合には使えない（元のファイルと長さも中身も違うため）。
- **ドロップ**（`session::open::dropped`）: 1つ目のファイルの先頭を読み、`sniff` が画像なら `PasteTexture` に回す。選択が無ければ断る（開く動作に回さない）。GLB・STL は今のまま。
- **キー**: `Action::TexturePaste`（名前 `texture.paste`、「画像を貼る」、まとまりは編集、既定 Cmd+T）、`Action::TextureRemove`（`texture.remove`、既定なし）。表は 42 → 45 行（`DisplayTextures` を含む）。入力中・ダイアログ・ヘルプの間は働かせない（今の決まり）。
- **画面**（`ui/texture.rs`。入力用の構造体だけを受け、`Session` を読まない）: 部品の名前は `texture.state`・`texture.paste`・`texture.remove`・`texture.size`・`texture.note`。置き場所は「色」の下。色の欄の注意（`color.note`）は、選んだ範囲に画像があるときだけ「色は画像に掛け合わされて見えます」。
- **自動操作**: 手順 `paste_texture`（`path`、基準は今の決まり）。`dump_state` に `textures`（枚数・表示・縮小・失敗・バイト数・表示の切り替え）と `selection_texture`（描画ごとに、ハッシュの先頭16桁・`"uv"` か `"box"`・`size`。最大8件と総数）。`wait_idle` は貼り付けの仕事と画像の転送も待つ。

## 壊れやすい所
1. **貼る先のずれ**。復号の間に選択が変わる。貼る先は、押したときの部品（鍵）であって、届いたときの選択ではない。
2. **履歴と表の番号**。表を詰めたり並べ替えたりすると、履歴が持つ `TexId` が別の画像を指す。表は足すだけにする。
3. **色を白にする範囲と閾値**。透明度まで 1 にする、または箱形で貼るときに `cutoff` を落とすと、MASK の部品の抜けが変わり、書き出して開き直した値とも食い違う（写した材質は `alphaMode` を保つので）。赤・緑・青だけを替え、`cutoff` は引き継ぐ。
4. **保護の足し忘れ**。貼った画像と同じ名前へ GLB を書き出せると、利用者の画像が消える。
5. **大きさの換算**。`k` を掛ける側と割る側を取り違えると、mm の STL（`k = 0.001`）で絵が100万倍ずれる。STL に貼る試験で値を固定する。
