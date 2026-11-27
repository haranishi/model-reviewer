//! 手順JSONの読み取り。実行は親モジュール。

use std::fs;
use std::path::{Component, Path};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use super::{
    DEFAULT_DRAG_FRAMES, DEFAULT_TIMEOUT, DEFAULT_TOLERANCE, Op, PathBase, PathStep, ScriptRun,
    Step,
};

pub(in crate::automation) fn load_script(script: &Path, out: &Path) -> Result<ScriptRun, String> {
    let text =
        fs::read_to_string(script).map_err(|err| format!("手順を読めませんでした: {err}"))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|err| format!("手順のJSONを解釈できません: {err}"))?;
    let root = value
        .as_object()
        .ok_or_else(|| "手順はオブジェクトです".to_owned())?;
    reject_unknown(root, &["timeout_secs", "steps"])?;
    let default_timeout = match root.get("timeout_secs") {
        Some(value) => parse_timeout(value)?,
        None => DEFAULT_TIMEOUT,
    };
    let steps_value = root
        .get("steps")
        .and_then(Value::as_array)
        .ok_or_else(|| "steps が必要です".to_owned())?;
    let mut steps = Vec::with_capacity(steps_value.len());
    for (index, step) in steps_value.iter().enumerate() {
        steps.push(parse_step(step).map_err(|err| format!("steps[{index}]: {err}"))?);
    }
    fs::create_dir_all(out).map_err(|err| format!("出力フォルダを作れませんでした: {err}"))?;
    let script_dir = script
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    Ok(ScriptRun {
        default_timeout,
        script_dir,
        out_dir: out.to_path_buf(),
        steps,
        index: 0,
        step_started: None,
        results: Vec::new(),
        wait: super::Wait::None,
        drag: None,
        shot: None,
        shot_path: None,
        inject_pending: false,
        staged: Vec::new(),
        waiting_note: None,
        settling: None,
        focus_seen: false,
        sample_waited: 0,
        samples: Vec::new(),
        finished: false,
        ok: false,
        started: Instant::now(),
    })
}

