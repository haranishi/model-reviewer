//! 自動操作の主ウィンドウと終了メニューを設定する。
//! 既定メニューの「終了」は `terminate:` を送り、閉じる要求を経由しない。
//! `performClose:` に付け替えると、閉じるボタンと同じ未保存の確認に入る。
//! メニュー自体は残す。

#![cfg(target_os = "macos")]

use objc2::MainThreadMarker;
use objc2::sel;
use objc2_app_kit::{NSApplication, NSFloatingWindowLevel, NSMenu, NSMenuItem, NSView};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// 初回表示の前に自動操作の主ウィンドウを透明にし、クリックを下の画面に通す。
/// 手前の層に置いても、別の Space や全画面のアプリを使っている間は、画面への提示が省かれることがある。
pub fn hide_script_window(cc: &eframe::CreationContext<'_>) -> Result<(), &'static str> {
    let window = cc
        .winit_window()
        .ok_or("自動操作の主ウィンドウが見つかりません")?;
    MainThreadMarker::new().ok_or("自動操作の主ウィンドウはメインスレッドでのみ設定できます")?;
    let handle = window
        .window_handle()
        .map_err(|_| "自動操作の主ウィンドウのハンドルを取得できません")?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return Err("自動操作の主ウィンドウに AppKit のビューがありません");
    };
    // SAFETY: メインスレッドで、主ウィンドウから借りたハンドルが有効な間だけ NSView を参照する。
    let view = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
    let window = view
        .window()
        .ok_or("自動操作のビューにウィンドウがありません")?;
    window.setAlphaValue(0.0);
    window.setIgnoresMouseEvents(true);
    window.setLevel(NSFloatingWindowLevel);
    Ok(())
}

pub(crate) fn deactivate_if_active() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    if app.isActive() {
        app.deactivate();
    }
}

/// 付け替えまで終えたら `true`。メニューがまだ無ければ `false`。
/// `final_attempt` のときだけ、メニューが無いこととメインスレッド以外を警告する。
pub(crate) fn retarget_quit_menu(final_attempt: bool) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        if final_attempt {
            log::warn!("終了メニューの付け替えはメインスレッドでだけ行う");
        }
        return false;
    };
    let app = NSApplication::sharedApplication(mtm);
    let Some(menu) = app.mainMenu() else {
        if final_attempt {
            log::warn!("既定メニューが無く、終了の動作を付け替えられない");
        }
        return false;
    };
    let changed = retarget_menu(&menu);
    if changed == 0 {
        log::warn!("既定メニューに terminate: の終了項目が無い");
    } else {
        log::debug!("終了メニューを {changed} 件、performClose: に付け替えた");
    }
    true
}

fn retarget_menu(menu: &NSMenu) -> usize {
    let mut changed = 0;
    let count = menu.numberOfItems();
    for index in 0..count {
        let Some(item) = menu.itemAtIndex(index) else {
            continue;
        };
        if item.action() == Some(sel!(terminate:)) {
            set_perform_close(&item);
            changed += 1;
        }
        if let Some(submenu) = item.submenu() {
            changed += retarget_menu(&submenu);
        }
    }
    changed
}

/// `performClose:` は NSWindow が実装しているセレクタで、閉じるボタンと同じ経路になる。
/// 項目の target は既定の nil のままなので、キーウィンドウへ届く。
/// winit の `windowShouldClose:` は CloseRequested を出して false を返す。ここではウィンドウを閉じない。
/// 呼ぶのはメインスレッドだけで、対象は生きているメニュー項目。
fn set_perform_close(item: &NSMenuItem) {
    unsafe {
        item.setAction(Some(sel!(performClose:)));
    }
}

#[cfg(test)]
mod tests {
    use super::{hide_script_window, retarget_quit_menu};

    #[test]
    fn scripted_startup_requires_a_root_window() {
        let cc = eframe::CreationContext::_new_kittest(egui::Context::default());
        assert_eq!(
            hide_script_window(&cc),
            Err("自動操作の主ウィンドウが見つかりません")
        );
    }

    #[test]
    fn retarget_quit_menu_without_menu_does_not_panic() {
        // テストプロセスには winit の既定メニューが無い。メインスレッド以外では AppKit を触らない。
        let _ = retarget_quit_menu(false);
    }
}
