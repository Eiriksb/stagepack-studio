//! Slide master and the layout catalogue. Layouts carry real types (`titleOnly`, `blank`) so
//! PowerPoint, Google Slides and Keynote all recognize them, and their placeholders carry the
//! geometry, anchors and autofit that slide placeholders inherit.
//!
//! Only the two layouts this app draws are here: Title Only and Blank.

use crate::xml::{NS_PML, XML_DECL, emu, emu_size, esc};

/// A box in inches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Slide geometry in inches, shared with the slide writer (where free shapes go) and the master
/// (where placeholders sit), so the two always line up. Every layout reserves the eyebrow band
/// like a real template, so titles sit at the same height on every slide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub width: f64,
    pub height: f64,
    pub margin_x: f64,
    pub title: Rect,
    pub body: Rect,
}

impl Geometry {
    pub fn new(width: f64, height: f64, margin_x: f64) -> Geometry {
        let content_w = width - margin_x * 2.0;
        let bottom_margin = 0.35;
        let band = |y: f64, h: f64| Rect { x: margin_x, y, w: content_w, h };
        Geometry {
            width,
            height,
            margin_x,
            title: band(0.8, 0.6),
            body: band(1.6, height - bottom_margin - 1.6),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placeholder {
    /// `title`, `body`, ... (the `type` attribute of `p:ph`), if it has one.
    pub kind: Option<&'static str>,
    pub idx: Option<u32>,
    pub name: &'static str,
    pub rect: Rect,
    /// Vertical anchor: `t`, `ctr` or `b`.
    pub anchor: &'static str,
    pub prompt: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutDef {
    pub key: &'static str,
    pub name: &'static str,
    /// The `type` of the `p:sldLayout`.
    pub kind: &'static str,
    pub placeholders: Vec<Placeholder>,
}

const TITLE_PROMPT: &str = "Click to edit Master title style";
const BODY_PROMPT: &str = "Click to edit Master text styles";

pub fn layout_defs(g: &Geometry) -> Vec<LayoutDef> {
    let title = Placeholder {
        kind: Some("title"),
        idx: None,
        name: "Title 1",
        rect: g.title,
        anchor: "b",
        prompt: TITLE_PROMPT,
    };
    vec![
        LayoutDef { key: "titleOnly", name: "Title Only", kind: "titleOnly", placeholders: vec![title] },
        LayoutDef { key: "blank", name: "Blank", kind: "blank", placeholders: vec![] },
    ]
}

const SP_TREE_HEADER: &str = concat!(
    "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>",
    "<p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>"
);

/// One prompt paragraph of a master or layout placeholder: a nesting level and its text.
struct Para<'a> {
    level: u32,
    text: &'a str,
}

/// A placeholder with a position. Slides leave the position out and inherit it from the layout,
/// which is what makes Reset Slide and layout switching work; the master and layouts say it.
fn placeholder_xml(id: u32, p: &Placeholder, paras: &[Para], lang: &str) -> String {
    let kind = p.kind.map(|k| format!(" type=\"{k}\"")).unwrap_or_default();
    let idx = p.idx.map(|i| format!(" idx=\"{i}\"")).unwrap_or_default();
    let lang = esc(lang);
    // A placeholder inherits its bullets from the master's bodyStyle, so a paragraph only says
    // its level. endParaRPr keeps an empty line at the run's size instead of 18pt.
    let paras: String = paras
        .iter()
        .map(|para| {
            let ppr = if para.level > 0 { format!("<a:pPr lvl=\"{}\"/>", para.level.min(8)) } else { String::new() };
            format!(
                "<a:p>{ppr}<a:r><a:rPr lang=\"{lang}\" dirty=\"0\"/><a:t>{}</a:t></a:r><a:endParaRPr lang=\"{lang}\" dirty=\"0\"/></a:p>",
                esc(para.text)
            )
        })
        .collect();
    let r = p.rect;
    format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name}\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr>\
         <p:nvPr><p:ph{kind}{idx}/></p:nvPr></p:nvSpPr>\
         <p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm></p:spPr>\
         <p:txBody><a:bodyPr anchor=\"{anchor}\"><a:normAutofit/></a:bodyPr><a:lstStyle/>{paras}</p:txBody></p:sp>",
        name = esc(p.name),
        x = emu(r.x),
        y = emu(r.y),
        cx = emu_size(r.w),
        cy = emu_size(r.h),
        anchor = p.anchor,
    )
}

pub fn layout_xml(def: &LayoutDef, lang: &str) -> String {
    let sps: String = def
        .placeholders
        .iter()
        .enumerate()
        .map(|(i, p)| placeholder_xml(i as u32 + 2, p, &[Para { level: 0, text: p.prompt }], lang))
        .collect();
    format!(
        "{XML_DECL}<p:sldLayout {NS_PML} type=\"{kind}\" preserve=\"1\"><p:cSld name=\"{name}\">\
         <p:spTree>{SP_TREE_HEADER}{sps}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>",
        kind = def.kind,
        name = esc(def.name),
    )
}

const CLR_MAP: &str = concat!(
    "<p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" ",
    "accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/>"
);

const P_ATTRS: &str = "algn=\"l\" defTabSz=\"914400\" rtl=\"0\" eaLnBrk=\"1\" latinLnBrk=\"0\" hangingPunct=\"1\"";

/// Default run properties of one text level: `heading` picks the theme's major font, otherwise the
/// minor one.
fn def_rpr(sz: u32, bold: bool, heading: bool) -> String {
    let font = if heading { "mj" } else { "mn" };
    let b = if bold { " b=\"1\"" } else { "" };
    format!(
        "<a:defRPr sz=\"{}\"{b} kern=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill>\
         <a:latin typeface=\"+{font}-lt\"/><a:ea typeface=\"+{font}-ea\"/><a:cs typeface=\"+{font}-cs\"/></a:defRPr>",
        sz * 100
    )
}

fn title_style() -> String {
    format!(
        "<p:titleStyle><a:lvl1pPr {P_ATTRS}><a:lnSpc><a:spcPct val=\"90000\"/></a:lnSpc><a:spcBef><a:spcPct val=\"0\"/></a:spcBef>\
         <a:buNone/>{}</a:lvl1pPr></p:titleStyle>",
        def_rpr(26, true, true)
    )
}

/// Body levels 1-3: 0.25"/0.5"/0.75" left margin with a 0.2" hanging bullet, 13/12/11 pt, accent1
/// bullets. The deck's brand color comes from the master, not from each slide.
fn body_style() -> String {
    let levels: String = [(228_600, 13), (457_200, 12), (685_800, 11)]
        .iter()
        .enumerate()
        .map(|(i, (mar_l, sz))| {
            let n = i + 1;
            format!(
                "<a:lvl{n}pPr marL=\"{mar_l}\" indent=\"-182880\" {P_ATTRS}><a:lnSpc><a:spcPct val=\"100000\"/></a:lnSpc>\
                 <a:spcBef><a:spcPts val=\"600\"/></a:spcBef><a:buClr><a:schemeClr val=\"accent1\"/></a:buClr>\
                 <a:buFont typeface=\"Arial\"/><a:buChar char=\"•\"/>{}</a:lvl{n}pPr>",
                def_rpr(*sz, false, false)
            )
        })
        .collect();
    format!("<p:bodyStyle>{levels}</p:bodyStyle>")
}

fn other_style() -> String {
    format!(
        "<p:otherStyle><a:defPPr><a:defRPr lang=\"en-US\"/></a:defPPr><a:lvl1pPr marL=\"0\" {P_ATTRS}>{}</a:lvl1pPr></p:otherStyle>",
        def_rpr(18, false, false)
    )
}

/// A layout's id in the master's `sldLayoutIdLst`, and the master's relationship id for it.
/// Masters and layouts share one id space that starts at 2^31.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutId {
    pub id: u32,
    pub r_id: String,
}

