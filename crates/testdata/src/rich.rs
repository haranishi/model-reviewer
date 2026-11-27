//! 書き出しの試験に使うGLB。テクスチャ・アニメーション・スキン・カメラ・知らない任意の拡張・
//! `extras` を、中身は偽でも構造として含む（本アプリの読み込みで開けること）。書き出しがそれらを
//! 値として保つかを確かめるための入力で、決定的に作る。
//!
//! ノード（番号は下の定数）:
//! ```text
//! シーン0（既定）: ROOT ─┬ BODY（matrix・分けられる。mesh 0 テクスチャ付き）── HAND（mesh 1 を共有・知らない拡張）
//!                        ├ ARM（アニメーションの対象・extras。mesh 1）── SKINNED（スキン付き。mesh 2）
//!                        ├ JOINT（名前なし・カメラ・スキンの関節。子なし）
//!                        └ SKEWED（matrix・傾いた拡大で分けられない。mesh 0）
//! シーン1: ORPHAN（mesh 1）
//! ```
//! BINの中身は421バイトで、`buffers[0].byteLength` は421、BINチャンクは0で詰めて424バイト。

use crate::glb_writer::write_glb;

pub const ROOT: u32 = 0;
pub const BODY: u32 = 1;
pub const ARM: u32 = 2;
pub const HAND: u32 = 3;
pub const SKINNED: u32 = 4;
pub const JOINT: u32 = 5;
pub const SKEWED: u32 = 6;
pub const ORPHAN: u32 = 7;
/// 既定のシーンから読み込まれる部品の数（ORPHAN を除く）。
pub const LOADED_NODES: u32 = 7;
/// `buffers[0].byteLength`（BINチャンクの長さより短い）。
pub const BIN_BYTE_LENGTH: u32 = 421;

struct Bin {
    bytes: Vec<u8>,
    views: Vec<String>,
}

impl Bin {
    fn align(&mut self) {
        while !self.bytes.len().is_multiple_of(4) {
            self.bytes.push(0);
        }
    }

    fn view(&mut self, data: &[u8], target: Option<u32>) {
        let offset = self.bytes.len();
        self.bytes.extend_from_slice(data);
        let target = target.map_or(String::new(), |target| format!(",\"target\":{target}"));
        self.views.push(format!(
            "{{\"buffer\":0,\"byteOffset\":{offset},\"byteLength\":{}{target}}}",
            data.len()
        ));
    }
}

