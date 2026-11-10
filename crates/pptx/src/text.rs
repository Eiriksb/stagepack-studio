//! DrawingML text bodies. Every paragraph gets at most one `a:pPr`, always first, and every
//! pPr/rPr child is written in schema order. (pptxgenjs writes a pPr per *run*, which is what
//! dropped bullets in LibreOffice and is a likely PowerPoint Online repair trigger.)
//!
//! Sizes in the native types are slide units: 1/144 inch, so half a point.

use slides_engine::{Align, Bullet, NativeParagraph, NativeRun};
use toolkit::Color;

use crate::xml::{color_xml_of, emu, emu_from_units, esc, round_half_up, solid_fill_of};

/// What text writing needs from the part it is written into.
pub struct TextCtx {
    /// The language every run is tagged with.
    pub lang: String,
    /// The relationship id for a link's target. It is asked for each piece of linked text, so
    /// one run that a line break cuts in two asks twice.
    pub link: Box<dyn FnMut(&str) -> String>,
}

/// The vertical anchor of a text body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Top,
    Center,
    Bottom,
}

impl Anchor {
    fn value(self) -> &'static str {
        match self {
            Anchor::Top => "t",
            Anchor::Center => "ctr",
            Anchor::Bottom => "b",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Autofit {
    /// Say nothing, so a placeholder keeps its layout's.
    #[default]
    Inherit,
    /// Shrink the text to fit (`a:normAutofit`).
    Shrink,
    /// Grow the shape to fit (`a:spAutoFit`).
    Grow,
    /// Neither (`a:noAutofit`).
    Off,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BodyOptions {
    /// A placeholder inherits its wrapping and its paragraphs' bullets from its layout.
    pub placeholder: bool,
    /// Leave lines unwrapped.
    pub no_wrap: bool,
    /// The space kept clear on every side, in slide units.
    pub inset: Option<f32>,
    pub anchor: Option<Anchor>,
    pub autofit: Autofit,
    /// With `Inherit`, a scale below 1 asks for `a:normAutofit` at that font scale.
    pub font_scale: Option<f32>,
}

/// What an `a:rPr` is made of. The end of a paragraph keeps only the size and the font.
struct Look<'a> {
    size: f32,
    bold: bool,
    italic: bool,
    underline: bool,
    color: Option<Color>,
    family: &'a str,
    link: Option<String>,
}

/// `sz` in hundredths of a point, held to what PowerPoint accepts. Not finite or zero gives none.
fn size_attr(size: f32) -> Option<i64> {
    if !size.is_finite() || size == 0.0 {
        return None;
    }
    Some(round_half_up(f64::from(size) * 50.0).clamp(100.0, 400_000.0) as i64)
}

fn rpr(look: &Look, ctx: &mut TextCtx, tag: &str) -> String {
    let mut attrs = format!(" lang=\"{}\"", esc(&ctx.lang));
    if let Some(sz) = size_attr(look.size) {
        attrs.push_str(&format!(" sz=\"{sz}\""));
    }
    if look.bold {
        attrs.push_str(" b=\"1\"");
    }
    if look.italic {
        attrs.push_str(" i=\"1\"");
    }
    if look.underline {
        attrs.push_str(" u=\"sng\"");
    }
    attrs.push_str(" dirty=\"0\"");
    // Child order per CT_TextCharacterProperties: fill, ..., latin, ea, cs, sym, hlinkClick.
    let mut kids = String::new();
    if let Some(color) = look.color {
        kids.push_str(&solid_fill_of(color));
    }
    if !look.family.is_empty() {
        let family = esc(look.family);
        kids.push_str(&format!("<a:latin typeface=\"{family}\"/><a:cs typeface=\"{family}\"/>"));
    }
    if let Some(url) = &look.link {
        kids.push_str(&format!("<a:hlinkClick r:id=\"{}\"/>", esc(&(ctx.link)(url))));
    }
    if kids.is_empty() { format!("<{tag}{attrs}/>") } else { format!("<{tag}{attrs}>{kids}</{tag}>") }
}

/// A talk's link as it is written, in the form the URL parser gives it: PowerPoint can complain
/// about a target that isn't a well-formed address. Only web and mail addresses have one, so a
/// talk can't put a file, a network share or a script behind a click in somebody's PowerPoint.
pub fn link_target(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url.trim()).ok()?;
    ["http", "https", "mailto"].contains(&parsed.scheme()).then(|| parsed.to_string())
}

fn run_look(run: &NativeRun, with_link: bool) -> Look<'_> {
    Look {
        size: run.size,
        bold: run.bold,
        italic: run.italic,
        underline: run.underline,
        color: Some(run.color),
        family: &run.family,
        link: run.link.as_deref().filter(|_| with_link).and_then(link_target),
    }
}

