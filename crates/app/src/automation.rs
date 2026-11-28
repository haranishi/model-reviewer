//! `--script` の手順を、本物のウィンドウの egui 入力へ注入して実行する。
//!
//! 手順JSONはオブジェクト。`timeout_secs`（既定30）と `steps`。各要素の `op`:
//! `open_model` `load_review` `save_review` `export_glb` `wait_idle` `drag_view` `scroll_view`
//! `click_view` `click_widget` `drag_widget` `type_text` `key` `screenshot` `dump_state`
//! `assert_state_equals` `sample` `quit`。
//! `sample`（`label`、待つフレーム数 `frames`）と、`drag_view`・`drag_widget` の `sample_every`
//! は、常駐メモリ・フレームのCPU時間・GPUへ送ったバイト数などを `report.json` の `samples` へ残す
//! （性能の測定用。ドラッグでは押したフレーム、`sample_every` ごと、離す直前、離した後に残す）。
//! パスは絶対パスにしない。読み取りは手順ファイルの隣、書き出しは `--out` が既定。
//! `relative_to` が `script` または `out` のときはそちらを使う。
//! ビュー座標は部品矩形の中の 0〜1。角度の状態は `OrbitCamera` のラジアンのまま書く。

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::command::{Command, LaunchArgs};
use crate::loader::LoadPhase;
use crate::metrics;
use crate::session::Session;
use crate::ui::ids;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_TOLERANCE: f64 = 1.0e-4;
const DEFAULT_DRAG_FRAMES: u32 = 8;

mod script;
mod state;

use script::load_script;
use state::{elapsed_ms, format_secs, save_png, state_value, values_match, write_json};

#[cfg(test)]
mod tests;

static EXIT_CODE: AtomicI32 = AtomicI32::new(0);

#[derive(Clone, Debug, Default)]
pub struct WidgetEntry {
    pub name: String,
    pub min: [f32; 2],
    pub max: [f32; 2],
}

#[derive(Clone, Debug, Default)]
pub struct WidgetFrame {
    entries: Vec<WidgetEntry>,
}

impl WidgetFrame {
    pub fn entries(&self) -> &[WidgetEntry] {
        &self.entries
    }

