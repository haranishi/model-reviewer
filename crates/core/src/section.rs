//! 断面。ワールドの軸に垂直な平面1枚で、片側を消す。描画（画素を捨てる）・当たり判定
//! （視線の範囲を切る）・画面の部品（枠と矢印）の3か所が、同じこの型を読む。
//!
//! 消すかどうかの式は、シェーダーと同じ `(座標の軸成分 − offset) × 符号 > 0` にする。

use glam::Vec3;

use crate::math::{Aabb, Ray};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    /// `Vec3` の成分の番号（X=0・Y=1・Z=2）。
    pub fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }

    /// 軸の正の向きの単位ベクトル。
    pub fn unit(self) -> Vec3 {
        match self {
            Self::X => Vec3::X,
            Self::Y => Vec3::Y,
            Self::Z => Vec3::Z,
        }
    }
}

/// 断面の平面。`offset` はワールドの座標（シーンの単位）。
/// `flipped = false` なら、軸の正の側を消す。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionPlane {
    pub axis: Axis,
    pub offset: f32,
    pub flipped: bool,
}

impl SectionPlane {
    /// 境界箱の中央を通る平面。`flipped = false`。境界箱が空か中央が有限でなければ 0 を通る。
    pub fn centered(axis: Axis, bounds: &Aabb) -> Self {
        let center = bounds.center()[axis.index()];
        Self {
            axis,
            offset: if center.is_finite() { center } else { 0.0 },
            flipped: false,
        }
    }

    /// 残す側から消す側へ向く単位ベクトル。`flipped = false` なら軸の正の向き。
    pub fn normal(&self) -> Vec3 {
        if self.flipped {
            -self.axis.unit()
        } else {
            self.axis.unit()
        }
    }

    /// 消す側か。平面の上の点は消さない。有限でない座標は消さない。
    pub fn cuts(&self, world: Vec3) -> bool {
        let value = (world[self.axis.index()] - self.offset) * self.sign();
        value > 0.0
    }

    /// 視線が残す側にある `t` の範囲（0 以上）。全部が消す側なら `None`。
    pub fn ray_range(&self, ray: &Ray) -> Option<(f32, f32)> {
        if !ray.origin.is_finite() || !ray.dir.is_finite() {
            return None;
        }
        // 位置が有限でない平面は何も消さない（`cuts` と同じ）。
        if !self.offset.is_finite() {
            return Some((0.0, f32::INFINITY));
        }
        let axis = self.axis.index();
        let sign = self.sign();
        // 出発点での「消す側への深さ」と、t が1進むごとの増え方。
        let start = (ray.origin[axis] - self.offset) * sign;
        let rate = ray.dir[axis] * sign;
        if !start.is_finite() {
            return None;
        }
        if rate.abs() <= 1.0e-12 {
            return (start <= 0.0).then_some((0.0, f32::INFINITY));
        }
        let crossing = -start / rate;
        if !crossing.is_finite() {
            return None;
        }
        if rate > 0.0 {
            // 消す側へ向かう。平面に着くまでが残す側。
            (crossing >= 0.0).then_some((0.0, crossing))
        } else {
            // 残す側へ向かう。平面を越えた後が残す側。
            Some((crossing.max(0.0), f32::INFINITY))
        }
    }

    /// `offset` を境界箱の範囲に収める。有限でなければ中央へ置き直す。
    pub fn clamped(self, bounds: &Aabb) -> Self {
        if !self.offset.is_finite() {
            return Self {
                flipped: self.flipped,
                ..Self::centered(self.axis, bounds)
            };
        }
        if bounds.is_empty() {
            return self;
        }
        let axis = self.axis.index();
        let (lo, hi) = (bounds.min[axis], bounds.max[axis]);
        if !lo.is_finite() || !hi.is_finite() || lo > hi {
            return self;
        }
        Self {
            offset: self.offset.clamp(lo, hi),
            ..self
        }
    }

