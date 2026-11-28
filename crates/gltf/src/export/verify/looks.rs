//! ノードから写しをたどり、元の材質・描画ごとに変更を許す項目を限定する。

use std::collections::{HashMap, HashSet};

use gltf::json::Value;
use model_reviewer_core::scene::Draw;
use model_reviewer_core::texture::{UvMap, same_look};

use super::{fail, without};
use crate::export::base::Base;
use crate::export::json::{array, obj};
use crate::export::textures::{Look, box_size, can_mark, model_look};
use crate::export::{ExportError, ExportNode, ExportTextures};

pub(super) struct MeshChecks<'a> {
    old: &'a Value,
    new: &'a Value,
    base: &'a Base<'a>,
    textures: &'a ExportTextures,
    origins: HashMap<usize, Option<usize>>,
    materials: HashSet<usize>,
}

impl<'a> MeshChecks<'a> {
    pub fn new(
        old: &'a Value,
        new: &'a Value,
        base: &'a Base<'a>,
        textures: &'a ExportTextures,
    ) -> Self {
        Self {
            old,
            new,
            base,
            textures,
            origins: HashMap::new(),
            materials: HashSet::new(),
        }
    }

    pub fn node(&mut self, node: &ExportNode, written: &Value) -> Result<(), ExportError> {
        let file = node.file_index.or(node.template);
        let before = file.map(|id| &self.old["nodes"][id as usize]);
        if before.and_then(|n| n.get("mesh")) == written.get("mesh") {
            return Ok(());
        }
        let origin = before.and_then(|n| index(n, "mesh"));
        let id = index(written, "mesh")
            .ok_or_else(|| fail("写したメッシュの番号がありません".into()))?;
        if id < array(self.old, "meshes").len()
            || id >= array(self.new, "meshes").len()
            || self
                .origins
                .insert(id, origin)
                .is_some_and(|other| other != origin)
        {
            return Err(fail("メッシュの写す元か書き出し先が違います".into()));
        }
        let mesh = &self.new["meshes"][id];
        let original = origin.map(|n| &self.old["meshes"][n]);
        let rest = original.map_or_else(obj, |m| without(m, &["primitives"]));
        let count = original.map_or(node.draws.len(), |m| array(m, "primitives").len());
        let primitives = array(mesh, "primitives");
        if primitives.len() != count || without(mesh, &["primitives"]) != rest {
            return Err(fail("メッシュの項目か形状の数が違います".into()));
        }
        let range = file.and_then(|n| self.base.draw_range(n));
        if range.as_ref().is_some_and(|r| r.len() != node.draws.len()) {
            return Err(fail("元の部品と文書の描画の数が違います".into()));
        }
        let may_bake = std::cell::LazyCell::new(|| file.is_none_or(|id| self.base.can_bake(id)));
        let mut changed = false;
        let mut draw_index = 0;
        for (slot, primitive) in primitives.iter().enumerate() {
            let prior = original.map(|m| &m["primitives"][slot]);
            let old_draw = if let Some(r) = &range {
                let i = r.start + draw_index;
                if i == r.end || self.base.sources[i].primitive as usize != slot {
                    if Some(primitive) != prior {
                        return Err(fail("描画しない形状が元と違います".into()));
                    }
                    continue;
                }
                Some(&self.base.model.draws[i])
            } else {
                None
            };
            let draw = node
                .draws
                .get(draw_index)
                .ok_or_else(|| fail("文書に形状がありません".into()))?;
            draw_index += 1;
            let look = self
                .textures
                .look(draw.tex)
                .map_err(|e| fail(e.message_ja()))?;
            let original_look = old_draw.and_then(|old| model_look(&self.base.model, old.tex));
            let texture_changed = !same_look(look, original_look);
            let color_changed = old_draw.is_none_or(|old| {
                old.base_color.map(f32::to_bits) != draw.base_color.map(f32::to_bits)
            });
            if !color_changed && !texture_changed {
                if prior != Some(primitive) {
                    return Err(fail("見た目を変えていない描画が元と違います".into()));
                }
                continue;
            }
            changed = true;
            let boxed = texture_changed && box_size(look).is_some();
            if boxed || prior.is_none() {
                let first_accessor = array(self.old, "accessors").len();
                if !new_geometry(primitive, first_accessor, boxed)
                    || (boxed && (written.get("skin").is_some() || !*may_bake))
                {
                    return Err(fail(
                        "作り直した形状の属性またはスキン・モーフが不正です".into(),
                    ));
                }
            } else if prior
                .is_some_and(|p| without(p, &["material"]) != without(primitive, &["material"]))
            {
                return Err(fail("形状の材質以外の項目が元と違います".into()));
            }
            let material = index(primitive, "material")
                .ok_or_else(|| fail("写した材質の番号がありません".into()))?;
            if material < array(self.old, "materials").len()
                || material >= array(self.new, "materials").len()
            {
                return Err(fail("形状が足した材質を指していません".into()));
            }
            self.materials.insert(material);
            let old_material = prior
                .and_then(|p| index(p, "material"))
                .map(|n| &self.old["materials"][n]);
            let mut expected = old_material.cloned().unwrap_or_else(obj);
            if prior.is_none() {
                expected["pbrMetallicRoughness"]["metallicFactor"] = 0.0.into();
            }
            self.check_material(
                &mut expected,
                &self.new["materials"][material],
                draw,
                look,
                color_changed || old_material.is_none(),
                texture_changed,
            )?;
        }
        if !changed || draw_index != node.draws.len() {
            return Err(fail("メッシュを写す描画が文書と違います".into()));
        }
        Ok(())
    }

