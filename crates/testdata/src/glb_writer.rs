//! GLB の容器を自前で書く。gltf クレートには依存しない。

/// JSON と任意の BIN から GLB を作る。JSON は 0x20、BIN は 0x00 で 4 バイト境界へ揃える。
pub fn write_glb(json: &str, bin: Option<&[u8]>) -> Vec<u8> {
    let mut json_bytes = json.as_bytes().to_vec();
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    let bin_bytes = bin.map(|bytes| {
        let mut padded = bytes.to_vec();
        while !padded.len().is_multiple_of(4) {
            padded.push(0);
        }
        padded
    });
    let mut total = 12 + 8 + json_bytes.len();
    if let Some(bin) = bin_bytes.as_ref() {
        total += 8 + bin.len();
    }
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    if let Some(bin) = bin_bytes.as_ref() {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(bin);
    }
    out
}

pub fn num(value: f32) -> String {
    format!("{value:?}")
}

pub fn vec3_json(value: [f32; 3]) -> String {
    format!("[{},{},{}]", num(value[0]), num(value[1]), num(value[2]))
}

pub fn vec4_json(value: [f32; 4]) -> String {
    format!(
        "[{},{},{},{}]",
        num(value[0]),
        num(value[1]),
        num(value[2]),
        num(value[3])
    )
}
