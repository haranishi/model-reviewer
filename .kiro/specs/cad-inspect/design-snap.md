# 吸着する計測：候補の探し方・辺の判定・計算量・計測の値

要件1・2・11。配置と印の見た目は [ux.md](ux.md)。

## 方針
吸着の候補は、**ポインタの視線が当たった三角形の近く**だけから探す。形状ごとの頂点の表や辺の表は作らない。第1段階の当たり判定の索引（形状ごとの三角形の BVH）に「この点の近くの三角形」を尋ねる関数を1つ足し、それで足りる分だけを見る。常駐メモリは増えない。

## 型の約束（`core`。GPUにも画面にも依存しない）
```rust
// bvh.rs
pub struct TriHit { pub t: f32, pub local_pos: Vec3, pub normal: Vec3, pub triangle: u32 }      // triangle を足す
impl Bvh { /// 境界箱が球（中心 center・半径 radius、形状の中の座標）に触れる三角形の番号を out に足す。
           /// limit を超えたら false を返す（out の中身は途中まで）。
           pub fn triangles_near(&self, center: Vec3, radius: f32, vertices: &[Vertex], indices: &[u32], limit: usize, out: &mut Vec<u32>) -> bool; }
// pick.rs
pub struct Hit { /* 既存の5項目 */ pub prim: PrimId, pub triangle: u32 }                        // 2項目を足す
pub fn pick_clipped(scene: &impl SceneRead, index: &PickIndex, vis: &Visibility, ray: &Ray, clip: Option<&SectionPlane>) -> Option<Hit>;
// pick は pick_clipped(.., None) を呼ぶだけにする（既存の呼び出しと結果は変わらない）
impl PickIndex { pub fn bvh(&self, prim: PrimId) -> Option<&Bvh>; }
// snap.rs
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)] pub enum SnapKind { Vertex, Edge, #[default] Face }
pub struct SnapSettings { pub enabled: bool, pub radius_px: f32 }                                  // 既定は enabled = true
pub struct SnapQuery<'a> { pub camera: &'a OrbitCamera, pub aspect: f32, pub viewport_px: [f32; 2], pub ndc: Vec2,
                           pub settings: SnapSettings, pub clip: Option<SectionPlane> }
pub struct SnapTarget { pub node: NodeId, pub kind: SnapKind, pub local: Vec3, pub world: Vec3,
                        pub edge: Option<[Vec3; 2]> }                                              // 辺のときの両端（ワールド。強調に使う）
pub fn snap(scene: &impl SceneRead, index: &PickIndex, vis: &Visibility, query: &SnapQuery) -> Option<SnapTarget>;
pub const FEATURE_ANGLE_DEG: f32 = 10.0;   pub const MAX_NEAR_TRIANGLES: usize = 512;   pub const EDGE_RADIUS_RATIO: f32 = 0.75;
// measure.rs（MeasurePoint は変えない）
pub struct Measurement { pub first: Option<MeasurePoint>, pub second: Option<MeasurePoint>, pub kinds: [SnapKind; 2] }
impl Measurement { pub fn push_with(&mut self, point: MeasurePoint, kind: SnapKind);   // push は push_with(point, Face)
                   pub fn delta(&self, doc: &Document) -> Option<Vec3>;                // 2点目 − 1点目（ワールド）。delta_with も置く
}
```
`radius_px` は物理画素で、12ポイント × `pixels_per_point`。

## 探す手順（`snap`）
1. `camera.screen_ray_in(ndc, aspect, &scene.bounds())` の視線で `pick_clipped`。当たれば手順3へ。
2. 当たらず、吸着が入っていれば、上下左右へ `radius_px / 2` ずらした4本で `pick_clipped` し、いちばん手前の当たりを使う。どれも当たらなければ `None`。この経路では、面の候補は返さない（ポインタは表面の上に無い）。
3. 吸着が切なら、当たった点を `Face` として返す。
4. 当たった点での「吸着の半径のワールドでの長さ」`r` を求める（`camera.half_height_at(当たった点) × 2 × radius_px / 画面の高さ`。透視と平行の両方で使える関数である）。形状の中の座標での半径は、`1.5 × r` を、部品のワールド行列の3つの列の長さの最小で割る。最小が 1e-20 以下なら手順5の上限超えと同じ扱いにする。
5. `triangles_near(当たった点, 半径, .., MAX_NEAR_TRIANGLES, ..)`。上限を超えたら、対象は当たった三角形1つだけにし、辺の候補は作らない（三角形が画面の数画素より細かい所である。頂点だけに吸着する）。
6. **頂点**: 対象の三角形の頂点をワールドへ移し、断面で消えた側の点と、当たった点から `1.5 × r` より遠い点を除く。画面へ投影してポインタとの距離を求め、`radius_px` 以内でいちばん近いものを候補にする。
7. **辺**: 対象の三角形の3辺それぞれについて、ポインタの視線にいちばん近い辺の上の点（線分と直線の最近点）を求め、頂点と同じ条件で絞る。距離が `radius_px × EDGE_RADIUS_RATIO` 以内のものを近い順に8本まで取り、下の「辺の判定」に通った最初の1本を候補にする。
8. 頂点の候補があれば頂点、無ければ辺、どちらも無ければ手順1の当たり（面）を返す。同じ距離のときは、視点に近いほう、次に三角形の番号の小さいほうを選ぶ（比べるときは `f32::total_cmp`）。
- 返す `local` は、頂点なら形状の頂点の座標そのもの、辺なら両端の座標の間を同じ割合で分けた点にする。逆行列でワールドから戻さない（丸めで辺から外れるのを避ける）。
- 視線の逆変換・投影のどこかで値が有限でなくなったら、その候補を捨てる。

