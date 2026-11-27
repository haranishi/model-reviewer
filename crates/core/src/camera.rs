//! 軌道カメラ。定数はここへまとめる。アプリがスクロール量から倍率を作るときは
//! [`ZOOM_EXP_PER_PX`] を使い、距離の制限は境界球半径に [`DISTANCE_MIN_RATIO`] と
//! [`DISTANCE_MAX_RATIO`] を掛けた値を `zoom` の `limits` へ渡す。

use glam::{Mat4, Vec2, Vec3};

use crate::math::{Aabb, Ray};

/// 回転。1ポイントあたりの角度（度）。画面の +x で yaw が増え、+y（上）で pitch が増える。
pub const ORBIT_DEG_PER_PX: f32 = 0.4;
/// ズーム。倍率は `exp(-スクロール量 * この値)`。1未満が寄りである。
pub const ZOOM_EXP_PER_PX: f32 = 0.002;
/// 距離の下限と上限。境界球の半径に対する倍率で、`zoom` の呼び出し側が使う。
pub const DISTANCE_MIN_RATIO: f32 = 1.0e-3;
pub const DISTANCE_MAX_RATIO: f32 = 1.0e3;

/// 仰角の上限。真上と真下（上面図・下面図）を出せるように ±90°。視点の行列は方位と仰角から
/// 直接作るので、90°でも右・上・奥の3軸が決まる。
const PITCH_LIMIT_DEG: f32 = 90.0;
const HOME_YAW_DEG: f32 = 35.0;
const HOME_PITCH_DEG: f32 = 20.0;
const HOME_FOV_Y_DEG: f32 = 40.0;
const FIT_MARGIN: f32 = 1.05;
const MIN_DISTANCE: f32 = 1.0e-4;
/// 平行投影の視線の出発点を、注視点の面から距離の何倍だけ手前へ下げるか。平行では目の後ろの
/// 形状も見えるので、当たり判定の光線（`t >= 0` だけを見る）がそれを取りこぼさないようにする。
/// 距離の下限は境界球半径の 1/1000（[`DISTANCE_MIN_RATIO`]）なので、それより深く下げる。
/// 下げすぎると f32 の誤差が増えるが、2048倍でも画面の高さの約 0.03% に収まる。
const ORTHO_RAY_BACKOFF: f32 = 2048.0;

/// 投影。平行のときの見える高さは `2 × distance × tan(fov_y / 2)`（切り替えても注視点での
/// 見かけの大きさが変わらず、保存の項目も増えない）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Projection {
    #[default]
    Perspective,
    Orthographic,
}

/// 決まった向き。モデルの正面は +Z なので、`Front` は +Z 側から見る（方位0°・仰角0°）。
/// `Top` は仰角90°、`Bottom` は仰角−90°。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ViewDir {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitCamera {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
    pub projection: Projection,
}

/// 床の格子の間隔。`10^floor(log10(distance / 5))`。状態欄と描画が同じ値を使う。
/// 距離が有限の正の数でなければ 1。結果は f32 で表せる範囲（10^-30〜10^30）に収める。
pub fn grid_spacing(distance: f32) -> f32 {
    if !distance.is_finite() || distance <= 0.0 {
        return 1.0;
    }
    let exponent = (distance / 5.0).log10().floor().clamp(-30.0, 30.0);
    let spacing = 10.0_f32.powi(exponent as i32);
    if spacing.is_finite() && spacing > 0.0 {
        spacing
    } else {
        1.0
    }
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            distance: 1.0,
            fov_y: HOME_FOV_Y_DEG.to_radians(),
            projection: Projection::Perspective,
        }
    }
}

impl OrbitCamera {
    pub fn home(bounds: &Aabb, aspect: f32) -> Self {
        let mut camera = Self {
            target: bounds.center(),
            yaw: HOME_YAW_DEG.to_radians(),
            pitch: HOME_PITCH_DEG.to_radians(),
            distance: 1.0,
            fov_y: HOME_FOV_Y_DEG.to_radians(),
            projection: Projection::Perspective,
        };
        camera.fit(bounds, aspect);
        camera
    }

