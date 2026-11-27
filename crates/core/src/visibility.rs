//! 部品の表示。親を隠すと子孫の実効表示だけが落ち、子自身の設定は残る。

use std::ops::Range;

use crate::scene::{NodeId, SceneRead};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Visibility {
    own: Vec<bool>,
    effective: Vec<bool>,
}

impl Visibility {
    pub fn all_visible(node_count: usize) -> Self {
        Self {
            own: vec![true; node_count],
            effective: vec![true; node_count],
        }
    }

    pub fn set(&mut self, model: &impl SceneRead, id: NodeId, visible: bool) {
        let index = id.0 as usize;
        if index >= self.own.len() || index >= model.node_count() {
            return;
        }
        self.own[index] = visible;
        self.recompute(model);
    }

    pub fn show_all(&mut self) {
        self.own.fill(true);
        self.effective.fill(true);
    }

    pub fn own(&self, id: NodeId) -> bool {
        self.own.get(id.0 as usize).copied().unwrap_or(false)
    }

    pub fn effective(&self, id: NodeId) -> bool {
        self.effective.get(id.0 as usize).copied().unwrap_or(false)
    }

    pub fn hidden_own(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.own.iter().enumerate().filter_map(|(index, visible)| {
            if *visible {
                None
            } else {
                Some(NodeId(index as u32))
            }
        })
    }

    /// 部品の表の `at` に連続範囲が挿入された後で、同じ位置へ各部品の自身の設定 `own` を継ぎ足し、
    /// 実効表示を `scene`（挿入後の表）から計算し直す。計算し直すのは継ぎ足した範囲だけ
    /// （範囲の外の実効表示は、範囲の中の部品に左右されない）。`at` が長さを超えれば末尾に足す。
    pub fn splice(&mut self, scene: &impl SceneRead, at: NodeId, own: &[bool]) {
        let start = (at.0 as usize).min(self.own.len());
        self.own.splice(start..start, own.iter().copied());
        self.effective
            .splice(start..start, std::iter::repeat_n(true, own.len()));
        let end = (start + own.len()).min(scene.node_count());
        for index in start..end {
            let parent_visible = match scene.parent(NodeId(index as u32)) {
                Some(parent) if (parent.0 as usize) < index => self
                    .effective
                    .get(parent.0 as usize)
                    .copied()
                    .unwrap_or(true),
                _ => true,
            };
            self.effective[index] = self.own[index] && parent_visible;
        }
    }

    /// 部品の表から取り除く連続範囲の、自身の設定を切り取って返す（取り消しで `splice` に戻す）。
    /// 範囲がまるごと部分木なので、残りの実効表示は変わらない。長さを超える分は無視する。
    pub fn cut(&mut self, range: Range<u32>) -> Vec<bool> {
        let len = self.own.len();
        let start = (range.start as usize).min(len);
        let end = (range.end as usize).clamp(start, len);
        let end_effective = end.min(self.effective.len());
        let start_effective = start.min(end_effective);
        self.effective.drain(start_effective..end_effective);
        self.own.drain(start..end).collect()
    }

    /// 複数の部品の自身の設定をまとめて変え、実効表示を1回だけ計算し直す（取り消し用）。
    /// 範囲外の番号は無視する。
    pub fn set_many(
        &mut self,
        model: &impl SceneRead,
        ids: impl IntoIterator<Item = NodeId>,
        visible: bool,
    ) {
        let limit = self.own.len().min(model.node_count());
        for id in ids {
            if (id.0 as usize) < limit {
                self.own[id.0 as usize] = visible;
            }
        }
        self.recompute(model);
    }

    /// 部品の数。
    pub fn len(&self) -> usize {
        self.own.len()
    }

    pub fn is_empty(&self) -> bool {
        self.own.is_empty()
    }

    fn recompute(&mut self, model: &impl SceneRead) {
        let count = self
            .own
            .len()
            .min(self.effective.len())
            .min(model.node_count());
        for index in 0..count {
            let parent_visible = match model.parent(NodeId(index as u32)) {
                Some(parent) if (parent.0 as usize) < index => self.effective[parent.0 as usize],
                _ => true,
            };
            self.effective[index] = self.own[index] && parent_visible;
        }
    }
}