    /// 境界箱を切った長方形の4つの角（枠を描く）。
    pub fn frame_corners(&self, bounds: &Aabb) -> [Vec3; 4] {
        let axis = self.axis.index();
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        let (min, max) = if bounds.is_empty() {
            (Vec3::ZERO, Vec3::ZERO)
        } else {
            (bounds.min, bounds.max)
        };
        let corner = |a: f32, b: f32| {
            let mut point = Vec3::ZERO;
            point[axis] = self.offset;
            point[u] = a;
            point[v] = b;
            point
        };
        [
            corner(min[u], min[v]),
            corner(max[u], min[v]),
            corner(max[u], max[v]),
            corner(min[u], max[v]),
        ]
    }

    /// 描画の定数に入れる符号つきの番号。X ±1、Y ±2、Z ±3。正は「軸の正の側を消す」。
    pub fn code(&self) -> f32 {
        let number = match self.axis {
            Axis::X => 1.0,
            Axis::Y => 2.0,
            Axis::Z => 3.0,
        };
        number * self.sign()
    }

    fn sign(&self) -> f32 {
        if self.flipped { -1.0 } else { 1.0 }
    }
}

/// 矢印のドラッグ。掴んだ時点の平面と視線から、いまの視線での新しい `offset` を返す。
/// 視線が軸とほぼ平行なら `None`（前の値を保つ）。
pub fn drag_offset(
    start: &SectionPlane,
    bounds: &Aabb,
    start_ray: &Ray,
    now_ray: &Ray,
) -> Option<f32> {
    let axis = start.axis.unit();
    // 軸の直線は、枠の中心（境界箱の中心を平面の上へ移した点）を通る。
    let mut anchor = bounds.center();
    anchor[start.axis.index()] = start.offset;
    if !anchor.is_finite() {
        return None;
    }
    let s_start = closest_on_axis(anchor, axis, start_ray)?;
    let s_now = closest_on_axis(anchor, axis, now_ray)?;
    let offset = start.offset + (s_now - s_start);
    offset.is_finite().then_some(offset)
}

