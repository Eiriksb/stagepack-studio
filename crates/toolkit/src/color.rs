use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }

    /// Reads `#rgb`, `#rrggbb` or one of a few colour names.
    pub fn parse(text: &str) -> Option<Color> {
        let text = text.trim();
        if let Some(hex) = text.strip_prefix('#') {
            if !hex.is_ascii() {
                return None;
            }
            let digit = |s: &str| u8::from_str_radix(s, 16).ok();
            return match hex.len() {
                3 => {
                    let d = |i: usize| digit(&hex[i..=i]).map(|v| v * 17);
                    Some(Color::rgb(d(0)?, d(1)?, d(2)?))
                }
                6 => Some(Color::rgb(digit(&hex[0..2])?, digit(&hex[2..4])?, digit(&hex[4..6])?)),
                _ => None,
            };
        }
        Some(match text {
            "black" => Color::rgb(0, 0, 0),
            "white" => Color::rgb(255, 255, 255),
            "red" => Color::rgb(255, 0, 0),
            "green" => Color::rgb(0, 128, 0),
            "blue" => Color::rgb(0, 0, 255),
            "yellow" => Color::rgb(255, 255, 0),
            "orange" => Color::rgb(255, 165, 0),
            "purple" => Color::rgb(128, 0, 128),
            "gray" => Color::rgb(128, 128, 128),
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub background: Color,
    pub text: Color,
    pub accent: Color,
    pub font: String,
    pub mono_font: String,
}

impl Default for Theme {
    fn default() -> Theme {
        Theme {
            background: Color::rgb(0xff, 0xff, 0xff),
            text: Color::rgb(0x1e, 0x29, 0x3b),
            accent: Color::rgb(0x25, 0x63, 0xeb),
            font: "Inter".into(),
            mono_font: "JetBrains Mono".into(),
        }
    }
}
