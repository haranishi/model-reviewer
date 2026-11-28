//! 部品と描画の順に見た目を解決する。写す元は文書の形状番号ではなく、現在の土台のノードから引く。

use std::collections::{HashMap, HashSet};

use gltf::json::Value;

use super::append::{Appender, Geometry, PrimRefs, SourceUvs};
use super::base::Base;
use super::json::{f32s, obj, push, remove};
use super::textures::{
    TextureEdit, TextureWriter, box_size, can_mark, color_texture, invalid, model_look,
    same_texture,
};
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

#[derive(Clone, PartialEq, Eq, Hash)]
enum PrimChange {
    /// 形状はそのまま、材質だけを替える。
    Material(usize),
    Baked(PrimRefs, usize),
    /// 開いた時点のメッシュ `mesh` の同じ番号の形状。材質が `None` なら材質も元のまま。
    Prior {
        mesh: usize,
        material: Option<usize>,
    },
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Change {
    slot: usize,
    primitive: PrimChange,
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
        let original = file.and_then(|id| scene.originals.get(&id)).filter(|o| {
            Some(o.mesh as usize) != mesh
                && (o.mesh as usize) < super::json::array(&base.value, "meshes").len()
                && o.draws.len() == node.draws.len()
        });
        let mut changes = Vec::new();
        let mut recolored = false;
        let mut retextured = false;
        for (i, draw) in node.draws.iter().enumerate() {
            let look = scene.textures.look(draw.tex)?;
            let base_index = range.as_ref().map(|r| r.start + i);
            let original_mesh = original.map(|o| o.mesh as usize);
            // 土台の形状が、前の書き出しで焼いた形状に替わっているか（開いた時点の形状と比べる）。
            let opened = match (original, base_index, mesh) {
                (Some(o), Some(index), Some(current)) => {
                    let slot = base.sources[index].primitive as usize;
                    let prior = &base.value["meshes"][o.mesh as usize]["primitives"][slot];
                    let now = &base.value["meshes"][current]["primitives"][slot];
                    rebaked(prior, now).then_some((slot, prior, &o.draws[i]))
                }
                _ => None,
            };
            // 箱形でない見た目が戻ったら、開いた時点の形状へ戻す。
            let prior = opened.filter(|_| box_size(look).is_none());
            if let (Some((slot, prior, old)), Some(m)) = (prior, original_mesh) {
                let previous = scene.textures.look(old.tex)?;
                let color_changed =
                    old.base_color.map(f32::to_bits) != draw.base_color.map(f32::to_bits);
                let texture_changed = !same_texture(previous, look, &base.value, &base.value);
                let material = if !color_changed && !texture_changed {
                    None
                } else {
                    if previous.and_then(|(tex, _)| tex.cutoff)
                        != look.and_then(|(tex, _)| tex.cutoff)
                    {
                        return Err(invalid("材質の透明度の閾値が元と違います"));
                    }
                    let texture = if texture_changed {
                        textures.edit(root, bin, &scene.textures, look)?
                    } else {
                        TextureEdit::Keep
                    };
                    let source = prior["material"].as_u64().map(|n| n as usize);
                    let key = MaterialKey {
                        source: Source::Material(source),
                        color: (color_changed || source.is_none())
                            .then(|| draw.base_color.map(f32::to_bits)),
                        texture,
                    };
                    let id = material_id(&mut materials, root, &base.value, key, &mut unmarked)?;
                    if color_changed {
                        colored_materials.insert(id);
                        recolored = true;
                    }
                    retextured |= texture_changed;
                    Some(id)
                };
                changes.push(Change {
                    slot,
                    primitive: PrimChange::Prior { mesh: m, material },
                });
                continue;
            }
            let original = base_index.map(|index| &base.model.draws[index]);
            let previous = original.and_then(|old| model_look(&base.model, old.tex));
            let color_changed = original.is_some_and(|old| {
                old.base_color.map(f32::to_bits) != draw.base_color.map(f32::to_bits)
            });
            let texture_changed = !same_texture(previous, look, &base.value, &base.value);
            if range.is_some() && !color_changed && !texture_changed {
                continue;
            }
            if previous.and_then(|(tex, _)| tex.cutoff) != look.and_then(|(tex, _)| tex.cutoff) {
                return Err(invalid("材質の透明度の閾値が元と違います"));
            }
            let sides = box_size(look)
                .filter(|_| texture_changed)
                .map(|(_, sides)| sides);
            if file.is_none() && look.is_some() && sides.is_none() {
                return Err(invalid("追加形状に使える貼り方は箱形です"));
            }
            if sides.is_some() && !*may_bake {
                return Err(invalid(
                    "スキンまたはモーフのある部品には箱形の投影を書き出せません",
                ));
            }
            let mut texture = if texture_changed {
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
                let refs = match (sides, opened.zip(original_mesh)) {
                    (None, _) => None,
                    // 焼いた形状を焼き直すと、前の箱形の組まで写して組が増える。開いた時点の
                    // 形状と UV の組から焼く。
                    (Some(s), Some(((_, first, _), m))) => {
                        let geometry = base.file_primitive(m, slot)?;
                        Some(bin.primitive(
                            Geometry::File {
                                mesh: m as u32,
                                slot: slot as u32,
                            },
                            &geometry,
                            Some(s),
                            SourceUvs {
                                key: uv_accessors(first),
                                read: Box::new(move || base.bake_uv_sets(m, slot)),
                            },
                        )?)
                    }
                    (Some(s), None) if let Some((index, geometry)) = scene.added_geometry(draw) => {
                        Some(bin.primitive(
                            Geometry::Added(index),
                            geometry,
                            Some(s),
                            SourceUvs::none(),
                        )?)
                    }
                    (Some(s), None) => {
                        let mesh = source.mesh as usize;
                        Some(bin.primitive(
                            Geometry::Base(prim),
                            &base.model.primitives[prim as usize],
                            Some(s),
                            SourceUvs {
                                key: uv_accessors(primitive),
                                read: Box::new(move || base.bake_uv_sets(mesh, slot)),
                            },
                        )?)
                    }
                };
                (slot, Source::Material(material), refs)
            } else {
                // shape() が追加形状の範囲を確かめている。
                let slot = draw.prim.0 - scene.base_primitive_count;
                let refs = bin.primitive(
                    Geometry::Added(slot),
                    &scene.added[slot as usize],
                    sides,
                    SourceUvs::none(),
                )?;
                (i, Source::Shape, Some(refs))
            };
            if let (TextureEdit::Box { set, .. }, Some(refs)) = (&mut texture, &refs) {
                *set = refs.box_set;
            }
            let write_color =
                color_changed || matches!(source, Source::Shape | Source::Material(None));
            let key = MaterialKey {
                source,
                color: write_color.then(|| draw.base_color.map(f32::to_bits)),
                texture,
            };
            let material = material_id(&mut materials, root, &base.value, key, &mut unmarked)?;
            if color_changed {
                colored_materials.insert(material);
                recolored = true;
            }
            retextured |= texture_changed;
            changes.push(Change {
                slot,
                primitive: match refs {
                    Some(refs) => PrimChange::Baked(refs, material),
                    None => PrimChange::Material(material),
                },
            });
        }
        if changes.is_empty() {
            continue;
        }
        let restored = original.map(|o| o.mesh as usize);
        let key = (mesh, changes);
        let id = match meshes.get(&key) {
            Some(&id) => id,
            None => {
                let value = mesh_copy(&base.value, mesh, &key.1)?;
                // 全部が開いた時点に戻れば、写さずに元のメッシュを指す。
                let id = match restored.filter(|m| base.value["meshes"][*m] == value) {
                    Some(m) => m,
                    None => push(root, "meshes", value)?,
                };
                meshes.insert(key, id);
                id
            }
        };
        mesh_of[at] = Some(id);
        if recolored && Some(id) != restored {
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
    if bin.dropped_sets > 0 {
        notes.push(ExportNote::UvSetsDropped {
            sets: bin.dropped_sets,
        });
    }
    Ok(Restyled { mesh_of, notes })
}

fn material_id(
    materials: &mut HashMap<MaterialKey, usize>,
    root: &mut Value,
    base: &Value,
    key: MaterialKey,
    unmarked: &mut bool,
) -> Result<usize, ExportError> {
    if let Some(&id) = materials.get(&key) {
        return Ok(id);
    }
    let value = material_copy(base, key, unmarked)?;
    let id = push(root, "materials", value)?;
    materials.insert(key, id);
    Ok(id)
}

/// 形状が元から材質以外で変わっている（箱形で焼いた形状に替わっている）か。
fn rebaked(prior: &Value, now: &Value) -> bool {
    let strip = |p: &Value| {
        let mut p = p.clone();
        remove(&mut p, "material");
        p
    };
    strip(prior) != strip(now)
}

/// 形状の `TEXCOORD_n` の組の番号と accessor。番号順。
pub(super) fn uv_accessors(primitive: &Value) -> Vec<(u32, u64)> {
    let mut sets: Vec<_> = primitive["attributes"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| {
            let set = key.strip_prefix("TEXCOORD_")?.parse().ok()?;
            Some((set, value.as_u64()?))
        })
        .collect();
    sets.sort_unstable();
    sets
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
        TextureEdit::Unresolved { from } => {
            pbr["baseColorTexture"] = color_texture(root, from).clone()
        }
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
        TextureEdit::Box { texture, size, set } => {
            let mut info = obj();
            info["index"] = texture.into();
            if set > 0 {
                info["texCoord"] = set.into();
            }
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
        match &change.primitive {
            PrimChange::Material(material) => primitive["material"] = (*material).into(),
            PrimChange::Baked(refs, material) => *primitive = refs.primitive(*material),
            PrimChange::Prior { mesh, material } => {
                *primitive = root["meshes"][*mesh]["primitives"][change.slot].clone();
                if let Some(material) = material {
                    primitive["material"] = (*material).into();
                }
            }
        }
    }
    Ok(value)
}
