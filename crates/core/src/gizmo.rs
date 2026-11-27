//! 移動・回転・拡大のハンドルの計算。GPUにも画面にも依存しない。
//!
//! 約束: 移動と回転はワールドの軸、拡大は部品の軸。中心は、選択した部品と子孫の見えている範囲
//! （ワールドの境界箱）の中心。ハンドルの操作は「ワールド空間での差分の行列 D」で表し、新しい
//! `local` は `親.world⁻¹ × D × world`。視線が軸とほぼ平行で値が定まらないときは `None` を返し、
//! 呼び出し側は前の値を保つ。
//!
//! 画面の座標は3Dビューの左上が原点で y が下向き（egui と同じ）。正規化座標（ndc）は中心が 0、
//! 上が +y。ハンドルの長さは、中心の奥行きで画面上 `size_px` になるワールドの長さにする。

use glam::{Affine3A, EulerRot, Mat3A, Quat, Vec2, Vec3, Vec3A};

use crate::camera::{OrbitCamera, Projection};
use crate::math::Ray;

/// 移動と拡大の軸の線は、中心からこの割合だけ離して始める（中心は全体の拡大に譲る）。
const SHAFT_START: f32 = 0.2;
/// 矢じりの長さ（`size_px` に対する割合）。
const ARROW_HEAD: f32 = 0.18;
/// 拡大の箱の半分の大きさ（`size_px` に対する割合）。
const SCALE_BOX_HALF: f32 = 0.07;
/// 全体の拡大の四角の半分の大きさ（`size_px` に対する割合）。
const SCALE_ALL_HALF: f32 = 0.09;
/// 画面上の長さがこの割合より短い軸（視線とほぼ平行）は出さない。掴んでも値が定まらない。
const MIN_SCREEN_AXIS: f32 = 0.2;
/// 輪の分割数。
const RING_SEGMENTS: usize = 64;
/// 視線と軸の向きの `1 - cos²`（= sin²）がこれ未満なら、移動と拡大の値が定まらないとする（約1.8°）。
const PARALLEL_SIN2: f32 = 1.0e-3;
/// 視線と回転の面がなす角の sin がこれ未満なら、回転の値が定まらないとする（約2.9°）。
const EDGE_ON_SIN: f32 = 0.05;
/// 拡大の倍率の下限。0 と裏返しを作らない。
const MIN_FACTOR: f32 = 1.0e-3;
/// 全体の拡大で、掴んだ点と中心がこの画素より近ければ値が定まらないとする。
const MIN_SCALE_ALL_PX: f32 = 2.0;

/// 道具。第1段階の注釈の入力は `Annotate` として残る。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tool {
    #[default]
    Select,
    Move,
    Rotate,
    Scale,
    Shape,
    Measure,
    Annotate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Handle {
    MoveX,
    MoveY,
    MoveZ,
    RotX,
    RotY,
    RotZ,
    ScaleX,
    ScaleY,
    ScaleZ,
    ScaleAll,
}

/// ハンドルを置く枠。`axes` は移動・回転ではワールドの軸、拡大では部品の軸（正規化済み）。
/// `viewport_px` は3Dビューの大きさ（物理画素ではなく、画面の座標と同じ単位）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GizmoFrame {
    pub pivot: Vec3,
    pub axes: [Vec3; 3],
    pub camera: OrbitCamera,
    pub viewport_px: [f32; 2],
    /// ハンドルの画面上の長さ。
    pub size_px: f32,
}

/// 画面上のハンドルの形（3Dビューの左上が原点、y は下向き。egui と同じ）。
#[derive(Clone, Debug, PartialEq)]
pub struct HandleShape {
    pub handle: Handle,
    /// 折れ線（矢印の軸・輪・箱つきの線）。
    pub points: Vec<Vec2>,
    /// 折れ線を閉じるか（輪と箱）。
    pub closed: bool,
}

