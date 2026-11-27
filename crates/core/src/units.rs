//! 単位の表示。シーンの数値は変えない。「ファイルの単位（前提）」と「表示の単位」を分け、
//! 換算は画面に出すときと画面から受け取るときだけ行う。表示の既定は「ファイルと同じ」で、
//! そのときは数値を1ビットも変えず、添える文字だけを替える。作業の間だけの設定で、保存しない。

use glam::{Mat3, Vec3};

/// STL で上と仮定する軸。画面では常に Y が上を向く。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpAxis {
    #[default]
    Z,
    Y,
}

impl UpAxis {
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Z => "Z",
            Self::Y => "Y",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Z => "z",
            Self::Y => "y",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "z" => Some(Self::Z),
            "y" => Some(Self::Y),
            _ => None,
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Z => Self::Y,
            Self::Y => Self::Z,
        }
    }

    pub fn rotation(self) -> Mat3 {
        match self {
            Self::Z => Mat3::from_cols(Vec3::X, Vec3::NEG_Z, Vec3::Y),
            Self::Y => Mat3::IDENTITY,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UnitLabel {
    #[default]
    M,
    Cm,
    Mm,
}

impl UnitLabel {
    /// 添える文字（"m"・"cm"・"mm"）。
    pub fn suffix(self) -> &'static str {
        match self {
            Self::M => "m",
            Self::Cm => "cm",
            Self::Mm => "mm",
        }
    }

    /// 1単位が何メートルか（1.0・0.01・0.001）。
    pub fn meters(self) -> f64 {
        match self {
            Self::M => 1.0,
            Self::Cm => 0.01,
            Self::Mm => 0.001,
        }
    }

    /// 状態欄を押したときの次（m → cm → mm → m）。
    pub fn next(self) -> Self {
        match self {
            Self::M => Self::Cm,
            Self::Cm => Self::Mm,
            Self::Mm => Self::M,
        }
    }
}

/// ファイルの単位（前提）と表示の単位。`display = None` は「ファイルと同じ」。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Units {
    pub source: UnitLabel,
    pub display: Option<UnitLabel>,
}

impl Units {
    /// 画面に出す単位（`display.unwrap_or(source)`）。
    pub fn shown(self) -> UnitLabel {
        self.display.unwrap_or(self.source)
    }

    /// シーンの値を表示の単位へ。`source` と `shown` が同じなら、値をそのまま返す。
    pub fn to_display(self, scene: f32) -> f32 {
        let shown = self.shown();
        if shown == self.source {
            return scene;
        }
        let ratio = self.source.meters() / shown.meters();
        (f64::from(scene) * ratio) as f32
    }

    /// 表示の単位の値をシーンの値へ。`source` と `shown` が同じなら、値をそのまま返す。
    pub fn from_display(self, shown_value: f32) -> f32 {
        let shown = self.shown();
        if shown == self.source {
            return shown_value;
        }
        let ratio = shown.meters() / self.source.meters();
        (f64::from(shown_value) * ratio) as f32
    }

    /// 換算して単位の文字を添える。有限でなければ「—」。
    pub fn format(self, scene: f32, digits: u8) -> String {
        format_length(self.to_display(scene), self.shown(), digits)
    }

    /// 表示の単位を押したときの次（ファイルと同じ → mm → cm → m → ファイルと同じ）。
    pub fn next_display(self) -> Option<UnitLabel> {
        match self.display {
            None => Some(UnitLabel::Mm),
            Some(UnitLabel::Mm) => Some(UnitLabel::Cm),
            Some(UnitLabel::Cm) => Some(UnitLabel::M),
            Some(UnitLabel::M) => None,
        }
    }
}

/// `format_distance` の有効数字の桁数。
const SIGNIFICANT: u8 = 4;
/// 小数点以下の桁数の上限（とても小さい値で桁が増え続けないように）。
const MAX_DECIMALS: i32 = 6;

/// 距離の文字（数値は変えずに単位を添える）。有効数字4桁を目安に小数点以下を決め、
/// 1000 以上は整数で出す。有限でなければ「—」。
pub fn format_distance(value: f32, unit: UnitLabel) -> String {
    format_length(value, unit, SIGNIFICANT)
}

