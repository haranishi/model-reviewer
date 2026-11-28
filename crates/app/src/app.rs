//! eframe の入口。`logic` で読み込みと終了、`ui` で5領域とコマンドの適用。

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;

use crate::automation::Automation;
use crate::command::{Axis, Command, DisplayMode, KeyInput, LaunchArgs, ShapeKind, Tool, ViewDir};
use crate::fonts;
use crate::gpu_view::GpuView;
use crate::keymap::{Action, Chord, Refusal};
use crate::metrics::{self, Counts, FrameClock};
use crate::session::{Session, tool_has_gizmo};
use crate::theme;
use crate::ui;

pub struct App {
    session: Session,
    launch: LaunchArgs,
    gpu: GpuView,
    automation: Automation,
    frames: FrameClock,
    /// 既定の終了メニューを付け替えたか。未作成なら最初の `logic` で1回だけ試す。
    #[cfg(target_os = "macos")]
    quit_menu_done: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, launch: LaunchArgs) -> Self {
        fonts::install(&cc.egui_ctx);
        theme::install(&cc.egui_ctx);
        let mut session = Session::from_launch(&launch);
        session.set_egui_ctx(&cc.egui_ctx);
        let gpu = GpuView::new(cc);
        let max_side = gpu.max_texture_side();
        session.configure_textures(&launch, max_side);
        if max_side.is_some() {
            session.enable_gpu_upload();
        }
        session.set_lines_supported(gpu.supports_lines());
        if let Some(path) = launch.positional.clone() {
            session.open_launch(path);
        }
        let automation = Automation::from_launch(&launch);
        Self {
            session,
            launch,
            gpu,
            automation,
            frames: FrameClock::default(),
            #[cfg(target_os = "macos")]
            quit_menu_done: crate::macos::retarget_quit_menu(false),
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    fn apply_ui_command(&mut self, command: Command) {
        let automated = self.automation.running() || self.automation.forcing_quit();
        let command = match resolve_dialog(&self.session, &command, automated) {
            Ok(Some(dialog)) => dialog.pick(&self.session),
            Ok(None) => Some(command),
            Err(reason) => {
                self.automation.fail(&self.session, reason.to_owned());
                return;
            }
        };
        if let Some(command) = command {
            self.session.apply(command);
        }
    }

    /// 描いた回数と送ったバイト数（試験用）。GPUが無ければ `None`。
    pub fn gpu_stats(&self) -> Option<model_reviewer_render::RenderStats> {
        self.gpu.stats()
    }

    /// 3Dビューに出している理由（GPUが使えない、送れなかったなど）。試験用。
    pub fn gpu_notice(&self) -> Option<&str> {
        self.gpu.unavailable_reason()
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        if !self.quit_menu_done {
            crate::macos::retarget_quit_menu(true);
            self.quit_menu_done = true;
        }
        #[cfg(target_os = "macos")]
        if self.launch.script.is_some() && _frame.winit_window().is_some() {
            crate::macos::deactivate_if_active();
        }
        self.session.set_egui_ctx(ctx);
        let loader_news = self.session.poll_loader();
        let export_news = self.session.poll_exporter();
        self.gpu.poll(&mut self.session);
        let gpu_busy = self.gpu.needs_repaint();
        self.automation.on_logic(ctx, &self.session, gpu_busy);
        if loader_news || export_news || gpu_busy || self.automation.needs_repaint() {
            ctx.request_repaint();
        }
        if self.automation.forcing_quit() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            let close_requested = ctx.input(|input| input.viewport().close_requested());
            if close_requested && self.session.on_close_requested() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
            if self.session.take_quit() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let started = Instant::now();
        self.session.metrics.physical_mem = metrics::sample_memory();
        self.session
            .observe_texture_memory(self.session.metrics.physical_mem);
        self.session
            .observe_export_memory(self.session.metrics.physical_mem);
        self.session.set_egui_ctx(ui.ctx());
        self.session.clear_keys_for_dialog();
        self.session.pixels_per_point = ui.pixels_per_point();
        let mut commands = Vec::new();
        let pixels = ui::show(ui, &self.session, &mut commands);
        let mut shortcuts = FrameShortcuts::read(ui.ctx(), &self.session);
        for command in commands {
            self.apply_ui_command(command);
        }
        // 速く打った G・X・数字が同じフレームでも、直前のキーで変わった道具と変形を見る。
        while let Some(command) = shortcuts.next_command(ui.ctx(), &self.session) {
            self.apply_ui_command(command);
        }
        let dropped = dropped_paths(ui);
        if !dropped.is_empty() {
            self.session.apply(Command::DroppedFiles(dropped));
        }
        self.session.set_viewport(pixels);
        self.gpu.frame(ui.ctx(), &mut self.session);
        let ctx = ui.ctx().clone();
        self.automation.observe_gpu(self.gpu.stats());
        self.automation
            .on_ui(&ctx, &mut self.session, self.gpu.needs_repaint());
        self.frames
            .observe(started, started.elapsed(), &mut self.session.metrics);
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.automation.inject(ctx, raw_input);
    }

    fn on_exit(&mut self) {
        self.automation.on_exit(&self.session);
        if self.launch.log_metrics {
            self.session.metrics.physical_mem = metrics::sample_memory();
            metrics::log_stderr(&self.session.metrics, model_counts(&self.session));
        }
    }
}

fn model_counts(session: &Session) -> Option<Counts> {
    session
        .model
        .as_ref()
        .map(|model| Counts::from_stats(model.stats))
}

/// 新規の文書を書き出すときの、保存先の既定の名前。
pub const NEW_FILE_NAME: &str = "無題.glb";

const TEXT_ESCAPE_FRAME_ID: &str = "app.text_escape_frame";
const TEXT_INPUT_FRAME_ID: &str = "app.text_input_frame";

pub(crate) fn mark_text_escape(ctx: &egui::Context) {
    mark_frame(ctx, TEXT_ESCAPE_FRAME_ID);
}

/// Enter で入力欄がフォーカスを手放しても、そのフレームのキーは入力欄のもの。
pub(crate) fn remember_text_focus(ctx: &egui::Context) {
    if ctx.text_edit_focused() {
        mark_frame(ctx, TEXT_INPUT_FRAME_ID);
    }
}

fn mark_frame(ctx: &egui::Context, id: &str) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|data| data.insert_temp(egui::Id::unique(id), frame));
}

