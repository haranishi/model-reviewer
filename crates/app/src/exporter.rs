//! GLBの書き出しを別スレッドで行う。工程は「読み取り中 → 組み立て中 → 書き込み中 → 確かめ中」。
//!
//! 約束:
//! - 元のファイルを読み直し、大きさとハッシュが開いたときの値と同じことを確かめる（違えば書かない）。
//!   土台が STL なら、照合の後に STL を読み、最小の GLB を合成して土台にする（STL は書き換えない）。
//!   新規の文書（`ExportBase::Empty`）は、ファイルを読まずに空の土台を使う。
//! - 保存先と同じフォルダの一時ファイルへ書き、`model_reviewer_gltf::export::verify` で照合してから
//!   名前を付け替える。失敗したら一時ファイルを消し、保存先には何も残さない。
//! - 「開く」で開いたファイル（`forbidden`）へは書き出さない（正規化したパスか同じファイルなら断る）。
//! - 画面を起こすのは工程が進んだときだけ（読み込みと同じ）。

use std::any::Any;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use model_reviewer_core::identity::ModelIdentity;
use model_reviewer_core::units::{UnitLabel, UpAxis};
use model_reviewer_gltf::export::{
    ExportError, ExportNote, ExportScene, build, empty_base, synthesize_base, verify_locals,
    write_container,
};
use model_reviewer_stl::{StlOptions, load_stl};

use crate::loader;
use crate::metrics;

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 書き出しの依頼。
#[derive(Clone, Debug)]
pub struct ExportRequest {
    /// いまの土台のファイル（開いたファイル、または前に書き出したファイル）。
    pub base_path: PathBuf,
    /// 土台を開いた（書き出した）ときの大きさとハッシュ。
    pub base_identity: ModelIdentity,
    pub target: PathBuf,
    pub scene: ExportScene,
    /// この作業で「開く」から開いたファイル。書き出し先にできない。
    pub forbidden: Vec<PathBuf>,
    /// 土台の種類。STL なら、読み直した STL から最小の GLB を合成して土台にする。
    pub base_kind: ExportBase,
}

/// 書き出しの土台の種類。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ExportBase {
    /// 土台のファイル（GLB）をそのまま使う。
    #[default]
    File,
    /// 土台の STL を開いたときと同じ仮定と部品名で読み直す。
    Stl {
        unit: UnitLabel,
        up: UpAxis,
        name: String,
    },
    /// 新規の文書。ファイルを読まず、識別も照合せず、`empty_base()` を土台にする
    /// （`base_path` と `base_identity` は使わない）。
    Empty,
}

/// 工程の前に呼ぶ試験用の関数。
pub type StageHook = Arc<dyn Fn() + Send + Sync>;
/// 一時ファイルの場所を受け取る試験用の関数。
pub type PathHook = Arc<dyn Fn(&Path) + Send + Sync>;
/// STL から合成した GLB を、編集を重ねる前に受け取る試験用の関数。
pub type StlBaseHook = Arc<dyn Fn(&[u8]) + Send + Sync>;

/// 各工程の前後に呼ぶ試験用の関数。本番は空。
#[derive(Clone, Default)]
pub struct ExportHooks {
    pub before_read: Option<StageHook>,
    pub after_stl_base: Option<StlBaseHook>,
    pub before_build: Option<StageHook>,
    pub before_write: Option<StageHook>,
    pub before_verify: Option<StageHook>,
    /// 一時ファイルへ書いた直後に呼ぶ（照合の失敗を再現する試験用）。
    pub after_write: Option<PathHook>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportStage {
    Reading,
    Building,
    Writing,
    Verifying,
}

impl ExportStage {
    /// 状態欄に出す工程名。
    pub fn label_ja(self) -> &'static str {
        match self {
            Self::Reading => "読み取り中",
            Self::Building => "組み立て中",
            Self::Writing => "書き込み中",
            Self::Verifying => "確かめ中",
        }
    }
}

