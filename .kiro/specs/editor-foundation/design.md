# Design Document（入口）

第2段階「操作画面と編集の土台」の設計。要件は [requirements.md](requirements.md) と [requirements-2.md](requirements-2.md)、体験の設計は [ux.md](ux.md)、作業の割り振りは [tasks.md](tasks.md)。第1段階の設計（`../model-reviewer/design*.md`）は有効で、この文書群は変える所と足す所だけを書く。判断と理由、境界の約束を書き、処理の逐語訳は書かない。

## Overview
読むだけだったアプリに、部品を動かし、簡単な形を足し、結果をGLBへ書き出す機能を足す。いちばん大きな変更は、不変だった `Arc<Model>` の上に、編集できる**文書**（`Document`）を載せることである。形状（頂点とインデックス）は不変のまま共有し、軽い部品の表だけを書き換える。書き出しは、元のGLBのJSONを値として読んで必要な所だけを直し、BINは末尾に足すだけにする。

### Goals
- 依頼の範囲（格子と軸、立方体、投影、表示、道具とハンドル、数値入力、取り消し、追加・複製・削除、計測、書き出し、状態欄）が実機で動く。
- 部品10万個・数百万三角形で、変形中にGPUへデータを送り直さず、構造の変更も1回の操作で終わる。
- 元のGLBの中身を保ったまま、変えた所だけを書き出す。開いたファイルは書き換えない。
- 文書・履歴・書き出し・ハンドルの計算を、GPUと画面なしで試験できる。

### Non-Goals
頂点・辺・面の編集、スケッチと拘束、親子関係の付け替え、複数選択、色と材質の編集、角度の計測、吸着、空からの新規作成、自動保存、設定の記憶、不要なデータを取り除く書き出し、別の作業で書き出したGLBへのレビューの引っ越し。どれも後から足せる形にしてある（[design-decisions.md](design-decisions.md)）。

## 文書の構成
| 文書 | 内容 |
|---|---|
| [design-data.md](design-data.md) | 文書のデータ構造、番号の付け替え、取り消しとやり直し、未保存の印、レビューとの関係 |
| [design-export.md](design-export.md) | 書き出しの方式、保つものと変えるもの、確かめ方、上書きの防止、レビューJSON版2 |
| [design-render.md](design-render.md) | 格子と軸、平行投影、表示の切り替え、変形中の描画、部分更新、足した形状の転送 |
| [design-ui.md](design-ui.md) | 画面の部品、入力の振り分け、ハンドルと立方体、数値入力、キー、自動操作の追加 |
| [design-testing.md](design-testing.md) | テストの置き場所、踏んだ罠と試験の対応、性能の目標と測り方 |
| [design-decisions.md](design-decisions.md) | 判断の一覧、捨てた案と理由、実物で確かめたAPIと未確認の項目 |

## Boundary Commitments
- **この仕様が持つもの**: `SceneRead`（部品の表を読む口）、`Document` と `History` の公開の操作、書き出しの入力（`ExportScene`）と保証、レビューJSON版2、`Command` の追加分、自動操作の追加の手順。
- **持たないもの**: 形状そのものの編集、GLB以外の書き出し、設定の永続化。
- **依存の向きは変えない**: `app → render / gltf / core`、`render → core`、`gltf → core`。書き出しは `gltf` クレートの中に置く（GLBの知識を1か所に集める）。`core` に wgpu・egui・gltf を入れない。
- **見直しが必要になる変更**: `SceneRead` の項目、部品の並び（行きがけ順の連続範囲）、書き出しの「既存の番号を変えない」保証、レビューJSONの意味。