    pub fn fit(&mut self, bounds: &Aabb, aspect: f32) {
        if bounds.is_empty() {
            return;
        }
        self.target = bounds.center();
        self.distance = fitting_distance(bounds.radius(), self.fov_y, aspect);
    }

    pub fn orbit(&mut self, delta_px: Vec2) {
        if !delta_px.is_finite() {
            return;
        }
        let step = ORBIT_DEG_PER_PX.to_radians();
        let yaw = self.yaw + delta_px.x * step;
        self.yaw = if yaw.is_finite() { yaw } else { 0.0 };
        self.pitch = clamp_pitch(self.pitch + delta_px.y * step);
    }

    pub fn pan(&mut self, delta_px: Vec2, viewport_height_px: f32) {
        if !delta_px.is_finite() || viewport_height_px <= 0.0 || !viewport_height_px.is_finite() {
            return;
        }
        let (right, up, _) = self.basis();
        let half = (self.fov() * 0.5).tan();
        if !half.is_finite() {
            return;
        }
        let scale = 2.0 * positive_distance(self.distance) * half / viewport_height_px;
        let next = self.target + (right * delta_px.x + up * delta_px.y) * scale;
        if next.is_finite() {
            self.target = next;
        }
    }

    pub fn zoom(&mut self, factor: f32, cursor_ndc: Option<Vec2>, aspect: f32, limits: (f32, f32)) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let before = cursor_ndc.and_then(|ndc| self.point_on_target_plane(ndc, aspect));
        self.distance = clamp_distance(self.distance * factor, limits);
        let Some(before) = before else {
            return;
        };
        let Some(ndc) = cursor_ndc else {
            return;
        };
        if let Some(after) = self.point_on_target_plane(ndc, aspect) {
            let next = self.target + (before - after);
            if next.is_finite() {
                self.target = next;
            }
        }
    }

    /// 方位と仰角だけを決まった向きに置き換える（注視点・距離・投影は変えない。補間しない）。
    /// 右は +X 側から見る（方位90°）、左は −X 側（方位−90°）、後ろは −Z 側（方位180°）。
    /// 上と下は方位0°なので、上面図では画面の下が前（+Z）になる。
    pub fn snap(&mut self, dir: ViewDir) {
        let (yaw_deg, pitch_deg) = match dir {
            ViewDir::Front => (0.0_f32, 0.0_f32),
            ViewDir::Back => (180.0, 0.0),
            ViewDir::Right => (90.0, 0.0),
            ViewDir::Left => (-90.0, 0.0),
            ViewDir::Top => (0.0, PITCH_LIMIT_DEG),
            ViewDir::Bottom => (0.0, -PITCH_LIMIT_DEG),
        };
        self.yaw = yaw_deg.to_radians();
        self.pitch = pitch_deg.to_radians();
    }

    /// 画面の3軸（右・上・視線の向き）。どれも長さ1。仰角±90°でも決まる。
    pub fn axes(&self) -> (Vec3, Vec3, Vec3) {
        self.basis()
    }

    /// `point` の奥行きで、画面の正規化座標の y が 1 進むあいだのワールドの長さ（見える高さの
    /// 半分）。透視では目の後ろの点は `None`。平行では奥行きに依らない。画面上の大きさを
    /// 一定にしたいもの（ハンドル）の長さを決めるのに使う。
    pub fn half_height_at(&self, point: Vec3) -> Option<f32> {
        let half = match self.projection {
            Projection::Orthographic => self.ortho_half_height(),
            Projection::Perspective => {
                if !point.is_finite() {
                    return None;
                }
                let (_, _, forward) = self.basis();
                let depth = (point - self.eye()).dot(forward);
                if depth <= 1.0e-6 || !depth.is_finite() {
                    return None;
                }
                depth * (self.fov() * 0.5).tan()
            }
        };
        if half.is_finite() && half > 0.0 {
            Some(half)
        } else {
            None
        }
    }

    /// 平行投影のときの見える高さの半分（注視点の面での透視の見える高さと同じ）。
    pub fn ortho_half_height(&self) -> f32 {
        positive_distance(self.distance) * (self.fov() * 0.5).tan()
    }

    pub fn eye(&self) -> Vec3 {
        let target = finite_vec3(self.target);
        let (_, _, forward) = self.basis();
        target - forward * positive_distance(self.distance)
    }

    /// 右手系の視点の行列。`look_at`（上向きの基準がY）は仰角90°で壊れるので、方位と仰角から
    /// 作った3軸をそのまま並べる（`look_at` と同じ並びと符号）。
    pub fn view(&self) -> Mat4 {
        let eye = self.eye();
        let (right, up, forward) = self.basis();
        Mat4::from_cols(
            Vec3::new(right.x, up.x, -forward.x).extend(0.0),
            Vec3::new(right.y, up.y, -forward.y).extend(0.0),
            Vec3::new(right.z, up.z, -forward.z).extend(0.0),
            Vec3::new(-eye.dot(right), -eye.dot(up), eye.dot(forward)).extend(1.0),
        )
    }

    /// 射影の行列。深度は反転Z（手前ほど大きく、0〜1）。透視は遠い面が無限遠。平行は、
    /// 近い面と遠い面を目の位置と全体の境界から決め（目の後ろの形状も切らない）、glam の
    /// 引数の近い面と遠い面を入れ替えて反転Zにする。
    pub fn proj(&self, aspect: f32, scene: &Aabb) -> Mat4 {
        let aspect = aspect_or_one(aspect);
        match self.projection {
            Projection::Perspective => {
                glam::camera::rh::proj::directx::perspective_infinite_reverse(
                    self.fov(),
                    aspect,
                    near_plane(self.eye(), scene),
                )
            }
            Projection::Orthographic => {
                let half_h = self.ortho_half_height();
                let half_w = half_h * aspect;
                let (near, far) = self.ortho_depth_range(scene);
                glam::camera::rh::proj::directx::orthographic(
                    -half_w, half_w, -half_h, half_h, far, near,
                )
            }
        }
    }

    /// 画面上の位置（正規化座標。中心が 0、上が +y）から出る視線。透視は目から放射状に出る。
    /// 平行は画面上の位置から視線の向きへ平行に出て、出発点は注視点の面から
    /// 距離 × [`ORTHO_RAY_BACKOFF`] だけ手前（目の後ろの形状も `t >= 0` で当たる）。
    pub fn screen_ray(&self, ndc: Vec2, aspect: f32) -> Ray {
        let (right, up, forward) = self.basis();
        let ndc = if ndc.is_finite() { ndc } else { Vec2::ZERO };
        let aspect = aspect_or_one(aspect);
        match self.projection {
            Projection::Perspective => {
                let origin = self.eye();
                let tan_half = (self.fov() * 0.5).tan();
                let dir = right * (ndc.x * aspect * tan_half) + up * (ndc.y * tan_half) + forward;
                Ray {
                    origin,
                    dir: normalize_or(dir, forward),
                }
            }
            Projection::Orthographic => {
                let half_h = self.ortho_half_height();
                let on_plane = finite_vec3(self.target)
                    + right * (ndc.x * aspect * half_h)
                    + up * (ndc.y * half_h);
                let back = positive_distance(self.distance) * ORTHO_RAY_BACKOFF;
                let origin = on_plane - forward * back;
                Ray {
                    origin: if origin.is_finite() {
                        origin
                    } else {
                        self.eye()
                    },
                    dir: forward,
                }
            }
        }
    }

    /// `screen_ray` と同じ視線で、平行のときだけ出発点を場面の境界のすぐ手前
    /// （描画の近い面と同じ位置）に置く。`screen_ray` の出発点は注視点から距離の2048倍も手前に
    /// あり、交点の座標に出発点の大きさに比例した丸めが出る（計測の距離が透視と表示の桁で
    /// 食い違う）。境界の手前から出しても、描かれる形状はすべて `t >= 0` の側にある。
    /// 場面が空なら `screen_ray` と同じ。
    pub fn screen_ray_in(&self, ndc: Vec2, aspect: f32, scene: &Aabb) -> Ray {
        let ray = self.screen_ray(ndc, aspect);
        if self.projection != Projection::Orthographic || scene.is_empty() {
            return ray;
        }
        let (near, _) = self.ortho_depth_range(scene);
        let (right, up, forward) = self.basis();
        let ndc = if ndc.is_finite() { ndc } else { Vec2::ZERO };
        let half_h = self.ortho_half_height();
        // 注視点の面（目からの深さ `distance`）の上の点から、近い面（深さ `near`）へ戻す。
        // 遠い出発点を経由せずに足すので、出発点の大きさは場面の大きさと同じ桁に収まる。
        let on_plane = finite_vec3(self.target)
            + right * (ndc.x * aspect_or_one(aspect) * half_h)
            + up * (ndc.y * half_h);
        let origin = on_plane + forward * (near - positive_distance(self.distance));
        if origin.is_finite() && forward == ray.dir {
            Ray {
                origin,
                dir: ray.dir,
            }
        } else {
            ray
        }
    }

    /// 世界の点を画面の正規化座標へ。透視では目の後ろの点は `None`。平行では奥行きに依らず
    /// 位置が決まる（近い面は境界全体を含むので、目の後ろの形状も描かれる）。
    pub fn project(&self, world: Vec3, aspect: f32) -> Option<Vec2> {
        if !world.is_finite() {
            return None;
        }
        let (right, up, forward) = self.basis();
        if self.projection == Projection::Orthographic {
            let half_h = self.ortho_half_height();
            let aspect = aspect_or_one(aspect);
            if half_h <= 0.0 || !half_h.is_finite() {
                return None;
            }
            let delta = world - finite_vec3(self.target);
            let ndc = Vec2::new(delta.dot(right) / (aspect * half_h), delta.dot(up) / half_h);
            return if ndc.is_finite() { Some(ndc) } else { None };
        }
        let delta = world - self.eye();
        let depth = delta.dot(forward);
        if depth <= 1.0e-6 || !depth.is_finite() {
            return None;
        }
        let tan_half = (self.fov() * 0.5).tan();
        if tan_half <= 0.0 || !tan_half.is_finite() {
            return None;
        }
        let aspect = aspect_or_one(aspect);
        let ndc = Vec2::new(
            delta.dot(right) / (aspect * tan_half * depth),
            delta.dot(up) / (tan_half * depth),
        );
        if ndc.is_finite() { Some(ndc) } else { None }
    }

    fn point_on_target_plane(&self, ndc: Vec2, aspect: f32) -> Option<Vec3> {
        let ray = self.screen_ray(ndc, aspect);
        let forward = self.basis().2;
        let denom = ray.dir.dot(forward);
        if !denom.is_finite() || denom.abs() < 1.0e-6 {
            return None;
        }
        let t = (finite_vec3(self.target) - ray.origin).dot(forward) / denom;
        if !t.is_finite() || t <= 0.0 {
            return None;
        }
        let point = ray.origin + ray.dir * t;
        if point.is_finite() { Some(point) } else { None }
    }

    /// 平行投影の近い面と遠い面（目からの視線方向の距離。負なら目の後ろ）。境界が空なら
    /// 注視点のまわりに距離の幅を取る。両者は必ず離れている。
    fn ortho_depth_range(&self, scene: &Aabb) -> (f32, f32) {
        let distance = positive_distance(self.distance);
        let (_, _, forward) = self.basis();
        let (center_depth, radius) = if scene.is_empty() {
            (distance, distance)
        } else {
            let depth = (scene.center() - self.eye()).dot(forward);
            let radius = scene.radius();
            if depth.is_finite() && radius.is_finite() {
                (depth, radius)
            } else {
                (distance, distance)
            }
        };
        let margin = (radius * 0.01).max(distance * 1.0e-3).max(1.0e-4);
        let near = center_depth - radius - margin;
        let far = center_depth + radius + margin;
        if near.is_finite() && far.is_finite() && far > near {
            (near, far)
        } else {
            (-distance, distance * 2.0)
        }
    }

    /// 右・上・奥（視線の向き）の3軸。方位と仰角から直接作る。右は仰角に依らず水平で、
    /// 真上・真下を向いても決まる。
    fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let pitch = clamp_pitch(self.pitch);
        let yaw = if self.yaw.is_finite() { self.yaw } else { 0.0 };
        let (sp, cp) = pitch.sin_cos();
        let (sy, cy) = yaw.sin_cos();
        let forward = normalize_or(-Vec3::new(cp * sy, sp, cp * cy), Vec3::NEG_Z);
        let right = normalize_or(Vec3::new(cy, 0.0, -sy), Vec3::X);
        let up = normalize_or(right.cross(forward), Vec3::Y);
        (right, up, forward)
    }

    fn fov(&self) -> f32 {
        if self.fov_y.is_finite()
            && self.fov_y > 1.0e-4
            && self.fov_y < std::f32::consts::PI - 1.0e-4
        {
            self.fov_y
        } else {
            HOME_FOV_Y_DEG.to_radians()
        }
    }
}

