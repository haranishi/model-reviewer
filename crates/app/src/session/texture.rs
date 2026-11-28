//! 貼る部品は受付時に決め、復号が終わってから文書と履歴を一緒に更新する。

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Instant;

use model_reviewer_core::doc::{DocError, Document, DrawLook, NodeKey};
use model_reviewer_core::history::{Edit, Step};
use model_reviewer_core::identity::ModelIdentity;
use model_reviewer_core::scene::{Draw, NodeId, SceneRead};
use model_reviewer_core::texture::{
    IDENTITY_TRANSFORM, ImageHash, ImageId, Mime, MipChain, Sampling, TexUse, UvMap, plan_levels,
    sniff,
};
use model_reviewer_core::units::Units;
use model_reviewer_texture::{TexError, decode, dimensions, strip_metadata};

use super::{Session, edit};
use crate::loader::{TextureOptions, TextureReady};

const SELECT_PART: &str = "貼る部品を選んでから、画像を貼ってください";
const NO_FACES: &str = "この部品には、貼る面がありません";
const BUSY: &str = "読み込み中・書き出し中は貼れません";
const NO_BOX: &str = "スキンかモーフのある部品には、この貼り方では貼れません";
const CHANGED: &str = "貼る間に文書が変わったため、貼りませんでした";
const FILE_LIMIT: u64 = 64 << 20;

pub struct TextureView {
    pub key: NodeKey,
    pub doc_rev: u64,
    pub units: Units,
    pub state: String,
    pub has_image: bool,
    pub has_box: bool,
    pub world_size: Option<f32>,
    pub paste_block: Option<&'static str>,
    pub locked: bool,
}

fn scale(doc: &Document, node: NodeId) -> f32 {
    doc.world(node).matrix3.determinant().abs().cbrt()
}

fn vertex_mapping(doc: &Document, draw: &Draw) -> Option<TexUse> {
    let tex = *doc.tex_use(draw.tex?)?;
    let UvMap::Vertex { info_from, .. } = tex.map else {
        return None;
    };
    if doc.prim_uvs(draw.prim).is_empty() {
        return None;
    }
    if tex.image.is_some() {
        return Some(tex);
    }
    let from = info_from?;
    // 外した MASK は閾値だけで書き出す。再び貼る場合は読み込んだ材質の UV とサンプラを使う。
    doc.base().textures.uses.iter().copied().find(|old| {
        matches!(old.map, UvMap::Vertex { info_from: Some(i), .. } if i == from)
            && old.image.is_some()
    })
}

pub(super) fn forbids_box(doc: &Document, node: NodeId, extra: &HashSet<u32>) -> bool {
    doc.file_index(node)
        .or(doc.nodes()[node.0 as usize].template)
        .is_some_and(|file| {
            doc.base()
                .textures
                .no_box_nodes
                .binary_search(&file)
                .is_ok()
                || extra.contains(&file)
        })
}

/// 同じ部品を映している間は子孫を走査し直さない。転送の進み具合は表示のたびに読む。
#[derive(Clone, Copy)]
pub(super) struct Selection {
    key: NodeKey,
    revision: u64,
    image: Option<ImageId>,
    count: usize,
    has_uv: bool,
    has_box: bool,
    world_size: Option<f32>,
    blocked: Option<&'static str>,
}

fn selection(session: &Session) -> Option<Selection> {
    let doc = session.doc.as_ref()?;
    let id = session.selection?;
    let key = doc.key(id)?;
    let revision = session.history.doc_rev();
    if let Some(cached) = session.texture_cache.get()
        && cached.key == key
        && cached.revision == revision
    {
        return Some(cached);
    }
    let mut images = HashSet::new();
    let mut has_uv = false;
    let mut has_box = false;
    let mut first_size = None;
    let mut mixed_size = false;
    let mut has_faces = false;
    let mut forbidden = false;
    for node in doc.subtree(id).map(NodeId) {
        let draws = doc.draws(node);
        if draws.is_empty() {
            continue;
        }
        let k = scale(doc, node);
        if k.is_finite() && k > 0.0 {
            has_faces = true;
            forbidden |= forbids_box(doc, node, &session.exported_no_box)
                && draws.iter().any(|draw| vertex_mapping(doc, draw).is_none());
        }
        for tex in draws.iter().filter_map(|draw| doc.tex_use(draw.tex?)) {
            let Some(image) = tex.image else { continue };
            images.insert(image);
            match tex.map {
                UvMap::Vertex { .. } => has_uv = true,
                UvMap::Box { size, .. } => {
                    has_box = true;
                    let world = size * k;
                    let first = *first_size.get_or_insert(world);
                    mixed_size |=
                        (first - world).abs() > 4.0 * f32::EPSILON * first.abs().max(world.abs());
                }
            }
        }
    }
    let selected = Selection {
        key,
        revision,
        image: images.iter().next().copied(),
        count: images.len(),
        has_uv,
        has_box,
        world_size: first_size.filter(|_| !mixed_size),
        blocked: if forbidden {
            Some(NO_BOX)
        } else if !has_faces {
            Some(NO_FACES)
        } else {
            None
        },
    };
    session.texture_cache.set(Some(selected));
    Some(selected)
}

