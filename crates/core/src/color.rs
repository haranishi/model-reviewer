//! 色の換算。画面では sRGB の8ビット、文書とファイルではリニアの小数で持つ。
//!
//! 換算は `f64` で計算してから `f32` にする。OSごとの数学関数の差が `u8` の結果に出ないことを、
//! 256通りの往復の試験で固定する（`linear_to_srgb8(srgb8_to_linear(c)) == c`）。

/// sRGB の8ビットからリニアの値へ（0.04045 を境にした2つの式）。
pub fn srgb8_to_linear(rgb: [u8; 3]) -> [f32; 3] {
    rgb.map(|channel| {
        let c = f64::from(channel) / 255.0;
        let linear = if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        };
        linear as f32
    })
}

/// リニアの値から sRGB の8ビットへ。0〜1 に収めてから逆の式で戻し、四捨五入する。
/// 有限でない値は 0 にする。
pub fn linear_to_srgb8(rgb: [f32; 3]) -> [u8; 3] {
    rgb.map(|channel| {
        if !channel.is_finite() {
            return 0;
        }
        let l = f64::from(channel).clamp(0.0, 1.0);
        let s = if l <= 0.003_130_8 {
            l * 12.92
        } else {
            1.055 * l.powf(1.0 / 2.4) - 0.055
        };
        (s * 255.0).round().clamp(0.0, 255.0) as u8
    })
}

/// 6桁の16進。前後の空白と先頭の `#` は許す。大文字と小文字を区別しない。
pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let text = text.trim();
    let digits = text.strip_prefix('#').unwrap_or(text);
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(&digits[at..at + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

/// 大文字の6桁（`#` は付けない）。
pub fn to_hex(rgb: [u8; 3]) -> String {
    format!("{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// 色の見本（名前と sRGB の8ビット）。画面の並びは6列×2段。
pub const SWATCHES: [(&str, [u8; 3]); 12] = [
    ("白", [0xF2, 0xF2, 0xF2]),
    ("薄い灰", [0xC8, 0xC8, 0xC8]),
    ("濃い灰", [0x6E, 0x6E, 0x6E]),
    ("黒", [0x22, 0x22, 0x22]),
    ("赤", [0xD6, 0x45, 0x45]),
    ("橙", [0xE0, 0x8A, 0x2E]),
    ("黄", [0xE3, 0xC6, 0x3B]),
    ("緑", [0x4F, 0xA3, 0x5B]),
    ("青緑", [0x2F, 0xA3, 0x9A]),
    ("青", [0x3D, 0x7B, 0xD9]),
    ("紫", [0x8B, 0x5F, 0xC7]),
    ("茶", [0x8A, 0x5A, 0x3C]),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f64) -> bool {
        let diff = (f64::from(a) - b).abs();
        diff <= 1e-6 || diff <= b.abs() * 1e-5
    }

    #[test]
    fn srgb_round_trips_all_256() {
        for value in 0..=255u8 {
            let rgb = [value, 255 - value, value / 2];
            assert_eq!(linear_to_srgb8(srgb8_to_linear(rgb)), rgb, "値 {value}");
        }
    }

    #[test]
    fn linear_values_follow_the_srgb_formula() {
        let [black, mid, white] = srgb8_to_linear([0, 128, 255]);
        assert_eq!(black.to_bits(), 0.0f32.to_bits());
        assert!(close(white, 1.0));
        // 128/255 = 0.50196… → ((c + 0.055) / 1.055)^2.4 ≈ 0.21586
        assert!(close(mid, 0.215_860_5), "{mid}");
        // 境目より下は直線の式。
        let [low, _, _] = srgb8_to_linear([10, 0, 0]);
        assert!(close(low, 10.0 / 255.0 / 12.92), "{low}");
    }

    #[test]
    fn out_of_range_and_non_finite_do_not_panic() {
        assert_eq!(
            linear_to_srgb8([f32::NAN, f32::INFINITY, f32::NEG_INFINITY]),
            [0, 0, 0]
        );
        assert_eq!(linear_to_srgb8([-1.0, 2.0, 1.0]), [0, 255, 255]);
    }

    #[test]
    fn hex_parse_table() {
        let accepted = [
            ("D64545", [0xD6, 0x45, 0x45]),
            ("#d64545", [0xD6, 0x45, 0x45]),
            ("  #3d7BD9 \n", [0x3D, 0x7B, 0xD9]),
            ("000000", [0, 0, 0]),
            ("ffffff", [255, 255, 255]),
        ];
        for (text, rgb) in accepted {
            assert_eq!(parse_hex(text), Some(rgb), "{text:?}");
        }
        let refused = [
            "",
            "#",
            "D6454",
            "D645451",
            "#D6454",
            "GG0000",
            "12 456",
            "##D64545",
            "+12345",
            "１２３４５６",
        ];
        for text in refused {
            assert_eq!(parse_hex(text), None, "{text:?}");
        }
    }

    #[test]
    fn hex_is_six_upper_case_digits() {
        assert_eq!(to_hex([0x3D, 0x7B, 0xD9]), "3D7BD9");
        assert_eq!(to_hex([0, 1, 255]), "0001FF");
        for (_, rgb) in SWATCHES {
            assert_eq!(parse_hex(&to_hex(rgb)), Some(rgb));
        }
    }

    #[test]
    fn swatches_are_distinct() {
        for (i, (name_a, rgb_a)) in SWATCHES.iter().enumerate() {
            for (name_b, rgb_b) in &SWATCHES[i + 1..] {
                assert_ne!(rgb_a, rgb_b, "{name_a} と {name_b}");
                assert_ne!(name_a, name_b);
            }
        }
    }
}
