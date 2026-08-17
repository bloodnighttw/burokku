
pub mod common;
pub mod length;
pub mod flex;
pub mod grid;



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
            3 => Some(Self::rgb(
                expand_hex(hex.as_bytes()[0])?,
                expand_hex(hex.as_bytes()[1])?,
                expand_hex(hex.as_bytes()[2])?,
            )),
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




