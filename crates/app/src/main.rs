//! 起動。引数は標準ライブラリだけで読む。
//!
//! - eframe の既定のGPU上限（`max_buffer_size` 256MiB）を、アダプタが許す値まで引き上げる。
//! - 日本語フォントは `fonts::install` が言語 "ja" で登録するので、eframe の自動登録は切る。
//! - GPUが無くウィンドウを作れないときは、理由を出して終了コード1で終わる（パニックしない）。

use std::sync::Arc;

use eframe::egui;
use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, wgpu};

fn main() {
    install_logger();
    let launch = match model_reviewer::parse_args(std::env::args_os()) {
        Ok(launch) => launch,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(2);
        }
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("model-reviewer")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([960.0, 600.0]);
    if launch.script.is_some() {
        viewport = viewport.with_active(false);
    }
    let options = eframe::NativeOptions {
        viewport,
        system_font_fallback: false,
        wgpu_options: wgpu_options(),
        ..Default::default()
    };
    if let Err(err) = eframe::run_native(
        "model-reviewer",
        options,
        Box::new(move |cc| Ok(Box::new(model_reviewer::App::new(cc, launch)))),
    ) {
        eprintln!("起動できませんでした（GPUが使えない可能性があります）: {err}");
        std::process::exit(1);
    }
    let code = model_reviewer::automation::exit_code();
    if code != 0 {
        std::process::exit(code);
    }
}

/// 既定の設定の `device_descriptor` を包み、バッファの上限をアダプタの値まで引き上げる。
/// 線の表示（`PolygonMode::Line`）に要る `POLYGON_MODE_LINE` は、アダプタが持っていれば要求する。
/// 持っていなければ要求せず、描画器の `supports_lines()` が偽になって画面が理由を出す。
fn wgpu_options() -> WgpuConfiguration {
    let mut config = WgpuConfiguration::default();
    if let WgpuSetup::CreateNew(create) = &mut config.wgpu_setup {
        let base = Arc::clone(&create.device_descriptor);
        create.device_descriptor = Arc::new(move |adapter: &wgpu::Adapter| {
            let mut desc = base(adapter);
            let allowed = adapter.limits();
            desc.required_limits.max_buffer_size = desc
                .required_limits
                .max_buffer_size
                .max(allowed.max_buffer_size);
            if adapter
                .features()
                .contains(wgpu::Features::POLYGON_MODE_LINE)
            {
                desc.required_features |= wgpu::Features::POLYGON_MODE_LINE;
            }
            desc
        });
    }
    config
}

/// 標準エラー出力へのログ。`MODEL_REVIEWER_LOG=debug` などで細かくできる。既定は警告以上。
fn install_logger() {
    struct Stderr;
    impl log::Log for Stderr {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.level() <= log::max_level()
        }
        fn log(&self, record: &log::Record<'_>) {
            if self.enabled(record.metadata()) {
                eprintln!(
                    "[{}] {}: {}",
                    record.level(),
                    record.target(),
                    record.args()
                );
            }
        }
        fn flush(&self) {}
    }
    static LOGGER: Stderr = Stderr;
    let level = std::env::var("MODEL_REVIEWER_LOG")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(log::LevelFilter::Warn);
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(level);
    }
}