/// 長さの文字（数値は変えずに単位を添える）。有効数字 `digits` 桁（1 未満は 1）を目安に
/// 小数点以下を決め、桁が整数部に収まらなければ整数で出す。0 は小数点以下 `digits − 1` 桁。
/// 有限でなければ「—」。`digits = 4` は `format_distance` と同じ文字になる。
pub fn format_length(value: f32, unit: UnitLabel, digits: u8) -> String {
    if !value.is_finite() {
        return format!("— {}", unit.suffix());
    }
    let significant = i32::from(digits.max(1));
    let magnitude = value.abs();
    let decimals = if magnitude == 0.0 {
        (significant - 1).clamp(0, MAX_DECIMALS)
    } else {
        (significant - 1 - magnitude.log10().floor() as i32).clamp(0, MAX_DECIMALS)
    };
    format!("{value:.prec$} {}", unit.suffix(), prec = decimals as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn up_axis_keys_and_other() {
        assert_eq!(UpAxis::default(), UpAxis::Z);
        for (up, key, suffix, other) in [
            (UpAxis::Z, "z", "Z", UpAxis::Y),
            (UpAxis::Y, "y", "Y", UpAxis::Z),
        ] {
            assert_eq!(up.key(), key);
            assert_eq!(UpAxis::from_key(key), Some(up));
            assert_eq!(up.suffix(), suffix);
            assert_eq!(up.other(), other);
        }
    }

    #[test]
    fn labels_cycle_and_numbers_stay_the_same() {
        assert_eq!(UnitLabel::default(), UnitLabel::M);
        assert_eq!(UnitLabel::M.next(), UnitLabel::Cm);
        assert_eq!(UnitLabel::Cm.next(), UnitLabel::Mm);
        assert_eq!(UnitLabel::Mm.next(), UnitLabel::M);
        assert_eq!(
            [UnitLabel::M, UnitLabel::Cm, UnitLabel::Mm].map(UnitLabel::suffix),
            ["m", "cm", "mm"]
        );
        assert_eq!(format_distance(1.0, UnitLabel::M), "1.000 m");
        assert_eq!(format_distance(1.0, UnitLabel::Mm), "1.000 mm");
        assert_eq!(format_distance(0.0, UnitLabel::Cm), "0.000 cm");
        assert_eq!(format_distance(12.3456, UnitLabel::M), "12.35 m");
        assert_eq!(format_distance(0.012345, UnitLabel::M), "0.01235 m");
        assert_eq!(format_distance(98765.4, UnitLabel::M), "98765 m");
        assert_eq!(format_distance(1.0e-9, UnitLabel::M), "0.000000 m");
        assert_eq!(format_distance(f32::NAN, UnitLabel::Cm), "— cm");
    }

    fn near(actual: f32, expected: f32) -> bool {
        (actual - expected).abs() <= 1.0e-6 + expected.abs() * 1.0e-5
    }

    #[test]
    fn meters_per_unit() {
        assert_eq!(UnitLabel::M.meters(), 1.0);
        assert_eq!(UnitLabel::Cm.meters(), 0.01);
        assert_eq!(UnitLabel::Mm.meters(), 0.001);
    }

    #[test]
    fn same_unit_keeps_every_bit() {
        for source in [UnitLabel::M, UnitLabel::Cm, UnitLabel::Mm] {
            for units in [
                Units {
                    source,
                    display: None,
                },
                Units {
                    source,
                    display: Some(source),
                },
            ] {
                assert_eq!(units.shown(), source);
                for value in [0.1_f32, 1.0 / 3.0, -7.25e-3, 12345.678, f32::NAN] {
                    assert_eq!(units.to_display(value).to_bits(), value.to_bits());
                    assert_eq!(units.from_display(value).to_bits(), value.to_bits());
                }
            }
        }
    }

    #[test]
    fn converts_between_units_and_back() {
        let mm = Units {
            source: UnitLabel::M,
            display: Some(UnitLabel::Mm),
        };
        assert_eq!(mm.shown(), UnitLabel::Mm);
        assert!(near(mm.to_display(0.0254), 25.4));
        assert!(near(mm.from_display(25.4), 0.0254));
        let cm_from_mm = Units {
            source: UnitLabel::Mm,
            display: Some(UnitLabel::Cm),
        };
        assert!(near(cm_from_mm.to_display(25.0), 2.5));
        for value in [0.0254_f32, 1.5, 1234.5, -0.003] {
            assert!(near(mm.from_display(mm.to_display(value)), value));
            assert!(near(
                cm_from_mm.from_display(cm_from_mm.to_display(value)),
                value
            ));
        }
        assert_eq!(mm.format(0.5, 4), "500.0 mm");
        assert_eq!(mm.format(f32::INFINITY, 4), "— mm");
        assert_eq!(Units::default().format(2.5, 4), "2.500 m");
    }

    #[test]
    fn display_cycles_through_file_mm_cm_m() {
        let mut units = Units::default();
        let mut seen = Vec::new();
        for _ in 0..4 {
            units.display = units.next_display();
            seen.push(units.display);
        }
        assert_eq!(
            seen,
            [
                Some(UnitLabel::Mm),
                Some(UnitLabel::Cm),
                Some(UnitLabel::M),
                None
            ]
        );
    }

    #[test]
    fn format_length_with_four_digits_matches_format_distance() {
        for value in [
            0.0_f32,
            0.5,
            2.5,
            1024.0,
            12.3456,
            0.012345,
            98765.4,
            1.0e-9,
            f32::NAN,
        ] {
            for unit in [UnitLabel::M, UnitLabel::Cm, UnitLabel::Mm] {
                assert_eq!(format_length(value, unit, 4), format_distance(value, unit));
            }
        }
        assert_eq!(format_length(2.5, UnitLabel::Mm, 6), "2.50000 mm");
        assert_eq!(format_length(1024.0, UnitLabel::M, 6), "1024.00 m");
        assert_eq!(format_length(0.0, UnitLabel::M, 6), "0.00000 m");
    }
}
