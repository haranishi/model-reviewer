// 選択の輪郭のためのマスク。選択中のインスタンスだけを深度なしで R8 へ 1 を書く。
// 他の部品に隠れていても形が残るので、輪郭線で場所が分かる。変形中の差分の行列も本体と同じく掛ける。
// 断面で消す側の画素は、本体と同じ式で捨てる。

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) world0: vec4<f32>,
    @location(3) world1: vec4<f32>,
    @location(4) world2: vec4<f32>,
    @location(5) world3: vec4<f32>,
    @location(10) ids: vec4<u32>,
};

struct MaskOut {
    @builtin(position) clip: vec4<f32>,
    // 断面で切るためのワールドの座標。
    @location(0) world_pos: vec3<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> MaskOut {
    var world = mat4x4<f32>(v.world0, v.world1, v.world2, v.world3);
    if (in_drag(v.ids.x)) {
        world = frame.drag_delta * world;
    }
    let wp = world * vec4<f32>(v.position, 1.0);
    var out: MaskOut;
    out.clip = frame.view_proj * wp;
    out.world_pos = wp.xyz;
    return out;
}

@fragment
fn fs_main(in: MaskOut) -> @location(0) vec4<f32> {
    if (section_cut(in.world_pos)) {
        discard;
    }
    return vec4<f32>(1.0, 0.0, 0.0, 1.0);
}
