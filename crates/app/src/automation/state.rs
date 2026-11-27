//! 状態の比較と、JSON・画像の書き出し。

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::session::Session;

pub(in crate::automation) fn state_value(session: &Session) -> Value {
    let hidden = session.review.as_ref().map(|review| {
        review
            .visibility
            .hidden_own()
            .map(|id| id.0)
            .collect::<Vec<_>>()
    });
    let annotations = session.review.as_ref().map(|review| {
        review
            .annotations
            .iter()
            .map(|note| {
                json!({
                    "id": note.id,
                    "node": note.node.0,
                    "local_position": vec3_json(note.local_pos),
                    "local_normal": vec3_json(note.local_normal),
                    "text": note.text,
                })
            })
            .collect::<Vec<_>>()
    });
    let views = session.review.as_ref().map(|review| {
        review
            .views
            .iter()
            .map(|view| {
                json!({
                    "id": view.id,
                    "name": view.name,
                    "camera": camera_json(&view.camera),
                })
            })
            .collect::<Vec<_>>()
    });
    let nodes = session.doc.as_ref().map(|doc| {
        doc.nodes()
            .iter()
            .map(|node| {
                json!({
                    "name": node.name.as_ref(),
                    "local": affine_json(&node.local),
                })
            })
            .collect::<Vec<_>>()
    });
    let measure_points = session.doc.as_ref().map_or_else(Vec::new, |doc| {
        session
            .measurement
            .world_points(doc)
            .into_iter()
            .map(vec3_json)
            .collect::<Vec<_>>()
    });
    json!({
        "tool": tool_name(session.tool),
        "nodes": nodes.unwrap_or_default(),
        "history": {
            "undo": session.history.len(),
            "can_redo": session.history.can_redo(),
            "bytes": session.history.bytes(),
            "doc_rev": session.history.doc_rev(),
            "review_rev": session.history.review_rev(),
        },
        "unsaved": session.has_unsaved(),
        "doc_unsaved": session.doc_unsaved(),
        "review_unsaved": session.review_unsaved(),
        "selection": session.selection.map(|id| id.0),
        "hidden": hidden.unwrap_or_default(),
        "annotations": annotations.unwrap_or_default(),
        "views": views.unwrap_or_default(),
        "camera": camera_json(&session.camera),
        "display": display_name(session.display),
        "measure": {
            "points": measure_points,
            "distance": crate::session::measure_distance(session).map(f64::from),
            "kinds": measure_kinds(session),
            "delta": crate::session::measure_delta(session).map(vec3_json),
        },
        // 前提の単位（第2段階からの項目。表示の単位は `units`）。
        "unit": session.units.source.suffix(),
        "units": {
            "source": session.units.source.suffix(),
            "display": session.units.display.map(|unit| unit.suffix()),
            "stl": match session.source_kind {
                crate::session::SourceKind::Glb => None,
                crate::session::SourceKind::Stl { unit } => Some(unit.suffix()),
            },
        },
        "snap": {
            "enabled": session.snap.enabled,
            "candidate": session.snap_preview.map(|target| json!({
                "kind": snap_kind_name(target.kind),
                "world": vec3_json(target.world),
            })),
        },
        "section": session.section.map(|plane| json!({
            "axis": match plane.axis {
                model_reviewer_core::section::Axis::X => "x",
                model_reviewer_core::section::Axis::Y => "y",
                model_reviewer_core::section::Axis::Z => "z",
            },
            "offset": f64::from(plane.offset),
            "flipped": plane.flipped,
        })),
        // 開いているモデルの元の形式（`glb` か `stl`。STL から書き出した後は `glb`）。
        "source_kind": match session.source_kind {
            crate::session::SourceKind::Glb => "glb",
            crate::session::SourceKind::Stl { .. } => "stl",
        },
        // 選んだ部品の外形と寸法（シーンの単位）。`selection` は第2段階からの番号の項目なので、
        // 意味を変えずに別の項目にした。
        "selection_dims": crate::session::selection_dims(session).map(|view| dims_json(&view)),
        "triangles": session.placed_triangles(),
        "grid": {
            "visible": session.grid_visible,
            "spacing": f64::from(model_reviewer_core::camera::grid_spacing(session.camera.distance)),
        },
    })
}

