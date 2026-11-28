// 形状の描画。色はリニアで計算し、sRGBのビューへ書く（ビュー形式が使えない環境では最後に変換する）。
// 法線の無い形状は、ワールド座標の微分から面法線を導く。面の向きによる間引きはせず、裏面は法線を裏返して照らす。
// 変形中の範囲の部品には、ワールド行列の前に差分の行列を掛ける（インスタンスは書き直さない）。
// 断面が入っていると、消す側の画素を捨てる。「面」「面と線」では、裏を向いた面を照明なしの切り口の色で塗る。

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) world0: vec4<f32>,
    @location(3) world1: vec4<f32>,
    @location(4) world2: vec4<f32>,
    @location(5) world3: vec4<f32>,
    @location(6) normal0: vec4<f32>,
    @location(7) normal1: vec4<f32>,
    @location(8) normal2: vec4<f32>,
    @location(9) color: vec4<f32>,
    @location(10) ids: vec4<u32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) @interpolate(flat) ids: vec2<u32>,
};

fn scene_vertex(v: VertexIn) -> VertexOut {
    var world = mat4x4<f32>(v.world0, v.world1, v.world2, v.world3);
    var normal_mat = mat3x3<f32>(v.normal0.xyz, v.normal1.xyz, v.normal2.xyz);
    if (in_drag(v.ids.x)) {
        world = frame.drag_delta * world;
        normal_mat = frame.drag_normal * normal_mat;
    }
    let wp = world * vec4<f32>(v.position, 1.0);
    var out: VertexOut;
    out.clip = frame.view_proj * wp;
    if ((v.ids.y & FLAG_CUT_OUT) != 0u) {
        // 3つの角を同じ画面の外の点へ寄せ、三角形を描かない。
        out.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    }
    out.world_pos = wp.xyz;
    out.normal = normal_mat * v.normal;
    out.color = v.color;
    out.ids = v.ids.xy;
    return out;
}

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    return scene_vertex(v);
}

const FLAG_DERIVE_NORMAL: u32 = 1u;
// ワールド行列の行列式が負の部品。表と裏の判定が逆になるので反転する。
const FLAG_MIRRORED: u32 = 2u;
// MASK の材質で、基本色の透明度が閾値より小さい描画。画像を掛けても閾値に届かないので描かない。
const FLAG_CUT_OUT: u32 = 4u;
// 切り口の色。sRGB の #D9822B をリニアにした値。
const CUT_COLOR: vec3<f32> = vec3<f32>(0.69387, 0.22323, 0.02416);
const SKY: vec3<f32> = vec3<f32>(0.42, 0.45, 0.50);
const GROUND: vec3<f32> = vec3<f32>(0.16, 0.15, 0.14);

fn is_selected(node: u32) -> bool {
    return node >= frame.selected.x && node < frame.selected.y;
}

@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // 微分は分岐の外で求める（一様な制御の流れの中でしか使えない）。断面で捨てるのは、その後にする。
    let face = cross(dpdx(in.world_pos), dpdy(in.world_pos));
    return shade_face(in, front, face, in.color.rgb);
}

fn shade_face(in: VertexOut, front: bool, face: vec3<f32>, base_rgb: vec3<f32>) -> vec4<f32> {
    if (section_cut(in.world_pos)) {
        discard;
    }
    if (section_on() && frame.selected.w <= 1u) {
        let mirrored = (in.ids.y & FLAG_MIRRORED) != 0u;
        if (front == mirrored) {
            return vec4<f32>(finish_color(CUT_COLOR), frame.params.y);
        }
    }
    var n = in.normal;
    if ((in.ids.y & FLAG_DERIVE_NORMAL) != 0u) {
        n = face;
    }
    let len = length(n);
    if (len > 1e-20) {
        n = n / len;
    } else {
        n = vec3<f32>(0.0, 1.0, 0.0);
    }
    let to_eye = normalize(frame.eye.xyz - in.world_pos);
    if (dot(n, to_eye) < 0.0) {
        n = -n;
    }
    // カメラ側からの平行光。少し上から当てて、正対する面ばかりが同じ明るさにならないようにする。
    let light = normalize(to_eye + vec3<f32>(0.0, 0.35, 0.0));
    let diffuse = max(dot(n, light), 0.0);
    let ambient = mix(GROUND, SKY, n.y * 0.5 + 0.5);
    var rgb = base_rgb * (ambient * 0.55 + vec3<f32>(diffuse * 0.75));
    if (is_selected(in.ids.x)) {
        rgb = rgb * 1.35 + vec3<f32>(0.012);
    }
    return vec4<f32>(finish_color(rgb), frame.params.y);
}

// 線の表示。「面と線」では面の上に暗い線、「線」では部品の色を明るくした線。
@fragment
fn fs_line(in: VertexOut) -> @location(0) vec4<f32> {
    if (section_cut(in.world_pos)) {
        discard;
    }
    var rgb: vec3<f32>;
    if (frame.selected.w == 2u) {
        rgb = mix(in.color.rgb, vec3<f32>(1.0), 0.35);
        if (is_selected(in.ids.x)) {
            rgb = mix(rgb, vec3<f32>(1.0), 0.5);
        }
    } else {
        rgb = in.color.rgb * 0.12 + vec3<f32>(0.004);
    }
    return vec4<f32>(finish_color(rgb), 1.0);
}
