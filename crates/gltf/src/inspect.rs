//! 容器、JSON、必須拡張、範囲、ノード階層を、形状を読む前に検査する。

use gltf::json::Root;
use gltf::json::accessor::{ComponentType, Type};
use gltf::json::mesh::Semantic;
use gltf::json::validation::Checked;

use crate::error::{ExtensionKind, LoadError};

const ALLOWED: &[&str] = &[
    "KHR_materials_unlit",
    "KHR_materials_emissive_strength",
    "KHR_materials_ior",
    "KHR_materials_transmission",
    "KHR_texture_transform",
    "KHR_lights_punctual",
];

pub struct Opened<'a> {
    pub document: gltf::Document,
    pub bin: &'a [u8],
    pub simplified: Vec<String>,
    pub unknown_optional: Vec<String>,
}

pub fn open(bytes: &[u8]) -> Result<Opened<'_>, LoadError> {
    if bytes.len() < 4 || &bytes[..4] != b"glTF" {
        return Err(LoadError::NotGlb);
    }
    let glb = gltf::binary::Glb::from_slice(bytes).map_err(container_error)?;
    let json = glb.json.to_vec();
    let has_bin = glb.bin.is_some();
    drop(glb);
    let bin = if has_bin { bin_chunk(bytes)? } else { &[] };

    let mut value: gltf::json::Value = gltf::json::deserialize::from_slice(&json)
        .map_err(|error| LoadError::Json(error.to_string()))?;
    strip_unknown_extensions(&mut value, 0)?;
    let root: Root = gltf::json::deserialize::from_value(value)
        .map_err(|error| LoadError::Json(error.to_string()))?;

    check_required(&root)?;
    check_buffers(&root, bin, has_bin)?;
    check_positions(&root)?;
    check_accessors(&root, bin)?;
    check_nodes(&root)?;
    let (simplified, unknown_optional) = extension_warnings(&root);

    // gltf-json は範囲外の参照を検証中に panic する。壊れたGLBではエラーにする。
    let document = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        gltf::Document::from_json(root)
    })) {
        Ok(result) => result.map_err(document_error)?,
        Err(_) => {
            return Err(LoadError::BadData {
                what: "glTFの検証".to_owned(),
            });
        }
    };
    Ok(Opened {
        document,
        bin,
        simplified,
        unknown_optional,
    })
}

pub(crate) fn fit_u32(value: u64, what: &str) -> Result<u32, LoadError> {
    u32::try_from(value).map_err(|_| LoadError::TooLarge {
        what: what.to_owned(),
    })
}

fn container_error(error: gltf::Error) -> LoadError {
    let detail = match error {
        gltf::Error::Binary(inner) => inner.to_string(),
        other => format!("{other:?}"),
    };
    LoadError::Container(detail)
}

