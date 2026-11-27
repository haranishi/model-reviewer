//! キーで動かす: 向きの固定と数値の入力、矢印で少しずつ（要件11・12）。変形の仕組みは増やさず、
//! いまの変形（差分の行列 → 確定）の3つ目の出どころ（`TransformSource::Keys`）になる。
//!
//! 約束: 打った文字は `KeyEntry.text` に持ち、文書は確定（`CommitTransform`）まで書き換えない。
//! 移動の量は表示の単位で打ち、シーンの値に直してから `key_delta` へ渡す。読めない量や 0 以下の
//! 倍率では差分を変えず、理由を状態欄に出す。確定の直前にも同じ計算をする。キーの振り分けは
//! `app::shortcut_commands` が行い、ここは `Command` を受けるだけ。

use glam::{Affine3A, Vec3};
use model_reviewer_core::camera::grid_spacing;
use model_reviewer_core::gizmo::{Handle, Tool, apply_world_delta, key_delta};
use model_reviewer_core::section::Axis;

use crate::command::KeyInput;

use super::Session;
use super::edit::{KeyEntry, Transform, TransformSource, commit_local_matrix, gizmo_frame};

/// 打てる文字の数の上限。
pub const MAX_TEXT: usize = 32;
/// 有効数字の桁数（`format_amount`）。
const SIGNIFICANT: i32 = 6;

const BAD_AMOUNT: &str = "数値として読めません";
const BAD_FACTOR: &str = "倍率は 0 より大きい数にしてください";
const NOT_FINITE: &str = "量が大きすぎて動かせません";
const HOW_TO: &str = "Enter で確定・Esc でやめる";

/// 打った文字を量に読む。空と "-" は `Some(0.0)`。数字・小数点・負号以外は `None`。
/// 負号は先頭の1つだけ、小数点は1つだけ。"." と "-." も 0 と読む（打っている途中の形）。
pub fn parse_amount(text: &str) -> Option<f64> {
    let body = text.strip_prefix('-').unwrap_or(text);
    if !body.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    if body.chars().filter(|c| *c == '.').count() > 1 {
        return None;
    }
    if !body.chars().any(|c| c.is_ascii_digit()) {
        return Some(0.0);
    }
    let value: f64 = text.parse().ok()?;
    value.is_finite().then_some(value)
}

/// 有効6桁。末尾の 0 と小数点を落とす。有限でなければ "0"。
pub fn format_amount(value: f64) -> String {
    if !value.is_finite() || value == 0.0 {
        return "0".to_owned();
    }
    let magnitude = value.abs().log10().floor() as i32;
    let decimals = (SIGNIFICANT - 1 - magnitude).clamp(0, 15) as usize;
    let mut text = format!("{value:.decimals$}");
    if text.contains('.') {
        let trimmed = text.trim_end_matches('0').trim_end_matches('.').len();
        text.truncate(trimmed);
    }
    if text == "-0" { "0".to_owned() } else { text }
}

/// 矢印の1歩。移動は格子（シーンの単位）、回転は 15・1（度）、拡大は 0.1・0.01。
/// 格子が有限の正の数でなければ 1 として扱う。
pub fn step_of(handle: Handle, grid: f32, fine: bool) -> f64 {
    let scale = if fine { 0.1 } else { 1.0 };
    match handle {
        Handle::MoveX | Handle::MoveY | Handle::MoveZ => {
            let grid = if grid.is_finite() && grid > 0.0 {
                f64::from(grid)
            } else {
                1.0
            };
            grid * scale
        }
        Handle::RotX | Handle::RotY | Handle::RotZ => {
            if fine {
                1.0
            } else {
                15.0
            }
        }
        Handle::ScaleX | Handle::ScaleY | Handle::ScaleZ | Handle::ScaleAll => 0.1 * scale,
    }
}

/// いまの道具と軸のハンドル。移動・回転・拡大の道具でなければ `None`。
fn handle_for(tool: Tool, axis: Axis) -> Option<Handle> {
    let handles = match tool {
        Tool::Move => [Handle::MoveX, Handle::MoveY, Handle::MoveZ],
        Tool::Rotate => [Handle::RotX, Handle::RotY, Handle::RotZ],
        Tool::Scale => [Handle::ScaleX, Handle::ScaleY, Handle::ScaleZ],
        _ => return None,
    };
    Some(handles[axis.index()])
}

