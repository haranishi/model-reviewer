//! JSONの値を作る小さな道具と、ノードの変形・名前・子を書き換える関数。
//! `serde_json` を直接の依存にしないため、オブジェクトは `Value` のまま扱う。

use glam::{Affine3A, Mat4, Quat, Vec3};
use gltf::json::Value;

use super::ExportError;

/// 空のオブジェクト。
pub(crate) fn obj() -> Value {
    Value::Object(Default::default())
}

/// オブジェクトに項目を入れる（オブジェクトでなければ何もしない。呼び出し側が型を確かめる）。
pub(crate) fn set(target: &mut Value, key: &str, value: Value) {
    if let Value::Object(map) = target {
        map.insert(key.to_owned(), value);
    }
}

pub(crate) fn remove(target: &mut Value, key: &str) {
    if let Value::Object(map) = target {
        map.remove(key);
    }
}

pub(crate) fn uint(value: u64) -> Value {
    Value::from(value)
}

pub(crate) fn uints(values: &[u32]) -> Value {
    Value::Array(values.iter().map(|value| uint(u64::from(*value))).collect())
}

/// 32ビットの値を64ビットで書く（読み戻すと元の32ビットの値に戻る）。有限でなければ断る。
pub(crate) fn f32s(values: &[f32]) -> Result<Value, ExportError> {
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        if !value.is_finite() {
            return Err(ExportError::InvalidScene(
                "有限でない数は書けません".to_owned(),
            ));
        }
        out.push(Value::from(f64::from(*value)));
    }
    Ok(Value::Array(out))
}

const TRANSFORM_KEYS: [&str; 4] = ["matrix", "translation", "rotation", "scale"];

/// ノードの変形を書き換える。位置・回転・大きさに分けられれば3項目（既定値の項目は省く）、
/// 分けられなければ `matrix` で書く。`matrix` で書いたら真を返す。
pub(crate) fn write_transform(node: &mut Value, local: Affine3A) -> Result<bool, ExportError> {
    for key in TRANSFORM_KEYS {
        remove(node, key);
    }
    if let Some((scale, rotation, translation)) = split_trs(local) {
        if translation != Vec3::ZERO {
            set(node, "translation", f32s(&translation.to_array())?);
        }
        if rotation != Quat::IDENTITY {
            set(node, "rotation", f32s(&rotation.to_array())?);
        }
        if scale != Vec3::ONE {
            set(node, "scale", f32s(&scale.to_array())?);
        }
        return Ok(false);
    }
    set(node, "matrix", f32s(&Mat4::from(local).to_cols_array())?);
    Ok(true)
}

/// 行列を位置・回転・大きさに分ける。組み直した行列が元と許容差の中で一致しないとき
/// （傾いた拡大、大きさ0など）は `None`。
pub(crate) fn split_trs(local: Affine3A) -> Option<(Vec3, Quat, Vec3)> {
    let (scale, rotation, translation) = local.to_scale_rotation_translation();
    if !scale.is_finite() || !rotation.is_finite() || !translation.is_finite() {
        return None;
    }
    if scale.cmpeq(Vec3::ZERO).any() || !rotation.is_normalized() {
        return None;
    }
    let back = Affine3A::from_scale_rotation_translation(scale, rotation, translation);
    let reach = local
        .matrix3
        .to_cols_array()
        .iter()
        .fold(1.0f32, |acc, value| acc.max(value.abs()));
    let tolerance = 1.0e-5 * reach;
    let close = local
        .matrix3
        .to_cols_array()
        .iter()
        .zip(back.matrix3.to_cols_array().iter())
        .all(|(a, b)| (a - b).abs() <= tolerance);
    close.then_some((scale, rotation, translation))
}

/// 名前を書き換える。空なら項目を消す（読み込みは「ノード n」と名付ける）。
pub(crate) fn write_name(node: &mut Value, name: &str) {
    if name.is_empty() {
        remove(node, "name");
    } else {
        set(node, "name", Value::String(name.to_owned()));
    }
}

/// 子を書き換える。空なら項目を消す（glTFの `children` は1つ以上の要素が要る）。
pub(crate) fn write_children(node: &mut Value, children: &[u32]) {
    if children.is_empty() {
        remove(node, "children");
    } else {
        set(node, "children", uints(children));
    }
}

/// ノードの `children` を番号の列で読む（無ければ空）。
pub(crate) fn read_children(node: &Value) -> Option<Vec<u64>> {
    match node.get("children") {
        None => Some(Vec::new()),
        Some(Value::Array(items)) => items.iter().map(Value::as_u64).collect(),
        Some(_) => None,
    }
}
