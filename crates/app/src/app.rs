//! eframe の入口。`logic` で読み込みと終了、`ui` で5領域とコマンドの適用。

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;

use crate::automation::Automation;
use crate::command::{Command, DisplayMode, LaunchArgs, ShapeKind, Tool, ViewDir};
use crate::fonts;
use crate::gpu_view::GpuView;
use crate::keymap::{Action, Chord};
use crate::metrics::{self, Counts, FrameClock};
use crate::session::Session;
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
        let mut session = Session::default();
        session.set_egui_ctx(&cc.egui_ctx);
        let gpu = GpuView::new(cc);
        if gpu.has_device() {
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
            .observe_export_memory(self.session.metrics.physical_mem);
        self.session.set_egui_ctx(ui.ctx());
        self.session.pixels_per_point = ui.pixels_per_point();
        let mut commands = Vec::new();
        let pixels = ui::show(ui, &self.session, &mut commands);
        commands.extend(shortcut_commands(ui.ctx(), &self.session));
        let dropped = dropped_paths(ui);
        if !dropped.is_empty() {
            commands.push(Command::DroppedFiles(dropped));
        }
        let resolved: Vec<Command> = commands
            .into_iter()
            .filter_map(|command| resolve_dialog(&self.session, command))
            .collect();
        for command in resolved {
            self.session.apply(command);
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

pub(crate) fn mark_text_escape(ctx: &egui::Context) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::unique(TEXT_ESCAPE_FRAME_ID), frame);
    });
}

fn text_escape_consumed(ctx: &egui::Context) -> bool {
    let frame = ctx.cumulative_frame_nr();
    ctx.data(|data| data.get_temp::<u64>(egui::Id::unique(TEXT_ESCAPE_FRAME_ID))) == Some(frame)
}

pub(crate) fn help_open(ctx: &egui::Context) -> bool {
    ctx.data(|data| {
        data.get_temp::<bool>(egui::Id::unique(ui::ids::HELP_OPEN))
            .unwrap_or(false)
    })
}

/// キーの出来事を読むのはここだけ。順は「ダイアログ → ヘルプ → 設定の画面 → Esc → 入力中なら
/// 終わり → 固定のキー（Delete・Backspace）→ 表」（design-keymap.md）。
///
/// Esc は、ダイアログ中は閉じる・やめるだけ。入力欄に文字を打っている間は、下書きの取り消しか
/// 入力欄を離れるだけ。それ以外は、変形の取り消し、下書き、選択以外の道具から選択への復帰、
/// 選択の解除の順で1回に1つ（要件5.5）。
///
/// 表のキーは、ポインタの場所（3Dビュー・一覧・ボタン・パネル）によらず働く（要件2.5）。入力欄で
/// ない部品が入力の位置を持っている間だけ、Delete・Backspace・矢印・Enter を捨てる。1フレームに
/// 複数のキーが届いたら、届いた順に積む。
pub fn shortcut_commands(ctx: &egui::Context, session: &crate::session::Session) -> Vec<Command> {
    if let Some(dialog) = &session.dialog {
        if key_press(ctx, egui::Key::Escape).is_some() {
            return dialog_escape_command(dialog).into_iter().collect();
        }
        return Vec::new();
    }
    if help_open(ctx) {
        if key_press(ctx, egui::Key::Escape).is_some() {
            ctx.data_mut(|data| data.insert_temp(egui::Id::unique(ui::ids::HELP_OPEN), false));
        }
        return Vec::new();
    }
    if session.keys.open {
        // 取り込みと検索の欄の Esc は WP-J1 が足す。割り当てた操作は働かせない。
        if key_press(ctx, egui::Key::Escape).is_some() {
            return vec![Command::SetKeysOpen(false)];
        }
        return Vec::new();
    }
    let typing = ctx.text_edit_focused();
    if key_press(ctx, egui::Key::Escape).is_some() {
        if text_escape_consumed(ctx) {
            return Vec::new();
        }
        if typing {
            if let Some(focused) = ctx.memory(|memory| memory.focused()) {
                ctx.memory_mut(|memory| memory.surrender_focus(focused));
            }
            return if session.draft.is_some() {
                vec![Command::CancelDraft]
            } else {
                Vec::new()
            };
        }
        return escape_command(session).into_iter().collect();
    }
    if typing {
        return Vec::new();
    }
    // 入力欄でない部品（一覧の行やボタン）が入力の位置を持っている。Tab で移っている人の操作を奪わない。
    let widget_focused = ctx.memory(|memory| memory.focused()).is_some();
    let mut out = Vec::new();
    let mut deleted = false;
    for (key, modifiers) in key_presses(ctx) {
        match key {
            // 削除は固定のキー。部品を選んでいるときだけ、1フレームに1回。
            egui::Key::Delete | egui::Key::Backspace => {
                if !widget_focused
                    && !deleted
                    && session.selection.is_some()
                    && plain(modifiers)
                    && !modifiers.shift
                {
                    out.push(Command::DeleteSelected);
                    deleted = true;
                }
                continue;
            }
            // 矢印と Enter は、いまは何もしない（キーで動かす分は WP-J1 が足す）。
            egui::Key::Enter
            | egui::Key::ArrowLeft
            | egui::Key::ArrowRight
            | egui::Key::ArrowUp
            | egui::Key::ArrowDown => continue,
            _ => {}
        }
        let Ok(chord) = Chord::from_event(key, modifiers) else {
            continue;
        };
        let Some(action) = session.keymap.action_for(chord) else {
            continue;
        };
        if let Some(command) = action_command(ctx, session, action) {
            out.push(command);
        }
    }
    out
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

fn key_press(ctx: &egui::Context, wanted: egui::Key) -> Option<egui::Modifiers> {
    ctx.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } if *key == wanted => Some(*modifiers),
            _ => None,
        })
    })
}

