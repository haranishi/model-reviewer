//! 読み込みは1回につき1本の別スレッド。UIはイベントを受け取るだけ。

use std::any::Any;
use std::collections::{HashMap, hash_map::Entry};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use model_reviewer_core::identity::{ModelIdentity, ModelRef};
use model_reviewer_core::pick::PickIndex;
use model_reviewer_core::scene::Model;
use model_reviewer_core::texture::{ImageId, ImageSource, MipChain, plan_levels};
use model_reviewer_core::units::{UnitLabel, UpAxis};
use model_reviewer_gltf::{LoadError, load_glb};
use model_reviewer_stl::{StlOptions, detect as detect_stl, load_stl};
use model_reviewer_texture::{TexError, decode, dimensions};

use crate::session::SourceKind;

/// 画面に出す工程。割合を持つのはGPU転送だけ。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LoadPhase {
    #[default]
    Idle,
    Reading,
    Parsing,
    Uploading {
        uploaded_bytes: u64,
        total_bytes: u64,
    },
    Failed(String),
}

impl LoadPhase {
    /// 進行中の工程名。完了と失敗は状態文の側に出す。転送は実測バイトだけ。
    pub fn label(&self) -> Option<String> {
        match self {
            Self::Reading => Some("読み取り中".to_owned()),
            Self::Parsing => Some("解析中".to_owned()),
            Self::Uploading {
                uploaded_bytes,
                total_bytes,
            } if *total_bytes > 0 => {
                Some(format!("転送中 {uploaded_bytes} / {total_bytes} バイト"))
            }
            Self::Uploading { .. } => Some("転送中".to_owned()),
            Self::Idle | Self::Failed(_) => None,
        }
    }
}

/// STL の読み込みで仮定する単位と上の向き。省略時は mm・Z が上。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadOptions {
    pub stl_unit: Option<UnitLabel>,
    pub stl_up: Option<UpAxis>,
}

impl LoadOptions {
    /// STL を開くときの単位の仮定（指定が無ければ mm。要件7.6・8.5）。
    pub fn stl_unit_or_default(self) -> UnitLabel {
        self.stl_unit.unwrap_or(UnitLabel::Mm)
    }
}

/// GLB でも STL でもないファイルの知らせ（要件5.2）。
pub const NEITHER_GLB_NOR_STL: &str = "GLB でも STL でもありません";

pub(crate) const DEFAULT_TEXTURE_BUDGET_MIB: u32 = 512;
pub(crate) const MAX_TEXTURE_SIDE: u32 = 4096;
const MAX_COMPRESSED_BYTES: usize = 512 << 20;

type CompressedImage = Result<Arc<[u8]>, TexError>;

#[derive(Clone, Copy)]
pub(crate) struct TextureOptions {
    pub enabled: bool,
    pub max_side: u32,
    pub budget_mib: u32,
}

impl Default for TextureOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            max_side: MAX_TEXTURE_SIDE,
            budget_mib: DEFAULT_TEXTURE_BUDGET_MIB,
        }
    }
}

/// 各工程の前に呼ぶ試験用の関数。本番は空。
#[derive(Clone, Default)]
pub struct LoadHooks {
    pub before_read: Option<Arc<dyn Fn() + Send + Sync>>,
    pub before_identity: Option<Arc<dyn Fn() + Send + Sync>>,
    pub before_parse: Option<Arc<dyn Fn() + Send + Sync>>,
    pub before_pick: Option<Arc<dyn Fn() + Send + Sync>>,
}

pub(crate) struct LoadedModel {
    pub generation: u64,
    pub model: Arc<Model>,
    pub model_ref: ModelRef,
    /// 元の形式（STL なら単位と向きの仮定つき）。
    pub source_kind: SourceKind,
    pub file_bytes: u64,
    pub read: Duration,
    pub identity: Duration,
    pub parse: Duration,
}