/// A run's properties as `tag` (`a:rPr`, or `a:endParaRPr`).
pub fn rpr_xml(run: &NativeRun, ctx: &mut TextCtx, tag: &str) -> String {
    rpr(&run_look(run, true), ctx, tag)
}

/// A text with a `\n` is cut into lines by `a:br`, which carries the run's properties so that the
/// line it ends is as tall as the text.
fn runs_xml(runs: &[NativeRun], ctx: &mut TextCtx) -> String {
    let mut out = String::new();
    for run in runs {
        for (i, piece) in run.text.split('\n').enumerate() {
            if i > 0 {
                out.push_str(&format!("<a:br>{}</a:br>", rpr(&run_look(run, false), ctx, "a:rPr")));
            }
            if !piece.is_empty() {
                out.push_str(&format!("<a:r>{}<a:t>{}</a:t></a:r>", rpr_xml(run, ctx, "a:rPr"), esc(piece)));
            }
        }
    }
    out
}

/// The most `a:spcPts` takes, in hundredths of a point.
const MOST_SPACING: f64 = 158_400.0;

/// A distance in slide units as `tag`, in hundredths of a point.
fn spacing_xml(tag: &str, units: f32) -> String {
    let val = if units.is_finite() { round_half_up(f64::from(units) * 50.0).clamp(0.0, MOST_SPACING) as i64 } else { 0 };
    format!("<{tag}><a:spcPts val=\"{val}\"/></{tag}>")
}

fn align_value(align: Align) -> &'static str {
    match align {
        Align::Left => "l",
        Align::Center => "ctr",
        Align::Right => "r",
    }
}

/// A paragraph's properties, or nothing. `previous` is the paragraph before it, which decides
/// whether a number has to say where it starts.
pub fn ppr_xml(para: &NativeParagraph, previous: Option<&NativeParagraph>, placeholder: bool) -> String {
    let level = para.level.min(8);
    let mut attrs: Vec<String> = Vec::new();
    let mut kids = String::new();
    let bulleted = matches!(para.bullet, Bullet::Dot | Bullet::Number(_));

    if para.bullet == Bullet::None && placeholder {
        // Body placeholders inherit bullets from the master's bodyStyle, so a plain paragraph has
        // to switch them off and drop the hanging indent explicitly.
        attrs.push("marL=\"0\"".into());
        attrs.push("indent=\"0\"".into());
    } else if !placeholder && bulleted {
        // Free text boxes inherit no list style, so they carry their own indent.
        attrs.push(format!("marL=\"{}\"", emu(0.25 * (f64::from(level) + 1.0))));
        attrs.push(format!("indent=\"{}\"", emu(-0.2)));
    }
    if level > 0 {
        attrs.push(format!("lvl=\"{level}\""));
    }
    // A placeholder inherits its layout's alignment (Title Slide centres), so it always says its own.
    if para.align != Align::Left || placeholder {
        attrs.push(format!("algn=\"{}\"", align_value(para.align)));
    }

    // Child order per CT_TextParagraphProperties: lnSpc, spcBef, spcAft, buClr, buSz, buFont, bullet.
    if let Some(line_height) = para.line_height {
        kids.push_str(&spacing_xml("a:lnSpc", line_height));
    }
    if let Some(space_before) = para.space_before {
        kids.push_str(&spacing_xml("a:spcBef", space_before));
    }
    let marker_color = para.bullet_color.map(|color| format!("<a:buClr>{}</a:buClr>", color_xml_of(color)));
    match para.bullet {
        Bullet::Dot if !placeholder => {
            kids.push_str(marker_color.as_deref().unwrap_or("<a:buClr><a:schemeClr val=\"accent1\"/></a:buClr>"));
            kids.push_str("<a:buFont typeface=\"Arial\"/><a:buChar char=\"•\"/>");
        }
        Bullet::Number(n) => {
            // Numbered paragraphs one after another count on by themselves; the first of a list
            // starts at 1 unless it says otherwise.
            let follows = match previous.map(|p| p.bullet) {
                Some(Bullet::Number(before)) => before.checked_add(1) == Some(n),
                _ => n == 1,
            };
            let start = if follows { String::new() } else { format!(" startAt=\"{n}\"") };
            kids.push_str(marker_color.as_deref().unwrap_or_default());
            kids.push_str(&format!("<a:buFont typeface=\"+mj-lt\"/><a:buAutoNum type=\"arabicPeriod\"{start}/>"));
        }
        Bullet::None if placeholder => kids.push_str("<a:buNone/>"),
        _ => {}
    }

    if attrs.is_empty() && kids.is_empty() {
        return String::new();
    }
    let attrs = if attrs.is_empty() { String::new() } else { format!(" {}", attrs.join(" ")) };
    if kids.is_empty() { format!("<a:pPr{attrs}/>") } else { format!("<a:pPr{attrs}>{kids}</a:pPr>") }
}

