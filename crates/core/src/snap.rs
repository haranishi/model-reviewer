//! 計測の点の吸着。ポインタの視線が当たった三角形の近くだけから、頂点・辺・面の候補を探す。
//! 形状ごとの頂点の表や辺の表は作らない（常駐のメモリを増やさない）。

use glam::{Vec2, Vec3};

use crate::camera::OrbitCamera;
use crate::pick::PickIndex;
use crate::scene::{NodeId, SceneRead};
use crate::section::SectionPlane;
use crate::visibility::Visibility;

/// 何に吸着したか。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SnapKind {
    Vertex,
    Edge,
    #[default]
    Face,
}

/// 吸着の設定。`radius_px` は物理画素（12ポイント × `pixels_per_point`）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapSettings {
    pub enabled: bool,
    pub radius_px: f32,
}

impl Default for SnapSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            radius_px: 12.0,
        }
    }
}

/// 1回の探索の入力。
#[derive(Clone, Copy, Debug)]
pub struct SnapQuery<'a> {
    pub camera: &'a OrbitCamera,
    pub aspect: f32,
    pub viewport_px: [f32; 2],
    pub ndc: Vec2,
    pub settings: SnapSettings,
    pub clip: Option<SectionPlane>,
}

/// 吸着した先。`local` は部品の中の座標、`world` はワールドの座標。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapTarget {
    pub node: NodeId,
    pub kind: SnapKind,
    pub local: Vec3,
    pub world: Vec3,
    /// 辺のときの両端（ワールド。強調に使う）。
    pub edge: Option<[Vec3; 2]>,
}

/// 辺とみなす、隣の面との角度（度）。
pub const FEATURE_ANGLE_DEG: f32 = 10.0;
/// 1回の探索で見る三角形の上限。超えたら当たった三角形だけを見る。
pub const MAX_NEAR_TRIANGLES: usize = 512;
/// 辺の候補にする距離の、吸着の半径に対する割合。
pub const EDGE_RADIUS_RATIO: f32 = 0.75;

/// ポインタの位置で吸着の先を探す。何にも当たらなければ `None`。
pub fn snap(
    scene: &impl SceneRead,
    index: &PickIndex,
    vis: &Visibility,
    query: &SnapQuery,
) -> Option<SnapTarget> {
    // 契約の先置き。中身は後の作業で書く。
    let _ = (scene, index, vis, query);
    None
}
