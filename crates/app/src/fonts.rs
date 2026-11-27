//! 日本語フォント。フォントは同梱せず、OSの日本語フォントを欧文より先に使う。
//!
//! eframe の自動登録（`system_font_fallback`）は、漢字のフォントをOSの言語設定で選ぶ。日本語以外の設定の
//! Macでは中国語の字形になりうるので、自動登録を切り、言語を "ja" に固定する。
//! 付属の欧文フォントを先に置くと、英数字だけ高さが違い下付き文字のように見える。
//! `FontProvider` が見つけたフォントは最後の順位にしか入らないので、起動時に「部」へ合うフォントを
//! `FontPriority::Highest` で比例フォントの先頭へ入れる。句読点の字形は実機の画像で確かめる。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::{FallbackRequest, FontFamily, FontInsert, FontPriority, FontProvider, InsertFontFamily};
use egui_system_font_provider::SystemFontProvider;

/// アプリとテストの両方から呼ぶ。日本語フォントが無くても起動し、警告は1回だけ出す。
pub fn install(ctx: &egui::Context) {
    let warned = Arc::new(AtomicBool::new(false));
    let provider = SystemFontProvider::new().with_locale(Some("ja"));
    promote_japanese(ctx, &provider, &warned);
    ctx.add_font_provider(Arc::new(WarnOnMissingJapanese {
        inner: provider,
        warned,
    }));
}

/// 「部」を描ける日本語フォントを、比例フォントの最優先にする。等幅は最後の順位。
fn promote_japanese(ctx: &egui::Context, provider: &SystemFontProvider, warned: &AtomicBool) {
    let family = FontFamily::Proportional;
    let request = FallbackRequest {
        cluster: "部",
        family: &family,
    };
    let Some(mut font) = provider.font_for(&request) else {
        warn_once(warned, "部");
        return;
    };
    font.families = vec![
        InsertFontFamily {
            family: FontFamily::Proportional,
            priority: FontPriority::Highest,
        },
        InsertFontFamily {
            family: FontFamily::Monospace,
            priority: FontPriority::Lowest,
        },
    ];
    ctx.add_font(font);
}

fn warn_once(warned: &AtomicBool, cluster: &str) {
    if !warned.swap(true, Ordering::Relaxed) {
        log::warn!("日本語のフォントがOSに見つからないため、文字が表示されない: {cluster:?}");
    }
}

/// 日本語の文字に合うフォントがOSに無いとき、1回だけログへ警告する。
struct WarnOnMissingJapanese {
    inner: SystemFontProvider,
    warned: Arc<AtomicBool>,
}

impl FontProvider for WarnOnMissingJapanese {
    fn fonts_for_family(&self, family: &egui::FontFamily) -> Vec<FontInsert> {
        self.inner.fonts_for_family(family)
    }

    fn font_for(&self, request: &FallbackRequest<'_>) -> Option<FontInsert> {
        let found = self.inner.font_for(request);
        if found.is_none() && request.cluster.chars().next().is_some_and(is_japanese) {
            warn_once(&self.warned, request.cluster);
        }
        found
    }
}

/// かな・漢字・全角の句読点。
pub fn is_japanese(c: char) -> bool {
    matches!(c,
        '\u{3000}'..='\u{303F}'   // 句読点・かっこ
        | '\u{3040}'..='\u{30FF}' // ひらがな・カタカナ
        | '\u{4E00}'..='\u{9FFF}' // 漢字
        | '\u{FF00}'..='\u{FFEF}' // 全角の英数と記号
    )
}

#[cfg(test)]
mod tests {
    use super::is_japanese;

    #[test]
    fn japanese_ranges() {
        for c in ['、', '。', '「', '」', 'あ', 'ア', '部', '（', '）'] {
            assert!(is_japanese(c), "{c}");
        }
        assert!(!is_japanese('a'));
        assert!(is_japanese('・') && is_japanese('ー'));
    }
}
