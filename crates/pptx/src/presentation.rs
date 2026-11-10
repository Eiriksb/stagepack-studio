//! presentation.xml plus the small package-level parts PowerPoint expects beside it.

use crate::xml::{NS_PML, XML_DECL, emu_size, esc};

const DEFAULT_TEXT_STYLE: &str = concat!(
    "<p:defaultTextStyle><a:defPPr><a:defRPr lang=\"en-US\"/></a:defPPr>",
    "<a:lvl1pPr marL=\"0\" algn=\"l\" defTabSz=\"914400\" rtl=\"0\" eaLnBrk=\"1\" latinLnBrk=\"0\" hangingPunct=\"1\">",
    "<a:defRPr sz=\"1800\" kern=\"1200\"><a:solidFill><a:schemeClr val=\"tx1\"/></a:solidFill>",
    "<a:latin typeface=\"+mn-lt\"/><a:ea typeface=\"+mn-ea\"/><a:cs typeface=\"+mn-cs\"/></a:defRPr></a:lvl1pPr></p:defaultTextStyle>"
);

/// A slide's id in `sldIdLst` (they start at 256) and the presentation's relationship id for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlideRef {
    pub id: u32,
    pub r_id: String,
}

/// One embedded font family: the relationship id of each style's font part that exists.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EmbeddedFont {
    pub typeface: String,
    pub regular: Option<String>,
    pub bold: Option<String>,
    pub italic: Option<String>,
    pub bold_italic: Option<String>,
}

pub struct Presentation<'a> {
    pub master_id: u32,
    pub master_r_id: &'a str,
    pub notes_master_r_id: Option<&'a str>,
    pub slides: &'a [SlideRef],
    /// Slide size in inches.
    pub width: f64,
    pub height: f64,
    pub embedded_fonts: &'a [EmbeddedFont],
}

/// PowerPoint only accepts slide sizes between 1" and 56".
fn clamp_size(inches: f64) -> i64 {
    emu_size(inches).clamp(914_400, 51_206_400)
}

pub fn presentation_xml(p: &Presentation) -> String {
    // embeddedFontLst comes after notesSz; embedTrueTypeFonts tells PowerPoint the file carries them.
    let fonts = if p.embedded_fonts.is_empty() {
        String::new()
    } else {
        let list: String = p
            .embedded_fonts
            .iter()
            .map(|f| {
                let styles: String = [
                    ("regular", &f.regular),
                    ("bold", &f.bold),
                    ("italic", &f.italic),
                    ("boldItalic", &f.bold_italic),
                ]
                .iter()
                .filter_map(|(tag, r_id)| r_id.as_ref().map(|r| format!("<p:{tag} r:id=\"{}\"/>", esc(r))))
                .collect();
                format!("<p:embeddedFont><p:font typeface=\"{}\"/>{styles}</p:embeddedFont>", esc(&f.typeface))
            })
            .collect();
        format!("<p:embeddedFontLst>{list}</p:embeddedFontLst>")
    };
    // Child order per CT_Presentation: sldMasterIdLst, notesMasterIdLst, sldIdLst, sldSz,
    // notesSz, defaultTextStyle. (pptxgenjs writes sldIdLst before notesMasterIdLst.)
    let notes = p
        .notes_master_r_id
        .map(|r| format!("<p:notesMasterIdLst><p:notesMasterId r:id=\"{}\"/></p:notesMasterIdLst>", esc(r)))
        .unwrap_or_default();
    let slide_ids = if p.slides.is_empty() {
        String::new()
    } else {
        let ids: String =
            p.slides.iter().map(|s| format!("<p:sldId id=\"{}\" r:id=\"{}\"/>", s.id, esc(&s.r_id))).collect();
        format!("<p:sldIdLst>{ids}</p:sldIdLst>")
    };
    let embed = if p.embedded_fonts.is_empty() { "" } else { " embedTrueTypeFonts=\"1\"" };
    format!(
        "{XML_DECL}<p:presentation {NS_PML}{embed}><p:sldMasterIdLst><p:sldMasterId id=\"{}\" r:id=\"{}\"/></p:sldMasterIdLst>\
         {notes}{slide_ids}<p:sldSz cx=\"{}\" cy=\"{}\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/>\
         {fonts}{DEFAULT_TEXT_STYLE}</p:presentation>",
        p.master_id,
        esc(p.master_r_id),
        clamp_size(p.width),
        clamp_size(p.height),
    )
}

pub fn pres_props_xml() -> String {
    format!("{XML_DECL}<p:presentationPr {NS_PML}/>")
}

pub fn view_props_xml() -> String {
    format!(
        "{XML_DECL}<p:viewPr {NS_PML}><p:normalViewPr><p:restoredLeft sz=\"15620\"/><p:restoredTop sz=\"94660\"/></p:normalViewPr>\
         <p:gridSpacing cx=\"76200\" cy=\"76200\"/></p:viewPr>"
    )
}

pub fn app_xml(slides: usize, notes: usize) -> String {
    format!(
        "{XML_DECL}<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" \
         xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\">\
         <Application>slidev-pptx-export</Application><PresentationFormat>Custom</PresentationFormat>\
         <Slides>{slides}</Slides><Notes>{notes}</Notes></Properties>"
    )
}

/// Dates are left out unless asked for (`date` is an ISO-8601 UTC time such as
/// `2026-11-10T09:08:07Z`), so two exports of the same deck are byte-identical.
pub fn core_xml(title: &str, author: &str, date: Option<&str>) -> String {
    let dates = date
        .map(|d| {
            let d = esc(d);
            format!(
                "<dcterms:created xsi:type=\"dcterms:W3CDTF\">{d}</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">{d}</dcterms:modified>"
            )
        })
        .unwrap_or_default();
    let (title, author) = (esc(title), esc(author));
    format!(
        "{XML_DECL}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" \
         xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\
         <dc:title>{title}</dc:title><dc:creator>{author}</dc:creator><cp:lastModifiedBy>{author}</cp:lastModifiedBy>\
         <cp:revision>1</cp:revision>{dates}</cp:coreProperties>"
    )
}
