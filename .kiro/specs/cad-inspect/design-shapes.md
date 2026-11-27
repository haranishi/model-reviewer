# 寸法を持つ図形・外形の表示・画面の部品・キー

要件9・10・12。配置と操作表は [ux.md](ux.md)。

## 方針
寸法は**部品の大きさ（`scale`）として持つ**。図形の形状は第2段階のまま1回だけ作り、寸法を変えても形状を作り直さない。寸法の確定は、既にある「数値の確定」（`CommitLocal`）と同じ操作になり、取り消し・書き出し・変形中の描画は、何も足さずに働く。文書が覚えるのは「この形状は、どの図形を、どの大きさで作ったか」だけである。

## 型の約束（`core`）
```rust
// shapes.rs
#[derive(Clone, Copy, Debug, PartialEq)] pub struct ShapeSpec { pub kind: ShapeKind, pub size: f32 }   // build(kind, size) の引数
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dims { Cube { x: f32, y: f32, z: f32 }, Sphere { d: f32 }, Cylinder { d: f32, h: f32 }, Plane { x: f32, z: f32 } }  // シーンの単位
pub const DIM_MIN: f32 = 1.0e-6;   pub const DIM_MAX: f32 = 1.0e9;
/// 大きさ（部品の中の3軸の拡大）から寸法を求める。球の3軸、円柱の X と Z が相対 1e-4 以内でそろっていなければ None。
/// 大きさの成分が0以下か有限でないとき（鏡写しの負の大きさを含む）も None（寸法の欄に負の値を出さない）。
pub fn dims_of(spec: ShapeSpec, scale: Vec3) -> Option<Dims>;
/// 寸法から大きさを求める。図形の種類が合わない、有限でない、DIM_MIN〜DIM_MAX の外なら None。平面の Y は keep_y をそのまま使う。
pub fn scale_for(spec: ShapeSpec, dims: Dims, keep_y: f32) -> Option<Vec3>;
// doc.rs
impl Document { pub fn add_shape_primitive(&mut self, prim: Arc<Primitive>, spec: ShapeSpec) -> PrimId;   // add_primitive ＋ 記録
                pub fn shape_spec(&self, prim: PrimId) -> Option<ShapeSpec>; }                            // 読み込んだ形状と記録の無い形状は None
```
- 寸法は `spec.size × 大きさの成分`。立方体は3軸、円柱は直径が X、高さが Y、球は直径が X、平面は X と Z。
- 複製は同じ形状を指すので、複製した図形の寸法も直せる。文書を作り直す（モデルを開く）と記録は消える。書き出して作業の対象が移っても、文書は同じなので記録は残る（要件9.6）。
- 足した図形は根の直下に置かれ、親を持たない（第2段階の `shape_piece`）。そのため「部品の大きさ ＝ ワールドでの大きさ」が成り立つ。親子の付け替えを入れるときは、この前提を見直す。

## つなぎ方（`session::dims`、`ui::dims`）
- `session::edit::add_shape` は `add_primitive` の代わりに `add_shape_primitive` を呼ぶ。変えるのはこの1行だけである。
- `session::dims::selection_dims(session) -> Option<DimsView>` が、右のパネルの入力を組み立てる。
```rust
pub struct DimsView { pub extent: Option<[f32; 3]>,            // 選択と子孫のワールドの境界箱の大きさ（シーンの単位）。空なら None
                      pub shape: Option<ShapeDims>, pub units: Units, pub doc_rev: u64 }
pub struct ShapeDims { pub dims: Option<Dims>, pub trs: Trs, pub spec: ShapeSpec, pub blocked: Option<&'static str> }
```
  `shape` が `Some` になるのは、選択した部品が子を持たず、描画が1つで、その形状に `ShapeSpec` があるとき。`decompose(local)` が `None`（傾いた拡大）か `dims_of` が `None` なら、`dims = None` と理由（「この図形は拡大や回転で形が変わっているため、寸法では直せません」）を入れる。
