//! Speaker notes: a notes master (written only when some slide has notes) and one notes slide per
//! slide that has them. Positions are PowerPoint's own defaults for a 7.5 x 10 inch notes page.
//! A port of `notes.mjs` from `slidev-pptx-export`.

use slides_engine::Rect;

use crate::shapes::{SP_TREE_HEADER, xfrm_xml};
use crate::text::TextCtx;
use crate::xml::{NS_PML, XML_DECL, esc};

const CLR_MAP: &str = concat!(
    "<p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" ",
    "accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/>"
);

const SLIDE_IMAGE_LOCKS: &str = "<a:spLocks noGrp=\"1\" noRot=\"1\" noChangeAspect=\"1\"/>";

/// JavaScript's `trim`, which also takes the byte order mark.
fn trim(line: &str) -> &str {
    line.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}')
}

/// Notes cut into paragraphs: a line each, with at most one empty paragraph for a run of blank
/// lines, and none at the start or the end. `None` is an empty paragraph.
fn notes_paragraphs(text: &str) -> Vec<Option<&str>> {
    let mut paras: Vec<Option<&str>> = Vec::new();
    for raw in text.split('\n') {
        let line = trim(raw);
        if line.is_empty() {
            if matches!(paras.last(), Some(Some(_))) {
                paras.push(None);
            }
            continue;
        }
        paras.push(Some(line));
    }
    while matches!(paras.last(), Some(None)) {
        paras.pop();
    }
    paras
}

/// The body placeholder's text: plain paragraphs that take everything from the master.
fn plain_body(paras: &[Option<&str>], ctx: &TextCtx) -> String {
    let lang = esc(&ctx.lang);
    let end = format!("<a:endParaRPr lang=\"{lang}\" dirty=\"0\"/>");
    let list: Vec<String> = if paras.is_empty() {
        vec![format!("<a:p>{end}</a:p>")]
    } else {
        paras
            .iter()
            .map(|para| match para {
                Some(line) => format!("<a:p><a:r><a:rPr lang=\"{lang}\" dirty=\"0\"/><a:t>{}</a:t></a:r>{end}</a:p>", esc(line)),
                None => format!("<a:p>{end}</a:p>"),
            })
            .collect()
    };
    format!("<p:txBody><a:bodyPr/><a:lstStyle/>{}</p:txBody>", list.concat())
}

fn body_placeholder(ph: &str, sp_pr: &str, paras: &[Option<&str>], ctx: &TextCtx) -> String {
    format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"Notes Placeholder 2\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr>\
         <p:nvPr>{ph}</p:nvPr></p:nvSpPr>{sp_pr}{}</p:sp>",
        plain_body(paras, ctx)
    )
}

pub fn notes_master_xml(ctx: &mut TextCtx) -> String {
    // 0.75, 1.25, 6 and 3.375 inches; then 0.75, 4.8125, 6 and 3.9375 (a slide unit is 1/144 inch).
    let image_box = Rect { x: 108.0, y: 180.0, w: 864.0, h: 486.0 };
    let body_box = Rect { x: 108.0, y: 693.0, w: 864.0, h: 567.0 };
    let image = format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Slide Image Placeholder 1\"/><p:cNvSpPr>{SLIDE_IMAGE_LOCKS}</p:cNvSpPr>\
         <p:nvPr><p:ph type=\"sldImg\" idx=\"2\"/></p:nvPr></p:nvSpPr>\
         <p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom><a:noFill/>\
         <a:ln w=\"12700\"><a:solidFill><a:prstClr val=\"black\"/></a:solidFill></a:ln></p:spPr></p:sp>",
        xfrm_xml(image_box, 0.0)
    );
    let body = body_placeholder(
        "<p:ph type=\"body\" idx=\"3\" sz=\"quarter\"/>",
        &format!("<p:spPr>{}</p:spPr>", xfrm_xml(body_box, 0.0)),
        &[Some("Click to edit Master text styles")],
        ctx,
    );
    let style = concat!(
        "<p:notesStyle><a:lvl1pPr marL=\"0\" algn=\"l\" defTabSz=\"914400\" rtl=\"0\" eaLnBrk=\"1\" latinLnBrk=\"0\" hangingPunct=\"1\">",
        "<a:defRPr sz=\"1200\" kern=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill>",
        "<a:latin typeface=\"+mn-lt\"/><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/></a:defRPr></a:lvl1pPr></p:notesStyle>"
    );
    format!(
        "{XML_DECL}<p:notesMaster {NS_PML}><p:cSld><p:bg><p:bgRef idx=\"1001\"><a:schemeClr val=\"bg1\"/></p:bgRef></p:bg>\
         <p:spTree>{SP_TREE_HEADER}{image}{body}</p:spTree></p:cSld>{CLR_MAP}{style}</p:notesMaster>"
    )
}

pub fn notes_slide_xml(text: &str, ctx: &mut TextCtx) -> String {
    let image = format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"Slide Image Placeholder 1\"/><p:cNvSpPr>{SLIDE_IMAGE_LOCKS}</p:cNvSpPr>\
         <p:nvPr><p:ph type=\"sldImg\"/></p:nvPr></p:nvSpPr><p:spPr/></p:sp>"
    );
    let body = body_placeholder("<p:ph type=\"body\" idx=\"1\"/>", "<p:spPr/>", &notes_paragraphs(text), ctx);
    format!(
        "{XML_DECL}<p:notes {NS_PML}><p:cSld><p:spTree>{SP_TREE_HEADER}{image}{body}</p:spTree></p:cSld>\
         <p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>"
    )
}