fn marked_this_frame(ctx: &egui::Context, id: &str) -> bool {
    let frame = ctx.cumulative_frame_nr();
    ctx.data(|data| data.get_temp::<u64>(egui::Id::unique(id))) == Some(frame)
}

pub(crate) fn help_open(ctx: &egui::Context) -> bool {
    ctx.data(|data| {
        data.get_temp::<bool>(egui::Id::unique(ui::ids::HELP_OPEN))
            .unwrap_or(false)
    })
}

/// 現在の状態で、このフレームのキーに対応する操作を返す。
pub fn shortcut_commands(ctx: &egui::Context, session: &Session) -> Vec<Command> {
    let mut frame = FrameShortcuts::read(ctx, session);
    std::iter::from_fn(|| frame.next_command(ctx, session)).collect()
}

/// 小窓・Esc・入力欄はフレームを占有する。それ以外のキーは届いた順に振り分ける。
enum FrameShortcuts {
    Exclusive(Option<Command>),
    Keys {
        presses: std::vec::IntoIter<(egui::Key, egui::Modifiers)>,
        deleted: bool,
    },
}

impl FrameShortcuts {
    fn read(ctx: &egui::Context, session: &Session) -> Self {
        let presses = ctx.input(|i| i.events.iter().filter_map(pressed_key).collect::<Vec<_>>());
        let escape = presses.iter().any(|&(key, _)| key == egui::Key::Escape);
        if let Some(dialog) = &session.dialog {
            return Self::Exclusive(if escape {
                dialog_escape_command(dialog)
            } else {
                None
            });
        }
        if help_open(ctx) {
            if escape {
                ctx.data_mut(|data| data.insert_temp(egui::Id::unique(ui::ids::HELP_OPEN), false));
            }
            return Self::Exclusive(None);
        }
        if session.keys.open {
            return Self::Exclusive(settings_key(ctx, session, escape));
        }
        if escape {
            if marked_this_frame(ctx, TEXT_ESCAPE_FRAME_ID) {
                return Self::Exclusive(None);
            }
            if ctx.text_edit_focused() {
                ui::surrender_focus(ctx);
                return Self::Exclusive(session.draft.is_some().then_some(Command::CancelDraft));
            }
            return Self::Exclusive(escape_command(session));
        }
        if ctx.text_edit_focused() || marked_this_frame(ctx, TEXT_INPUT_FRAME_ID) {
            return Self::Exclusive(None);
        }
        Self::Keys {
            presses: presses.into_iter(),
            deleted: false,
        }
    }