/// 画面上の折れ線を作る。ハンドルの無い道具（選択・図形・計測・注釈）では空。中心が目の後ろに
/// あるときも空。視線とほぼ平行な軸の移動と拡大のハンドルは出さない（掴んでも値が定まらない）。
///
/// 1つのハンドルが複数の形を持つことがある（軸の線と矢じり、軸の線と箱）。
pub fn layout(tool: Tool, frame: &GizmoFrame) -> Vec<HandleShape> {
    let Some(view) = ScreenFrame::new(frame) else {
        return Vec::new();
    };
    let mut shapes = Vec::new();
    match tool {
        Tool::Move => {
            for (index, handle) in [Handle::MoveX, Handle::MoveY, Handle::MoveZ]
                .into_iter()
                .enumerate()
            {
                if let Some((start, tip, dir)) = view.axis_on_screen(frame.axes[index]) {
                    let head = ARROW_HEAD * view.size_px;
                    let base = tip - dir * head;
                    let side = dir.perp() * head * 0.45;
                    shapes.push(HandleShape {
                        handle,
                        points: vec![start, base],
                        closed: false,
                    });
                    shapes.push(HandleShape {
                        handle,
                        points: vec![tip, base + side, base - side],
                        closed: true,
                    });
                }
            }
        }
        Tool::Rotate => {
            for (index, handle) in [Handle::RotX, Handle::RotY, Handle::RotZ]
                .into_iter()
                .enumerate()
            {
                if let Some(points) = view.ring_on_screen(frame.axes[index]) {
                    shapes.push(HandleShape {
                        handle,
                        points,
                        closed: true,
                    });
                }
            }
        }
        Tool::Scale => {
            for (index, handle) in [Handle::ScaleX, Handle::ScaleY, Handle::ScaleZ]
                .into_iter()
                .enumerate()
            {
                if let Some((start, tip, dir)) = view.axis_on_screen(frame.axes[index]) {
                    let half = SCALE_BOX_HALF * view.size_px;
                    let side = dir.perp() * half;
                    let along = dir * half;
                    shapes.push(HandleShape {
                        handle,
                        points: vec![start, tip - along],
                        closed: false,
                    });
                    shapes.push(HandleShape {
                        handle,
                        points: vec![
                            tip - along - side,
                            tip + along - side,
                            tip + along + side,
                            tip - along + side,
                        ],
                        closed: true,
                    });
                }
            }
            let half = SCALE_ALL_HALF * view.size_px;
            let c = view.pivot_px;
            shapes.push(HandleShape {
                handle: Handle::ScaleAll,
                points: vec![
                    c + Vec2::new(-half, -half),
                    c + Vec2::new(half, -half),
                    c + Vec2::new(half, half),
                    c + Vec2::new(-half, half),
                ],
                closed: true,
            });
        }
        Tool::Select | Tool::Shape | Tool::Measure | Tool::Annotate => {}
    }
    shapes
}

/// `pointer_px` から `tolerance_px` 以内で最も近いハンドル。距離は折れ線の辺までで測る。
/// 閉じた形のうち輪（回転）以外（矢じり・箱・全体の拡大の四角）は、内側も当たりにする。
/// 同じ距離なら先に並んだ形が勝つ。
pub fn hit(shapes: &[HandleShape], pointer_px: Vec2, tolerance_px: f32) -> Option<Handle> {
    if !pointer_px.is_finite() || !tolerance_px.is_finite() || tolerance_px < 0.0 {
        return None;
    }
    let mut best: Option<(f32, Handle)> = None;
    for shape in shapes {
        let Some(distance) = shape_distance(shape, pointer_px) else {
            continue;
        };
        if distance > tolerance_px {
            continue;
        }
        if best.is_none_or(|(current, _)| distance < current) {
            best = Some((distance, shape.handle));
        }
    }
    best.map(|(_, handle)| handle)
}

fn shape_distance(shape: &HandleShape, point: Vec2) -> Option<f32> {
    let points = &shape.points;
    if points.is_empty() || points.iter().any(|p| !p.is_finite()) {
        return None;
    }
    if points.len() == 1 {
        return Some(point.distance(points[0]));
    }
    let filled =
        shape.closed && !matches!(shape.handle, Handle::RotX | Handle::RotY | Handle::RotZ);
    if filled && points.len() >= 3 && inside_polygon(points, point) {
        return Some(0.0);
    }
    let mut best = f32::INFINITY;
    for pair in points.windows(2) {
        best = best.min(segment_distance(point, pair[0], pair[1]));
    }
    if shape.closed && points.len() >= 3 {
        best = best.min(segment_distance(point, points[points.len() - 1], points[0]));
    }
    Some(best)
}

