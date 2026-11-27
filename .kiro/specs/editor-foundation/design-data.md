# 文書のデータ構造・取り消し・未保存

## 形状と構造を分ける
第1段階の `Arc<Model>` は不変で、部品は行きがけ順の配列、子孫は `[id, subtree_end)` の連続範囲に並ぶ。表示状態・注釈・当たり判定・輪郭線・一覧の描画がこの並びに頼っている。この並びは**変えない**。変えるのは「誰が配列を持つか」である。

```rust
// core::doc
pub struct NodeKey(pub u64);              // 文書の中で変わらない鍵。開いたときは並び順に 0 から振る
pub struct DocNode {
    pub key: NodeKey, pub name: Arc<str>, pub parent: Option<NodeId>, pub subtree_end: u32, pub depth: u32,
    pub local: Affine3A, pub world: Affine3A, pub draws: Range<u32>, pub world_aabb: Aabb,
    pub template: Option<u32>,            // まだファイルに無い複製が、どのノードのJSONを写すか（書き出し用）
}
pub struct Document { /* base: Arc<Model>、added: Vec<Arc<Primitive>>、nodes: Vec<DocNode>、draws: Vec<Draw>、
                         鍵→添字の表、鍵→ファイル内のノード番号の表、bounds */ }
```
- **形状は不変のまま共有する。** `base` の形状（`PrimId` が `base.primitives.len()` 未満）はGPUに送ったものをそのまま使う。足した図形の形状は `added` に追記する（`PrimId` は続きの番号）。形状は作業中に消さない。図形の追加を取り消しても形状は残る（1個あたり数十KBで、モデルを閉じると消える）。
- **書き換えるのは部品の表だけ。** 1件は約200バイトで、10万個で約20MB。名前は `Arc<str>` にして、表の作り直しで文字列を複製しない。
- `base.nodes` は読み込んだ時点の控えとして残り、文書を作るときに1回だけ写す。以後、画面と計算は文書の表だけを見る。`Model.nodes` を直接読むコードを残さないことを、接合の作業の合格条件にする（`grep` で確かめる）。
- `Node` に `local: Affine3A` を足し、読み込みがファイルの値から作った行列をそのまま入れる。ワールド行列から逆算すると誤差が入り、数値入力に `0.30000001` のような値が出て、書き出しで「変えていない部品」を見分けられなくなる。

## 読む側の口：`SceneRead`
```rust
pub trait SceneRead {          // core::scene。Model と Document が実装する
    fn node_count(&self) -> usize;                      fn parent(&self, id: NodeId) -> Option<NodeId>;
    fn subtree_end(&self, id: NodeId) -> u32;           fn depth(&self, id: NodeId) -> u32;
    fn name(&self, id: NodeId) -> &str;                 fn world(&self, id: NodeId) -> &Affine3A;
    fn world_aabb(&self, id: NodeId) -> Aabb;           fn draws(&self, id: NodeId) -> &[Draw];
    fn primitive(&self, id: PrimId) -> Option<&Primitive>;  fn primitive_count(&self) -> usize;
    fn file_index(&self, id: NodeId) -> Option<u32>;    // GLBの中のノード番号。まだファイルに無ければ None
    fn find_file_index(&self, gltf_index: u32) -> Option<NodeId>;   fn bounds(&self) -> Aabb;
}
```
`pick`・`Visibility`・`instances::build`・`review_file`・`ReviewState::new` は、引数を `&Model` から `&impl SceneRead` に替える。中身の処理は変えない。最初の作業（E0）でこの置き換えだけを行い、振る舞いが変わらないことを既存の試験と自動操作で確かめる。

## 番号の付け替え
`NodeId` は配列の添字なので、追加と削除でずれる。履歴と書き出しは `NodeKey` で部品を指し、画面と計算は `NodeId` を使う。
- 構造の変更は、行きがけ順の**連続範囲の挿入か削除**に限る（親子の付け替えは範囲外）。ずれ方は `Remap::Inserted { at, len }` か `Remap::Removed { at, len }` の1つで表せ、`Remap::map(id) -> Option<NodeId>` は定数時間で済む。
- `NodeId` を持つ場所は、`Visibility`（配列を同じ範囲で継ぎ足す・切り取る）、注釈（`ReviewState::apply_remap`）、選択、下書き、選択中の注釈。文書と `ReviewState` の分は `History` が付け替え、残りは `Session` が `Applied.remaps` を見て付け替える。付け替え漏れは、削除→取り消しの後に全部の持ち主が元の部品を指すことを試験で確かめる。計測の点と、部品一覧の開閉の記憶は、はじめから `NodeKey` で持つ。

## 操作ごとの仕事量
| 操作 | CPU | GPU |
|---|---|---|
| 変形のドラッグ中 | 行列1つの計算 | 定数1つの更新だけ（[design-render.md](design-render.md)） |
| 変形の確定 | 動かした範囲の `world` と `world_aabb` を作り直す。全体の `bounds` は全部品を1回なめる | 動かした範囲のインスタンスだけ書き直す（1件144バイト） |
| 追加・複製・削除 | 部品の表を1回作り直す（部品数に比例）。鍵の表も作り直す | インスタンスを1回作り直す。足した形状だけを転送する |
| 表示の切り替え | 第1段階と同じ | 第1段階と同じ（インスタンスの作り直し） |
形状の再構築と再転送は、どの操作でも起きない。当たり判定の索引は形状ごとなので、変形や複製では作り直さない。足した図形の分だけ、その場で作って足す。

