//! 2点の距離。点は `(NodeKey, 部品の中の座標)` で持つので、部品を動かすと距離が変わり、
//! 部品を削除すると計測が消える。3回目の点は新しい計測の1点目になる。保存せず、履歴にも入れない。
//!
//! 文書の読み方は「鍵 → いまのワールド行列」の1つだけなので、計算はその関数を受け取る形でも
//! 置いてある（`*_with`）。文書以外の表（試験の表など）でも同じ計算が使える。

use glam::{Affine3A, Vec3};

use crate::doc::{Document, NodeKey};
use crate::scene::SceneRead;
use crate::snap::SnapKind;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasurePoint {
    pub node: NodeKey,
    pub local: Vec3,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Measurement {
    pub first: Option<MeasurePoint>,
    pub second: Option<MeasurePoint>,
    /// 1点目と2点目が何に吸着したか。点が無い側は `Face`。
    pub kinds: [SnapKind; 2],
}

impl Measurement {
    /// 点を足す（面に置いた点として）。`push_with(point, SnapKind::Face)` と同じ。
    pub fn push(&mut self, point: MeasurePoint) {
        self.push_with(point, SnapKind::Face);
    }

    /// 点を、何に吸着したかと一緒に足す。2点そろっていれば、新しい計測の1点目にする。
    /// 有限でない座標は受け付けない。
    pub fn push_with(&mut self, point: MeasurePoint, kind: SnapKind) {
        if !point.local.is_finite() {
            return;
        }
        match (self.first, self.second) {
            (Some(_), None) => {
                self.second = Some(point);
                self.kinds[1] = kind;
            }
            _ => {
                self.first = Some(point);
                self.second = None;
                self.kinds = [kind, SnapKind::Face];
            }
        }
    }

    /// 2点目 − 1点目（ワールド）。2点がそろっていなければ `None`。
    pub fn delta(&self, doc: &Document) -> Option<Vec3> {
        self.delta_with(|key| world_of(doc, key))
    }

    pub fn delta_with(&self, world: impl Fn(NodeKey) -> Option<Affine3A>) -> Option<Vec3> {
        let (first, second) = (self.first?, self.second?);
        let a = world(first.node)?.transform_point3(first.local);
        let b = world(second.node)?.transform_point3(second.local);
        let delta = b - a;
        delta.is_finite().then_some(delta)
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// 点のワールド座標（そろっている分だけ）。部品が文書に無ければ、その点は無い。
    pub fn world_points(&self, doc: &Document) -> Vec<Vec3> {
        self.world_points_with(|key| world_of(doc, key))
    }

    /// 2点がそろっていれば距離。
    pub fn distance(&self, doc: &Document) -> Option<f32> {
        self.distance_with(|key| world_of(doc, key))
    }

    /// 文書に無くなった部品の点があれば計測を消す。消したら `true`。
    pub fn prune(&mut self, doc: &Document) -> bool {
        self.prune_with(|key| doc.id_of(key).is_some())
    }

    /// `world` は鍵からいまのワールド行列を返す（部品が無ければ `None`）。
    pub fn world_points_with(&self, world: impl Fn(NodeKey) -> Option<Affine3A>) -> Vec<Vec3> {
        [self.first, self.second]
            .into_iter()
            .flatten()
            .filter_map(|point| {
                let position = world(point.node)?.transform_point3(point.local);
                position.is_finite().then_some(position)
            })
            .collect()
    }

    pub fn distance_with(&self, world: impl Fn(NodeKey) -> Option<Affine3A>) -> Option<f32> {
        let (first, second) = (self.first?, self.second?);
        let a = world(first.node)?.transform_point3(first.local);
        let b = world(second.node)?.transform_point3(second.local);
        let distance = a.distance(b);
        distance.is_finite().then_some(distance)
    }

    /// `exists` は鍵の部品がいまの文書にあるか。
    pub fn prune_with(&mut self, exists: impl Fn(NodeKey) -> bool) -> bool {
        let gone = [self.first, self.second]
            .into_iter()
            .flatten()
            .any(|point| !exists(point.node));
        if gone {
            self.clear();
        }
        gone
    }
}

fn world_of(doc: &Document, key: NodeKey) -> Option<Affine3A> {
    let id = doc.id_of(key)?;
    if id.0 as usize >= doc.node_count() {
        return None;
    }
    Some(*doc.world(id))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::units::{UnitLabel, format_distance};

    fn point(node: u64, local: Vec3) -> MeasurePoint {
        MeasurePoint {
            node: NodeKey(node),
            local,
        }
    }

    #[test]
    fn two_points_give_a_distance_and_a_third_starts_over() {
        let table: HashMap<NodeKey, Affine3A> = HashMap::from([
            (
                NodeKey(1),
                Affine3A::from_translation(Vec3::new(1.0, 0.0, 0.0)),
            ),
            (
                NodeKey(2),
                Affine3A::from_translation(Vec3::new(0.0, 0.0, 4.0)),
            ),
        ]);
        let world = |key: NodeKey| table.get(&key).copied();
        let mut measure = Measurement::default();
        assert_eq!(measure.distance_with(world), None);
        measure.push(point(1, Vec3::new(2.0, 0.0, 0.0)));
        assert_eq!(
            measure.world_points_with(world),
            vec![Vec3::new(3.0, 0.0, 0.0)]
        );
        assert_eq!(measure.distance_with(world), None);
        measure.push(point(2, Vec3::new(3.0, 0.0, 0.0)));
        // (3,0,0) と (3,0,4)。
        assert_eq!(measure.distance_with(world), Some(4.0));
        assert_eq!(measure.world_points_with(world).len(), 2);

        measure.push(point(2, Vec3::ZERO));
        assert_eq!(measure.first, Some(point(2, Vec3::ZERO)));
        assert_eq!(measure.second, None);
        measure.push(point(1, Vec3::new(f32::NAN, 0.0, 0.0)));
        assert_eq!(measure.second, None);
        measure.clear();
        assert_eq!(measure, Measurement::default());
    }

    #[test]
    fn moving_a_part_changes_the_distance_and_units_do_not() {
        let mut table: HashMap<NodeKey, Affine3A> = HashMap::from([
            (NodeKey(1), Affine3A::IDENTITY),
            (NodeKey(2), Affine3A::from_translation(Vec3::X)),
        ]);
        let mut measure = Measurement::default();
        measure.push(point(1, Vec3::ZERO));
        measure.push(point(2, Vec3::ZERO));
        let before = measure.distance_with(|key| table.get(&key).copied());
        assert_eq!(before, Some(1.0));
        table.insert(
            NodeKey(2),
            Affine3A::from_scale_rotation_translation(
                Vec3::splat(2.0),
                glam::Quat::from_rotation_y(0.7),
                Vec3::new(0.0, 2.5, 0.0),
            ),
        );
        let after = measure
            .distance_with(|key| table.get(&key).copied())
            .unwrap();
        assert!((after - 2.5).abs() < 1.0e-6, "{after}");
        let texts: Vec<String> = [UnitLabel::M, UnitLabel::Cm, UnitLabel::Mm]
            .into_iter()
            .map(|unit| format_distance(after, unit))
            .collect();
        assert_eq!(texts, ["2.500 m", "2.500 cm", "2.500 mm"]);
    }

    #[test]
    fn push_with_keeps_the_kinds_and_a_third_point_starts_over() {
        let mut measure = Measurement::default();
        assert_eq!(measure.kinds, [SnapKind::Face; 2]);
        measure.push_with(point(1, Vec3::ZERO), SnapKind::Vertex);
        assert_eq!(measure.kinds, [SnapKind::Vertex, SnapKind::Face]);
        measure.push_with(point(1, Vec3::X), SnapKind::Edge);
        assert_eq!(measure.kinds, [SnapKind::Vertex, SnapKind::Edge]);
        // 有限でない点は受け付けず、種類も変えない。
        measure.push_with(point(1, Vec3::splat(f32::INFINITY)), SnapKind::Vertex);
        assert_eq!(measure.kinds, [SnapKind::Vertex, SnapKind::Edge]);
        assert_eq!(measure.second, Some(point(1, Vec3::X)));
        // 3点目は新しい1点目。2点目の種類は面に戻る。
        measure.push_with(point(2, Vec3::Y), SnapKind::Edge);
        assert_eq!(measure.kinds, [SnapKind::Edge, SnapKind::Face]);
        assert_eq!(measure.second, None);
        // `push` は面として足す。
        measure.push(point(2, Vec3::Z));
        assert_eq!(measure.kinds, [SnapKind::Edge, SnapKind::Face]);
        measure.push(point(2, Vec3::ZERO));
        assert_eq!(measure.kinds, [SnapKind::Face, SnapKind::Face]);
    }

    #[test]
    fn delta_is_second_minus_first_and_follows_moved_parts() {
        let mut table: HashMap<NodeKey, Affine3A> = HashMap::from([
            (NodeKey(1), Affine3A::IDENTITY),
            (
                NodeKey(2),
                Affine3A::from_translation(Vec3::new(1.0, -2.0, 0.5)),
            ),
        ]);
        let mut measure = Measurement::default();
        measure.push(point(1, Vec3::new(0.5, 0.5, 0.5)));
        assert_eq!(measure.delta_with(|key| table.get(&key).copied()), None);
        measure.push(point(2, Vec3::ZERO));
        let delta = measure.delta_with(|key| table.get(&key).copied()).unwrap();
        // (1, −2, 0.5) − (0.5, 0.5, 0.5)。
        assert!(
            (delta - Vec3::new(0.5, -2.5, 0.0)).length() < 1.0e-6,
            "{delta}"
        );
        // 順を逆にすると符号が逆になる。
        let mut reversed = Measurement::default();
        reversed.push(point(2, Vec3::ZERO));
        reversed.push(point(1, Vec3::new(0.5, 0.5, 0.5)));
        let back = reversed.delta_with(|key| table.get(&key).copied()).unwrap();
        assert!((back + delta).length() < 1.0e-6);
        // 部品を動かすと差が変わる。
        table.insert(
            NodeKey(2),
            Affine3A::from_translation(Vec3::new(4.0, 0.5, 0.5)),
        );
        let moved = measure.delta_with(|key| table.get(&key).copied()).unwrap();
        assert!(
            (moved - Vec3::new(3.5, 0.0, 0.0)).length() < 1.0e-6,
            "{moved}"
        );
        // 部品が無ければ差も無い。
        assert_eq!(
            measure.delta_with(|key| (key == NodeKey(1)).then_some(Affine3A::IDENTITY)),
            None
        );
    }

    #[test]
    fn removed_parts_clear_the_measurement() {
        let mut measure = Measurement::default();
        measure.push(point(1, Vec3::ZERO));
        measure.push(point(2, Vec3::ONE));
        assert!(!measure.prune_with(|_| true));
        assert!(measure.second.is_some());
        assert_eq!(
            measure.distance_with(|key| (key != NodeKey(2)).then_some(Affine3A::IDENTITY)),
            None
        );
        assert!(measure.prune_with(|key| key != NodeKey(2)));
        assert_eq!(measure, Measurement::default());
        assert!(!measure.prune_with(|_| false));
    }
}
