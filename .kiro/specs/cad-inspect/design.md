# Design Document（入口）

第3段階「受け取ったデータの寸法を確かめる」の設計。要件は [requirements.md](requirements.md) と [requirements-2.md](requirements-2.md)、体験の設計は [ux.md](ux.md)、作業の割り振りは [tasks.md](tasks.md)。第1段階と第2段階の設計（`../model-reviewer/design*.md`、`../editor-foundation/design*.md`）は有効で、この文書群は変える所と足す所だけを書く。判断と理由、境界の約束を書き、処理の逐語訳は書かない。

## Overview
4つを足す。①頂点と辺へ吸着する計測、②軸に沿った断面、③STL の読み込み、④寸法を指定できる図形。どれも「いまある物の上に薄く載せる」形にした。吸着は当たり判定の索引を使い回し、断面は描画の定数の空きに入れ、STL は「バイト列 → `Model`」のクレートを1つ足し、寸法は部品の大きさ（`scale`）として持つ。新しい外部の依存は無い。

### Goals
- 依頼の4項目が実機で動き、数値の出どころ（何に吸着したか、単位の前提）が画面で分かる。
- 部品10万個・数千万三角形で、吸着も断面も、形状と部品の表を送り直さず、常駐メモリを増やさない。
- STL を書き換えず、STL から始めた作業を、規格どおりメートルで読める GLB として渡せる。
- 吸着・断面・STL・単位・寸法の計算を、GPUと画面なしで、3つのOSで試験できる。

### Non-Goals
STEP、境界表現、スケッチ、頂点・辺・面の編集、複数選択、ブール演算、斜めや複数の断面、切り口のふたと輪郭線、角度・半径・面積の計測、辺の中点や円の中心への吸着、別の部品の頂点への吸着、STL の書き出し、インチ、設定の記憶。保留の理由は [design-decisions.md](design-decisions.md)。

## 文書の構成
| 文書 | 内容 |
|---|---|
| [design-snap.md](design-snap.md) | 吸着の候補の探し方、辺の判定、計算量とメモリ、計測の値 |
| [design-section.md](design-section.md) | 断面の描き方、当たり判定、平面の枠と矢印、ほかの表示との組み合わせ |
| [design-stl-units.md](design-stl-units.md) | STL の判定と読み込み、壊れた入力、単位の換算、書き出しとレビュー |
| [design-shapes.md](design-shapes.md) | 寸法を持つ図形、外形の表示、画面の部品とキー、状態欄 |
| [design-testing.md](design-testing.md) | 試験の置き場所、自動操作に足すもの、性能の測り方、外部の STL での確認 |
| [design-decisions.md](design-decisions.md) | 判断の一覧、捨てた案、落とし穴、確かめた事実と未確認、保留 |

## Boundary Commitments
- **この仕様が持つもの**: `core::snap` と `core::section` の公開の型、`Hit` と `TriHit` に足す項目、`core::units::Units`、`ShapeSpec` と `Dims`、`model-reviewer-stl` の入口、`gltf::export::synthesize_base`、`ExportBase`、レビューJSONの `model.stl_unit`、`Command` の追加分、自動操作の追加の手順。
- **持たないもの**: 形状そのものの編集、GLB 以外の書き出し、設定の永続化、部品の表の持ち方。
- **依存の向き**: `app → render / gltf / stl / core`、`render → core`、`gltf → core`、`stl → core`。`stl` と `gltf` は互いに依存しない（つなぐのは `app`）。`core` に wgpu・egui・ファイル形式のクレートを入れない。
- **見直しが必要になる変更**: 描画の定数の並びと大きさ（304バイト）、`SceneRead` の項目、「シーンの単位は、GLB ではファイルの値、STL ではメートル」という約束、レビューJSONの意味。