    pub fn push(&mut self, entry: WidgetEntry) {
        self.entries.push(entry);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// 手順が終わったあとの終了コード。スクリプトが無いときは 0。
pub fn exit_code() -> i32 {
    EXIT_CODE.load(Ordering::SeqCst)
}

fn set_exit_code(code: i32) {
    EXIT_CODE.store(code, Ordering::SeqCst);
}

struct RepaintWaker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl RepaintWaker {
    fn spawn(ctx: egui::Context) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("model-reviewer-repaint".to_owned())
            .spawn(move || {
                while !flag.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(100));
                    if flag.load(Ordering::SeqCst) {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .ok();
        Self { stop, thread }
    }

    fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for RepaintWaker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathBase {
    Script,
    Out,
}

#[derive(Clone, Debug)]
struct PathStep {
    path: String,
    base: PathBase,
}

#[derive(Clone, Debug)]
enum Op {
    /// STL の単位と上の向きは省略できる（mm・Z が上）。
    OpenModel {
        path: PathStep,
        stl_unit: Option<model_reviewer_core::units::UnitLabel>,
        stl_up: Option<model_reviewer_core::units::UpAxis>,
    },
    LoadReview(PathStep),
    SaveReview(PathStep),
    ExportGlb(PathStep),
    WaitIdle,
    DragView {
        from: [f32; 2],
        to: [f32; 2],
        button: egui::PointerButton,
        modifiers: egui::Modifiers,
        frames: u32,
        sample_every: u32,
    },
    ScrollView {
        at: [f32; 2],
        delta: [f32; 2],
        unit: egui::MouseWheelUnit,
        modifiers: egui::Modifiers,
    },
    ClickView {
        at: [f32; 2],
        button: egui::PointerButton,
        modifiers: egui::Modifiers,
    },
    /// 3Dビューの中の位置へポインタを動かす（押さない）。計測の吸着の候補を出す。
    HoverView {
        at: [f32; 2],
    },
    ClickWidget {
        name: String,
    },
    /// 登録した名前の部品の中心から、`delta`（画面のポイント）だけドラッグする。押す前に1フレーム
    /// その位置へポインタを置く（触れてから現れる操作域、たとえばハンドルが押下を受けられるように）。
    DragWidget {
        name: String,
        delta: [f32; 2],
        button: egui::PointerButton,
        modifiers: egui::Modifiers,
        frames: u32,
        sample_every: u32,
    },
    /// `frames` 回の `ui` を待ってから、性能値を1件残す。
    Sample {
        label: String,
        frames: u32,
    },
    TypeText {
        text: String,
    },
    Key {
        key: egui::Key,
        modifiers: egui::Modifiers,
    },
    Screenshot(PathStep),
    DumpState(PathStep),
    AssertState {
        path: PathStep,
        tolerance: f64,
    },
    Quit,
}

impl Op {
    fn name(&self) -> &'static str {
        match self {
            Self::OpenModel { .. } => "open_model",
            Self::LoadReview(_) => "load_review",
            Self::SaveReview(_) => "save_review",
            Self::ExportGlb(_) => "export_glb",
            Self::WaitIdle => "wait_idle",
            Self::DragView { .. } => "drag_view",
            Self::ScrollView { .. } => "scroll_view",
            Self::ClickView { .. } => "click_view",
            Self::HoverView { .. } => "hover_view",
            Self::ClickWidget { .. } => "click_widget",
            Self::DragWidget { .. } => "drag_widget",
            Self::TypeText { .. } => "type_text",
            Self::Key { .. } => "key",
            Self::Screenshot(_) => "screenshot",
            Self::DumpState(_) => "dump_state",
            Self::Sample { .. } => "sample",
            Self::AssertState { .. } => "assert_state_equals",
            Self::Quit => "quit",
        }
    }
}

#[derive(Clone, Debug)]
struct Step {
    op: Op,
    timeout: Option<Duration>,
}

#[derive(Clone, Debug)]
struct StepRecord {
    index: usize,
    op: String,
    status: &'static str,
    message: String,
    elapsed_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wait {
    None,
    /// 矩形や読み込みを、次の `ui` まで待つ。同じフレームでやり直さない。
    Retry,
    Inject,
    /// `scroll_view` の入力は届いたが、egui が量を数フレームに分けて渡している途中。
    Scroll,
    Shot,
}

struct Drag {
    from: egui::Pos2,
    to: egui::Pos2,
    button: egui::PointerButton,
    modifiers: egui::Modifiers,
    frames: u32,
    next: u32,
    /// 押すフレーム。それより前は `from` に置くだけ。
    press_at: u32,
    /// 何フレームごとに性能値を残すか。0なら残さない（押した・離す直前・離した後も残さない）。
    sample_every: u32,
}

struct ScriptRun {
    default_timeout: Duration,
    script_dir: PathBuf,
    out_dir: PathBuf,
    steps: Vec<Step>,
    index: usize,
    step_started: Option<Instant>,
    results: Vec<StepRecord>,
    wait: Wait,
    drag: Option<Drag>,
    shot: Option<Arc<Mutex<Option<Arc<egui::ColorImage>>>>>,
    shot_path: Option<PathBuf>,
    inject_pending: bool,
    staged: Vec<egui::Event>,
    waiting_note: Option<String>,
    /// `click_widget` が前の回に見た部品の位置。
    settling: Option<egui::Rect>,
    /// `type_text` が前の回に入力位置を見たか。
    focus_seen: bool,
    /// `sample` が待った `ui` の回数。
    sample_waited: u32,
    /// 残した性能値（`report.json` の `samples`）。
    samples: Vec<Value>,
    finished: bool,
    ok: bool,
    started: Instant,
}

pub struct Automation {
    active: bool,
    out_dir: Option<PathBuf>,
    error: Option<String>,
    run: Option<ScriptRun>,
    saw_hook: bool,
    finished: bool,
    reported: bool,
    forcing_quit: bool,
    waker: Option<RepaintWaker>,
    /// 直前のフレームの描画の統計（性能値の記録用）。
    gpu_stats: Option<model_reviewer_render::RenderStats>,
}

impl Automation {
    pub fn from_launch(launch: &LaunchArgs) -> Self {
        let mut automation = Self {
            active: launch.script.is_some(),
            out_dir: launch.out.clone(),
            error: None,
            run: None,
            saw_hook: false,
            finished: launch.script.is_none(),
            reported: launch.script.is_none(),
            forcing_quit: false,
            waker: None,
            gpu_stats: None,
        };
        let Some(script) = launch.script.clone() else {
            return automation;
        };
        let Some(out) = launch.out.clone() else {
            automation.error = Some("自動操作には --out が必要です".to_owned());
            return automation;
        };
        match load_script(&script, &out) {
            Ok(run) => automation.run = Some(run),
            Err(err) => automation.error = Some(err),
        }
        automation
    }

    pub fn inject(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if !self.running() {
            return;
        }
        let staged = self
            .run
            .as_ref()
            .filter(|run| run.inject_pending)
            .map(|run| run.staged.clone());
        self.saw_hook = true;
        strip_user_input(raw);
        if let Some(staged) = staged {
            raw.events.extend(staged);
        }
    }

    /// `on_ui` の前に、そのフレームの描画の統計を渡す（`sample` の記録に使う）。
    pub fn observe_gpu(&mut self, stats: Option<model_reviewer_render::RenderStats>) {
        self.gpu_stats = stats;
    }

    pub fn needs_repaint(&self) -> bool {
        self.running()
    }

    pub fn forcing_quit(&self) -> bool {
        self.forcing_quit
    }

    /// 手順JSONを読めなかった理由。読めたときは `None`。
    pub fn script_error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// 手順の実行中に毎フレーム呼ぶ。隠れて `ui` が無くても時間切れにする。
    pub fn on_logic(&mut self, ctx: &egui::Context, session: &Session, _gpu_busy: bool) {
        if !self.active || self.reported {
            return;
        }
        if self.error.is_some() {
            let message = self.error.clone().unwrap_or_default();
            self.fail(session, message);
            return;
        }
        if self.finished {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
        self.arm_clock();
        if self.timed_out(session) {
            return;
        }
        if self.waker.is_none() {
            self.waker = Some(RepaintWaker::spawn(ctx.clone()));
        }
    }

    /// `ui` が入力を受け取った直後に呼ぶ。ここで手順を進める。
    pub fn on_ui(&mut self, ctx: &egui::Context, session: &mut Session, gpu_busy: bool) {
        if !self.active || self.reported {
            return;
        }
        let delivered = self.saw_hook
            && self
                .run
                .as_ref()
                .is_some_and(|run| run.inject_pending && run.wait == Wait::Inject);
        self.saw_hook = false;
        if let Some(run) = &mut self.run {
            if delivered {
                run.inject_pending = false;
            }
            if run.wait == Wait::Retry {
                run.wait = Wait::None;
            }
        }
        self.advance(ctx, session, gpu_busy, delivered);
        if self.forcing_quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    pub fn on_exit(&mut self, session: &Session) {
        if self.active && !self.reported {
            self.fail(session, "ウィンドウが閉じました".to_owned());
        }
    }

    pub(crate) fn running(&self) -> bool {
        self.active && !self.finished && !self.reported
    }

    fn arm_clock(&mut self) {
        if let Some(run) = &mut self.run
            && !run.finished
            && run.step_started.is_none()
        {
            run.step_started = Some(Instant::now());
        }
    }

    fn timed_out(&mut self, session: &Session) -> bool {
        let (elapsed, timeout, note) = {
            let Some(run) = &self.run else {
                return false;
            };
            if run.finished {
                return false;
            }
            let Some(started) = run.step_started else {
                return false;
            };
            let timeout = run.current_timeout();
            (started.elapsed(), timeout, run.waiting_note.clone())
        };
        if elapsed < timeout {
            return false;
        }
        let mut message = format!("時間切れ（{}秒）", format_secs(timeout));
        if let Some(note) = note {
            message.push_str(": ");
            message.push_str(&note);
        }
        self.fail(session, message);
        true
    }

    fn advance(
        &mut self,
        ctx: &egui::Context,
        session: &mut Session,
        gpu_busy: bool,
        delivered: bool,
    ) {
        loop {
            if self.finished || self.run.is_none() {
                return;
            }
            if self
                .run
                .as_ref()
                .is_some_and(|run| run.wait == Wait::Inject)
            {
                if !delivered {
                    return;
                }
                if let Some(label) = self.drag_sample_due() {
                    self.record_sample(session, label);
                }
                if self.continue_drag() {
                    return;
                }
                if self.wait_for_scroll(ctx) {
                    return;
                }
                self.mark_ok();
                continue;
            }
            if self
                .run
                .as_ref()
                .is_some_and(|run| run.wait == Wait::Scroll)
            {
                if ctx.input(|input| input.is_scrolling()) {
                    return;
                }
                self.mark_ok();
                continue;
            }
            if self.run.as_ref().is_some_and(|run| run.wait == Wait::Shot) {
                match self.poll_shot() {
                    None => return,
                    Some(image) => {
                        if let Err(err) = self.write_shot(&image) {
                            self.fail(session, err);
                            return;
                        }
                        self.mark_ok();
                        continue;
                    }
                }
            }
            if self
                .run
                .as_ref()
                .is_some_and(|run| run.index >= run.steps.len())
            {
                self.succeed(session);
                return;
            }
            self.arm_clock();
            let op = self
                .run
                .as_ref()
                .and_then(|run| run.steps.get(run.index).map(|step| step.op.clone()));
            let Some(op) = op else {
                self.succeed(session);
                return;
            };
            if let Err(err) = self.start_op(ctx, session, gpu_busy, op) {
                self.fail(session, err);
                return;
            }
            if self.finished || self.run.as_ref().is_some_and(|run| run.wait != Wait::None) {
                return;
            }
        }
    }

    fn start_op(
        &mut self,
        ctx: &egui::Context,
        session: &mut Session,
        gpu_busy: bool,
        op: Op,
    ) -> Result<(), String> {
        match op {
            Op::OpenModel {
                path,
                stl_unit,
                stl_up,
            } => {
                let path = self.resolve(&path)?;
                if stl_unit.is_none() && stl_up.is_none() {
                    session.apply(Command::OpenModel(path));
                } else {
                    session.open_model_with(path, crate::loader::LoadOptions { stl_unit, stl_up });
                }
                self.mark_ok();
            }
            Op::LoadReview(path) => {
                session.apply(Command::LoadReview(self.resolve(&path)?));
                self.mark_ok();
            }
            Op::SaveReview(path) => {
                session.apply(Command::SaveReview(self.resolve(&path)?));
                self.mark_ok();
            }
            Op::ExportGlb(path) => {
                session.apply(Command::ExportGlb(self.resolve(&path)?));
                self.mark_ok();
            }
            Op::WaitIdle => match idle_error(session, gpu_busy) {
                Ok(true) => self.mark_ok(),
                Ok(false) => {
                    self.note("読み込み・書き出し・形状と画像の転送・当たり判定の準備待ち")
                }
                Err(err) => return Err(err),
            },
            Op::DragView {
                from,
                to,
                button,
                modifiers,
                frames,
                sample_every,
            } => {
                let rect = match widget_rect(ctx, ids::VIEWPORT) {
                    Some(rect) => rect,
                    None => {
                        self.note("viewport がありません");
                        return Ok(());
                    }
                };
                let start = unit_to_pos(from, rect)?;
                let end = unit_to_pos(to, rect)?;
                if let Some(run) = &mut self.run {
                    run.drag = Some(Drag {
                        from: start,
                        to: end,
                        button,
                        modifiers,
                        frames,
                        next: 0,
                        press_at: 0,
                        sample_every,
                    });
                }
                self.queue_drag_frame();
            }
            Op::DragWidget {
                name,
                delta,
                button,
                modifiers,
                frames,
                sample_every,
            } => {
                let Some(rect) = widget_rect(ctx, &name) else {
                    self.note(format!("{name} がありません"));
                    return Ok(());
                };
                // 出たばかりの部品は位置が定まらない。2回続けて同じ位置になってから掴む。
                let settled = self.run.as_mut().is_some_and(|run| {
                    let same = run.settling == Some(rect);
                    run.settling = (!same).then_some(rect);
                    same
                });
                if !settled {
                    self.note(format!("{name} の位置が定まるのを待っています"));
                    return Ok(());
                }
                let from = rect.center();
                let to = from + egui::vec2(delta[0], delta[1]);
                if let Some(run) = &mut self.run {
                    run.drag = Some(Drag {
                        from,
                        to,
                        button,
                        modifiers,
                        frames: frames + 1,
                        next: 0,
                        press_at: 1,
                        sample_every,
                    });
                }
                self.queue_drag_frame();
            }
            Op::ScrollView {
                at,
                delta,
                unit,
                modifiers,
            } => {
                let Some(pos) = self.view_pos(ctx, at)? else {
                    return Ok(());
                };
                self.queue(scroll_events(pos, delta, unit, modifiers));
            }
            Op::ClickView {
                at,
                button,
                modifiers,
            } => {
                let Some(pos) = self.view_pos(ctx, at)? else {
                    return Ok(());
                };
                self.queue(click_events(pos, button, modifiers));
            }
            Op::HoverView { at } => {
                let Some(pos) = self.view_pos(ctx, at)? else {
                    return Ok(());
                };
                self.queue(vec![egui::Event::PointerMoved(pos)]);
            }
            Op::ClickWidget { name } => {
                let Some(rect) = widget_rect(ctx, &name) else {
                    self.note(format!("{name} がありません"));
                    return Ok(());
                };
                // 出たばかりの部品（入力欄の小窓など）は、最初の回の位置が定まらない。
                // 2回続けて同じ位置になってから押す。
                let settled = self.run.as_mut().is_some_and(|run| {
                    let same = run.settling == Some(rect);
                    run.settling = (!same).then_some(rect);
                    same
                });
                if !settled {
                    self.note(format!("{name} の位置が定まるのを待っています"));
                    return Ok(());
                }
                self.queue(click_events(
                    rect.center(),
                    egui::PointerButton::Primary,
                    egui::Modifiers::NONE,
                ));
            }
            Op::TypeText { text } => {
                // 打つ先の入力欄に入力位置が移るまで待つ。出た直後の回に送ると捨てられる。
                // 直前の入力欄の入力位置が1回だけ残ることがあるので、2回続けて確かめる。
                let focused = ctx.text_edit_focused();
                let settled = self.run.as_mut().is_some_and(|run| {
                    let seen = focused && run.focus_seen;
                    run.focus_seen = focused && !seen;
                    seen
                });
                if !settled {
                    self.note("入力欄に入力位置がありません");
                    return Ok(());
                }
                self.queue(vec![egui::Event::Text(text)]);
            }
            Op::Key { key, modifiers } => {
                self.queue(key_events(key, modifiers));
            }
            Op::Screenshot(path) => {
                let resolved = self.resolve(&path)?;
                let slot = Arc::new(Mutex::new(None));
                let writer = Arc::clone(&slot);
                ctx.request_screenshot(move |image| {
                    if let Ok(mut guard) = writer.lock() {
                        *guard = Some(image);
                    }
                });
                if let Some(run) = &mut self.run {
                    run.shot = Some(slot);
                    run.shot_path = Some(resolved);
                    run.wait = Wait::Shot;
                    run.waiting_note = Some("画像の到着待ち".to_owned());
                }
            }
            Op::DumpState(path) => {
                let resolved = self.resolve(&path)?;
                write_json(&resolved, &state_value(session))?;
                self.mark_ok();
            }
            Op::AssertState { path, tolerance } => {
                let resolved = self.resolve(&path)?;
                let text = fs::read_to_string(&resolved)
                    .map_err(|_| format!("比較する状態を読めませんでした: {}", path.path))?;
                let expected: Value = serde_json::from_str(&text)
                    .map_err(|err| format!("比較する状態のJSONを解釈できません: {err}"))?;
                let actual = state::comparable(state_value(session));
                let expected = state::comparable(expected);
                values_match(&actual, &expected, tolerance, "$")?;
                self.mark_ok();
            }
            Op::Sample { label, frames } => {
                let waited = self.run.as_mut().map_or(0, |run| {
                    let waited = run.sample_waited;
                    run.sample_waited += 1;
                    waited
                });
                if waited < frames {
                    // 次の `ui` で数え直す（入力は送らない）。
                    if let Some(run) = &mut self.run {
                        run.wait = Wait::Retry;
                        run.waiting_note = Some("フレームを待っています".to_owned());
                    }
                    return Ok(());
                }
                if let Some(run) = &mut self.run {
                    run.sample_waited = 0;
                }
                self.record_sample(session, label);
                self.mark_ok();
            }
            Op::Quit => {
                self.mark_ok();
                self.finish(session, true);
            }
        }
        Ok(())
    }

    /// 矩形が無いときは `Ok(None)` で次フレームまで待つ。範囲外は失敗。
    fn view_pos(
        &mut self,
        ctx: &egui::Context,
        unit: [f32; 2],
    ) -> Result<Option<egui::Pos2>, String> {
        let Some(rect) = widget_rect(ctx, ids::VIEWPORT) else {
            self.note("viewport がありません");
            return Ok(None);
        };
        unit_to_pos(unit, rect).map(Some)
    }

    fn resolve(&self, path: &PathStep) -> Result<PathBuf, String> {
        let Some(run) = &self.run else {
            return Err("手順が読み込まれていません".to_owned());
        };
        if !script::is_plain_relative(&path.path) {
            return Err("手順のパスは、手順ファイルか --out からの相対パスです".to_owned());
        }
        let base = match path.base {
            PathBase::Script => &run.script_dir,
            PathBase::Out => &run.out_dir,
        };
        Ok(base.join(&path.path))
    }

    /// いま届いたドラッグのフレームで性能値を残すなら、その名前。
    fn drag_sample_due(&self) -> Option<String> {
        let run = self.run.as_ref()?;
        let drag = run.drag.as_ref()?;
        if drag.sample_every == 0 {
            return None;
        }
        let index = run.index;
        let frame = drag.next;
        if frame == drag.press_at {
            Some(format!("step{index}:press"))
        } else if drag.next + 1 == drag.frames {
            Some(format!("step{index}:released"))
        } else if drag.next + 2 == drag.frames {
            Some(format!("step{index}:before-release"))
        } else if (frame - drag.press_at).is_multiple_of(drag.sample_every) {
            Some(format!("step{index}:frame{}", frame - drag.press_at))
        } else {
            None
        }
    }

    fn record_sample(&mut self, session: &Session, label: String) {
        let stats = self.gpu_stats;
        let m = &session.metrics;
        let ms = |d: Option<Duration>| d.map(|d| d.as_secs_f64() * 1000.0);
        let value = json!({
            "label": label,
            "physical_mem": metrics::sample_memory(),
            "texture_present_ms": ms(m.texture_present),
            "texture_peak_mem": m.texture_peak_mem,
            "texture_count": stats.map(|s| s.texture_count),
            "texture_bytes": stats.map(|s| s.texture_bytes),
            "textured_draws": stats.map(|s| s.textured_draws),
            "texture_shrunk": session.textures.shrunk,
            "frame_cpu_avg_ms": ms(m.frame_cpu_avg),
            "frame_cpu_max_ms": ms(m.frame_cpu_max),
            "edit_commit_ms": ms(m.edit_commit),
            "undo_redo_ms": ms(m.undo_redo),
            "history_bytes": m.history_bytes,
            "history_steps": m.history_len,
            "export_read_ms": ms(m.export_read),
            "export_build_ms": ms(m.export_build),
            "export_write_ms": ms(m.export_write),
            "export_verify_ms": ms(m.export_verify),
            "export_peak_mem": m.export_peak_mem,
            "snap_ms": ms(m.snap_last),
            "snap_max_ms": ms(m.snap_max),
            "uploaded_bytes": stats.map(|s| s.uploaded_bytes),
            "frame_bytes": stats.map(|s| s.frame_bytes),
            "draw_count": stats.map(|s| s.draw_count),
            "instance_rebuilds": stats.map(|s| s.instance_rebuilds),
            "instance_patches": stats.map(|s| s.instance_patches),
            "nodes": session.doc.as_ref().map(model_reviewer_core::scene::SceneRead::node_count),
        });
        if let Some(run) = &mut self.run {
            run.samples.push(value);
        }
    }

    fn note(&mut self, note: impl Into<String>) {
        if let Some(run) = &mut self.run {
            run.waiting_note = Some(note.into());
            run.wait = Wait::Retry;
        }
    }

    fn queue(&mut self, events: Vec<egui::Event>) {
        if let Some(run) = &mut self.run {
            run.staged = events;
            run.inject_pending = true;
            run.wait = Wait::Inject;
            run.waiting_note = Some("入力の反映待ち".to_owned());
            run.drag = None;
        }
    }

    fn queue_drag_frame(&mut self) {
        let Some(run) = &mut self.run else {
            return;
        };
        let Some(drag) = &run.drag else {
            return;
        };
        let events = drag_frame_events(drag);
        run.staged = events;
        run.inject_pending = true;
        run.wait = Wait::Inject;
        run.waiting_note = Some("入力の反映待ち".to_owned());
    }

    fn continue_drag(&mut self) -> bool {
        let Some(run) = &mut self.run else {
            return false;
        };
        let Some(drag) = &mut run.drag else {
            return false;
        };
        drag.next += 1;
        if drag.next >= drag.frames {
            run.drag = None;
            return false;
        }
        let events = drag_frame_events(drag);
        run.staged = events;
        run.inject_pending = true;
        run.wait = Wait::Inject;
        true
    }

    /// egui はホイールの量を数フレームに分けて渡す（滑らかなスクロール）。渡し終える前に次の手順へ
    /// 進むと、次の手順でポインタが3Dビューを離れたときに残りが捨てられ、結果が実行ごとに変わる。
    /// `scroll_view` は、egui がスクロールを終えたと判断するまで待つ。待つときは `true`。
    fn wait_for_scroll(&mut self, ctx: &egui::Context) -> bool {
        let Some(run) = &mut self.run else {
            return false;
        };
        let scrolling_step = run
            .steps
            .get(run.index)
            .is_some_and(|step| matches!(step.op, Op::ScrollView { .. }));
        if !scrolling_step || !ctx.input(|input| input.is_scrolling()) {
            return false;
        }
        run.wait = Wait::Scroll;
        run.waiting_note = Some("スクロールの反映待ち".to_owned());
        true
    }

    fn poll_shot(&mut self) -> Option<Arc<egui::ColorImage>> {
        let run = self.run.as_mut()?;
        let slot = run.shot.as_ref()?;
        let mut guard = slot.lock().unwrap_or_else(|err| err.into_inner());
        guard.take()
    }

    fn write_shot(&mut self, image: &egui::ColorImage) -> Result<(), String> {
        let path = self
            .run
            .as_ref()
            .and_then(|run| run.shot_path.clone())
            .ok_or_else(|| "画像の保存先がありません".to_owned())?;
        save_png(&path, image)
    }

    fn mark_ok(&mut self) {
        let Some(run) = &mut self.run else {
            return;
        };
        if run.index >= run.steps.len() {
            return;
        }
        let op = run.steps[run.index].op.name().to_owned();
        let elapsed_ms = elapsed_ms(run.step_started);
        let index = run.index;
        run.results.push(StepRecord {
            index,
            op,
            status: "ok",
            message: String::new(),
            elapsed_ms,
        });
        run.index += 1;
        run.step_started = None;
        run.wait = Wait::None;
        run.waiting_note = None;
        run.shot = None;
        run.shot_path = None;
    }

    fn succeed(&mut self, session: &Session) {
        self.finish(session, cfg!(target_os = "macos"));
    }

    fn finish(&mut self, session: &Session, force_close: bool) {
        if self.finished {
            return;
        }
        if let Some(run) = &mut self.run {
            while run.index < run.steps.len() {
                let op = run.steps[run.index].op.name().to_owned();
                let index = run.index;
                run.results.push(StepRecord {
                    index,
                    op,
                    status: "skipped",
                    message: "quit の後なので実行していません".to_owned(),
                    elapsed_ms: 0,
                });
                run.index += 1;
            }
            run.finished = true;
            run.ok = !run.results.iter().any(|record| record.status == "failed");
        }
        self.finished = true;
        self.forcing_quit = force_close;
        self.stop_waker();
        self.write_report(session);
    }

    pub(crate) fn fail(&mut self, session: &Session, message: String) {
        if self.reported {
            return;
        }
        if let Some(run) = &mut self.run
            && !run.finished
        {
            if run.index < run.steps.len() {
                let op = run.steps[run.index].op.name().to_owned();
                let elapsed_ms = elapsed_ms(run.step_started);
                let index = run.index;
                run.results.push(StepRecord {
                    index,
                    op,
                    status: "failed",
                    message: message.clone(),
                    elapsed_ms,
                });
                run.index += 1;
            }
            while run.index < run.steps.len() {
                let op = run.steps[run.index].op.name().to_owned();
                let index = run.index;
                run.results.push(StepRecord {
                    index,
                    op,
                    status: "skipped",
                    message: String::new(),
                    elapsed_ms: 0,
                });
                run.index += 1;
            }
            run.finished = true;
            run.ok = false;
        }
        self.finished = true;
        self.forcing_quit = true;
        if self.error.is_none() && self.run.is_none() {
            self.error = Some(message);
        }
        self.stop_waker();
        self.write_report(session);
    }

    fn stop_waker(&mut self) {
        if let Some(waker) = &self.waker {
            waker.stop();
        }
    }

    fn write_report(&mut self, session: &Session) {
        if self.reported {
            return;
        }
        self.reported = true;
        let ok = self.run.as_ref().is_some_and(|run| run.ok) && self.error.is_none();
        set_exit_code(if ok { 0 } else { 1 });
        let Some(out) = self.out_dir.clone() else {
            if let Some(message) = &self.error {
                eprintln!("自動操作: {message}");
            }
            return;
        };
        let steps = self
            .run
            .as_ref()
            .map(|run| {
                run.results
                    .iter()
                    .map(|record| {
                        json!({
                            "index": record.index,
                            "op": record.op,
                            "status": record.status,
                            "message": record.message,
                            "elapsed_ms": record.elapsed_ms,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let elapsed_ms = self
            .run
            .as_ref()
            .map(|run| elapsed_ms(Some(run.started)))
            .unwrap_or(0);
        let body = json!({
            "ok": ok,
            "elapsed_ms": elapsed_ms,
            "error": self.error,
            "steps": steps,
            "samples": self.run.as_ref().map(|run| run.samples.clone()).unwrap_or_default(),
            "metrics": metrics::report_value(&session.metrics),
        });
        if let Err(err) = write_json(&out.join("report.json"), &body) {
            eprintln!("自動操作: {err}");
        }
    }
}

impl ScriptRun {
    fn current_timeout(&self) -> Duration {
        self.steps
            .get(self.index)
            .and_then(|step| step.timeout)
            .unwrap_or(self.default_timeout)
    }
}

fn idle_error(session: &Session, gpu_busy: bool) -> Result<bool, String> {
    if let LoadPhase::Failed(message) = &session.load {
        return Err(format!("読み込みに失敗しました: {message}"));
    }
    if let Some(message) = session.export_error() {
        return Err(format!("書き出しに失敗しました: {message}"));
    }
    let loading = !matches!(session.load, LoadPhase::Idle);
    let exporting = session.is_exporting();
    let picking = session.model.is_some() && session.pick_index.is_none();
    Ok(!loading && !exporting && !picking && !gpu_busy && !session.textures_busy())
}

fn widget_rect(ctx: &egui::Context, name: &str) -> Option<egui::Rect> {
    ctx.data(|data| {
        let frame = data.get_temp::<WidgetFrame>(egui::Id::NULL)?;
        let entry = frame
            .entries()
            .iter()
            .rev()
            .find(|entry| entry.name == name)?;
        Some(egui::Rect::from_min_max(
            egui::pos2(entry.min[0], entry.min[1]),
            egui::pos2(entry.max[0], entry.max[1]),
        ))
    })
}

fn unit_to_pos(unit: [f32; 2], rect: egui::Rect) -> Result<egui::Pos2, String> {
    let Some(point) = view_to_screen(unit, [rect.min.x, rect.min.y], [rect.max.x, rect.max.y])
    else {
        return Err("ビュー座標は0以上1以下です".to_owned());
    };
    Ok(egui::pos2(point[0], point[1]))
}

fn view_to_screen(unit: [f32; 2], min: [f32; 2], max: [f32; 2]) -> Option<[f32; 2]> {
    if !unit[0].is_finite() || !unit[1].is_finite() {
        return None;
    }
    if !(0.0..=1.0).contains(&unit[0]) || !(0.0..=1.0).contains(&unit[1]) {
        return None;
    }
    let width = max[0] - min[0];
    let height = max[1] - min[1];
    if !min[0].is_finite()
        || !min[1].is_finite()
        || !width.is_finite()
        || !height.is_finite()
        || width < 0.0
        || height < 0.0
    {
        return None;
    }
    Some([min[0] + unit[0] * width, min[1] + unit[1] * height])
}

fn click_events(
    pos: egui::Pos2,
    button: egui::PointerButton,
    modifiers: egui::Modifiers,
) -> Vec<egui::Event> {
    vec![
        egui::Event::ModifiersChanged(modifiers),
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button,
            pressed: true,
            modifiers,
        },
        egui::Event::PointerButton {
            pos,
            button,
            pressed: false,
            modifiers,
        },
        egui::Event::ModifiersChanged(egui::Modifiers::NONE),
    ]
}

fn scroll_events(
    pos: egui::Pos2,
    delta: [f32; 2],
    unit: egui::MouseWheelUnit,
    modifiers: egui::Modifiers,
) -> Vec<egui::Event> {
    vec![
        egui::Event::ModifiersChanged(modifiers),
        egui::Event::PointerMoved(pos),
        egui::Event::MouseWheel {
            unit,
            delta: egui::vec2(delta[0], delta[1]),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers,
        },
        egui::Event::ModifiersChanged(egui::Modifiers::NONE),
    ]
}

fn key_events(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
    vec![
        egui::Event::ModifiersChanged(modifiers),
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        },
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers,
        },
        egui::Event::ModifiersChanged(egui::Modifiers::NONE),
    ]
}

fn drag_frame_events(drag: &Drag) -> Vec<egui::Event> {
    let pos = if drag.next <= drag.press_at {
        drag.from
    } else {
        let denom = (drag.frames - 1 - drag.press_at).max(1) as f32;
        let t = (drag.next - drag.press_at) as f32 / denom;
        drag.from + (drag.to - drag.from) * t
    };
    if drag.next < drag.press_at {
        return vec![egui::Event::PointerMoved(pos)];
    }
    let mut events = vec![
        egui::Event::ModifiersChanged(drag.modifiers),
        egui::Event::PointerMoved(pos),
    ];
    if drag.next == drag.press_at {
        events.push(egui::Event::PointerButton {
            pos,
            button: drag.button,
            pressed: true,
            modifiers: drag.modifiers,
        });
    }
    if drag.next + 1 == drag.frames {
        events.push(egui::Event::PointerButton {
            pos,
            button: drag.button,
            pressed: false,
            modifiers: drag.modifiers,
        });
        events.push(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    }
    events
}

fn strip_user_input(raw: &mut egui::RawInput) {
    raw.events.retain(|event| !is_user_pointer_or_key(event));
    raw.hovered_files.clear();
    raw.dropped_files.clear();
}

fn is_user_pointer_or_key(event: &egui::Event) -> bool {
    !matches!(
        event,
        egui::Event::WindowFocused(_) | egui::Event::AccessKitActionRequest(_)
    )
}