pub fn master_xml(g: &Geometry, lang: &str, layout_ids: &[LayoutId]) -> String {
    let title = Placeholder {
        kind: Some("title"),
        idx: None,
        name: "Title Placeholder 1",
        rect: g.title,
        anchor: "b",
        prompt: TITLE_PROMPT,
    };
    let body = Placeholder {
        kind: Some("body"),
        idx: Some(1),
        name: "Text Placeholder 2",
        rect: g.body,
        anchor: "t",
        prompt: BODY_PROMPT,
    };
    let title = placeholder_xml(2, &title, &[Para { level: 0, text: TITLE_PROMPT }], lang);
    let body = placeholder_xml(
        3,
        &body,
        &[
            Para { level: 0, text: BODY_PROMPT },
            Para { level: 1, text: "Second level" },
            Para { level: 2, text: "Third level" },
        ],
        lang,
    );
    let ids: String =
        layout_ids.iter().map(|l| format!("<p:sldLayoutId id=\"{}\" r:id=\"{}\"/>", l.id, esc(&l.r_id))).collect();
    format!(
        "{XML_DECL}<p:sldMaster {NS_PML}><p:cSld><p:bg><p:bgRef idx=\"1001\"><a:schemeClr val=\"bg1\"/></p:bgRef></p:bg>\
         <p:spTree>{SP_TREE_HEADER}{title}{body}</p:spTree></p:cSld>{CLR_MAP}\
         <p:sldLayoutIdLst>{ids}</p:sldLayoutIdLst><p:txStyles>{ts}{bs}{os}</p:txStyles></p:sldMaster>",
        ts = title_style(),
        bs = body_style(),
        os = other_style(),
    )
}