/// 工程ごとの時間（性能値に出す）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExportTimes {
    pub read: Duration,
    pub build: Duration,
    pub write: Duration,
    pub verify: Duration,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportDone {
    pub path: PathBuf,
    /// 書いたバイト列の大きさとハッシュ（新しい作業対象の識別）。
    pub identity: ModelIdentity,
    /// 行きがけ順の各部品のファイル内の番号（`Document::set_file_indices` へ渡す）。
    pub assigned: Vec<u32>,
    /// 書いたファイルを読み直した、行きがけ順の各部品の親に対する行列
    /// （`Document::adopt_file_locals` へ渡す。開き直したときと同じ値）。
    pub locals: Vec<glam::Affine3A>,
    pub notes: Vec<ExportNote>,
    pub times: ExportTimes,
    /// 書き出し中に観測した常駐メモリの最大（取れなければ `None`）。
    pub peak_mem: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExportFailure {
    /// 元のファイルが、開いたときから変わっている。
    BaseChanged,
    /// 保存先が「開く」で開いたファイルと同じ。
    SameAsOpened,
    Export(ExportError),
    Io(String),
}

impl ExportFailure {
    /// 利用者に出す理由。
    pub fn message_ja(&self) -> String {
        match self {
            Self::BaseChanged => {
                "元のファイルが開いたときから変わっているため、書き出せません。編集はそのまま残っています。"
                    .to_owned()
            }
            Self::SameAsOpened => {
                "開いたファイルへは書き出せません。別の名前を選んでください。".to_owned()
            }
            Self::Export(error) => error.message_ja(),
            Self::Io(detail) => format!("書き出せませんでした: {detail}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExportEvent {
    Stage(ExportStage),
    Done(Box<ExportDone>),
    Failed(ExportFailure),
}

/// 実行中の書き出し。
pub struct ExportHandle {
    events: Receiver<ExportEvent>,
}

impl ExportHandle {
    /// 届いた知らせを取り出す（待たない）。
    pub fn drain(&self) -> Vec<ExportEvent> {
        self.events.try_iter().collect()
    }
}

/// 書き出しを始める。`wake` は工程が進んだときに画面を起こすための文脈。
pub fn start(request: ExportRequest, wake: Option<egui::Context>) -> ExportHandle {
    start_with_hooks(request, ExportHooks::default(), wake)
}

/// 試験用のフック付きで書き出しを始める。
pub fn start_with_hooks(
    request: ExportRequest,
    hooks: ExportHooks,
    wake: Option<egui::Context>,
) -> ExportHandle {
    let (tx, events) = mpsc::channel();
    let sender = tx.clone();
    let wake_if_unspawned = wake.clone();
    let spawned = thread::Builder::new()
        .name("model-export".to_owned())
        .spawn(move || {
            let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let outcome = run_export(request, hooks, &sender, &wake);
                match outcome {
                    Ok(done) => emit(&sender, &wake, ExportEvent::Done(Box::new(done))),
                    Err(failure) => emit(&sender, &wake, ExportEvent::Failed(failure)),
                }
            }));
            if let Err(payload) = panicked {
                let detail = panic_text(payload);
                log::error!("export thread panicked: {detail}");
                emit(
                    &sender,
                    &wake,
                    ExportEvent::Failed(ExportFailure::Io(format!("内部エラー（{detail}）"))),
                );
            }
        });
    if let Err(err) = spawned {
        log::error!("failed to spawn export thread: {err}");
        emit(
            &tx,
            &wake_if_unspawned,
            ExportEvent::Failed(ExportFailure::Io(format!(
                "書き出しを始められませんでした（{err}）"
            ))),
        );
    }
    ExportHandle { events }
}

/// 2つのパスが同じファイルか（Unix は装置番号とiノード番号、Windows はファイルの識別子、または正規化したパスで判定）。
/// リンク経由（シンボリックリンク・ハードリンク）と相対パスも含めて見分ける。
pub fn same_file(a: &Path, b: &Path) -> bool {
    if a.as_os_str().is_empty() || b.as_os_str().is_empty() {
        return false;
    }
    // Windows はファイルの識別子で比べる。ハードリンクは正規化したパスが一致しない。
    #[cfg(windows)]
    if let Ok(true) = ::same_file::is_same_file(a, b) {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(ma), Ok(mb)) = (fs::metadata(a), fs::metadata(b))
            && ma.dev() == mb.dev()
            && ma.ino() == mb.ino()
        {
            return true;
        }
        // 壊れたシンボリックリンク同士が同じリンクを指す場合も検出する。
        if let (Ok(ma), Ok(mb)) = (fs::symlink_metadata(a), fs::symlink_metadata(b))
            && ma.dev() == mb.dev()
            && ma.ino() == mb.ino()
        {
            return true;
        }
    }
    if let (Ok(ca), Ok(cb)) = (fs::canonicalize(a), fs::canonicalize(b))
        && ca == cb
    {
        return true;
    }
    matches!(
        (normalize_path(a), normalize_path(b)),
        (Some(na), Some(nb)) if na == nb
    )
}

