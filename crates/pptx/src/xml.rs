//! Shared XML primitives for every OOXML part writer: escaping, unit conversion and DrawingML
//! color elements. Nothing here knows about slides.

use toolkit::Color;

pub const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

pub const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
pub const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub const NS_P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
pub const NS_RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub const NS_CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";

/// The namespace declarations on the root of every PresentationML part.
pub const NS_PML: &str = concat!(
    "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" ",
    "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" ",
    "xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\""
);

/// Escapes text for an attribute or element. XML 1.0 allows TAB, LF, CR, U+0020-U+D7FF,
/// U+E000-U+FFFD and U+10000-U+10FFFF; anything else is dropped, because one stray control
/// character is enough to make PowerPoint reject the whole part. (The JavaScript also strips
/// lone surrogates, which a Rust string can't hold.)
pub fn esc(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\t' | '\n' | '\r' => out.push(c),
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}' => out.push(c),
            _ => {}
        }
    }
    out
}

pub const EMU_PER_INCH: f64 = 914_400.0;

/// One slide unit (the engine lays slides out on 1920 x 1080) is 1/144 inch.
pub const EMU_PER_UNIT: f64 = 6350.0;

/// JavaScript's `Math.round`: halves go up, also below zero.
fn round_half_up(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

fn to_emu(value: f64) -> i64 {
    if value.is_finite() { round_half_up(value) as i64 } else { 0 }
}

/// Slide units to EMU. Not finite gives 0.
pub fn emu_from_units(units: f32) -> i64 {
    to_emu(f64::from(units) * EMU_PER_UNIT)
}

/// Inches to EMU. Not finite gives 0.
pub fn emu(inches: f64) -> i64 {
    to_emu(inches * EMU_PER_INCH)
}

/// Extents (`a:ext` cx/cy) must not be negative: PowerPoint repairs the file if they are.
pub fn emu_size(inches: f64) -> i64 {
    emu(inches).max(0)
}

/// The theme slots a color can point at. `dk1`/`lt1`/`dk2`/`lt2` are referenced from slide content
/// through the master's color map (`tx1`/`bg1`/`tx2`/`bg2`): that's what PowerPoint itself
/// writes, and what Google Slides and Keynote expect. Accents are referenced directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Dk1,
    Lt1,
    Dk2,
    Lt2,
    Accent1,
    Accent2,
    Accent3,
    Accent4,
    Accent5,
    Accent6,
}

impl Slot {
    fn scheme_value(self) -> &'static str {
        match self {
            Slot::Dk1 => "tx1",
            Slot::Lt1 => "bg1",
            Slot::Dk2 => "tx2",
            Slot::Lt2 => "bg2",
            Slot::Accent1 => "accent1",
            Slot::Accent2 => "accent2",
            Slot::Accent3 => "accent3",
            Slot::Accent4 => "accent4",
            Slot::Accent5 => "accent5",
            Slot::Accent6 => "accent6",
        }
    }
}

/// A fixed color, or a theme slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorRef {
    Rgb(Color),
    Slot(Slot),
}

/// `RRGGBB`, upper case.
pub fn hex(color: Color) -> String {
    format!("{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

/// A DrawingML color element. `alpha` below 1 adds an `a:alpha` child.
pub fn color_xml(color: ColorRef, alpha: Option<f64>) -> String {
    let modifier = match alpha {
        Some(a) if a < 1.0 => format!("<a:alpha val=\"{}\"/>", round_half_up(a * 100_000.0) as i64),
        _ => String::new(),
    };
    let (tag, val) = match color {
        ColorRef::Rgb(c) => ("srgbClr", hex(c)),
        ColorRef::Slot(slot) => ("schemeClr", slot.scheme_value().to_string()),
    };
    if modifier.is_empty() {
        format!("<a:{tag} val=\"{val}\"/>")
    } else {
        format!("<a:{tag} val=\"{val}\">{modifier}</a:{tag}>")
    }
}

pub fn solid_fill(color: ColorRef, alpha: Option<f64>) -> String {
    format!("<a:solidFill>{}</a:solidFill>", color_xml(color, alpha))
}