fn segment_distance(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 <= f32::EPSILON {
        return point.distance(a);
    }
    let t = ((point - a).dot(ab) / len2).clamp(0.0, 1.0);
    point.distance(a + ab * t)
}

fn inside_polygon(points: &[Vec2], point: Vec2) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;
    for i in 0..points.len() {
        let (a, b) = (points[i], points[j]);
        if (a.y > point.y) != (b.y > point.y) {
            let x = (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x;
            if point.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// 画面に置くための値をまとめたもの。
struct ScreenFrame {
    pivot: Vec3,
    pivot_px: Vec2,
    /// 中心の奥行きで、画面上 `size_px` になるワールドの長さ。
    world_len: f32,
    size_px: f32,
    camera: OrbitCamera,
    viewport: Vec2,
}

impl ScreenFrame {
    fn new(frame: &GizmoFrame) -> Option<Self> {
        let viewport = Vec2::from(frame.viewport_px);
        if !viewport.is_finite()
            || viewport.x <= 0.0
            || viewport.y <= 0.0
            || !frame.size_px.is_finite()
            || frame.size_px <= 0.0
            || !frame.pivot.is_finite()
        {
            return None;
        }
        let aspect = viewport.x / viewport.y;
        let ndc = frame.camera.project(frame.pivot, aspect)?;
        let half = frame.camera.half_height_at(frame.pivot)?;
        let world_len = frame.size_px * 2.0 * half / viewport.y;
        if !world_len.is_finite() || world_len <= 0.0 {
            return None;
        }
        Some(Self {
            pivot: frame.pivot,
            pivot_px: ndc_to_px(ndc, viewport),
            world_len,
            size_px: frame.size_px,
            camera: frame.camera,
            viewport,
        })
    }

    fn to_px(&self, world: Vec3) -> Option<Vec2> {
        let ndc = self
            .camera
            .project(world, self.viewport.x / self.viewport.y)?;
        let px = ndc_to_px(ndc, self.viewport);
        px.is_finite().then_some(px)
    }

    /// 軸の線の始まり・先端・画面上の向き。短すぎる（視線とほぼ平行）か、先端が目の後ろなら `None`。
    fn axis_on_screen(&self, axis: Vec3) -> Option<(Vec2, Vec2, Vec2)> {
        let axis = unit(axis)?;
        let tip = self.to_px(self.pivot + axis * self.world_len)?;
        let along = tip - self.pivot_px;
        let length = along.length();
        if !length.is_finite() || length < MIN_SCREEN_AXIS * self.size_px {
            return None;
        }
        let dir = along / length;
        Some((self.pivot_px + along * SHAFT_START, tip, dir))
    }

    /// 軸に垂直な面の輪。どこかの点が目の後ろなら `None`。
    fn ring_on_screen(&self, axis: Vec3) -> Option<Vec<Vec2>> {
        let axis = unit(axis)?;
        let u = axis.any_orthonormal_vector();
        let v = axis.cross(u);
        let mut points = Vec::with_capacity(RING_SEGMENTS);
        for i in 0..RING_SEGMENTS {
            let angle = i as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
            let (sin, cos) = angle.sin_cos();
            points.push(self.to_px(self.pivot + (u * cos + v * sin) * self.world_len)?);
        }
        Some(points)
    }
}

fn ndc_to_px(ndc: Vec2, viewport: Vec2) -> Vec2 {
    Vec2::new(
        (ndc.x + 1.0) * 0.5 * viewport.x,
        (1.0 - ndc.y) * 0.5 * viewport.y,
    )
}

fn unit(value: Vec3) -> Option<Vec3> {
    if value.is_finite() && value.length_squared() > 1.0e-12 {
        Some(value.normalize())
    } else {
        None
    }
}

/// 変化量の表示。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Readout {
    /// 軸に沿って動いた距離（符号つき。軸の正の向きが正）。
    Distance(f32),
    /// 軸まわりの角度（度。軸の正の向きから見て反時計回りが正）。
    AngleDeg(f32),
    /// 倍率。
    Factor(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragState {
    pub world_delta: Affine3A,
    pub readout: Readout,
}

/// 掴んだ時点の値。
#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    handle: Handle,
    frame: GizmoFrame,
    start_ndc: Vec2,
    /// 移動と拡大は軸の上の位置、全体の拡大は中心からの画素の距離。
    start_value: f32,
    /// 回転だけが使う。掴んだ点の、中心からの向き（回転の面の上）。
    start_dir: Vec3,
}

impl Drag {
    /// 掴み始める。`ndc` はポインタの正規化座標（中心が 0、上が +y）。値が定まらなければ `None`。
    pub fn begin(handle: Handle, frame: &GizmoFrame, ndc: Vec2) -> Option<Self> {
        let mut drag = Self {
            handle,
            frame: *frame,
            start_ndc: ndc,
            start_value: 0.0,
            start_dir: Vec3::ZERO,
        };
        let world_len = ScreenFrame::new(frame)?.world_len;
        match handle {
            Handle::MoveX | Handle::MoveY | Handle::MoveZ => {
                drag.start_value = drag.axis_param(ndc)?;
            }
            Handle::ScaleX | Handle::ScaleY | Handle::ScaleZ => {
                let value = drag.axis_param(ndc)?;
                if value.abs() < world_len * 0.05 {
                    return None;
                }
                drag.start_value = value;
            }
            Handle::RotX | Handle::RotY | Handle::RotZ => {
                drag.start_dir = drag.ring_dir(ndc)?;
            }
            Handle::ScaleAll => {
                let value = drag.pixels_from_pivot(ndc)?;
                if value < MIN_SCALE_ALL_PX {
                    return None;
                }
                drag.start_value = value;
            }
        }
        Some(drag)
    }

    pub fn handle(&self) -> Handle {
        self.handle
    }

    /// 掴んだときのポインタ（正規化座標）。
    pub fn start_ndc(&self) -> Vec2 {
        self.start_ndc
    }

    /// いまのポインタから差分の行列を作る。値が定まらなければ `None`（呼び出し側は前の値を保つ）。
    pub fn update(&self, ndc: Vec2) -> Option<DragState> {
        let pivot = self.frame.pivot;
        let state = match self.handle {
            Handle::MoveX | Handle::MoveY | Handle::MoveZ => {
                let axis = self.axis()?;
                let moved = self.axis_param(ndc)? - self.start_value;
                DragState {
                    world_delta: Affine3A::from_translation(axis * moved),
                    readout: Readout::Distance(moved),
                }
            }
            Handle::RotX | Handle::RotY | Handle::RotZ => {
                let axis = self.axis()?;
                let now = self.ring_dir(ndc)?;
                let angle = self
                    .start_dir
                    .cross(now)
                    .dot(axis)
                    .atan2(self.start_dir.dot(now));
                let rotation = Affine3A::from_axis_angle(axis, angle);
                DragState {
                    world_delta: about_pivot(pivot, rotation),
                    readout: Readout::AngleDeg(angle.to_degrees()),
                }
            }
            Handle::ScaleX | Handle::ScaleY | Handle::ScaleZ => {
                let factor = (self.axis_param(ndc)? / self.start_value).max(MIN_FACTOR);
                DragState {
                    world_delta: axis_scale(
                        pivot,
                        &self.frame.axes,
                        axis_index(self.handle),
                        factor,
                    )?,
                    readout: Readout::Factor(factor),
                }
            }
            Handle::ScaleAll => {
                let factor = (self.pixels_from_pivot(ndc)? / self.start_value).max(MIN_FACTOR);
                DragState {
                    world_delta: about_pivot(pivot, Affine3A::from_scale(Vec3::splat(factor))),
                    readout: Readout::Factor(factor),
                }
            }
        };
        let finite = state.world_delta.is_finite()
            && match state.readout {
                Readout::Distance(value) | Readout::AngleDeg(value) | Readout::Factor(value) => {
                    value.is_finite()
                }
            };
        finite.then_some(state)
    }

    fn aspect(&self) -> f32 {
        self.frame.viewport_px[0] / self.frame.viewport_px[1]
    }

    fn axis(&self) -> Option<Vec3> {
        unit(self.frame.axes[axis_index(self.handle)])
    }

    /// ポインタの視線の、中心に最も近い点を出発点にした光線。平行投影の `screen_ray` は出発点が
    /// 遠く（座標が大きく丸めが乗る）、角度が少しずれるので、注視点の面の上の点から作る。
    fn ray(&self, ndc: Vec2) -> Option<Ray> {
        if !ndc.is_finite() {
            return None;
        }
        let camera = &self.frame.camera;
        let ray = match camera.projection {
            Projection::Perspective => camera.screen_ray(ndc, self.aspect()),
            Projection::Orthographic => {
                let (right, up, forward) = camera.axes();
                let half = camera.ortho_half_height();
                Ray {
                    origin: camera.target
                        + right * (ndc.x * self.aspect() * half)
                        + up * (ndc.y * half),
                    dir: forward,
                }
            }
        };
        let dir = unit(ray.dir)?;
        let origin = ray.origin + dir * (self.frame.pivot - ray.origin).dot(dir);
        origin.is_finite().then_some(Ray { origin, dir })
    }

    /// 視線と軸の直線が最も近づく点の、軸の上の位置（中心から）。ほぼ平行なら `None`。
    fn axis_param(&self, ndc: Vec2) -> Option<f32> {
        let axis = self.axis()?;
        let ray = self.ray(ndc)?;
        let w0 = self.frame.pivot - ray.origin;
        let b = axis.dot(ray.dir);
        let denom = 1.0 - b * b;
        if !denom.is_finite() || denom < PARALLEL_SIN2 {
            return None;
        }
        let s = (b * ray.dir.dot(w0) - axis.dot(w0)) / denom;
        s.is_finite().then_some(s)
    }

    /// 視線と回転の面（中心を通り軸に垂直）の交点の、中心からの向き。面が視線とほぼ平行なら `None`。
    fn ring_dir(&self, ndc: Vec2) -> Option<Vec3> {
        let axis = self.axis()?;
        let ray = self.ray(ndc)?;
        let denom = ray.dir.dot(axis);
        if !denom.is_finite() || denom.abs() < EDGE_ON_SIN {
            return None;
        }
        let t = (self.frame.pivot - ray.origin).dot(axis) / denom;
        let offset = ray.origin + ray.dir * t - self.frame.pivot;
        let offset = offset - axis * offset.dot(axis);
        unit(offset)
    }

    fn pixels_from_pivot(&self, ndc: Vec2) -> Option<f32> {
        if !ndc.is_finite() {
            return None;
        }
        let viewport = Vec2::from(self.frame.viewport_px);
        let pivot_ndc = self.frame.camera.project(self.frame.pivot, self.aspect())?;
        let distance = ndc_to_px(ndc, viewport).distance(ndc_to_px(pivot_ndc, viewport));
        distance.is_finite().then_some(distance)
    }
}

fn axis_index(handle: Handle) -> usize {
    match handle {
        Handle::MoveX | Handle::RotX | Handle::ScaleX | Handle::ScaleAll => 0,
        Handle::MoveY | Handle::RotY | Handle::ScaleY => 1,
        Handle::MoveZ | Handle::RotZ | Handle::ScaleZ => 2,
    }
}

/// 部品の軸 `axes[index]` の向きにだけ `factor` 倍する、`pivot` を動かさない変換。軸がそろって
/// いない（つぶれている）か有限でなければ `None`。
fn axis_scale(pivot: Vec3, axes: &[Vec3; 3], index: usize, factor: f32) -> Option<Affine3A> {
    let basis = Mat3A::from_cols(
        Vec3A::from(unit(axes[0])?),
        Vec3A::from(unit(axes[1])?),
        Vec3A::from(unit(axes[2])?),
    );
    let det = basis.determinant();
    if !det.is_finite() || det.abs() < 1.0e-6 {
        return None;
    }
    let mut diagonal = Vec3::ONE;
    diagonal[index] = factor;
    let linear = basis * Mat3A::from_diagonal(diagonal) * basis.inverse();
    Some(about_pivot(pivot, Affine3A::from_mat3(linear.into())))
}

/// `pivot` を動かさない変換（`T(pivot) × linear × T(-pivot)`）。
fn about_pivot(pivot: Vec3, linear: Affine3A) -> Affine3A {
    Affine3A::from_translation(pivot) * linear * Affine3A::from_translation(-pivot)
}

/// 差分の行列 D を掛けた後の新しい `local`（`親.world⁻¹ × D × world`）。親の行列が逆にできない
/// か、結果が有限でなければ `None`。根の部品は `parent_world` に単位行列を渡す。
pub fn apply_world_delta(
    parent_world: &Affine3A,
    world: &Affine3A,
    delta: &Affine3A,
) -> Option<Affine3A> {
    if !parent_world.is_finite() || !world.is_finite() || !delta.is_finite() {
        return None;
    }
    let det = parent_world.matrix3.determinant();
    if !det.is_finite() || det.abs() < 1.0e-20 {
        return None;
    }
    let local = parent_world.inverse() * *delta * *world;
    local.is_finite().then_some(local)
}

/// 軸に沿った量から、ワールドでの差分の行列を作る。`axes` と `pivot` は `GizmoFrame` と同じもの。
/// 移動は距離（シーンの単位）、回転は度、拡大は倍率。有限でない量と、0 以下の倍率は `None`。
/// `ScaleAll` は `None`。カメラは使わない（視線と軸が平行でも値が出る）。
///
/// 量が変化なし（移動と回転の 0、拡大の 1）なら、差分はちょうど単位行列にする（確定で何も積まない
/// ため。回転の行列を通すと丸めで単位行列から外れることがある）。
pub fn key_delta(handle: Handle, pivot: Vec3, axes: [Vec3; 3], value: f32) -> Option<DragState> {
    if !value.is_finite() || !pivot.is_finite() {
        return None;
    }
    let index = axis_index(handle);
    let axis = unit(axes[index])?;
    let state = match handle {
        Handle::MoveX | Handle::MoveY | Handle::MoveZ => DragState {
            world_delta: Affine3A::from_translation(axis * value),
            readout: Readout::Distance(value),
        },
        Handle::RotX | Handle::RotY | Handle::RotZ => DragState {
            world_delta: if value == 0.0 {
                Affine3A::IDENTITY
            } else {
                about_pivot(pivot, Affine3A::from_axis_angle(axis, value.to_radians()))
            },
            readout: Readout::AngleDeg(value),
        },
        Handle::ScaleX | Handle::ScaleY | Handle::ScaleZ => {
            if value <= 0.0 {
                return None;
            }
            DragState {
                world_delta: if value == 1.0 {
                    Affine3A::IDENTITY
                } else {
                    axis_scale(pivot, &axes, index, value)?
                },
                readout: Readout::Factor(value),
            }
        }
        Handle::ScaleAll => return None,
    };
    state.world_delta.is_finite().then_some(state)
}

/// 位置・回転・大きさ。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trs {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Trs {
    fn default() -> Self {
        Self {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

/// 分けて合成し直した結果が元と合わない（傾いた拡大を含む）か、大きさが 0 か、有限でなければ
/// `None`（数値の欄を無効にする）。裏返し（行列式が負）は X の大きさを負にして表す。
pub fn decompose(local: &Affine3A) -> Option<Trs> {
    if !local.is_finite() {
        return None;
    }
    let det = local.matrix3.determinant();
    if !det.is_finite() || det.abs() < 1.0e-30 {
        return None;
    }
    let (scale, rotation, translation) = local.to_scale_rotation_translation();
    if !scale.is_finite() || !rotation.is_finite() || !translation.is_finite() {
        return None;
    }
    let trs = Trs {
        translation,
        rotation: rotation.normalize(),
        scale,
    };
    let back = compose(&trs);
    let magnitude = local
        .matrix3
        .to_cols_array()
        .iter()
        .fold(0.0_f32, |acc, value| acc.max(value.abs()));
    let tolerance = 1.0e-4 * magnitude.max(1.0e-30);
    let close = back
        .matrix3
        .to_cols_array()
        .iter()
        .zip(local.matrix3.to_cols_array().iter())
        .all(|(a, b)| (a - b).abs() <= tolerance);
    close.then_some(trs)
}

pub fn compose(trs: &Trs) -> Affine3A {
    Affine3A::from_scale_rotation_translation(trs.scale, trs.rotation, trs.translation)
}

/// 回転を度のオイラー角で表す。順は X・Y・Z の外部回転（ワールドの X、次に Y、次に Z の軸の
/// まわりに回す。行列では `Rz × Ry × Rx`）。Y が ±90° 付近では X と Z の分け方は一意でない。
pub fn euler_deg(rotation: Quat) -> Vec3 {
    if !rotation.is_finite() || rotation.length_squared() < 1.0e-12 {
        return Vec3::ZERO;
    }
    let (x, y, z) = rotation.normalize().to_euler(EulerRot::XYZEx);
    Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
}

/// 度のオイラー角（[`euler_deg`] と同じ順）から回転を作る。有限でなければ単位回転。
pub fn from_euler_deg(deg: Vec3) -> Quat {
    if !deg.is_finite() {
        return Quat::IDENTITY;
    }
    Quat::from_euler(
        EulerRot::XYZEx,
        deg.x.to_radians(),
        deg.y.to_radians(),
        deg.z.to_radians(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: [f32; 2] = [800.0, 600.0];

    fn frame(axes: [Vec3; 3]) -> GizmoFrame {
        GizmoFrame {
            pivot: Vec3::new(1.0, 0.5, -0.5),
            axes,
            camera: OrbitCamera {
                target: Vec3::new(1.0, 0.5, -0.5),
                yaw: 35_f32.to_radians(),
                pitch: 25_f32.to_radians(),
                distance: 6.0,
                fov_y: 40_f32.to_radians(),
                projection: Projection::Perspective,
            },
            viewport_px: VIEWPORT,
            size_px: 90.0,
        }
    }

    fn close(a: &Affine3A, b: &Affine3A) -> bool {
        a.to_cols_array()
            .iter()
            .zip(b.to_cols_array().iter())
            .all(|(x, y)| (x - y).abs() <= 1.0e-4 * (1.0 + y.abs()))
    }

    fn value_of(readout: Readout) -> f32 {
        match readout {
            Readout::Distance(v) | Readout::AngleDeg(v) | Readout::Factor(v) => v,
        }
    }

    /// 同じ量のドラッグの結果（`Drag::update` の差分）と一致する。
    #[test]
    fn key_delta_matches_drag_result() {
        let tilted = Quat::from_euler(EulerRot::XYZ, 0.3, -0.4, 0.2);
        let part_axes = [tilted * Vec3::X, tilted * Vec3::Y, tilted * Vec3::Z];
        let world = [Vec3::X, Vec3::Y, Vec3::Z];
        let cases = [
            (Handle::MoveX, world, Vec2::new(0.25, 0.05)),
            (Handle::MoveY, world, Vec2::new(0.02, 0.3)),
            (Handle::MoveZ, world, Vec2::new(-0.2, -0.1)),
            (Handle::RotX, world, Vec2::new(0.1, 0.25)),
            (Handle::RotY, world, Vec2::new(0.3, -0.1)),
            (Handle::RotZ, world, Vec2::new(-0.2, 0.2)),
            (Handle::ScaleX, part_axes, Vec2::new(0.3, 0.1)),
            (Handle::ScaleY, part_axes, Vec2::new(0.05, 0.35)),
            (Handle::ScaleZ, part_axes, Vec2::new(-0.25, -0.15)),
        ];
        for (handle, axes, offset) in cases {
            let frame = frame(axes);
            let grab = grab_points_of(handle, &frame);
            let drag = Drag::begin(handle, &frame, grab).expect("掴める");
            let state = drag.update(grab + offset).expect("値が出る");
            let value = value_of(state.readout);
            assert!(value.abs() > 1.0e-3, "{handle:?} の量が小さすぎる: {value}");
            let keyed = key_delta(handle, frame.pivot, axes, value).expect("量から作れる");
            assert!(
                close(&keyed.world_delta, &state.world_delta),
                "{handle:?}: {:?} と {:?}",
                keyed.world_delta,
                state.world_delta
            );
            assert_eq!(keyed.readout, state.readout, "{handle:?}");
        }
    }

    /// ハンドルの掴める点の正規化座標（軸の線の先寄り、輪の上の点）。
    fn grab_points_of(handle: Handle, frame: &GizmoFrame) -> Vec2 {
        let tool = match handle {
            Handle::MoveX | Handle::MoveY | Handle::MoveZ => Tool::Move,
            Handle::RotX | Handle::RotY | Handle::RotZ => Tool::Rotate,
            _ => Tool::Scale,
        };
        let shape = layout(tool, frame)
            .into_iter()
            .find(|s| s.handle == handle)
            .expect("形がある");
        let point = if shape.closed {
            shape.points[shape.points.len() / 4]
        } else {
            shape.points[0].lerp(shape.points[1], 0.8)
        };
        let viewport = Vec2::from(frame.viewport_px);
        Vec2::new(
            point.x / viewport.x * 2.0 - 1.0,
            1.0 - point.y / viewport.y * 2.0,
        )
    }

    /// 視線と軸が平行でドラッグでは値が定まらない向きでも、量からは差分が出る。
    #[test]
    fn key_delta_works_when_view_is_parallel() {
        let base = frame([Vec3::X, Vec3::Y, Vec3::Z]);
        let (_, _, forward) = base.camera.axes();
        let axes = [forward, forward.any_orthonormal_vector(), Vec3::ZERO];
        let axes = [axes[0], axes[1], axes[0].cross(axes[1])];
        let parallel = GizmoFrame { axes, ..base };
        let center = parallel
            .camera
            .project(parallel.pivot, 800.0 / 600.0)
            .unwrap();
        assert!(
            Drag::begin(Handle::MoveX, &parallel, center).is_none(),
            "ドラッグでは定まらない向き"
        );
        let moved = key_delta(Handle::MoveX, parallel.pivot, axes, 2.5).expect("量から作れる");
        assert!(
            moved
                .world_delta
                .translation
                .distance(Vec3A::from(forward * 2.5))
                < 1.0e-5
        );
        assert_eq!(moved.readout, Readout::Distance(2.5));
        let turned = key_delta(Handle::RotZ, parallel.pivot, axes, 90.0).expect("回転も作れる");
        assert!(
            turned
                .world_delta
                .transform_point3(parallel.pivot)
                .distance(parallel.pivot)
                < 1.0e-5
        );
        let scaled = key_delta(Handle::ScaleX, parallel.pivot, axes, 2.0).expect("拡大も作れる");
        let tip = scaled
            .world_delta
            .transform_point3(parallel.pivot + forward);
        assert!(tip.distance(parallel.pivot + forward * 2.0) < 1.0e-5);
    }

    /// 有限でない量、0 以下の倍率、全体の拡大、つぶれた軸は作らない。変化なしは単位行列そのもの。
    #[test]
    fn key_delta_refuses_bad_amounts() {
        let axes = [Vec3::X, Vec3::Y, Vec3::Z];
        let pivot = Vec3::new(1.0, 2.0, 3.0);
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for handle in [Handle::MoveX, Handle::RotY, Handle::ScaleZ] {
                assert!(
                    key_delta(handle, pivot, axes, value).is_none(),
                    "{handle:?} {value}"
                );
            }
        }
        for value in [0.0, -0.5, -2.0] {
            assert!(
                key_delta(Handle::ScaleX, pivot, axes, value).is_none(),
                "{value}"
            );
        }
        assert!(key_delta(Handle::ScaleAll, pivot, axes, 2.0).is_none());
        assert!(key_delta(Handle::MoveX, Vec3::NAN, axes, 1.0).is_none());
        assert!(key_delta(Handle::MoveY, pivot, [Vec3::X, Vec3::ZERO, Vec3::Z], 1.0).is_none());
        assert!(key_delta(Handle::ScaleY, pivot, [Vec3::X, Vec3::X, Vec3::Z], 2.0).is_none());
        // 負の移動と回転は量として正しい。
        assert!(key_delta(Handle::MoveX, pivot, axes, -3.0).is_some());
        assert!(key_delta(Handle::RotX, pivot, axes, -45.0).is_some());
        for (handle, neutral) in [
            (Handle::MoveZ, 0.0),
            (Handle::RotX, 0.0),
            (Handle::ScaleY, 1.0),
        ] {
            let state = key_delta(handle, pivot, axes, neutral).expect("変化なし");
            assert_eq!(state.world_delta, Affine3A::IDENTITY, "{handle:?}");
        }
    }
}
