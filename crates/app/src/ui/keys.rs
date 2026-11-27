//! ショートカットの設定の画面。`Session` を読まず、入力用の構造体だけを受け取って `Command` を積む。
//!
//! いまは形だけを置いている（WP-V1 が中身を書く）。`show` は何も描かない。

use crate::command::Command;
use crate::keymap::{Action, Platform, Refusal};
use crate::session::KeyConflict;

/// 設定の画面の入力。
#[derive(Clone, Debug, PartialEq)]
pub struct KeysInput {
    pub rows: Vec<KeyRow>,
    pub capture: Option<Action>,
    pub conflict: Option<KeyConflict>,
    pub refusal: Option<(Action, Refusal)>,
    pub reset_all_armed: bool,
    /// 保存されない事情があるときだけの文。
    pub store_note: Option<String>,
    pub platform: Platform,
}

/// 1つの操作の行。
#[derive(Clone, Debug, PartialEq)]
pub struct KeyRow {
    pub action: Action,
    /// 今のキーの文字（未割り当ては `None`）。
    pub key: Option<String>,
    /// 既定と違うか。
    pub changed: bool,
    pub default_key: Option<String>,
    /// 修飾なしの X・Y・Z を持つ行の注意（要件13.2）。
    pub note: Option<&'static str>,
}

pub fn show(ui: &mut egui::Ui, input: &KeysInput, out: &mut Vec<Command>) {
    let _ = (ui, input, out);
}

/// 名前かキーに `filter` を含むか。英字は大文字と小文字を区別しない。空の `filter` はすべて残す。
pub fn matches(row: &KeyRow, filter: &str) -> bool {
    let filter = filter.trim();
    if filter.is_empty() {
        return true;
    }
    let wanted = filter.to_lowercase();
    row.action.label().to_lowercase().contains(&wanted)
        || row
            .key
            .as_deref()
            .is_some_and(|key| key.to_lowercase().contains(&wanted))
}
