//! 設定ファイル（既定と違う割り当てだけ）の場所・読み・書き。画面も `Session` も読まない。
//! 環境変数は直接読まず、引数で受ける（本物の環境を渡すのは実行ファイルの入口だけ）。
//!
//! 起動しただけでは何も書かない。壊れたファイルは、初めて保存する直前に `.broken` として残す。
//! 新しい版と読めないファイルは上書きしない（design-settings.md の「読むときの決まり」）。

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::{Action, Chord, Keymap};

/// ファイルの `format` の値。
pub const FORMAT: &str = "model-reviewer-keymap";
/// このアプリが読み書きする版。
pub const VERSION: u64 = 1;
/// 設定として読み書きするバイト数の上限。
pub const MAX_BYTES: u64 = 64 * 1024;

const APP_DIR: &str = "model-reviewer";
const FILE_NAME: &str = "keymap.json";

const BROKEN_NOTICE: &str = "設定を読めなかったため、初期値で起動しました";
const NEWER_NOTICE: &str = "新しい版の設定です。このアプリでは変更を保存しません";
const DOCUMENT_NOTICE: &str =
    "モデル・レビュー・自動操作の手順と同じファイルには設定を保存しません";
const LARGE_NOTICE: &str = "ファイルが 64 KiB を超えるため、設定を保存しません";
const NON_JSON_NOTICE: &str =
    "JSON 形式ではないため、初期値を使います。このファイルには設定を保存しません";

/// 設定の場所の決まりを分けるOS。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    Other,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Other
        }
    }
}

/// OSごとの設定ファイルの場所。決められなければ `None`。`env` は環境変数を読む関数。
///
/// 値が絶対の場所かは、実行しているOSではなく `os` の決まりで見る（試験をどのOSでも同じ
/// 結果にするため）。フォルダは作らない。
pub fn default_path(os: Os, env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let base = |name: &str| env(name).filter(|value| is_absolute_for(os, value));
    let folder = match os {
        Os::Mac => PathBuf::from(base("HOME")?)
            .join("Library")
            .join("Application Support"),
        Os::Windows => PathBuf::from(base("APPDATA")?),
        Os::Other => match base("XDG_CONFIG_HOME") {
            Some(config) => PathBuf::from(config),
            None => PathBuf::from(base("HOME")?).join(".config"),
        },
    };
    Some(folder.join(APP_DIR).join(FILE_NAME))
}

/// `os` の決まりで絶対の場所か。空は偽。
fn is_absolute_for(os: Os, value: &OsString) -> bool {
    let text = value.to_string_lossy();
    match os {
        Os::Mac | Os::Other => text.starts_with('/'),
        Os::Windows => {
            let bytes = text.as_bytes();
            let unc = text.starts_with("\\\\") || text.starts_with("//");
            let drive = bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && matches!(bytes[2], b'\\' | b'/');
            unc || drive
        }
    }
}

/// 起動の引数から場所を決める。「`--keymap` があればそれ ＞ 自動操作なら `None` ＞ `default_path`」。
pub fn resolve_path(
    flag: Option<&Path>,
    scripted: bool,
    os: Os,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    if let Some(flag) = flag {
        return Some(flag.to_path_buf());
    }
    if scripted {
        return None;
    }
    default_path(os, env)
}

/// 設定ファイルの様子。保存できるか、画面に出す事情があるかを決める。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreState {
    /// 場所が無い（読まない・書かない）。
    NoPath,
    /// ファイルが無かった、または読めた。保存できる。
    Ready,
    /// 読めない形だった。保存できる（初めて保存する直前に元の内容を `.broken` に残す）。
    Broken,
    /// このアプリより新しい版。保存しない。
    Newer,
    /// 設定で置き換えられない利用者のファイル。
    Protected(&'static str),
    /// 読めなかった（権限など）。保存しない。理由を持つ。
    Unreadable(String),
}

