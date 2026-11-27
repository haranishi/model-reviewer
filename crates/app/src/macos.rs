//! 既定メニューの「終了」は `terminate:` を送り、閉じる要求を経由しない。
//! `performClose:` に付け替えると、閉じるボタンと同じ未保存の確認に入る。
//! メニュー自体は残す。

#![cfg(target_os = "macos")]

use objc2::MainThreadMarker;
use objc2::sel;
use objc2_app_kit::{NSMenu, NSMenuItem};

/// 付け替えまで終えたら `true`。メニューがまだ無ければ `false`。
/// `final_attempt` のときだけ、メニューが無いこととメインスレッド以外を警告する。
pub(crate) fn retarget_quit_menu(final_attempt: bool) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        if final_attempt {
            log::warn!("終了メニューの付け替えはメインスレッドでだけ行う");
        }
        return false;
    };
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
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
    use super::retarget_quit_menu;

    #[test]
    fn retarget_quit_menu_without_menu_does_not_panic() {
        // テストプロセスには winit の既定メニューが無い。メインスレッド以外では AppKit を触らない。
        let _ = retarget_quit_menu(false);
    }
}