pub(crate) enum LoadEvent {
    Stage {
        generation: u64,
        phase: LoadPhase,
    },
    Loaded(Box<LoadedModel>),
    Mismatch {
        generation: u64,
        actual: ModelIdentity,
    },
    Failed {
        generation: u64,
        message: String,
    },
    Pick {
        generation: u64,
        index: PickIndex,
        elapsed: Duration,
    },
    TexturePlan {
        generation: u64,
        total: usize,
        shrunk: Vec<ImageId>,
        skipped: Vec<(ImageId, String)>,
    },
    TextureFailed {
        generation: u64,
        image: ImageId,
        error: String,
    },
    TexturesDone {
        generation: u64,
    },
}

pub(crate) struct TextureReady {
    pub generation: u64,
    pub image: ImageId,
    pub chain: MipChain,
}

pub(crate) struct LoadHandle {
    cancel: Arc<AtomicBool>,
    rx: Receiver<LoadEvent>,
    pub images: Receiver<TextureReady>,
    next_image: mpsc::Sender<()>,
}

impl LoadHandle {
    /// 最初は形状の表示後、以後は画像を待ち行列から取り出したときに1枚だけ許す。
    pub(crate) fn allow_image(&self) {
        let _ = self.next_image.send(());
    }

    pub(crate) fn drain(&self) -> Vec<LoadEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            events.push(event);
        }
        events
    }
}

impl Drop for LoadHandle {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

pub(crate) fn spawn(
    generation: u64,
    path: PathBuf,
    expected_identity: Option<ModelIdentity>,
    options: LoadOptions,
    textures: TextureOptions,
    hooks: LoadHooks,
    ctx: Option<egui::Context>,
) -> LoadHandle {
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let (image_tx, images) = mpsc::sync_channel(1);
    let (next_image, permits) = mpsc::channel();
    let flag = Arc::clone(&cancel);
    let sender = tx.clone();
    let ctx_if_unspawned = ctx.clone();
    let spawned = thread::Builder::new()
        .name("model-load".to_owned())
        .spawn(move || {
            let emit = |event| {
                let sent = sender.send(event).is_ok();
                if let Some(ctx) = &ctx {
                    ctx.request_repaint();
                }
                sent
            };
            let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                load_thread(
                    generation,
                    Request {
                        path,
                        expected_identity,
                        options,
                        textures: ImageStream {
                            options: textures,
                            sender: image_tx,
                            permits,
                        },
                    },
                    hooks,
                    flag,
                    &emit,
                    ctx.as_ref(),
                );
            }));
            if let Err(payload) = panicked {
                let detail = panic_text(payload);
                log::error!("load thread panicked: {detail}");
                emit(LoadEvent::Failed {
                    generation,
                    message: LoadError::Internal(detail).message_ja(),
                });
            }
            emit(LoadEvent::TexturesDone { generation });
        });
    if let Err(err) = spawned {
        log::error!("failed to spawn load thread: {err}");
        let _ = tx.send(LoadEvent::Failed {
            generation,
            message: LoadError::Internal(err.to_string()).message_ja(),
        });
        let _ = tx.send(LoadEvent::TexturesDone { generation });
        if let Some(ctx) = &ctx_if_unspawned {
            ctx.request_repaint();
        }
    }
    LoadHandle {
        cancel,
        rx,
        images,
        next_image,
    }
}

/// 読み込む物（読み込みのスレッドへ渡す）。
struct Request {
    path: PathBuf,
    expected_identity: Option<ModelIdentity>,
    options: LoadOptions,
    textures: ImageStream,
}

struct ImageStream {
    options: TextureOptions,
    sender: mpsc::SyncSender<TextureReady>,
    permits: Receiver<()>,
}

