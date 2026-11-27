//! 画面からセッションへの一方通行の操作。

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use glam::Vec2;
use model_reviewer_core::scene::NodeId;

pub use model_reviewer_core::camera::{Projection, ViewDir};
pub use model_reviewer_core::gizmo::{Handle, Tool, Trs};
pub use model_reviewer_core::section::{Axis, SectionPlane};
pub use model_reviewer_core::shapes::ShapeKind;
pub use model_reviewer_core::units::UnitLabel;
pub use model_reviewer_render::DisplayMode;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    OpenModelDialog,
    OpenModel(PathBuf),
    DroppedFiles(Vec<PathBuf>),
    SaveReviewDialog,
    SaveReview(PathBuf),
    LoadReviewDialog,
    LoadReview(PathBuf),
    RelinkModelDialog,
    RelinkModel(PathBuf),
    Orbit(Vec2),
    Pan(Vec2),
    Zoom {
        factor: f32,
        cursor_ndc: Option<Vec2>,
    },
    FitAll,
    FitSelection,
    Home,
    ViewClick {
        ndc: Vec2,
    },
    Select(Option<NodeId>),
    SetVisible {
        node: NodeId,
        visible: bool,
    },
    HideSelected,
    ShowAll,
    SetAnnotateMode(bool),
    CommitDraft {
        text: String,
    },
    CancelDraft,
    EditAnnotation {
        id: u64,
        text: String,
    },
    DeleteAnnotation(u64),
    SelectAnnotation(Option<u64>),
    SaveView {
        name: String,
    },
    ApplyView(u64),
    DeleteView(u64),
    SetPanel {
        panel: PanelKind,
        open: bool,
    },
    DismissDialog,
    /// 復元をやめる。読み込み失敗の「閉じる」では復元を消さない。
    CancelRestore,
    ConfirmDiscard,
    CancelPending,
    /// 下書き欄に未登録の文字があるか。同じ値の再送は状態を変えない。
    SetDraftPending(bool),
    /// 注釈本文が未確定か。同じ値の再送は状態を変えない。
    SetBodyPending(bool),

    // ---- 第2段階：見る（session/view.rs） ----
    /// 方位と仰角だけを決まった向きにする（立方体・数字キー）。
    SnapView(ViewDir),
    SetDisplay(DisplayMode),
    SetProjection(Projection),
    ToggleProjection,
    /// 床の格子と軸を出すか。
    SetGrid(bool),

    // ---- 第2段階：道具と編集（session/edit.rs） ----
    /// 道具を選ぶだけ（G・R・S もすぐ掴む動きにはしない）。`SetAnnotateMode` は
    /// `Annotate` と `Select` の切り替えとして残る。
    SetTool(Tool),
    /// ハンドルを掴む。`ndc` はポインタの正規化座標（中心が 0、上が +y）。
    BeginTransform {
        handle: Handle,
        ndc: Vec2,
    },
    /// 掴んだまま動かす。文書は書き換えず、差分の行列だけを見せる。
    DragTransform {
        ndc: Vec2,
    },
    /// 離した。ここで初めて文書を書き換え、履歴に1回分を積む。
    CommitTransform,
    /// 変形（ハンドルまたは数値の欄）をやめる。文書も履歴も変えない。
    CancelTransform,
    /// 数値の欄をドラッグ中。選択中の部品の新しい `local` の見た目だけを出す。
    PreviewLocal(Trs),
    /// 数値の欄を確定した。
    CommitLocal(Trs),
    Undo,
    Redo,
    /// 視点の注視点に図形を置き、選択して移動の道具にする。
    AddShape(ShapeKind),
    DuplicateSelected,
    DeleteSelected,

    // ---- 第2段階：書き出し（session/export.rs） ----
    /// 保存先のダイアログを出す（画面側が処理し、`apply` は何もしない）。
    ExportGlbDialog,
    ExportGlb(PathBuf),

    // ---- 第2段階：計測（session/measure.rs） ----
    ClearMeasure,
    /// ファイルの単位（前提）を替える。GLB だけ。数値は換算しない。
    SetUnit(UnitLabel),

    // ---- 第3段階：寸法を確かめる（session/{snap,section}.rs ほか）。どれも履歴と未保存の印に入らない ----
    /// 計測の道具でポインタが3Dビューの上を動いた（`Some`）か、3Dビューを出た（`None`）。
    ViewHover {
        ndc: Option<Vec2>,
    },
    /// 吸着を入れるか。
    SetSnap(bool),
    ToggleSnap,
    /// 断面を置く（`None` で外す）。置く前に境界箱へ収める。
    SetSection(Option<SectionPlane>),
    /// 断面の入・切。切ってから入れると、前の軸と位置を使う。
    ToggleSection,
    /// 表示の単位。`None` は「ファイルと同じ」。
    SetDisplayUnit(Option<UnitLabel>),
    /// STL の単位の仮定を替える（同じファイルを開き直す）。
    SetStlUnit(UnitLabel),

    // ---- 第5段階：新規作成（session/open.rs） ----
    /// 部品が0個の空の文書を開く。履歴と未保存の印には入らない（文書ごと入れ替える）。
    /// 未保存の変更があれば、別のモデルを開くときと同じ確認を通す。
    NewDocument,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelKind {
    Parts,
    Notes,
    Details,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneDirty {
    pub camera: bool,
    pub instances: bool,
    pub selection: bool,
    pub model: bool,
    pub size: bool,
    /// 表示・格子・投影の切り替え。
    pub display: bool,
    /// 変形中の差分の行列が変わった（インスタンスは書き直さない）。
    pub drag: bool,
    /// 断面が変わった（定数だけを送り直す。部品の表もインスタンスも作り直さない）。
    pub section: bool,
}

impl SceneDirty {
    pub fn any(self) -> bool {
        self.camera
            || self.instances
            || self.selection
            || self.model
            || self.size
            || self.display
            || self.drag
            || self.section
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DialogButton {
    pub id: String,
    pub label: String,
    pub command: Command,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DialogRequest {
    pub title: String,
    pub body: String,
    pub buttons: Vec<DialogButton>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchArgs {
    pub positional: Option<PathBuf>,
    pub script: Option<PathBuf>,
    pub out: Option<PathBuf>,
    pub log_metrics: bool,
}

pub fn is_review_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
}

pub fn usage() -> &'static str {
    "使い方: model-reviewer [GLBまたはレビューJSON] [--script 手順.json] [--out 出力フォルダ] [--log-metrics]"
}

pub fn parse_args<I, S>(args: I) -> Result<LaunchArgs, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut iter = args.into_iter();
    if iter.next().is_none() {
        return Err(usage().to_owned());
    }
    let mut launch = LaunchArgs {
        positional: None,
        script: None,
        out: None,
        log_metrics: false,
    };
    while let Some(arg) = iter.next() {
        let os = arg.as_ref();
        let text = os.to_string_lossy();
        if text == "--script" {
            launch.script = Some(next_path(&mut iter, "--script")?);
        } else if text == "--out" {
            launch.out = Some(next_path(&mut iter, "--out")?);
        } else if text == "--log-metrics" {
            launch.log_metrics = true;
        } else if text.starts_with('-') {
            return Err(format!("不明な引数です: {text}\n{}", usage()));
        } else if launch.positional.is_some() {
            return Err(format!("位置引数は1つだけです\n{}", usage()));
        } else {
            launch.positional = Some(PathBuf::from(os));
        }
    }
    Ok(launch)
}

fn next_path<I, S>(iter: &mut I, flag: &str) -> Result<PathBuf, String>
where
    I: Iterator<Item = S>,
    S: AsRef<OsStr>,
{
    match iter.next() {
        Some(value) => {
            let text = value.as_ref().to_string_lossy();
            if text.starts_with('-') {
                Err(format!("{flag} の後にパスが必要です\n{}", usage()))
            } else {
                Ok(PathBuf::from(value.as_ref()))
            }
        }
        None => Err(format!("{flag} の後にパスが必要です\n{}", usage())),
    }
}

#[cfg(test)]
mod tests {
    use super::{is_review_path, parse_args};
    use std::path::Path;

    #[test]
    fn flags_can_come_in_any_order() {
        let args = parse_args([
            "model-reviewer",
            "--log-metrics",
            "--out",
            "out",
            "notes.JSON",
            "--script",
            "steps.json",
        ])
        .unwrap();
        assert!(is_review_path(args.positional.as_deref().unwrap()));
        assert_eq!(args.script.unwrap(), Path::new("steps.json"));
        assert_eq!(args.out.unwrap(), Path::new("out"));
        assert!(args.log_metrics);
        assert!(is_review_path(Path::new("A.JSON")));
        assert!(!is_review_path(Path::new("model.glb")));
    }

    #[test]
    fn rejects_unknown_flags_and_a_second_path() {
        let unknown = parse_args(["model-reviewer", "--script=steps.json"]).unwrap_err();
        assert!(unknown.contains("不明"));
        assert!(unknown.contains("使い方"));
        let extra = parse_args(["model-reviewer", "a.glb", "b.glb"]).unwrap_err();
        assert!(extra.contains("1つ"));
        let missing = parse_args(["model-reviewer", "--script"]).unwrap_err();
        assert!(missing.contains("--script"));
    }
}