/// 割り当てがどこから来たか（自動操作の状態の `keys.source`）。起動時に決まり、保存では変えない。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreSource {
    Default,
    File,
}

/// 設定ファイルの持ち手。
#[derive(Clone, Debug, PartialEq)]
pub struct Store {
    path: Option<PathBuf>,
    documents: Vec<PathBuf>,
    state: StoreState,
    source: StoreSource,
    /// 知らない最上位の項目（読んだ値のまま書き戻す）。
    extra: serde_json::Map<String, serde_json::Value>,
}

/// `Store::open` の結果。
#[derive(Clone, Debug, PartialEq)]
pub struct Opened {
    pub store: Store,
    pub keymap: Keymap,
    /// 起動時に状態欄へ出す知らせ（読めなかった、捨てた件数など）。
    pub notice: Option<String>,
}

impl Default for Store {
    fn default() -> Self {
        Self::none()
    }
}

/// 壊れた設定の控えには、検査したバイト列をそのまま使う。
enum Contents {
    Missing,
    Broken(Vec<u8>),
    Settings {
        bindings: Map<String, Value>,
        extra: Map<String, Value>,
    },
}

impl Store {
    /// 場所の無い持ち手（読まない・書かない）。
    pub fn none() -> Store {
        Store {
            path: None,
            documents: Vec::new(),
            state: StoreState::NoPath,
            source: StoreSource::Default,
            extra: serde_json::Map::new(),
        }
    }

    /// 場所のファイルを読む。どんな中身でも落ちない。起動しただけでは何も書かない。
    pub fn open(path: PathBuf) -> Opened {
        let (bindings, extra) = match read_settings(&path) {
            Ok(Contents::Settings { bindings, extra }) => (bindings, extra),
            Ok(Contents::Missing) => return Self::fallback(path, StoreState::Ready),
            Ok(Contents::Broken(_)) => return Self::fallback(path, StoreState::Broken),
            Err(state) => return Self::fallback(path, state),
        };
        let (keymap, dropped) = build_keymap(bindings);
        let notice = (dropped > 0).then(|| format!("割り当て {dropped} 件を読めませんでした"));
        Opened {
            store: Store {
                path: Some(path),
                documents: Vec::new(),
                state: StoreState::Ready,
                source: StoreSource::File,
                extra,
            },
            keymap,
            notice,
        }
    }

    pub(crate) fn open_for(path: PathBuf, documents: Vec<PathBuf>) -> Opened {
        if documents
            .iter()
            .any(|doc| crate::exporter::same_file(doc, &path))
        {
            return Self::fallback(path, StoreState::Protected(DOCUMENT_NOTICE));
        }
        let mut opened = Self::open(path);
        opened.store.documents = documents;
        opened
    }

    fn fallback(path: PathBuf, state: StoreState) -> Opened {
        let notice = match &state {
            StoreState::Broken => Some(BROKEN_NOTICE.into()),
            StoreState::Newer => Some(NEWER_NOTICE.into()),
            StoreState::Protected(reason) => Some((*reason).into()),
            StoreState::Unreadable(reason) => Some(format!(
                "設定ファイルを読めませんでした（{reason}）。初期値で起動し、変更は保存しません"
            )),
            _ => None,
        };
        Opened {
            store: Store {
                path: Some(path),
                documents: Vec::new(),
                state,
                source: StoreSource::Default,
                extra: Map::new(),
            },
            keymap: Keymap::default(),
            notice,
        }
    }

