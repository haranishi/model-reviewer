//! 単位の表示。数値は換算せず、添える文字だけを替える（画面の数値がファイルの値と食い違わない）。
//! 作業の間だけの設定で、保存しない。

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

    /// 状態欄を押したときの次（m → cm → mm → m）。
    pub fn next(self) -> Self {
        match self {
            Self::M => Self::Cm,
            Self::Cm => Self::Mm,
            Self::Mm => Self::M,
        }
    }
}

/// 有効数字の桁数。
const SIGNIFICANT: i32 = 4;
/// 小数点以下の桁数の上限（とても小さい値で桁が増え続けないように）。
const MAX_DECIMALS: i32 = 6;

/// 距離の文字（数値は変えずに単位を添える）。有効数字4桁を目安に小数点以下を決め、
/// 1000 以上は整数で出す。有限でなければ「—」。
pub fn format_distance(value: f32, unit: UnitLabel) -> String {
    if !value.is_finite() {
        return format!("— {}", unit.suffix());
    }
    let magnitude = value.abs();
    let decimals = if magnitude == 0.0 {
        3
    } else {
        (SIGNIFICANT - 1 - magnitude.log10().floor() as i32).clamp(0, MAX_DECIMALS)
    };
    format!("{value:.prec$} {}", unit.suffix(), prec = decimals as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
