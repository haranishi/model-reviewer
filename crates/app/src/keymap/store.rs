//! 設定ファイル（既定と違う割り当てだけ）の場所・読み・書き。画面も `Session` も読まない。
//! 環境変数は直接読まず、引数で受ける（本物の環境を渡すのは実行ファイルの入口だけ）。
//!
//! いまは形だけを置いている（WP-S1 が中身を書く）。`default_path` と `resolve_path` は場所を
//! 決めず、`open` は既定の割り当てを返して何も読まず、`save` は何も書かない。

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::Keymap;

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
pub fn default_path(os: Os, env: &dyn Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let _ = (os, env);
    None
}

/// 起動の引数から場所を決める。「`--keymap` があればそれ ＞ 自動操作なら `None` ＞ `default_path`」。
pub fn resolve_path(
    flag: Option<&Path>,
    scripted: bool,
    os: Os,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Option<PathBuf> {
    let _ = (flag, scripted, os, env);
    None
}

/// 設定ファイルの様子。保存できるか、画面に出す事情があるかを決める。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreState {
    /// 場所が無い（読まない・書かない）。
    NoPath,
    /// ファイルが無かった、または読めた。保存できる。
    Ready,
    /// 読めない形だった。保存できる（初めて保存する直前に `.broken` へ名前を替えて残す）。
    Broken,
    /// このアプリより新しい版。保存しない。
    Newer,
    /// 読めなかった（権限など）。保存しない。理由を持つ。
    Unreadable(String),
}

/// 割り当てがどこから来たか（自動操作の状態の `keys.source`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreSource {
    Default,
    File,
}

/// 設定ファイルの持ち手。
#[derive(Clone, Debug, PartialEq)]
pub struct Store {
    path: Option<PathBuf>,
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

impl Store {
    /// 場所の無い持ち手（読まない・書かない）。
    pub fn none() -> Store {
        Store {
            path: None,
            state: StoreState::NoPath,
            source: StoreSource::Default,
            extra: serde_json::Map::new(),
        }
    }

    /// 場所のファイルを読む。どんな中身でも落ちない。起動しただけでは何も書かない。
    pub fn open(path: PathBuf) -> Opened {
        Opened {
            store: Store {
                path: Some(path),
                state: StoreState::Ready,
                source: StoreSource::Default,
                extra: serde_json::Map::new(),
            },
            keymap: Keymap::default(),
            notice: None,
        }
    }

    /// 既定と違う割り当てだけを書く。保存しない様子なら何もせず `Ok`。失敗の理由は日本語の文。
    pub fn save(&mut self, keymap: &Keymap) -> Result<(), String> {
        let _ = keymap;
        Ok(())
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
}

#[cfg(test)]
mod tests {
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
}
