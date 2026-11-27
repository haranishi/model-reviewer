//! 表示・注釈・視点。注釈と視点の番号は一つの連番。

use std::collections::HashSet;
use std::ops::Range;

use glam::Vec3;

use crate::camera::OrbitCamera;
use crate::doc::Remap;
use crate::pick::Hit;
use crate::review_file::MAX_PARSED_ID;
use crate::scene::{NodeId, SceneRead};
use crate::visibility::Visibility;

#[derive(Clone, Debug, PartialEq)]
pub struct Annotation {
    pub id: u64,
    pub node: NodeId,
    pub local_pos: Vec3,
    pub local_normal: Vec3,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SavedView {
    pub id: u64,
    pub name: String,
    pub camera: OrbitCamera,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReviewState {
    pub visibility: Visibility,
    pub annotations: Vec<Annotation>,
    pub views: Vec<SavedView>,
    next_id: u64,
}

impl ReviewState {
    pub fn new(model: &impl SceneRead) -> Self {
        Self {
            visibility: Visibility::all_visible(model.node_count()),
            annotations: Vec::new(),
            views: Vec::new(),
            next_id: 1,
        }
    }

    pub fn add_annotation(&mut self, hit: &Hit, text: String) -> Option<u64> {
        let id = self.allocate_id()?;
        self.annotations.push(Annotation {
            id,
            node: hit.node,
            local_pos: hit.local_pos,
            local_normal: hit.local_normal,
            text,
        });
        Some(id)
    }

    pub fn edit_annotation(&mut self, id: u64, text: String) -> bool {
        let Some(note) = self.annotations.iter_mut().find(|note| note.id == id) else {
            return false;
        };
        note.text = text;
        true
    }

    pub fn remove_annotation(&mut self, id: u64) -> bool {
        let before = self.annotations.len();
        self.annotations.retain(|note| note.id != id);
        self.annotations.len() != before
    }

    pub fn add_view(&mut self, name: String, camera: OrbitCamera) -> Option<u64> {
        let id = self.allocate_id()?;
        self.views.push(SavedView { id, name, camera });
        Some(id)
    }

    pub fn remove_view(&mut self, id: u64) -> bool {
        let before = self.views.len();
        self.views.retain(|view| view.id != id);
        self.views.len() != before
    }

    /// 部品の表の番号のずれに合わせて、注釈の `node` を付け替える。消えた部品の注釈は落とす
    /// （落とす前に `take_annotations_in` で取り出しておく）。表示状態は `Visibility::splice/cut` が扱う。
    pub fn apply_remap(&mut self, remap: &Remap) {
        self.annotations
            .retain_mut(|note| match remap.map(note.node) {
                Some(node) => {
                    note.node = node;
                    true
                }
                None => false,
            });
    }

    /// `range` の部品に付いた注釈を、並びの位置とともに取り出す（削除を1回分にまとめるため）。
    /// 位置は取り出す前の並びでの位置で、小さい順。その順に `insert_annotation_at` で戻すと元の並びになる。
    pub fn take_annotations_in(&mut self, range: Range<u32>) -> Vec<(usize, Annotation)> {
        if !self
            .annotations
            .iter()
            .any(|note| range.contains(&note.node.0))
        {
            return Vec::new();
        }
        let mut taken = Vec::new();
        let mut kept = Vec::with_capacity(self.annotations.len());
        for (index, note) in std::mem::take(&mut self.annotations)
            .into_iter()
            .enumerate()
        {
            if range.contains(&note.node.0) {
                taken.push((index, note));
            } else {
                kept.push(note);
            }
        }
        self.annotations = kept;
        taken
    }

    /// 注釈を、番号を保ったまま並びの `index` へ戻す（取り消し用）。番号が使用中なら `false`。
    /// `index` が長さを超えれば末尾に足す。次に振る番号は、戻した番号より後ろにする。
    pub fn insert_annotation_at(&mut self, index: usize, note: Annotation) -> bool {
        if self.id_used(note.id) {
            return false;
        }
        self.reserve_id(note.id);
        let index = index.min(self.annotations.len());
        self.annotations.insert(index, note);
        true
    }

    /// 視点を、番号を保ったまま並びの `index` へ戻す（取り消し用）。番号が使用中なら `false`。
    pub fn insert_view_at(&mut self, index: usize, view: SavedView) -> bool {
        if self.id_used(view.id) {
            return false;
        }
        self.reserve_id(view.id);
        let index = index.min(self.views.len());
        self.views.insert(index, view);
        true
    }

    /// 最後に振った番号 `id` を、振る前に戻す（追加の取り消しで、状態を元とそろえるため）。
    /// 次に振る番号が `id` の直後のときだけ戻す。
    pub(crate) fn release_id(&mut self, id: u64) {
        if self.next_id == id.wrapping_add(1) && !self.id_used(id) {
            self.next_id = id;
        }
    }

    fn reserve_id(&mut self, id: u64) {
        if let Some(next) = id.checked_add(1)
            && next > self.next_id
        {
            self.next_id = next;
        }
    }

    pub(crate) fn from_parts(
        visibility: Visibility,
        annotations: Vec<Annotation>,
        views: Vec<SavedView>,
    ) -> Self {
        let max_id = annotations
            .iter()
            .map(|note| note.id)
            .chain(views.iter().map(|view| view.id))
            .max()
            .unwrap_or(0);
        Self {
            visibility,
            annotations,
            views,
            next_id: max_id.saturating_add(1).max(1),
        }
    }

    /// 保存して読み直せる範囲で採番する。上限に達したら1から空きを探す。
    fn allocate_id(&mut self) -> Option<u64> {
        let end = MAX_PARSED_ID + 1;
        let start = self.next_id.min(end);
        let id = self.unused_id_in((start..end).chain(1..start))?;
        self.next_id = id + 1;
        Some(id)
    }

    fn unused_id_in(&self, mut candidates: impl Iterator<Item = u64>) -> Option<u64> {
        let first = candidates.next()?;
        if !self.id_used(first) {
            return Some(first);
        }
        let used: HashSet<_> = self.ids().collect();
        candidates.find(|id| !used.contains(id))
    }

    fn id_used(&self, id: u64) -> bool {
        self.ids().any(|used| used == id)
    }

    fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.annotations
            .iter()
            .map(|note| note.id)
            .chain(self.views.iter().map(|view| view.id))
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use crate::pick::Hit;
    use crate::review_file::MAX_PARSED_ID;
    use crate::scene::NodeId;
    use crate::visibility::Visibility;

    use super::{Annotation, ReviewState};

    fn hit() -> Hit {
        Hit {
            node: NodeId(0),
            t: 1.0,
            world_pos: Vec3::ZERO,
            local_pos: Vec3::ZERO,
            local_normal: Vec3::Y,
        }
    }

    #[test]
    fn ids_near_the_end_are_not_reused() {
        let mut past_limit = ReviewState::from_parts(
            Visibility::all_visible(1),
            vec![Annotation {
                id: u64::MAX,
                node: NodeId(0),
                local_pos: Vec3::ZERO,
                local_normal: Vec3::Y,
                text: "範囲外".to_owned(),
            }],
            Vec::new(),
        );
        assert_eq!(past_limit.add_annotation(&hit(), "次".to_owned()), Some(1));
        assert_eq!(past_limit.annotations.len(), 2);
        assert_eq!(past_limit.unused_id_in(1..=1), None);

        let mut near = ReviewState::from_parts(
            Visibility::all_visible(1),
            vec![Annotation {
                id: MAX_PARSED_ID - 2,
                node: NodeId(0),
                local_pos: Vec3::ZERO,
                local_normal: Vec3::Y,
                text: "手前".to_owned(),
            }],
            Vec::new(),
        );
        let first = near.add_annotation(&hit(), "a".to_owned()).unwrap();
        let second = near.add_annotation(&hit(), "b".to_owned()).unwrap();
        assert_eq!(first, MAX_PARSED_ID - 1);
        assert_eq!(second, MAX_PARSED_ID);
        assert_eq!(near.unused_id_in(MAX_PARSED_ID - 2..=MAX_PARSED_ID), None);
        assert_eq!(near.add_annotation(&hit(), "c".to_owned()), Some(1));
        let mut ids: Vec<u64> = near.annotations.iter().map(|note| note.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), near.annotations.len());
    }
}
