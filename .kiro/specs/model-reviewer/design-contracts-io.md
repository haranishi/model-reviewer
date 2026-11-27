# 境界の型と関数（読み込み・描画）

[design-contracts.md](design-contracts.md) の続き。コアの型（`Model`・`Visibility` など）を受け渡しに使う。

## gltf
```rust
pub fn load_glb(bytes: &[u8]) -> Result<Model, LoadError>;               // 入力が何であってもパニックしない
pub enum LoadError { NotGlb, Container(String), Json(String), Invalid(String), UnsupportedRequiredExtension { name: String, kind: ExtensionKind },
                     ExternalBuffer, MissingBin, BadData { what: String }, NodeGraph(String), NoMesh, TooLarge { what: String }, Internal(String) }
pub enum ExtensionKind { GeometryCompression, Quantization, Instancing, Unknown }
```
`LoadError` と `LoadWarning` は、利用者向けの日本語の文を返す `fn message_ja(&self) -> String` を持つ。文言をUI側に散らさないためである。

## render
```rust
pub struct BufferPlan { /* ページ分割の結果 */ }
pub fn plan_buffers(model: &Model, limits: &wgpu::Limits) -> Result<BufferPlan, RenderError>;   // GPUなしで試験できる純粋関数
pub struct FrameParams { pub size_px: [u32; 2], pub view_proj: Mat4, pub eye: Vec3, pub selected: Option<Range<u32>>, pub clear: [f32; 4] }
pub struct UploadProgress { pub uploaded_bytes: u64, pub total_bytes: u64, pub done: bool }
pub struct RenderOutput<'a> { pub view: &'a wgpu::TextureView, pub recreated: bool }           // Rgba8Unorm として読める画像
impl SceneRenderer {
    pub fn new(device: &wgpu::Device) -> Self;
    pub fn begin_upload(&mut self, device: &wgpu::Device, model: Arc<Model>) -> Result<(), RenderError>;  // 表示中の組は残し、転送中の組を作る
    pub fn continue_upload(&mut self, queue: &wgpu::Queue, budget_bytes: u64) -> UploadProgress;      // 送り切った回に表示中と入れ替える
    pub fn cancel_upload(&mut self);                                                                    // 転送中の組だけを捨てる
    pub fn clear_model(&mut self);
    pub fn set_instances(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, model: &Model, vis: &Visibility);
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &FrameParams) -> RenderOutput<'_>;
    pub fn stats(&self) -> RenderStats;       // 描画した回数・転送したバイト数。再描画しないことの試験に使う
}
pub enum RenderError { LimitExceeded { what: String, needed: u64, limit: u64 }, OutOfMemory, Device(String) }
```
`selected` は `NodeId` の連続範囲（選択したノードとその子孫）。