fn run_export(
    request: ExportRequest,
    hooks: ExportHooks,
    tx: &mpsc::Sender<ExportEvent>,
    wake: &Option<egui::Context>,
) -> Result<ExportDone, ExportFailure> {
    if is_forbidden(&request.target, &request.forbidden) {
        return Err(ExportFailure::SameAsOpened);
    }
    let destination = resolve_destination(&request.target)?;
    if is_forbidden(&destination, &request.forbidden) {
        return Err(ExportFailure::SameAsOpened);
    }
    if destination.is_dir() {
        return Err(ExportFailure::Io("保存先がフォルダです".to_owned()));
    }

    let mut peak_mem = metrics::sample_memory();
    let sample_peak = |peak: &mut Option<u64>| {
        if let Some(now) = metrics::sample_memory() {
            *peak = Some(peak.map_or(now, |prev| prev.max(now)));
        }
    };

    // 1. 読み取り中（元のファイルを読み直し、大きさとハッシュを照合）。
    emit(tx, wake, ExportEvent::Stage(ExportStage::Reading));
    call_hook(&hooks.before_read);
    let started = Instant::now();
    let base_bytes = match &request.base_kind {
        // 新規の文書には元のファイルが無い。読む工程と識別の照合を飛ばす（要件2.1）。
        ExportBase::Empty => empty_base(),
        ExportBase::File | ExportBase::Stl { .. } => {
            let base_bytes = fs::read(&request.base_path).map_err(|err| {
                ExportFailure::Io(format!(
                    "元のファイルを読めません（{}）",
                    loader::io_error_ja(&err)
                ))
            })?;
            let actual_identity = ModelIdentity::from_bytes(&base_bytes);
            if actual_identity != request.base_identity {
                return Err(ExportFailure::BaseChanged);
            }
            // 土台が STL なら、読み直した STL から最小の GLB を作り、それを土台にする（要件8.1）。
            // STL のバイト列は合成が終わったら手放す（土台のバイト列を常駐させない）。
            match &request.base_kind {
                ExportBase::Stl { unit, up, name } => {
                    let base = synthesize_from_stl(&base_bytes, *unit, *up, name)?;
                    if let Some(hook) = &hooks.after_stl_base {
                        hook(&base);
                    }
                    base
                }
                _ => base_bytes,
            }
        }
    };
    sample_peak(&mut peak_mem);
    let read_time = started.elapsed();
    sample_peak(&mut peak_mem);

    // 2. 組み立て中。
    emit(tx, wake, ExportEvent::Stage(ExportStage::Building));
    call_hook(&hooks.before_build);
    let started = Instant::now();
    let built = build(&base_bytes, &request.scene).map_err(ExportFailure::Export)?;
    let build_time = started.elapsed();
    sample_peak(&mut peak_mem);

    // 3. 書き込み中（保存先と同じフォルダの一時ファイルへ書く）。
    emit(tx, wake, ExportEvent::Stage(ExportStage::Writing));
    call_hook(&hooks.before_write);
    let started = Instant::now();
    let (file, temp) = create_temp(&destination).map_err(ExportFailure::Io)?;
    {
        let mut writer = BufWriter::new(file);
        write_container(&mut writer, &base_bytes, &built).map_err(ExportFailure::Export)?;
        // 名前を付け替えた後に電源が落ちても、保存先が空のファイルにならないよう、中身を
        // 装置へ書き切ってから付け替える。
        let file = writer
            .into_inner()
            .map_err(|err| ExportFailure::Io(format!("書き込めません（{}）", err.error())))?;
        file.sync_all()
            .map_err(|err| ExportFailure::Io(format!("書き込めません（{err}）")))?;
    }
    if let Some(hook) = &hooks.after_write {
        hook(&temp.path);
    }
    let write_time = started.elapsed();
    sample_peak(&mut peak_mem);

    // 4. 確かめ中（一時ファイルを読み直して照合してから、名前を付け替える）。
    emit(tx, wake, ExportEvent::Stage(ExportStage::Verifying));
    call_hook(&hooks.before_verify);
    let started = Instant::now();
    let written = fs::read(&temp.path).map_err(|err| {
        ExportFailure::Io(format!(
            "書いた一時ファイルを読めません（{}）",
            loader::io_error_ja(&err)
        ))
    })?;
    sample_peak(&mut peak_mem);
    let identity = ModelIdentity::from_bytes(&written);
    let locals = verify_locals(&written, &base_bytes, &request.scene, &built)
        .map_err(ExportFailure::Export)?;
    // 名前を付け替える直前にも、開いたファイルと重なっていないことを確かめる。
    if is_forbidden(&destination, &request.forbidden) {
        return Err(ExportFailure::SameAsOpened);
    }
    temp.replace(&destination).map_err(|err| {
        ExportFailure::Io(format!(
            "保存先へ置き換えられませんでした（{}）",
            loader::io_error_ja(&err)
        ))
    })?;
    let verify_time = started.elapsed();
    sample_peak(&mut peak_mem);

    Ok(ExportDone {
        path: destination,
        identity,
        assigned: built.assigned,
        locals,
        notes: built.notes,
        times: ExportTimes {
            read: read_time,
            build: build_time,
            write: write_time,
            verify: verify_time,
        },
        peak_mem,
    })
}