/// 状態の比較で見ない項目。履歴は開き直すと空になる（保存されるものではない）。
pub(in crate::automation) const NOT_COMPARED: [&str; 1] = ["history"];

fn dims_json(view: &crate::session::DimsView) -> Value {
    use model_reviewer_core::shapes::Dims;
    let dims = view
        .shape
        .and_then(|shape| shape.dims)
        .map(|dims| match dims {
            Dims::Cube { x, y, z } => {
                json!({ "kind": "cube", "x": f64::from(x), "y": f64::from(y), "z": f64::from(z) })
            }
            Dims::Sphere { d } => json!({ "kind": "sphere", "d": f64::from(d) }),
            Dims::Cylinder { d, h } => {
                json!({ "kind": "cylinder", "d": f64::from(d), "h": f64::from(h) })
            }
            Dims::Plane { x, z } => {
                json!({ "kind": "plane", "x": f64::from(x), "z": f64::from(z) })
            }
        });
    json!({
        "extent": view.extent.map(|e| [f64::from(e[0]), f64::from(e[1]), f64::from(e[2])]),
        "editable": view.shape.is_some_and(|shape| shape.dims.is_some() && shape.blocked.is_none()),
        "dims": dims,
    })
}

/// 置いた点の種類（置いた数だけ）。
fn measure_kinds(session: &Session) -> Vec<&'static str> {
    let measurement = &session.measurement;
    [measurement.first, measurement.second]
        .iter()
        .zip(measurement.kinds)
        .filter(|(point, _)| point.is_some())
        .map(|(_, kind)| snap_kind_name(kind))
        .collect()
}

fn snap_kind_name(kind: model_reviewer_core::snap::SnapKind) -> &'static str {
    use model_reviewer_core::snap::SnapKind;
    match kind {
        SnapKind::Vertex => "vertex",
        SnapKind::Edge => "edge",
        SnapKind::Face => "face",
    }
}

fn tool_name(tool: model_reviewer_core::gizmo::Tool) -> &'static str {
    use model_reviewer_core::gizmo::Tool;
    match tool {
        Tool::Select => "select",
        Tool::Move => "move",
        Tool::Rotate => "rotate",
        Tool::Scale => "scale",
        Tool::Shape => "shape",
        Tool::Measure => "measure",
        Tool::Annotate => "annotate",
    }
}

/// 親に対する行列。列ごと（X軸・Y軸・Z軸・位置）の12個。
fn affine_json(value: &glam::Affine3A) -> Value {
    let cols = value.to_cols_array();
    Value::Array(cols.iter().map(|v| json!(f64::from(*v))).collect())
}

fn display_name(mode: model_reviewer_render::DisplayMode) -> &'static str {
    use model_reviewer_render::DisplayMode;
    match mode {
        DisplayMode::Faces => "faces",
        DisplayMode::FacesAndLines => "faces_and_lines",
        DisplayMode::Lines => "lines",
        DisplayMode::XRay => "xray",
    }
}

fn camera_json(camera: &model_reviewer_core::camera::OrbitCamera) -> Value {
    json!({
        "target": [f64::from(camera.target.x), f64::from(camera.target.y), f64::from(camera.target.z)],
        "yaw": f64::from(camera.yaw),
        "pitch": f64::from(camera.pitch),
        "distance": f64::from(camera.distance),
        "fov_y": f64::from(camera.fov_y),
        "projection": match camera.projection {
            model_reviewer_core::camera::Projection::Perspective => "perspective",
            model_reviewer_core::camera::Projection::Orthographic => "orthographic",
        },
    })
}