    fn next_command(&mut self, ctx: &egui::Context, session: &Session) -> Option<Command> {
        let (presses, deleted) = match self {
            Self::Exclusive(command) => return command.take(),
            Self::Keys { presses, deleted } => (presses, deleted),
        };
        for (key, modifiers) in presses {
            if session.dialog.is_some() || session.keys.open || help_open(ctx) {
                break;
            }
            if let Some(command) = pressed_command(ctx, session, key, modifiers) {
                if command == Command::DeleteSelected && std::mem::replace(deleted, true) {
                    continue;
                }
                return Some(command);
            }
        }
        None
    }
}

fn settings_key(ctx: &egui::Context, session: &Session, escape: bool) -> Option<Command> {
    if session.keys.capture.is_some() {
        return ctx.input(|input| {
            input.raw.events.iter().find_map(|event| match event {
                egui::Event::Copy
                | egui::Event::Cut
                | egui::Event::Paste(_)
                | egui::Event::PasteImage(_) => {
                    Some(Command::CaptureKey(Err(Refusal::TextEditing)))
                }
                _ => pressed_key(event).map(|(key, mods)| {
                    if key == egui::Key::Escape {
                        Command::CancelKeyCapture
                    } else {
                        Command::CaptureKey(Chord::from_event(key, mods))
                    }
                }),
            })
        });
    }
    if !escape {
        return None;
    }
    if session.keys.conflict.is_some() {
        return Some(Command::ResolveKeyConflict { replace: false });
    }
    let filter = egui::Id::unique(ui::ids::KEYS_FILTER);
    // egui は Esc のフレームの先頭で入力位置を外す。検索中の1回目は欄を離れるだけ。
    if ctx.memory(|m| m.has_focus(filter) || m.had_focus_last_frame(filter)) {
        ctx.memory_mut(|m| m.surrender_focus(filter));
        return None;
    }
    Some(Command::SetKeysOpen(false))
}

