//! 検出表の一行ずつに対応する、壊れた、または警告だけの GLB。

use crate::glb_writer::write_glb;

pub fn bytes() -> Vec<u8> {
    wrong_magic()
}

pub fn wrong_magic() -> Vec<u8> {
    let mut bytes = triangle();
    bytes[..4].copy_from_slice(b"XXXX");
    bytes
}

pub fn truncated() -> Vec<u8> {
    triangle()[..20].to_vec()
}

pub fn bad_json() -> Vec<u8> {
    write_glb("{", Some(&[0, 0, 0, 0]))
}

pub fn view_past_bin() -> Vec<u8> {
    with_json_patch(triangle(), "\"byteLength\":36", "\"byteLength\":400")
}

pub fn index_oob() -> Vec<u8> {
    // 位置 36 バイト、法線 36 バイトの直後がインデックス。3 つ目を 5 にする。
    let mut bytes = triangle();
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().expect("json len")) as usize;
    let index_at = 20 + json_len + 8 + 72 + 8;
    bytes[index_at..index_at + 4].copy_from_slice(&5u32.to_le_bytes());
    bytes
}

pub fn nan_positions() -> Vec<u8> {
    poke_first_position(triangle(), f32::NAN)
}

pub fn inf_positions() -> Vec<u8> {
    poke_first_position(triangle(), f32::INFINITY)
}

pub fn cyclic_nodes() -> Vec<u8> {
    scene_glb(SceneKind::Cycle, Extras::default())
}

pub fn two_parents() -> Vec<u8> {
    scene_glb(SceneKind::TwoParents, Extras::default())
}

pub fn scene_root_is_child() -> Vec<u8> {
    scene_glb(SceneKind::RootIsChild, Extras::default())
}

pub fn required_extension(name: &str) -> Vec<u8> {
    scene_glb(
        SceneKind::One,
        Extras {
            required: vec![name.to_owned()],
            used: vec![name.to_owned()],
            ..Extras::default()
        },
    )
}

pub fn external_buffer() -> Vec<u8> {
    scene_glb(
        SceneKind::One,
        Extras {
            uri: Some("mesh.bin".to_owned()),
            ..Extras::default()
        },
    )
}

pub fn data_uri_buffer() -> Vec<u8> {
    scene_glb(
        SceneKind::One,
        Extras {
            uri: Some("data:application/octet-stream,AAAA".to_owned()),
            ..Extras::default()
        },
    )
}

pub fn missing_bin() -> Vec<u8> {
    scene_glb(
        SceneKind::One,
        Extras {
            omit_bin: true,
            ..Extras::default()
        },
    )
}

pub fn no_mesh() -> Vec<u8> {
    scene_glb(SceneKind::NoMesh, Extras::default())
}

pub fn huge_vertex_count() -> Vec<u8> {
    with_json_patch(triangle(), "\"count\":3", "\"count\":4294967296")
}

pub fn zero_count() -> Vec<u8> {
    with_json_patch(
        triangle(),
        "\"count\":3,\"type\":\"VEC3\"",
        "\"count\":0,\"type\":\"VEC3\"",
    )
}

pub fn lines_and_points() -> Vec<u8> {
    scene_glb(
        SceneKind::One,
        Extras {
            lines: true,
            points: true,
            ..Extras::default()
        },
    )
}

pub fn lines_only() -> Vec<u8> {
    scene_glb(
        SceneKind::One,
        Extras {
            lines: true,
            drop_triangles: true,
            ..Extras::default()
        },
    )
}

pub fn triangle_strip() -> Vec<u8> {
    scene_glb(
        SceneKind::Strip,
        Extras {
            mode: Some(5),
            ..Extras::default()
        },
    )
}

pub fn triangle_fan() -> Vec<u8> {
    scene_glb(
        SceneKind::Fan,
        Extras {
            mode: Some(6),
            ..Extras::default()
        },
    )
}

pub fn textures() -> Vec<u8> {
    scene_glb(
        SceneKind::One,
        Extras {
            texture: true,
            ..Extras::default()
        },
    )
}

