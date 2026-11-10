//! theme1.xml from the deck's theme slots and fonts. The format scheme is the minimal valid one
//! (each list needs exactly three entries); shapes here set their own fills/lines, so it only
//! matters for shapes users insert later in PowerPoint.

use toolkit::Theme;

use crate::xml::{NS_A, XML_DECL, esc, hex};

// The deck's text, background and accent colors fill `dk1`, `lt1` and `accent1`. The rest are the
// exporter's defaults (`default-config.mjs`): a dark blue, a light panel gray, a violet, a muted
// gray and a border gray; the config has no fifth or sixth accent, so a neutral gray and amber
// keep charts with extra series readable. Links use the accent, followed links the dark blue.
const DK2: &str = "1E3A8A";
const LT2: &str = "F1F5F9";
const ACCENT2: &str = "7C3AED";
const ACCENT3: &str = "6B7280";
const ACCENT4: &str = "E2E8F0";
const ACCENT5: &str = "9CA3AF";
const ACCENT6: &str = "F59E0B";
const FOL_HLINK: &str = "1E3A8A";

fn line(w: u32) -> String {
    format!(
        "<a:ln w=\"{w}\" cap=\"flat\" cmpd=\"sng\" algn=\"ctr\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:prstDash val=\"solid\"/><a:miter lim=\"800000\"/></a:ln>"
    )
}

fn fmt_scheme() -> String {
    let mut s = String::from("<a:fmtScheme name=\"Deck\"><a:fillStyleLst>");
    s.push_str("<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>");
    s.push_str("<a:solidFill><a:schemeClr val=\"phClr\"><a:tint val=\"50000\"/></a:schemeClr></a:solidFill>");
    s.push_str("<a:solidFill><a:schemeClr val=\"phClr\"><a:shade val=\"80000\"/></a:schemeClr></a:solidFill>");
    s.push_str("</a:fillStyleLst>");
    s.push_str(&format!("<a:lnStyleLst>{}{}{}</a:lnStyleLst>", line(6350), line(12700), line(19050)));
    s.push_str("<a:effectStyleLst>");
    s.push_str(&"<a:effectStyle><a:effectLst/></a:effectStyle>".repeat(3));
    s.push_str("</a:effectStyleLst><a:bgFillStyleLst>");
    s.push_str("<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>");
    s.push_str("<a:solidFill><a:schemeClr val=\"phClr\"><a:tint val=\"95000\"/></a:schemeClr></a:solidFill>");
    s.push_str("<a:solidFill><a:schemeClr val=\"phClr\"><a:shade val=\"90000\"/></a:schemeClr></a:solidFill>");
    s.push_str("</a:bgFillStyleLst></a:fmtScheme>");
    s
}

fn slots(theme: &Theme) -> [(&'static str, String); 12] {
    let hex_of = hex;
    [
        ("dk1", hex_of(theme.text)),
        ("lt1", hex_of(theme.background)),
        ("dk2", DK2.into()),
        ("lt2", LT2.into()),
        ("accent1", hex_of(theme.accent)),
        ("accent2", ACCENT2.into()),
        ("accent3", ACCENT3.into()),
        ("accent4", ACCENT4.into()),
        ("accent5", ACCENT5.into()),
        ("accent6", ACCENT6.into()),
        ("hlink", hex_of(theme.accent)),
        ("folHlink", FOL_HLINK.into()),
    ]
}

/// The theme part. Heading and body text both use the deck's font.
pub fn theme_xml(name: &str, theme: &Theme) -> String {
    let colors: String =
        slots(theme).iter().map(|(slot, value)| format!("<a:{slot}><a:srgbClr val=\"{value}\"/></a:{slot}>")).collect();
    let font = |typeface: &str| format!("<a:latin typeface=\"{}\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/>", esc(typeface));
    let name = esc(name);
    format!(
        "{XML_DECL}<a:theme xmlns:a=\"{NS_A}\" name=\"{name}\"><a:themeElements>\
         <a:clrScheme name=\"{name}\">{colors}</a:clrScheme>\
         <a:fontScheme name=\"{name}\"><a:majorFont>{major}</a:majorFont><a:minorFont>{minor}</a:minorFont></a:fontScheme>\
         {fmt}</a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>",
        major = font(&theme.font),
        minor = font(&theme.font),
        fmt = fmt_scheme(),
    )
}
