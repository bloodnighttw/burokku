/// A strongly typed color stored in authoritative element data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RgbaColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl RgbaColor {
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: u8::MAX,
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        let hex = value.strip_prefix('#')?;
        if !hex.as_bytes().iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        match hex.len() {
            3 | 4 => Some(Self {
                red: expand_hex(hex.as_bytes()[0])?,
                green: expand_hex(hex.as_bytes()[1])?,
                blue: expand_hex(hex.as_bytes()[2])?,
                alpha: if hex.len() == 4 {
                    expand_hex(hex.as_bytes()[3])?
                } else {
                    u8::MAX
                },
            }),
            6 | 8 => Some(Self {
                red: parse_hex_byte(&hex[0..2])?,
                green: parse_hex_byte(&hex[2..4])?,
                blue: parse_hex_byte(&hex[4..6])?,
                alpha: if hex.len() == 8 {
                    parse_hex_byte(&hex[6..8])?
                } else {
                    u8::MAX
                },
            }),
            _ => None,
        }
    }
}

fn expand_hex(value: u8) -> Option<u8> {
    let digit = (value as char).to_digit(16)? as u8;
    Some((digit << 4) | digit)
}

fn parse_hex_byte(value: &str) -> Option<u8> {
    u8::from_str_radix(value, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_css_hex_forms() {
        assert_eq!(
            RgbaColor::parse("#123"),
            Some(RgbaColor::rgb(0x11, 0x22, 0x33))
        );
        assert_eq!(
            RgbaColor::parse("#1234"),
            Some(RgbaColor {
                red: 0x11,
                green: 0x22,
                blue: 0x33,
                alpha: 0x44,
            })
        );
        assert_eq!(RgbaColor::parse("#010203"), Some(RgbaColor::rgb(1, 2, 3)));
        assert_eq!(RgbaColor::parse("red"), None);
    }
}
