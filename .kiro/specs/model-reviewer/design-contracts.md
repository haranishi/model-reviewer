# 境界の型と関数（コア）

作業パッケージが並行で進むための約束。最初の作業パッケージ（WP-00）がこの通りに型と空の実装を置き、以後は本文だけを埋める。名前・引数・意味を変えるときは、この文書を先に直す。読み込みと描画の境界は [design-contracts-io.md](design-contracts-io.md)、アプリ層の `Command` と `Session` は [design-app.md](design-app.md)。

## core::math / core::scene
```rust
pub struct Aabb { pub min: Vec3, pub max: Vec3 }          // 空は min > max（empty / is_empty）。union / transformed(&Affine3A) / center / radius。空の center は 0、radius は 0
pub struct Ray  { pub origin: Vec3, pub dir: Vec3 }       // dir は呼び出し側が正規化済み。型は正規化し直さない
pub struct NodeId(pub u32); pub struct PrimId(pub u32);   // Model.nodes / Model.primitives の添字。Copy・Eq・Hash

#[repr(C)] #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex { pub position: [f32; 3], pub normal: [f32; 3] }        // 24バイト。GPUへそのまま送る

pub struct Primitive {                 // 共有される形状（glTFの mesh.primitives の1個）
    pub vertices: Vec<Vertex>, pub indices: Vec<u32>,   // 三角形リスト。len は3の倍数、全要素 < vertices.len()
    pub local_aabb: Aabb, pub has_normals: bool,        // false のとき normal は0。描画側が面法線を導く
}
pub struct Draw { pub prim: PrimId, pub base_color: [f32; 4] }           // リニアRGBA（baseColorFactor）
pub struct Node {
    pub name: String,                  // 無名は「ノード <gltf_index>」
    pub gltf_index: u32,               // GLB内のノード番号。レビューJSONはこれで部品を指す
    pub parent: Option<NodeId>, pub subtree_end: u32, pub depth: u32,
    pub world: Affine3A,               // 親から合成済み
    pub draws: Range<u32>,             // Model.draws の範囲。メッシュの無いノードは空
    pub world_aabb: Aabb,              // 自分の draws だけ。無ければ空
}
pub struct Model {
    pub nodes: Vec<Node>, pub draws: Vec<Draw>, pub primitives: Vec<Primitive>,
    pub bounds: Aabb, pub stats: ModelStats, pub warnings: Vec<LoadWarning>,
}
pub struct ModelStats { pub nodes: u32, pub meshes: u32, pub primitives: u32, pub unique_triangles: u64, pub placed_triangles: u64 }
impl Model {
    pub fn node(&self, id: NodeId) -> &Node;
    pub fn subtree(&self, id: NodeId) -> Range<u32>;                      // id.0 .. subtree_end
    pub fn subtree_bounds(&self, id: NodeId) -> Aabb;
    pub fn node_by_gltf_index(&self, gltf_index: u32) -> Option<NodeId>;
}
```
**ノードの並びの規則**: `nodes` は深さ優先・行きがけ順で、あるノードの子孫は必ず `[id, subtree_end)` に連続して並ぶ。表示の伝播、強調する範囲、ツリーの平坦化、子孫の境界は、すべてこの連続範囲で処理する。この規則を変えると描画・UI・当たり判定が同時に壊れる。
`LoadWarning` は簡略化の一覧に出す列挙。件数か拡張名を持つ。`Textures`・`VertexColors`・`AlphaBlend`・`Animations`・`Skins`・`Morphs`・`DerivedNormals`（いずれも `{ count }`）、`OmittedLinesAndPoints { lines, points }`、`SimplifiedExtension { name }`（見た目だけの既知拡張）、`UnsupportedOptionalExtension { name }`。`message_ja` が利用者向けの文を返す。

