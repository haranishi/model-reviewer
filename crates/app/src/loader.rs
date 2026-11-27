//! 読み込みは1回につき1本の別スレッド。UIはイベントを受け取るだけ。

use std::any::Any;
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
use model_reviewer_core::units::UnitLabel;
use model_reviewer_gltf::{LoadError, load_glb};
use model_reviewer_stl::{StlOptions, detect as detect_stl, load_stl};

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

/// 読み込みの指定。`stl_unit` は STL の単位の仮定（`None` は mm）。GLB では使わない。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadOptions {
    pub stl_unit: Option<UnitLabel>,
}

impl LoadOptions {
    /// STL を開くときの単位の仮定（指定が無ければ mm。要件7.6・8.5）。
    pub fn stl_unit_or_default(self) -> UnitLabel {
        self.stl_unit.unwrap_or(UnitLabel::Mm)
    }
}

/// GLB でも STL でもないファイルの知らせ（要件5.2）。
pub const NEITHER_GLB_NOR_STL: &str = "GLB でも STL でもありません";

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
    /// 元の形式（STL なら単位の仮定つき）。
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
}

pub(crate) struct LoadHandle {
    cancel: Arc<AtomicBool>,
    rx: Receiver<LoadEvent>,
}

impl LoadHandle {
    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    pub(crate) fn drain(&self) -> Vec<LoadEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            events.push(event);
        }
        events
    }
}

pub(crate) fn spawn(
    generation: u64,
    path: PathBuf,
    expected_identity: Option<ModelIdentity>,
    options: LoadOptions,
    hooks: LoadHooks,
    ctx: Option<egui::Context>,
) -> LoadHandle {
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let flag = Arc::clone(&cancel);
    let sender = tx.clone();
    let ctx_if_unspawned = ctx.clone();
    let spawned = thread::Builder::new()
        .name("model-load".to_owned())
        .spawn(move || {
            let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                load_thread(
                    generation,
                    Request {
                        path,
                        expected_identity,
                        options,
                    },
                    hooks,
                    flag,
                    &sender,
                    &ctx,
                );
            }));
            if let Err(payload) = panicked {
                let detail = panic_text(payload);
                log::error!("load thread panicked: {detail}");
                let _ = sender.send(LoadEvent::Failed {
                    generation,
                    message: LoadError::Internal(detail).message_ja(),
                });
                if let Some(ctx) = &ctx {
                    ctx.request_repaint();
                }
            }
        });
    if let Err(err) = spawned {
        log::error!("failed to spawn load thread: {err}");
        let _ = tx.send(LoadEvent::Failed {
            generation,
            message: LoadError::Internal(err.to_string()).message_ja(),
        });
        if let Some(ctx) = &ctx_if_unspawned {
            ctx.request_repaint();
        }
    }
    LoadHandle { cancel, rx }
}

/// 読み込む物（読み込みのスレッドへ渡す）。
struct Request {
    path: PathBuf,
    expected_identity: Option<ModelIdentity>,
    options: LoadOptions,
}

fn load_thread(
    generation: u64,
    request: Request,
    hooks: LoadHooks,
    cancel: Arc<AtomicBool>,
    tx: &mpsc::Sender<LoadEvent>,
    ctx: &Option<egui::Context>,
) {
    let Request {
        path,
        expected_identity,
        options,
    } = request;
    let cancelled = || cancel.load(Ordering::Acquire);
    let emit = |event: LoadEvent| -> bool {
        if tx.send(event).is_err() {
            return false;
        }
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
        true
    };

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
            let name = stl_part_name(&path);
            load_stl(
                &bytes,
                &StlOptions {
                    name: &name,
                    unit,
                    allow_trailing,
                },
            )
            .map(|model| (model, SourceKind::Stl { unit }))
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
    drop(bytes);
    let model = Arc::new(model);
    let stl_unit = match source_kind {
        SourceKind::Glb => None,
        SourceKind::Stl { unit } => Some(unit),
    };
    let model_ref = model_ref_for(resolved_path, identity, stl_unit);
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
    let cancel_for_pick = Arc::clone(&cancel);
    let index =
        PickIndex::build_cancellable(&model, &move || cancel_for_pick.load(Ordering::Acquire));
    let elapsed = started.elapsed();
    if cancelled() {
        return;
    }
    if let Some(index) = index {
        let _ = emit(LoadEvent::Pick {
            generation,
            index,
            elapsed,
        });
    }
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
    stl_unit: Option<UnitLabel>,
) -> ModelRef {
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
        for (bytes, unit) in [
            (b"part-\xff.stl".as_slice(), Some(UnitLabel::Mm)),
            (b"part-\xfe.glb".as_slice(), None),
        ] {
            let path = Path::new(OsStr::from_bytes(bytes));
            let reference =
                model_ref_for(path.to_owned(), ModelIdentity::from_bytes(b"model"), unit);
            assert_eq!(reference.path, path);
            assert_eq!(reference.file_name, path.to_string_lossy());
            let json = review_file::to_json(&review, &model, &reference).unwrap();
            let saved = review_file::parse(&json).unwrap().model;
            assert_eq!(saved.path, path.to_string_lossy());
            assert_eq!(saved.file_name, reference.file_name);
        }
    }
}
