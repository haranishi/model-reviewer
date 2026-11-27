// 選択の輪郭のためのマスク。選択中のインスタンスだけを深度なしで R8 へ 1 を書く。
// 他の部品に隠れていても形が残るので、輪郭線で場所が分かる。変形中の差分の行列も本体と同じく掛ける。

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) world0: vec4<f32>,
    @location(3) world1: vec4<f32>,
    @location(4) world2: vec4<f32>,
    @location(5) world3: vec4<f32>,
    @location(10) ids: vec4<u32>,
};

@vertex
fn vs_main(v: VertexIn) -> @builtin(position) vec4<f32> {
    var world = mat4x4<f32>(v.world0, v.world1, v.world2, v.world3);
    if (in_drag(v.ids.x)) {
        world = frame.drag_delta * world;
    }
    return frame.view_proj * (world * vec4<f32>(v.position, 1.0));
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(1.0, 0.0, 0.0, 1.0);
}
