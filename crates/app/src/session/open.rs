//! モデルとレビューを開く。

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use model_reviewer_core::identity::ModelIdentity;
use model_reviewer_core::scene::LoadWarning;
use model_reviewer_core::texture::{ImageId, chain_bytes, sniff};

use crate::command::{LaunchArgs, is_review_path};
use crate::gpu_view::ImageSink;
use crate::loader::{self, LoadOptions};

use super::{Pending, Session};

/// 表示中のモデルの画像。`done` は、送り終えた画像と省いた画像を数える。
#[derive(Default)]
pub struct TextureProgress {
    pub total: usize,
    pub done: usize,
    pub shrunk: usize,
    pub failed: Vec<(ImageId, String)>,
    pub bytes: u64,
    resident: HashMap<ImageId, [u32; 2]>,
    pasted: Option<loader::TextureReady>,
    reasons: BTreeMap<String, usize>,
    pub(super) planned_shrinks: Vec<ImageId>,
    in_flight: Option<(ImageId, [u32; 2])>,
    pub(super) opened: Option<Instant>,
    last_upload: Option<Duration>,
}

impl TextureProgress {
    /// 元のモデルに属する画像と、転送途中ならその進捗だけを残す。
    pub(super) fn discard_edits(&mut self, base_images: usize) {
        let original = |id: ImageId| (id.0 as usize) < base_images;
        self.pasted.take_if(|ready| !original(ready.image));
        self.resident.retain(|&id, size| {
            if original(id) {
                true
            } else {
                self.bytes -= chain_bytes(*size);
                false
            }
        });
        if self.in_flight.take_if(|(id, _)| !original(*id)).is_some() {
            self.bytes = self.resident.values().copied().map(chain_bytes).sum();
        }
        self.failed.retain(|&(id, _)| original(id));
        self.reasons.clear();
        for (_, reason) in &self.failed {
            *self.reasons.entry(reason.clone()).or_default() += 1;
        }
        self.planned_shrinks.retain(|&id| original(id));
        self.shrunk = self
            .planned_shrinks
            .iter()
            .filter(|id| self.resident.contains_key(id))
            .count();
        self.done = self.displayed() + self.failed.len();
        self.total = self.total.min(base_images);
    }

    pub fn displayed(&self) -> usize {
        self.resident.len()
    }

    pub fn is_resident(&self, image: ImageId) -> bool {
        self.resident.contains_key(&image)
    }

    pub(super) fn display_size(&self, image: ImageId) -> Option<[u32; 2]> {
        self.resident.get(&image).copied()
    }

    pub(super) fn queue_pasted(&mut self, ready: loader::TextureReady, shrunk: bool) {
        let id = ready.image;
        if let Some(i) = self.failed.iter().position(|(failed, _)| *failed == id) {
            let (_, reason) = self.failed.remove(i);
            if let Some(count) = self.reasons.get_mut(&reason) {
                *count -= 1;
                if *count == 0 {
                    self.reasons.remove(&reason);
                }
            }
            self.done -= 1;
        } else {
            self.total += 1;
        }
        match (shrunk, self.planned_shrinks.binary_search(&id)) {
            (true, Err(at)) => self.planned_shrinks.insert(at, id),
            (false, Ok(at)) => {
                self.planned_shrinks.remove(at);
            }
            _ => {}
        }
        self.pasted = Some(ready);
    }

    pub(super) fn fail(&mut self, image: ImageId, reason: String) {
        *self.reasons.entry(reason.clone()).or_default() += 1;
        self.failed.push((image, reason));
        self.done += 1;
    }

    pub(crate) fn warning_count(&self) -> usize {
        usize::from(self.shrunk > 0) + self.reasons.len()
    }

    pub(crate) fn warnings(&self, budget_mib: u32) -> impl Iterator<Item = String> + '_ {
        (self.shrunk > 0)
            .then(|| {
                format!(
                    "画像 {} 枚を縮小して表示しています（上限 {budget_mib} MiB）",
                    self.shrunk
                )
            })
            .into_iter()
            .chain(self.reasons.iter().map(|(reason, n)| {
                format!("画像 {n} 枚は読めないため、色だけで表示します（{reason}）")
            }))
    }
}

impl Session {
    /// 状態欄の「簡略表示」に出すモデルの知らせ。画像を基本色に掛けて表示する知らせ
    /// （`Textures`）は省いたものではないので出さない（色の欄の注意で伝える）。
    pub fn simplification_warnings(&self) -> impl Iterator<Item = &LoadWarning> {
        self.model
            .iter()
            .flat_map(|model| model.warnings.iter())
            .filter(|warning| !matches!(warning, LoadWarning::Textures { .. }))
    }

