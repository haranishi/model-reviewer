# 操作と画面：部品・入力の振り分け・ハンドル・数値入力・キー・自動操作

配置と操作表は [ux.md](ux.md) にある。ここは、実装の境界と、間違えやすい所を書く。

## 画面の部品は、入力用の小さな構造体を読む
第1段階の部品は `&Session` を読んだ。この段階で足す部品は、**自分のファイルに置いた入力用の構造体**を受け取り、`Command` を積む。`Session` の中身が決まる前に並行して作れ、`Session` なしで試験できる。構造体を `Session` から組み立てるのは、接合の作業が行う。
| 部品（新規のファイル） | 受け取るもの | 積む `Command` |
|---|---|---|
| `ui::viewcube` | カメラの方位と仰角、置く矩形 | `SnapView(ViewDir)`、ドラッグは `Orbit` |
| `ui::viewbar` | 表示・投影・格子のいまの値、線が使えるか | `SetDisplay`・`SetProjection`・`SetGrid` |
| `ui::toolrail` | いまの道具、モデルの有無、編集できない理由 | `SetTool`・`AddShape(ShapeKind)` |
| `ui::inspector` | 選択中の部品の位置・回転・大きさ（無ければ理由）、文書の版 | `PreviewLocal(Trs)`・`CommitLocal(Trs)`・`CancelTransform` |
| `ui::gizmo_view` | 道具、中心、軸の向き、カメラ、矩形、掴んでいるハンドル、変化量 | `BeginTransform`・`DragTransform`・`CommitTransform`・`CancelTransform` |
| `ui::measure_view` | 計測の点（画面上の位置）、距離、単位 | なし（描くだけ） |
`Command` に足すのは上の分と、`Undo`・`Redo`・`DuplicateSelected`・`DeleteSelected`・`ToggleProjection`・`ClearMeasure`・`SetUnit(UnitLabel)`・`ExportGlbDialog`・`ExportGlb(PathBuf)`。`Tool` は `Select・Move・Rotate・Scale・Shape・Measure・Annotate`。第1段階の `SetAnnotateMode(bool)` は残し、`Annotate` と `Select` の切り替えとして働かせる（既存の手順と試験が動く）。

## 入力の振り分け（3Dビューの中）
押した位置で、上から順に最初に当たったものが受ける。
1. ダイアログ（出ていれば、ほかは受けない）
2. 上端のボタン、道具の列、図形のボタン、立方体（egui の部品。3Dビューの操作域より後に置くので、重なった所ではこちらが勝つ）
3. ハンドル（移動・回転・拡大の道具で、部品が選ばれているとき）
4. 注釈の印（第1段階のまま）
5. 3Dビュー: ドラッグは視点の回転（Shift や右ボタンで移動）、クリックは道具に応じて選択・注釈の下書き・計測の点
- ハンドルは線なので、矩形の部品として置けない。ポインタが当たりの幅（12ポイント）に入っている間だけ、ポインタの位置に小さな操作域を重ねて、押下とドラッグを受ける。掴んでいる間は、操作域を3Dビュー全体へ広げる。
- 図形の道具を選んでいる間も、クリックは選択のままにする。図形はボタンを押した時点で置かれる。

## ハンドル（`core::gizmo`。GPUにも画面にも依存しない）
```rust
pub enum Handle { MoveX, MoveY, MoveZ, RotX, RotY, RotZ, ScaleX, ScaleY, ScaleZ, ScaleAll }
pub struct GizmoFrame { pub pivot: Vec3, pub axes: [Vec3; 3], pub camera: OrbitCamera, pub viewport_px: [f32; 2], pub size_px: f32 }
pub fn layout(tool: Tool, frame: &GizmoFrame) -> Vec<HandleShape>;        // 画面上の折れ線（矢印・輪・箱つきの線）
pub fn hit(shapes: &[HandleShape], pointer_px: Vec2, tolerance_px: f32) -> Option<Handle>;
pub struct Drag { /* 掴んだ時点の値 */ }
impl Drag { pub fn begin(handle: Handle, frame: &GizmoFrame, ndc: Vec2) -> Option<Self>;
            pub fn update(&self, ndc: Vec2) -> Option<DragState>; }       // DragState { world_delta: Affine3A, readout: Readout }
pub enum Readout { Distance(f32), AngleDeg(f32), Factor(f32) }
pub fn apply_world_delta(parent_world: &Affine3A, world: &Affine3A, delta: &Affine3A) -> Option<Affine3A>;   // 新しい local。有限でなければ None
pub struct Trs { pub translation: Vec3, pub rotation: Quat, pub scale: Vec3 }
pub fn decompose(local: &Affine3A) -> Option<Trs>;   pub fn compose(trs: &Trs) -> Affine3A;                    // 傾いた拡大を含めば None
pub fn euler_deg(rotation: Quat) -> Vec3;            pub fn from_euler_deg(deg: Vec3) -> Quat;
```
- **中心は、選択した部品と子孫の、見えている範囲（ワールドの境界箱）の中心。** 部品の原点にすると、原点が遠くにあるモデル（設計ソフトからの書き出しに多い）でハンドルが画面の外に出る。
- **移動と回転はワールドの軸、拡大は部品の軸。** 移動と回転は、床の軸や立方体の文字と同じ向きなので迷わない。拡大をワールドの軸で行うと、回っている部品に傾いた拡大ができ、位置・回転・大きさに分けられなくなる。向きを切り替える機能は後続とする。
- 移動は「ポインタの視線と軸の直線が最も近づく点」の差、回転は「軸に垂直な平面上での角度」の差、拡大は軸方向の距離の比で求める。視線が軸とほぼ平行で値が定まらないときは `None` を返し、前の値を保つ。
- 親に不均一な拡大があると、ワールドの軸での回転が傾いた拡大を生むことがある。その場合も行列としては正しく動かし、数値の欄を無効にする（要件7.4）。