pub(crate) fn paste_block(session: &Session) -> Option<&'static str> {
    let Some(selected) = selection(session) else {
        return Some(SELECT_PART);
    };
    if session.is_loading() || session.is_exporting() || session.textures_busy() {
        Some(BUSY)
    } else if !session.texture_options.enabled {
        Some("画像の読み込みが無効です")
    } else {
        selected.blocked
    }
}

pub fn texture_view(session: &Session) -> Option<TextureView> {
    let selected = selection(session)?;
    let state =
        if session.paste_job.as_ref().is_some_and(|job| {
            job.key == selected.key && job.generation == session.shown_generation
        }) {
            "読み込み中…".to_owned()
        } else if selected.count > 1 {
            format!("複数の画像（{}枚）", selected.count)
        } else if let Some(image) = selected.image {
            let mapping = match (selected.has_uv, selected.has_box) {
                (true, true) => "モデルの UV と箱形の投影",
                (_, true) => "箱形の投影",
                _ => "モデルの UV",
            };
            if let Some([w, h]) = session.textures.display_size(image) {
                let shrunk = if session
                    .textures
                    .planned_shrinks
                    .binary_search(&image)
                    .is_ok()
                {
                    "、縮小して表示"
                } else {
                    ""
                };
                format!("{w} × {h}（{mapping}{shrunk}）")
            } else if session.textures_busy() {
                "読み込み中…".to_owned()
            } else {
                format!("画像は表示されていません（{mapping}）")
            }
        } else {
            "画像はありません".to_owned()
        };
    Some(TextureView {
        key: selected.key,
        doc_rev: selected.revision,
        units: session.units,
        state,
        has_image: selected.count > 0,
        has_box: selected.has_box,
        world_size: selected.world_size,
        paste_block: paste_block(session),
        locked: session.edits_locked(),
    })
}

struct PreparedImage {
    canonical: PathBuf,
    original: ModelIdentity,
    bytes: Arc<[u8]>,
    mime: Mime,
    size: [u32; 2],
    chain: Option<MipChain>,
    shrunk: bool,
}

pub(super) struct PasteJob {
    key: NodeKey,
    generation: u64,
    revision: u64,
    pub(super) stale: bool,
    path: PathBuf,
    result: mpsc::Receiver<Result<PreparedImage, String>>,
}

fn read_image(
    path: &Path,
    options: TextureOptions,
    available: u64,
    resident: HashSet<ImageHash>,
) -> Result<PreparedImage, String> {
    let io_message =
        |err: io::Error| TexError::Broken(crate::loader::io_error_ja(&err)).message_ja();
    let canonical = fs::canonicalize(path).map_err(io_message)?;
    let file = File::open(&canonical).map_err(io_message)?;
    let length = file.metadata().map_err(io_message)?.len();
    let too_large = "画像が大きすぎます（64 MiB まで）";
    if length > FILE_LIMIT {
        return Err(too_large.into());
    }
    let mut raw = Vec::with_capacity(length as usize);
    file.take(FILE_LIMIT + 1)
        .read_to_end(&mut raw)
        .map_err(io_message)?;
    if raw.len() as u64 > FILE_LIMIT {
        return Err(too_large.into());
    }
    let original = ModelIdentity::from_bytes(&raw);
    let mime = sniff(&raw).ok_or_else(|| TexError::Unsupported.message_ja())?;
    let bytes = strip_metadata(&raw, mime).map_err(|e| e.message_ja())?;
    drop(raw);
    let size = dimensions(&bytes).map_err(|e| e.message_ja())?;
    // 同じ画像が表示中なら追加の GPU 容量は要らない。付帯情報を落とした結果は毎回確かめる。
    let reuse = !resident.is_empty() && resident.contains(&ImageHash::of(&bytes));
    let skip = if reuse {
        u8::MAX
    } else {
        plan_levels(&[size], options.max_side, available)[0].ok_or_else(|| {
            format!(
                "画像のメモリの上限（{} MiB）に空きがありません",
                options.budget_mib
            )
        })?
    };
    let chain = decode(&bytes, skip).map_err(|e| e.message_ja())?;
    Ok(PreparedImage {
        canonical,
        original,
        bytes: bytes.into(),
        mime,
        size,
        chain: (!reuse).then_some(chain),
        shrunk: skip > 0,
    })
}