fn vec3_json(value: glam::Vec3) -> Value {
    json!([f64::from(value.x), f64::from(value.y), f64::from(value.z)])
}

pub(in crate::automation) fn values_match(
    actual: &Value,
    expected: &Value,
    tolerance: f64,
    path: &str,
) -> Result<(), String> {
    match (actual, expected) {
        (Value::Number(left), Value::Number(right)) => {
            if numbers_match(left, right, tolerance) {
                Ok(())
            } else {
                Err(format!("{path} が一致しません: {left} と {right}"))
            }
        }
        (Value::Array(left), Value::Array(right)) => {
            if left.len() != right.len() {
                return Err(format!(
                    "{path} の長さが一致しません: {} と {}",
                    left.len(),
                    right.len()
                ));
            }
            for (index, (actual_item, expected_item)) in left.iter().zip(right).enumerate() {
                values_match(
                    actual_item,
                    expected_item,
                    tolerance,
                    &format!("{path}[{index}]"),
                )?;
            }
            Ok(())
        }
        (Value::Object(left), Value::Object(right)) => {
            let mut left_keys: Vec<&String> = left.keys().collect();
            let mut right_keys: Vec<&String> = right.keys().collect();
            left_keys.sort();
            right_keys.sort();
            if left_keys != right_keys {
                return Err(format!("{path} の項目が一致しません"));
            }
            for key in left_keys {
                values_match(&left[key], &right[key], tolerance, &format!("{path}.{key}"))?;
            }
            Ok(())
        }
        _ if actual == expected => Ok(()),
        _ => Err(format!("{path} の型または値が一致しません")),
    }
}

pub(in crate::automation) fn numbers_match(
    actual: &serde_json::Number,
    expected: &serde_json::Number,
    tolerance: f64,
) -> bool {
    match (actual.as_f64(), expected.as_f64()) {
        (Some(left), Some(right)) => {
            if left.is_nan() && right.is_nan() {
                return true;
            }
            if !left.is_finite() || !right.is_finite() {
                return left == right;
            }
            (left - right).abs() <= tolerance
        }
        _ => actual == expected,
    }
}

pub(in crate::automation) fn save_png(path: &Path, image: &egui::ColorImage) -> Result<(), String> {
    let width = u32::try_from(image.size[0]).map_err(|_| "画像の幅が大きすぎます".to_owned())?;
    let height = u32::try_from(image.size[1]).map_err(|_| "画像の高さが大きすぎます".to_owned())?;
    if width == 0 || height == 0 {
        return Err("画像が空です".to_owned());
    }
    let mut bytes = Vec::with_capacity(image.pixels.len().saturating_mul(4));
    for pixel in &image.pixels {
        bytes.extend(pixel.to_srgba_unmultiplied());
    }
    let buffer = image::RgbaImage::from_raw(width, height, bytes)
        .ok_or_else(|| "画像の寸法と画素数が一致しません".to_owned())?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|err| format!("画像のフォルダを作れませんでした: {err}"))?;
    }
    buffer
        .save(path)
        .map_err(|err| format!("画像を保存できませんでした: {err}"))
}

pub(in crate::automation) fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|err| format!("フォルダを作れませんでした: {err}"))?;
    }
    let mut text = serde_json::to_string_pretty(value)
        .map_err(|err| format!("JSONにできませんでした: {err}"))?;
    text.push('\n');
    fs::write(path, text).map_err(|err| format!("ファイルを書けませんでした: {err}"))
}

pub(in crate::automation) fn elapsed_ms(started: Option<Instant>) -> u64 {
    let Some(started) = started else {
        return 0;
    };
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

pub(in crate::automation) fn format_secs(duration: Duration) -> String {
    let secs = duration.as_secs_f64();
    if secs.fract() == 0.0 && secs <= u64::MAX as f64 {
        format!("{}", duration.as_secs())
    } else {
        format!("{secs}")
    }
}