fn load_thread(
    generation: u64,
    request: Request,
    hooks: LoadHooks,
    cancel: Arc<AtomicBool>,
    emit: &impl Fn(LoadEvent) -> bool,
    ctx: Option<&egui::Context>,
) {
    let Request {
        path,
        expected_identity,
        options,
        textures,
    } = request;
    let cancelled = || cancel.load(Ordering::Acquire);

    if cancelled() {
        return;
    }
    call_hook(&hooks.before_read);
    if !emit(LoadEvent::Stage {
        generation,
        phase: LoadPhase::Reading,
    }) {
        return;
    }
    let started = Instant::now();
    let resolved_path = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    let bytes = match std::fs::read(&resolved_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            log::error!("failed to read model: {err}");
            let _ = emit(LoadEvent::Failed {
                generation,
                message: io_error_ja(&err),
            });
            return;
        }
    };
    let read = started.elapsed();
    if cancelled() {
        return;
    }

    call_hook(&hooks.before_identity);
    let started = Instant::now();
    let identity = ModelIdentity::from_bytes(&bytes);
    let identity_elapsed = started.elapsed();
    if cancelled() {
        return;
    }

    if let Some(expected) = &expected_identity
        && identity != *expected
    {
        let _ = emit(LoadEvent::Mismatch {
            generation,
            actual: identity,
        });
        return;
    }

    // 形式は中身で決める（要件5.2）。`glTF` の印が無ければ STL として確かめる。拡張子が `.stl`
    // なら、判定に通らなくても読んでみて、具体的な理由を出す（末尾の余りも警告にして読む）。
    let format = if is_glb(&bytes) {
        Format::Glb
    } else {
        let stl_extension = has_stl_extension(&path);
        if stl_extension || detect_stl(&bytes, false).is_some() {
            Format::Stl {
                allow_trailing: stl_extension,
            }
        } else {
            let _ = emit(LoadEvent::Failed {
                generation,
                message: NEITHER_GLB_NOR_STL.to_owned(),
            });
            return;
        }
    };

    call_hook(&hooks.before_parse);
    if !emit(LoadEvent::Stage {
        generation,
        phase: LoadPhase::Parsing,
    }) {
        return;
    }
    let started = Instant::now();
    let parsed = match format {
        Format::Glb => load_glb(&bytes)
            .map(|model| (model, SourceKind::Glb))
            .map_err(|err| {
                log::error!("failed to parse model: {err}");
                err.message_ja()
            }),
        Format::Stl { allow_trailing } => {
            let unit = options.stl_unit_or_default();
            let up = options.stl_up.unwrap_or_default();
            let name = stl_part_name(&path);
            load_stl(
                &bytes,
                &StlOptions {
                    name: &name,
                    unit,
                    up,
                    allow_trailing,
                },
            )
            .map(|model| (model, SourceKind::Stl { unit, up }))
            .map_err(|err| {
                log::error!("failed to parse stl: {err}");
                err.message_ja()
            })
        }
    };
    let (model, source_kind) = match parsed {
        Ok(parsed) => parsed,
        Err(message) => {
            let _ = emit(LoadEvent::Failed {
                generation,
                message,
            });
            return;
        }
    };
    let parse = started.elapsed();
    // `load_glb`・`load_stl` の途中では止められない。届いた結果は世代番号で捨てる。
    // 読んだバイト列はここで手放す（索引を作る間に常駐させない）。
    let file_bytes = bytes.len() as u64;
    let images = if textures.options.enabled {
        let sources = model.textures.images.iter().map(|info| &info.source);
        let Some(images) = compressed_images(&bytes, sources, cancelled) else {
            return;
        };
        images
    } else {
        Vec::new()
    };
    drop(bytes);
    let model = Arc::new(model);
    let model_ref = model_ref_for(resolved_path, identity, source_kind);
    if !emit(LoadEvent::Loaded(Box::new(LoadedModel {
        generation,
        model: Arc::clone(&model),
        model_ref,
        source_kind,
        file_bytes,
        read,
        identity: identity_elapsed,
        parse,
    }))) {
        return;
    }
    if cancelled() {
        return;
    }

    call_hook(&hooks.before_pick);
    let started = Instant::now();
    let index = PickIndex::build_cancellable(&model, &cancelled);
    let elapsed = started.elapsed();
    drop(model);
    if cancelled() {
        return;
    }
    if let Some(index) = index {
        emit(LoadEvent::Pick {
            generation,
            index,
            elapsed,
        });
    }
    load_images(generation, textures, images, cancelled, emit, ctx);
}

