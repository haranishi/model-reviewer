// マスクの縁を、描き終えた画像へ二重の輪郭で重ねる。全画面を覆う三角形1枚で描く。
// 部品の内側だけを塗る。外縁は暗く、その内側は明るい。色の違う部品の上でもどちらかが見える。

struct Outline {
    inner: vec4<f32>,
    outer: vec4<f32>,
    // x: 暗い帯の太さ（画素）。y: 1 なら最後にsRGBへ変換する。z: 輪郭全体の太さ（画素）。
    params: vec4<f32>,
};

@group(0) @binding(0) var mask: texture_2d<f32>;
@group(0) @binding(1) var<uniform> outline: Outline;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn sample_mask(p: vec2<i32>, size: vec2<i32>) -> f32 {
    let q = clamp(p, vec2<i32>(0), size - vec2<i32>(1));
    return textureLoad(mask, q, 0).r;
}

@fragment
fn fs_main(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(mask));
    let p = vec2<i32>(pos.xy);
    if (sample_mask(p, size) < 0.5) {
        discard;
    }
    let dark_r = max(i32(outline.params.x), 1);
    let outer_r = max(i32(outline.params.z), dark_r + 1);
    var nearest = 999.0;
    for (var dy = -outer_r; dy <= outer_r; dy = dy + 1) {
        for (var dx = -outer_r; dx <= outer_r; dx = dx + 1) {
            let d2 = dx * dx + dy * dy;
            if (d2 <= outer_r * outer_r && sample_mask(p + vec2<i32>(dx, dy), size) < 0.5) {
                nearest = min(nearest, sqrt(f32(d2)));
            }
        }
    }
    if (nearest > f32(outer_r)) {
        discard;
    }
    let dark = nearest <= f32(dark_r);
    var rgb = select(outline.inner.rgb, outline.outer.rgb, dark);
    if (outline.params.y > 0.5) {
        rgb = linear_to_srgb(rgb);
    }
    return vec4<f32>(rgb, 1.0);
}