## 変形の持ち方
- 部品は親に対する行列（`local`）で持つ。変えた部品とその子孫だけ `world = 親.world × local` を計算し直す。触れていない部品の `world` は読み込んだ値のまま残す。
- ハンドルの操作は「ワールド空間での差分の行列 D」で表す。新しい `local` は `親.world⁻¹ × D × world`。子孫には同じ D が掛かるので、変形中の見た目は D を1つGPUへ渡すだけで済む。
- 数値入力は、`local` を位置・回転（四元数）・大きさに分けて見せる。回転は度のオイラー角（XYZの順）で表示し、入力された3つの値から四元数を作り直す。分けて合成し直した結果が元の行列と合わない（傾いた拡大を含む）部品は、数値の欄を無効にする（要件7.4）。

## 取り消しとやり直し
```rust
// core::history
pub enum Edit { SetLocal { key, before, after }, Insert { at: Place, piece: Subtree }, Remove { at: Place, piece: Subtree },
                SetVisible { key, before, after }, ShowAll { hidden: Vec<NodeKey> },
                AddNote(..), EditNote { id, before, after }, RemoveNote { index, note }, AddView(..), RemoveView { index, view } }
pub struct Step { pub label: &'static str, pub edits: Vec<Edit>, pub select: Option<NodeKey>, /* 版の番号 */ }
pub struct Applied { pub remaps: Vec<Remap>, pub touched: Vec<Range<u32>>, pub structure: bool, pub visibility: bool,
                     pub select: Option<NodeKey>, pub doc_rev: u64, pub review_rev: u64, pub label: &'static str }
impl History { pub fn push(&mut self, step: Step);  pub fn undo(&mut self, doc: &mut Document, review: &mut ReviewState) -> Option<Applied>;
               pub fn redo(..) -> Option<Applied>;  pub fn can_undo(&self) -> bool;  pub fn can_redo(&self) -> bool;
               pub fn bytes(&self) -> u64;  pub fn len(&self) -> usize;  pub fn clear(&mut self); }
```
- **持つのは逆の操作であり、文書の写しではない。** 写しで持つと、10万部品で1回あたり約20MBになる。変形は鍵と前後の行列（約100バイト）、削除は取り除いた部品の表・表示の設定・注釈（`Subtree`。形状は含まない）を持つ。`Place` は「親の鍵と、次の兄弟の鍵」で、戻す場所を一意に決める。
- **単位は、利用者から見た1回の操作。** 押してから離すまでのドラッグ、数値の確定1回、追加・複製・削除の各1回。削除は「部品を除く」と「付いていた注釈を除く」を1つの `Step` にまとめる。
- **対象は、未保存の印が付く操作すべて**（変形、追加、複製、削除、表示の切り替え、注釈と視点の追加・編集・削除）。規則が1つで済み、「これは戻せるのか」を利用者が覚えなくてよい。視点の移動と選択は入れない。取り消したときは関わった部品を選択するが、視点は動かさない。
- **上限は64MiBと500回（初期値。実測で見直す）。** 各 `Edit` が自分の大きさを申告し、超えたら古い順に捨てる。1回分だけで上限を超える操作は実行したうえで履歴を空にし、「これより前へは戻せません」と状態欄に出す。
- 入力欄の中の Cmd+Z は、egui の入力欄が自分で処理する（文字の取り消し）。入力欄に入力位置がある間、アプリの取り消しは働かせない。
- モデルやレビューを開くと履歴を空にする。書き出しでは残す。

## 未保存の印と版の番号
- `Step` ごとに通しの番号を振る。`Session` は「文書の版」と「レビューの版」を別々に持ち、実行・取り消し・やり直しのたびに、その `Step` が変えた側の版を更新する。削除は両方を変える。
- 未保存は「いまの版 ≠ 最後に書き出した（保存した）版」で決める。取り消しで書き出した時点まで戻れば、印は消える。履歴の上限で捨てられた版へは戻れないので、印は付いたままになる（番号が一意なので取り違えない）。
- 入力途中の文字（第1段階の `draft_pending`・`body_pending`）は、これまでどおりレビューの未保存に数える。
- 確認のダイアログは、失うものを名前で挙げる（「書き出していない形の編集」「保存していないレビュー」「入力途中の注釈」）。ボタンは第1段階と同じ「破棄して続ける」「やめる」。

## レビューとの関係
- 注釈は部品の中の座標なので、部品を動かすと一緒に動く。表示は毎フレーム `world × local_pos` で計算しており、追加の処理は要らない。変形中は D を掛けて見せる。
- レビューJSONは部品を `gltf_index` で指す。ファイルに無い部品（足した図形、複製）は指せないので、**書き出していない形の編集がある間はレビューを保存させない**（要件12.2）。
- 書き出しが終わると、書き出しが返した「鍵→ノード番号」を文書の表に入れ、`ModelRef` を新しいファイルへ替える。注釈・表示・視点はそのまま残り、レビューは未保存になる。
- 削除した部品に付いていた注釈は、部品と一緒に消える（取り消せば戻る）。書き出しの結果の一覧に、消した注釈の件数は出さない。削除の時点で状態欄に「部品 n 個と注釈 m 件を削除しました」と出す。
