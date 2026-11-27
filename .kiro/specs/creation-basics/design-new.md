# 新規作成

要件1・2。書き出しの中身（空の土台）は [design-export.md](design-export.md)。

## 方針
- **空のモデルを、読み込んだモデルと同じ受け渡しへ流す。** いまの `Session` は、読み込みが終わると `Incoming` を置き、GPUへの転送の後に `commit_pending` で文書・レビュー・視点・履歴を1か所で入れ替える。新規作成は、ファイルを読む工程だけを飛ばして、この受け渡しに入る。入れ替えの決まり（選択を消す、断面を戻す、履歴を空にする、GPUの古い形状を解放する）を二重に書かない。
- **文書は「形状の土台（`Arc<Model>`）＋ 足した形状 ＋ 部品の表」のまま。** 土台が空なので、形状はすべて「足した形状」になり、部品はすべて「まだファイルに無い部品」になる。図形の追加・複製・削除・書き出しの「足す」経路が、そのまま働く。
- **元の形式に「新規」を足す。** 第3段階は、元が STL のとき `SourceKind::Stl` を持ち、書き出すと `Glb` に移る。新規も同じ移り方にする。

## 型の約束
```rust
// core::scene
impl Model { pub fn empty() -> Model; }       // 部品・描画・形状が0個、bounds は空、stats は 0、warnings は空
// app::session
pub enum SourceKind { Glb, Stl { unit: UnitLabel }, New }          // New を足す
enum Pending { OpenModel(PathBuf), LoadReview(PathBuf), Quit, New } // New を足す（未保存の確認の後に続ける操作）
struct Incoming { /* … */ model_ref: Option<ModelRef>, /* … */ }   // 新規は None。ファイルの大きさと時間の項目は 0
// app::command
Command::NewDocument                                                 // 履歴と未保存の印に入らない（文書ごと入れ替える）
```
- `Session.model_ref` は今も `Option` である。新規の文書では `None` のままにし、書き出しが終わったときに、書き出した GLB の `ModelRef` を入れる（今の `finish_export` がしていること）。
- `Model::empty()` は `load_glb` からは出てこない（読み込みは、メッシュの無いファイルを今までどおり断る）。

## 流れ（`session::open::new_guarded` → `new_document`）
1. 書き出し中・読み込み中なら断る（「書き出し中は新規作成できません」「読み込み中は新規作成できません」）。
2. `hold(Pending::New)` で未保存の確認を通す。確認の後は `confirm_discard` が続ける（開くときと同じ）。
3. 復元の途中（`restore`）を消し、進行中の読み込みを取り消し、世代を進めて `Incoming { model: Arc::new(Model::empty()), model_ref: None, .. }` を置く。
4. GPUがあれば、空のモデルの転送を始める（送るものは0バイトで、1回で終わる）。GPUが無ければ、そのまま `commit_pending` へ進む（今の、GPUなしの経路）。
5. `commit_pending` は、`model_ref` が `None` のとき、次のように分かれる。ほかは開いたときと同じである。
   - レビューの復元（`take_restored`）を呼ばない。`ReviewState::new(&doc)` で始める。
   - `source_kind = SourceKind::New`、`units = Units::default()`（要件1.5）。
   - 視点は `OrbitCamera::home` の結果に、`target = 原点`・`distance = NEW_DISTANCE`（4.0）を入れる。投影は今までどおり保つ。`grid_spacing(4.0)` は 0.1 になる（要件1.6）。
   - 状態欄は「新規の文書を作りました」。性能値のうち、ファイルの大きさ・読み取り・識別・解析は `None` にする。
   - `opened_files` には何も足さない（書き出し先を禁じるファイルが無い）。
- 部品をすべて消した後や、すべて取り消した後も、文書は空のまま残る。`bounds` が空のときの動き: 「全体表示」と「初期視点」は視点を変えない（`OrbitCamera::fit` は空の範囲で何もしない。確かめた）。図形の大きさは `shapes::size_for` が 1 を返す（半径が正でなければ 1。確かめた）。置く場所は、今までどおり視点の注視点である（新規の直後は原点）。