fn pressed_command(
    ctx: &egui::Context,
    session: &Session,
    key: egui::Key,
    mods: egui::Modifiers,
) -> Option<Command> {
    use egui::Key;
    let arrow = match key {
        Key::ArrowLeft => Some((Axis::X, -1)),
        Key::ArrowRight => Some((Axis::X, 1)),
        Key::ArrowDown => Some((Axis::Y, -1)),
        Key::ArrowUp => Some((Axis::Y, 1)),
        _ => None,
    };
    if (arrow.is_some() || matches!(key, Key::Enter | Key::Backspace | Key::Delete))
        && ctx.memory(|m| m.focused().is_some())
    {
        return None;
    }

    let entering = session.key_entry().is_some();
    let editing = tool_has_gizmo(session.tool) && session.selection.is_some();
    if editing && mods.is_none() {
        let axis = match key {
            Key::X => Some(Axis::X),
            Key::Y => Some(Axis::Y),
            Key::Z => Some(Axis::Z),
            _ => None,
        };
        if let Some(axis) = axis {
            if session.edits_locked() || (session.transform.is_some() && !entering) {
                return None;
            }
            ui::surrender_focus(ctx);
            return Some(Command::BeginKeyTransform(axis));
        }
    }
    if let Some((axis, sign)) = arrow {
        if !(mods.is_none() || mods.shift_only()) || session.edits_locked() {
            return None;
        }
        let fine = mods.shift;
        if entering {
            return Some(Command::KeyTransformInput(KeyInput::Step {
                up: sign > 0,
                fine,
            }));
        }
        return (editing && session.tool == Tool::Move && session.transform.is_none())
            .then_some(Command::Nudge { axis, sign, fine });
    }
    if entering {
        if mods.any() {
            return None;
        }
        let input = match key {
            Key::Enter => return Some(Command::CommitTransform),
            Key::Backspace => KeyInput::Backspace,
            Key::Minus => KeyInput::Char('-'),
            Key::Period => KeyInput::Char('.'),
            _ => match key.name().as_bytes() {
                [digit @ b'0'..=b'9'] => KeyInput::Char(char::from(*digit)),
                _ => return None,
            },
        };
        return Some(Command::KeyTransformInput(input));
    }
    if matches!(key, Key::Delete | Key::Backspace) {
        return (mods.is_none() && session.selection.is_some()).then_some(Command::DeleteSelected);
    }
    let chord = Chord::from_event(key, mods).ok()?;
    action_command(ctx, session, session.keymap.action_for(chord)?)
}

/// 操作を、同じ操作のボタンと同じ `Command` にする。押せない状態なら `None`。
/// 第3段階からあるキーの条件は、そのときの分岐の条件を写している（既定のままなら働きが同じ）。
/// 足した操作の条件は、ボタンの側の条件と同じ。
fn action_command(
    ctx: &egui::Context,
    session: &crate::session::Session,
    action: Action,
) -> Option<Command> {
    let has_model = session.model.is_some();
    let exporting = session.is_exporting();
    let can_edit = has_model && session.edit_block_reason().is_none();
    // 道具の切り替え。変形の途中（数値の欄をドラッグしているときも含む）は断る。
    let tool = |tool: Tool| {
        (session.tool != tool && session.transform.is_none()).then_some(Command::SetTool(tool))
    };
    let shape = |kind: ShapeKind| can_edit.then_some(Command::AddShape(kind));
    match action {
        Action::FileNew => Some(Command::NewDocument),
        Action::FileOpen => (!exporting).then_some(Command::OpenModelDialog),
        Action::FileSaveReview => (has_model && !exporting).then_some(Command::SaveReviewDialog),
        Action::FileLoadReview => (!exporting).then_some(Command::LoadReviewDialog),
        Action::FileExport => {
            (has_model && !session.edits_locked()).then_some(Command::ExportGlbDialog)
        }
        Action::EditUndo => Some(Command::Undo),
        Action::EditRedo => Some(Command::Redo),
        Action::EditDuplicate => session
            .selection
            .is_some()
            .then_some(Command::DuplicateSelected),
        Action::TexturePaste => Some(Command::PasteTextureDialog),
        Action::TextureRemove => Some(Command::RemoveTexture),
        Action::ToolSelect => tool(Tool::Select),
        Action::ToolMove => tool(Tool::Move),
        Action::ToolRotate => tool(Tool::Rotate),
        Action::ToolScale => tool(Tool::Scale),
        Action::ToolMeasure => tool(Tool::Measure),
        Action::ToolShape => tool(Tool::Shape).filter(|_| can_edit),
        Action::ToolAnnotate => session
            .transform
            .is_none()
            .then_some(Command::SetAnnotateMode(!session.annotate_mode)),
        Action::ShapeCube => shape(ShapeKind::Cube),
        Action::ShapeSphere => shape(ShapeKind::Sphere),
        Action::ShapeCylinder => shape(ShapeKind::Cylinder),
        Action::ShapePlane => shape(ShapeKind::Plane),
        Action::MeasureSnap => Some(Command::ToggleSnap),
        Action::MeasureClear => session
            .measurement
            .first
            .is_some()
            .then_some(Command::ClearMeasure),
        Action::ViewFitAll => Some(Command::FitAll),
        Action::ViewFitSelection => Some(Command::FitSelection),
        Action::ViewHome => Some(Command::Home),
        Action::ViewFront => Some(Command::SnapView(ViewDir::Front)),
        Action::ViewRight => Some(Command::SnapView(ViewDir::Right)),
        Action::ViewTop => Some(Command::SnapView(ViewDir::Top)),
        Action::ViewBack => Some(Command::SnapView(ViewDir::Back)),
        Action::ViewLeft => Some(Command::SnapView(ViewDir::Left)),
        Action::ViewBottom => Some(Command::SnapView(ViewDir::Bottom)),
        Action::ViewProjection => Some(Command::ToggleProjection),
        Action::DisplayFacesLines => {
            Some(Command::SetDisplay(toggled_display(session.display, false)))
        }
        Action::DisplayXray => Some(Command::SetDisplay(toggled_display(session.display, true))),
        Action::DisplayFaces => (session.display != DisplayMode::Faces)
            .then_some(Command::SetDisplay(DisplayMode::Faces)),
        Action::DisplayLines => (session.lines_supported && session.display != DisplayMode::Lines)
            .then_some(Command::SetDisplay(DisplayMode::Lines)),
        Action::DisplayGrid => Some(Command::SetGrid(!session.grid_visible)),
        Action::DisplayTextures => session
            .texture_options
            .enabled
            .then_some(Command::SetTexturesShown(!session.textures_shown)),
        // 断面はモデルがあるときだけ（第3段階の分岐と同じ）。
        Action::DisplaySection => has_model.then_some(Command::ToggleSection),
        Action::PartsHideSelected => Some(Command::HideSelected),
        Action::PartsShowAll => Some(Command::ShowAll),
        Action::UnitsDisplayNext => Some(Command::SetDisplayUnit(session.units.next_display())),
        Action::AppHelp => {
            // ヘルプは egui の一時の記憶で開く（ここへ来るのは閉じているときだけ）。
            ctx.data_mut(|data| data.insert_temp(egui::Id::unique(ui::ids::HELP_OPEN), true));
            None
        }
        Action::AppKeys => Some(Command::SetKeysOpen(true)),
    }
}

