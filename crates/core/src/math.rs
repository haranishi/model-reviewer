//! 境界ボックスと光線。カメラの計算は `camera` にある。

use glam::{Affine3A, Vec3};

/// 軸平行境界ボックス。空はいずれかの軸で `min > max`。
/// 正規の空は `min = +∞`、`max = −∞` で、`union` の単位元になる。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

/// 光線。`dir` は呼び出し側が正規化済みで、この型は正規化し直さない。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
}

impl Aabb {
    pub fn empty() -> Self {
        Self {
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
        }
    }

    pub fn is_empty(self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    pub fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    pub fn transformed(self, world: &Affine3A) -> Self {
        if self.is_empty() {
            return self;
        }
        let mut out = Self::empty();
        for corner in self.corners() {
            let point = world.transform_point3(corner);
            out.min = out.min.min(point);
            out.max = out.max.max(point);
        }
        out
    }

    pub fn center(self) -> Vec3 {
        if self.is_empty() {
            Vec3::ZERO
        } else {
            (self.min + self.max) * 0.5
        }
    }

    /// 外接球の半径（対角線の半分）。空は 0。
    pub fn radius(self) -> f32 {
        if self.is_empty() {
            0.0
        } else {
            (self.max - self.min).length() * 0.5
        }
    }

    fn corners(self) -> [Vec3; 8] {
        let (min, max) = (self.min, self.max);
        [
            Vec3::new(min.x, min.y, min.z),
            Vec3::new(max.x, min.y, min.z),
            Vec3::new(min.x, max.y, min.z),
            Vec3::new(max.x, max.y, min.z),
            Vec3::new(min.x, min.y, max.z),
            Vec3::new(max.x, min.y, max.z),
            Vec3::new(min.x, max.y, max.z),
            Vec3::new(max.x, max.y, max.z),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::{Aabb, Ray};
    use glam::{Affine3A, Vec3};

    fn unit() -> Aabb {
        Aabb {
            min: Vec3::ZERO,
            max: Vec3::ONE,
        }
    }

    fn near(actual: Vec3, expected: Vec3) {
        for (got, want) in actual.to_array().into_iter().zip(expected.to_array()) {
            assert!(
                (got - want).abs() < 1e-5,
                "got {actual:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn empty_is_the_identity_for_union_and_stays_empty_when_transformed() {
        let box_ = unit();
        let empty = Aabb::empty();
        assert!(empty.is_empty());
        assert_eq!(empty.union(box_), box_);
        assert_eq!(box_.union(empty), box_);
        assert_eq!(empty.center(), Vec3::ZERO);
        assert_eq!(empty.radius(), 0.0);

        let moved = empty.transformed(&Affine3A::from_translation(Vec3::X));
        assert!(moved.is_empty());
        assert_eq!(moved.center(), Vec3::ZERO);
        assert_eq!(moved.radius(), 0.0);
    }

    #[test]
    fn non_canonical_empty_does_not_mix_into_the_other_box() {
        let broken = Aabb {
            min: Vec3::new(1.0, 0.0, 0.0),
            max: Vec3::new(-1.0, 1.0, 1.0),
        };
        let box_ = unit();
        assert!(broken.is_empty());
        assert_eq!(broken.union(box_), box_);
        assert_eq!(box_.union(broken), box_);
        let moved = broken.transformed(&Affine3A::from_translation(Vec3::X));
        assert!(moved.is_empty());
        assert_eq!(moved.min, broken.min);
        assert_eq!(moved.max, broken.max);
    }

    #[test]
    fn translation_scale_and_mirror_of_the_unit_cube() {
        let cube = unit();
        let translated = cube.transformed(&Affine3A::from_translation(Vec3::new(2.0, 3.0, 4.0)));
        assert_eq!(translated.min, Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(translated.max, Vec3::new(3.0, 4.0, 5.0));

        let scaled = cube.transformed(&Affine3A::from_scale(Vec3::splat(2.0)));
        assert_eq!(scaled.min, Vec3::ZERO);
        assert_eq!(scaled.max, Vec3::splat(2.0));

        let mirrored = cube.transformed(&Affine3A::from_scale(Vec3::new(-1.0, 1.0, 1.0)));
        assert_eq!(mirrored.min, Vec3::new(-1.0, 0.0, 0.0));
        assert_eq!(mirrored.max, Vec3::new(0.0, 1.0, 1.0));
    }

    #[test]
    fn yaw_90_of_the_unit_cube() {
        let turned = unit().transformed(&Affine3A::from_rotation_y(std::f32::consts::FRAC_PI_2));
        near(turned.min, Vec3::new(0.0, 0.0, -1.0));
        near(turned.max, Vec3::new(1.0, 1.0, 0.0));
    }

    #[test]
    fn center_and_radius_of_a_cube() {
        let cube = Aabb {
            min: Vec3::ZERO,
            max: Vec3::splat(2.0),
        };
        near(cube.center(), Vec3::ONE);
        assert!((cube.radius() - 3.0_f32.sqrt()).abs() < 1e-5);
    }

    #[test]
    fn ray_keeps_the_given_direction() {
        let ray = Ray {
            origin: Vec3::ZERO,
            dir: Vec3::new(2.0, 0.0, 0.0),
        };
        assert_eq!(ray.dir, Vec3::new(2.0, 0.0, 0.0));
    }
}