pub(crate) fn axis_letter(handle: Handle) -> &'static str {
    match handle {
        Handle::MoveX | Handle::RotX | Handle::ScaleX | Handle::ScaleAll => "X",
        Handle::MoveY | Handle::RotY | Handle::ScaleY => "Y",
        Handle::MoveZ | Handle::RotZ | Handle::ScaleZ => "Z",
    }
}

fn is_move(handle: Handle) -> bool {
    matches!(handle, Handle::MoveX | Handle::MoveY | Handle::MoveZ)
}

fn is_scale(handle: Handle) -> bool {
    matches!(
        handle,
        Handle::ScaleX | Handle::ScaleY | Handle::ScaleZ | Handle::ScaleAll
    )
}

/// 空のときの量（移動と回転は 0、拡大は倍率 1）。
fn neutral(handle: Handle) -> f64 {
    if is_scale(handle) { 1.0 } else { 0.0 }
}

/// 打った文字から、`key_delta` に渡す量（移動はシーンの単位）。空は変化なしの量。
fn scene_amount(session: &Session, entry: &KeyEntry) -> Result<f32, &'static str> {
    let has_digit = entry.text.chars().any(|c| c.is_ascii_digit());
    let typed = parse_amount(&entry.text).ok_or(BAD_AMOUNT)?;
    let typed = if has_digit {
        typed
    } else {
        neutral(entry.handle)
    };
    if is_scale(entry.handle) && typed <= 0.0 {
        return Err(BAD_FACTOR);
    }
    let value = typed as f32;
    let value = if is_move(entry.handle) {
        session.units.from_display(value)
    } else {
        value
    };
    if value.is_finite() {
        Ok(value)
    } else {
        Err(NOT_FINITE)
    }
}

/// 距離に添える単位（表示が「ファイルと同じ」なら付けない。変化量の文字と同じ決まり）。
fn unit_suffix(session: &Session) -> String {
    session
        .units
        .display
        .map(|_| format!(" {}", session.units.shown().suffix()))
        .unwrap_or_default()
}

/// 量の文字（打った文字のまま。空は変化なしの量）と、単位や記号。
fn amount_text(session: &Session, handle: Handle, text: &str) -> String {
    let shown = if text.is_empty() {
        format_amount(neutral(handle))
    } else {
        text.to_owned()
    };
    if is_move(handle) {
        format!("{shown}{}", unit_suffix(session))
    } else if is_scale(handle) {
        format!("×{shown}")
    } else {
        format!("{shown}°")
    }
}

/// 1歩の文字（移動は表示の単位）。
fn step_text(session: &Session, handle: Handle) -> String {
    let step = step_display(session, handle, false);
    if is_move(handle) {
        format!("{}{}", format_amount(step), unit_suffix(session))
    } else if is_scale(handle) {
        format_amount(step)
    } else {
        format!("{}°", format_amount(step))
    }
}

/// 1歩を打つ量の単位で（移動は表示の単位へ換算する）。
fn step_display(session: &Session, handle: Handle, fine: bool) -> f64 {
    let grid = grid_spacing(session.camera.distance);
    let step = step_of(handle, grid, fine);
    if is_move(handle) {
        f64::from(session.units.to_display(step as f32))
    } else {
        step
    }
}

fn verb(handle: Handle) -> &'static str {
    if is_move(handle) {
        "移動"
    } else if is_scale(handle) {
        "拡大"
    } else {
        "回転"
    }
}

/// 状態欄の文（「移動 X: 12.5 mm ｜ Enter で確定・Esc でやめる・矢印で 100 mm ずつ」）。
/// `reason` があれば、案内の代わりに理由を出す。
pub(super) fn status_with(session: &Session, entry: &KeyEntry, reason: &str) -> String {
    let head = format!(
        "{} {}: {}",
        verb(entry.handle),
        axis_letter(entry.handle),
        amount_text(session, entry.handle, &entry.text)
    );
    if reason.is_empty() {
        format!(
            "{head} ｜ {HOW_TO}・矢印で {} ずつ",
            step_text(session, entry.handle)
        )
    } else {
        format!("{head} ｜ {reason}")
    }
}

