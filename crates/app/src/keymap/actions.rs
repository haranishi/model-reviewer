//! 割り当てられる操作の一覧と既定。名前（`id`）はファイルに書く文字列で、一度出したら変えない。

use super::chord::Chord;

/// 設定の画面のまとまり。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    File,
    Edit,
    Tool,
    View,
    Display,
    Other,
}

impl Group {
    pub const ALL: &'static [Group] = &[
        Group::File,
        Group::Edit,
        Group::Tool,
        Group::View,
        Group::Display,
        Group::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::File => "ファイル",
            Group::Edit => "編集",
            Group::Tool => "道具",
            Group::View => "見る",
            Group::Display => "表示",
            Group::Other => "その他",
        }
    }
}

/// 割り当てられる操作（42個）。並びは設定の画面の並び。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    FileNew,
    FileOpen,
    FileSaveReview,
    FileLoadReview,
    FileExport,
    EditUndo,
    EditRedo,
    EditDuplicate,
    ToolSelect,
    ToolMove,
    ToolRotate,
    ToolScale,
    ToolShape,
    ToolMeasure,
    ToolAnnotate,
    ShapeCube,
    ShapeSphere,
    ShapeCylinder,
    ShapePlane,
    MeasureSnap,
    MeasureClear,
    ViewFitAll,
    ViewFitSelection,
    ViewHome,
    ViewFront,
    ViewRight,
    ViewTop,
    ViewBack,
    ViewLeft,
    ViewBottom,
    ViewProjection,
    DisplayFacesLines,
    DisplayXray,
    DisplayFaces,
    DisplayLines,
    DisplayGrid,
    DisplaySection,
    PartsHideSelected,
    PartsShowAll,
    UnitsDisplayNext,
    AppHelp,
    AppKeys,
}

/// 名前・画面の言葉・まとまり・既定（キー、Shift、Cmd）。`None` は既定なし。
type Row = (
    Action,
    &'static str,
    &'static str,
    Group,
    Option<(egui::Key, bool, bool)>,
);

const fn key(key: egui::Key) -> Option<(egui::Key, bool, bool)> {
    Some((key, false, false))
}

const fn cmd(key: egui::Key) -> Option<(egui::Key, bool, bool)> {
    Some((key, false, true))
}

const fn shift(key: egui::Key) -> Option<(egui::Key, bool, bool)> {
    Some((key, true, false))
}

const fn shift_cmd(key: egui::Key) -> Option<(egui::Key, bool, bool)> {
    Some((key, true, true))
}

use egui::Key as K;