    /// ファイルの解析で省いた参照も一覧に数える。表示を無効にしても元の枚数は分かる。
    pub(crate) fn texture_counts(&self) -> (usize, usize) {
        let Some(model) = self.model.as_ref() else {
            return (0, 0);
        };
        let skipped: usize = model
            .warnings
            .iter()
            .filter_map(|w| match w {
                LoadWarning::ImagesSkipped { count, .. } => Some(*count as usize),
                _ => None,
            })
            .sum();
        (
            self.doc.as_ref().map_or(0, |doc| doc.images().count()) + skipped,
            self.textures.failed.len() + skipped,
        )
    }

    pub fn configure_textures(&mut self, launch: &LaunchArgs, device_max_side: Option<u32>) {
        self.texture_options = loader::TextureOptions {
            enabled: !launch.no_textures && device_max_side.is_some(),
            max_side: device_max_side.unwrap_or(0).min(loader::MAX_TEXTURE_SIDE),
            budget_mib: launch.texture_budget_mib,
        };
        self.textures_shown = self.texture_options.enabled;
    }

    /// 計画がまだ無い期間と、スレッドが終わった後の最後の転送も含む。
    pub fn textures_busy(&self) -> bool {
        self.loader.is_some()
            || self.shown_loader.is_some()
            || self.paste_job.is_some()
            || self.textures.pasted.is_some()
            || self.textures.in_flight.is_some()
    }

    pub(crate) fn allow_first_image(&self) {
        if let Some(handle) = &self.shown_loader {
            handle.allow_image();
        }
    }

    pub(super) fn reset_textures(&mut self) {
        if let Some(job) = &mut self.paste_job {
            job.stale = true;
        }
        self.texture_cache.set(None);
        self.paste_error = None;
        self.textures = TextureProgress::default();
        self.metrics.texture_present = None;
        self.metrics.texture_peak_mem = None;
    }

    pub fn texture_progress_label(&self) -> Option<String> {
        let p = &self.textures;
        (p.done < p.total).then(|| format!("テクスチャ {}/{}", p.done, p.total))
    }

    pub(crate) fn observe_texture_memory(&mut self, bytes: Option<u64>) {
        if self.texture_options.enabled
            && self.textures.opened.is_some()
            && (self.shown_loader.is_some() || self.textures.in_flight.is_some())
            && self
                .model
                .as_ref()
                .is_some_and(|m| !m.textures.images.is_empty())
            && let Some(bytes) = bytes
        {
            self.metrics.texture_peak_mem =
                Some(self.metrics.texture_peak_mem.unwrap_or(0).max(bytes));
        }
    }

    pub(super) fn finish_texture_metrics(&mut self) {
        if self.textures.done == self.textures.total {
            self.metrics.texture_present = self.textures.last_upload;
            self.textures.opened = None;
        }
    }

    /// 形状を送った残りの予算で、表示中のモデルへ最大1枚を送る。戻り値は描き直しの予約。
    pub fn poll_images(&mut self, sink: &mut impl ImageSink, budget: u64) -> bool {
        if self.textures.in_flight.is_none() {
            let ready = if self.textures.pasted.is_some() {
                if budget < 4 {
                    return true;
                }
                self.textures.pasted.take().unwrap()
            } else {
                let Some(handle) = &self.shown_loader else {
                    return false;
                };
                if self.textures.done >= self.textures.total || budget < 4 {
                    return self.texture_options.enabled
                        && self
                            .model
                            .as_ref()
                            .is_some_and(|m| !m.textures.images.is_empty());
                }
                let Ok(ready) = handle.images.try_recv() else {
                    return true;
                };
                handle.allow_image();
                ready
            };
            if ready.generation != self.shown_generation {
                return true;
            }
            let size = ready.chain.size;
            if let Err(reason) = sink.begin_image(ready.image, ready.chain) {
                self.textures.fail(ready.image, reason);
                self.finish_texture_metrics();
                return true;
            }
            self.textures.in_flight = Some((ready.image, size));
        }
        let progress = sink.continue_images(budget);
        self.textures.bytes += progress.uploaded_bytes;
        if let Some(image) = progress.completed {
            let (expected, size) = self.textures.in_flight.take().unwrap();
            debug_assert_eq!(expected, image);
            self.textures.resident.insert(image, size);
            self.textures.done += 1;
            self.textures.shrunk +=
                usize::from(self.textures.planned_shrinks.binary_search(&image).is_ok());
            if let Some(started) = self.textures.opened {
                self.textures.last_upload = Some(started.elapsed());
            }
            self.finish_texture_metrics();
            self.color_cache.set(None);
            self.scene_dirty.display = true;
        }
        true
    }
}