fn parse_step(value: &Value) -> Result<Step, String> {
    let map = value
        .as_object()
        .ok_or_else(|| "手順はオブジェクトです".to_owned())?;
    let op_name = map
        .get("op")
        .and_then(Value::as_str)
        .ok_or_else(|| "op が必要です".to_owned())?;
    let timeout = match map.get("timeout_secs") {
        Some(value) => Some(parse_timeout(value)?),
        None => None,
    };
    let op = match op_name {
        "open_model" => {
            reject_unknown(
                map,
                &["op", "timeout_secs", "path", "relative_to", "stl_unit"],
            )?;
            Op::OpenModel {
                path: parse_path(map, PathBase::Script)?,
                stl_unit: parse_stl_unit(map.get("stl_unit"))?,
            }
        }
        "load_review" => {
            reject_unknown(map, &["op", "timeout_secs", "path", "relative_to"])?;
            Op::LoadReview(parse_path(map, PathBase::Script)?)
        }
        "save_review" => {
            reject_unknown(map, &["op", "timeout_secs", "path", "relative_to"])?;
            Op::SaveReview(parse_path(map, PathBase::Out)?)
        }
        "export_glb" => {
            reject_unknown(map, &["op", "timeout_secs", "path", "relative_to"])?;
            Op::ExportGlb(parse_path(map, PathBase::Out)?)
        }
        "wait_idle" => {
            reject_unknown(map, &["op", "timeout_secs"])?;
            Op::WaitIdle
        }
        "drag_view" => {
            reject_unknown(
                map,
                &[
                    "op",
                    "timeout_secs",
                    "from",
                    "to",
                    "button",
                    "modifiers",
                    "frames",
                    "sample_every",
                ],
            )?;
            Op::DragView {
                from: parse_pair(map, "from")?,
                to: parse_pair(map, "to")?,
                button: parse_button(map.get("button"))?,
                modifiers: parse_modifiers(map.get("modifiers"))?,
                frames: parse_frames(map.get("frames"))?,
                sample_every: parse_count(map.get("sample_every"), "sample_every")?,
            }
        }
        "scroll_view" => {
            reject_unknown(
                map,
                &["op", "timeout_secs", "at", "delta", "unit", "modifiers"],
            )?;
            Op::ScrollView {
                at: parse_pair(map, "at")?,
                delta: parse_pair(map, "delta")?,
                unit: parse_wheel_unit(map.get("unit"))?,
                modifiers: parse_modifiers(map.get("modifiers"))?,
            }
        }
        "click_view" => {
            reject_unknown(map, &["op", "timeout_secs", "at", "button", "modifiers"])?;
            Op::ClickView {
                at: parse_pair(map, "at")?,
                button: parse_button(map.get("button"))?,
                modifiers: parse_modifiers(map.get("modifiers"))?,
            }
        }
        "hover_view" => {
            reject_unknown(map, &["op", "timeout_secs", "at"])?;
            Op::HoverView {
                at: parse_pair(map, "at")?,
            }
        }
        "click_widget" => {
            reject_unknown(map, &["op", "timeout_secs", "name"])?;
            Op::ClickWidget {
                name: required_string(map, "name")?,
            }
        }
        "drag_widget" => {
            reject_unknown(
                map,
                &[
                    "op",
                    "timeout_secs",
                    "name",
                    "delta",
                    "button",
                    "modifiers",
                    "frames",
                    "sample_every",
                ],
            )?;
            Op::DragWidget {
                name: required_string(map, "name")?,
                delta: parse_pair(map, "delta")?,
                button: parse_button(map.get("button"))?,
                modifiers: parse_modifiers(map.get("modifiers"))?,
                frames: parse_frames(map.get("frames"))?,
                sample_every: parse_count(map.get("sample_every"), "sample_every")?,
            }
        }
        "type_text" => {
            reject_unknown(map, &["op", "timeout_secs", "text"])?;
            Op::TypeText {
                text: required_string(map, "text")?,
            }
        }
        "key" => {
            reject_unknown(map, &["op", "timeout_secs", "key", "modifiers"])?;
            Op::Key {
                key: parse_key(&required_string(map, "key")?)?,
                modifiers: parse_modifiers(map.get("modifiers"))?,
            }
        }
        "screenshot" => {
            reject_unknown(map, &["op", "timeout_secs", "path", "relative_to"])?;
            Op::Screenshot(parse_path(map, PathBase::Out)?)
        }
        "dump_state" => {
            reject_unknown(map, &["op", "timeout_secs", "path", "relative_to"])?;
            Op::DumpState(parse_path(map, PathBase::Out)?)
        }
        "assert_state_equals" => {
            reject_unknown(
                map,
                &["op", "timeout_secs", "path", "relative_to", "tolerance"],
            )?;
            Op::AssertState {
                path: parse_path(map, PathBase::Script)?,
                tolerance: parse_tolerance(map.get("tolerance"))?,
            }
        }
        "sample" => {
            reject_unknown(map, &["op", "timeout_secs", "label", "frames"])?;
            Op::Sample {
                label: required_string(map, "label")?,
                frames: parse_count(map.get("frames"), "frames")?,
            }
        }
        "quit" => {
            reject_unknown(map, &["op", "timeout_secs"])?;
            Op::Quit
        }
        other => return Err(format!("未知の op です: {other}")),
    };
    Ok(Step { op, timeout })
}

/// 手順の中のパスは相対だけ。Windows のルート始まり（`/a`）とドライブ指定（`C:a`）も断る。
pub(super) fn is_plain_relative(path: &str) -> bool {
    !path.is_empty()
        && !Path::new(path)
            .components()
            .any(|part| matches!(part, Component::Prefix(_) | Component::RootDir))
}

fn parse_path(map: &Map<String, Value>, default_base: PathBase) -> Result<PathStep, String> {
    let path = required_string(map, "path")?;
    if !is_plain_relative(&path) {
        return Err("path は相対パスです".to_owned());
    }
    let base = match map.get("relative_to") {
        None => default_base,
        Some(Value::String(text)) if text == "script" => PathBase::Script,
        Some(Value::String(text)) if text == "out" => PathBase::Out,
        _ => return Err("relative_to は script か out です".to_owned()),
    };
    Ok(PathStep { path, base })
}

fn parse_timeout(value: &Value) -> Result<Duration, String> {
    let secs = value
        .as_f64()
        .ok_or_else(|| "timeout_secs は数です".to_owned())?;
    if !secs.is_finite() || secs < 0.0 {
        return Err("timeout_secs は0以上の数です".to_owned());
    }
    Duration::try_from_secs_f64(secs).map_err(|_| "timeout_secs が大きすぎます".to_owned())
}

fn parse_pair(map: &Map<String, Value>, key: &str) -> Result<[f32; 2], String> {
    let array = map
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{key} は数値2つの配列です"))?;
    if array.len() != 2 {
        return Err(format!("{key} は数値2つの配列です"));
    }
    Ok([parse_f32(&array[0], key)?, parse_f32(&array[1], key)?])
}

fn parse_f32(value: &Value, key: &str) -> Result<f32, String> {
    let number = value.as_f64().ok_or_else(|| format!("{key} は数です"))?;
    if !number.is_finite() || number.abs() > f32::MAX as f64 {
        return Err(format!("{key} は有限の数です"));
    }
    Ok(number as f32)
}