pub fn vertex_colors() -> Vec<u8> {
    scene_glb(
        SceneKind::One,
        Extras {
            vertex_color: true,
            ..Extras::default()
        },
    )
}

pub fn alpha_blend() -> Vec<u8> {
    scene_glb(
        SceneKind::One,
        Extras {
            alpha_blend: true,
            ..Extras::default()
        },
    )
}

pub fn animation_skin_morph() -> Vec<u8> {
    scene_glb(
        SceneKind::One,
        Extras {
            animation: true,
            skin: true,
            morph: true,
            ..Extras::default()
        },
    )
}

pub fn unknown_optional(name: &str) -> Vec<u8> {
    scene_glb(
        SceneKind::One,
        Extras {
            used: vec![name.to_owned()],
            ..Extras::default()
        },
    )
}

pub fn allowlisted_unlit() -> Vec<u8> {
    scene_glb(
        SceneKind::One,
        Extras {
            used: vec!["KHR_materials_unlit".to_owned()],
            required: vec!["KHR_materials_unlit".to_owned()],
            material_ext: Some("\"KHR_materials_unlit\":{}".to_owned()),
            ..Extras::default()
        },
    )
}

pub fn allowlisted_emissive() -> Vec<u8> {
    scene_glb(
        SceneKind::One,
        Extras {
            used: vec!["KHR_materials_emissive_strength".to_owned()],
            required: vec!["KHR_materials_emissive_strength".to_owned()],
            material_ext: Some(
                "\"KHR_materials_emissive_strength\":{\"emissiveStrength\":2.0}".to_owned(),
            ),
            ..Extras::default()
        },
    )
}

pub fn optional_draco_with_fallback() -> Vec<u8> {
    scene_glb(
        SceneKind::One,
        Extras {
            used: vec!["KHR_draco_mesh_compression".to_owned()],
            primitive_ext: Some(
                "\"KHR_draco_mesh_compression\":{\"bufferView\":0,\"attributes\":{\"POSITION\":0}}"
                    .to_owned(),
            ),
            ..Extras::default()
        },
    )
}

pub fn optional_draco_without_position() -> Vec<u8> {
    scene_glb(
        SceneKind::One,
        Extras {
            used: vec!["KHR_draco_mesh_compression".to_owned()],
            omit_position: true,
            ..Extras::default()
        },
    )
}

pub fn optional_meshopt_with_fallback() -> Vec<u8> {
    scene_glb(
        SceneKind::One,
        Extras {
            used: vec!["EXT_meshopt_compression".to_owned()],
            ..Extras::default()
        },
    )
}

pub fn deep_chain(depth: u32) -> Vec<u8> {
    scene_glb(SceneKind::Deep(depth.max(1)), Extras::default())
}

pub fn triangle() -> Vec<u8> {
    scene_glb(SceneKind::One, Extras::default())
}

/// 正しい三角形に、位置アクセサの sparse だけを足す。
pub fn sparse_accessor() -> Vec<u8> {
    scene_glb(
        SceneKind::One,
        Extras {
            sparse: true,
            ..Extras::default()
        },
    )
}

#[derive(Clone, Default)]
struct Extras {
    used: Vec<String>,
    required: Vec<String>,
    uri: Option<String>,
    omit_bin: bool,
    alpha_blend: bool,
    vertex_color: bool,
    texture: bool,
    animation: bool,
    skin: bool,
    morph: bool,
    lines: bool,
    points: bool,
    drop_triangles: bool,
    mode: Option<u32>,
    omit_position: bool,
    material_ext: Option<String>,
    primitive_ext: Option<String>,
    sparse: bool,
}

enum SceneKind {
    One,
    Cycle,
    TwoParents,
    RootIsChild,
    NoMesh,
    Strip,
    Fan,
    Deep(u32),
}