pub fn paste(session: &mut Session, path: PathBuf) {
    session.paste_error = None;
    if let Some(reason) = paste_block(session) {
        reject(session, reason.into());
        return;
    }
    let doc = session.doc.as_ref().unwrap();
    let key = doc.key(session.selection.unwrap()).unwrap();
    let resident = doc
        .images()
        .enumerate()
        .filter(|(i, _)| session.textures.is_resident(ImageId(*i as u32)))
        .map(|(_, image)| image.hash)
        .collect();
    let options = session.texture_options;
    let available = (u64::from(options.budget_mib) << 20).saturating_sub(session.textures.bytes);
    let (tx, result) = mpsc::channel();
    let wake = session.egui_ctx.clone();
    let input = path.clone();
    let spawned = std::thread::Builder::new()
        .name("texture-paste".into())
        .spawn(move || {
            let prepared =
                std::panic::catch_unwind(|| read_image(&input, options, available, resident))
                    .unwrap_or_else(|_| Err("画像を読めません: 画素の復号に失敗しました".into()));
            let _ = tx.send(prepared);
            if let Some(ctx) = wake {
                ctx.request_repaint();
            }
        });
    if spawned.is_err() {
        reject(session, "画像の読み込みを始められませんでした".into());
        return;
    }
    session.paste_job = Some(PasteJob {
        key,
        path,
        result,
        stale: false,
        generation: session.shown_generation,
        revision: session.history.doc_rev(),
    });
    session.status_text = "画像を読み込んでいます".into();
}

fn reject(session: &mut Session, message: String) {
    session.paste_error = Some(message.clone());
    session.status_text = message;
}

pub(super) fn poll(session: &mut Session) -> bool {
    let Some(job) = session.paste_job.as_ref() else {
        return false;
    };
    let result = match job.result.try_recv() {
        Ok(result) => result,
        Err(mpsc::TryRecvError::Empty) => return false,
        Err(mpsc::TryRecvError::Disconnected) => Err("画像の読み込みが中断されました".into()),
    };
    let job = session.paste_job.take().unwrap();
    if job.stale
        || job.generation != session.shown_generation
        || job.revision != session.history.doc_rev()
        || session.is_loading()
        || session.is_exporting()
    {
        reject(session, CHANGED.into());
    } else if let Err(reason) = result.and_then(|image| finish(session, job, image)) {
        reject(session, reason);
    }
    true
}

fn finish(session: &mut Session, job: PasteJob, image: PreparedImage) -> Result<(), String> {
    let started = Instant::now();
    let doc = session.doc.as_mut().ok_or(CHANGED)?;
    let root = doc.id_of(job.key).ok_or(CHANGED)?;
    let bounds = doc.subtree_bounds(root);
    let longest = (bounds.max - bounds.min).max_element();
    let world_size = if longest > 0.0 && longest.is_finite() {
        longest
    } else {
        1.0
    };
    let image_id = doc
        .try_add_image(image.bytes, image.mime)
        .map_err(|err| match err {
            DocError::TooLarge => "貼った画像のメモリの上限（256 MiB）に空きがありません",
            _ => "画像を文書に追加できませんでした",
        })?;
    let applied = change_looks(
        session,
        job.key,
        "画像の貼り付け",
        "画像を貼りました",
        started,
        |doc, draw, k| {
            if !k.is_finite() || k <= 0.0 {
                return None;
            }
            let old = draw.tex.and_then(|id| doc.tex_use(id));
            let mut tex = vertex_mapping(doc, draw).unwrap_or(TexUse {
                image: None,
                sampling: Sampling::default(),
                map: UvMap::Box {
                    size: match old.map(|tex| tex.map) {
                        Some(UvMap::Box { size, .. }) => size,
                        _ => world_size / k,
                    },
                    aspect: image.size[0] as f32 / image.size[1] as f32,
                },
                cutoff: old.and_then(|tex| tex.cutoff),
                baked_box: None,
            });
            tex.image = Some(image_id);
            Some(([1.0, 1.0, 1.0, draw.base_color[3]], Some(tex)))
        },
    )?;
    if !applied {
        session.status_text = "画像を貼りました".into();
    }
    session.remember_opened_file(&job.path);
    session.remember_opened_file(&image.canonical);
    session.file_history.remember_opened(&image.original);
    session.protect_document(&job.path);
    if let Some(chain) = image.chain {
        session.textures.queue_pasted(
            TextureReady {
                generation: job.generation,
                image: image_id,
                chain,
            },
            image.shrunk,
        );
    }
    Ok(())
}