/// STL のバイト列から、読み込みと同じ `Model` を作り直して最小の GLB にする。開いたときと同じ
/// 名前・単位・向きで読むので、部品0・描画0・形状0が文書と同じ物を指す。
fn synthesize_from_stl(
    bytes: &[u8],
    unit: UnitLabel,
    up: UpAxis,
    name: &str,
) -> Result<Vec<u8>, ExportFailure> {
    let model = load_stl(
        bytes,
        &StlOptions {
            name,
            unit,
            up,
            allow_trailing: true,
        },
    )
    .map_err(|err| ExportFailure::Io(format!("元の STL を読めません（{}）", err.message_ja())))?;
    synthesize_base(&model).map_err(ExportFailure::Export)
}

fn is_forbidden(candidate: &Path, forbidden: &[PathBuf]) -> bool {
    forbidden.iter().any(|opened| same_file(opened, candidate))
}

fn resolve_destination(target: &Path) -> Result<PathBuf, ExportFailure> {
    let Some(file_name) = target.file_name() else {
        return Err(ExportFailure::Io(
            "保存先のファイル名がありません".to_owned(),
        ));
    };
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let canonical_parent = fs::canonicalize(parent).map_err(|err| {
        ExportFailure::Io(format!(
            "保存先のフォルダを確認できません（{}）",
            loader::io_error_ja(&err)
        ))
    })?;
    Ok(canonical_parent.join(file_name))
}

pub(crate) struct TempFile {
    path: PathBuf,
    armed: bool,
}

impl TempFile {
    pub(crate) fn replace(mut self, destination: &Path) -> io::Result<()> {
        fs::rename(&self.path, destination)?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(crate) fn create_temp(destination: &Path) -> Result<(File, TempFile), String> {
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = destination
        .file_name()
        .ok_or_else(|| "保存先のファイル名がありません".to_owned())?;
    let pid = std::process::id();
    for _ in 0..64 {
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let mut tmp_name = OsString::from(".");
        tmp_name.push(name);
        tmp_name.push(format!(".{pid}.{seq}.tmp"));
        let path = parent.join(tmp_name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((file, TempFile { path, armed: true })),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(format!(
                    "一時ファイルを作れません（{}）",
                    loader::io_error_ja(&err)
                ));
            }
        }
    }
    Err("一時ファイルを作れません".to_owned())
}

fn normalize_path(path: &Path) -> Option<PathBuf> {
    if path.as_os_str().is_empty() {
        return None;
    }
    if let Ok(canon) = fs::canonicalize(path) {
        return Some(canon);
    }
    if let Some(file_name) = path.file_name() {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if let Ok(canon_parent) = fs::canonicalize(parent) {
            return Some(canon_parent.join(file_name));
        }
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    Some(clean_lexical(&absolute))
}

fn clean_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(component.as_os_str());
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn call_hook(hook: &Option<StageHook>) {
    if let Some(hook) = hook {
        hook();
    }
}

fn emit(tx: &mpsc::Sender<ExportEvent>, wake: &Option<egui::Context>, event: ExportEvent) {
    if tx.send(event).is_ok()
        && let Some(ctx) = wake
    {
        ctx.request_repaint();
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