    /// 開こうとしたパスを保存直前にも照合する（後から同じファイルへのリンクになり得る）。
    pub(crate) fn protect_document(&mut self, document: &Path) -> Option<&'static str> {
        if !self.saves() {
            return None;
        }
        if self
            .path()
            .is_some_and(|path| crate::exporter::same_file(document, path))
        {
            self.state = StoreState::Protected(DOCUMENT_NOTICE);
            return Some(DOCUMENT_NOTICE);
        }
        if !self.documents.iter().any(|path| path == document) {
            self.documents.push(document.to_owned());
        }
        None
    }

    /// 既定と違う割り当てだけを書く。保存しない様子なら何もせず `Ok`。失敗の理由は日本語の文。
    ///
    /// 同じフォルダの一時ファイルに書き切ってから付け替える。壊れたファイルの内容は、初めて
    /// 保存する直前に `<名前>.broken` に残す（すでにあれば置き換える）。
    pub fn save(&mut self, keymap: &Keymap) -> Result<(), String> {
        if !self.saves() {
            return Ok(());
        }
        let Some(path) = self.path.clone() else {
            return Ok(());
        };
        let bytes = file_bytes(keymap, &self.extra)?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        if path.file_name().is_none() {
            return Err("設定の保存先にファイル名がありません".to_owned());
        }
        std::fs::create_dir_all(parent).map_err(|err| {
            format!(
                "設定を保存するフォルダを作れませんでした（{}）",
                io_reason(&err)
            )
        })?;
        let tmp = write_temp(&path, &bytes)
            .map_err(|err| format!("設定を書けませんでした（{}）", io_reason(&err)))?;
        let result = self.replace(&tmp, &path);
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }

    fn replace(&mut self, tmp: &Path, path: &Path) -> Result<(), String> {
        if let Contents::Broken(original) = self.check_destination(path)? {
            let mut name = path.as_os_str().to_owned();
            name.push(".broken");
            let backup = PathBuf::from(name);
            let copy = write_temp(&backup, &original).map_err(|err| {
                format!(
                    "読めなかった設定の控えを書けませんでした（{}）",
                    io_reason(&err)
                )
            })?;
            let result = self
                .check_destination(&backup)
                .and_then(|_| rename(&copy, &backup));
            if let Err(reason) = result {
                let _ = std::fs::remove_file(copy);
                return Err(reason);
            }
            if let Contents::Broken(current) = self.check_destination(path)?
                && current != original
            {
                return Err("控えを作る間に設定が変更されたため、保存しませんでした".to_owned());
            }
        }
        rename(tmp, path)?;
        self.state = StoreState::Ready;
        Ok(())
    }

    fn check_destination(&mut self, path: &Path) -> Result<Contents, String> {
        let contents = if self
            .documents
            .iter()
            .any(|doc| crate::exporter::same_file(doc, path))
        {
            Err(StoreState::Protected(DOCUMENT_NOTICE))
        } else {
            read_settings(path)
        };
        contents.map_err(|state| {
            if let StoreState::Unreadable(reason) = state {
                return format!("設定ファイルを置き換えられませんでした（{reason}）");
            }
            self.state = state;
            self.note(false)
                .unwrap_or("設定を保存できませんでした")
                .to_owned()
        })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn state(&self) -> &StoreState {
        &self.state
    }

    pub fn source(&self) -> StoreSource {
        self.source
    }

    /// 変更を保存するか。
    pub fn saves(&self) -> bool {
        self.path.is_some() && matches!(self.state, StoreState::Ready | StoreState::Broken)
    }

    /// 知らない最上位の項目。
    pub fn extra(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.extra
    }

    /// 設定の画面に出す「保存されない事情」の文（`KeysInput.store_note`）。保存するなら `None`。
    /// 場所が無いとき、自動操作で起動したか（`scripted`）で文を分ける。
    pub fn note(&self, scripted: bool) -> Option<&'static str> {
        match &self.state {
            StoreState::NoPath if scripted => Some("自動操作のため、変更は保存されません"),
            StoreState::NoPath => Some("設定の場所が決められないため、変更は保存されません"),
            StoreState::Newer => Some("新しい版の設定のため、変更は保存されません"),
            StoreState::Protected(reason) => Some(reason),
            StoreState::Unreadable(_) => Some("設定ファイルを読めないため、変更は保存されません"),
            StoreState::Ready | StoreState::Broken => None,
        }
    }
}

