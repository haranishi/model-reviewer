//! 部品と描画の順に見た目を解決する。写す元は文書の形状番号ではなく、現在の土台のノードから引く。

use std::collections::{HashMap, HashSet};

use gltf::json::Value;
use model_reviewer_core::texture::same_look;

use super::append::{Appender, Geometry, PrimRefs};
use super::base::Base;
use super::json::{f32s, obj, push, remove};
use super::textures::{TextureEdit, TextureWriter, box_size, can_mark, invalid, model_look};
use super::{ExportError, ExportNote, ExportScene};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Source {
    Material(Option<usize>),
    Shape,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct MaterialKey {
    source: Source,
    color: Option<[u32; 4]>,
    texture: TextureEdit,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Change {
    slot: usize,
    material: usize,
    geometry: Option<PrimRefs>,
}

pub(super) struct Restyled {
    pub mesh_of: Vec<Option<usize>>,
    pub notes: Vec<ExportNote>,
}

pub(super) fn apply(
    base: &Base<'_>,
    scene: &ExportScene,
    root: &mut Value,
    bin: &mut Appender,
) -> Result<Restyled, ExportError> {
    let mut textures = TextureWriter::new(base);
    let mut materials = HashMap::new();
    let mut meshes = HashMap::new();
    let mut mesh_of = vec![None; scene.nodes.len()];
    let mut colored_nodes = 0;
    let mut colored_materials = HashSet::new();
    let mut colored_meshes = HashSet::new();
    let mut textured_nodes = 0;
    let mut unmarked = false;

    for (at, node) in scene.nodes.iter().enumerate() {
        let file = node.file_index.or(node.template);
        let range = file
            .map(|id| {
                base.draw_range(id)
                    .ok_or_else(|| invalid("部品の元のノードがありません"))
            })
            .transpose()?;
        if range.as_ref().is_some_and(|r| r.len() != node.draws.len()) {
            return Err(invalid("部品の描画の数が元と違います"));
        }
        let mesh = file
            .and_then(|id| base.value["nodes"][id as usize]["mesh"].as_u64())
            .map(|n| n as usize);
        let may_bake = std::cell::LazyCell::new(|| file.is_none_or(|id| base.can_bake(id)));
        let mut changes = Vec::new();
        let mut recolored = false;
        let mut retextured = false;
        for (i, draw) in node.draws.iter().enumerate() {
            let look = scene.textures.look(draw.tex)?;
            let base_index = range.as_ref().map(|r| r.start + i);
            let original = base_index.map(|index| &base.model.draws[index]);
            let previous = original.and_then(|old| model_look(&base.model, old.tex));
            let color_changed = original.is_some_and(|old| {
                old.base_color.map(f32::to_bits) != draw.base_color.map(f32::to_bits)
            });
            let texture_changed = !same_look(previous, look);
            if range.is_some() && !color_changed && !texture_changed {
                continue;
            }
            if previous.and_then(|(tex, _)| tex.cutoff) != look.and_then(|(tex, _)| tex.cutoff) {
                return Err(invalid("材質の透明度の閾値が元と違います"));
            }
            let size = box_size(look).filter(|_| texture_changed);
            if file.is_none() && look.is_some() && size.is_none() {
                return Err(invalid("追加形状に使える貼り方は箱形です"));
            }
            if size.is_some() && !*may_bake {
                return Err(invalid(
                    "スキンまたはモーフのある部品には箱形の投影を書き出せません",
                ));
            }
            let texture = if texture_changed {
                textures.edit(root, bin, &scene.textures, look)?
            } else {
                TextureEdit::Keep
            };
            let (slot, source, refs) = if let Some(index) = base_index {
                let source = base.sources[index];
                let slot = source.primitive as usize;
                let primitive = &base.value["meshes"][source.mesh as usize]["primitives"][slot];
                let material = primitive["material"].as_u64().map(|n| n as usize);
                let prim = base.model.draws[index].prim.0;
                let refs = size
                    .map(|s| {
                        bin.primitive(
                            Geometry::Base(prim),
                            &base.model.primitives[prim as usize],
                            Some(s),
                        )
                    })
                    .transpose()?;
                (slot, Source::Material(material), refs)
            } else {
                // shape() が追加形状の範囲を確かめている。
                let slot = draw.prim.0 - scene.base_primitive_count;
                let refs =
                    bin.primitive(Geometry::Added(slot), &scene.added[slot as usize], size)?;
                (i, Source::Shape, Some(refs))
            };
            let write_color =
                color_changed || matches!(source, Source::Shape | Source::Material(None));
            let key = MaterialKey {
                source,
                color: write_color.then(|| draw.base_color.map(f32::to_bits)),
                texture,
            };
            let material = match materials.get(&key) {
                Some(&id) => id,
                None => {
                    let value = material_copy(&base.value, key, &mut unmarked)?;
                    let id = push(root, "materials", value)?;
                    materials.insert(key, id);
                    id
                }
            };
            if color_changed {
                colored_materials.insert(material);
                recolored = true;
            }
            retextured |= texture_changed;
            changes.push(Change {
                slot,
                material,
                geometry: refs,
            });
        }
        if changes.is_empty() {
            continue;
        }
        let key = (mesh, changes);
        let id = match meshes.get(&key) {
            Some(&id) => id,
            None => {
                let value = mesh_copy(&base.value, mesh, &key.1)?;
                let id = push(root, "meshes", value)?;
                meshes.insert(key, id);
                id
            }
        };
        mesh_of[at] = Some(id);
        if recolored {
            colored_nodes += 1;
            colored_meshes.insert(id);
        }
        textured_nodes += u32::from(retextured);
    }
    let mut notes = Vec::new();
    if colored_nodes > 0 {
        notes.push(ExportNote::Recolored {
            nodes: colored_nodes,
            materials: colored_materials.len() as u32,
            meshes: colored_meshes.len() as u32,
        });
    }
    if textured_nodes > 0 {
        notes.push(ExportNote::Retextured {
            nodes: textured_nodes,
            images: textures.image_count(),
            image_bytes: textures.image_bytes,
            baked: bin.baked,
        });
    }
    if unmarked {
        notes.push(ExportNote::BoxSizeUnmarked);
    }
    Ok(Restyled { mesh_of, notes })
}

fn material_copy(
    root: &Value,
    key: MaterialKey,
    unmarked: &mut bool,
) -> Result<Value, ExportError> {
    let mut value = match key.source {
        Source::Material(Some(index)) => root["materials"][index].clone(),
        Source::Material(None) | Source::Shape => obj(),
    };
    let pbr = &mut value["pbrMetallicRoughness"];
    if key.source == Source::Shape {
        pbr["metallicFactor"] = 0.0.into();
    }
    if let Some(bits) = key.color {
        pbr["baseColorFactor"] = f32s(&bits.map(f32::from_bits))?;
    }
    match key.texture {
        TextureEdit::Keep => {}
        TextureEdit::Remove => remove(pbr, "baseColorTexture"),
        TextureEdit::Vertex { texture, from } => {
            let mut info =
                root["materials"][from as usize]["pbrMetallicRoughness"]["baseColorTexture"]
                    .clone();
            if !info.is_object() {
                return Err(invalid("貼り方の写し元に基本色の画像がありません"));
            }
            info["index"] = texture.into();
            pbr["baseColorTexture"] = info;
        }
        TextureEdit::Box { texture, size } => {
            let mut info = obj();
            info["index"] = texture.into();
            pbr["baseColorTexture"] = info;
            if can_mark(&value) {
                value["extras"]["model_reviewer"]["box_size"] =
                    f64::from(f32::from_bits(size)).into();
            } else {
                *unmarked = true;
            }
        }
    }
    Ok(value)
}

fn mesh_copy(root: &Value, mesh: Option<usize>, changes: &[Change]) -> Result<Value, ExportError> {
    let mut value = match mesh {
        Some(index) => root["meshes"][index].clone(),
        None => {
            let mut value = obj();
            value["primitives"] = Value::Array(vec![Value::Null; changes.len()]);
            value
        }
    };
    let primitives = value["primitives"]
        .as_array_mut()
        .ok_or_else(|| invalid("メッシュに形状がありません"))?;
    for change in changes {
        let primitive = primitives
            .get_mut(change.slot)
            .ok_or_else(|| invalid("形状の番号が範囲の外です"))?;
        if let Some(refs) = change.geometry {
            *primitive = refs.primitive(change.material);
        } else {
            primitive["material"] = change.material.into();
        }
    }
    Ok(value)
}
