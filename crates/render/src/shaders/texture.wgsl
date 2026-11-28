struct TextureParams {
    u: vec4<f32>,
    v: vec4<f32>,
    // x: MASK の閾値（なしは -1）、y: 箱形の辺、z: 貼り方、w: 未使用。
    params: vec4<f32>,
};
@group(1) @binding(0) var base_image: texture_2d<f32>;
@group(1) @binding(1) var base_sampler: sampler;
@group(1) @binding(2) var<uniform> material: TextureParams;

struct TexturedVertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) @interpolate(flat) ids: vec2<u32>,
    @location(4) coord: vec3<f32>,
};

fn textured_vertex(v: VertexIn, coord: vec3<f32>) -> TexturedVertexOut {
    let p = scene_vertex(v);
    return TexturedVertexOut(p.clip, p.world_pos, p.normal, p.color, p.ids, coord);
}

@vertex
fn vs_uv(v: VertexIn, @location(11) uv: vec2<f32>) -> TexturedVertexOut {
    let p = vec3<f32>(uv, 1.0);
    return textured_vertex(v, vec3<f32>(dot(material.u.xyz, p), dot(material.v.xyz, p), 0.0));
}

@vertex
fn vs_box(v: VertexIn) -> TexturedVertexOut {
    return textured_vertex(v, v.position);
}

fn textured_face(in: VertexOut, front: bool, uv: vec2<f32>) -> vec4<f32> {
    let face = cross(dpdx(in.world_pos), dpdy(in.world_pos));
    let base = textureSample(base_image, base_sampler, uv) * in.color;
    if (base.a < material.params.x) {
        discard;
    }
    return shade_face(in, front, face, base.rgb);
}

@fragment
fn scene_uv(in: VertexOut, @builtin(front_facing) front: bool,
            @location(4) coord: vec3<f32>) -> @location(0) vec4<f32> {
    return textured_face(in, front, coord.xy);
}

// 画面の +Y は下。表では dpdx × dpdy が頂点の巻き順と逆になる。
// ローカル座標の微分と front は鏡写しで両方反転するので、ここで行列の符号を重ねて掛けない。
const FRONT_DERIVATIVE_SIGN: f32 = -1.0;

@fragment
fn scene_box(in: VertexOut, @builtin(front_facing) front: bool,
             @location(4) p: vec3<f32>) -> @location(0) vec4<f32> {
    let n = cross(dpdx(p), dpdy(p)) * select(-FRONT_DERIVATIVE_SIGN, FRONT_DERIVATIVE_SIGN, front);
    let a = abs(n);
    var uv: vec2<f32>;
    if (a.x >= a.y && a.x >= a.z) {
        uv = vec2<f32>(select(-p.z, p.z, n.x < 0.0), -p.y);
    } else if (a.y >= a.z) {
        uv = vec2<f32>(p.x, select(p.z, -p.z, n.y < 0.0));
    } else {
        uv = vec2<f32>(select(p.x, -p.x, n.z < 0.0), -p.y);
    }
    return textured_face(in, front, uv / material.params.y);
}