fn fitting_distance(radius: f32, fov_y: f32, aspect: f32) -> f32 {
    if radius <= 0.0 || !radius.is_finite() {
        return 1.0;
    }
    let fov_y = if fov_y.is_finite() && fov_y > 1.0e-4 && fov_y < std::f32::consts::PI - 1.0e-4 {
        fov_y
    } else {
        HOME_FOV_Y_DEG.to_radians()
    };
    let aspect = aspect_or_one(aspect);
    let tan_half_y = (fov_y * 0.5).tan();
    let fov_x = 2.0 * (tan_half_y * aspect).atan();
    let half = (fov_y.min(fov_x) * 0.5).clamp(1.0e-4, std::f32::consts::FRAC_PI_2 - 1.0e-4);
    let distance = radius / half.sin() * FIT_MARGIN;
    if distance.is_finite() && distance > MIN_DISTANCE {
        distance
    } else {
        1.0
    }
}

fn near_plane(eye: Vec3, scene: &Aabb) -> f32 {
    if scene.is_empty() {
        return 1.0e-2;
    }
    let radius = scene.radius();
    let dist = (eye - scene.center()).length();
    let near = ((dist - radius) * 0.5).max(radius * 1.0e-4);
    if near.is_finite() && near > 1.0e-6 {
        near
    } else {
        1.0e-3
    }
}