/// 中心と軸（ハンドルと同じもの）。大きさは使わないので、1×1 の枠で作る。
fn pivot_and_axes(session: &Session) -> Option<(Vec3, [Vec3; 3])> {
    gizmo_frame(session, [1.0, 1.0], 1.0).map(|frame| (frame.pivot, frame.axes))
}

/// 打った文字から差分の行列と変化量を作り直し、状態欄を書く。読めなければ差分を変えない。
pub(super) fn rebuild(session: &mut Session) -> bool {
    let Some(Transform {
        source: TransformSource::Keys(entry),
        ..
    }) = session.transform.as_ref()
    else {
        return false;
    };
    let entry = entry.clone();
    let made = scene_amount(session, &entry).and_then(|value| {
        let (pivot, axes) = pivot_and_axes(session).ok_or(BAD_AMOUNT)?;
        key_delta(entry.handle, pivot, axes, value).ok_or(NOT_FINITE)
    });
    let reason = match made {
        Ok(state) => {
            if let Some(transform) = session.transform.as_mut() {
                if transform.delta != state.world_delta {
                    transform.delta = state.world_delta;
                    session.scene_dirty.drag = true;
                }
                transform.readout = Some(state.readout);
            }
            ""
        }
        Err(reason) => reason,
    };
    session.status_text = status_with(session, &entry, reason);
    reason.is_empty()
}

/// `Command::BeginKeyTransform`。いまの道具と軸からハンドルを決め、数値の入力を始める。
/// 途中なら軸だけ替える（打った量は残す）。ハンドルや数値の欄を動かしている途中なら何もしない。
pub(super) fn begin(session: &mut Session, axis: Axis) {
    let Some(handle) = handle_for(session.tool, axis) else {
        return;
    };
    if session.selection.is_none() || session.block_edit_if_locked() {
        return;
    }
    match session.transform.as_mut() {
        Some(Transform {
            source: TransformSource::Keys(entry),
            ..
        }) => {
            entry.handle = handle;
        }
        Some(_) => return,
        None => {
            let Some(id) = session.selection else {
                return;
            };
            let Some(doc) = session.doc.as_ref() else {
                return;
            };
            let Some(key) = doc.key(id) else {
                return;
            };
            if pivot_and_axes(session).is_none() {
                return;
            }
            let nodes = doc.subtree(id);
            session.transform = Some(Transform {
                key,
                nodes,
                source: TransformSource::Keys(KeyEntry {
                    handle,
                    text: String::new(),
                }),
                delta: Affine3A::IDENTITY,
                readout: None,
            });
            session.scene_dirty.drag = true;
        }
    }
    rebuild(session);
}

/// `Command::KeyTransformInput`。打った文字を直し、差分を作り直す。数値の入力の途中でなければ何もしない。
pub(super) fn input(session: &mut Session, key: KeyInput) {
    if session.edits_locked() {
        return;
    }
    let units_step = match (&key, session.transform.as_ref()) {
        (
            KeyInput::Step { fine, .. },
            Some(Transform {
                source: TransformSource::Keys(entry),
                ..
            }),
        ) => step_display(session, entry.handle, *fine),
        _ => 0.0,
    };
    let Some(Transform {
        source: TransformSource::Keys(entry),
        ..
    }) = session.transform.as_mut()
    else {
        return;
    };
    match key {
        KeyInput::Char(c) => {
            let accept = match c {
                '0'..='9' => true,
                '.' => !entry.text.contains('.'),
                '-' => entry.text.is_empty(),
                _ => false,
            };
            if !accept || entry.text.chars().count() >= MAX_TEXT {
                return;
            }
            entry.text.push(c);
        }
        KeyInput::Backspace => {
            if entry.text.pop().is_none() {
                return;
            }
        }
        KeyInput::Step { up, .. } => {
            let has_digit = entry.text.chars().any(|c| c.is_ascii_digit());
            let now = match parse_amount(&entry.text) {
                Some(value) if has_digit => value,
                Some(_) => neutral(entry.handle),
                None => return,
            };
            let next = if up {
                now + units_step
            } else {
                now - units_step
            };
            entry.text = format_amount(next);
        }
    }
    rebuild(session);
}