const TABLE: [Row; 42] = [
    (
        Action::FileNew,
        "file.new",
        "新規作成",
        Group::File,
        cmd(K::N),
    ),
    (
        Action::FileOpen,
        "file.open",
        "開く",
        Group::File,
        cmd(K::O),
    ),
    (
        Action::FileSaveReview,
        "file.save_review",
        "レビュー保存",
        Group::File,
        cmd(K::S),
    ),
    (
        Action::FileLoadReview,
        "file.load_review",
        "レビュー読込",
        Group::File,
        None,
    ),
    (
        Action::FileExport,
        "file.export",
        "書き出し",
        Group::File,
        cmd(K::E),
    ),
    (
        Action::EditUndo,
        "edit.undo",
        "戻す",
        Group::Edit,
        cmd(K::Z),
    ),
    (
        Action::EditRedo,
        "edit.redo",
        "やり直す",
        Group::Edit,
        shift_cmd(K::Z),
    ),
    (
        Action::EditDuplicate,
        "edit.duplicate",
        "複製",
        Group::Edit,
        cmd(K::D),
    ),
    (
        Action::ToolSelect,
        "tool.select",
        "選択",
        Group::Tool,
        key(K::V),
    ),
    (
        Action::ToolMove,
        "tool.move",
        "移動",
        Group::Tool,
        key(K::G),
    ),
    (
        Action::ToolRotate,
        "tool.rotate",
        "回転",
        Group::Tool,
        key(K::R),
    ),
    (
        Action::ToolScale,
        "tool.scale",
        "拡大",
        Group::Tool,
        key(K::S),
    ),
    (Action::ToolShape, "tool.shape", "図形", Group::Tool, None),
    (
        Action::ToolMeasure,
        "tool.measure",
        "計測",
        Group::Tool,
        key(K::M),
    ),
    (
        Action::ToolAnnotate,
        "tool.annotate",
        "注釈",
        Group::Tool,
        key(K::N),
    ),
    (
        Action::ShapeCube,
        "shape.cube",
        "立方体を置く",
        Group::Tool,
        None,
    ),
    (
        Action::ShapeSphere,
        "shape.sphere",
        "球を置く",
        Group::Tool,
        None,
    ),
    (
        Action::ShapeCylinder,
        "shape.cylinder",
        "円柱を置く",
        Group::Tool,
        None,
    ),
    (
        Action::ShapePlane,
        "shape.plane",
        "平面を置く",
        Group::Tool,
        None,
    ),
    (
        Action::MeasureSnap,
        "measure.snap",
        "吸着",
        Group::Tool,
        key(K::X),
    ),
    (
        Action::MeasureClear,
        "measure.clear",
        "計測を消す",
        Group::Tool,
        None,
    ),
    (
        Action::ViewFitAll,
        "view.fit_all",
        "全体表示",
        Group::View,
        key(K::A),
    ),
    (
        Action::ViewFitSelection,
        "view.fit_selection",
        "選択を表示",
        Group::View,
        key(K::F),
    ),
    (
        Action::ViewHome,
        "view.home",
        "初期視点",
        Group::View,
        key(K::Home),
    ),
    (
        Action::ViewFront,
        "view.front",
        "前から見る",
        Group::View,
        key(K::Num1),
    ),
    (
        Action::ViewRight,
        "view.right",
        "右から見る",
        Group::View,
        key(K::Num3),
    ),
    (
        Action::ViewTop,
        "view.top",
        "上から見る",
        Group::View,
        key(K::Num7),
    ),
    (
        Action::ViewBack,
        "view.back",
        "後から見る",
        Group::View,
        None,
    ),
    (
        Action::ViewLeft,
        "view.left",
        "左から見る",
        Group::View,
        None,
    ),
    (
        Action::ViewBottom,
        "view.bottom",
        "下から見る",
        Group::View,
        None,
    ),
    (
        Action::ViewProjection,
        "view.projection",
        "透視と平行",
        Group::View,
        key(K::Num5),
    ),
    (
        Action::DisplayFacesLines,
        "display.faces_lines",
        "面と線",
        Group::Display,
        key(K::Z),
    ),
    (
        Action::DisplayXray,
        "display.xray",
        "透かし",
        Group::Display,
        shift(K::Z),
    ),
    (
        Action::DisplayFaces,
        "display.faces",
        "面",
        Group::Display,
        None,
    ),
    (
        Action::DisplayLines,
        "display.lines",
        "線",
        Group::Display,
        None,
    ),
    (
        Action::DisplayGrid,
        "display.grid",
        "格子",
        Group::Display,
        None,
    ),
    (
        Action::DisplaySection,
        "display.section",
        "断面",
        Group::Display,
        key(K::C),
    ),
    (
        Action::PartsHideSelected,
        "parts.hide_selected",
        "選択を非表示",
        Group::Display,
        key(K::H),
    ),
    (
        Action::PartsShowAll,
        "parts.show_all",
        "すべて表示",
        Group::Display,
        shift(K::H),
    ),
    (
        Action::UnitsDisplayNext,
        "units.display_next",
        "表示の単位",
        Group::Display,
        None,
    ),
    (
        Action::AppHelp,
        "app.help",
        "ヘルプ",
        Group::Other,
        key(K::F1),
    ),
    (
        Action::AppKeys,
        "app.keys",
        "ショートカットの設定",
        Group::Other,
        None,
    ),
];

const fn all() -> [Action; 42] {
    let mut out = [Action::FileNew; 42];
    let mut i = 0;
    while i < 42 {
        out[i] = TABLE[i].0;
        i += 1;
    }
    out
}

const ALL_ACTIONS: [Action; 42] = all();

impl Action {
    /// すべての操作。並びは設定の画面の並び（`index` と同じ順）。
    pub const ALL: &'static [Action] = &ALL_ACTIONS;

    /// 一覧の中の位置（0〜41）。
    pub fn index(self) -> usize {
        self as usize
    }

