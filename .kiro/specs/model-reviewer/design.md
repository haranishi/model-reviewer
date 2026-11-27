# Design Document（入口）

model-reviewer 初版の設計。要件は [requirements.md](requirements.md) と [requirements-2.md](requirements-2.md)、体験の設計は [ux.md](ux.md)、作業の割り振りは [tasks.md](tasks.md) と [tasks-2.md](tasks-2.md)。この文書は判断と理由、境界の約束を書く。関数の一覧や処理の逐語訳は書かない。

## Overview
GLBを開いて確認し、部品の表示切り替え・注釈・視点保存をして、結果を別のJSONに残すデスクトップアプリを作る。使うのは、重い制作ソフトを立ち上げずにモデルを確かめて指摘したい人である。初版は読むだけのツールだが、後で制作ソフトへ育てる前提で、形状・カメラ・当たり判定・レビュー状態を、GPUにも画面にも依存しないコアへ分ける。

### Goals
- 依頼の必須機能（開く・表示・カメラ・選択と表示切り替え・注釈・視点・保存と復元）が実機で動く。
- 無駄な処理をしない構造にする。形状はGPUに1回だけ送り、3Dは変化のあったフレームだけ描き直す。
- 人のマウスを動かさずに、一連の操作を自動で検証できる。GPUの無い環境でも論理の大半を検証できる。

### Non-Goals
- 形状編集、テクスチャ、アニメーション再生、STEP等の他形式、プラグイン基盤、取り消し／やり直し。
- 他ソフトとの速度比較の主張。

## 文書の構成
| 文書 | 内容 |
|---|---|
| [design-contracts.md](design-contracts.md)、[design-contracts-io.md](design-contracts-io.md) | クレートと作業パッケージの境界になる型と関数（コア／読み込み・描画） |
| [design-app.md](design-app.md) | アプリ層の構成、コマンド、再描画の方針、フォント、性能値の定義、自動操作 |
| [design-render.md](design-render.md) | 座標系、カメラ、描画方式、深度、リサイズ、強調表示、GPU資源と上限 |
| [design-loading.md](design-loading.md) | 読み込みの流れ、世代番号、未対応の検出一覧、法線なし、エラー |
| [design-review.md](design-review.md) | 当たり判定、注釈の位置、レビューJSON v1、モデルの識別と不一致時の方針 |
| [design-testing.md](design-testing.md) | テスト方針、必須テスト、サンプルGLBの仕様、検証コマンド |
| [design-decisions.md](design-decisions.md) | 依存と選定理由、ライセンス、捨てた案、実物のソースで確かめた落とし穴 |

## Boundary Commitments
- **この仕様が持つもの**: レビューJSON v1 の形式、コアの公開型、未対応GLBの判定基準、性能値の定義、自動操作スクリプトの形式。
- **持たないもの**: 形状の編集と書き出し、GLB以外の読み込み、設定の永続化（ウィンドウ位置やパネル幅）、多言語対応。
- **依存の向き**: `app → render / gltf / core`、`render → core`、`gltf → core`、`testdata` はテスト用。`core` は wgpu・egui・ファイル形式のクレートに依存しない。この向きを逆にする変更は設計の見直しを要する。
- **見直しが必要になる変更**: レビューJSONの項目や意味の変更、`Model` のノード順序の規則、座標系、wgpu と egui の版の組み合わせ。

## Architecture
```
                ┌────────────────────── crates/app（実行ファイル model-reviewer）──────────────────────┐
 入力(egui) ──▶ │ ui/* ──Command──▶ Session ──▶ ReviewState / OrbitCamera（coreの型）                   │
 自動操作 ─────▶│ automation（入力を注入）      loader（別スレッド, 世代番号）   metrics   fonts/theme   │
                └──────┬──────────────────────────────┬───────────────────────┬────────────────────────┘
                       ▼                              ▼                       ▼
              crates/render                     crates/gltf              crates/core
              wgpuで画像へ描く                  GLB → Model              Model・カメラ・当たり判定・
              （egui非依存）                    未対応の検出             表示状態・注釈・レビューJSON
```
- **状態の流れは一方向**にする。画面の部品は `Session` を読むだけで、変更は `Command` を積む。`Session::apply` だけが状態を書き換える。キーボード・ボタン・自動操作・テストが同じ入口を通るので、画面なしで機能を検証できる。
- **描画はオフスクリーンの画像へ行い、eguiには画像として渡す**（理由は design-render.md）。当たり判定はCPUで行う。この2つにより、GPUが無くても選択・注釈・保存の流れが動く。