fn para_xml(para: &NativeParagraph, previous: Option<&NativeParagraph>, ctx: &mut TextCtx, placeholder: bool) -> String {
    // endParaRPr keeps an empty trailing line (and an empty paragraph) at the last run's size and
    // font instead of PowerPoint's 18pt default.
    let (size, family) = para.runs.last().map_or((0.0, ""), |last| (last.size, last.family.as_str()));
    let end = Look { size, bold: false, italic: false, underline: false, color: None, family, link: None };
    let end = rpr(&end, ctx, "a:endParaRPr");
    format!("<a:p>{}{}{end}</a:p>", ppr_xml(para, previous, placeholder), runs_xml(&para.runs, ctx))
}

pub fn body_pr_xml(body: &BodyOptions) -> String {
    let mut attrs: Vec<String> = Vec::new();
    // Placeholders inherit wrapping from their layout unless told otherwise.
    if body.no_wrap {
        attrs.push("wrap=\"none\"".into());
    } else if !body.placeholder {
        attrs.push("wrap=\"square\"".into());
    }
    if let Some(inset) = body.inset {
        let i = emu_from_units(inset);
        attrs.push(format!("lIns=\"{i}\" tIns=\"{i}\" rIns=\"{i}\" bIns=\"{i}\""));
    }
    if let Some(anchor) = body.anchor {
        attrs.push(format!("anchor=\"{}\"", anchor.value()));
    }
    if !body.placeholder {
        attrs.push("rtlCol=\"0\"".into());
    }
    let kid = match (body.autofit, body.font_scale) {
        (Autofit::Inherit, Some(scale)) if scale < 1.0 => {
            let scale = round_half_up(f64::from(scale.max(0.0)) * 100_000.0) as i64;
            format!("<a:normAutofit fontScale=\"{scale}\" lnSpcReduction=\"10000\"/>")
        }
        (Autofit::Shrink, _) => "<a:normAutofit/>".into(),
        (Autofit::Grow, _) => "<a:spAutoFit/>".into(),
        (Autofit::Off, _) => "<a:noAutofit/>".into(),
        _ => String::new(),
    };
    let attrs = if attrs.is_empty() { String::new() } else { format!(" {}", attrs.join(" ")) };
    if kid.is_empty() { format!("<a:bodyPr{attrs}/>") } else { format!("<a:bodyPr{attrs}>{kid}</a:bodyPr>") }
}

/// A text body as `tag` (`p:txBody`, or `a:txBody` in a table cell). No paragraphs gives one
/// empty one: a body must have a paragraph.
pub fn tx_body_xml(paragraphs: &[NativeParagraph], ctx: &mut TextCtx, body: &BodyOptions, tag: &str) -> String {
    let empty = [NativeParagraph { runs: Vec::new(), align: Align::Left, level: 0, bullet: Bullet::None, line_height: None, space_before: None, bullet_color: None }];
    let list = if paragraphs.is_empty() { &empty[..] } else { paragraphs };
    let paras: String = list
        .iter()
        .enumerate()
        .map(|(i, para)| para_xml(para, i.checked_sub(1).map(|before| &list[before]), ctx, body.placeholder))
        .collect();
    format!("<{tag}>{}<a:lstStyle/>{paras}</{tag}>", body_pr_xml(body))
}