- `ui::dims` は、欄の値（表示の単位）を `Units::from_display` でシーンの値へ戻し、`scale_for` で大きさを求め、`trs.scale` だけを差し替えた `Trs` で、ドラッグ中は `PreviewLocal`、確定で `CommitLocal`、Esc で `CancelTransform` を積む。**新しい `Command` は足さない。** 欄の持ち方（選択と文書の版を鍵にした一時メモリ、確定の判定を egui のメモリで行う）は、第2段階の `ui::inspector` と同じにする。
- 受け付けない値（要件9.5）は、`scale_for` が `None` を返す。欄を元の値へ戻し、欄の下に理由を出す（「0より大きい数を入れてください」「大きすぎます」「小さすぎます」）。
- 外形（要件10）は `extent` を3つの数値で出す（有効数字6桁）。`shape` が無いときは、要件10.2の1行を添える。

## 画面の部品（この段階で足す・変えるもの）
| 部品 | 受け取るもの | 積む `Command` |
|---|---|---|
| `ui::toolrail`（変更） | 計測の道具か、吸着が入っているか | `SetSnap(bool)` |
| `ui::measure_view`（変更） | 候補（位置・種類・辺の両端）、置いた点の種類、消えた側か | なし |
| `ui::measure_panel`（新規） | 距離、差、2点の種類、`Units` | `ClearMeasure`（「消す」ボタン） |
| `ui::dims`（新規） | `DimsView` | `PreviewLocal`・`CommitLocal`・`CancelTransform` |
| `ui::section_bar`・`ui::section_view`（新規） | design-section.md | `SetSection` |
| `ui::viewbar`（変更） | 断面が入っているか | `ToggleSection` |
| `ui::status`（変更） | `Units`、`SourceKind` | `SetUnit`・`SetStlUnit`・`SetDisplayUnit` |
`Command` に足すのは `ViewHover { ndc: Option<Vec2> }`・`SetSnap(bool)`・`ToggleSnap`・`SetSection(Option<SectionPlane>)`・`ToggleSection`・`SetDisplayUnit(Option<UnitLabel>)`・`SetStlUnit(UnitLabel)` の7つ。どれも履歴と未保存の印に入らない。

## キー（要件12.2）
| キー | 働き | 備考 |
|---|---|---|
| X | 吸着の入・切（`ToggleSnap`） | 計測の道具でなくても切り替わり、状態欄に「吸着: 入」「吸着: 切」と出す |
| C | 断面の入・切（`ToggleSection`） | モデルが無ければ何もしない |
- どちらも `app.rs` の既存の振り分けに足し、既存の `plain()`（修飾なし）で受ける。「ダイアログ中・ヘルプ中・入力中・パネルの上」では働かせない条件も、そのまま通す。
- Cmd+X・Cmd+C は `egui-winit` が切り取りとコピーの出来事に変える（第2段階で確かめた）。修飾つきは割り当てない。
- 既存のキー（V・G・R・S・M・N、1・3・7・5、Z、A・F・Home、H、Cmd+Z・Cmd+D・Delete）と重ならない。X・C は英字なので、日本語配列でも同じ位置にある。実キーで届くことは、第2段階の数字キーと同じく**未確認**（自動操作はOSのキー配列を通らない）。

## 部品の名前（`ui/ids.rs`。自動操作の指し方）
`measure.snap`、`view.section`、`section.axis.{x,y,z}`、`section.offset`、`section.flip`、`section.handle`、`dims.{x,y,z,d,h}`、`measure.clear`、`status.unit`（既存。STL でも同じ名前）、`status.display_unit`。

## ヘルプと案内
- 操作表に ux.md の8行を足す。「できないこと」として、吸着は当たった部品の中だけ、切り口にふたと輪郭は無い、円柱と球は32分割、STL の単位は仮定、の4つを書く。
- 道具の使い方の一文（状態欄）: 計測は「計測: 角や辺に近づけてクリック（X で吸着を切り替え）」、断面が入っている間に道具の一文が空なら「断面: 矢印をドラッグ、または位置を入力」。
