// 描画の定数。scene・mask・grid の先頭に付けて使う（Rust の FrameUniform と同じ並び、304バイト）。

struct Frame {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    // x..y: 選択した NodeId の範囲（x >= y なら選択なし）。z: 1 なら最後にsRGBへ変換する。
    // w: 表示（0 面、1 面と線、2 線、3 透かし）。
    selected: vec4<u32>,
    // 画素の視線を作るための view_proj の逆行列（透視でも平行投影でも同じ式で使える）。
    inv_view_proj: mat4x4<f32>,
    // 変形中の差分の行列と、その法線用の行列。範囲の外の部品には掛けない。
    drag_delta: mat4x4<f32>,
    drag_normal: mat3x3<f32>,
    // x..y: 変形中の NodeId の範囲（x >= y なら変形なし）。
    drag: vec4<u32>,
    // x: 格子の間隔。y: 面の不透明度（透かしのときだけ1未満）。
    // z: 断面の位置（ワールドの座標）。w: 断面の軸と向き（X ±1、Y ±2、Z ±3。正は軸の正の側を消す。0 は断面なし）。
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> frame: Frame;

fn in_drag(node: u32) -> bool {
    return node >= frame.drag.x && node < frame.drag.y;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn finish_color(rgb: vec3<f32>) -> vec3<f32> {
    if (frame.selected.z != 0u) {
        return linear_to_srgb(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)));
    }
    return rgb;
}

// 断面が入っているか。
fn section_on() -> bool {
    return frame.params.w != 0.0;
}

// 断面で消す側か。`core::section::SectionPlane::cuts` と同じ式（平面の上の点は消さない）。
fn section_cut(p: vec3<f32>) -> bool {
    let code = frame.params.w;
    if (code == 0.0) {
        return false;
    }
    let a = abs(code);
    var v = p.z;
    if (a < 1.5) {
        v = p.x;
    } else if (a < 2.5) {
        v = p.y;
    }
    return (v - frame.params.z) * select(-1.0, 1.0, code > 0.0) > 0.0;
}