fn load_images(
    generation: u64,
    stream: ImageStream,
    images: Vec<CompressedImage>,
    cancelled: impl Fn() -> bool,
    emit: &impl Fn(LoadEvent) -> bool,
    ctx: Option<&egui::Context>,
) {
    if images.is_empty() || stream.permits.recv().is_err() || cancelled() {
        return;
    }

    let mut sizes = Vec::with_capacity(images.len());
    let mut skipped = Vec::new();
    for (i, bytes) in images.iter().enumerate() {
        if cancelled() {
            return;
        }
        match bytes
            .as_deref()
            .map_err(TexError::message_ja)
            .and_then(|bytes| dimensions(bytes).map_err(|e| e.message_ja()))
        {
            Ok(size) => sizes.push(size),
            Err(reason) => {
                sizes.push([0, 0]);
                skipped.push((ImageId(i as u32), reason));
            }
        }
    }
    let plan = plan_levels(
        &sizes,
        stream.options.max_side,
        u64::from(stream.options.budget_mib) << 20,
    );
    let mut shrunk = Vec::new();
    for (i, (&size, &skip)) in sizes.iter().zip(&plan).enumerate() {
        let id = ImageId(i as u32);
        match skip {
            Some(1..) => shrunk.push(id),
            None if size != [0, 0] => skipped.push((
                id,
                format!(
                    "画像の上限（長辺 {} 画素・合計 {} MiB）",
                    stream.options.max_side, stream.options.budget_mib
                ),
            )),
            _ => {}
        }
    }
    if !emit(LoadEvent::TexturePlan {
        generation,
        total: images.len(),
        shrunk,
        skipped,
    }) {
        return;
    }
    for (i, (bytes, skip)) in images.into_iter().zip(plan).enumerate() {
        if cancelled() {
            return;
        }
        let (Ok(bytes), Some(skip)) = (bytes, skip) else {
            continue;
        };
        let image = ImageId(i as u32);
        let decoded = std::panic::catch_unwind(|| decode(&bytes, skip))
            .unwrap_or_else(|_| Err(TexError::Broken("画素の復号に失敗しました".into())));
        drop(bytes);
        match decoded {
            Ok(chain) => {
                if cancelled()
                    || stream
                        .sender
                        .send(TextureReady {
                            generation,
                            image,
                            chain,
                        })
                        .is_err()
                {
                    return;
                }
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
                if stream.permits.recv().is_err() {
                    return;
                }
            }
            Err(error) => {
                if !emit(LoadEvent::TextureFailed {
                    generation,
                    image,
                    error: error.message_ja(),
                }) {
                    return;
                }
            }
        }
    }
}

fn compressed_images<'a>(
    file: &[u8],
    sources: impl Iterator<Item = &'a ImageSource>,
    cancelled: impl Fn() -> bool,
) -> Option<Vec<CompressedImage>> {
    let mut images = Vec::new();
    let mut ranges = HashMap::new();
    let mut remaining = file.len().min(MAX_COMPRESSED_BYTES);
    for source in sources {
        if cancelled() {
            return None;
        }
        let bytes = match source {
            ImageSource::Held(bytes) => Ok(Arc::clone(bytes)),
            ImageSource::File { offset, len } => match ranges.entry((*offset, *len)) {
                Entry::Occupied(entry) => Ok(Arc::clone(entry.get())),
                Entry::Vacant(entry) => image_range(file, *offset, *len).and_then(|range| {
                    remaining = remaining.checked_sub(range.len()).ok_or_else(|| {
                        TexError::Broken("圧縮画像のコピーが合計の上限を超えます".into())
                    })?;
                    Ok(Arc::clone(entry.insert(Arc::from(range))))
                }),
            },
        };
        images.push(bytes);
    }
    Some(images)
}

/// `load_glb` が検証した JSON チャンクの次が BIN。offset はそのデータの先頭から数える。
fn image_range(file: &[u8], offset: u64, len: u64) -> Result<&[u8], TexError> {
    let range_error = || TexError::Broken("画像の範囲が不正です".into());
    let json_len = file
        .get(12..16)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(range_error)?;
    let start = (28 + u64::from(u32::from_le_bytes(json_len)))
        .checked_add(offset)
        .ok_or_else(range_error)?;
    let end = start.checked_add(len).ok_or_else(range_error)?;
    let range = usize::try_from(start).ok().zip(usize::try_from(end).ok());
    range
        .and_then(|(a, b)| file.get(a..b))
        .ok_or_else(range_error)
}

fn call_hook(hook: &Option<Arc<dyn Fn() + Send + Sync>>) {
    if let Some(hook) = hook {
        hook();
    }
}