## core::camera
```rust
#[derive(Clone, Copy, PartialEq)]
pub struct OrbitCamera { pub target: Vec3, pub yaw: f32, pub pitch: f32, pub distance: f32, pub fov_y: f32 }  // ラジアン
impl OrbitCamera {
    pub fn home(bounds: &Aabb, aspect: f32) -> Self;                     // 初期視点
    pub fn fit(&mut self, bounds: &Aabb, aspect: f32);                   // 向きは保つ
    pub fn orbit(&mut self, delta_px: Vec2);
    pub fn pan(&mut self, delta_px: Vec2, viewport_height_px: f32);
    pub fn zoom(&mut self, factor: f32, cursor_ndc: Option<Vec2>, aspect: f32, limits: (f32, f32));
    pub fn eye(&self) -> Vec3;
    pub fn view(&self) -> Mat4;
    pub fn proj(&self, aspect: f32, scene: &Aabb) -> Mat4;               // 反転Z・無限遠
    pub fn screen_ray(&self, ndc: Vec2, aspect: f32) -> Ray;
    pub fn project(&self, world: Vec3, aspect: f32) -> Option<Vec2>;     // NDC。カメラ背面は None
}
```
NDCは x 右向き・y 上向き・範囲 -1〜1。画面座標との変換はアプリ層の1か所（`ui::viewport`）だけで行う。

## core::visibility / core::pick
```rust
pub struct Visibility { /* own: Vec<bool>, effective: Vec<bool> */ }
impl Visibility {
    pub fn all_visible(node_count: usize) -> Self;
    pub fn set(&mut self, model: &Model, id: NodeId, visible: bool);     // 子孫の effective も更新
    pub fn show_all(&mut self);
    pub fn own(&self, id: NodeId) -> bool;                               // その部品自身の設定
    pub fn effective(&self, id: NodeId) -> bool;                         // 祖先も含めた結果
    pub fn hidden_own(&self) -> impl Iterator<Item = NodeId> + '_;
}
pub struct PickIndex { /* プリミティブごとの三角形の索引 */ }
impl PickIndex { pub fn build(model: &Model) -> Self;  pub fn build_cancellable(model: &Model, cancel: &dyn Fn() -> bool) -> Option<Self>; }  // 中止なら None
pub struct Hit { pub node: NodeId, pub t: f32, pub world_pos: Vec3, pub local_pos: Vec3, pub local_normal: Vec3 }
pub fn pick(model: &Model, index: &PickIndex, vis: &Visibility, ray: &Ray) -> Option<Hit>;
```

## core::review / core::review_file / core::identity
```rust
pub struct Annotation { pub id: u64, pub node: NodeId, pub local_pos: Vec3, pub local_normal: Vec3, pub text: String }
pub struct SavedView  { pub id: u64, pub name: String, pub camera: OrbitCamera }
pub struct ReviewState { pub visibility: Visibility, pub annotations: Vec<Annotation>, pub views: Vec<SavedView>, /* next_id */ }
impl ReviewState {
    pub fn new(model: &Model) -> Self;
    pub fn add_annotation(&mut self, hit: &Hit, text: String) -> u64;
    pub fn edit_annotation(&mut self, id: u64, text: String) -> bool;
    pub fn remove_annotation(&mut self, id: u64) -> bool;
    pub fn add_view(&mut self, name: String, camera: OrbitCamera) -> u64;
    pub fn remove_view(&mut self, id: u64) -> bool;
}
pub struct ModelIdentity { pub size_bytes: u64, pub blake3_hex: String }
impl ModelIdentity { pub fn from_bytes(bytes: &[u8]) -> Self; }
pub struct ModelRef { pub file_name: String, pub path: String, pub relative_path: Option<String>, pub identity: ModelIdentity }
pub struct ReviewFile { /* JSON v1 と1対1のserde用の型。design-review.md */ }
pub fn to_json(state: &ReviewState, model: &Model, model_ref: &ModelRef) -> String;
pub fn parse(json: &str) -> Result<ReviewFile, ReviewError>;             // 形式名・版・有限値を検査
pub fn restore(file: &ReviewFile, model: &Model, identity: &ModelIdentity) -> Result<ReviewState, ReviewError>;
pub enum ReviewError { Json(String), WrongFormat, NewerVersion { found: u32, supported: u32 }, InvalidValue(String),
                       ModelMismatch { expected: ModelIdentity, actual: ModelIdentity }, UnknownNode { gltf_index: u32 } }
```
