//! PresentationML shape builders: placeholders, text boxes and geometry shapes, pictures and
//! graphic frames (tables). Positions arrive in slide units, and ids come from the caller's
//! per-part counter so they stay unique within a slide.

use slides_engine::{Align, NativeContent, NativeParagraph, Rect};
use toolkit::{Color, ShapeKind};

use crate::text::{Anchor, BodyOptions, TextCtx, tx_body_xml};
use crate::xml::{NS_PML, XML_DECL, emu_from_units, esc, round_half_up, solid_fill_of};

pub const SP_TREE_HEADER: &str = concat!(
    "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>",
    "<p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>"
);

fn xfrm_tagged(rect: Rect, rotate: f32, tag: &str) -> String {
    // rot is in 60000ths of a degree, 0..21599999. Not finite, or none, says nothing.
    let rot = if rotate.is_finite() && rotate != 0.0 {
        let degrees = f64::from(rotate).rem_euclid(360.0);
        format!(" rot=\"{}\"", round_half_up(degrees * 60_000.0) as i64 % 21_600_000)
    } else {
        String::new()
    };
    // Extents must not be negative: PowerPoint repairs the file if they are.
    format!(
        "<{tag}{rot}><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></{tag}>",
        emu_from_units(rect.x),
        emu_from_units(rect.y),
        emu_from_units(rect.w).max(0),
        emu_from_units(rect.h).max(0),
    )
}

/// A position and size, turned `rotate` degrees.
pub fn xfrm_xml(rect: Rect, rotate: f32) -> String {
    xfrm_tagged(rect, rotate, "a:xfrm")
}

/// Which placeholder of the layout a shape stands in for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ph<'a> {
    /// `title`, `body`, ... (the `type` attribute of `p:ph`), if it has one.
    pub kind: Option<&'a str>,
    pub idx: Option<u32>,
}

/// A placeholder. With no `rect` it inherits its position and size from the layout, which is what
/// makes Reset Slide and layout switching work.
pub fn placeholder_xml(
    id: u32,
    name: &str,
    ph: &Ph,
    rect: Option<Rect>,
    paragraphs: &[NativeParagraph],
    ctx: &mut TextCtx,
    body: &BodyOptions,
) -> String {
    placeholder_xml_rotated(id, name, ph, rect, 0.0, None, paragraphs, ctx, body)
}

/// `placeholder_xml`, turned `rotate` degrees (which needs a `rect`) and with a `fill` behind it.
#[allow(clippy::too_many_arguments)]
pub fn placeholder_xml_rotated(
    id: u32,
    name: &str,
    ph: &Ph,
    rect: Option<Rect>,
    rotate: f32,
    fill: Option<Color>,
    paragraphs: &[NativeParagraph],
    ctx: &mut TextCtx,
    body: &BodyOptions,
) -> String {
    let kind = ph.kind.map(|k| format!(" type=\"{}\"", esc(k))).unwrap_or_default();
    let idx = ph.idx.map(|i| format!(" idx=\"{i}\"")).unwrap_or_default();
    let xfrm = rect.map(|r| xfrm_xml(r, rotate)).unwrap_or_default();
    let fill = fill.map(solid_fill_of).unwrap_or_default();
    let sp_pr = if xfrm.is_empty() && fill.is_empty() { "<p:spPr/>".to_string() } else { format!("<p:spPr>{xfrm}{fill}</p:spPr>") };
    let body = tx_body_xml(paragraphs, ctx, &BodyOptions { placeholder: true, ..body.clone() }, "p:txBody");
    format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{}\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr>\
         <p:nvPr><p:ph{kind}{idx}/></p:nvPr></p:nvSpPr>{sp_pr}{body}</p:sp>",
        esc(name)
    )
}

fn geometry_xml(kind: ShapeKind, rect: Rect, radius: f32) -> String {
    if kind == ShapeKind::Rounded {
        // roundRect's adj is the corner radius as a fraction (x100000) of the shorter side.
        let side = f64::from(rect.w.min(rect.h)).max(0.0001 * 144.0);
        let ratio = f64::from(radius) / side * 100_000.0;
        let adj = if ratio.is_finite() { round_half_up(ratio.clamp(0.0, 50_000.0)) as i64 } else { 0 };
        return format!("<a:prstGeom prst=\"roundRect\"><a:avLst><a:gd name=\"adj\" fmla=\"val {adj}\"/></a:avLst></a:prstGeom>");
    }
    let preset = match kind {
        ShapeKind::Ellipse => "ellipse",
        ShapeKind::Line | ShapeKind::Arrow => "line",
        ShapeKind::Callout => "wedgeRoundRectCallout",
        ShapeKind::Rect | ShapeKind::Rounded => "rect",
    };
    format!("<a:prstGeom prst=\"{preset}\"><a:avLst/></a:prstGeom>")
}

fn line_xml(outline: Option<(Color, f32)>, arrow: bool) -> String {
    // Explicit "no line": without a p:style there is no line anyway in PowerPoint, but Keynote
    // and Google Slides apply their own defaults to a shape with no a:ln at all.
    let head = if arrow { "<a:tailEnd type=\"triangle\"/>" } else { "" };
    match outline {
        None => format!("<a:ln><a:noFill/>{head}</a:ln>"),
        Some((color, width)) => format!("<a:ln w=\"{}\">{}{head}</a:ln>", emu_from_units(width), solid_fill_of(color)),
    }
}

/// What `sp_xml` needs to write one shape or text box.
struct Sp<'a> {
    kind: ShapeKind,
    fill: Option<Color>,
    outline: Option<(Color, f32)>,
    radius: f32,
    paragraphs: &'a [NativeParagraph],
    inset: f32,
    anchor: Anchor,
}