fn change_looks(
    session: &mut Session,
    key: NodeKey,
    label: &'static str,
    message: &str,
    started: Instant,
    change: impl Fn(&Document, &Draw, f32) -> Option<([f32; 4], Option<TexUse>)>,
) -> Result<bool, &'static str> {
    let doc = session.doc.as_mut().ok_or(SELECT_PART)?;
    let id = doc.id_of(key).ok_or(CHANGED)?;
    let before = doc.subtree_looks(key).map_err(|_| CHANGED)?;
    let mut after = before.clone();
    let mut at = 0;
    for node in doc.subtree(id).map(NodeId) {
        let k = scale(doc, node);
        for slot in 0..doc.draws(node).len() {
            let draw = doc.draws(node)[slot];
            if let Some((base_color, use_)) = change(doc, &draw, k) {
                let tex = use_
                    .map(|tex| doc.add_tex_use(tex))
                    .transpose()
                    .map_err(|_| "貼り方を文書に追加できませんでした")?;
                after[at] = DrawLook { base_color, tex };
            }
            at += 1;
        }
    }
    if before == after {
        return Ok(false);
    }
    let step = Step::new(
        label,
        vec![Edit::SetLooks { key, before, after }],
        Some(key),
    );
    let applied = edit::execute(session, step).ok_or("画像の変更を確定できませんでした")?;
    if applied.retextured {
        session.scene_dirty.instances = true;
        session.pending_patches.clear();
    }
    session.metrics.edit_commit = Some(started.elapsed());
    if session.history.can_undo() {
        session.status_text = message.into();
    }
    Ok(true)
}

#[derive(Clone, Copy)]
pub(super) enum Adjustment {
    Remove,
    Size(f32),
}

pub(super) fn adjust(session: &mut Session, adjustment: Adjustment) {
    if session.block_edit_if_locked() {
        return;
    }
    let Some(key) = session
        .selection
        .and_then(|id| session.doc.as_ref()?.key(id))
    else {
        return;
    };
    let (label, message) = match adjustment {
        Adjustment::Remove => ("画像を外す", "画像を外しました"),
        Adjustment::Size(size) if size.is_finite() && size > 0.0 => {
            ("画像の大きさ", "画像の大きさを変えました")
        }
        Adjustment::Size(_) => {
            session.status_text =
                "1枚の大きさには、0 より大きい有限の長さを入力してください".into();
            return;
        }
    };
    if let Err(reason) = change_looks(
        session,
        key,
        label,
        message,
        Instant::now(),
        |doc, draw, k| {
            let mut tex = *doc.tex_use(draw.tex?)?;
            tex.image?;
            match adjustment {
                Adjustment::Remove => {
                    let info_from = match tex.map {
                        UvMap::Vertex { info_from, .. } => info_from,
                        UvMap::Box { .. } => None,
                    };
                    let mask = tex.cutoff.map(|cutoff| TexUse {
                        image: None,
                        sampling: Sampling::default(),
                        cutoff: Some(cutoff),
                        baked_box: None,
                        map: UvMap::Vertex {
                            transform: IDENTITY_TRANSFORM,
                            tex_coord: 0,
                            info_from,
                        },
                    });
                    Some((draw.base_color, mask))
                }
                Adjustment::Size(world) => {
                    let UvMap::Box { aspect, .. } = tex.map else {
                        return None;
                    };
                    let size = world / k;
                    if !(size.is_finite() && size > 0.0) {
                        return None;
                    }
                    tex.map = UvMap::Box { size, aspect };
                    Some((draw.base_color, Some(tex)))
                }
            }
        },
    ) {
        session.status_text = reason.into();
    }
}
