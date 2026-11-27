//! 読み込んだモデル。ノードは深さ優先の行きがけ順で、子孫は `[id, subtree_end)` に並ぶ。

use std::ops::Range;

use glam::Affine3A;

use crate::math::Aabb;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PrimId(pub u32);

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Primitive {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub local_aabb: Aabb,
    pub has_normals: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Draw {
    pub prim: PrimId,
    pub base_color: [f32; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub name: String,
    pub gltf_index: u32,
    pub parent: Option<NodeId>,
    pub subtree_end: u32,
    pub depth: u32,
    /// 親に対する行列。読み込みがファイルの値から作ったものをそのまま入れる（ワールドから逆算しない）。
    pub local: Affine3A,
    pub world: Affine3A,
    pub draws: Range<u32>,
    pub world_aabb: Aabb,
}

/// 部品の表を読む口。`Model`（読み込んだまま）と `core::doc::Document`（編集中）が実装する。
/// 当たり判定・表示状態・レビューJSON・インスタンス作成は、この口だけを見る。
///
/// 約束: 部品は行きがけ順で、子孫は `[id, subtree_end)` に連続する。`NodeId` を取る関数は
/// `id < node_count()` を前提にし、範囲外はパニックしてよい（`Model::node` と同じ）。
pub trait SceneRead {
    fn node_count(&self) -> usize;
    fn parent(&self, id: NodeId) -> Option<NodeId>;
    fn subtree_end(&self, id: NodeId) -> u32;
    fn depth(&self, id: NodeId) -> u32;
    fn name(&self, id: NodeId) -> &str;
    fn world(&self, id: NodeId) -> &Affine3A;
    fn world_aabb(&self, id: NodeId) -> Aabb;
    /// その部品の描画。範囲が壊れていれば空。
    fn draws(&self, id: NodeId) -> &[Draw];
    /// 形状。読み込んだ分の後に、足した図形の分が続く番号で並ぶ。
    fn primitive(&self, id: PrimId) -> Option<&Primitive>;
    fn primitive_count(&self) -> usize;
    /// GLBの中のノード番号。まだファイルに無い部品（足した図形・複製）は `None`。
    fn file_index(&self, id: NodeId) -> Option<u32>;
    fn find_file_index(&self, gltf_index: u32) -> Option<NodeId>;
    fn bounds(&self) -> Aabb;
    /// `model` の形状を土台にしているか（GPUへ送った形状の組と同じか）。描画が、表示中でない
    /// モデルの部品の表を受け取らないための確かめ。`Model` は自分自身と同じときだけ真。
    fn shares_geometry_with(&self, model: &Model) -> bool;
}

/// `Arc` に包んだまま渡せるようにする（呼び出し側で `&*` を書かずに済む）。
impl<T: SceneRead + ?Sized> SceneRead for std::sync::Arc<T> {
    fn node_count(&self) -> usize {
        (**self).node_count()
    }
    fn parent(&self, id: NodeId) -> Option<NodeId> {
        (**self).parent(id)
    }
    fn subtree_end(&self, id: NodeId) -> u32 {
        (**self).subtree_end(id)
    }
    fn depth(&self, id: NodeId) -> u32 {
        (**self).depth(id)
    }
    fn name(&self, id: NodeId) -> &str {
        (**self).name(id)
    }
    fn world(&self, id: NodeId) -> &Affine3A {
        (**self).world(id)
    }
    fn world_aabb(&self, id: NodeId) -> Aabb {
        (**self).world_aabb(id)
    }
    fn draws(&self, id: NodeId) -> &[Draw] {
        (**self).draws(id)
    }
    fn primitive(&self, id: PrimId) -> Option<&Primitive> {
        (**self).primitive(id)
    }
    fn primitive_count(&self) -> usize {
        (**self).primitive_count()
    }
    fn file_index(&self, id: NodeId) -> Option<u32> {
        (**self).file_index(id)
    }
    fn find_file_index(&self, gltf_index: u32) -> Option<NodeId> {
        (**self).find_file_index(gltf_index)
    }
    fn bounds(&self) -> Aabb {
        (**self).bounds()
    }
    fn shares_geometry_with(&self, model: &Model) -> bool {
        (**self).shares_geometry_with(model)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub nodes: Vec<Node>,
    pub draws: Vec<Draw>,
    pub primitives: Vec<Primitive>,
    pub bounds: Aabb,
    pub stats: ModelStats,
    pub warnings: Vec<LoadWarning>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModelStats {
    pub nodes: u32,
    pub meshes: u32,
    pub primitives: u32,
    pub unique_triangles: u64,
    pub placed_triangles: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadWarning {
    Textures {
        count: u32,
    },
    VertexColors {
        count: u32,
    },
    AlphaBlend {
        count: u32,
    },
    Animations {
        count: u32,
    },
    Skins {
        count: u32,
    },
    Morphs {
        count: u32,
    },
    DerivedNormals {
        count: u32,
    },
    OmittedLinesAndPoints {
        lines: u32,
        points: u32,
    },
    SimplifiedExtension {
        name: String,
    },
    UnsupportedOptionalExtension {
        name: String,
    },
    /// ASCII の STL に `solid` が複数あり、1つの部品にまとめた。`count` はまとめた `solid` の数。
    StlSolidsMerged {
        count: u32,
    },
    /// バイナリの STL の、三角形の後ろに余ったバイト。
    StlTrailingBytes {
        bytes: u64,
    },
}

impl LoadWarning {
    pub fn message_ja(&self) -> String {
        match self {
            Self::Textures { count } => {
                format!("テクスチャ {count} 件は読まず、色は baseColorFactor だけで表示します")
            }
            Self::VertexColors { count } => {
                format!("頂点色 {count} 件は読まず、色は baseColorFactor だけで表示します")
            }
            Self::AlphaBlend { count } => {
                format!("アルファ合成 {count} 件は読まず、色は baseColorFactor だけで表示します")
            }
            Self::Animations { count } => {
                format!("アニメーション {count} 件は再生せず、静止した姿で表示します")
            }
            Self::Skins { count } => {
                format!("スキン {count} 件は再生せず、静止した姿で表示します")
            }
            Self::Morphs { count } => {
                format!("モーフ {count} 件は再生せず、静止した姿で表示します")
            }
            Self::DerivedNormals { count } => {
                format!("法線の無い形状 {count} 件は、面ごとの法線で表示します")
            }
            Self::OmittedLinesAndPoints { lines, points } => {
                format!("線 {lines} 個、点 {points} 個を省略しました")
            }
            Self::SimplifiedExtension { name } => {
                format!("拡張 {name} は見た目だけを簡略化して表示します")
            }
            Self::UnsupportedOptionalExtension { name } => {
                format!("未対応の任意拡張 {name} は無視します")
            }
            Self::StlSolidsMerged { count } => {
                format!("STL の solid {count} 個を、1つの部品にまとめました")
            }
            Self::StlTrailingBytes { bytes } => {
                format!("STL の末尾の余分な {bytes} バイトは読みませんでした")
            }
        }
    }
}

impl Model {
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    /// `id.0 .. subtree_end`。子孫はこの半開区間に連続する。
    pub fn subtree(&self, id: NodeId) -> Range<u32> {
        id.0..self.node(id).subtree_end
    }

    pub fn subtree_bounds(&self, id: NodeId) -> Aabb {
        let range = self.subtree(id);
        self.nodes[range.start as usize..range.end as usize]
            .iter()
            .fold(Aabb::empty(), |bounds, node| bounds.union(node.world_aabb))
    }

    pub fn node_by_gltf_index(&self, gltf_index: u32) -> Option<NodeId> {
        self.nodes
            .iter()
            .position(|node| node.gltf_index == gltf_index)
            .map(|index| NodeId(index as u32))
    }
}

impl SceneRead for Model {
    fn node_count(&self) -> usize {
        self.nodes.len()
    }

    fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).parent
    }

    fn subtree_end(&self, id: NodeId) -> u32 {
        self.node(id).subtree_end
    }

    fn depth(&self, id: NodeId) -> u32 {
        self.node(id).depth
    }

    fn name(&self, id: NodeId) -> &str {
        &self.node(id).name
    }

    fn world(&self, id: NodeId) -> &Affine3A {
        &self.node(id).world
    }

    fn world_aabb(&self, id: NodeId) -> Aabb {
        self.node(id).world_aabb
    }

    fn draws(&self, id: NodeId) -> &[Draw] {
        let range = &self.node(id).draws;
        self.draws
            .get(range.start as usize..range.end as usize)
            .unwrap_or(&[])
    }

    fn primitive(&self, id: PrimId) -> Option<&Primitive> {
        self.primitives.get(id.0 as usize)
    }

    fn primitive_count(&self) -> usize {
        self.primitives.len()
    }

    fn file_index(&self, id: NodeId) -> Option<u32> {
        Some(self.node(id).gltf_index)
    }

    fn find_file_index(&self, gltf_index: u32) -> Option<NodeId> {
        self.node_by_gltf_index(gltf_index)
    }

    fn bounds(&self) -> Aabb {
        self.bounds
    }

    fn shares_geometry_with(&self, model: &Model) -> bool {
        std::ptr::eq(self, model)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use glam::{Affine3A, Vec3};

    use super::{LoadWarning, Model, ModelStats, Node, NodeId, PrimId, Vertex};
    use crate::math::Aabb;

    fn box_at(min: f32, max: f32) -> Aabb {
        Aabb {
            min: Vec3::splat(min),
            max: Vec3::splat(max),
        }
    }

    fn node(name: &str, gltf_index: u32, parent: Option<NodeId>, end: u32, aabb: Aabb) -> Node {
        Node {
            name: name.to_owned(),
            gltf_index,
            parent,
            subtree_end: end,
            depth: u32::from(parent.is_some()),
            local: Affine3A::IDENTITY,
            world: Affine3A::IDENTITY,
            draws: 0..0,
            world_aabb: aabb,
        }
    }

    fn sample() -> Model {
        Model {
            nodes: vec![
                node("root", 0, None, 3, Aabb::empty()),
                node("arm", 4, Some(NodeId(0)), 2, box_at(0.0, 1.0)),
                node("wheel", 7, Some(NodeId(0)), 3, box_at(2.0, 3.0)),
            ],
            draws: Vec::new(),
            primitives: Vec::new(),
            bounds: box_at(0.0, 3.0),
            stats: ModelStats {
                nodes: 3,
                ..ModelStats::default()
            },
            warnings: Vec::new(),
        }
    }

    #[test]
    fn vertex_is_24_bytes_and_roundtrips() {
        assert_eq!(std::mem::size_of::<Vertex>(), 24);
        let vertex = Vertex {
            position: [1.0, -2.0, 3.5],
            normal: [0.0, 1.0, 0.0],
        };
        let bytes: [u8; 24] = bytemuck::cast(vertex);
        let back: Vertex = bytemuck::cast(bytes);
        assert_eq!(back, vertex);
    }

    #[test]
    fn ids_are_copy_eq_hash() {
        let id = NodeId(3);
        let copy = id;
        assert_eq!(id, copy);
        let mut nodes = HashSet::new();
        nodes.insert(id);
        assert!(nodes.contains(&NodeId(3)));
        assert!(!nodes.contains(&NodeId(4)));

        let mut prims = HashSet::new();
        prims.insert(PrimId(1));
        assert!(prims.contains(&PrimId(1)));
    }

    #[test]
    fn subtree_ranges_bounds_and_gltf_lookup() {
        let model = sample();
        assert_eq!(model.subtree(NodeId(0)), 0..3);
        assert_eq!(model.subtree(NodeId(1)), 1..2);
        assert_eq!(model.subtree(NodeId(2)), 2..3);

        let all = model.subtree_bounds(NodeId(0));
        assert_eq!(all.min, Vec3::ZERO);
        assert_eq!(all.max, Vec3::splat(3.0));
        let arm = model.subtree_bounds(NodeId(1));
        assert_eq!(arm.min, Vec3::ZERO);
        assert_eq!(arm.max, Vec3::ONE);

        assert_eq!(model.node_by_gltf_index(7), Some(NodeId(2)));
        assert_eq!(model.node_by_gltf_index(99), None);
        assert_eq!(model.node(NodeId(2)).name, "wheel");
    }

    #[test]
    fn warning_messages_name_the_simplification() {
        let texture = LoadWarning::Textures { count: 2 };
        assert!(texture.message_ja().contains("baseColorFactor"));
        assert!(texture.message_ja().contains('2'));
        assert!(
            LoadWarning::VertexColors { count: 1 }
                .message_ja()
                .contains("baseColorFactor")
        );
        assert!(
            LoadWarning::AlphaBlend { count: 1 }
                .message_ja()
                .contains("baseColorFactor")
        );
        assert!(
            LoadWarning::Animations { count: 1 }
                .message_ja()
                .contains("静止した姿")
        );
        assert!(
            LoadWarning::Skins { count: 1 }
                .message_ja()
                .contains("静止した姿")
        );
        assert!(
            LoadWarning::Morphs { count: 1 }
                .message_ja()
                .contains("静止した姿")
        );
        assert!(
            LoadWarning::DerivedNormals { count: 3 }
                .message_ja()
                .contains("面ごとの法線")
        );
        let omitted = LoadWarning::OmittedLinesAndPoints {
            lines: 4,
            points: 5,
        };
        assert!(omitted.message_ja().contains("省略"));
        assert!(omitted.message_ja().contains('4'));
        assert!(
            LoadWarning::SimplifiedExtension {
                name: "KHR_materials_unlit".to_owned(),
            }
            .message_ja()
            .contains("KHR_materials_unlit")
        );
        assert!(
            LoadWarning::UnsupportedOptionalExtension {
                name: "EXT_x".to_owned(),
            }
            .message_ja()
            .contains("EXT_x")
        );
        assert_eq!(texture, LoadWarning::Textures { count: 2 });
    }
}