    fn row(self) -> &'static Row {
        &TABLE[self.index()]
    }

    /// ファイルに書く名前（`tool.move` など）。
    pub fn id(self) -> &'static str {
        self.row().1
    }

    pub fn from_id(id: &str) -> Option<Action> {
        TABLE.iter().find(|row| row.1 == id).map(|row| row.0)
    }

    /// 画面の言葉。
    pub fn label(self) -> &'static str {
        self.row().2
    }

    pub fn group(self) -> Group {
        self.row().3
    }

    pub fn default_chord(self) -> Option<Chord> {
        self.row().4.map(|(key, shift, command)| Chord {
            key,
            shift,
            command,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{Action, Group};
    use crate::keymap::Chord;

    #[test]
    fn table_order_matches_the_enum() {
        assert_eq!(Action::ALL.len(), 42);
        for (i, action) in Action::ALL.iter().enumerate() {
            assert_eq!(action.index(), i, "{action:?}");
        }
        for group in Group::ALL {
            assert!(!group.label().is_empty());
        }
    }

    #[test]
    fn defaults_are_unique_and_allowed() {
        let mut ids = HashSet::new();
        let mut chords = HashSet::new();
        for &action in Action::ALL {
            assert!(ids.insert(action.id()), "名前が重なる: {}", action.id());
            assert_eq!(Action::from_id(action.id()), Some(action));
            assert!(!action.label().is_empty());
            if let Some(chord) = action.default_chord() {
                assert!(chords.insert(chord), "既定が重なる: {action:?}");
                let modifiers = egui::Modifiers {
                    shift: chord.shift,
                    command: chord.command,
                    ctrl: chord.command && !cfg!(target_os = "macos"),
                    mac_cmd: chord.command && cfg!(target_os = "macos"),
                    alt: false,
                };
                assert_eq!(
                    Chord::from_event(chord.key, modifiers),
                    Ok(chord),
                    "{action:?}"
                );
                assert_eq!(Chord::parse(&chord.to_file()), Ok(chord), "{action:?}");
            }
        }
        assert_eq!(ids.len(), 42);
        assert_eq!(Action::from_id("tool.unknown"), None);
    }

    /// 第3段階の既定（足した4つと Cmd+N を含む）を書き写して固定する。
    #[test]
    fn defaults_match_stage_three() {
        let expected: [(&str, Option<&str>); 42] = [
            ("file.new", Some("Command+N")),
            ("file.open", Some("Command+O")),
            ("file.save_review", Some("Command+S")),
            ("file.load_review", None),
            ("file.export", Some("Command+E")),
            ("edit.undo", Some("Command+Z")),
            ("edit.redo", Some("Shift+Command+Z")),
            ("edit.duplicate", Some("Command+D")),
            ("tool.select", Some("V")),
            ("tool.move", Some("G")),
            ("tool.rotate", Some("R")),
            ("tool.scale", Some("S")),
            ("tool.shape", None),
            ("tool.measure", Some("M")),
            ("tool.annotate", Some("N")),
            ("shape.cube", None),
            ("shape.sphere", None),
            ("shape.cylinder", None),
            ("shape.plane", None),
            ("measure.snap", Some("X")),
            ("measure.clear", None),
            ("view.fit_all", Some("A")),
            ("view.fit_selection", Some("F")),
            ("view.home", Some("Home")),
            ("view.front", Some("1")),
            ("view.right", Some("3")),
            ("view.top", Some("7")),
            ("view.back", None),
            ("view.left", None),
            ("view.bottom", None),
            ("view.projection", Some("5")),
            ("display.faces_lines", Some("Z")),
            ("display.xray", Some("Shift+Z")),
            ("display.faces", None),
            ("display.lines", None),
            ("display.grid", None),
            ("display.section", Some("C")),
            ("parts.hide_selected", Some("H")),
            ("parts.show_all", Some("Shift+H")),
            ("units.display_next", None),
            ("app.help", Some("F1")),
            ("app.keys", None),
        ];
        for (&action, (id, chord)) in Action::ALL.iter().zip(expected) {
            assert_eq!(action.id(), id);
            assert_eq!(
                action.default_chord().map(Chord::to_file).as_deref(),
                chord,
                "{id}"
            );
        }
    }
}