fn scene_glb(kind: SceneKind, extras: Extras) -> Vec<u8> {
    let positions: Vec<[f32; 3]> = match kind {
        SceneKind::Strip | SceneKind::Fan => vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
        ],
        _ => vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
    };
    let normals: Vec<[f32; 3]> = vec![[0.0, 0.0, 1.0]; positions.len()];
    let tri_indices: Vec<u32> = match kind {
        SceneKind::Strip | SceneKind::Fan => vec![0, 1, 2, 3],
        _ => vec![0, 1, 2],
    };
    let mut bin = Vec::new();
    let pos_off = 0u32;
    for position in &positions {
        for value in position {
            bin.extend_from_slice(&value.to_le_bytes());
        }
    }
    let pos_len = bin.len() as u32;
    let nrm_off = bin.len() as u32;
    for normal in &normals {
        for value in normal {
            bin.extend_from_slice(&value.to_le_bytes());
        }
    }
    let nrm_len = bin.len() as u32 - nrm_off;
    let idx_off = bin.len() as u32;
    for index in &tri_indices {
        bin.extend_from_slice(&index.to_le_bytes());
    }
    let idx_len = bin.len() as u32 - idx_off;
    let line_off = bin.len() as u32;
    bin.extend_from_slice(&0u32.to_le_bytes());
    bin.extend_from_slice(&1u32.to_le_bytes());
    let line_len = 8u32;
    let point_off = bin.len() as u32;
    bin.extend_from_slice(&0u32.to_le_bytes());
    let point_len = 4u32;
    let time_off = bin.len() as u32;
    bin.extend_from_slice(&0f32.to_le_bytes());
    bin.extend_from_slice(&1f32.to_le_bytes());
    let time_len = 8u32;
    let morph_off = bin.len() as u32;
    for _ in 0..positions.len() {
        bin.extend_from_slice(&0f32.to_le_bytes());
        bin.extend_from_slice(&0f32.to_le_bytes());
        bin.extend_from_slice(&0.1f32.to_le_bytes());
    }
    let morph_len = bin.len() as u32 - morph_off;

    let pos_count = positions.len();
    let mut views = vec![
        format!("{{\"buffer\":0,\"byteOffset\":{pos_off},\"byteLength\":{pos_len}}}"),
        format!("{{\"buffer\":0,\"byteOffset\":{nrm_off},\"byteLength\":{nrm_len}}}"),
        format!("{{\"buffer\":0,\"byteOffset\":{idx_off},\"byteLength\":{idx_len}}}"),
    ];
    let mut accessors = vec![
        format!(
            "{{\"bufferView\":0,\"componentType\":5126,\"count\":{pos_count},\"type\":\"VEC3\",\"min\":[0.0,0.0,0.0],\"max\":[1.0,1.0,0.0]{}{}}}",
            if extras.sparse { "," } else { "" },
            if extras.sparse {
                "\"sparse\":{\"count\":1,\"indices\":{\"bufferView\":2,\"componentType\":5123},\"values\":{\"bufferView\":0}}"
            } else {
                ""
            }
        ),
        format!(
            "{{\"bufferView\":1,\"componentType\":5126,\"count\":{pos_count},\"type\":\"VEC3\"}}"
        ),
        format!(
            "{{\"bufferView\":2,\"componentType\":5125,\"count\":{},\"type\":\"SCALAR\"}}",
            tri_indices.len()
        ),
    ];
    let mut next_view = 3usize;
    let line_accessor = if extras.lines {
        views.push(format!(
            "{{\"buffer\":0,\"byteOffset\":{line_off},\"byteLength\":{line_len}}}"
        ));
        accessors.push(format!(
            "{{\"bufferView\":{next_view},\"componentType\":5125,\"count\":2,\"type\":\"SCALAR\"}}"
        ));
        let index = accessors.len() - 1;
        next_view += 1;
        Some(index)
    } else {
        None
    };
    let point_accessor = if extras.points {
        views.push(format!(
            "{{\"buffer\":0,\"byteOffset\":{point_off},\"byteLength\":{point_len}}}"
        ));
        accessors.push(format!(
            "{{\"bufferView\":{next_view},\"componentType\":5125,\"count\":1,\"type\":\"SCALAR\"}}"
        ));
        let index = accessors.len() - 1;
        next_view += 1;
        Some(index)
    } else {
        None
    };
    let time_accessor = if extras.animation {
        views.push(format!(
            "{{\"buffer\":0,\"byteOffset\":{time_off},\"byteLength\":{time_len}}}"
        ));
        accessors.push(format!(
            "{{\"bufferView\":{next_view},\"componentType\":5126,\"count\":2,\"type\":\"SCALAR\",\"min\":[0.0],\"max\":[1.0]}}"
        ));
        let index = accessors.len() - 1;
        next_view += 1;
        Some(index)
    } else {
        None
    };
    let morph_accessor = if extras.morph {
        views.push(format!(
            "{{\"buffer\":0,\"byteOffset\":{morph_off},\"byteLength\":{morph_len}}}"
        ));
        accessors.push(format!(
            "{{\"bufferView\":{next_view},\"componentType\":5126,\"count\":{pos_count},\"type\":\"VEC3\"}}"
        ));
        Some(accessors.len() - 1)
    } else {
        None
    };
    let _ = next_view;

    let color_attr = if extras.vertex_color {
        ",\"COLOR_0\":0".to_owned()
    } else {
        String::new()
    };
    let position_attr = if extras.omit_position {
        String::new()
    } else {
        format!("\"POSITION\":0,\"NORMAL\":1{color_attr}")
    };
    let mode = extras
        .mode
        .map(|mode| format!(",\"mode\":{mode}"))
        .unwrap_or_default();
    let targets = morph_accessor
        .map(|index| format!(",\"targets\":[{{\"POSITION\":{index}}}]"))
        .unwrap_or_default();
    let prim_ext = extras
        .primitive_ext
        .as_ref()
        .map(|body| format!(",\"extensions\":{{{body}}}"))
        .unwrap_or_default();
    let mut prims = Vec::new();
    if !extras.drop_triangles {
        prims.push(format!(
            "{{\"attributes\":{{{position_attr}}},\"indices\":2,\"material\":0{mode}{targets}{prim_ext}}}"
        ));
    }
    if let Some(indices) = line_accessor {
        prims.push(format!(
            "{{\"attributes\":{{\"POSITION\":0}},\"indices\":{indices},\"mode\":1,\"material\":0}}"
        ));
    }
    if let Some(indices) = point_accessor {
        prims.push(format!(
            "{{\"attributes\":{{\"POSITION\":0}},\"indices\":{indices},\"mode\":0,\"material\":0}}"
        ));
    }
    if prims.is_empty() {
        prims.push("{\"attributes\":{},\"mode\":1}".to_owned());
    }

    let alpha = if extras.alpha_blend {
        ",\"alphaMode\":\"BLEND\""
    } else {
        ""
    };
    let mat_ext = extras
        .material_ext
        .as_ref()
        .map(|body| format!(",\"extensions\":{{{body}}}"))
        .unwrap_or_default();
    let texture = if extras.texture {
        ",\"baseColorTexture\":{\"index\":0}"
    } else {
        ""
    };
    let material = format!(
        "{{\"pbrMetallicRoughness\":{{\"baseColorFactor\":[0.2,0.4,0.8,1.0]{texture}}}{alpha}{mat_ext}}}"
    );

    let (nodes, scene_nodes) = match kind {
        SceneKind::One | SceneKind::Strip | SceneKind::Fan => (
            "[{\"name\":\"only\",\"mesh\":0}]".to_owned(),
            "[0]".to_owned(),
        ),
        SceneKind::NoMesh => ("[{\"name\":\"empty\"}]".to_owned(), "[0]".to_owned()),
        SceneKind::Cycle => (
            "[{\"name\":\"a\",\"mesh\":0,\"children\":[1]},{\"name\":\"b\",\"children\":[0]}]"
                .to_owned(),
            "[0]".to_owned(),
        ),
        SceneKind::TwoParents => (
            "[{\"name\":\"a\",\"mesh\":0,\"children\":[2]},{\"name\":\"b\",\"children\":[2]},{\"name\":\"c\"}]"
                .to_owned(),
            "[0]".to_owned(),
        ),
        SceneKind::RootIsChild => (
            "[{\"name\":\"a\",\"mesh\":0,\"children\":[1]},{\"name\":\"b\"}]".to_owned(),
            "[0,1]".to_owned(),
        ),
        SceneKind::Deep(depth) => {
            let depth = depth.max(1);
            let mut nodes = Vec::new();
            for index in 0..depth {
                if index + 1 == depth {
                    nodes.push(format!("{{\"name\":\"n{index}\",\"mesh\":0}}"));
                } else {
                    nodes.push(format!(
                        "{{\"name\":\"n{index}\",\"children\":[{}]}}",
                        index + 1
                    ));
                }
            }
            (format!("[{}]", nodes.join(",")), "[0]".to_owned())
        }
    };
    let meshes = if matches!(kind, SceneKind::NoMesh) {
        "[]".to_owned()
    } else {
        format!("[{{\"primitives\":[{}]}}]", prims.join(","))
    };
    let images = if extras.texture {
        ",\"images\":[{\"bufferView\":0,\"mimeType\":\"image/png\"}],\"textures\":[{\"source\":0}]"
    } else {
        ""
    };
    let animations = if let Some(time) = time_accessor {
        format!(
            ",\"animations\":[{{\"channels\":[{{\"sampler\":0,\"target\":{{\"node\":0,\"path\":\"translation\"}}}}],\"samplers\":[{{\"input\":{time},\"output\":0}}]}}]"
        )
    } else {
        String::new()
    };
    let skins = if extras.skin {
        ",\"skins\":[{\"joints\":[0]}]"
    } else {
        ""
    };
    let extensions_used = if extras.used.is_empty() {
        String::new()
    } else {
        format!(
            ",\"extensionsUsed\":[{}]",
            extras
                .used
                .iter()
                .map(|name| format!("\"{name}\""))
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    let extensions_required = if extras.required.is_empty() {
        String::new()
    } else {
        format!(
            ",\"extensionsRequired\":[{}]",
            extras
                .required
                .iter()
                .map(|name| format!("\"{name}\""))
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    let uri = extras
        .uri
        .as_ref()
        .map(|uri| format!(",\"uri\":\"{uri}\""))
        .unwrap_or_default();
    let accessors_json = if matches!(kind, SceneKind::NoMesh) {
        "[]".to_owned()
    } else {
        format!("[{}]", accessors.join(","))
    };
    let views_json = if matches!(kind, SceneKind::NoMesh) {
        "[]".to_owned()
    } else {
        format!("[{}]", views.join(","))
    };
    let buffers = if matches!(kind, SceneKind::NoMesh) {
        "[]".to_owned()
    } else {
        format!("[{{\"byteLength\":{}{uri}}}]", bin.len())
    };
    let json = format!(
        "{{\"asset\":{{\"version\":\"2.0\",\"generator\":\"model-reviewer-testdata\"}},\"scene\":0,\"scenes\":[{{\"nodes\":{scene_nodes}}}],\"nodes\":{nodes},\"meshes\":{meshes},\"materials\":[{material}],\"accessors\":{accessors_json},\"bufferViews\":{views_json},\"buffers\":{buffers}{images}{animations}{skins}{extensions_used}{extensions_required}}}"
    );
    let bin = if extras.omit_bin || matches!(kind, SceneKind::NoMesh) {
        None
    } else {
        Some(bin.as_slice())
    };
    write_glb(&json, bin)
}

fn poke_first_position(mut bytes: Vec<u8>, value: f32) -> Vec<u8> {
    // JSON チャンクの直後が BIN。先頭の f32 が最初の座標。
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().expect("json len")) as usize;
    let bin_at = 12 + 8 + json_len + 8;
    bytes[bin_at..bin_at + 4].copy_from_slice(&value.to_le_bytes());
    bytes
}

fn with_json_patch(bytes: Vec<u8>, from: &str, to: &str) -> Vec<u8> {
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().expect("json len")) as usize;
    let json = bytes[20..20 + json_len].to_vec();
    let text = String::from_utf8_lossy(&json);
    let patched = text.replacen(from, to, 1);
    let bin_at = 20 + json_len;
    let bin = if bin_at + 8 <= bytes.len() {
        let bin_len =
            u32::from_le_bytes(bytes[bin_at..bin_at + 4].try_into().expect("bin len")) as usize;
        Some(bytes[bin_at + 8..bin_at + 8 + bin_len].to_vec())
    } else {
        None
    };
    write_glb(&patched, bin.as_deref())
}
