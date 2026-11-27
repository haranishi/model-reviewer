//! eframe の入口。`logic` で読み込みと終了、`ui` で5領域とコマンドの適用。

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;

use crate::automation::Automation;
use crate::command::{Command, DisplayMode, LaunchArgs, Tool, ViewDir};
use crate::fonts;
use crate::gpu_view::GpuView;
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

/// Esc はここでだけ振り分ける。ダイアログ中は閉じる・やめるだけ。
/// 入力欄に文字を打っている間は、下書きの取り消しか入力欄を離れるだけ。
/// それ以外は、変形の取り消し、下書き、選択以外の道具から選択への復帰、選択の解除の順で
/// 1回に1つ（要件5.5）。
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
    let over_panel = ctx.is_pointer_over_egui() && !ui::viewport::pointer_on_viewport(ctx);
    if typing || over_panel {
        return Vec::new();
    }
    let mut out = Vec::new();
    // 取り消しとやり直し（Windows と Linux では Ctrl）。入力欄の中の Cmd+Z は文字の取り消しなので、
    // 上の「入力中」で先に返している。
    if let Some(modifiers) = key_press(ctx, egui::Key::Z)
        && modifiers.command
        && !modifiers.alt
    {
        out.push(if modifiers.shift {
            Command::Redo
        } else {
            Command::Undo
        });
    }
    // 複製（Cmd+D）と削除（Delete・Backspace）。部品を選んでいるときだけ。
    if session.selection.is_some() {
        if let Some(modifiers) = key_press(ctx, egui::Key::D)
            && modifiers.command
            && !modifiers.alt
            && !modifiers.shift
        {
            out.push(Command::DuplicateSelected);
        }
        if [egui::Key::Delete, egui::Key::Backspace]
            .into_iter()
            .filter_map(|key| key_press(ctx, key))
            .any(|modifiers| plain(modifiers) && !modifiers.shift)
        {
            out.push(Command::DeleteSelected);
        }
    }
    for (key, tool) in [
        (egui::Key::V, Tool::Select),
        (egui::Key::G, Tool::Move),
        (egui::Key::R, Tool::Rotate),
        (egui::Key::S, Tool::Scale),
        (egui::Key::M, Tool::Measure),
    ] {
        if let Some(modifiers) = key_press(ctx, key)
            && plain(modifiers)
            && !modifiers.shift
            && session.tool != tool
        {
            out.push(Command::SetTool(tool));
        }
    }
    if let Some(modifiers) = key_press(ctx, egui::Key::A)
        && plain(modifiers)
        && !modifiers.shift
    {
        out.push(Command::FitAll);
    }
    if let Some(modifiers) = key_press(ctx, egui::Key::F)
        && plain(modifiers)
        && !modifiers.shift
    {
        out.push(Command::FitSelection);
    }
    if let Some(modifiers) = key_press(ctx, egui::Key::Home)
        && plain(modifiers)
    {
        out.push(Command::Home);
    }
    if let Some(modifiers) = key_press(ctx, egui::Key::H)
        && plain(modifiers)
    {
        if modifiers.shift {
            out.push(Command::ShowAll);
        } else {
            out.push(Command::HideSelected);
        }
    }
    if let Some(modifiers) = key_press(ctx, egui::Key::N)
        && plain(modifiers)
        && !modifiers.shift
    {
        out.push(Command::SetAnnotateMode(!session.annotate_mode));
    }
    // 数字は文字キーの段（テンキーは使わない）。
    for (key, command) in [
        (egui::Key::Num1, Command::SnapView(ViewDir::Front)),
        (egui::Key::Num3, Command::SnapView(ViewDir::Right)),
        (egui::Key::Num7, Command::SnapView(ViewDir::Top)),
        (egui::Key::Num5, Command::ToggleProjection),
    ] {
        if let Some(modifiers) = key_press(ctx, key)
            && plain(modifiers)
            && !modifiers.shift
        {
            out.push(command);
        }
    }
    // 吸着（X）と断面（C）。修飾つき（Cmd+X・Cmd+C）は切り取りとコピーなので受けない。
    if let Some(modifiers) = key_press(ctx, egui::Key::X)
        && plain(modifiers)
        && !modifiers.shift
    {
        out.push(Command::ToggleSnap);
    }
    if let Some(modifiers) = key_press(ctx, egui::Key::C)
        && plain(modifiers)
        && !modifiers.shift
        && session.model.is_some()
    {
        out.push(Command::ToggleSection);
    }
    if let Some(modifiers) = key_press(ctx, egui::Key::Z)
        && plain(modifiers)
    {
        out.push(Command::SetDisplay(toggled_display(
            session.display,
            modifiers.shift,
        )));
    }
    out
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
        Command::SaveReviewDialog if session.doc_unsaved() || session.is_exporting() => {
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
        Command::ExportGlbDialog if session.model.is_none() || session.edits_locked() => {
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

/// 書き出しの保存先の既定の名前。STL を開いているときは「<元の名前>.glb」。GLB は既定の名前を
/// 付けない（開いたファイルと同じ名前になり、そこへは書き出せないため）。
pub fn export_file_name(session: &Session) -> Option<String> {
    if !matches!(session.source_kind, crate::session::SourceKind::Stl { .. }) {
        return None;
    }
    let path = std::path::Path::new(&session.model_ref.as_ref()?.path);
    Some(format!("{}.glb", crate::loader::stl_part_name(path)))
}
