# 断面：描き方・当たり判定・枠と矢印・ほかの表示との組み合わせ

要件3・4・11.3。配置は [ux.md](ux.md)。

## 方針
平面は**ワールドの軸に垂直な1枚**だけにする。描画は、消す側の画素をフラグメントシェーダーで捨てる。平面の情報は、描画の定数（304バイト）の空いている2つの数に入れるので、定数の大きさも、形状と部品の表も変わらない。位置を動かしている間にGPUへ送るのは、毎フレームの定数1つだけである。

## 型の約束（`core::section`。GPUにも画面にも依存しない）
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Axis { X, Y, Z }
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionPlane { pub axis: Axis, pub offset: f32, pub flipped: bool }   // offset はワールドの座標（シーンの単位）
impl SectionPlane {
    pub fn centered(axis: Axis, bounds: &Aabb) -> Self;        // 境界箱の中央。flipped = false
    pub fn normal(&self) -> Vec3;                              // 残す側から消す側へ向く。flipped = false なら軸の正の向き
    pub fn cuts(&self, world: Vec3) -> bool;                   // 消す側か（平面の上の点は消さない）
    pub fn ray_range(&self, ray: &Ray) -> Option<(f32, f32)>;  // 視線が残す側にある t の範囲（0 以上）。全部が消す側なら None
    pub fn clamped(self, bounds: &Aabb) -> Self;               // offset を境界箱の範囲に収める。有限でなければ中央
    pub fn frame_corners(&self, bounds: &Aabb) -> [Vec3; 4];   // 境界箱を切った長方形の4つの角（枠を描く）
    pub fn code(&self) -> f32;                                 // 定数に入れる符号つきの番号: X ±1、Y ±2、Z ±3。正は「軸の正の側を消す」
}
/// 矢印のドラッグ。掴んだ時点の平面と視線から、いまの視線での新しい offset を返す。
/// 軸の直線（枠の中心を通る）と視線が最も近づく点の差で求める。視線が軸とほぼ平行なら None（前の値を保つ）。
pub fn drag_offset(start: &SectionPlane, bounds: &Aabb, start_ray: &Ray, now_ray: &Ray) -> Option<f32>;
```
`cuts` と、シェーダーの判定は同じ式にする: `(座標の軸成分 − offset) × 符号 > 0` なら消す。

## 描画（`render`）
- `FrameParams` に `section: Option<SectionPlane>` を足す（既定は `None` で、第2段階と同じ絵）。`SceneDirty` に `section` を足し、変わったフレームだけ描き直す。
- 定数 `params`（`vec4<f32>`）は、いま x（格子の間隔）と y（不透明度）だけを使っている。**z に `offset`、w に `code()`（断面なしは 0）を入れる。** `FrameUniform` の大きさ（304バイト）と、その大きさを確かめている試験は変えない。
- `frame.wgsl` に `fn section_cut(p: vec3<f32>) -> bool` を足す。`scene.wgsl` の `fs_main`・`fs_line` と `mask.wgsl` の `fs_main` で、真なら `discard` する。`mask.wgsl` には、頂点シェーダーからワールドの座標を渡す項目を足す。これで、面・線・透かし・選択の輪郭のすべてが同じ平面で切れる。変形中の差分の行列は、ワールドの座標を作る前に掛かっているので、動かしている最中の部品も正しく切れる。
- `discard` は、`scene.wgsl` が微分（`dpdx`・`dpdy`）を求めた**後**に置く（微分は一様な制御の流れの中でしか使えない。naga が `discard` の後の微分をどう扱うかは**要確認**なので、順番で避ける）。
- **切り口の色**: 断面が入っていて、表示が「面」「面と線」のとき、裏を向いた面（`@builtin(front_facing)` が偽）を、照明なしの1色（リニアで sRGB #D9822B 相当）で塗る。面の向きによる間引きは今も行っていない（`cull_mode: None`）ので、パイプラインは増えない。「線」と「透かし」では塗らない。
- **鏡写しの部品**: ワールド行列の行列式が負の部品は、表と裏の判定が逆になる。`instances::build` が、行列式が負のときに印 `FLAG_MIRRORED = 2`（既存の `FLAG_DERIVE_NORMAL = 1` と同じ場所）を立て、シェーダーで判定を反転する。変形中の差分の行列が鏡写しになる場合（拡大のハンドルを負まで引く）は、確定まで色が逆になることを許す。
- 床の格子と軸（`grid.wgsl`）は切らない。断面の枠と矢印は egui で描く（下）。
- GPUの追加の機能は使わない（`discard` と `front_facing` は WGSL の標準）。クリップ距離の機能（`Features::CLIP_DISTANCES`）は使わない。対応しない環境の分岐を作らずに済む。

### できないこと（要件3.5。ヘルプと `docs/` に書く）
- 切り口にふたをする面と輪郭線は無い。見えるのは「奥にある面の裏側」である。中が詰まった物体でも、殻として見える。
- 閉じていない形状（平面の図形、1枚の板）と、面の向きがそろっていない形状は、切っていない所でも裏側に切り口の色が出る。
- 切り口の上の点は形状の面ではないので、計測の点を置けるのは、切り口の縁（残った面の辺と頂点）までである。

## 当たり判定（`core::pick::pick_clipped`）
- `ray_range` で、視線が残す側にある範囲 `(t0, t1)` を求める。`None` なら当たりなし。
- 当たりは「`t0` より手前を除き、`t1` より奥を除いた中で、いちばん手前」とする。実装は、視線の始点を `origin + dir × t0` へずらして既存の探索を行い、`t1 − t0` を上限にし、返す `t` と `world_pos` を元の視線の値へ戻す。三角形が平面をまたいでいても、当たった点が残す側なら当たりになる。
- 切って見えた裏側の面にも当たる（`ray_triangle` は両面を見る）。選択・計測・注釈は、画面に見えている面と同じ面に付く。注釈の法線は、当たった面の向きのままにする。
- 吸着の候補は、`cuts` が真の点を除く（design-snap.md の手順6・7）。辺が平面をまたぐときは、最近点が残す側にあるときだけ候補にする。
- 「全体表示」「選択を表示」「ハンドルの位置」は断面を見ない（要件4.4）。

## 画面の部品
| 部品 | 受け取るもの | 積む `Command` |
|---|---|---|
| `ui::viewbar`（変更） | 断面が入っているか | `ToggleSection` |
| `ui::section_bar`（新規） | 平面、位置の範囲（境界箱）、`Units` | `SetSection(Some(plane))` |
| `ui::section_view`（新規） | 平面、境界箱、カメラ、矩形、掴んでいるか | `SetSection(Some(plane))`（ドラッグ中は毎フレーム） |
- `ToggleSection` は、切のとき `SectionPlane::centered(Axis::Z, 見えている全体の境界箱)` を入れ、入のとき `None` にする。切ってからもう一度入れると、前の軸と位置を覚えている（作業の間だけ）。
- `SetSection` は `clamped` を通してから置く。軸を替えたら、位置は新しい軸の中央にする（`section_bar` が組み立てる）。
- 位置の欄は `DragValue`。表示の単位で出し、`Units::from_display` でシーンの値へ戻す。確定の判定は、第2段階の数値の欄と同じく egui のメモリで行う（`has_focus` を使わない）。断面は履歴に入れないので、ドラッグ中も確定も同じ `SetSection` でよい。
- 枠は `frame_corners` を投影した破線。視点の後ろに回った角がある辺は描かない。矢印は枠の中心から法線の両方向へ、画面上で約60ポイント。当たりは線から12ポイントで、第2段階のハンドルと同じ「当たりに入っている間だけ小さな操作域を重ねる」やり方を使う。
- 消えた側にある注釈の印と計測の点は、`ui::overlay` と `ui::measure_view` が不透明度40%で描き、操作域を置かない（要件4.3）。判定は `cuts(ワールドの位置)`。

## ほかの状態との関係
- 断面は `Session::section: Option<SectionPlane>` と `section_memory: SectionPlane`（切っている間の控え）で持つ。履歴・未保存の印・レビューJSONに入れない。モデルを開く・閉じると両方を消す。
- 編集で境界箱が変わったら、次に `SetSection` が来たときに収め直す。編集のたびに位置を動かすことはしない（見ている切り口が勝手に動かないようにする）。
- 断面を変えたら `snap_preview` を消し、`scene_dirty.section` を立てる。部品の表もインスタンスも作り直さない。
