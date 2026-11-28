//! 選んだ部品と子孫の基本色。表示用の丸めは文書へ書き戻さない。

use std::collections::HashSet;
use std::time::Instant;

use model_reviewer_core::color::linear_to_srgb8;
use model_reviewer_core::doc::Document;
use model_reviewer_core::history::{Edit, Step};
use model_reviewer_core::scene::{LoadWarning, Model, NodeId, SceneRead};
use model_reviewer_render::DisplayMode;

use super::{Session, edit};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorState {
    NoFaces,
    One([u8; 3]),
    Mixed { count: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorView {
    pub state: ColorState,
    pub note: Option<&'static str>,
    pub locked: bool,
    pub selection_rev: (u64, u64),
}

pub(crate) fn draw_colors(doc: &Document, id: NodeId) -> impl Iterator<Item = [f32; 4]> {
    doc.subtree(id)
        .flat_map(|index| doc.draws(NodeId(index)))
        .map(|draw| draw.base_color)
}

fn color_state(doc: &Document, id: NodeId) -> ColorState {
    let mut colors = draw_colors(doc, id).map(|[r, g, b, _]| [r, g, b].map(f32::to_bits));
    let Some(first) = colors.next() else {
        return ColorState::NoFaces;
    };
    let mut others = HashSet::new();
    for rgb in colors {
        if rgb != first {
            others.insert(rgb);
            if others.len() == 98 {
                break;
            }
        }
    }
    if others.is_empty() {
        ColorState::One(linear_to_srgb8(first.map(f32::from_bits)))
    } else {
        ColorState::Mixed {
            count: 1 + others.len() as u32,
        }
    }
}

pub(super) fn model_note(model: &Model) -> Option<&'static str> {
    color_note(model, !model.textures.images.is_empty())
}

fn color_note(model: &Model, textures: bool) -> Option<&'static str> {
    let vertices = model
        .warnings
        .iter()
        .any(|w| matches!(w, LoadWarning::VertexColors { .. }));
    match (textures, vertices) {
        (true, true) => {
            Some("色は画像に掛け合わされて見えます。ほかのソフトでは頂点の色も掛け合わされます。")
        }
        (true, false) => Some("色は画像に掛け合わされて見えます"),
        (false, true) => Some(
            "このモデルには頂点の色があります。この画面には出ていません。ほかのソフトでは、選んだ色が頂点の色に掛け合わされて見えます。",
        ),
        (false, false) => None,
    }
}

pub fn selection_color(session: &Session) -> Option<ColorView> {
    let doc = session.doc.as_ref()?;
    let id = session.selection?;
    let selection_rev = (doc.key(id)?.0, session.history.doc_rev());
    let locked = session.edits_locked();
    if let Some(view) = session
        .color_cache
        .get()
        .filter(|v| v.selection_rev == selection_rev)
    {
        return Some(ColorView { locked, ..view });
    }
    let textured = session.textures_shown
        && matches!(
            session.display,
            DisplayMode::Faces | DisplayMode::FacesAndLines
        )
        && session.textures.displayed() > 0
        && doc.subtree(id).map(NodeId).any(|node| {
            session
                .review
                .as_ref()
                .is_none_or(|r| r.visibility.effective(node))
                && doc
                    .draws(node)
                    .iter()
                    .filter_map(|draw| doc.tex_use(draw.tex?)?.image)
                    .any(|image| session.textures.is_resident(image))
        });
    let view = ColorView {
        state: color_state(doc, id),
        note: color_note(doc.base(), textured),
        locked,
        selection_rev,
    };
    session.color_cache.set(Some(view));
    Some(view)
}

pub fn set_color(session: &mut Session, rgb: [f32; 3]) {
    if session.block_edit_if_locked() {
        return;
    }
    edit::cancel_transform(session);
    let Some(doc) = session.doc.as_ref() else {
        return;
    };
    let Some(key) = session.selection.and_then(|id| doc.key(id)) else {
        return;
    };
    if !rgb.iter().all(|value| (0.0..=1.0).contains(value)) {
        session.status_text = "色の値が範囲外です".to_owned();
        return;
    }
    let started = Instant::now();
    let Ok(before) = doc.subtree_colors(key) else {
        return;
    };
    let bits = rgb.map(f32::to_bits);
    if before
        .iter()
        .all(|&[r, g, b, _]| [r, g, b].map(f32::to_bits) == bits)
    {
        return;
    }
    let step = Step::new(
        "色の変更",
        vec![Edit::SetColors { key, before, rgb }],
        Some(key),
    );
    if edit::execute(session, step).is_some() {
        session.metrics.edit_commit = Some(started.elapsed());
        // 履歴の上限を超えたときは execute が出した理由を残す。
        if session.history.can_undo() {
            session.status_text = "色の変更を確定しました".to_owned();
        }
    }
}