    fn check_material(
        &self,
        expected: &mut Value,
        written: &Value,
        draw: &Draw,
        look: Look<'_>,
        write_color: bool,
        texture_changed: bool,
    ) -> Result<(), ExportError> {
        const PBR: &str = "pbrMetallicRoughness";
        let info = written[PBR].get("baseColorTexture");
        if write_color {
            let factor = &written[PBR]["baseColorFactor"];
            let valid = factor.as_array().is_some_and(|rgba| {
                rgba.len() == 4
                    && rgba.iter().zip(draw.base_color).all(|(v, c)| {
                        v.as_f64()
                            .is_some_and(|n| (n as f32).to_bits() == c.to_bits())
                    })
            });
            if !valid {
                return Err(fail("基本色が文書と違います".into()));
            }
            expected[PBR]["baseColorFactor"] = factor.clone();
        }
        if texture_changed {
            match look.filter(|(tex, _)| tex.image.is_some()) {
                None => {
                    if info.is_some() {
                        return Err(fail("外した基本色の画像が残っています".into()));
                    }
                    if let Some(pbr) = expected[PBR].as_object_mut() {
                        pbr.remove("baseColorTexture");
                    }
                }
                Some((tex, _)) => {
                    let info = info
                        .filter(|value| {
                            index(value, "index")
                                .is_some_and(|i| i < array(self.new, "textures").len())
                        })
                        .ok_or_else(|| fail("基本色のテクスチャ番号が不正です".into()))?;
                    let original_info = match tex.map {
                        UvMap::Vertex {
                            info_from: Some(from),
                            ..
                        } => {
                            let original =
                                &self.old["materials"][from as usize][PBR]["baseColorTexture"];
                            if !original.is_object() {
                                return Err(fail("写し元に基本色の画像がありません".into()));
                            }
                            without(original, &["index"])
                        }
                        UvMap::Box { size } => {
                            if can_mark(expected) {
                                let mark = &written["extras"]["model_reviewer"]["box_size"];
                                if !mark
                                    .as_f64()
                                    .is_some_and(|n| (n as f32).to_bits() == size.to_bits())
                                {
                                    return Err(fail("箱形の大きさの記録が文書と違います".into()));
                                }
                                expected["extras"]["model_reviewer"]["box_size"] = mark.clone();
                            }
                            obj()
                        }
                        _ => return Err(fail("UV の写し元がありません".into())),
                    };
                    if without(info, &["index"]) != original_info {
                        return Err(fail("UV の番号・変換・追加情報が写し元と違います".into()));
                    }
                    expected[PBR]["baseColorTexture"] = info.clone();
                }
            }
        }
        if expected != written {
            return Err(fail("変更対象以外の材質の項目が元と違います".into()));
        }
        Ok(())
    }

    pub fn finish(self) -> Result<(), ExportError> {
        let all_used = (array(self.old, "meshes").len()..array(self.new, "meshes").len())
            .all(|i| self.origins.contains_key(&i))
            && (array(self.old, "materials").len()..array(self.new, "materials").len())
                .all(|i| self.materials.contains(&i));
        if !all_used {
            return Err(fail("使われていないメッシュか材質を足しています".into()));
        }
        Ok(())
    }
}

fn index(value: &Value, key: &str) -> Option<usize> {
    usize::try_from(value.get(key)?.as_u64()?).ok()
}

fn new_geometry(p: &Value, first_accessor: usize, boxed: bool) -> bool {
    let Some(attributes) = p["attributes"].as_object() else {
        return false;
    };
    without(p, &["attributes", "indices", "material", "mode"]) == obj()
        && (p.get("mode").is_none() && !boxed || p["mode"].as_u64() == Some(4))
        && index(p, "indices").is_some_and(|i| i >= first_accessor)
        && attributes.contains_key("POSITION")
        && attributes.contains_key("TEXCOORD_0") == boxed
        && attributes.iter().all(|(k, v)| {
            matches!(k.as_str(), "POSITION" | "NORMAL" | "TEXCOORD_0")
                && v.as_u64().is_some_and(|i| i >= first_accessor as u64)
        })
}