fn clamp_pitch(pitch: f32) -> f32 {
    let limit = PITCH_LIMIT_DEG.to_radians();
    if pitch.is_finite() {
        pitch.clamp(-limit, limit)
    } else {
        0.0
    }
}

fn clamp_distance(distance: f32, limits: (f32, f32)) -> f32 {
    let mut lo = if limits.0.is_finite() {
        limits.0
    } else {
        MIN_DISTANCE
    };
    let mut hi = if limits.1.is_finite() {
        limits.1
    } else {
        f32::MAX
    };
    if lo > hi {
        std::mem::swap(&mut lo, &mut hi);
    }
    lo = lo.max(MIN_DISTANCE);
    hi = hi.max(lo);
    if distance.is_finite() {
        distance.clamp(lo, hi)
    } else {
        lo
    }
}

fn aspect_or_one(aspect: f32) -> f32 {
    if aspect.is_finite() && aspect > 1.0e-6 {
        aspect
    } else {
        1.0
    }
}

fn positive_distance(distance: f32) -> f32 {
    if distance.is_finite() && distance > MIN_DISTANCE {
        distance
    } else {
        MIN_DISTANCE
    }
}

fn finite_vec3(value: Vec3) -> Vec3 {
    if value.is_finite() { value } else { Vec3::ZERO }
}

fn normalize_or(value: Vec3, fallback: Vec3) -> Vec3 {
    if value.is_finite() && value.length_squared() > 1.0e-12 {
        value.normalize()
    } else {
        fallback
    }
}
