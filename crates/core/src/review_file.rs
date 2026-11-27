//! レビューJSON。知らない項目は無視し、非数と f32 に収まらない数は拒む。
//!
//! 版2は版1に「視点の `camera.projection` に `"orthographic"` を許す」を足しただけ。平行投影の
//! 視点を1つでも含むときだけ版2で書き、含まなければ版1のまま書く（前の版のアプリでも読める）。
//! 読むときは版1と版2の両方を受け付ける。版1のファイルに `"orthographic"` があれば不正とする。
//!
//! STL の単位の仮定は `model.stl_unit`（`"mm"`・`"cm"`・`"m"`）として書く。GLB では項目ごと
//! 省略し、版も上げない。読むときは省略なら `None`、3つ以外の値は不正とする。

use std::collections::HashSet;

use glam::Vec3;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::camera::{OrbitCamera, Projection};
use crate::error::ReviewError;
use crate::identity::{ModelIdentity, ModelRef};
use crate::review::{Annotation, ReviewState, SavedView};
use crate::scene::{NodeId, SceneRead};
use crate::units::UnitLabel;
use crate::visibility::Visibility;

const FORMAT: &str = "model-reviewer.review";
/// 書ける・読める最も新しい版。
const VERSION: u32 = 2;
/// 平行投影の視点が無いときに書く版。
const VERSION_PERSPECTIVE_ONLY: u32 = 1;
const PROJECTION_PERSPECTIVE: &str = "perspective";
const PROJECTION_ORTHOGRAPHIC: &str = "orthographic";
/// レビューJSONが受け付ける注釈・視点の番号の上限。
pub(crate) const MAX_PARSED_ID: u64 = u64::MAX - 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewFile {
    pub format: String,
    pub version: u32,
    pub app_version: String,
    pub model: ReviewModelRecord,
    #[serde(default)]
    pub hidden_nodes: Vec<ReviewNodeRecord>,
    #[serde(default)]
    pub annotations: Vec<ReviewAnnotationRecord>,
    #[serde(default)]
    pub views: Vec<ReviewViewRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewModelRecord {
    pub file_name: String,
    pub path: String,
    #[serde(default)]
    pub relative_path: Option<String>,
    pub size_bytes: u64,
    pub hash: ReviewHashRecord,
    pub node_count: u32,
    /// STL の単位の仮定。GLB では `None` で、項目ごと書かない。
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "stl_unit_text"
    )]
    pub stl_unit: Option<UnitLabel>,
}