pub(super) fn open_guarded(session: &mut Session, path: PathBuf, options: LoadOptions) {
    open_guarded_with_identity(session, path, options, None);
}

pub(super) fn open_guarded_with_identity(
    session: &mut Session,
    path: PathBuf,
    options: LoadOptions,
    expected: Option<ModelIdentity>,
) {
    if session.is_exporting() {
        session.status_text = "書き出し中は別のファイルを開けません".to_owned();
        return;
    }
    session.protect_document(&path);
    if session.hold(Pending::OpenModel(path.clone(), options, expected.clone())) {
        return;
    }
    open_model(session, path, options, expected);
}

pub(super) fn review_guarded(session: &mut Session, path: PathBuf) {
    if session.is_exporting() {
        session.status_text = "書き出し中は別のファイルを開けません".to_owned();
        return;
    }
    session.protect_document(&path);
    if session.hold(Pending::LoadReview(path.clone())) {
        return;
    }
    load_review(session, path);
}

pub(super) fn open_model(
    session: &mut Session,
    path: PathBuf,
    options: LoadOptions,
    expected: Option<ModelIdentity>,
) {
    if session.is_exporting() {
        session.end_discard_bypass();
        session.status_text = "書き出し中は別のファイルを開けません".to_owned();
        return;
    }
    session.restore = None;
    session.start_model_load_with_expected(path, expected, options);
}

/// 新規作成（要件1.1・1.3・1.4）。書き出し中・読み込み中は断る。未保存があれば確認を出し、
/// 選ばれるまで今の文書を残す（確認の後は `confirm_discard` が `new_document` へ続ける）。
pub(super) fn new_guarded(session: &mut Session) {
    if refuse_new(session) {
        return;
    }
    if session.hold(Pending::New) {
        return;
    }
    new_document(session);
}

/// 未保存の確認を済ませた後の新規作成。復元の途中と進行中の読み込みを消し、空のモデルを置く。
pub fn new_document(session: &mut Session) {
    if refuse_new(session) {
        session.end_discard_bypass();
        return;
    }
    session.restore = None;
    session.start_new_document();
}

fn refuse_new(session: &mut Session) -> bool {
    let reason = if session.is_exporting() {
        "書き出し中は新規作成できません"
    } else if session.is_loading() {
        "読み込み中は新規作成できません"
    } else {
        return false;
    };
    session.status_text = reason.to_owned();
    true
}

pub fn load_review(session: &mut Session, path: PathBuf) {
    super::persist::load_review(session, path);
}

pub(super) const NOT_REGULAR_FILE: &str = "通常のファイルではないため開けません";

pub fn dropped(session: &mut Session, paths: Vec<PathBuf>) {
    let Some(first) = paths.first().cloned() else {
        return;
    };
    if paths.len() > 1 {
        session.notice = "複数のファイルのうち、最初の1つだけを開きます".to_owned();
    } else {
        session.notice.clear();
    }
    // 書き手のいない名前付きパイプなどは開くだけで止まる。中身を読む前に通常のファイルか確かめる。
    if std::fs::metadata(&first).is_ok_and(|meta| !meta.is_file()) {
        session.status_text = NOT_REGULAR_FILE.to_owned();
        return;
    }
    let mut header = Vec::with_capacity(8);
    let _ = std::fs::File::open(&first).and_then(|f| f.take(8).read_to_end(&mut header));
    if sniff(&header).is_some() {
        super::texture::paste(session, first);
    } else if session.is_exporting() {
        session.status_text = "書き出し中は別のファイルを開けません".to_owned();
    } else if header.starts_with(b"glTF") || !is_review_path(&first) {
        open_guarded(session, first, LoadOptions::default());
    } else {
        review_guarded(session, first);
    }
}

/// 起動引数。拡張子は目安で、先頭4バイトが glTF ならモデルとして開く。
pub(super) fn open_launch(session: &mut Session, path: PathBuf) {
    if is_review_path(&path) && !loader::file_starts_with_glb(&path) {
        review_guarded(session, path);
    } else {
        open_guarded(session, path, LoadOptions::default());
    }
}