fn sp_xml(id: u32, rect: Rect, rotate: f32, sp: &Sp, ctx: &mut TextCtx) -> String {
    let text_box = sp.kind == ShapeKind::Rect && sp.fill.is_none() && sp.outline.is_none();
    let (name, tx_box) = if text_box { ("TextBox", " txBox=\"1\"") } else { ("Shape", "") };
    // spPr child order: xfrm, geometry, fill, ln.
    let fill = sp.fill.map_or("<a:noFill/>".to_string(), solid_fill_of);
    let options = BodyOptions { inset: Some(sp.inset), anchor: Some(sp.anchor), ..BodyOptions::default() };
    format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name} {id}\"/><p:cNvSpPr{tx_box}/><p:nvPr/></p:nvSpPr>\
         <p:spPr>{}{}{fill}{}</p:spPr>{}</p:sp>",
        xfrm_xml(rect, rotate),
        geometry_xml(sp.kind, rect, sp.radius),
        line_xml(sp.outline, sp.kind == ShapeKind::Arrow),
        tx_body_xml(sp.paragraphs, ctx, &options, "p:txBody"),
    )
}

/// A text box or a shape at `rect`, which is where it goes on the slide. Tables and pictures
/// are not shapes, and give `None`.
pub fn shape_xml(id: u32, rect: Rect, content: &NativeContent, ctx: &mut TextCtx) -> Option<String> {
    shape_xml_rotated(id, rect, 0.0, content, ctx)
}

/// `shape_xml`, turned `rotate` degrees around its centre.
pub fn shape_xml_rotated(id: u32, rect: Rect, rotate: f32, content: &NativeContent, ctx: &mut TextCtx) -> Option<String> {
    match content {
        NativeContent::Text { paragraphs, fill, inset } => {
            let sp = Sp { kind: ShapeKind::Rect, fill: *fill, outline: None, radius: 0.0, paragraphs, inset: *inset, anchor: Anchor::Top };
            Some(sp_xml(id, rect, rotate, &sp, ctx))
        }
        NativeContent::Shape { kind, fill, outline, outline_width, radius, paragraphs, inset, centered } => {
            let outline = outline.filter(|_| outline_width.is_finite() && *outline_width > 0.0).map(|c| (c, *outline_width));
            // A shape that centres its text centres every paragraph in it.
            let centred: Vec<NativeParagraph>;
            let paragraphs = if *centered {
                centred = paragraphs.iter().map(|p| NativeParagraph { align: Align::Center, ..p.clone() }).collect();
                &centred
            } else {
                paragraphs
            };
            let anchor = if *centered { Anchor::Center } else { Anchor::Top };
            let sp = Sp { kind: *kind, fill: *fill, outline, radius: *radius, paragraphs, inset: *inset, anchor };
            Some(sp_xml(id, rect, rotate, &sp, ctx))
        }
        NativeContent::Table { .. } | NativeContent::Picture { .. } => None,
    }
}

/// The slide's background (`p:bg`): a picture (by relationship id) stretched over the slide, or a color.
pub fn background_xml(color: Color, picture_r_id: Option<&str>) -> String {
    let fill = match picture_r_id {
        Some(r_id) => format!(
            "<a:blipFill dpi=\"0\" rotWithShape=\"1\"><a:blip r:embed=\"{}\"/><a:srcRect/><a:stretch><a:fillRect/></a:stretch></a:blipFill>",
            esc(r_id)
        ),
        None => solid_fill_of(color),
    };
    format!("<p:bg><p:bgPr>{fill}<a:effectLst/></p:bgPr></p:bg>")
}

/// A picture stretched over `rect`. `alt` is its description for screen readers, if it has one.
pub fn picture_xml(id: u32, name: &str, alt: &str, r_id: &str, rect: Rect) -> String {
    picture_xml_rotated(id, name, alt, r_id, rect, 0.0)
}

/// `picture_xml`, turned `rotate` degrees.
pub fn picture_xml_rotated(id: u32, name: &str, alt: &str, r_id: &str, rect: Rect, rotate: f32) -> String {
    let descr = if alt.is_empty() { String::new() } else { format!(" descr=\"{}\"", esc(alt)) };
    format!(
        "<p:pic><p:nvPicPr><p:cNvPr id=\"{id}\" name=\"{}\"{descr}/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>\
         <p:blipFill><a:blip r:embed=\"{}\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>\
         <p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>",
        esc(name),
        esc(r_id),
        xfrm_xml(rect, rotate),
    )
}

/// A frame holding `inner`, a table's `a:tbl` for one, as `uri` says.
pub fn graphic_frame_xml(id: u32, name: &str, rect: Rect, uri: &str, inner: &str) -> String {
    format!(
        "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"{}\"/>\
         <p:cNvGraphicFramePr><a:graphicFrameLocks noGrp=\"1\"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr>\
         {}<a:graphic><a:graphicData uri=\"{}\">{inner}</a:graphicData></a:graphic></p:graphicFrame>",
        esc(name),
        xfrm_tagged(rect, 0.0, "p:xfrm"),
        esc(uri),
    )
}

/// A slide: its background (`background_xml`, or nothing), the shapes in drawing order, and its
/// transition and timing (`transition_xml` and `animation_timing_xml`, or nothing), in that order.
pub fn slide_xml(shapes: &str, background: &str, timing: &str, transition: &str) -> String {
    format!(
        "{XML_DECL}<p:sld {NS_PML}><p:cSld>{background}<p:spTree>{SP_TREE_HEADER}{shapes}</p:spTree></p:cSld>\
         <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>{transition}{timing}</p:sld>"
    )
}
