//! 部品数と三角形数を指定する大きな GLB。形状は 1 つを全部品で共有する。
//! 部品ごとに固有の頂点を持つと、10 万部品 × 200 三角形で約 1.4GB になるため置かない。
//! 置かれる三角形数は部品数 × 三角形数。ファイルは呼び出し側が `target/` へ書く。

use crate::glb_writer::{num, write_glb};
use crate::shapes::triangle_grid;

const MAX_PARTS: u32 = 1_000_000;
const MAX_TRIANGLES: u32 = 1_000_000;

pub fn note() -> &'static str {
    "stress <部品数> <三角形数> [target/ 以下のパス]（階層つきは stress-nested）"
}

pub fn stress_glb(parts: u32, triangles: u32) -> Result<Vec<u8>, &'static str> {
    check_size(parts, triangles)?;
    let mut nodes = Vec::with_capacity(parts as usize);
    let mut roots = Vec::with_capacity(parts as usize);
    for index in 0..parts {
        let x = num(index as f32);
        nodes.push(format!(
            "{{\"name\":\"part-{index}\",\"mesh\":0,\"translation\":[{x},0.0,0.0]}}"
        ));
        roots.push(index.to_string());
    }
    assemble(&nodes, &roots, triangles)
}

/// まとめ役の子孫の数。`stress_nested_glb(parts, _)` で、まとめ役の部分木は自分を除いてこの数になる。
pub fn nested_descendants(parts: u32) -> u32 {
    parts / 2
}

/// まとめ役の下で、何個ごとに中間のまとめ（小さなまとめ役）を置くか。
const SUBGROUP: u32 = 100;

/// `stress_glb` と同じ部品と形状で、根の下に「子孫を半分持つまとめ役」を置く形。部品の数は
/// `parts`（根とまとめ役を含む）、まとめ役の子孫は `parts / 2` 個。決定的に作る。
///
/// 並び（ファイル内の番号）: 0 が根 `root`、1 がまとめ役 `group-half`、続く `parts / 2` 個が
/// まとめ役の子孫（100個ごとに先頭が中間のまとめ `sub-n` で、残りはその子）、残りは根の直接の子。
/// すべての部品が同じ形状を持つので、配置後の三角形数は `placed_triangles(parts, triangles)`。
pub fn stress_nested_glb(parts: u32, triangles: u32) -> Result<Vec<u8>, &'static str> {
    check_size(parts, triangles)?;
    let descendants = nested_descendants(parts);
    if parts < 4 {
        return Err("階層つきの形は部品数 4 以上です");
    }
    let first_other = 2 + descendants;
    let mut nodes = Vec::with_capacity(parts as usize);
    let mut root_children = vec!["1".to_owned()];
    root_children.extend((first_other..parts).map(|index| index.to_string()));
    nodes.push(format!(
        "{{\"name\":\"root\",\"mesh\":0,\"children\":[{}]}}",
        root_children.join(",")
    ));
    let subgroups: Vec<String> = (0..descendants)
        .step_by(SUBGROUP as usize)
        .map(|offset| (2 + offset).to_string())
        .collect();
    nodes.push(format!(
        "{{\"name\":\"group-half\",\"mesh\":0,\"translation\":[0.0,2.0,0.0],\"children\":[{}]}}",
        subgroups.join(",")
    ));
    for offset in 0..descendants {
        let index = 2 + offset;
        let slot = offset % SUBGROUP;
        if slot == 0 {
            let end = (offset + SUBGROUP).min(descendants);
            let children: Vec<String> = (offset + 1..end)
                .map(|child| (2 + child).to_string())
                .collect();
            let y = num((offset / SUBGROUP) as f32);
            let children = if children.is_empty() {
                String::new()
            } else {
                format!(",\"children\":[{}]", children.join(","))
            };
            nodes.push(format!(
                "{{\"name\":\"sub-{}\",\"mesh\":0,\"translation\":[0.0,{y},0.0]{children}}}",
                offset / SUBGROUP
            ));
        } else {
            let x = num(slot as f32);
            nodes.push(format!(
                "{{\"name\":\"part-{index}\",\"mesh\":0,\"translation\":[{x},0.0,0.0]}}"
            ));
        }
    }
    for index in first_other..parts {
        let x = num((index - first_other) as f32);
        nodes.push(format!(
            "{{\"name\":\"part-{index}\",\"mesh\":0,\"translation\":[{x},0.0,-2.0]}}"
        ));
    }
    assemble(&nodes, &["0".to_owned()], triangles)
}