#[derive(Clone, Copy)]
enum Format {
    Glb,
    Stl { allow_trailing: bool },
}

/// 拡張子が `.stl`（大文字小文字を問わない）か。
pub fn has_stl_extension(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.to_string_lossy().eq_ignore_ascii_case("stl"))
}

/// STL の部品の名前（ファイル名から拡張子を除いたもの）。空にはしない（空の名前は、書き出した
/// GLB を開き直すと別の名前になる）。
pub fn stl_part_name(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or_else(|| "STL".to_owned())
}

fn is_glb(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && &bytes[..4] == b"glTF"
}

/// 拡張子ではなく、先頭4バイトが `glTF` か。開けないときは false。
pub fn file_starts_with_glb(path: &Path) -> bool {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut magic = [0; 4];
    file.read_exact(&mut magic).is_ok() && is_glb(&magic)
}

pub(crate) fn io_error_ja(err: &std::io::Error) -> String {
    if err.kind() == std::io::ErrorKind::NotFound {
        "ファイルが見つかりません".to_owned()
    } else {
        format!("ファイルを読めません（{err}）")
    }
}

pub(crate) fn model_ref_for(
    path: PathBuf,
    identity: ModelIdentity,
    source_kind: SourceKind,
) -> ModelRef {
    let (stl_unit, stl_up) = match source_kind {
        SourceKind::Stl { unit, up } => (Some(unit), Some(up)),
        SourceKind::Glb | SourceKind::New => (None, None),
    };
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "モデル".to_owned());
    ModelRef {
        file_name,
        path,
        relative_path: None,
        identity,
        stl_unit,
        stl_up,
    }
}

fn panic_text(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic".to_owned()
    }
}

#[cfg(test)]
mod compressed_tests {
    use super::*;

    #[test]
    fn bin_ranges_share_storage() {
        let file = [0u8; 128];
        let sources = vec![ImageSource::File { offset: 0, len: 96 }; 64];
        let images = compressed_images(&file, sources.iter(), || false).unwrap();
        assert_eq!(images.len(), sources.len());
        let first = images[0].as_ref().unwrap();
        assert_eq!(first.len(), 96);
        assert!(
            images
                .iter()
                .all(|image| Arc::ptr_eq(image.as_ref().unwrap(), first))
        );
    }

    #[test]
    fn bin_copies_are_bounded() {
        let file = [0u8; 128];
        let sources = [
            ImageSource::File { offset: 0, len: 96 },
            ImageSource::File { offset: 1, len: 96 },
            ImageSource::File { offset: 0, len: 96 },
            ImageSource::File { offset: 96, len: 4 },
        ];
        let images = compressed_images(&file, sources.iter(), || false).unwrap();
        assert!(images[0].is_ok());
        assert!(
            images[1]
                .as_ref()
                .unwrap_err()
                .message_ja()
                .contains("上限")
        );
        assert!(Arc::ptr_eq(
            images[0].as_ref().unwrap(),
            images[2].as_ref().unwrap()
        ));
        assert_eq!(images[3].as_ref().unwrap().len(), 4);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use model_reviewer_core::{review::ReviewState, review_file};
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    #[test]
    fn model_reference_keeps_the_native_reopen_path() {
        let model = load_glb(&model_reviewer_testdata::inputs::triangle()).unwrap();
        let review = ReviewState::new(&model);
        for (bytes, source_kind) in [
            (
                b"part-\xff.stl".as_slice(),
                SourceKind::Stl {
                    unit: UnitLabel::Mm,
                    up: UpAxis::Z,
                },
            ),
            (b"part-\xfe.glb".as_slice(), SourceKind::Glb),
        ] {
            let path = Path::new(OsStr::from_bytes(bytes));
            let reference = model_ref_for(
                path.to_owned(),
                ModelIdentity::from_bytes(b"model"),
                source_kind,
            );
            assert_eq!(reference.path, path);
            assert_eq!(reference.file_name, path.to_string_lossy());
            let json = review_file::to_json(&review, &model, &reference).unwrap();
            let saved = review_file::parse(&json).unwrap().model;
            assert_eq!(saved.path, path.to_string_lossy());
            assert_eq!(saved.file_name, reference.file_name);
        }
    }
}