/// `model.stl_unit` の文字（`"mm"`・`"cm"`・`"m"`）。ほかの文字と文字列でない値は不正。
mod stl_unit_text {
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serializer};

    use crate::units::UnitLabel;

    pub fn serialize<S: Serializer>(
        value: &Option<UnitLabel>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(unit) => serializer.serialize_str(unit.suffix()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<UnitLabel>, D::Error> {
        let text = String::deserialize(deserializer)?;
        match text.as_str() {
            "mm" => Ok(Some(UnitLabel::Mm)),
            "cm" => Ok(Some(UnitLabel::Cm)),
            "m" => Ok(Some(UnitLabel::M)),
            _ => Err(D::Error::custom("stl_unit")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewHashRecord {
    pub algorithm: String,
    pub hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewNodeRecord {
    pub gltf_index: u32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewAnnotationRecord {
    pub id: u64,
    pub node: ReviewNodeRecord,
    pub local_position: [f32; 3],
    pub local_normal: [f32; 3],
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewViewRecord {
    pub id: u64,
    pub name: String,
    pub camera: ReviewCameraRecord,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewCameraRecord {
    pub projection: String,
    pub target: [f32; 3],
    pub yaw_deg: f32,
    pub pitch_deg: f32,
    pub distance: f32,
    pub fov_y_deg: f32,
}

pub fn to_json(
    state: &ReviewState,
    model: &impl SceneRead,
    model_ref: &ModelRef,
) -> Result<String, ReviewError> {
    ensure_finite(state)?;
    let version = if state
        .views
        .iter()
        .any(|view| view.camera.projection == Projection::Orthographic)
    {
        VERSION
    } else {
        VERSION_PERSPECTIVE_ONLY
    };
    let file = ReviewFile {
        format: FORMAT.to_owned(),
        version,
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        model: ReviewModelRecord {
            file_name: model_ref.file_name.clone(),
            path: model_ref.path.clone(),
            relative_path: model_ref.relative_path.clone(),
            size_bytes: model_ref.identity.size_bytes,
            hash: ReviewHashRecord {
                algorithm: "blake3".to_owned(),
                hex: model_ref.identity.blake3_hex.clone(),
            },
            node_count: model.node_count() as u32,
            stl_unit: model_ref.stl_unit,
        },
        hidden_nodes: state
            .visibility
            .hidden_own()
            .filter_map(|id| node_record(model, id))
            .collect(),
        annotations: state
            .annotations
            .iter()
            .filter_map(|note| annotation_record(model, note))
            .collect(),
        views: state.views.iter().map(view_record).collect(),
    };
    serde_json::to_string_pretty(&file).map_err(|err| ReviewError::Json(err.to_string()))
}

pub fn parse(json: &str) -> Result<ReviewFile, ReviewError> {
    let value: Value =
        serde_json::from_str(json).map_err(|err| ReviewError::Json(err.to_string()))?;
    if value.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(ReviewError::WrongFormat);
    }
    let version = read_version(value.get("version"))?;
    if version > VERSION {
        return Err(ReviewError::NewerVersion {
            found: version,
            supported: VERSION,
        });
    }
    if !(VERSION_PERSPECTIVE_ONLY..=VERSION).contains(&version) {
        return Err(ReviewError::InvalidValue("version".to_owned()));
    }
    reject_overflowing_numbers(&value)?;
    let mut file: ReviewFile =
        serde_json::from_value(value).map_err(|err| ReviewError::InvalidValue(err.to_string()))?;
    file.model.hash.hex.make_ascii_lowercase();
    validate_file(&file)?;
    Ok(file)
}

pub fn restore(
    file: &ReviewFile,
    model: &impl SceneRead,
    identity: &ModelIdentity,
) -> Result<ReviewState, ReviewError> {
    validate_file(file)?;
    let expected = ModelIdentity {
        size_bytes: file.model.size_bytes,
        blake3_hex: file.model.hash.hex.to_ascii_lowercase(),
    };
    if expected != *identity {
        return Err(ReviewError::ModelMismatch {
            expected,
            actual: identity.clone(),
        });
    }
    let mut visibility = Visibility::all_visible(model.node_count());
    if !file.hidden_nodes.is_empty() {
        let hidden = file
            .hidden_nodes
            .iter()
            .map(|record| require_node(model, record))
            .collect::<Result<Vec<_>, _>>()?;
        visibility.set_many(model, hidden, false);
    }
    let mut annotations = Vec::with_capacity(file.annotations.len());
    for record in &file.annotations {
        let node = require_node(model, &record.node)?;
        annotations.push(Annotation {
            id: record.id,
            node,
            local_pos: Vec3::from(record.local_position),
            local_normal: Vec3::from(record.local_normal),
            text: record.text.clone(),
        });
    }
    let mut views = Vec::with_capacity(file.views.len());
    for record in &file.views {
        views.push(SavedView {
            id: record.id,
            name: record.name.clone(),
            camera: camera_from_record(&record.camera, file.version)?,
        });
    }
    Ok(ReviewState::from_parts(visibility, annotations, views))
}

fn node_record(model: &impl SceneRead, id: NodeId) -> Option<ReviewNodeRecord> {
    if id.0 as usize >= model.node_count() {
        return None;
    }
    Some(ReviewNodeRecord {
        gltf_index: model.file_index(id)?,
        name: model.name(id).to_owned(),
    })
}

fn annotation_record(model: &impl SceneRead, note: &Annotation) -> Option<ReviewAnnotationRecord> {
    Some(ReviewAnnotationRecord {
        id: note.id,
        node: node_record(model, note.node)?,
        local_position: note.local_pos.to_array(),
        local_normal: note.local_normal.to_array(),
        text: note.text.clone(),
    })
}

fn view_record(view: &SavedView) -> ReviewViewRecord {
    ReviewViewRecord {
        id: view.id,
        name: view.name.clone(),
        camera: ReviewCameraRecord {
            projection: match view.camera.projection {
                Projection::Perspective => PROJECTION_PERSPECTIVE,
                Projection::Orthographic => PROJECTION_ORTHOGRAPHIC,
            }
            .to_owned(),
            target: view.camera.target.to_array(),
            yaw_deg: view.camera.yaw.to_degrees(),
            pitch_deg: view.camera.pitch.to_degrees(),
            distance: view.camera.distance,
            fov_y_deg: view.camera.fov_y.to_degrees(),
        },
    }
}

fn camera_from_record(
    camera: &ReviewCameraRecord,
    version: u32,
) -> Result<OrbitCamera, ReviewError> {
    let projection = match camera.projection.as_str() {
        PROJECTION_PERSPECTIVE => Projection::Perspective,
        PROJECTION_ORTHOGRAPHIC if version >= VERSION => Projection::Orthographic,
        _ => return Err(ReviewError::InvalidValue("projection".to_owned())),
    };
    let target = Vec3::from(camera.target);
    let yaw = camera.yaw_deg.to_radians();
    let pitch = camera.pitch_deg.to_radians();
    let fov_y = camera.fov_y_deg.to_radians();
    if !finite3(camera.target)
        || !camera.yaw_deg.is_finite()
        || !camera.pitch_deg.is_finite()
        || !camera.distance.is_finite()
        || camera.distance <= 0.0
        || !camera.fov_y_deg.is_finite()
        || camera.fov_y_deg <= 0.0
        || camera.fov_y_deg >= 180.0
        || !target.is_finite()
        || !yaw.is_finite()
        || !pitch.is_finite()
        || !fov_y.is_finite()
    {
        return Err(ReviewError::InvalidValue("camera".to_owned()));
    }
    Ok(OrbitCamera {
        target,
        yaw,
        pitch,
        distance: camera.distance,
        fov_y,
        projection,
    })
}

fn require_node(model: &impl SceneRead, record: &ReviewNodeRecord) -> Result<NodeId, ReviewError> {
    let Some(id) = model.find_file_index(record.gltf_index) else {
        return Err(ReviewError::UnknownNode {
            gltf_index: record.gltf_index,
        });
    };
    if model.name(id) != record.name {
        return Err(ReviewError::UnknownNode {
            gltf_index: record.gltf_index,
        });
    }
    Ok(id)
}

fn validate_file(file: &ReviewFile) -> Result<(), ReviewError> {
    if file.format != FORMAT {
        return Err(ReviewError::WrongFormat);
    }
    if file.version > VERSION {
        return Err(ReviewError::NewerVersion {
            found: file.version,
            supported: VERSION,
        });
    }
    if !(VERSION_PERSPECTIVE_ONLY..=VERSION).contains(&file.version) {
        return Err(ReviewError::InvalidValue("version".to_owned()));
    }
    if file.model.hash.algorithm != "blake3" || !valid_hex(&file.model.hash.hex) {
        return Err(ReviewError::InvalidValue("hash".to_owned()));
    }
    for note in &file.annotations {
        if !finite3(note.local_position) || !finite3(note.local_normal) {
            return Err(ReviewError::InvalidValue("non-finite number".to_owned()));
        }
    }
    for view in &file.views {
        camera_from_record(&view.camera, file.version)?;
    }
    validate_ids(file)?;
    Ok(())
}

/// 注釈と視点は一つの番号。重複と上限を超える番号は拒む。
fn validate_ids(file: &ReviewFile) -> Result<(), ReviewError> {
    let mut seen = HashSet::with_capacity(file.annotations.len() + file.views.len());
    for id in file
        .annotations
        .iter()
        .map(|note| note.id)
        .chain(file.views.iter().map(|view| view.id))
    {
        if id > MAX_PARSED_ID {
            return Err(ReviewError::InvalidValue("id".to_owned()));
        }
        if !seen.insert(id) {
            return Err(ReviewError::InvalidValue("duplicate id".to_owned()));
        }
    }
    Ok(())
}

fn ensure_finite(state: &ReviewState) -> Result<(), ReviewError> {
    for note in &state.annotations {
        if !note.local_pos.is_finite() || !note.local_normal.is_finite() {
            return Err(ReviewError::InvalidValue("non-finite number".to_owned()));
        }
    }
    for view in &state.views {
        let camera = &view.camera;
        let yaw_deg = camera.yaw.to_degrees();
        let pitch_deg = camera.pitch.to_degrees();
        let fov_y_deg = camera.fov_y.to_degrees();
        if !camera.target.is_finite()
            || !camera.yaw.is_finite()
            || !yaw_deg.is_finite()
            || !camera.pitch.is_finite()
            || !pitch_deg.is_finite()
            || !camera.distance.is_finite()
            || !camera.fov_y.is_finite()
            || !fov_y_deg.is_finite()
        {
            return Err(ReviewError::InvalidValue("non-finite number".to_owned()));
        }
        if camera.distance <= 0.0 || fov_y_deg <= 0.0 || fov_y_deg >= 180.0 {
            return Err(ReviewError::InvalidValue("camera".to_owned()));
        }
    }
    Ok(())
}

fn valid_hex(hex: &str) -> bool {
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn finite3(value: [f32; 3]) -> bool {
    value.iter().all(|item| item.is_finite())
}

fn read_version(value: Option<&Value>) -> Result<u32, ReviewError> {
    let Some(value) = value else {
        return Err(ReviewError::InvalidValue("version".to_owned()));
    };
    let Some(number) = value.as_u64() else {
        return Err(ReviewError::InvalidValue("version".to_owned()));
    };
    if number > u64::from(u32::MAX) {
        return Err(ReviewError::NewerVersion {
            found: u32::MAX,
            supported: VERSION,
        });
    }
    Ok(number as u32)
}

/// f32 へ入れる数だけを見る。u64 のバイト数は f32 に収まらなくてもよい。
fn reject_overflowing_numbers(value: &Value) -> Result<(), ReviewError> {
    let Some(object) = value.as_object() else {
        return Err(ReviewError::InvalidValue("object".to_owned()));
    };
    if let Some(notes) = object.get("annotations") {
        reject_float_fields(notes, &["local_position", "local_normal"])?;
    }
    if let Some(views) = object.get("views") {
        let Value::Array(views) = views else {
            return Err(ReviewError::InvalidValue("views".to_owned()));
        };
        for view in views {
            let Some(camera) = view.get("camera") else {
                continue;
            };
            reject_float_fields(
                camera,
                &["target", "yaw_deg", "pitch_deg", "distance", "fov_y_deg"],
            )?;
        }
    }
    Ok(())
}

fn reject_float_fields(value: &Value, fields: &[&str]) -> Result<(), ReviewError> {
    let items: Vec<&Value> = match value {
        Value::Array(items) => items.iter().collect(),
        other => vec![other],
    };
    for item in items {
        for field in fields {
            let Some(child) = item.get(*field) else {
                continue;
            };
            reject_f32_number(child)?;
        }
    }
    Ok(())
}

fn reject_f32_number(value: &Value) -> Result<(), ReviewError> {
    match value {
        Value::Array(items) => {
            for item in items {
                reject_f32_number(item)?;
            }
            Ok(())
        }
        Value::Number(number) => match number.as_f64() {
            Some(value) if value.is_finite() && (value as f32).is_finite() => Ok(()),
            _ => Err(ReviewError::InvalidValue("non-finite number".to_owned())),
        },
        _ => Ok(()),
    }
}