/// 読む。無い・読めない・通常のファイルでないを分ける。上限より1バイト多くまでしか読まない。
fn read_settings(path: &Path) -> Result<Contents, StoreState> {
    let read = || -> io::Result<Vec<u8>> {
        if !std::fs::metadata(path)?.is_file() {
            return Err(io::Error::other("通常のファイルではありません"));
        }
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    };
    let bytes = match read() {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Contents::Missing),
        Err(err) => return Err(StoreState::Unreadable(io_reason(&err))),
    };
    if bytes.starts_with(b"glTF") {
        return Err(StoreState::Protected("GLB ファイルには設定を保存しません"));
    }
    if bytes.len() as u64 > MAX_BYTES {
        return Err(StoreState::Protected(LARGE_NOTICE));
    }
    let value =
        serde_json::from_slice(&bytes).map_err(|_| StoreState::Protected(NON_JSON_NOTICE))?;
    let Value::Object(mut top) = value else {
        return Ok(Contents::Broken(bytes));
    };
    match top.get("format").and_then(Value::as_str) {
        Some(FORMAT) => {}
        Some(model_reviewer_core::review_file::FORMAT) => {
            return Err(StoreState::Protected(DOCUMENT_NOTICE));
        }
        _ => return Ok(Contents::Broken(bytes)),
    }
    match top.get("version").and_then(Value::as_u64) {
        Some(VERSION) => {}
        Some(version) if version > VERSION => return Err(StoreState::Newer),
        _ => return Ok(Contents::Broken(bytes)),
    }
    let Some(Value::Object(bindings)) = top.remove("bindings") else {
        return Ok(Contents::Broken(bytes));
    };
    top.remove("format");
    top.remove("version");
    Ok(Contents::Settings {
        bindings,
        extra: top,
    })
}

/// 版1の `bindings` から表を作る。返すのは表と、捨てた件数。
///
/// ①名前の順に利用者の項目を当てる（書き方が違う・割り当てられない組・ほかの利用者の項目と
/// 重なる組は、その1件を捨てる）。②残りの操作に既定を当てる。既定の組が利用者の項目で
/// 使われていれば、その操作は未割り当てにする。知らない操作の名前は、値のまま持つ。
fn build_keymap(bindings: Map<String, Value>) -> (Keymap, usize) {
    let mut slots: Vec<Option<Option<Chord>>> = vec![None; Action::ALL.len()];
    let mut unknown = BTreeMap::new();
    let mut dropped = 0;
    // `serde_json::Map` は名前の順に並ぶ（`preserve_order` を使っていない）。
    for (name, value) in bindings {
        let Some(action) = Action::from_id(&name) else {
            unknown.insert(name, value);
            continue;
        };
        let chord = match &value {
            Value::Null => None,
            Value::String(text) => match Chord::parse(text) {
                Ok(chord) => Some(chord),
                Err(_) => {
                    dropped += 1;
                    continue;
                }
            },
            _ => {
                dropped += 1;
                continue;
            }
        };
        if let Some(chord) = chord
            && slots.contains(&Some(Some(chord)))
        {
            dropped += 1;
            continue;
        }
        slots[action.index()] = Some(chord);
    }
    let taken: Vec<Chord> = slots.iter().filter_map(|slot| slot.flatten()).collect();
    let bindings = Action::ALL
        .iter()
        .zip(slots)
        .map(|(action, slot)| match slot {
            Some(chord) => chord,
            None => action
                .default_chord()
                .filter(|chord| !taken.contains(chord)),
        })
        .collect();
    (
        Keymap {
            bindings,
            unknown,
            rev: 0,
        },
        dropped,
    )
}

