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
        // 契約の先置き。中身は後の作業で書く。
        let _ = ray;
        None
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
        // 契約の先置き。中身は後の作業で書く。
        let _ = bounds;
        [Vec3::ZERO; 4]
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
    // 契約の先置き。中身は後の作業で書く。
    let _ = (start, bounds, start_ray, now_ray);
    None
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
}