## Architecture
```
 入力(egui) ─▶ ui/*（入力用の小さな構造体を読み、Command を積む）
                 │ Command（ViewHover・SetSnap・SetSection・SetDisplayUnit・SetStlUnit ほか）
                 ▼
 Session ── doc: Document（足した形状ごとに ShapeSpec）   units: Units   section: Option<SectionPlane>
   │           └─ SceneRead ─▶ core::snap（pick_clipped → 近くの三角形 → 頂点・辺・面）
   ├─ loader ─▶ glTF の印なら gltf::load_glb ／ そうでなければ stl::load_stl（どちらも Model を返す）
   ├─ gpu_view ─▶ render（断面は FrameParams.section → 定数の params.z・w。形状は送り直さない）
   └─ exporter ─▶ 土台が STL なら stl::load_stl → gltf::export::synthesize_base → 既存の build
```
- **シーンの単位は変えない。** GLB はファイルの数値のまま。STL は部品の `scale` に倍率を入れてメートルにする。換算は画面の出入り口（`Units`）だけで行う。
- **吸着は「当たった三角形の近く」だけを見る。** 全部の頂点を走査しない。1回の探索は、既存の当たり判定1回と、当たった形状の索引への問い合わせ数回である。
- **断面は平面1枚の情報を3か所で使う。** 描画（画素を捨てる）、当たり判定（視線の範囲を切る）、画面の部品（枠と矢印）。3つとも `core::section::SectionPlane` を読む。
- **状態の変更は `Command` と `Session::apply` を通る**（第1段階のまま）。吸着・断面・表示の単位は履歴に入れない。寸法の確定は、既存の `CommitLocal`（部品の変形）として履歴に入る。

## File Structure Plan（足すファイルと、変えるファイル）
```
crates/core/src/    snap.rs  section.rs（新規）   bvh.rs pick.rs measure.rs units.rs shapes.rs doc.rs scene.rs review_file.rs lib.rs（変更）
crates/stl/         Cargo.toml  src/{lib,detect,binary,ascii,weld,error}.rs  tests/{parse,malformed,external}.rs（新規のクレート）
crates/gltf/src/    export/synth.rs（新規）  export/mod.rs（変更）
crates/render/src/  plan.rs lib.rs instances.rs pipelines.rs shaders/{frame,scene,mask}.wgsl（変更）
crates/app/src/     session/{snap,section,dims}.rs  ui/{section_bar,section_view,dims,measure_panel}.rs（新規）
                    command.rs loader.rs exporter.rs app.rs gpu_view.rs metrics.rs automation* session/{mod,measure,pick,open,persist,export,edit}.rs
                    ui/{mod,ids,viewport,overlay,measure_view,toolrail,viewbar,inspector,status,help,dialogs}.rs（変更）
crates/app/e2e/     inspect-1.json  stl-1.json  stl-2.json  heavy-inspect.json（新規）
crates/testdata/src/ stl.rs（新規）  main.rs lib.rs（変更）      assets/samples/review-sample.stl（生成して置く）
docs/               stl.md（新規）  glb.md review-json.md verify.md roadmap.md  README.ja.md README.md（変更）   Cargo.toml（members に crates/stl）
```

## 画面（配置と操作表は ux.md）
足す部品は、第2段階と同じく自分のファイルに置いた入力用の構造体を受け取り、`Command` を積む（[design-shapes.md](design-shapes.md) の表）。入力の振り分けは、第2段階の「3 ハンドル」に断面の矢印を並べる（重なったら変形のハンドルが先）。状態欄は幅が足りないとき、右から「格子」「表示の単位」「部品の数」の順に省く。前提の単位と未保存の印は省かない。

## Requirements Traceability
| 要件 | 実現する場所 | 詳細 |
|---|---|---|
| 1 吸着する計測 | core::{snap,bvh,pick}、session::snap、ui::measure_view | design-snap.md |
| 2 計測の値 | core::{measure,units}、ui::measure_panel | design-snap.md |
| 3 断面の表示 | core::section、render（定数とシェーダー）、ui::{section_bar,section_view} | design-section.md |
| 4 断面と当たり判定 | core::{section,pick,snap}、ui::overlay | design-section.md |
| 5・6 STL | crates/stl、app::loader | design-stl-units.md |
| 7 単位 | core::units、session、ui::status | design-stl-units.md |
| 8 STL の書き出しとレビュー | gltf::export::synth、app::exporter、core::review_file、session::{export,persist} | design-stl-units.md |
| 9・10 寸法 | core::{shapes,doc}、session::dims、ui::dims | design-shapes.md |
| 11 軽さと測定 | core::snap、render、app::metrics | design-snap.md、design-testing.md |
| 12 操作と画面 | ui/*、app（キー） | design-shapes.md、ux.md |
| 13 検証と体裁 | 各クレートの試験、app::automation、docs/ | design-testing.md |

## 実装者への注意
- 第1段階の落とし穴13件と、第2段階の「確かめたAPI」「罠と試験の対応」は、そのまま当てはまる。先に読むこと。
- この段階の落とし穴は [design-decisions.md](design-decisions.md) の「落とし穴」にある。「要確認」と書いた項目は、使う前に依存の実物か小さな試験で確かめ、結果を報告する。