/// Z は面と「面と線」、Shift+Z は透かしの切り替え。どちらも、外すときは面へ戻す。
fn toggled_display(now: DisplayMode, xray: bool) -> DisplayMode {
    match (xray, now) {
        (true, DisplayMode::XRay) | (false, DisplayMode::FacesAndLines) => DisplayMode::Faces,
        (true, _) => DisplayMode::XRay,
        (false, _) => DisplayMode::FacesAndLines,
    }
}

fn dialog_escape_command(dialog: &crate::command::DialogRequest) -> Option<Command> {
    dialog
        .buttons
        .iter()
        .find(|button| {
            matches!(
                button.command,
                Command::DismissDialog | Command::CancelPending | Command::CancelRestore
            )
        })
        .map(|button| button.command.clone())
}

fn escape_command(session: &crate::session::Session) -> Option<Command> {
    if session.transform.is_some() {
        Some(Command::CancelTransform)
    } else if session.draft.is_some() {
        Some(Command::CancelDraft)
    } else if session.annotate_mode {
        Some(Command::SetAnnotateMode(false))
    } else if session.tool == Tool::Measure && session.measurement.first.is_some() {
        // 計測の道具では、先に計測を消し、次の Esc で選択の道具へ戻る。
        Some(Command::ClearMeasure)
    } else if session.tool != Tool::Select {
        Some(Command::SetTool(Tool::Select))
    } else if session.selection.is_some() {
        // 部品の選択を解くと、注釈の選択も解ける（選択は1つ）。
        Some(Command::Select(None))
    } else {
        None
    }
}

