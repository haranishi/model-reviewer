//! 性能値。測っていないものは `None` のままにする（0で埋めない）。

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use model_reviewer_core::scene::ModelStats;

const FRAME_WINDOW: usize = 120;
/// これ以上空いた間隔は、連続して描いているときの値にしない。
const CONTINUOUS_GAP: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Metrics {
    pub physical_mem: Option<u64>,
    /// 読み取ったバイト数。
    pub file_bytes: Option<u64>,
    /// `fs::read` の経過時間。
    pub read: Option<Duration>,
    /// blake3 の経過時間。
    pub identity: Option<Duration>,
    /// 容器とJSONの解析から `Model` の完成までの経過時間。
    pub parse: Option<Duration>,
    /// `continue_upload` にかかったCPU時間の合計。GPUの処理時間ではない。
    pub upload_cpu: Option<Duration>,
    /// `continue_upload` を呼んだ回数。
    pub upload_frames: Option<u64>,
    /// 開く操作を受けてから、そのモデルの最初の描画命令を送り終えるまで。
    /// 画面に出た瞬間（提示）そのものは含まない。
    pub first_present: Option<Duration>,
    /// 開く操作から最後の画像の全段を送るまで。
    pub texture_present: Option<Duration>,
    /// 画像を読み込む間にフレームごとに測った常駐メモリの最大。
    pub texture_peak_mem: Option<u64>,
    /// `PickIndex::build` の経過時間。表示とは並行。
    pub pick: Option<Duration>,
    /// `App::ui` の直近最大120回の平均。
    pub frame_cpu_avg: Option<Duration>,
    /// `App::ui` の直近最大120回の最大。
    pub frame_cpu_max: Option<Duration>,
    /// 直近最大120回の、100ms未満だったフレーム間隔の平均。
    pub frame_interval: Option<Duration>,
    /// 同じ範囲の最大。
    pub frame_interval_max: Option<Duration>,
    /// 直近の編集の確定（変形・数値）の `apply` にかかった時間。
    pub edit_commit: Option<Duration>,
    /// 直近の取り消し・やり直しの `apply` にかかった時間。
    pub undo_redo: Option<Duration>,
    /// 履歴が持つ操作の大きさの見積もり（バイト）。
    pub history_bytes: Option<u64>,
    /// 取り消せる回数。
    pub history_len: Option<u64>,
    /// 直近のGLB書き出しの工程ごとの時間。
    pub export_read: Option<Duration>,
    pub export_build: Option<Duration>,
    pub export_write: Option<Duration>,
    pub export_verify: Option<Duration>,
    /// 直近のGLB書き出し中に観測した常駐メモリの最大。
    pub export_peak_mem: Option<u64>,
    /// 直近の吸着の探索（クリックとポインタの移動。当たり判定を含む）。
    pub snap_last: Option<Duration>,
    /// この作業の間の吸着の探索の最大。
    pub snap_max: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Counts {
    pub nodes: u64,
    pub meshes: u64,
    pub triangles: u64,
    pub placed_triangles: u64,
}

impl Counts {
    pub fn from_stats(stats: ModelStats) -> Self {
        Self {
            nodes: u64::from(stats.nodes),
            meshes: u64::from(stats.meshes),
            triangles: stats.unique_triangles,
            placed_triangles: stats.placed_triangles,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportLine {
    pub label: &'static str,
    pub value: String,
    pub about: &'static str,
}

pub fn sample_memory() -> Option<u64> {
    memory_stats::memory_stats().and_then(|stats| u64::try_from(stats.physical_mem).ok())
}

pub fn report_lines(metrics: &Metrics, counts: Option<Counts>) -> Vec<ReportLine> {
    let mut lines = vec![
        ReportLine {
            label: "ファイルサイズ",
            value: fmt_bytes(metrics.file_bytes),
            about: "読み取ったバイト数",
        },
        ReportLine {
            label: "ノード数／メッシュ数／三角形数",
            value: fmt_counts(counts),
            about: "GLBのノード、メッシュ、形状として持つ三角形。配置後は共有された形状を部品の数だけ数えた値",
        },
        ReportLine {
            label: "読み取り",
            value: fmt_ms(metrics.read),
            about: "fs::read の経過時間",
        },
        ReportLine {
            label: "識別の計算",
            value: fmt_ms(metrics.identity),
            about: "blake3 の経過時間",
        },
        ReportLine {
            label: "解析",
            value: fmt_ms(metrics.parse),
            about: "容器とJSONの解析から Model の完成までの経過時間",
        },
        ReportLine {
            label: "GPU転送（CPU側）",
            value: fmt_upload(metrics),
            about: "continue_upload の合計とフレーム数。GPUの処理時間ではない",
        },
        ReportLine {
            label: "初回表示まで",
            value: fmt_ms(metrics.first_present),
            about: "開く操作から最初の描画命令を送り終えるまで。画面に出た瞬間は含まない",
        },
        ReportLine {
            label: "テクスチャ表示まで",
            value: fmt_ms(metrics.texture_present),
            about: "開く操作から最後の画像を送り終えるまで",
        },
        ReportLine {
            label: "当たり判定の準備",
            value: fmt_ms(metrics.pick),
            about: "PickIndex::build の経過時間。表示とは並行",
        },
        ReportLine {
            label: "フレームのCPU時間",
            value: fmt_cpu(metrics),
            about: "App::ui 1回の経過時間。直近120回の平均と最大。GPUが追いつかないときは前の描画を待つ時間を含む。GPUの処理時間そのものではない",
        },
        ReportLine {
            label: "フレーム間隔",
            value: fmt_interval(metrics),
            about: "ui の開始から次の開始まで。直近120回の平均と最大。100ms以上の空きは除く。画面に出ている間は更新周期で頭打ち",
        },
        ReportLine {
            label: "編集の確定",
            value: fmt_ms(metrics.edit_commit),
            about: "直近の変形・数値の確定で、文書と履歴を書き換えた時間",
        },
        ReportLine {
            label: "取り消し・やり直し",
            value: fmt_ms(metrics.undo_redo),
            about: "直近の取り消し・やり直しで、文書とレビューを書き換えた時間",
        },
        ReportLine {
            label: "履歴",
            value: fmt_history(metrics),
            about: "取り消しとやり直しに持つ操作の大きさの見積もりと、取り消せる回数。上限は 64 MiB と 500 回",
        },
        ReportLine {
            label: "書き出し",
            value: fmt_export(metrics),
            about: "直近のGLB書き出しの工程ごとの時間（読み取り／組み立て／書き込み／確かめ）",
        },
        ReportLine {
            label: "吸着の探索",
            value: fmt_snap(metrics),
            about: "計測の点の候補を探す時間（当たり判定を含む）。直近と最大。8ms を超えると、かかった時間の4倍だけ移動に合わせた探索を休む",
        },
        ReportLine {
            label: "GPUの処理時間",
            value: "未計測".to_owned(),
            about: "初版では計らない",
        },
    ];
    // メモリは測れたときだけ出す。
    if let Some(bytes) = metrics.texture_peak_mem {
        lines.push(ReportLine {
            label: "読み込み中の最大の常駐メモリ",
            value: format!("{bytes} バイト"),
            about: "画像を読み込む間にフレームごとに測ったプロセスの常駐メモリの最大",
        });
    }
    if let Some(bytes) = metrics.export_peak_mem {
        lines.push(ReportLine {
            label: "書き出し中の最大メモリ",
            value: format!("{bytes} バイト"),
            about: "直近のGLB書き出し中に観測した physical_mem の最大",
        });
    }
    if let Some(bytes) = metrics.physical_mem {
        lines.push(ReportLine {
            label: "常駐メモリ",
            value: format!("{bytes} バイト"),
            about: "memory_stats の physical_mem（プロセスの常駐メモリ。GPU に置いた画像などは含まないことがあります）",
        });
    }
    lines
}

/// 自動操作の `report.json` に出す性能値。測っていないものは `value` が null。
pub fn report_value(metrics: &Metrics) -> serde_json::Value {
    serde_json::json!({
        "file_bytes": slot(metrics.file_bytes.map(|bytes| serde_json::json!(bytes)), "読み取ったバイト数"),
        "read_ms": slot(metrics.read.map(ms), "fs::read の経過時間"),
        "identity_ms": slot(metrics.identity.map(ms), "blake3 の経過時間"),
        "parse_ms": slot(metrics.parse.map(ms), "容器とJSONの解析から Model の完成までの経過時間"),
        "upload_cpu_ms": slot(metrics.upload_cpu.map(ms), "continue_upload の合計。GPUの処理時間ではない"),
        "upload_frames": slot(metrics.upload_frames.map(|frames| serde_json::json!(frames)), "continue_upload を呼んだ回数"),
        "first_present_ms": slot(metrics.first_present.map(ms), "開く操作から最初の描画命令を送り終えるまで。画面に出た瞬間は含まない"),
        "pick_ms": slot(metrics.pick.map(ms), "PickIndex::build の経過時間。表示とは並行"),
        "frame_cpu_avg_ms": slot(metrics.frame_cpu_avg.map(ms), "App::ui 1回の経過時間。直近120回の平均。前の描画を待つ時間を含む。GPUの処理時間そのものではない"),
        "frame_cpu_max_ms": slot(metrics.frame_cpu_max.map(ms), "App::ui 1回の経過時間。直近120回の最大。前の描画を待つ時間を含む。GPUの処理時間そのものではない"),
        "frame_interval_ms": slot(metrics.frame_interval.map(ms), "ui の開始から次の開始まで。直近120回の平均。100ms以上の空きは除く"),
        "frame_interval_max_ms": slot(metrics.frame_interval_max.map(ms), "ui の開始から次の開始まで。直近120回の最大。100ms以上の空きは除く"),
        "physical_mem": slot(metrics.physical_mem.map(|bytes| serde_json::json!(bytes)), "memory_stats の physical_mem（プロセスの常駐メモリ）"),
        "texture_present_ms": slot(metrics.texture_present.map(ms), "開く操作から最後の画像を送り終えるまで"),
        "texture_peak_mem": slot(metrics.texture_peak_mem.map(|bytes| serde_json::json!(bytes)), "画像を読み込む間に測った常駐メモリの最大"),
        "edit_commit_ms": slot(metrics.edit_commit.map(ms), "直近の変形・数値の確定で、文書と履歴を書き換えた時間"),
        "undo_redo_ms": slot(metrics.undo_redo.map(ms), "直近の取り消し・やり直しで、文書とレビューを書き換えた時間"),
        "history_bytes": slot(metrics.history_bytes.map(|bytes| serde_json::json!(bytes)), "履歴が持つ操作の大きさの見積もり"),
        "history_steps": slot(metrics.history_len.map(|steps| serde_json::json!(steps)), "取り消せる回数"),
        "export_read_ms": slot(metrics.export_read.map(ms), "直近の書き出しの読み取り時間"),
        "export_build_ms": slot(metrics.export_build.map(ms), "直近の書き出しの組み立て時間"),
        "export_write_ms": slot(metrics.export_write.map(ms), "直近の書き出しの書き込み時間"),
        "export_verify_ms": slot(metrics.export_verify.map(ms), "直近の書き出しの確かめ時間"),
        "export_peak_mem": slot(metrics.export_peak_mem.map(|bytes| serde_json::json!(bytes)), "直近の書き出し中に観測した physical_mem の最大"),
        "snap_ms": slot(metrics.snap_last.map(ms), "直近の吸着の探索（当たり判定を含む）"),
        "snap_max_ms": slot(metrics.snap_max.map(ms), "この作業の間の吸着の探索の最大"),
        "gpu_ms": slot(None, "初版では計らない"),
    })
}

fn slot(value: Option<serde_json::Value>, about: &'static str) -> serde_json::Value {
    serde_json::json!({ "value": value, "about": about })
}

fn ms(duration: Duration) -> serde_json::Value {
    serde_json::json!(duration.as_secs_f64() * 1000.0)
}

pub fn format_line(line: &ReportLine) -> String {
    format!("{}: {}（{}）", line.label, line.value, line.about)
}

pub fn log_stderr(metrics: &Metrics, counts: Option<Counts>) {
    for line in report_lines(metrics, counts) {
        eprintln!("{}", format_line(&line));
    }
}

/// `App::ui` の開始時刻と経過。`Metrics` には `Instant` を入れない。
#[derive(Clone, Debug, Default)]
pub struct FrameClock {
    cpu: VecDeque<Duration>,
    gaps: VecDeque<Duration>,
    last_start: Option<Instant>,
}

impl FrameClock {
    pub fn observe(&mut self, started: Instant, elapsed: Duration, metrics: &mut Metrics) {
        if let Some(previous) = self.last_start {
            let gap = started.saturating_duration_since(previous);
            if gap < CONTINUOUS_GAP {
                if self.gaps.len() == FRAME_WINDOW {
                    self.gaps.pop_front();
                }
                self.gaps.push_back(gap);
                metrics.frame_interval = average(&self.gaps);
                metrics.frame_interval_max = self.gaps.iter().copied().max();
            }
        }
        self.last_start = Some(started);
        if self.cpu.len() == FRAME_WINDOW {
            self.cpu.pop_front();
        }
        self.cpu.push_back(elapsed);
        metrics.frame_cpu_avg = average(&self.cpu);
        metrics.frame_cpu_max = self.cpu.iter().copied().max();
    }
}

fn average(values: &VecDeque<Duration>) -> Option<Duration> {
    let count = values.len() as u128;
    if count == 0 {
        return None;
    }
    let sum = values.iter().map(Duration::as_nanos).sum::<u128>();
    Some(Duration::from_nanos(
        u64::try_from(sum / count).unwrap_or(u64::MAX),
    ))
}

fn fmt_interval(metrics: &Metrics) -> String {
    match (metrics.frame_interval, metrics.frame_interval_max) {
        (Some(avg), Some(max)) => format!("平均 {}／最大 {}", fmt_ms(Some(avg)), fmt_ms(Some(max))),
        _ => "未計測".to_owned(),
    }
}

fn fmt_history(metrics: &Metrics) -> String {
    match (metrics.history_bytes, metrics.history_len) {
        (Some(bytes), Some(steps)) => format!("{bytes} バイト、{steps} 回"),
        _ => "未計測".to_owned(),
    }
}

fn fmt_export(metrics: &Metrics) -> String {
    match (
        metrics.export_read,
        metrics.export_build,
        metrics.export_write,
        metrics.export_verify,
    ) {
        (None, None, None, None) => "未計測".to_owned(),
        (read, build, write, verify) => format!(
            "読み取り {}／組み立て {}／書き込み {}／確かめ {}",
            fmt_ms(read),
            fmt_ms(build),
            fmt_ms(write),
            fmt_ms(verify),
        ),
    }
}

fn fmt_snap(metrics: &Metrics) -> String {
    match (metrics.snap_last, metrics.snap_max) {
        (Some(last), Some(max)) => {
            format!("直近 {}／最大 {}", fmt_ms(Some(last)), fmt_ms(Some(max)))
        }
        _ => "未計測".to_owned(),
    }
}

fn fmt_upload(metrics: &Metrics) -> String {
    match (metrics.upload_cpu, metrics.upload_frames) {
        (None, None) => "未計測".to_owned(),
        (cpu, frames) => format!("{}、{} 回", fmt_ms(cpu), fmt_frames(frames)),
    }
}

fn fmt_cpu(metrics: &Metrics) -> String {
    match (metrics.frame_cpu_avg, metrics.frame_cpu_max) {
        (Some(avg), Some(max)) => format!("平均 {}／最大 {}", fmt_ms(Some(avg)), fmt_ms(Some(max))),
        _ => "未計測".to_owned(),
    }
}

fn fmt_counts(counts: Option<Counts>) -> String {
    match counts {
        Some(counts) => format!(
            "{}／{}／{}（配置後 {}）",
            counts.nodes, counts.meshes, counts.triangles, counts.placed_triangles
        ),
        None => "未計測".to_owned(),
    }
}

fn fmt_ms(value: Option<Duration>) -> String {
    match value {
        Some(duration) => format!("{:.1} ms", duration.as_secs_f64() * 1000.0),
        None => "未計測".to_owned(),
    }
}

fn fmt_bytes(value: Option<u64>) -> String {
    match value {
        Some(bytes) => format!("{bytes} バイト"),
        None => "未計測".to_owned(),
    }
}

fn fmt_frames(value: Option<u64>) -> String {
    match value {
        Some(frames) => frames.to_string(),
        None => "未計測".to_owned(),
    }
}