fn f32_bytes(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

pub fn rich_glb() -> Vec<u8> {
    let mut bin = Bin {
        bytes: Vec::new(),
        views: Vec::new(),
    };
    // 0〜3: 四角（テクスチャ座標つき、16ビットのインデックス）。
    let quad = [
        0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0f32,
    ];
    bin.view(&f32_bytes(&quad), Some(34962));
    bin.view(&f32_bytes(&[0.0, 0.0, 1.0f32].repeat(4)), Some(34962));
    bin.view(
        &f32_bytes(&[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0f32]),
        Some(34962),
    );
    let quad_indices: Vec<u8> = [0u16, 1, 2, 0, 2, 3]
        .iter()
        .flat_map(|index| index.to_le_bytes())
        .collect();
    bin.view(&quad_indices, Some(34963));
    // 4: 三角形（位置だけ、インデックスなし）。
    bin.view(
        &f32_bytes(&[0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5, 0.0f32]),
        Some(34962),
    );
    // 5〜9: スキン付きの三角形（関節・重み、8ビットのインデックス）。
    bin.view(
        &f32_bytes(&[0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0f32]),
        Some(34962),
    );
    bin.view(&f32_bytes(&[1.0, 0.0, 0.0f32].repeat(3)), Some(34962));
    bin.view(&[0u8; 12], Some(34962));
    bin.view(&f32_bytes(&[1.0, 0.0, 0.0, 0.0f32].repeat(3)), Some(34962));
    bin.view(&[0u8, 1, 2], Some(34963));
    bin.align();
    // 10: 逆バインド行列。11〜12: アニメーションの時刻と値。
    let identity = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0f32,
    ];
    bin.view(&f32_bytes(&identity), None);
    bin.view(&f32_bytes(&[0.0, 1.0f32]), None);
    bin.view(&f32_bytes(&[0.0, 0.0, 0.0, 0.0, 1.0, 0.0f32]), None);
    // 13: 画像（中身は偽のPNG）。
    bin.view(b"\x89PNG\r\n\x1a\nfake!", None);
    let byte_length = bin.bytes.len();
    debug_assert_eq!(byte_length, BIN_BYTE_LENGTH as usize);

    let json = format!(
        r#"{{"asset":{{"version":"2.0","generator":"model-reviewer-testdata rich","copyright":"testdata","extras":{{"k":1}}}},
"extensionsUsed":["EXT_testdata_unknown"],
"extensions":{{"EXT_testdata_unknown":{{"nodes":[3,4],"note":"偽の拡張"}}}},
"extras":{{"author":"testdata","nested":{{"a":[1,2.5,"x",null,true],"b":0.1}}}},
"scene":0,
"scenes":[{{"name":"main","nodes":[0],"extras":{{"s":0}}}},{{"name":"other","nodes":[7]}}],
"nodes":[
{{"name":"root","translation":[0.0,0.5,0.0],"children":[1,2,5,6]}},
{{"name":"body","mesh":0,"matrix":[2.0,0.0,0.0,0.0,0.0,2.0,0.0,0.0,0.0,0.0,2.0,0.0,0.0,1.0,0.0,1.0],"children":[3]}},
{{"name":"arm","mesh":1,"translation":[1.0,0.0,0.0],"rotation":[0.0,0.0,0.38268343,0.9238795],"children":[4],"extras":{{"rig":"arm","weight":0.75}}}},
{{"name":"hand","mesh":1,"translation":[0.0,0.25,0.0],"scale":[0.5,0.5,0.5],"extensions":{{"EXT_testdata_unknown":{{"target":2}}}}}},
{{"name":"skinned","mesh":2,"skin":0,"translation":[0.0,0.0,0.1]}},
{{"camera":0,"translation":[0.0,0.0,3.0]}},
{{"name":"skewed","mesh":0,"matrix":[1.0,0.0,0.0,0.0,0.5,1.0,0.0,0.0,0.0,0.0,1.0,0.0,3.0,0.0,0.0,1.0]}},
{{"name":"orphan","mesh":1}}
],
"meshes":[
{{"name":"quad","primitives":[{{"attributes":{{"POSITION":0,"NORMAL":1,"TEXCOORD_0":2}},"indices":3,"material":0}}],"extras":{{"m":"quad"}}}},
{{"name":"tri","primitives":[{{"attributes":{{"POSITION":4}},"material":1}}]}},
{{"name":"skin-tri","primitives":[{{"attributes":{{"POSITION":5,"NORMAL":6,"JOINTS_0":7,"WEIGHTS_0":8}},"indices":9}}]}}
],
"materials":[
{{"name":"textured","pbrMetallicRoughness":{{"baseColorFactor":[0.8,0.7,0.6,1.0],"baseColorTexture":{{"index":0,"texCoord":0}},"metallicFactor":0.0}},"extensions":{{"EXT_testdata_unknown":{{"glow":1}}}},"extras":{{"finish":"matte"}}}},
{{"pbrMetallicRoughness":{{"baseColorFactor":[0.2,0.4,0.9,1.0]}},"doubleSided":true}}
],
"textures":[{{"sampler":0,"source":0}}],
"samplers":[{{"magFilter":9729,"minFilter":9987,"wrapS":10497,"wrapT":33648}}],
"images":[{{"bufferView":13,"mimeType":"image/png","name":"fake"}}],
"cameras":[{{"type":"perspective","perspective":{{"yfov":0.8,"znear":0.1,"zfar":100.0}}}}],
"skins":[{{"name":"skin","joints":[5],"inverseBindMatrices":10,"skeleton":5}}],
"animations":[{{"name":"wave","channels":[{{"sampler":0,"target":{{"node":2,"path":"translation"}}}}],"samplers":[{{"input":11,"output":12,"interpolation":"LINEAR"}}],"extras":{{"loop":true}}}}],
"accessors":[
{{"bufferView":0,"componentType":5126,"count":4,"type":"VEC3","min":[0.0,0.0,0.0],"max":[1.0,1.0,0.0]}},
{{"bufferView":1,"componentType":5126,"count":4,"type":"VEC3"}},
{{"bufferView":2,"componentType":5126,"count":4,"type":"VEC2"}},
{{"bufferView":3,"componentType":5123,"count":6,"type":"SCALAR"}},
{{"bufferView":4,"componentType":5126,"count":3,"type":"VEC3","min":[0.0,0.0,0.0],"max":[0.5,0.5,0.0]}},
{{"bufferView":5,"componentType":5126,"count":3,"type":"VEC3","min":[0.0,0.0,0.0],"max":[0.0,1.0,1.0]}},
{{"bufferView":6,"componentType":5126,"count":3,"type":"VEC3"}},
{{"bufferView":7,"componentType":5121,"count":3,"type":"VEC4"}},
{{"bufferView":8,"componentType":5126,"count":3,"type":"VEC4"}},
{{"bufferView":9,"componentType":5121,"count":3,"type":"SCALAR"}},
{{"bufferView":10,"componentType":5126,"count":1,"type":"MAT4"}},
{{"bufferView":11,"componentType":5126,"count":2,"type":"SCALAR","min":[0.0],"max":[1.0]}},
{{"bufferView":12,"componentType":5126,"count":2,"type":"VEC3"}}
],
"bufferViews":[{views}],
"buffers":[{{"byteLength":{byte_length},"name":"bin","extras":{{"b":true}}}}]}}"#,
        views = bin.views.join(","),
    );
    write_glb(&json, Some(&bin.bytes))
}

#[cfg(test)]
mod tests {
    use super::{BIN_BYTE_LENGTH, rich_glb};

    #[test]
    fn rich_is_deterministic_and_keeps_bin_padding() {
        let bytes = rich_glb();
        assert_eq!(bytes, rich_glb());
        let json_len = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        let bin_at = 20 + json_len;
        let bin_len = u32::from_le_bytes([
            bytes[bin_at],
            bytes[bin_at + 1],
            bytes[bin_at + 2],
            bytes[bin_at + 3],
        ]);
        assert_eq!(&bytes[bin_at + 4..bin_at + 8], b"BIN\0");
        assert!(bin_len > BIN_BYTE_LENGTH);
        assert!(bin_len.is_multiple_of(4));
    }
}