/// 割り当ての取り込みも変形も、1回の押下につき1つだけ受け取る。
fn pressed_key(event: &egui::Event) -> Option<(egui::Key, egui::Modifiers)> {
    match event {
        egui::Event::Key {
            key,
            modifiers,
            pressed: true,
            repeat: false,
            ..
        } => Some((*key, *modifiers)),
        _ => None,
    }
}

fn dropped_paths(ui: &egui::Ui) -> Vec<PathBuf> {
    ui.input(|input| {
        input
            .raw
            .dropped_files
            .iter()
            .map(|file| file.path().to_path_buf())
            .collect()
    })
}

#[derive(Debug, PartialEq, Eq)]
enum FileDialog {
    OpenModel,
    SaveReview,
    LoadReview,
    RelinkModel,
    ExportGlb,
    PasteTexture,
}

/// 選択画面が不要な操作と、Session が自分で断る操作は、そのまま Session に渡す。
fn resolve_dialog(
    session: &Session,
    command: &Command,
    automated: bool,
) -> Result<Option<FileDialog>, &'static str> {
    if session.is_exporting() {
        return Ok(None);
    }
    let dialog = match command {
        Command::OpenModelDialog => FileDialog::OpenModel,
        Command::PasteTextureDialog if crate::session::texture::paste_block(session).is_none() => {
            FileDialog::PasteTexture
        }
        // 新規の文書は書き出すまで保存できず、読み込みの間は前のレビューで置き換えないように、
        // どちらも保存先を聞かずに Session が断る。
        Command::SaveReviewDialog
            if !session.doc_unsaved() && !session.is_new_document() && !session.edits_locked() =>
        {
            FileDialog::SaveReview
        }
        Command::LoadReviewDialog => FileDialog::LoadReview,
        Command::RelinkModelDialog => FileDialog::RelinkModel,
        Command::ExportGlbDialog
            if session.model.is_some()
                && !session.edits_locked()
                && !crate::session::nothing_to_export(session) =>
        {
            FileDialog::ExportGlb
        }
        _ => return Ok(None),
    };
    if automated {
        Err(
            "自動操作ではファイル選択の画面を開きません。open_model・paste_texture・export_glb などでパスを指定してください",
        )
    } else {
        Ok(Some(dialog))
    }
}

impl FileDialog {
    fn pick(self, session: &Session) -> Option<Command> {
        let dialog = rfd::FileDialog::new();
        match self {
            Self::PasteTexture => dialog
                .add_filter("PNG・JPEG", &["png", "jpg", "jpeg", "PNG", "JPG", "JPEG"])
                .pick_file()
                .map(Command::PasteTexture),
            Self::OpenModel => dialog
                .add_filter("GLB・STL", &["glb", "stl", "STL"])
                .add_filter("GLB", &["glb"])
                .add_filter("STL", &["stl", "STL"])
                .pick_file()
                .map(Command::OpenModel),
            Self::SaveReview => dialog
                .add_filter("JSON", &["json"])
                .save_file()
                .map(Command::SaveReview),
            Self::LoadReview => dialog
                .add_filter("JSON", &["json"])
                .pick_file()
                .map(Command::LoadReview),
            Self::RelinkModel => dialog
                .add_filter("GLB・STL", &["glb", "stl", "STL"])
                .pick_file()
                .map(Command::RelinkModel),
            Self::ExportGlb => {
                let mut dialog = dialog.add_filter("GLB", &["glb"]);
                if let Some(name) = export_file_name(session) {
                    dialog = dialog.set_file_name(name);
                }
                dialog.save_file().map(Command::ExportGlb)
            }
        }
    }
}