## Architecture
```
 入力(egui) ─▶ ui/*（部品は入力用の小さな構造体を読み、Command を積む）
                 │ Command
                 ▼
 Session ── doc: Document ◀─ History（1回分 = Step。逆の操作で戻す）
   │           │  base: Arc<Model>（形状は不変・共有）＋ added（足した形状）
   │           └─ SceneRead ─▶ 当たり判定・表示状態・レビューJSON・部品の一覧・インスタンス作成
   ├─ gpu_view ─▶ render（全体の作り直し／範囲の部分更新／変形中は行列1つ）
   └─ exporter（別スレッド）─▶ gltf::export（元のバイト列 ＋ ExportScene → 新しいGLB → 読み直して照合）
```
- **形状と構造を分ける。** 重い形状は `Arc<Model>` と `Arc<Primitive>` のまま1回だけGPUへ送る。書き換えるのは部品の表（1件およそ200バイト）だけである。
- **読む側は `SceneRead` だけを見る。** 当たり判定・表示状態・インスタンス作成・レビューJSONは、`Model` と `Document` のどちらでも同じコードで動く。読み込みと転送の経路（第1段階で不具合を直した所）には手を入れない。
- **状態の変更は `Command` と `Session::apply` を通る**（第1段階のまま）。履歴に入る操作は、`apply` の中で `History` へ1回分を積む。
- **ハンドルと立方体は egui の上に描く。** 3Dの描画には入れない。当たり判定がCPUの計算になり、画面なしで試験でき、触れたときの強調で3Dを描き直さずに済む。

## File Structure Plan（足すファイルと、変えるファイル）
```
crates/core/src/   doc.rs  history.rs  gizmo.rs  shapes.rs  measure.rs  units.rs      （新規）
                   scene.rs（SceneRead、Node.local） camera.rs（投影・仰角90°・決まった向き） pick.rs visibility.rs review.rs review_file.rs（変更）
crates/gltf/src/   export/{mod,patch,append,verify}.rs（新規）  build.rs（Node.local を入れる）
crates/render/src/ grid.rs  display.rs  shaders/{grid,wire}.wgsl（新規）  lib.rs instances.rs pipelines.rs upload.rs plan.rs shaders/{scene,mask}.wgsl（変更）
crates/app/src/    exporter.rs  session/{view,edit,export,measure}.rs  ui/{viewcube,viewbar,toolrail,inspector,gizmo_view,measure_view}.rs（新規）
                   command.rs session/mod.rs ほか session/*  app.rs gpu_view.rs main.rs metrics.rs automation*  ui/{mod,ids,viewport,overlay,toolbar,status,tree,details,help,dialogs}.rs（変更）
crates/app/e2e/    view-1.json  edit-1.json  edit-2.json  edit-3.json  measure-1.json  heavy-edit.json（新規）
docs/              glb.md  review-json.md  verify.md  roadmap.md（変更）
```
共有される大きなファイル（`session/mod.rs`・`app.rs`・`command.rs`・`ui/mod.rs`・`gpu_view.rs`・`render/src/lib.rs`）を誰がいつ触るかは、tasks.md の「接合」の作業に集めてある。

## Requirements Traceability
| 要件 | 実現する場所 | 詳細 |
|---|---|---|
| 1 格子と軸 | render::grid、core::camera（間隔） | design-render.md |
| 2 立方体 | ui::viewcube、core::camera | design-ui.md |
| 3 投影 | core::camera、review_file（版2） | design-render.md、design-export.md |
| 4 表示 | render::display、ui::viewbar | design-render.md |
| 5 道具 | ui::toolrail、app（キーと Esc）、Session | design-ui.md |
| 6 変形 | core::gizmo、ui::gizmo_view、session::edit、render（変形中の行列） | design-ui.md、design-render.md |
| 7 数値入力 | ui::inspector、core::gizmo（分解と合成） | design-ui.md |
| 8 取り消し | core::history、session::edit | design-data.md |
| 9 追加・複製・削除 | core::shapes、core::doc、session::edit | design-data.md |
| 10 計測と単位 | core::measure、core::units、ui::measure_view | design-ui.md |
| 11 書き出し | gltf::export、app::exporter、session::export | design-export.md |
| 12 レビューとの関係 | session::export、review_file | design-data.md、design-export.md |
| 13 未保存 | Session、ui::status、ui::dialogs | design-data.md |
| 14 状態欄と案内 | ui::status、ui::help | design-ui.md |
| 15 軽さと測定 | render、gpu_view、app::metrics | design-render.md、design-testing.md |
| 16 検証と体裁 | 各クレートのテスト、app::automation、docs/ | design-testing.md |

## 実装者への注意
- 第1段階の [design-decisions.md](../model-reviewer/design-decisions.md) の「落とし穴」13件は、そのまま当てはまる。先に読むこと。
- この段階で名指ししたAPIは、依存の実物（`~/.cargo/registry/src/` の下）で確かめた。確かめた箇所と、確かめられなかった項目は [design-decisions.md](design-decisions.md) の最後にある。「未確認」と書いた項目は、使う前に実物で確かめ、結果を報告する。