/// 直線 `anchor + axis × s`（`axis` は長さ1）の上で、視線の直線に最も近い点の `s`。
/// 視線が軸とほぼ平行なら `None`。
fn closest_on_axis(anchor: Vec3, axis: Vec3, ray: &Ray) -> Option<f32> {
    if !ray.origin.is_finite() || !ray.dir.is_finite() {
        return None;
    }
    let dir = ray.dir.try_normalize()?;
    let b = axis.dot(dir);
    let denom = 1.0 - b * b;
    // 視線と軸の角度がおよそ0.1°より小さいとき。
    if !denom.is_finite() || denom < 3.0e-6 {
        return None;
    }
    let w = anchor - ray.origin;
    let s = (b * dir.dot(w) - axis.dot(w)) / denom;
    s.is_finite().then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(actual: f32, expected: f32) -> bool {
        (actual - expected).abs() <= 1.0e-6 + expected.abs() * 1.0e-5
    }

    fn bounds() -> Aabb {
        Aabb {
            min: Vec3::new(-1.0, 2.0, -4.0),
            max: Vec3::new(3.0, 6.0, 0.0),
        }
    }

    #[test]
    fn centered_is_the_middle_of_each_axis() {
        let b = bounds();
        assert!(near(SectionPlane::centered(Axis::X, &b).offset, 1.0));
        assert!(near(SectionPlane::centered(Axis::Y, &b).offset, 4.0));
        assert!(near(SectionPlane::centered(Axis::Z, &b).offset, -2.0));
        assert!(!SectionPlane::centered(Axis::Z, &b).flipped);
        assert_eq!(SectionPlane::centered(Axis::Y, &Aabb::empty()).offset, 0.0);
    }

    #[test]
    fn normal_code_and_cuts_agree() {
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            for flipped in [false, true] {
                let plane = SectionPlane {
                    axis,
                    offset: 0.5,
                    flipped,
                };
                let normal = plane.normal();
                let on = axis.unit() * 0.5;
                assert!(!plane.cuts(on), "平面の上は消さない");
                assert!(plane.cuts(on + normal));
                assert!(!plane.cuts(on - normal));
                assert_eq!(plane.code().abs(), (axis.index() + 1) as f32);
                assert_eq!(plane.code() > 0.0, !flipped);
                assert_eq!(normal[axis.index()] > 0.0, !flipped);
            }
        }
        let plane = SectionPlane::centered(Axis::X, &bounds());
        assert!(!plane.cuts(Vec3::new(f32::NAN, 0.0, 0.0)));
    }

    #[test]
    fn clamped_stays_inside_and_recenters_non_finite() {
        let b = bounds();
        let plane = SectionPlane {
            axis: Axis::Y,
            offset: 100.0,
            flipped: true,
        };
        let clamped = plane.clamped(&b);
        assert_eq!(clamped.offset, 6.0);
        assert!(clamped.flipped);
        let low = SectionPlane {
            offset: -100.0,
            ..plane
        }
        .clamped(&b);
        assert_eq!(low.offset, 2.0);
        let nan = SectionPlane {
            offset: f32::NAN,
            ..plane
        }
        .clamped(&b);
        assert!(near(nan.offset, 4.0));
        assert!(nan.flipped);
        let inside = SectionPlane {
            offset: 3.0,
            ..plane
        };
        assert_eq!(inside.clamped(&b), inside);
        assert_eq!(inside.clamped(&Aabb::empty()), inside);
    }

    fn ray(origin: Vec3, dir: Vec3) -> Ray {
        Ray {
            origin,
            dir: dir.normalize(),
        }
    }

    #[test]
    fn ray_range_agrees_with_cuts() {
        let dirs = [
            Vec3::new(1.0, 0.2, -0.3),
            Vec3::new(-1.0, 0.5, 0.1),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.3, -0.4, 1.0),
            Vec3::new(0.0, 0.0, -1.0),
        ];
        let origins = [
            Vec3::new(-3.0, 0.0, 0.0),
            Vec3::new(3.0, 4.0, -2.0),
            Vec3::new(0.5, 0.5, 0.5),
        ];
        let mut kept = 0;
        let mut gone = 0;
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            for flipped in [false, true] {
                let plane = SectionPlane {
                    axis,
                    offset: 0.75,
                    flipped,
                };
                for origin in origins {
                    for dir in dirs {
                        let r = ray(origin, dir);
                        let range = plane.ray_range(&r);
                        for step in 0..60 {
                            let t = step as f32 * 0.25;
                            let point = r.origin + r.dir * t;
                            // 平面のごく近くは丸めで揺れるので見ない。
                            if (point[axis.index()] - plane.offset).abs() < 1.0e-3 {
                                continue;
                            }
                            let inside = range.is_some_and(|(t0, t1)| t >= t0 && t <= t1);
                            assert_eq!(inside, !plane.cuts(point), "{plane:?} {r:?} t={t}");
                            if inside {
                                kept += 1;
                            } else {
                                gone += 1;
                            }
                        }
                        if let Some((t0, t1)) = range {
                            assert!(t0 >= 0.0 && t0 <= t1);
                        }
                    }
                }
            }
        }
        assert!(kept > 100 && gone > 100);
    }

    #[test]
    fn ray_range_edge_cases() {
        let plane = SectionPlane {
            axis: Axis::Z,
            offset: 0.0,
            flipped: false,
        };
        // 消す側から、平面と平行に進む。
        assert_eq!(plane.ray_range(&ray(Vec3::Z, Vec3::X)), None);
        // 残す側から、平面と平行に進む。
        assert_eq!(
            plane.ray_range(&ray(-Vec3::Z, Vec3::X)),
            Some((0.0, f32::INFINITY))
        );
        // 消す側から、さらに遠ざかる。
        assert_eq!(plane.ray_range(&ray(Vec3::Z, Vec3::Z)), None);
        // 消す側から平面へ向かう。平面を越えた所から残す側。
        let (t0, t1) = plane.ray_range(&ray(Vec3::Z * 2.0, -Vec3::Z)).unwrap();
        assert!(near(t0, 2.0));
        assert_eq!(t1, f32::INFINITY);
        // 残す側から消す側へ。
        let (t0, t1) = plane.ray_range(&ray(-Vec3::Z * 3.0, Vec3::Z)).unwrap();
        assert_eq!(t0, 0.0);
        assert!(near(t1, 3.0));
        // 有限でない視線。
        assert_eq!(plane.ray_range(&ray(Vec3::splat(f32::NAN), Vec3::Z)), None);
        // 有限でない位置の平面は何も消さない。
        let nan = SectionPlane {
            offset: f32::NAN,
            ..plane
        };
        assert_eq!(
            nan.ray_range(&ray(Vec3::Z, Vec3::Z)),
            Some((0.0, f32::INFINITY))
        );
    }

    #[test]
    fn frame_corners_lie_on_the_plane_and_span_the_box() {
        let b = bounds();
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            let plane = SectionPlane::centered(axis, &b);
            let corners = plane.frame_corners(&b);
            let mut span = Aabb::empty();
            for corner in corners {
                assert!(near(corner[axis.index()], plane.offset));
                span = span.union(Aabb {
                    min: corner,
                    max: corner,
                });
            }
            for other in 0..3 {
                if other == axis.index() {
                    continue;
                }
                assert!(near(span.min[other], b.min[other]));
                assert!(near(span.max[other], b.max[other]));
            }
            // 隣り合う角は1つの成分だけが違う（長方形を順に回る）。
            for i in 0..4 {
                let (a, c) = (corners[i], corners[(i + 1) % 4]);
                let differ = (0..3).filter(|&k| a[k] != c[k]).count();
                assert_eq!(differ, 1);
            }
        }
        let empty = SectionPlane {
            axis: Axis::Y,
            offset: 2.5,
            flipped: false,
        }
        .frame_corners(&Aabb::empty());
        assert!(empty.iter().all(|c| c.is_finite() && c.y == 2.5));
    }

    #[test]
    fn drag_offset_follows_the_pointer_along_the_axis() {
        let b = bounds();
        let start = SectionPlane::centered(Axis::X, &b);
        // 横から（-Z の向きへ）見ている視線を、X 方向へ 1.5 ずらす。
        let start_ray = ray(Vec3::new(1.0, 4.0, 10.0), -Vec3::Z);
        let now_ray = ray(Vec3::new(2.5, 4.0, 10.0), -Vec3::Z);
        let offset = drag_offset(&start, &b, &start_ray, &now_ray).unwrap();
        assert!(near(offset, start.offset + 1.5), "{offset}");
        // 斜めの視線でも、軸に沿って動かした量が返る。
        let tilted = Vec3::new(0.3, -0.5, -1.0);
        let a = ray(Vec3::new(0.0, 6.0, 8.0), tilted);
        let c = ray(Vec3::new(-0.8, 6.0, 8.0), tilted);
        let offset = drag_offset(&start, &b, &a, &c).unwrap();
        assert!(near(offset, start.offset - 0.8), "{offset}");
        // 戻せば元の位置。
        let back = drag_offset(&start, &b, &a, &a).unwrap();
        assert!(near(back, start.offset));
        // 視線が軸と平行。
        let along = ray(Vec3::new(-5.0, 4.0, -2.0), Vec3::X);
        assert_eq!(drag_offset(&start, &b, &along, &start_ray), None);
        assert_eq!(drag_offset(&start, &b, &start_ray, &along), None);
        // 有限でない視線。
        let bad = ray(Vec3::splat(f32::NAN), Vec3::Z);
        assert_eq!(drag_offset(&start, &b, &bad, &start_ray), None);
    }
}