## 向きの立方体（`ui::viewcube`）
- 8つの角をカメラの回転だけで画面に移し、手前を向いた面（最大3つ）を多角形として描く。面の文字は「前・後・左・右・上・下」。当たり判定は、点が多角形の中にあるかで決める。計算は関数に分け、画面なしで試験する。
- モデルの正面は +Z（第1段階の約束）なので、「前」は +Z 側から見た視点（方位0°・仰角0°）。「上」は仰角90°。
- 自動操作のために、見えている面ごとに、面の中心まわりの小さな矩形を `viewcube.front` などの名前で登録する。

## 数値入力（`ui::inspector`）
- 欄は egui の `DragValue`。値は、選択中の部品と文書の版を鍵にした一時メモリに置き、`Session` へは `Command` でだけ伝える。
- 欄を左右にドラッグしている間は `PreviewLocal`（見た目だけ。変形中と同じ経路）、離したら `CommitLocal`。文字で打ったときは、欄を離れたフレーム（Enter、Tab、ほかの場所のクリック）で `CommitLocal`。Esc は値を戻して `CancelTransform`。
- **欄に入力位置があるかは、egui のメモリで判定する。** `Response::has_focus` は、ウィンドウが前面に無いと常に偽を返す（`egui/src/response.rs`。第1段階で踏んだ）。`DragValue` 自身はメモリで判定している（`widgets/drag_value.rs`）が、前面でないウィンドウでの確定は、実機の自動操作で確かめる。
- 1文字のキーと Cmd+Z は、`ctx.text_edit_focused()` が真の間は働かせない（第1段階の仕組みをそのまま使う）。入力欄の中の Cmd+Z は文字の取り消しになる。

## 図形の追加
- 置く場所は視点の注視点。大きさは `10^round(log10(全体の境界球の半径 / 4))` を1辺（球と円柱は直径、円柱は高さも同じ）にする。モデルの単位が何であっても、見える大きさで出る。
- 形状は `core::shapes` が作る（立方体は面ごとに法線を分ける。球は経度32・緯度16、円柱は32分割）。名前は「立方体」「球」「円柱」「平面」、複製は「<元の名前> コピー」。色は中間の灰色1色。
- 置いたら選択して移動の道具へ切り替える。1回分の操作として履歴に入る。

## 計測と単位
- 計測の点は `(NodeKey, 部品の中の座標)` で持つ。部品を動かすと距離が変わり、部品を削除すると計測を消す。3回目のクリックは、新しい計測の1点目になる。
- 線・両端の点・距離の文字は egui で描く。距離は状態欄にも出す。計測は保存せず、履歴にも入れない。計測の道具の間、状態欄に「計測は保存されません」と添える。
- 単位は `UnitLabel { M, Cm, Mm }`。数値は変えず、添える文字だけを替える。状態欄の「1単位 = m」を押すと順に切り替わる（開くメニューにしない）。作業の間だけの設定で、保存しない。

## キー
| キー | 働き | 備考 |
|---|---|---|
| V・G・R・S・M | 道具（選択・移動・回転・拡大・計測） | G・R・S はすぐ掴む動きにはしない。道具を選ぶだけ |
| 1・3・7／5 | 前・右・上／投影の切り替え | テンキーではなく文字キーの段。日本語配列でこの数字が `Key::Num1` などとして届くかは**未確認**（実キーでの確認が要る） |
| Z／Shift+Z | 面と「面と線」の切り替え／透かしの切り替え | |
| Cmd+Z／Shift+Cmd+Z／Cmd+D | 取り消し／やり直し／複製 | Cmd+C・X・V は egui が別の出来事に変える（`egui-winit` に専用の判定がある）ので使わない |
| Delete・Backspace | 削除 | |
既存の A・F・H・Shift+H・N・Home・Esc は変えない。キーは `app.rs` の1か所で振り分け、「ダイアログ中・ヘルプ中・入力中・パネルの上」では働かせない条件も、第1段階のものをそのまま通す。Cmd の付くキーは、既存の `plain()`（修飾なし）とは別の分岐で受ける。

## 状態欄
左から「ファイル名」「●形の編集 未書き出し」「●レビュー未保存」「道具の使い方の一文、または直前の結果」「選択中の部品名（長ければ省略）」「1単位 = m」「格子 0.1」「部品と三角形の数」「詳細」。幅960で重ならないことを、配置の試験で確かめる。印は記号と言葉の両方で出す。

## 自動操作に足すもの
- 手順: `export_glb`（保存先のパス。ダイアログを通らない）、`drag_widget`（登録した名前の中心から、指定した量だけドラッグ。ハンドルと数値の欄に使う）。入力の注入は既存の仕組み（前の分を `ui` が受け取ってから次を入れる、隠れている間は描画を要求する）を通す。
- `dump_state` に、道具・投影・表示・格子・部品の数・各部品の `local`・履歴の件数・2つの未保存の印・計測の距離を足す。
- 部品の名前（`ui/ids.rs`）: `tool.*`、`shape.*`、`view.display.*`、`view.projection.*`、`view.grid`、`viewcube.*`、`gizmo.<道具>.<軸>`、`inspector.<pos|rot|scale>.<x|y|z>`、`toolbar.export`・`toolbar.undo`・`toolbar.redo`、`status.unit`。
- 新しく出る部品を押す手順は、既存の「位置が2回続けて同じになってから押す」を通る。この段階の部品は開く小窓を使わないが、書き出しの結果と確認のダイアログは小窓なので、この規則に頼る。