/// 押されたキー（押し続けを除く）を、届いた順に。
fn key_presses(ctx: &egui::Context) -> Vec<(egui::Key, egui::Modifiers)> {
    ctx.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
            .collect()
    })
}

fn plain(modifiers: egui::Modifiers) -> bool {
    !modifiers.command && !modifiers.ctrl && !modifiers.alt
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

fn resolve_dialog(session: &Session, command: Command) -> Option<Command> {
    match command {
        Command::OpenModelDialog if session.is_exporting() => Some(Command::OpenModelDialog),
        Command::OpenModelDialog => rfd::FileDialog::new()
            .add_filter("GLB・STL", &["glb", "stl", "STL"])
            .add_filter("GLB", &["glb"])
            .add_filter("STL", &["stl", "STL"])
            .pick_file()
            .map(Command::OpenModel),
        // 新規の文書は書き出すまで保存できない。保存先を聞かずに `Session` が断る（要件2.6）。
        Command::SaveReviewDialog
            if session.doc_unsaved() || session.is_exporting() || session.is_new_document() =>
        {
            Some(Command::SaveReviewDialog)
        }
        Command::SaveReviewDialog => rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .save_file()
            .map(Command::SaveReview),
        Command::LoadReviewDialog if session.is_exporting() => Some(Command::LoadReviewDialog),
        Command::LoadReviewDialog => rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .pick_file()
            .map(Command::LoadReview),
        Command::RelinkModelDialog if session.is_exporting() => Some(Command::RelinkModelDialog),
        Command::RelinkModelDialog => rfd::FileDialog::new()
            .add_filter("GLB・STL", &["glb", "stl", "STL"])
            .pick_file()
            .map(Command::RelinkModel),
        // 部品の無い新規の文書も、保存先を聞かずに `Session` が断る（要件2.2）。
        Command::ExportGlbDialog
            if session.model.is_none()
                || session.edits_locked()
                || crate::session::nothing_to_export(session) =>
        {
            Some(Command::ExportGlbDialog)
        }
        Command::ExportGlbDialog => {
            let mut dialog = rfd::FileDialog::new().add_filter("GLB", &["glb"]);
            if let Some(name) = export_file_name(session) {
                dialog = dialog.set_file_name(name);
            }
            dialog.save_file().map(Command::ExportGlb)
        }
        other => Some(other),
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
    let path = std::path::Path::new(&session.model_ref.as_ref()?.path);
    Some(format!("{}.glb", crate::loader::stl_part_name(path)))
}