## 辺の判定（要件1.4）
辺の両端の**位置**が同じ（ビットが同じ。`-0.0` は `0.0` とみなす）辺を持つ三角形を、対象の三角形の中から探す。面積0の三角形は数えない。
- 相手が0個 → 縁。辺として扱う。
- 相手が2個以上 → 枝分かれした形。辺として扱う。
- 相手が1個 → 2つの面の法線の角度が `FEATURE_ANGLE_DEG` 以上なら辺として扱う。頂点の並びが逆向きの相手は、法線を裏返して比べる。比べ方は `dot(n1, n2) < cos(10°) × |n1| × |n2|`（`acos` を使わない）。
頂点の番号ではなく位置で比べるので、法線ごとに頂点を分けた形状（立方体）と、頂点を共有しない STL の両方で働く。頂点が辺の途中に乗っているだけの継ぎ目（T字）は相手と認めず、縁として扱う。

## 計算量とメモリの見込み（測った値ではない。測り方は design-testing.md）
| 処理 | 計算量 | 見込み |
|---|---|---|
| `pick_clipped` 1回 | 部品の数に比例（境界箱との交差）＋当たった形状の BVH を1回たどる | 既存の `pick` と同じ。部品10万個で数ミリ秒以内の見込み。**未計測**（第2段階までの記録に、クリックの当たり判定の時間は無い） |
| 表面の外での探索 | 上の最大5回 | 輪郭の外にポインタがある間だけ |
| `triangles_near` | BVH の深さ＋触れた三角形の数（上限512） | 0.1ミリ秒未満の見込み |
| 頂点・辺の評価 | 三角形512個で頂点1,536・辺1,536。辺の判定は最大8本 × 512個 | 0.1ミリ秒未満の見込み |
| メモリ | 探索のたびに `Vec<u32>`（最大512個）を1つ | 常駐の増加は0 |
- **部品の数に比例する走査は残る。** 第1段階の `pick` が部品の境界箱を全部調べる作りだからである。これを毎フレームは行わない。行うのは「計測の道具で、ポインタが動いたフレーム」と「クリック」だけである（要件11.2）。
- 探索が8ミリ秒を超えたら、次の探索まで「かかった時間 × 4」だけ休む（要件1.8）。休んでいる間、候補の印は前のまま残さず消す（古い候補を見せない）。クリックは休まず探す。
- 実測で遅ければ、部品側の階層的な索引（ロードマップの「大きなモデルを軽く扱う」）を先に入れる。`PickIndex` の中に隠れているので、公開の型は変えずに済む。

## つなぎ方（`session::snap`、`ui::viewport`、`ui::measure_view`）
- `ui::viewport` は、計測の道具で、ポインタが3Dビューの上にあり、前のフレームから位置が変わり、ドラッグ中でないときだけ `Command::ViewHover { ndc: Some(..) }` を積む。3Dビューを出たら `ViewHover { ndc: None }` を1回積む。
- `session::snap::hover` は `snap` を呼び、結果を `Session::snap_preview` に置く。前と同じ結果なら描き直しを求めない。かかった時間を性能値「吸着の探索」（直近と最大）に入れる。
- 計測のクリック（`ViewClick`）は、`snap_preview` を使わず、その場で `snap` を呼ぶ（要件1.3）。結果を `Measurement::push_with` に渡す。当たらなければ、今までどおり「計測: 表面をクリックしてください」。
- `ui::measure_view` の入力に、候補（画面上の位置、種類、辺の両端の画面上の位置）と、置いた2点の種類を足す。印は ux.md のとおり。
- 道具を替える、モデルを開く、断面を変える、部品の表が変わる（編集・取り消し）ときは `snap_preview` を消す。

## 計測の値（要件2）
- 距離は今までどおり。差は「2点目 − 1点目」のワールドの成分。値は `Units`（design-stl-units.md）を通して出す。
- 右のパネルの「計測」（`ui::measure_panel`）に、距離・ΔX・ΔY・ΔZ（有効数字6桁）と、「点1: 頂点」「点2: 辺」を出す。3Dビューの線の脇と状態欄は、距離だけ（4桁）のままにする。
- `f32` の精度は7桁前後なので、6桁目は目安である。パネルの説明に「値は単精度の範囲です」と添える。