/// ファイルの中身。並びは名前の順、改行は `\n`、末尾に改行を1つ。同じ割り当てなら同じバイト列。
fn file_bytes(keymap: &Keymap, extra: &Map<String, Value>) -> Result<Vec<u8>, String> {
    let mut bindings = Map::new();
    for &action in Action::ALL {
        if keymap.is_changed(action) {
            let value = match keymap.chord(action) {
                Some(chord) => Value::String(chord.to_file()),
                None => Value::Null,
            };
            bindings.insert(action.id().to_owned(), value);
        }
    }
    for (name, value) in keymap.unknown() {
        bindings.insert(name.clone(), value.clone());
    }
    let mut top = extra.clone();
    top.insert("format".to_owned(), Value::String(FORMAT.to_owned()));
    top.insert("version".to_owned(), Value::from(VERSION));
    top.insert("bindings".to_owned(), Value::Object(bindings));
    let mut bytes = serde_json::to_vec_pretty(&top).expect("JSON の値は必ず文字にできる");
    if bytes.len() as u64 >= MAX_BYTES {
        bytes = serde_json::to_vec(&top).expect("JSON の値は必ず文字にできる");
        if bytes.len() as u64 >= MAX_BYTES {
            return Err("保存する設定が 64 KiB を超えるため、変更を保存しませんでした".to_owned());
        }
    }
    bytes.push(b'\n');
    Ok(bytes)
}

/// 同じフォルダに、新規作成できた一時名を取る。同時に保存中の別の窓のファイルには触れない。
fn create_temp(path: &Path) -> io::Result<(PathBuf, File)> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut base = OsString::from(".");
    base.push(
        path.file_name()
            .ok_or_else(|| io::Error::other("ファイル名がありません"))?,
    );
    loop {
        let mut name = base.clone();
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        name.push(format!(".{}.{id}.tmp", std::process::id()));
        let candidate = path.with_file_name(name);
        match File::create_new(&candidate) {
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            result => return result.map(|file| (candidate, file)),
        }
    }
}

fn write_temp(path: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    let (tmp, mut file) = create_temp(path)?;
    let result = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(err) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(tmp)
}

fn rename(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::rename(from, to).map_err(|err| {
        format!(
            "設定ファイルを置き換えられませんでした（{}）",
            io_reason(&err)
        )
    })
}

