//! A `<Shape>` as an inline SVG, drawn as the engine draws it: the same corners, the same tail
//! on a callout and the same head on an arrow, in the same colours and widths.

use crate::export::px;

/// The kinds of shape there are. Any other kind is a rectangle, as it is in the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rect,
    Rounded,
    Ellipse,
    Line,
    Arrow,
    Callout,
}

impl Kind {
    pub fn read(value: Option<&str>) -> Kind {
        match value.map(str::trim) {
            Some("rounded") => Kind::Rounded,
            Some("ellipse") => Kind::Ellipse,
            Some("line") => Kind::Line,
            Some("arrow") => Kind::Arrow,
            Some("callout") => Kind::Callout,
            _ => Kind::Rect,
        }
    }
}

/// The corner of a rounded shape that gives none.
const DEFAULT_RADIUS: f32 = 24.0;
const DEFAULT_LINE_WIDTH: f32 = 4.0;
const MAX_LINE_WIDTH: f32 = 256.0;
/// How far a callout's tail reaches below its body.
pub const CALLOUT_TAIL: f32 = 40.0;
const MIN_ARROW_HEAD: f32 = 24.0;
const ARROW_HEAD_PER_LINE_WIDTH: f32 = 6.0;

/// What a shape looks like, once its colours are read.
pub struct Look {
    pub kind: Kind,
    /// As `#rrggbb`.
    pub fill: Option<String>,
    pub outline: Option<String>,
    /// As given, with 1 for an outline that gives none.
    pub outline_width: f32,
    /// The talk's text colour, which a line with no colour of its own is drawn in.
    pub text: String,
}

/// The `<svg>` of a shape `w` by `h`, on one line, filling the box it's placed in. `id` is
/// unique in the talk, for an arrow's head.
pub fn svg(look: &Look, w: f32, h: f32, id: &str) -> String {
    let open = format!(
        "<svg width=\"{w}\" height=\"{h}\" style=\"position:absolute;left:0;top:0;overflow:visible\">",
        w = px(w),
        h = px(h)
    );
    format!("{open}{}</svg>", drawing(look, w, h, id))
}

fn paint(look: &Look) -> String {
    let fill = look.fill.as_deref().unwrap_or("none");
    let width = look.outline_width.clamp(0.0, MAX_LINE_WIDTH);
    match look.outline.as_deref().filter(|_| width > 0.0) {
        Some(outline) => format!("fill=\"{fill}\" stroke=\"{outline}\" stroke-width=\"{}\"", px(width)),
        None => format!("fill=\"{fill}\""),
    }
}

fn drawing(look: &Look, w: f32, h: f32, id: &str) -> String {
    if matches!(look.kind, Kind::Line | Kind::Arrow) {
        return line(look, w, h, id);
    }
    if w <= 0.0 || h <= 0.0 {
        return String::new();
    }
    let paint = paint(look);
    match look.kind {
        Kind::Rounded => format!("<path d=\"{}\" {paint}/>", rounded_path(w, h, false)),
        Kind::Ellipse => format!("<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\" {paint}/>", px(w / 2.0), px(h / 2.0), px(w / 2.0), px(h / 2.0)),
        Kind::Callout => format!("<path d=\"{}\" {paint}/>", rounded_path(w, h, true)),
        _ => format!("<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" {paint}/>", px(w), px(h)),
    }
}

/// A rectangle with corners curved as the engine's `rounded_rect` curves them, and for a
/// callout a tail at the bottom left, as its `closed_path` draws it.
fn rounded_path(w: f32, h: f32, callout: bool) -> String {
    let tail = if callout { CALLOUT_TAIL.min(h / 2.0) } else { 0.0 };
    let body = h - tail;
    let r = DEFAULT_RADIUS.min(w / 2.0).min(body / 2.0);
    let p = |x: f32, y: f32| format!("{},{}", px(x), px(y));
    let mut steps = vec![
        format!("M{}", p(r, 0.0)),
        format!("L{}", p(w - r, 0.0)),
        format!("Q{} {}", p(w, 0.0), p(w, r)),
        format!("L{}", p(w, body - r)),
        format!("Q{} {}", p(w, body), p(w - r, body)),
    ];
    if callout {
        steps.extend([format!("L{}", p(w * 0.35, body)), format!("L{}", p(w * 0.15, h)), format!("L{}", p(w * 0.2, body))]);
    }
    steps.extend([
        format!("L{}", p(r, body)),
        format!("Q{} {}", p(0.0, body), p(0.0, body - r)),
        format!("L{}", p(0.0, r)),
        format!("Q{} {}", p(0.0, 0.0), p(r, 0.0)),
        "Z".to_string(),
    ]);
    steps.join(" ")
}

/// A line from the top-left corner to the bottom-right one, in the outline colour or, with
/// none, the fill, or else the talk's text. An arrow's head is a marker at the line's end,
/// as long as the engine's, and the line stops where the head starts.
fn line(look: &Look, w: f32, h: f32, id: &str) -> String {
    let length = w.hypot(h);
    if length == 0.0 {
        return String::new();
    }
    let color = look.outline.as_deref().or(look.fill.as_deref()).unwrap_or(&look.text);
    let width = if look.outline_width > 0.0 { look.outline_width.min(MAX_LINE_WIDTH) } else { DEFAULT_LINE_WIDTH };
    if look.kind != Kind::Arrow {
        return format!("<line x1=\"0\" y1=\"0\" x2=\"{}\" y2=\"{}\" stroke=\"{color}\" stroke-width=\"{}\"/>", px(w), px(h), px(width));
    }
    let head = (width * ARROW_HEAD_PER_LINE_WIDTH).max(MIN_ARROW_HEAD).min(length);
    let (end_x, end_y) = (w - w / length * head, h - h / length * head);
    format!(
        "<defs><marker id=\"{id}\" markerUnits=\"userSpaceOnUse\" markerWidth=\"{head}\" markerHeight=\"{head}\" refX=\"0\" refY=\"{half}\" orient=\"auto\"><path d=\"M0,0 L{head},{half} L0,{head} Z\" fill=\"{color}\"/></marker></defs><line x1=\"0\" y1=\"0\" x2=\"{}\" y2=\"{}\" stroke=\"{color}\" stroke-width=\"{}\" marker-end=\"url(#{id})\"/>",
        px(end_x),
        px(end_y),
        px(width),
        head = px(head),
        half = px(head / 2.0),
    )
}