## 新規の文書でできないこと・断ること
| 操作 | 新規（未書き出し）での扱い | 文 |
|---|---|---|
| レビュー保存 | 断る。形の編集があるときは、今の「先に GLB を書き出してください」の確認。編集が無いときは、失敗のダイアログ | 「先に GLB を書き出すと、レビューを保存できます」 |
| レビュー読込 | 今までどおり（レビューが指すモデルを開く。未保存があれば確認） | — |
| 書き出し | 描画を持つ部品が無ければ断る。あれば、土台を `ExportBase::Empty` にして始める | 「書き出す部品がありません。図形を置いてから書き出してください」 |
| ファイルの単位（m）の切り替え | GLB と同じに働く（数値は換算しない。第3段階のまま） | — |
- 書き出しの要求（`ExportRequest`）は、`base_kind: ExportBase::Empty`、`base_path` は空、`base_identity` は使わない、`forbidden` は `opened_files`（この作業で開いたファイルがあれば、そこへは書かない決まりを保つ）。
- 書き出しが終わると、`source_kind = Glb`、`model_ref = 書き出したファイル`、`exported_since_load = true`、`saved_doc_rev` を今の版にする。レビューに内容（注釈・視点・非表示）があれば、今の決まりどおり未保存にする。`Session.model`（空のモデル）は形状の土台として残る（GPUへ送った形状の組の照合に使う。第2段階の「書き出した後は `session.model` が今の土台と異なる」と同じ状態）。

## 画面の入口
- **何も開いていない画面**（`ui/empty.rs`）: 案内の文の下に［新規作成］と「空の状態から作ります（Cmd+N）」。読み込み中は出さない。部品の名前は `empty.new`。いま `out` を使っていない関数なので、`Command::NewDocument` を積む1か所が増えるだけである。
- **キー**: Cmd+N（Windows と Linux では Ctrl+N）。`app::shortcut_commands` の、ダイアログ・ヘルプ・入力中の確かめの後、「パネルの上」の確かめの**前**に置く（ポインタの場所によらず働かせる）。第4段階の M1 が、この分岐を表（`file.new`）へ移す。
- **ヘルプの表**: 「Cmd+N｜新規作成（空の状態から作り始める）」の1行。
- **図形を置こうとして、何も開いていないとき**: 今の「モデルを開くと図形を置けます」を、「［新規作成］（Cmd+N）か、モデルを開くと図形を置けます」に替える。
- **上部のボタンは増やさない。** 幅960で余りが無い（第4段階の ux.md と同じ理由）。
- **状態欄のファイル名**: `model_ref` が無く、`source_kind` が `New` のとき「新規（未書き出し）」。詳細パネルの「場所」は「—」。

## 自動操作
- 新しい手順は足さない。`click_widget`（`empty.new`）か `key`（`N` と `command`）で作る。
- `dump_state` の `source_kind` に `"new"` を足す。`model` の項目（ファイル名・大きさ・識別）は、新規の間は `null` にする（**要確認**: 今の書き出しが、`model_ref` が無いときにどう書いているか。項目の形を変えない）。

## 要確認（使う前に確かめる）
- 空の `Model` で、`plan_buffers`・`UploadJob`・`set_instances`・`PickIndex` の構築・輪郭線・`recount_triangles` がパニックしないこと。`plan_buffers` は形状0個でページ0枚を返す形に読めるが、実行では確かめていない。GPUの試験を1本足す。
- 空の表示中のモデルに `add_primitives` で形状を足す経路（今は、読み込んだ形状が1個以上ある状態でしか通っていない）。
- `commit_pending` の後に呼ばれる処理のうち、`model_ref` を必ずあるものとして読む箇所（状態の書き出し、詳細パネル、性能値の出力、ウィンドウの題名）。`grep` で洗い出して、`None` の分岐を入れる。
- 当たり判定の表が空のときの、クリック・吸着・断面の既定の位置（範囲が空）。
