# Design Document（入口）

第5段階「作るための基本」の設計。要件は [requirements.md](requirements.md)、体験の設計は [ux.md](ux.md)、作業の割り振りは [tasks.md](tasks.md)。第1〜第4段階の設計は有効で、この文書群は変える所と足す所だけを書く。判断と理由、境界の約束を書き、処理の逐語訳は書かない。

## Overview
2つを足す。①**新規作成**: 部品が0個の空のモデルを、読み込んだモデルと同じ受け渡しで表示し、書き出しでは「空の土台」を元のファイルの代わりに使う。②**色の変更**: 文書がすでに部品ごとに持っている描画の表（形状の番号と基本色）の、基本色だけを書き換える。書き出しでは、元の材質とメッシュの JSON を写して足す。どちらも、今ある仕組み（文書・履歴・インスタンスの部分更新・書き出しの「足すだけ」）の上に乗り、新しい外部の依存は無い。

### Goals
- ファイルを1つも持たずに、起動 → 新規作成 → 図形 → 色 → 書き出し、までが通る。
- 色を変えても、形状のデータはGPUにもファイルにも増えない。変えていない部品の JSON は元のまま。
- 元の材質を壊さない。書き出したファイルを開き直すと、文書と同じ色になる（照合で確かめる）。
- 画面で変えられるのは、画面に出るものだけ（基本色）。
- 論理は GPU と実機の画面なしで、3つのOSで試験できる。

### Non-Goals
テクスチャの表示・貼り付け、金属らしさ・粗さ・透明度・発光、面ごとの色、材質の一覧、色のつまみ（連続の試し塗り）、新規の文書のレビュー保存、使われなくなった材質の掃除、glTF（.gltf と外部ファイル）への書き出し。保留の理由は [design-decisions.md](design-decisions.md)。

## 文書の構成
| 文書 | 内容 |
|---|---|
| [design-new.md](design-new.md) | 新規作成: 空のモデル、表示までの経路、元の形式「新規」、単位と初期視点、未保存の確認、レビュー、画面の入口 |
| [design-color.md](design-color.md) | 色: 文書と履歴の型、色の換算、`Command`、インスタンスの更新、右のパネルの部品、注意の文 |
| [design-export.md](design-export.md) | 書き出し: 空の土台、色の変わった部品の材質とメッシュの写し、照合に足すもの、結果の一覧 |
| [design-texture-outlook.md](design-texture-outlook.md) | 次の段階（テクスチャ）で決めるべきことの見立て。設計ではない |
| [design-testing.md](design-testing.md) | 試験の置き場所、自動操作に足すもの、進行役が実機で確かめること |
| [design-decisions.md](design-decisions.md) | 判断の一覧、捨てた案、落とし穴、確かめた事実と要確認、保留、依頼者に聞く分かれ道 |

## Boundary Commitments
- **この仕様が持つもの**: `core::scene::Model::empty`、`core::color`（色の換算）、`Document` の色の読み書き、`Edit::SetColors` と `Applied::recolored`、`render::SceneRenderer::update_instances` が色も書き直す約束、`gltf::export::empty_base` と色の写しの決まり、`ExportNote::Recolored`、`app::exporter::ExportBase::Empty`、`SourceKind::New`、`Command::NewDocument`・`SetColor`、右のパネルの「色」と部品の名前、自動操作の状態に足す項目。
- **持たないもの**: キーの表と振り分け（shortcuts-guide。Cmd+N は、この仕様が今の振り分けに1つ足し、第4段階の M1 が表へ移す）、キーで動かす操作、描画の式（照明・シェーダー・頂点とインスタンスの型は変えない）、レビューJSONの形式、読み込み（`load_glb` はメッシュの無いファイルを今までどおり断る）。
- **依存の向き**: 変えない。`core` は egui も GPU も知らない。`gltf` は `core` だけに頼る。色の換算は `core` に置き、`app` の画面と `gltf` の試験の両方から使う。
- **見直しが必要になる変更**: `Draw` に材質の情報（テクスチャの有無、金属らしさ）を足すこと、色の対象の範囲（選んだ部品とその子孫）、「元の材質を残す」決まり、空の土台のバイト列（同じ文書から同じファイルになる約束に関わる）。