### Technology Stack
| 層 | 採用と版 | 役割 |
|---|---|---|
| 言語 | Rust 2024 edition、最小対応 1.95（eframeの要求）。`rust-toolchain.toml` で 1.99.0 に固定 | 全コード |
| 画面 | eframe / egui / egui-wgpu 0.37.0、egui_system_font_provider 0.37.0 | ウィンドウ、UI、日本語フォント |
| GPU | wgpu 30.0.2（依存木に1版だけ） | 3D描画 |
| 読み込み | gltf 1.4.1（`import` 機能なし） | GLBの容器とJSON、アクセサの読み取り |
| 数学 | glam 0.34.3 | ベクトル、行列、カメラ行列 |
| 保存 | serde 1 / serde_json 1、blake3 1.8.7 | レビューJSON、モデルの識別 |
| その他 | rfd 0.18.0、memory-stats 1.2.0、bytemuck 1.25、thiserror 2、image 0.25（PNGのみ）、log 0.4 | ダイアログ、メモリ、型変換、エラー、画像保存 |
| テスト | egui_kittest 0.37.0（`eframe`・`wgpu`・`snapshot` 機能） | 画面なしの自動テスト |

## File Structure Plan
```
Cargo.toml  Cargo.lock  rust-toolchain.toml  LICENSE-MIT  LICENSE-APACHE  README.md  CONTRIBUTING.md
assets/samples/      review-sample.glb と出典・ライセンスの記録
crates/core/         model-reviewer-core   math scene camera bvh pick visibility review review_file identity error
crates/gltf/         model-reviewer-gltf   inspect（未対応の検出） build（Model組み立て） error
crates/render/       model-reviewer-render plan（上限の検査） upload instances targets pipelines outline shaders/*.wgsl
crates/testdata/     model-reviewer-testdata  glb_writer shapes sample broken stress、生成用の実行ファイル
crates/app/          model-reviewer        app command loader metrics fonts theme gpu_view automation  tests/  e2e/*.json
                       session/{mod,open,camera,parts,pick,notes,persist}  ui/{mod,ids,dialogs,toolbar,viewport,overlay,tree,annotations,views,status,empty,details,help}
docs/roadmap.md
```
`app` はライブラリと実行ファイルの両方を持つ。統合テストが `App` を直接組み立てるためである。`session/` と `ui/` を細かく分けているのは、作業パッケージの担当ファイルを重ねないためで、割り振りは tasks.md にある。

## Requirements Traceability
| 要件 | 実現する場所 | 詳細 |
|---|---|---|
| 1 入力と未対応 | gltf::inspect / gltf::build | design-loading.md |
| 2 開く | app::loader、Session、ui::status | design-loading.md、design-app.md |
| 3 表示 | render、core::scene | design-render.md |
| 4 カメラ | core::camera、ui::viewport | design-render.md、ux.md |
| 5 選択・表示切替 | core::pick / visibility、ui::tree、render（輪郭） | design-review.md、design-render.md |
| 6 注釈 | core::review、ui::annotations、ui::viewport | design-review.md |
| 7 視点 | core::review、ui::views | design-review.md |
| 8 保存・復元 | core::review_file / identity、Session、ui::dialogs | design-review.md |
| 9 軽量 | render（共有・分割転送）、app（再描画の方針） | design-render.md、design-app.md |
| 10 性能値 | app::metrics、ui::details | design-app.md |
| 11 画面 | app::ui、fonts、theme | ux.md、design-app.md |
| 12 検証 | testdata、各クレートのテスト、app::automation | design-testing.md |
| 13 公開の体裁 | README、LICENSE、CONTRIBUTING | design-decisions.md |

## 実装者への注意
設計で名指ししたAPIは 2026-11-26 に `~/.cargo/registry/src/` の実物で確かめた。記憶と違う箇所が多いので、書く前に [design-decisions.md](design-decisions.md) の「落とし穴」を読むこと。