fn document_error(error: gltf::Error) -> LoadError {
    match error {
        gltf::Error::Deserialize(inner) => LoadError::Json(inner.to_string()),
        gltf::Error::Validation(errors) => {
            let detail = errors
                .iter()
                .take(4)
                .map(|(path, error)| format!("{path}: {error:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            LoadError::Invalid(detail)
        }
        other => LoadError::Invalid(format!("{other:?}")),
    }
}

fn bin_chunk(bytes: &[u8]) -> Result<&[u8], LoadError> {
    let json_len = read_u32(bytes, 12)? as usize;
    let json_end = 20usize
        .checked_add(json_len)
        .ok_or_else(|| LoadError::Container("JSONの長さ".to_owned()))?;
    let bin_len = read_u32(bytes, json_end)? as usize;
    let kind = bytes
        .get(json_end + 4..json_end + 8)
        .ok_or_else(|| LoadError::Container("BIN".to_owned()))?;
    if kind != b"BIN\0" {
        return Err(LoadError::Container("BINチャンクがありません".to_owned()));
    }
    let start = json_end
        .checked_add(8)
        .ok_or_else(|| LoadError::Container("BIN".to_owned()))?;
    let end = start
        .checked_add(bin_len)
        .ok_or_else(|| LoadError::Container("BINの長さ".to_owned()))?;
    bytes
        .get(start..end)
        .ok_or_else(|| LoadError::Container("BINの長さ".to_owned()))
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32, LoadError> {
    let chunk = bytes
        .get(at..at + 4)
        .ok_or_else(|| LoadError::Container("長さ".to_owned()))?;
    let bytes: [u8; 4] = chunk
        .try_into()
        .map_err(|_| LoadError::Container("長さ".to_owned()))?;
    Ok(u32::from_le_bytes(bytes))
}

fn strip_unknown_extensions(value: &mut gltf::json::Value, depth: u32) -> Result<(), LoadError> {
    if depth > 32 {
        return Err(LoadError::BadData {
            what: "JSONの入れ子".to_owned(),
        });
    }
    match value {
        gltf::json::Value::Array(items) => {
            for item in items {
                strip_unknown_extensions(item, depth + 1)?;
            }
        }
        gltf::json::Value::Object(map) => {
            if let Some(extensions) = map.get_mut("extensions") {
                if let Some(object) = extensions.as_object_mut() {
                    object.retain(|key, _| ALLOWED.iter().any(|name| name == key));
                }
                let empty = extensions
                    .as_object()
                    .is_some_and(|object| object.is_empty());
                if empty {
                    map.remove("extensions");
                }
            }
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    strip_unknown_extensions(child, depth + 1)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn extension_kind(name: &str) -> ExtensionKind {
    match name {
        "KHR_draco_mesh_compression" | "EXT_meshopt_compression" => {
            ExtensionKind::GeometryCompression
        }
        "KHR_mesh_quantization" => ExtensionKind::Quantization,
        "EXT_mesh_gpu_instancing" => ExtensionKind::Instancing,
        _ => ExtensionKind::Unknown,
    }
}

fn check_required(root: &Root) -> Result<(), LoadError> {
    for name in &root.extensions_required {
        if ALLOWED.iter().any(|allowed| *allowed == name) {
            continue;
        }
        return Err(LoadError::UnsupportedRequiredExtension {
            name: name.clone(),
            kind: extension_kind(name),
        });
    }
    Ok(())
}

fn check_buffers(root: &Root, bin: &[u8], has_bin: bool) -> Result<(), LoadError> {
    let mut refs = 0u32;
    for buffer in &root.buffers {
        if buffer.uri.is_some() {
            return Err(LoadError::ExternalBuffer);
        }
        refs = refs.saturating_add(1);
        if refs > 1 {
            return Err(LoadError::BadData {
                what: "バッファ".to_owned(),
            });
        }
    }
    if refs > 0 && !has_bin {
        return Err(LoadError::MissingBin);
    }
    let bin_len = u64::try_from(bin.len()).unwrap_or(u64::MAX);
    for buffer in &root.buffers {
        if buffer.uri.is_none() && buffer.byte_length.0 > bin_len {
            return Err(LoadError::BadData {
                what: "バッファ".to_owned(),
            });
        }
    }
    Ok(())
}

fn check_positions(root: &Root) -> Result<(), LoadError> {
    for mesh in &root.meshes {
        for primitive in &mesh.primitives {
            let has_position = primitive
                .attributes
                .keys()
                .any(|semantic| matches!(semantic.as_ref(), Checked::Valid(Semantic::Positions)));
            if !has_position {
                return Err(LoadError::BadData {
                    what: "POSITION".to_owned(),
                });
            }
        }
    }
    Ok(())
}

fn check_accessors(root: &Root, bin: &[u8]) -> Result<(), LoadError> {
    let bin_len = u64::try_from(bin.len()).unwrap_or(u64::MAX);
    for view in &root.buffer_views {
        if view.buffer.value() >= root.buffers.len() {
            return Err(LoadError::BadData {
                what: "bufferView".to_owned(),
            });
        }
        let offset = view.byte_offset.map(|value| value.0).unwrap_or(0);
        let end = offset
            .checked_add(view.byte_length.0)
            .ok_or_else(|| LoadError::BadData {
                what: "範囲".to_owned(),
            })?;
        let buffer_len = root.buffers[view.buffer.value()].byte_length.0;
        if end > buffer_len || end > bin_len {
            return Err(LoadError::BadData {
                what: "範囲".to_owned(),
            });
        }
    }
    for accessor in &root.accessors {
        if accessor.count.0 > u64::from(u32::MAX) {
            return Err(LoadError::TooLarge {
                what: "頂点数".to_owned(),
            });
        }
        if accessor.count.0 == 0 {
            return Err(LoadError::BadData {
                what: "個数".to_owned(),
            });
        }
        if accessor.sparse.is_some() {
            return Err(LoadError::UnsupportedSparse);
        }
        let Some(view_index) = accessor.buffer_view else {
            continue;
        };
        let view = root
            .buffer_views
            .get(view_index.value())
            .ok_or_else(|| LoadError::BadData {
                what: "bufferView".to_owned(),
            })?;
        let element = element_size(accessor)?;
        let stride = match view.byte_stride {
            Some(gltf::json::buffer::Stride(0)) | None => element,
            Some(gltf::json::buffer::Stride(stride)) => {
                let stride = u64::try_from(stride).map_err(|_| LoadError::BadData {
                    what: "stride".to_owned(),
                })?;
                if stride < element {
                    return Err(LoadError::BadData {
                        what: "stride".to_owned(),
                    });
                }
                stride
            }
        };
        let view_offset = view.byte_offset.map(|value| value.0).unwrap_or(0);
        let accessor_offset = accessor.byte_offset.map(|value| value.0).unwrap_or(0);
        let start = view_offset
            .checked_add(accessor_offset)
            .ok_or_else(|| LoadError::BadData {
                what: "範囲".to_owned(),
            })?;
        let span = stride
            .checked_mul(accessor.count.0 - 1)
            .and_then(|value| value.checked_add(element))
            .ok_or_else(|| LoadError::BadData {
                what: "範囲".to_owned(),
            })?;
        let end = start.checked_add(span).ok_or_else(|| LoadError::BadData {
            what: "範囲".to_owned(),
        })?;
        let view_end =
            view_offset
                .checked_add(view.byte_length.0)
                .ok_or_else(|| LoadError::BadData {
                    what: "範囲".to_owned(),
                })?;
        if end > view_end {
            return Err(LoadError::BadData {
                what: "範囲".to_owned(),
            });
        }
        let max_count = bin.len() / usize::try_from(element).unwrap_or(usize::MAX).max(1);
        if accessor.count.0 as usize > max_count {
            return Err(LoadError::BadData {
                what: "範囲".to_owned(),
            });
        }
    }
    Ok(())
}

fn element_size(accessor: &gltf::json::Accessor) -> Result<u64, LoadError> {
    let component = match accessor.component_type.as_ref() {
        Checked::Valid(gltf::json::accessor::GenericComponentType(component)) => *component,
        Checked::Invalid => {
            return Err(LoadError::BadData {
                what: "アクセサの型".to_owned(),
            });
        }
    };
    let kind = match accessor.type_.as_ref() {
        Checked::Valid(kind) => *kind,
        Checked::Invalid => {
            return Err(LoadError::BadData {
                what: "アクセサの型".to_owned(),
            });
        }
    };
    let component_size: u64 = match component {
        ComponentType::I8 | ComponentType::U8 => 1,
        ComponentType::I16 | ComponentType::U16 => 2,
        ComponentType::U32 | ComponentType::F32 => 4,
    };
    let components: u64 = match kind {
        Type::Scalar => 1,
        Type::Vec2 => 2,
        Type::Vec3 => 3,
        Type::Vec4 | Type::Mat2 => 4,
        Type::Mat3 => 9,
        Type::Mat4 => 16,
    };
    component_size
        .checked_mul(components)
        .ok_or_else(|| LoadError::BadData {
            what: "アクセサの型".to_owned(),
        })
}

pub(crate) fn check_nodes(root: &Root) -> Result<(), LoadError> {
    let count = root.nodes.len();
    if count > u32::MAX as usize {
        return Err(LoadError::TooLarge {
            what: "インスタンス数".to_owned(),
        });
    }
    let mut children = Vec::with_capacity(count);
    let mut parent_count = vec![0u32; count];
    for node in &root.nodes {
        let mut kids = Vec::new();
        if let Some(list) = &node.children {
            for child in list {
                let index = child.value();
                if index >= count {
                    return Err(LoadError::BadData {
                        what: "ノード".to_owned(),
                    });
                }
                parent_count[index] = parent_count[index].saturating_add(1);
                if parent_count[index] > 1 {
                    return Err(LoadError::NodeGraph("複数の親".to_owned()));
                }
                kids.push(index);
            }
        }
        children.push(kids);
    }
    let mut color = vec![0u8; count];
    let mut stack = Vec::new();
    for start in 0..count {
        if color[start] != 0 {
            continue;
        }
        color[start] = 1;
        stack.push((start, 0usize));
        while let Some((node, child_index)) = stack.last().copied() {
            if child_index < children[node].len() {
                if let Some(top) = stack.last_mut() {
                    top.1 = child_index + 1;
                }
                let child = children[node][child_index];
                match color[child] {
                    1 => return Err(LoadError::NodeGraph("循環".to_owned())),
                    0 => {
                        color[child] = 1;
                        stack.push((child, 0));
                    }
                    _ => {}
                }
            } else {
                color[node] = 2;
                stack.pop();
            }
        }
    }
    let roots = scene_roots(root)?;
    let mut seen = vec![false; count];
    for index in roots {
        if index >= count {
            return Err(LoadError::BadData {
                what: "シーン".to_owned(),
            });
        }
        if parent_count[index] > 0 || seen[index] {
            return Err(LoadError::NodeGraph("シーンの根".to_owned()));
        }
        seen[index] = true;
    }
    Ok(())
}

pub(crate) fn scene_roots(root: &Root) -> Result<Vec<usize>, LoadError> {
    if root.scenes.is_empty() {
        let mut parents = vec![0u32; root.nodes.len()];
        for node in &root.nodes {
            if let Some(list) = &node.children {
                for child in list {
                    let index = child.value();
                    if index < parents.len() {
                        parents[index] = parents[index].saturating_add(1);
                    }
                }
            }
        }
        return Ok((0..root.nodes.len())
            .filter(|index| parents[*index] == 0)
            .collect());
    }
    let scene_index = root.scene.map(|index| index.value()).unwrap_or(0);
    let scene = root
        .scenes
        .get(scene_index)
        .ok_or_else(|| LoadError::BadData {
            what: "シーン".to_owned(),
        })?;
    Ok(scene.nodes.iter().map(|node| node.value()).collect())
}

fn extension_warnings(root: &Root) -> (Vec<String>, Vec<String>) {
    let mut simplified = Vec::new();
    let mut unknown = Vec::new();
    let mut seen = Vec::new();
    for name in root
        .extensions_used
        .iter()
        .chain(root.extensions_required.iter())
    {
        if seen.iter().any(|seen: &String| seen == name) {
            continue;
        }
        seen.push(name.clone());
        if ALLOWED.iter().any(|allowed| *allowed == name) {
            simplified.push(name.clone());
        } else if !root
            .extensions_required
            .iter()
            .any(|required| required == name)
        {
            unknown.push(name.clone());
        }
    }
    simplified.sort();
    unknown.sort();
    (simplified, unknown)
}

#[cfg(test)]
mod tests {
    use super::fit_u32;
    use crate::error::LoadError;

    #[test]
    fn instance_count_above_u32_is_too_large() {
        let error = fit_u32(u64::from(u32::MAX) + 1, "インスタンス数").unwrap_err();
        match error {
            LoadError::TooLarge { what } => assert_eq!(what, "インスタンス数"),
            other => panic!("{other:?}"),
        }
    }
}