## Architecture
```
 ［新規作成］・Cmd+N ─▶ Command::NewDocument ─ 未保存の確認（Pending::New）
        ▼
 Session: Model::empty() を「読み込んだモデル」と同じ受け渡しへ（転送 → commit_pending）── SourceKind::New・model_ref = None
        ▼ 図形・複製・変形（今のまま）                    右のパネル「色」─▶ Command::SetColor([f32; 3])
 Document（部品ごとの描画の表: 形状の番号 ＋ 基本色）◀── History（Edit::SetColors。取り消しで元の色へ）
        │                                  └▶ Applied::recolored ─▶ pending_patches ─▶ render::update_instances（範囲だけ）
        ▼ 書き出し（ExportScene は今のまま。色は draws に入っている）
 exporter: 土台 = File ／ Stl ／ **Empty（empty_base()）** ─▶ gltf::export::build
        ├ ファイルにある部品: 基本色が土台と違えば、材質とメッシュの写しを足してノードの mesh を替える
        ├ 複製（template）: 写す元のノードの色と違えば、同じ
        └ 足した図形: 今までどおり（色ごとの材質）
        ▼ write_container ─▶ verify（開き直して、色も文書と一致）
```
- **色は、文書がすでに持っている値を書き換えるだけ。** `Document` は部品ごとに描画の表を持ち、複製は自分の写しを持つ（第2段階）。形状（`PrimId`）は共有のまま、色だけが部品ごとに違ってよい。「片方だけを変える」ための新しい仕組みは要らない。
- **書き出しは「土台と違う所を見つけて、足す」。** 変形と同じやり方で、色もビット単位で土台と比べる。色を変えたかどうかの印を文書に持たせない（取り消しで元の色に戻せば、ファイルも元のままになる）。
- **新規作成は、専用の近道を作らない。** 空のモデルを、読み込みと同じ入口から流す。GPUの持ち物の入れ替え、当たり判定の表、履歴の初期化を、開いたときと同じ1か所に任せる。

## File Structure Plan（足すファイルと、変えるファイル）
```
crates/core/src/    color.rs（新規）  scene.rs  doc.rs  history.rs  lib.rs
crates/gltf/src/    build.rs  export/{mod,base,patch,append,verify,synth}.rs  export/recolor.rs（新規）
crates/render/src/  lib.rs（update_instances だけ）
crates/app/src/     command.rs  app.rs  exporter.rs  session/{mod,open,export,persist,edit}.rs  session/color.rs（新規）
                    ui/{mod,ids,empty,help,status,inspector}.rs  ui/color.rs（新規）  automation/{state,script}.rs
crates/app/tests/   new_doc.rs  color.rs  color_panel.rs（新規）  既存の試験（構造体の組み立てだけ）
crates/app/e2e/     new-1.json  color-1.json  desk-1.json（新規）
docs/               glb.md  verify.md  roadmap.md（変更）  README（変更）
```

## Requirements Traceability
| 要件 | 実現する場所 | 詳細 |
|---|---|---|
| 1 新規作成 | core::scene（`Model::empty`）、session::{mod,open}、ui::{empty,help,status}、app.rs | design-new.md |
| 2 新規の書き出しとレビュー | gltf::export（`empty_base`、`base`）、app::exporter、session::{export,persist} | design-new.md、design-export.md |
| 3 色を変える | core::{color,doc,history}、render（`update_instances` が色も読む）、session::color、ui::color | design-color.md |
| 4 色の書き出し | gltf::{build,export::{recolor,patch,verify}} | design-export.md |
| 5 見た目に出るもの・出ないもの | ui::color、session::export（結果の一覧）、docs | design-color.md、design-export.md |
| 6 自動操作と試験 ／ 7 検証と体裁 | automation、各試験、docs/verify.md | design-testing.md |

## 実装者への注意
- 第1段階の落とし穴13件、第2〜第4段階の「確かめたAPI」「落とし穴」は、そのまま当てはまる。先に読むこと。
- この段階の落とし穴は [design-decisions.md](design-decisions.md) にある。「要確認」と書いた項目は、使う前に依存の実物か小さな試験で確かめ、結果を報告する。
- この文書の行番号や文言は、第3段階の区切りAの時点のコードを読んで書いた。区切りB（STL と寸法）と第4段階が同じファイルを変える。**取り込んだ後の内容を土台にする。**