/// 書き出しの保存先の既定の名前。STL を開いているときは「<元の名前>.glb」、新規の文書は
/// 「無題.glb」（要件2.4）。GLB は既定の名前を付けない（開いたファイルと同じ名前になり、そこへは
/// 書き出せないため）。
pub fn export_file_name(session: &Session) -> Option<String> {
    if session.is_new_document() {
        return Some(NEW_FILE_NAME.to_owned());
    }
    if !matches!(session.source_kind, crate::session::SourceKind::Stl { .. }) {
        return None;
    }
    let path = &session.model_ref.as_ref()?.path;
    Some(format!("{}.glb", crate::loader::stl_part_name(path)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::LoadPhase;
    use model_reviewer_core::scene::Model;

    const FILE_DIALOG_COMMANDS: [(Command, FileDialog); 6] = [
        (Command::OpenModelDialog, FileDialog::OpenModel),
        (Command::SaveReviewDialog, FileDialog::SaveReview),
        (Command::LoadReviewDialog, FileDialog::LoadReview),
        (Command::RelinkModelDialog, FileDialog::RelinkModel),
        (Command::ExportGlbDialog, FileDialog::ExportGlb),
        (Command::PasteTextureDialog, FileDialog::PasteTexture),
    ];

    #[test]
    fn scripts_refuse_every_native_file_dialog_but_accept_paths() {
        let mut session = Session::default();
        session.set_model(
            model_reviewer_gltf::load_glb(&model_reviewer_testdata::sample_glb())
                .unwrap()
                .into(),
        );
        session.apply(Command::Select(Some(model_reviewer_core::scene::NodeId(0))));
        for (command, dialog) in FILE_DIALOG_COMMANDS {
            assert_eq!(resolve_dialog(&session, &command, false), Ok(Some(dialog)));
            assert_eq!(
                resolve_dialog(&session, &command, true),
                Err(
                    "自動操作ではファイル選択の画面を開きません。open_model・paste_texture・export_glb などでパスを指定してください"
                )
            );
        }
        for command in [
            Command::OpenModel("model.glb".into()),
            Command::SaveReview("review.json".into()),
            Command::LoadReview("review.json".into()),
            Command::RelinkModel("model.stl".into()),
            Command::ExportGlb("export.glb".into()),
            Command::PasteTexture("image.png".into()),
        ] {
            assert_eq!(resolve_dialog(&session, &command, true), Ok(None));
        }
    }

    #[test]
    fn unavailable_save_and_export_stay_with_the_session() {
        let mut session = Session::default();
        let export = Command::ExportGlbDialog;
        let save = Command::SaveReviewDialog;
        assert_eq!(resolve_dialog(&session, &export, true), Ok(None));

        session.apply(Command::NewDocument);
        assert_eq!(resolve_dialog(&session, &save, true), Ok(None));
        assert_eq!(resolve_dialog(&session, &export, true), Ok(None));

        session.apply(Command::AddShape(ShapeKind::Cube));
        session.source_kind = crate::session::SourceKind::Glb;
        assert!(session.doc_unsaved());
        assert_eq!(resolve_dialog(&session, &save, true), Ok(None));
        session.load = LoadPhase::Reading;
        assert_eq!(resolve_dialog(&session, &export, true), Ok(None));
    }

    #[test]
    fn saving_during_a_replacement_load_stays_with_the_session() {
        let mut session = Session::default();
        session.set_model(Model::empty().into());
        session.load = LoadPhase::Reading;
        assert!(session.edits_locked() && !session.doc_unsaved());
        for automated in [false, true] {
            assert_eq!(
                resolve_dialog(&session, &Command::SaveReviewDialog, automated),
                Ok(None)
            );
        }
    }

    #[test]
    fn exporting_leaves_all_file_dialog_commands_to_the_session() {
        let mut session = Session::default();
        session.apply(Command::NewDocument);
        session.apply(Command::AddShape(ShapeKind::Cube));
        let out =
            std::env::temp_dir().join(format!("model-reviewer-dialog-{}.glb", std::process::id()));
        session.apply(Command::ExportGlb(out.clone()));
        assert!(session.is_exporting());
        for (command, _) in FILE_DIALOG_COMMANDS {
            assert_eq!(resolve_dialog(&session, &command, true), Ok(None));
            assert_eq!(resolve_dialog(&session, &command, false), Ok(None));
        }
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while session.is_exporting() {
            session.poll_exporter();
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        std::fs::remove_file(out).unwrap();
    }
}