/// `Command::Nudge`。移動の道具で、選択があり、変形の途中でないとき、ワールドの軸に沿って
/// 格子1つぶん（`fine` なら10分の1）動かして確定する。位置を格子の目にはそろえない。
pub(super) fn nudge(session: &mut Session, axis: Axis, sign: i8, fine: bool) {
    if session.tool != Tool::Move
        || session.transform.is_some()
        || session.edits_locked()
        || sign == 0
    {
        return;
    }
    let Some(handle) = handle_for(Tool::Move, axis) else {
        return;
    };
    let Some(id) = session.selection else {
        return;
    };
    let Some((pivot, axes)) = pivot_and_axes(session) else {
        return;
    };
    let step = step_of(handle, grid_spacing(session.camera.distance), fine) as f32;
    let value = if sign < 0 { -step } else { step };
    let Some(state) = key_delta(handle, pivot, axes, value) else {
        return;
    };
    let Some(doc) = session.doc.as_ref() else {
        return;
    };
    let (Some(node), Some(key)) = (doc.node(id), doc.key(id)) else {
        return;
    };
    let parent_world = node
        .parent
        .and_then(|parent| doc.node(parent))
        .map_or(Affine3A::IDENTITY, |parent| parent.world);
    let before = node.local;
    let Some(after) = apply_world_delta(&parent_world, &node.world, &state.world_delta) else {
        return;
    };
    if commit_local_matrix(session, key, before, after, "移動") {
        let shown = f64::from(session.units.to_display(value));
        let sign = if shown < 0.0 { "−" } else { "+" };
        session.status_text = format!(
            "移動 {}: {sign}{}{}",
            axis_letter(handle),
            format_amount(shown.abs()),
            unit_suffix(session)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amount_parse_and_format_table() {
        let parse = [
            ("", Some(0.0)),
            ("-", Some(0.0)),
            (".", Some(0.0)),
            ("-.", Some(0.0)),
            ("12.5", Some(12.5)),
            ("-12.5", Some(-12.5)),
            ("0.25", Some(0.25)),
            (".5", Some(0.5)),
            ("3.", Some(3.0)),
            ("007", Some(7.0)),
            ("1.2.3", None),
            ("1-2", None),
            ("--1", None),
            ("1e3", None),
            ("+1", None),
            (" 1", None),
            ("１", None),
            ("1,5", None),
        ];
        for (text, expected) in parse {
            assert_eq!(parse_amount(text), expected, "{text:?}");
        }
        let format = [
            (0.0, "0"),
            (-0.0, "0"),
            (12.5, "12.5"),
            (-12.5, "-12.5"),
            (100.0, "100"),
            (0.1 + 0.2, "0.3"),
            (1.0 / 3.0, "0.333333"),
            (123456.7, "123457"),
            (1234567.0, "1234567"),
            (0.00012345678, "0.000123457"),
            (9.9999999, "10"),
            (-0.0000000001, "-0.0000000001"),
            (f64::NAN, "0"),
            (f64::INFINITY, "0"),
        ];
        for (value, expected) in format {
            assert_eq!(format_amount(value), expected, "{value}");
        }
        // 書いた文字は、読み直すと同じ値になる（有効6桁の範囲で）。
        for value in [12.5, -0.75, 1500.0, 0.001] {
            assert_eq!(parse_amount(&format_amount(value)), Some(value));
        }
    }

    #[test]
    fn step_follows_grid_and_fine() {
        assert_eq!(step_of(Handle::MoveX, 0.1, false), f64::from(0.1_f32));
        assert_eq!(step_of(Handle::MoveZ, 10.0, false), 10.0);
        assert!((step_of(Handle::MoveY, 10.0, true) - 1.0).abs() < 1.0e-12);
        assert_eq!(step_of(Handle::MoveX, f32::NAN, false), 1.0);
        assert_eq!(step_of(Handle::MoveX, 0.0, false), 1.0);
        assert_eq!(step_of(Handle::RotX, 0.1, false), 15.0);
        assert_eq!(step_of(Handle::RotZ, 0.1, true), 1.0);
        assert!((step_of(Handle::ScaleX, 0.1, false) - 0.1).abs() < 1.0e-12);
        assert!((step_of(Handle::ScaleY, 100.0, true) - 0.01).abs() < 1.0e-12);
        // 格子の間隔は、状態欄と描画が使う関数の値。
        let grid = grid_spacing(14.0);
        assert_eq!(step_of(Handle::MoveX, grid, false), f64::from(grid));
    }
}
