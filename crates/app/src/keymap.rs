//! 操作とキーの表。キーの振り分け（`app::shortcut_commands`）と画面のキーの文字は、どちらも
//! この表から作る。`Session` も画面も読まない。
//!
//! 不変の条件: 2つの操作が同じ `Chord` を持たない。

mod actions;
mod chord;
pub mod store;

use std::collections::BTreeMap;
use std::sync::Arc;

pub use actions::{Action, Group};
pub use chord::{Chord, Platform, Refusal};

/// 操作ごとの今の割り当て。`Default` は既定の割り当て。
#[derive(Clone, Debug, PartialEq)]
pub struct Keymap {
    /// `Action::index` の順。
    bindings: Vec<Option<Chord>>,
    /// 設定ファイルにあった、このアプリの知らない操作の名前の項目（読んだ値のまま書き戻す）。
    unknown: BTreeMap<String, serde_json::Value>,
    /// 割り当てが変わるたびに増える版の番号。
    rev: u64,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            bindings: Action::ALL.iter().map(|a| a.default_chord()).collect(),
            unknown: BTreeMap::new(),
            rev: 0,
        }
    }
}

impl Keymap {
    pub fn chord(&self, action: Action) -> Option<Chord> {
        self.bindings[action.index()]
    }

    /// その組を持つ操作。
    pub fn action_for(&self, chord: Chord) -> Option<Action> {
        self.bindings
            .iter()
            .position(|binding| *binding == Some(chord))
            .map(|i| Action::ALL[i])
    }

    /// 既定と違うか。
    pub fn is_changed(&self, action: Action) -> bool {
        self.chord(action) != action.default_chord()
    }

    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// 割り当てる。ほかの操作が同じ組を持っていれば、`replace` が偽なら何も変えずに
    /// `Err(相手)` を返し、真なら相手を未割り当てにする。「初期値に戻す」もこれを使う。
    pub fn assign(&mut self, action: Action, chord: Chord, replace: bool) -> Result<(), Action> {
        if let Some(holder) = self.action_for(chord)
            && holder != action
        {
            if !replace {
                return Err(holder);
            }
            self.bindings[holder.index()] = None;
            self.bindings[action.index()] = Some(chord);
            self.rev += 1;
            return Ok(());
        }
        self.set(action, Some(chord));
        Ok(())
    }

    /// 未割り当てにする。
    pub fn clear(&mut self, action: Action) {
        self.set(action, None);
    }

    /// すべて既定に戻す。知らない操作の項目は残す。
    pub fn reset_all(&mut self) {
        let defaults: Vec<Option<Chord>> = Action::ALL.iter().map(|a| a.default_chord()).collect();
        if self.bindings != defaults {
            self.bindings = defaults;
            self.rev += 1;
        }
    }

    /// 知らない操作の名前の項目（名前の順）。
    pub fn unknown(&self) -> &BTreeMap<String, serde_json::Value> {
        &self.unknown
    }

    /// 画面の文字。割り当てが変わったときだけ作り直す（`rev` を見る）。
    pub fn labels(&self, platform: Platform) -> KeyLabels {
        KeyLabels(Arc::new(
            self.bindings
                .iter()
                .map(|binding| binding.map(|chord| chord.label(platform)))
                .collect(),
        ))
    }

    fn set(&mut self, action: Action, chord: Option<Chord>) {
        let slot = &mut self.bindings[action.index()];
        if *slot != chord {
            *slot = chord;
            self.rev += 1;
        }
    }
}

/// 操作ごとのキーの文字（未割り当ては `None`）。画面の部品は入力用の構造体でこれを受け取る。
/// `Default` は今のOSの既定の文字（空ではない）。
#[derive(Clone, Debug, PartialEq)]
pub struct KeyLabels(Arc<Vec<Option<String>>>);

impl Default for KeyLabels {
    fn default() -> Self {
        Keymap::default().labels(Platform::current())
    }
}

impl KeyLabels {
    pub fn get(&self, action: Action) -> Option<&str> {
        self.0[action.index()].as_deref()
    }

    /// 「移動（G）」。未割り当てなら括弧を出さず「図形」。
    pub fn with_key(&self, action: Action, name: &str) -> String {
        match self.get(action) {
            Some(key) => format!("{name}（{key}）"),
            None => name.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use egui::Key;

    use super::{Action, Chord, KeyLabels, Keymap, Platform};

    fn chord(key: Key, shift: bool, command: bool) -> Chord {
        Chord {
            key,
            shift,
            command,
        }
    }

    #[test]
    fn assign_reports_the_holder() {
        let mut map = Keymap::default();
        let g = chord(Key::G, false, false);
        assert_eq!(map.action_for(g), Some(Action::ToolMove));
        // 重なると相手を返し、何も変えない。
        assert_eq!(
            map.assign(Action::ToolRotate, g, false),
            Err(Action::ToolMove)
        );
        assert_eq!(map, Keymap::default());
        assert_eq!(map.rev(), 0);
        // 置き換えると相手は未割り当て。
        assert_eq!(map.assign(Action::ToolRotate, g, true), Ok(()));
        assert_eq!(map.chord(Action::ToolRotate), Some(g));
        assert_eq!(map.chord(Action::ToolMove), None);
        assert!(map.is_changed(Action::ToolMove) && map.is_changed(Action::ToolRotate));
        assert_eq!(map.rev(), 1);
        // 同じ組を同じ操作へ割り当て直しても、版は増えない。
        assert_eq!(map.assign(Action::ToolRotate, g, false), Ok(()));
        assert_eq!(map.rev(), 1);
        // 空いた組への割り当てと、外す。
        let t = chord(Key::T, false, false);
        assert_eq!(map.assign(Action::ToolMove, t, false), Ok(()));
        assert_eq!(map.action_for(t), Some(Action::ToolMove));
        assert_eq!(map.rev(), 2);
        map.clear(Action::ToolMove);
        map.clear(Action::ToolMove);
        assert_eq!(map.chord(Action::ToolMove), None);
        assert_eq!(map.rev(), 3);
        // 初期値に戻すのは assign で、既定の組がほかで使われていれば相手を返す。
        let default = Action::ToolMove.default_chord().unwrap();
        assert_eq!(
            map.assign(Action::ToolMove, default, false),
            Err(Action::ToolRotate)
        );
        map.reset_all();
        assert_eq!(map.rev(), 4);
        map.reset_all();
        assert_eq!(map.rev(), 4);
        for &action in Action::ALL {
            assert!(!map.is_changed(action));
        }
        assert!(map.unknown().is_empty());
    }

    #[test]
    fn labels_follow_the_keymap() {
        let mut map = Keymap::default();
        let labels = map.labels(Platform::Mac);
        assert_eq!(labels.get(Action::ToolMove), Some("G"));
        assert_eq!(labels.get(Action::EditRedo), Some("Shift+Cmd+Z"));
        assert_eq!(
            map.labels(Platform::Other).get(Action::EditRedo),
            Some("Shift+Ctrl+Z")
        );
        assert_eq!(labels.with_key(Action::ToolMove, "移動"), "移動（G）");
        assert_eq!(labels.with_key(Action::ToolShape, "図形"), "図形");
        map.clear(Action::ToolMove);
        assert_eq!(map.labels(Platform::Mac).get(Action::ToolMove), None);
        assert_eq!(
            KeyLabels::default(),
            Keymap::default().labels(Platform::current())
        );
    }
}