fn check_size(parts: u32, triangles: u32) -> Result<(), &'static str> {
    if parts == 0 || triangles == 0 {
        return Err("部品数と三角形数は 1 以上です");
    }
    if parts > MAX_PARTS || triangles > MAX_TRIANGLES {
        return Err("部品数と三角形数はそれぞれ 1000000 までです");
    }
    Ok(())
}

/// 共有する形状1つと、渡したノードと根でGLBを作る。
fn assemble(nodes: &[String], roots: &[String], triangles: u32) -> Result<Vec<u8>, &'static str> {
    let geometry = triangle_grid(triangles);
    let mut bin = Vec::new();
    for position in &geometry.positions {
        for value in position {
            bin.extend_from_slice(&value.to_le_bytes());
        }
    }
    let pos_len = bin.len() as u32;
    let normal_off = bin.len() as u32;
    let normals = geometry.normals.as_ref().ok_or("法線がありません")?;
    for normal in normals {
        for value in normal {
            bin.extend_from_slice(&value.to_le_bytes());
        }
    }
    let normal_len = bin.len() as u32 - normal_off;
    let index_off = bin.len() as u32;
    for index in &geometry.indices {
        bin.extend_from_slice(&index.to_le_bytes());
    }
    let index_len = bin.len() as u32 - index_off;
    let (min, max) = bounds(&geometry.positions);
    let json = format!(
        "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"model-reviewer-testdata\"}},\"scene\":0,\"scenes\":[{{\"nodes\":[{roots}]}}],\"nodes\":[{nodes}],\"meshes\":[{{\"primitives\":[{{\"attributes\":{{\"POSITION\":0,\"NORMAL\":1}},\"indices\":2,\"material\":0}}]}}],\"materials\":[{{\"pbrMetallicRoughness\":{{\"baseColorFactor\":[0.6,0.6,0.6,1.0]}}}}],\"accessors\":[{{\"bufferView\":0,\"componentType\":5126,\"count\":{pos_count},\"type\":\"VEC3\",\"min\":{min},\"max\":{max}}}, {{\"bufferView\":1,\"componentType\":5126,\"count\":{normal_count},\"type\":\"VEC3\"}}, {{\"bufferView\":2,\"componentType\":5125,\"count\":{index_count},\"type\":\"SCALAR\"}}],\"bufferViews\":[{{\"buffer\":0,\"byteOffset\":0,\"byteLength\":{pos_len}}}, {{\"buffer\":0,\"byteOffset\":{normal_off},\"byteLength\":{normal_len}}}, {{\"buffer\":0,\"byteOffset\":{index_off},\"byteLength\":{index_len}}}],\"buffers\":[{{\"byteLength\":{bin_len}}}]}}",
        roots = roots.join(","),
        nodes = nodes.join(","),
        pos_count = geometry.positions.len(),
        min = format_vec3(min),
        max = format_vec3(max),
        normal_count = normals.len(),
        index_count = geometry.indices.len(),
        bin_len = bin.len(),
    );
    Ok(write_glb(&json, Some(&bin)))
}

pub fn placed_triangles(parts: u32, triangles: u32) -> u64 {
    u64::from(parts) * u64::from(triangles)
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

fn format_vec3(value: [f32; 3]) -> String {
    format!("[{},{},{}]", num(value[0]), num(value[1]), num(value[2]))
}

#[cfg(test)]
mod tests {
    use super::{nested_descendants, stress_glb, stress_nested_glb};

    #[test]
    fn nested_is_deterministic_and_checks_its_size() {
        let first = stress_nested_glb(1_000, 2).unwrap();
        assert_eq!(first, stress_nested_glb(1_000, 2).unwrap());
        assert_ne!(first, stress_glb(1_000, 2).unwrap());
        assert_eq!(nested_descendants(100_000), 50_000);
        assert!(stress_nested_glb(3, 1).is_err());
        assert!(stress_nested_glb(4, 1).is_ok());
        assert!(stress_nested_glb(0, 1).is_err());
        assert!(stress_nested_glb(10, 0).is_err());
    }
}
