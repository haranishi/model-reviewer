//! GLB の容器を自前で書く。gltf クレートには依存しない。

use crate::shapes::Geometry;

/// JSON と任意の BIN から GLB を作る。JSON は 0x20、BIN は 0x00 で 4 バイト境界へ揃える。
pub fn write_glb(json: &str, bin: Option<&[u8]>) -> Vec<u8> {
    let json_len = (json.len() as u64).next_multiple_of(4);
    let bin = bin.map(|bytes| (bytes, (bytes.len() as u64).next_multiple_of(4)));
    let total = 20 + json_len + bin.map_or(0, |(_, length)| 8 + length);
    let total = u32::try_from(total).expect("GLB の長さが32ビットに収まりません");
    let mut out = Vec::with_capacity(total as usize);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&total.to_le_bytes());
    out.extend_from_slice(&(json_len as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(json.as_bytes());
    out.resize(20 + json_len as usize, b' ');
    if let Some((bytes, length)) = bin {
        out.extend_from_slice(&(length as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(bytes);
        out.resize(total as usize, 0);
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

#[derive(Default)]
pub(crate) struct Bank {
    pub bin: Vec<u8>,
    pub views: Vec<String>,
    pub accessors: Vec<String>,
}

impl Bank {
    fn add_vec3(&mut self, values: &[[f32; 3]], min_max: bool) -> usize {
        let mut bytes = Vec::with_capacity(values.len() * 12);
        for value in values {
            for component in value {
                bytes.extend_from_slice(&component.to_le_bytes());
            }
        }
        let view = self.add_view(&bytes, None);
        let mut json = format!(
            "{{\"bufferView\":{view},\"componentType\":5126,\"count\":{},\"type\":\"VEC3\"",
            values.len()
        );
        if min_max {
            let (min, max) = bounds(values);
            json.push_str(&format!(
                ",\"min\":{},\"max\":{}",
                vec3_json(min),
                vec3_json(max)
            ));
        }
        json.push('}');
        self.accessors.push(json);
        self.accessors.len() - 1
    }

    fn add_indices(&mut self, values: &[u32]) -> usize {
        let mut bytes = Vec::with_capacity(values.len() * 4);
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let view = self.add_view(&bytes, None);
        self.accessors.push(format!(
            "{{\"bufferView\":{view},\"componentType\":5125,\"count\":{},\"type\":\"SCALAR\"}}",
            values.len()
        ));
        self.accessors.len() - 1
    }

    pub fn add_view(&mut self, bytes: &[u8], stride: Option<u32>) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        let stride = stride.map_or(String::new(), |n| format!(r#","byteStride":{n}"#));
        self.views.push(format!(
            r#"{{"buffer":0,"byteOffset":{offset},"byteLength":{}{stride}}}"#,
            bytes.len()
        ));
        self.views.len() - 1
    }
}

fn bounds(values: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX, f32::MAX, f32::MAX];
    let mut max = [f32::MIN, f32::MIN, f32::MIN];
    for value in values {
        for axis in 0..3 {
            min[axis] = min[axis].min(value[axis]);
            max[axis] = max[axis].max(value[axis]);
        }
    }
    (min, max)
}

pub(crate) struct Stored {
    position: usize,
    normal: Option<usize>,
    indices: usize,
}

pub(crate) fn store(bank: &mut Bank, geometry: &Geometry) -> Stored {
    let position = bank.add_vec3(&geometry.positions, true);
    let normal = geometry
        .normals
        .as_ref()
        .map(|normals| bank.add_vec3(normals, false));
    let indices = bank.add_indices(&geometry.indices);
    Stored {
        position,
        normal,
        indices,
    }
}

impl Stored {
    pub fn primitive(&self, material: Option<u32>, uv_sets: &[usize]) -> String {
        let mut attributes = format!(r#""POSITION":{}"#, self.position);
        if let Some(normal) = self.normal {
            attributes.push_str(&format!(r#","NORMAL":{normal}"#));
        }
        for (set, accessor) in uv_sets.iter().enumerate() {
            attributes.push_str(&format!(r#","TEXCOORD_{set}":{accessor}"#));
        }
        let material = material.map_or(String::new(), |i| format!(r#","material":{i}"#));
        format!(
            r#"{{"attributes":{{{attributes}}},"indices":{}{material}}}"#,
            self.indices
        )
    }
}