/// 0以上の回数（省略は0）。
fn parse_count(value: Option<&Value>, name: &str) -> Result<u32, String> {
    let Some(value) = value else {
        return Ok(0);
    };
    value
        .as_u64()
        .filter(|number| *number < 100_000)
        .map(|number| number as u32)
        .ok_or_else(|| format!("{name} は0以上の整数です"))
}

fn parse_frames(value: Option<&Value>) -> Result<u32, String> {
    let Some(value) = value else {
        return Ok(DEFAULT_DRAG_FRAMES);
    };
    let number = value
        .as_u64()
        .ok_or_else(|| "frames は2以上の整数です".to_owned())?;
    if !(2..100_000).contains(&number) {
        return Err("frames は2以上の整数です".to_owned());
    }
    Ok(number as u32)
}

fn parse_button(value: Option<&Value>) -> Result<egui::PointerButton, String> {
    let Some(value) = value else {
        return Ok(egui::PointerButton::Primary);
    };
    let name = value
        .as_str()
        .ok_or_else(|| "button は文字列です".to_owned())?;
    match name {
        "primary" | "left" => Ok(egui::PointerButton::Primary),
        "secondary" | "right" => Ok(egui::PointerButton::Secondary),
        "middle" => Ok(egui::PointerButton::Middle),
        "extra1" => Ok(egui::PointerButton::Extra1),
        "extra2" => Ok(egui::PointerButton::Extra2),
        _ => Err(format!("未知のボタンです: {name}")),
    }
}

fn parse_modifiers(value: Option<&Value>) -> Result<egui::Modifiers, String> {
    let Some(value) = value else {
        return Ok(egui::Modifiers::NONE);
    };
    let names = value
        .as_array()
        .ok_or_else(|| "modifiers は文字列の配列です".to_owned())?;
    let mut modifiers = egui::Modifiers::NONE;
    for name in names {
        let name = name
            .as_str()
            .ok_or_else(|| "modifiers は文字列の配列です".to_owned())?;
        match name {
            "shift" => modifiers.shift = true,
            "ctrl" | "control" => {
                modifiers.ctrl = true;
                if !cfg!(target_os = "macos") {
                    modifiers.command = true;
                }
            }
            "alt" | "option" => modifiers.alt = true,
            "command" | "cmd" | "meta" => {
                modifiers.command = true;
                modifiers.mac_cmd = cfg!(target_os = "macos");
            }
            _ => return Err(format!("未知の修飾キーです: {name}")),
        }
    }
    Ok(modifiers)
}

fn parse_stl_unit(
    value: Option<&Value>,
) -> Result<Option<model_reviewer_core::units::UnitLabel>, String> {
    use model_reviewer_core::units::UnitLabel;
    let Some(value) = value else {
        return Ok(None);
    };
    match value.as_str() {
        Some("mm") => Ok(Some(UnitLabel::Mm)),
        Some("cm") => Ok(Some(UnitLabel::Cm)),
        Some("m") => Ok(Some(UnitLabel::M)),
        _ => Err("stl_unit は mm、cm、m のいずれかです".to_owned()),
    }
}

fn parse_wheel_unit(value: Option<&Value>) -> Result<egui::MouseWheelUnit, String> {
    let Some(value) = value else {
        return Ok(egui::MouseWheelUnit::Line);
    };
    match value.as_str() {
        Some("point") => Ok(egui::MouseWheelUnit::Point),
        Some("line") => Ok(egui::MouseWheelUnit::Line),
        Some("page") => Ok(egui::MouseWheelUnit::Page),
        _ => Err("unit は point、line、page のいずれかです".to_owned()),
    }
}

fn parse_key(name: &str) -> Result<egui::Key, String> {
    if let Some(key) = egui::Key::from_name(name) {
        return Ok(key);
    }
    let mut chars = name.chars();
    let titled = match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    };
    egui::Key::from_name(&titled).ok_or_else(|| format!("未知のキーです: {name}"))
}

fn parse_tolerance(value: Option<&Value>) -> Result<f64, String> {
    let Some(value) = value else {
        return Ok(DEFAULT_TOLERANCE);
    };
    let number = value
        .as_f64()
        .ok_or_else(|| "tolerance は0以上の数です".to_owned())?;
    if !number.is_finite() || number < 0.0 {
        return Err("tolerance は0以上の数です".to_owned());
    }
    Ok(number)
}

fn required_string(map: &Map<String, Value>, key: &str) -> Result<String, String> {
    map.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{key} が必要です"))
}

fn reject_unknown(map: &Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    for key in map.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("未知の項目です: {key}"));
        }
    }
    Ok(())
}
