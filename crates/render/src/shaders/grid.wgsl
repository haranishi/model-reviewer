// 床の格子と軸。高さ0の水平面を画面全体の三角形1枚で描く。画素ごとに、逆行列から視線を作って平面との
// 交点を求め、交点の座標から線の濃さを、交点の深度を frag_depth に出す。視線は近い面（深度1）と
// 深度0.5の2点を戻して作るので、透視でも平行投影でも同じ式で働く（交点が2点の外でもよい）。

struct GridOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> GridOut {
    let x = f32((index << 1u) & 2u) * 2.0 - 1.0;
    let y = f32(index & 2u) * 2.0 - 1.0;
    var out: GridOut;
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.ndc = vec2<f32>(x, y);
    return out;
}

struct GridFrag {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
};

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

// 線の太さ（画素）。
const MAJOR_PX: f32 = 1.3;
const MINOR_PX: f32 = 1.0;
const AXIS_PX: f32 = 2.6;

// 格子の座標 c（間隔で割った値）での線の被覆率。太さは画素で決める。
fn coverage(c: vec2<f32>, d: vec2<f32>, width_px: f32) -> f32 {
    let g = abs(fract(c - 0.5) - 0.5) / max(d, vec2<f32>(1e-12));
    let l = min(g.x, g.y);
    return clamp(width_px * 0.5 + 0.5 - l, 0.0, 1.0);
}

fn axis_coverage(v: f32, d: f32) -> f32 {
    return clamp(AXIS_PX * 0.5 + 0.5 - abs(v) / max(d, 1e-12), 0.0, 1.0);
}

@fragment
fn fs_main(in: GridOut) -> GridFrag {
    let a4 = frame.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let b4 = frame.inv_view_proj * vec4<f32>(in.ndc, 0.5, 1.0);
    let a = a4.xyz / a4.w;
    let b = b4.xyz / b4.w;
    let dir = b - a;
    let flat_ray = abs(dir.y) < 1e-12;
    let t = -a.y / select(dir.y, 1.0, flat_ray);
    let hit = a + dir * t;
    let clip = frame.view_proj * vec4<f32>(hit, 1.0);
    // 深度は 0〜1 に収める。平行投影の遠い面は全体の境界のすぐ外にあり、近い面は目の後ろ（負）にも
    // なる。そこを外れた交点も捨てずに、端の深度で描く（遠い側は0より僅かに大きくして、クリア値の0に
    // 「手前」として勝たせる）。透視では目の前（clip.w > 0）の交点だけを描く。
    let depth = clamp(clip.z / clip.w, 1e-9, 1.0);

    // 微分は捨てる判定より前に、一様な流れの中で求める。
    let p = hit.xz;
    let spacing = max(frame.params.x, 1e-6);
    let dp = fwidth(p);
    let major_c = p / spacing;
    let minor_c = p / (spacing * 0.1);
    let major_d = fwidth(major_c);
    let minor_d = fwidth(minor_c);
    let major = coverage(major_c, major_d, MAJOR_PX);
    let minor = coverage(minor_c, minor_d, MINOR_PX);
    // 1マスが数画素より小さくなる所（遠く・浅い角度）では線を薄くする。
    let major_fade = 1.0 - smoothstep(0.12, 0.4, max(major_d.x, major_d.y));
    let minor_fade = 1.0 - smoothstep(0.08, 0.25, max(minor_d.x, minor_d.y));
    let x_axis = axis_coverage(p.y, dp.y) * (1.0 - smoothstep(0.5, 2.0, dp.y / spacing));
    let z_axis = axis_coverage(p.x, dp.x) * (1.0 - smoothstep(0.5, 2.0, dp.x / spacing));

    var rgb = vec3<f32>(0.42, 0.44, 0.48);
    var alpha = max(major * 0.42 * major_fade, minor * 0.16 * minor_fade);
    let x_color = srgb_to_linear(vec3<f32>(224.0, 101.0, 95.0) / 255.0);
    let z_color = srgb_to_linear(vec3<f32>(91.0, 141.0, 239.0) / 255.0);
    rgb = mix(rgb, x_color, x_axis);
    alpha = max(alpha, x_axis * 0.95);
    rgb = mix(rgb, z_color, z_axis);
    alpha = max(alpha, z_axis * 0.95);

    // 平行投影では clip.w は常に1なので、目の後ろ側の交点も残る（線上のどこでも見えるため）。
    let valid = !flat_ray && clip.w > 0.0 && alpha > 0.004;
    if (!valid) {
        discard;
    }
    var out: GridFrag;
    out.color = vec4<f32>(finish_color(rgb), alpha);
    out.depth = depth;
    return out;
}