fn io_reason(err: &std::io::Error) -> String {
    match err.kind() {
        std::io::ErrorKind::NotFound => "見つかりません".to_owned(),
        std::io::ErrorKind::PermissionDenied => "権限がありません".to_owned(),
        _ => err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    use super::{Os, Store, StoreSource, StoreState, default_path, resolve_path};
    use crate::keymap::Keymap;

    #[test]
    fn placeholders_do_not_touch_anything() {
        let env = |_: &str| None;
        assert_eq!(default_path(Os::current(), &env), None);
        assert_eq!(resolve_path(None, true, Os::Other, &env), None);
        let mut store = Store::none();
        assert_eq!(store.state(), &StoreState::NoPath);
        assert_eq!(store.source(), StoreSource::Default);
        assert!(!store.saves());
        assert_eq!(store.save(&Keymap::default()), Ok(()));
        assert!(store.extra().is_empty());
    }

    /// 決めた値だけを返す環境。
    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + use<> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name: &str| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| OsString::from(v))
        }
    }

    fn expected(base: &str, parts: &[&str]) -> PathBuf {
        let mut path = PathBuf::from(base);
        for part in parts {
            path = path.join(part);
        }
        path.join("model-reviewer").join("keymap.json")
    }

    #[test]
    fn default_path_per_os() {
        // macOS: HOME の下の Application Support。
        let env = env_of(&[
            ("HOME", "/u/a"),
            ("APPDATA", "C:\\x"),
            ("XDG_CONFIG_HOME", "/c"),
        ]);
        assert_eq!(
            default_path(Os::Mac, &env),
            Some(expected("/u/a", &["Library", "Application Support"]))
        );
        // Windows: APPDATA の下。HOME は見ない。
        assert_eq!(
            default_path(Os::Windows, &env),
            Some(expected("C:\\x", &[]))
        );
        let unc = env_of(&[("APPDATA", "\\\\server\\share")]);
        assert_eq!(
            default_path(Os::Windows, &unc),
            Some(expected("\\\\server\\share", &[]))
        );
        // そのほか: XDG_CONFIG_HOME が先。
        assert_eq!(default_path(Os::Other, &env), Some(expected("/c", &[])));
        // XDG_CONFIG_HOME が無い・空・相対なら HOME/.config。
        for xdg in [None, Some(""), Some("rel/dir")] {
            let mut pairs = vec![("HOME", "/u/b")];
            if let Some(xdg) = xdg {
                pairs.push(("XDG_CONFIG_HOME", xdg));
            }
            assert_eq!(
                default_path(Os::Other, &env_of(&pairs)),
                Some(expected("/u/b", &[".config"])),
                "XDG_CONFIG_HOME = {xdg:?}"
            );
        }
        // 変数が無い・空・相対なら None。
        for value in [None, Some(""), Some("rel"), Some("~/x")] {
            let pairs: Vec<(&str, &str)> = value
                .map(|v| vec![("HOME", v), ("XDG_CONFIG_HOME", v)])
                .unwrap_or_default();
            let env = env_of(&pairs);
            assert_eq!(default_path(Os::Mac, &env), None, "HOME = {value:?}");
            assert_eq!(default_path(Os::Other, &env), None, "HOME = {value:?}");
        }
        for value in [
            None,
            Some(""),
            Some("x\\y"),
            Some("\\x"),
            Some("C:x"),
            Some("/x"),
        ] {
            let pairs: Vec<(&str, &str)> = value
                .map(|v| vec![("APPDATA", v), ("HOME", "/u/a")])
                .unwrap_or_default();
            assert_eq!(
                default_path(Os::Windows, &env_of(&pairs)),
                None,
                "APPDATA = {value:?}"
            );
        }
        // 実行しているOSの決まりではなく、渡した OS の決まりで見る。
        let win_only = env_of(&[("HOME", "C:\\u")]);
        assert_eq!(default_path(Os::Mac, &win_only), None);
    }

    #[test]
    fn resolve_path_prefers_flag_then_script_then_os() {
        let env = env_of(&[("HOME", "/u/a")]);
        let flag = Path::new("given").join("keys.json");
        // 引数があれば、自動操作でもそれ（相対のまま）。
        assert_eq!(
            resolve_path(Some(&flag), true, Os::Mac, &env),
            Some(flag.clone())
        );
        assert_eq!(
            resolve_path(Some(&flag), false, Os::Other, &env),
            Some(flag.clone())
        );
        // 自動操作で引数が無ければ None（環境を見ない）。
        let untouched = |name: &str| -> Option<OsString> {
            panic!("自動操作では環境を読まない（{name}）")
        };
        assert_eq!(resolve_path(None, true, Os::Mac, &untouched), None);
        // どちらも無ければ OS ごとの場所。
        assert_eq!(
            resolve_path(None, false, Os::Mac, &env),
            default_path(Os::Mac, &env)
        );
        assert!(resolve_path(None, false, Os::Mac, &env).is_some());
        assert_eq!(resolve_path(None, false, Os::Windows, &env), None);
    }

    #[test]
    fn notes_explain_why_nothing_is_saved() {
        let none = Store::none();
        assert_eq!(
            none.note(true),
            Some("自動操作のため、変更は保存されません")
        );
        assert_eq!(
            none.note(false),
            Some("設定の場所が決められないため、変更は保存されません")
        );
        let mut store = Store::none();
        store.path = Some(PathBuf::from("k.json"));
        for (state, note) in [
            (StoreState::Ready, None),
            (StoreState::Broken, None),
            (
                StoreState::Newer,
                Some("新しい版の設定のため、変更は保存されません"),
            ),
            (
                StoreState::Unreadable("権限がありません".to_owned()),
                Some("設定ファイルを読めないため、変更は保存されません"),
            ),
        ] {
            store.state = state;
            assert_eq!(store.note(false), note);
            assert_eq!(store.saves(), note.is_none());
        }
    }
}
