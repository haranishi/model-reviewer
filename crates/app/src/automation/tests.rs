//! 自動操作の試験。公開の入口と非公開の読み取り・比較を、親モジュール経由で呼ぶ。

use super::script::{is_plain_relative, load_script};
use super::state::{numbers_match, values_match};
use super::{Automation, Op, WidgetEntry, WidgetFrame, view_to_screen};
use crate::command::parse_args;
use crate::loader::LoadPhase;
use crate::session::Session;
use crate::ui::ids;
use serde_json::{Number, json};
use std::fs;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

#[test]
fn only_plain_relative_paths_are_accepted() {
    assert!(is_plain_relative("a.glb"));
    assert!(is_plain_relative("../assets/a.glb"));
    assert!(!is_plain_relative(""));
    assert!(!is_plain_relative("/tmp/a.glb"));
    #[cfg(windows)]
    for path in [r"C:\a.glb", "C:a.glb", r"\a.glb", r"\\server\share\a.glb"] {
        assert!(!is_plain_relative(path), "{path}");
    }
}

/// 終了コードはプロセス全体で1つなので、書くテストは同時に走らせない。
fn exit_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|err| err.into_inner())
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("model-reviewer-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn parses_steps_and_rejects_absolute_paths() {
    let dir = scratch("parse");
    let script = dir.join("steps.json");
    fs::write(
        &script,
        r#"{
              "timeout_secs": 5,
              "steps": [
                {"op": "open_model", "path": "model.glb", "timeout_secs": 2},
                {"op": "wait_idle"},
                {"op": "drag_view", "from": [0, 0.5], "to": [1, 0.25], "button": "right", "modifiers": ["shift"], "frames": 4},
                {"op": "scroll_view", "at": [0.5, 0.5], "delta": [0, -1]},
                {"op": "click_view", "at": [0.2, 0.8]},
                {"op": "hover_view", "at": [0.4, 0.6]},
                {"op": "click_widget", "name": "toolbar.fit_all"},
                {"op": "type_text", "text": "視点"},
                {"op": "key", "key": "enter", "modifiers": ["ctrl"]},
                {"op": "screenshot", "path": "shot.png"},
                {"op": "dump_state", "path": "state.json"},
                {"op": "assert_state_equals", "path": "state.json", "relative_to": "out", "tolerance": 0.01},
                {"op": "save_review", "path": "review.json"},
                {"op": "export_glb", "path": "edited.glb"},
                {"op": "load_review", "path": "review.json", "relative_to": "out"},
                {"op": "quit"}
              ]
            }"#,
    )
    .unwrap();
    let run = load_script(&script, &dir.join("out")).unwrap();
    assert_eq!(run.steps.len(), 16);
    assert!(matches!(run.steps[5].op, Op::HoverView { at } if at == [0.4, 0.6]));
    let extra = dir.join("hover-extra.json");
    fs::write(
        &extra,
        r#"{"steps":[{"op":"hover_view","at":[0.5,0.5],"button":"left"}]}"#,
    )
    .unwrap();
    assert!(
        load_script(&extra, &dir.join("out")).is_err(),
        "押さない手順にボタンは無い"
    );
    assert_eq!(run.default_timeout, Duration::from_secs(5));
    assert_eq!(run.steps[0].timeout, Some(Duration::from_secs(2)));

    let absolute = dir.join("bad.json");
    fs::write(
        &absolute,
        r#"{"steps":[{"op":"open_model","path":"/tmp/a.glb"}]}"#,
    )
    .unwrap();
    let err = match load_script(&absolute, &dir.join("out")) {
        Err(err) => err,
        Ok(_) => panic!("絶対パスは拒否する"),
    };
    assert!(err.contains("相対"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn compares_floats_with_tolerance_and_objects() {
    let tolerance = 1.0e-4;
    values_match(
        &json!({"camera": {"yaw": 1.0, "nested": [1.0, 2.0]}}),
        &json!({"camera": {"yaw": 1.00001, "nested": [1.0, 2.00002]}}),
        tolerance,
        "$",
    )
    .unwrap();
    let mismatch = values_match(&json!(1.0), &json!(1.1), tolerance, "$.yaw").unwrap_err();
    assert!(mismatch.contains("yaw"));
    assert!(numbers_match(&Number::from(1), &Number::from(1), tolerance));
    assert!(!numbers_match(
        &Number::from_f64(1.0).unwrap(),
        &Number::from_f64(2.0).unwrap(),
        tolerance
    ));
}

#[test]
fn converts_unit_view_coordinates() {
    assert_eq!(
        view_to_screen([0.0, 1.0], [10.0, 20.0], [110.0, 220.0]),
        Some([10.0, 220.0])
    );
    assert_eq!(
        view_to_screen([0.25, 0.75], [10.0, 20.0], [110.0, 220.0]),
        Some([35.0, 170.0])
    );
    assert_eq!(view_to_screen([1.1, 0.0], [0.0, 0.0], [1.0, 1.0]), None);
    assert_eq!(
        view_to_screen([0.0, f32::NAN], [0.0, 0.0], [1.0, 1.0]),
        None
    );
}

#[test]
fn injects_view_click_once_and_drops_real_pointer() {
    let _guard = exit_lock();
    let dir = scratch("inject");
    let script = dir.join("steps.json");
    fs::write(
        &script,
        r#"{"steps":[{"op":"click_view","at":[0.25,0.75],"button":"secondary","modifiers":["shift"]}]}"#,
    )
    .unwrap();
    let launch = parse_args([
        "model-reviewer",
        "--script",
        script.to_str().unwrap(),
        "--out",
        dir.join("out").to_str().unwrap(),
    ])
    .unwrap();
    let mut automation = Automation::from_launch(&launch);
    let ctx = egui::Context::default();
    ctx.data_mut(|data| {
        let mut frame = WidgetFrame::default();
        frame.push(WidgetEntry {
            name: ids::VIEWPORT.to_owned(),
            min: [10.0, 20.0],
            max: [110.0, 220.0],
        });
        data.insert_temp(egui::Id::NULL, frame);
    });
    let mut session = Session::default();
    automation.on_ui(&ctx, &mut session, false);

    let mut raw = egui::RawInput::default();
    raw.events
        .push(egui::Event::PointerMoved(egui::pos2(1.0, 1.0)));
    raw.events.push(egui::Event::Text("user".to_owned()));
    automation.inject(&ctx, &mut raw);
    assert!(raw.events.iter().all(|event| !matches!(
        event,
        egui::Event::Text(text) if text == "user"
    )));
    let presses: Vec<_> = raw
        .events
        .iter()
        .filter_map(|event| match event {
            egui::Event::PointerButton { pos, pressed, .. } => Some((*pos, *pressed)),
            _ => None,
        })
        .collect();
    assert_eq!(
        presses,
        vec![
            (egui::pos2(35.0, 170.0), true),
            (egui::pos2(35.0, 170.0), false)
        ]
    );

    let mut carried = raw.clone();
    carried
        .events
        .push(egui::Event::PointerMoved(egui::pos2(9.0, 9.0)));
    automation.inject(&ctx, &mut carried);
    let second: Vec<_> = carried
        .events
        .iter()
        .filter_map(|event| match event {
            egui::Event::PointerButton { pos, pressed, .. } => Some((*pos, *pressed)),
            _ => None,
        })
        .collect();
    assert_eq!(second, presses);
    assert!(!carried.events.iter().any(|event| matches!(
        event,
        egui::Event::PointerMoved(pos) if *pos == egui::pos2(9.0, 9.0)
    )));

    automation.on_ui(&ctx, &mut session, false);
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("out").join("report.json")).unwrap())
            .unwrap();
    assert_eq!(report["ok"], true);
    assert_eq!(report["steps"][0]["status"], "ok");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn waits_for_viewport_instead_of_failing() {
    let _guard = exit_lock();
    let dir = scratch("viewport");
    let script = dir.join("steps.json");
    fs::write(&script, r#"{"steps":[{"op":"click_view","at":[0.5,0.5]}]}"#).unwrap();
    let launch = parse_args([
        "model-reviewer",
        "--script",
        script.to_str().unwrap(),
        "--out",
        dir.join("out").to_str().unwrap(),
    ])
    .unwrap();
    let mut automation = Automation::from_launch(&launch);
    let ctx = egui::Context::default();
    let mut session = Session::default();
    automation.on_ui(&ctx, &mut session, false);
    automation.on_ui(&ctx, &mut session, false);
    assert!(
        !dir.join("out").join("report.json").is_file(),
        "viewport が無い間は失敗にしない"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn times_out_while_waiting_for_idle() {
    let _guard = exit_lock();
    let dir = scratch("timeout");
    let script = dir.join("steps.json");
    fs::write(
        &script,
        r#"{"timeout_secs":0,"steps":[{"op":"wait_idle"},{"op":"quit"}]}"#,
    )
    .unwrap();
    let launch = parse_args([
        "model-reviewer",
        "--script",
        script.to_str().unwrap(),
        "--out",
        dir.join("out").to_str().unwrap(),
    ])
    .unwrap();
    let mut automation = Automation::from_launch(&launch);
    let mut session = Session::default();
    session.metrics.read = Some(Duration::from_millis(12));
    session.metrics.file_bytes = Some(7);
    session.load = LoadPhase::Reading;
    let ctx = egui::Context::default();
    automation.on_logic(&ctx, &session, false);
    automation.on_ui(&ctx, &mut session, false);
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("out").join("report.json")).unwrap())
            .unwrap();
    assert_eq!(report["ok"], false);
    assert_eq!(report["steps"][0]["status"], "failed");
    assert_eq!(report["steps"][1]["status"], "skipped");
    assert!(
        report["steps"][0]["message"]
            .as_str()
            .unwrap()
            .contains("時間切れ")
    );
    let read_ms = report["metrics"]["read_ms"]["value"].as_f64().unwrap();
    assert!((read_ms - 12.0).abs() < 0.05);
    assert_eq!(report["metrics"]["file_bytes"]["value"], 7);
    assert!(report["metrics"]["physical_mem"]["value"].is_null());
    assert!(report["metrics"]["identity_ms"]["value"].is_null());
    assert!(report["metrics"]["gpu_ms"]["value"].is_null());
    assert!(
        report["metrics"]["read_ms"]["about"]
            .as_str()
            .unwrap()
            .contains("fs::read")
    );
    assert_eq!(super::exit_code(), 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn kittest_runs_dump_assert_and_quit() {
    let _guard = exit_lock();
    let dir = scratch("kittest");
    let script = dir.join("steps.json");
    let out = dir.join("out");
    fs::write(
        &script,
        r#"{"steps":[
                {"op":"dump_state","path":"state.json"},
                {"op":"assert_state_equals","path":"state.json","relative_to":"out"},
                {"op":"quit"}
            ]}"#,
    )
    .unwrap();
    let launch = parse_args([
        "model-reviewer",
        "--script",
        script.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ])
    .unwrap();
    let mut harness = egui_kittest::Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_max_steps(8)
        .build_eframe(|cc| crate::App::new(cc, launch));
    harness.run_steps(1);
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["ok"], true);
    assert_eq!(report["steps"].as_array().unwrap().len(), 3);
    assert!(out.join("state.json").is_file());
    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("state.json")).unwrap()).unwrap();
    assert!(state.get("camera").is_some());
    assert!(state["hidden"].as_array().unwrap().is_empty());
    // 第3段階の項目。既存の項目（`unit` など）はそのまま。
    assert_eq!(state["unit"], "m");
    assert_eq!(state["units"]["source"], "m");
    assert!(state["units"]["display"].is_null());
    assert!(state["units"]["stl"].is_null());
    assert_eq!(state["snap"]["enabled"], true);
    assert!(state["snap"]["candidate"].is_null());
    assert!(state["section"].is_null());
    assert!(state["measure"]["kinds"].as_array().unwrap().is_empty());
    assert!(state["measure"]["delta"].is_null());
    drop(harness);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn sample_waits_frames_and_lands_in_the_report() {
    let _guard = exit_lock();
    let dir = scratch("sample");
    let script = dir.join("steps.json");
    let out = dir.join("out");
    fs::write(
        &script,
        r#"{"steps":[
                {"op":"sample","label":"first"},
                {"op":"sample","label":"later","frames":3},
                {"op":"quit"}
            ]}"#,
    )
    .unwrap();
    let launch = parse_args([
        "model-reviewer",
        "--script",
        script.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ])
    .unwrap();
    let mut harness = egui_kittest::Harness::builder()
        .allow_missing_glyphs()
        .with_accessibility_check(false)
        .with_max_steps(16)
        .build_eframe(|cc| crate::App::new(cc, launch));
    harness.run_steps(8);
    let report: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(out.join("report.json")).unwrap()).unwrap();
    assert_eq!(report["ok"], true, "{report}");
    let samples = report["samples"].as_array().unwrap();
    let labels: Vec<_> = samples
        .iter()
        .map(|s| s["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, ["first", "later"]);
    assert!(samples[0].get("physical_mem").is_some());
    // 範囲外の値は読み込みで断る。
    fs::write(
        &script,
        r#"{"steps":[{"op":"sample","label":"x","frames":-1}]}"#,
    )
    .unwrap();
    assert!(load_script(&script, &out).is_err());
    fs::write(
        &script,
        r#"{"steps":[{"op":"drag_view","from":[0,0],"to":[1,1],"sample_every":"a"}]}"#,
    )
    .unwrap();
    assert!(load_script(&script, &out).is_err());
    drop(harness);
    let _ = fs::remove_dir_all(&dir);
}

/// egui はホイールの量を数フレームに分けて渡す（滑らかなスクロール）。`scroll_view` は、
/// 分けた量を渡し終えるまで次の手順へ進まない。進むと、次の手順でポインタが3Dビューを離れたとき
/// 残りが捨てられ、実行するたびに視点の距離が変わる。
#[test]
fn scroll_view_waits_until_the_smoothed_scroll_is_delivered() {
    let _guard = exit_lock();
    let dir = scratch("scroll");
    let script = dir.join("steps.json");
    fs::write(
        &script,
        r#"{"steps":[{"op":"scroll_view","at":[0.5,0.5],"delta":[0,24]},{"op":"quit"}]}"#,
    )
    .unwrap();
    let launch = parse_args([
        "model-reviewer",
        "--script",
        script.to_str().unwrap(),
        "--out",
        dir.join("out").to_str().unwrap(),
    ])
    .unwrap();
    let mut automation = Automation::from_launch(&launch);
    let ctx = egui::Context::default();
    let mut session = Session::default();
    let mut delivered = 0.0_f32;
    let mut delivered_when_done = None;
    for frame in 0..120 {
        let mut raw = egui::RawInput {
            time: Some(f64::from(frame) / 60.0),
            ..Default::default()
        };
        automation.inject(&ctx, &mut raw);
        let mut output = ctx.run_ui(raw, |ui| {
            ui.ctx().data_mut(|data| {
                let mut widgets = WidgetFrame::default();
                widgets.push(WidgetEntry {
                    name: ids::VIEWPORT.to_owned(),
                    min: [0.0, 0.0],
                    max: [200.0, 100.0],
                });
                data.insert_temp(egui::Id::NULL, widgets);
            });
            delivered += ui.input(|input| input.smooth_scroll_delta.y);
            automation.on_ui(ui.ctx(), &mut session, false);
        });
        output.textures_delta.clear();
        let index = automation.run.as_ref().map_or(0, |run| run.index);
        if index >= 1 && delivered_when_done.is_none() {
            delivered_when_done = Some(delivered);
        }
    }
    let at_done = delivered_when_done.expect("scroll_view が終わらない");
    // 既定の単位は行なので、量は 24 行分（egui の1行の点数を掛けた値）になる。
    assert!(delivered > 24.0, "スクロールが届いていない: {delivered}");
    assert!(
        (at_done - delivered).abs() <= delivered * 1.0e-4,
        "scroll_view が終わった時点で {at_done} しか渡していない（全体は {delivered}）"
    );
    assert!(automation.finished, "quit まで進む");
    let _ = fs::remove_dir_all(&dir);
}
