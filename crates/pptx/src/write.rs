//! A whole `.pptx` from a laid-out talk: the part wiring of `writePptx` in `slidev-pptx-export`,
//! over the writers of the other modules. This file owns part names, relationships and ids; the
//! XML of each part comes from the module for it.
//!
//! The order of the package calls is the original's, so the relationship ids come out the same:
//! the theme, the layouts, the master, the notes master, each slide with its notes, the small
//! package parts, then the presentation itself.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use tiny_skia::Pixmap;
use slides_engine::{Deck, Fonts, LayerSpec, NativeContent, NativeElement, NativeParagraph, Rect, TalkFiles, render_layer};

use crate::eot::eot_from_ttf;
use crate::master::{Geometry, LayoutId, layout_defs, layout_xml, master_xml};
use crate::notes::{notes_master_xml, notes_slide_xml};
use crate::package::{CT, Package, REL};
use crate::presentation::{
    EmbeddedFont, Presentation, SlideRef, app_xml, core_xml, pres_props_xml, presentation_xml, view_props_xml,
};
use crate::shapes::{
    Ph, background_xml, freeform_xml, graphic_frame_xml, picture_xml_rotated, placeholder_xml_rotated, shape_xml_rotated, slide_xml_shown_or_not,
};
use crate::table::{TABLE_URI, table_styles_xml, table_xml};
use crate::text::{Anchor, Autofit, BodyOptions, TextCtx, link_target};
use crate::theme::theme_xml;
use crate::timing::{Effect, animation_timing_xml, transition_xml};

const PRES: &str = "/ppt/presentation.xml";
const MASTER: &str = "/ppt/slideMasters/slideMaster1.xml";
const THEME: &str = "/ppt/theme/theme1.xml";
const NOTES_THEME: &str = "/ppt/theme/theme2.xml";
const NOTES_MASTER: &str = "/ppt/notesMasters/notesMaster1.xml";
const LANG: &str = "en-US";
/// What a terminal recording's picture is described as, for screen readers.
const TERMINAL_ALT: &str = "Terminal recording";
/// What a picture of a terminal that names a command and no recording is described as.
const LIVE_TERMINAL_ALT: &str = "Live terminal";
/// The longest edge a terminal recording's picture is drawn at, and the most pixels it's drawn
/// at for each slide unit.
const TERMINAL_EDGE: f32 = 3840.0;
const TERMINAL_SCALE: f32 = 2.0;

/// Slides are 1920 by 1080 units, and a unit is 1/144 inch.
const SLIDE_WIDTH_IN: f64 = 1920.0 / 144.0;
const SLIDE_HEIGHT_IN: f64 = 1080.0 / 144.0;
const MARGIN_X_IN: f64 = 0.6;

/// Masters and layouts share one id space that starts at 2^31.
const FIRST_MASTER_ID: u32 = 2_147_483_648;
const FIRST_SLIDE_ID: u32 = 256;

pub struct PptxOutput {
    pub bytes: Vec<u8>,
    /// What the engine had to say about the slides, such as a component it couldn't read.
    pub notices: Vec<String>,
}

type SharedPackage = Rc<RefCell<Package>>;

/// Each distinct picture is stored once, however many slides use it.
#[derive(Default)]
struct MediaStore {
    /// Parts by the hash of their extension and bytes. A match is checked against the bytes.
    by_hash: HashMap<u64, Vec<String>>,
    /// Parts by the talk file they were read from, so a file used again isn't read again.
    by_source: HashMap<String, String>,
    stored: usize,
}

impl MediaStore {
    fn add(&mut self, pkg: &SharedPackage, bytes: Vec<u8>, ext: &str) -> String {
        let mut hasher = DefaultHasher::new();
        (ext, &bytes).hash(&mut hasher);
        let key = hasher.finish();
        let suffix = format!(".{ext}");
        let mut package = pkg.borrow_mut();
        let candidates = self.by_hash.entry(key).or_default();
        if let Some(found) = candidates.iter().find(|part| part.ends_with(&suffix) && package.data(part) == Some(&bytes[..])) {
            return found.clone();
        }
        self.stored += 1;
        let part = format!("/ppt/media/image{}.{ext}", self.stored);
        package.add_media(&part, bytes);
        candidates.push(part.clone());
        part
    }
}

/// The extension a picture file is stored under, if PowerPoint takes that kind as it is.
fn native_picture_ext(src: &str) -> Option<&'static str> {
    match src.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "png" => Some("png"),
        "jpg" | "jpeg" => Some("jpg"),
        "gif" => Some("gif"),
        _ => None,
    }
}

/// Where an element sits on the slide, and how far it is turned. An element of a turned layer
/// goes around the layer's centre, and takes the layer's turn.
fn place(layer: &LayerSpec, element: &NativeElement) -> Option<(Rect, f32)> {
    let (l, e) = (layer.rect, element.rect);
    let finite = [l.x, l.y, l.w, l.h, e.x, e.y, e.w, e.h, layer.rotate].iter().all(|v| v.is_finite());
    if !finite {
        return None;
    }
    let turn = f64::from(layer.rotate).to_radians();
    let (sin, cos) = turn.sin_cos();
    let layer_centre = (f64::from(l.x) + f64::from(l.w) / 2.0, f64::from(l.y) + f64::from(l.h) / 2.0);
    let centre = (
        f64::from(l.x) + f64::from(e.x) + f64::from(e.w) / 2.0,
        f64::from(l.y) + f64::from(e.y) + f64::from(e.h) / 2.0,
    );
    let (dx, dy) = (centre.0 - layer_centre.0, centre.1 - layer_centre.1);
    let turned = (layer_centre.0 + dx * cos - dy * sin, layer_centre.1 + dx * sin + dy * cos);
    let rect = Rect {
        x: (turned.0 - f64::from(e.w) / 2.0) as f32,
        y: (turned.1 - f64::from(e.h) / 2.0) as f32,
        w: e.w,
        h: e.h,
    };
    Some((rect, layer.rotate))
}

/// Everything slide-sized writing needs, shared between the shapes of one slide.
struct SlideEnv<'a> {
    pkg: &'a SharedPackage,
    media: &'a mut MediaStore,
    files: &'a dyn TalkFiles,
    part: &'a str,
    ctx: TextCtx,
}

impl SlideEnv<'_> {
    /// The part of a picture: the talk's own file when PowerPoint takes its kind, else the
    /// picture as it was drawn.
    fn picture_part(&mut self, src: Option<&str>, pixmap: &Pixmap) -> Option<String> {
        if let Some(src) = src {
            if let Some(part) = self.media.by_source.get(src) {
                return Some(part.clone());
            }
            if let Some((ext, bytes)) = native_picture_ext(src).zip(self.files.read(src)) {
                let part = self.media.add(self.pkg, bytes, ext);
                self.media.by_source.insert(src.to_string(), part.clone());
                return Some(part);
            }
        }
        let bytes = pixmap.encode_png().ok()?;
        Some(self.media.add(self.pkg, bytes, "png"))
    }

    /// The element as a shape of the slide, and its effect on `click`.
    fn element_xml(&mut self, id: u32, layer: &LayerSpec, element: &NativeElement, title: bool) -> Option<(String, bool)> {
        let (rect, rotate) = place(layer, element)?;
        match &element.content {
            NativeContent::Text { paragraphs, fill, inset } if title => {
                // As the app draws it: the text where the box starts, not the master's insets,
                // bottom anchor and shrinking.
                let ph = Ph { kind: Some("title"), idx: None };
                let body = BodyOptions { inset: Some(*inset), anchor: Some(Anchor::Top), autofit: Autofit::Off, ..BodyOptions::default() };
                let xml = placeholder_xml_rotated(id, "Title 1", &ph, Some(rect), rotate, *fill, paragraphs, &mut self.ctx, &body);
                Some((xml, true))
            }
            NativeContent::Text { .. } => Some((shape_xml_rotated(id, rect, rotate, &element.content, &mut self.ctx)?, true)),
            NativeContent::Shape { paragraphs, .. } => {
                Some((shape_xml_rotated(id, rect, rotate, &element.content, &mut self.ctx)?, !paragraphs.is_empty()))
            }
            NativeContent::Table { rows, column_widths, row_heights, line } => {
                // A table frame can't be turned in PowerPoint, so it is only moved.
                let inner = table_xml(rows, column_widths, row_heights, rect, *line, &mut self.ctx);
                Some((graphic_frame_xml(id, &format!("Table {id}"), rect, TABLE_URI, &inner), false))
            }
            NativeContent::Picture { src, alt, pixmap } => {
                let part = self.picture_part(src.as_deref(), pixmap)?;
                let r_id = self.pkg.borrow_mut().rel(self.part, &part, REL::IMAGE, false);
                Some((picture_xml_rotated(id, &format!("Picture {id}"), alt, &r_id, rect, rotate), false))
            }
            NativeContent::Path { steps, fill, stroke, stroke_width } => {
                let stroke = stroke.filter(|_| stroke_width.is_finite() && *stroke_width > 0.0).map(|c| (c, *stroke_width));
                Some((freeform_xml(id, rect, rotate, steps, *fill, stroke), false))
            }
        }
    }
}

/// The layers of slide `index` that are terminal recordings, each drawn as the picture
/// PowerPoint gets of it, by the place of its layer: its last screen, or its panel. It's drawn
/// as it stands, the layer turning it as it turns anything else.
fn terminal_pictures(deck: &mut Deck, index: usize) -> Vec<(usize, NativeElement)> {
    let (specs, fonts, _) = deck.specs_fonts_and_files(index);
    let mut pictures = Vec::new();
    for (at, spec) in specs.iter().enumerate().filter(|(_, spec)| spec.terminal.is_some()) {
        let rect = Rect { x: 0.0, y: 0.0, ..spec.rect };
        let standing = LayerSpec { rect, rotate: 0.0, ..spec.clone() };
        let scale = (TERMINAL_EDGE / rect.w.max(rect.h).max(1.0)).min(TERMINAL_SCALE);
        let pixmap = render_layer(&standing, scale, fonts).pixmap;
        pictures.push((at, NativeElement { rect, content: NativeContent::Picture { src: None, alt: if spec.terminal.as_ref().is_some_and(|on| on.src.is_empty()) { LIVE_TERMINAL_ALT } else { TERMINAL_ALT }.to_string(), pixmap } }));
    }
    pictures
}

/// The links of the slide that aren't written, each once.
fn left_out_links(specs: &[LayerSpec]) -> Vec<String> {
    let mut left_out: Vec<String> = Vec::new();
    for element in specs.iter().flat_map(|spec| &spec.native) {
        let paragraphs: Vec<&NativeParagraph> = match &element.content {
            NativeContent::Text { paragraphs, .. } | NativeContent::Shape { paragraphs, .. } => paragraphs.iter().collect(),
            NativeContent::Table { rows, .. } => rows.iter().flatten().flatten().collect(),
            NativeContent::Picture { .. } | NativeContent::Path { .. } => Vec::new(),
        };
        for url in paragraphs.iter().flat_map(|p| &p.runs).filter_map(|run| run.link.as_deref()) {
            if link_target(url).is_none() && !left_out.iter().any(|seen| seen == url) {
                left_out.push(url.to_string());
            }
        }
    }
    left_out
}

fn has_fill(content: &NativeContent) -> bool {
    match content {
        NativeContent::Text { fill, .. } => fill.is_some(),
        NativeContent::Shape { fill, outline, outline_width, .. } => {
            fill.is_some() || (outline.is_some() && outline_width.is_finite() && *outline_width > 0.0)
        }
        NativeContent::Path { fill, stroke, stroke_width, .. } => {
            fill.is_some() || (stroke.is_some() && stroke_width.is_finite() && *stroke_width > 0.0)
        }
        NativeContent::Table { .. } | NativeContent::Picture { .. } => false,
    }
}

/// The layer and element that become the slide's title placeholder: the first text of the title
/// layer.
fn title_of(specs: &[LayerSpec]) -> Option<(usize, usize)> {
    let (layer, spec) = specs.iter().enumerate().find(|(_, spec)| spec.is_title)?;
    let element = spec.native.iter().position(|e| matches!(e.content, NativeContent::Text { .. }))?;
    Some((layer, element))
}

/// Every font name written to a part so far, sorted, bullet fonts apart. A theme reference (`+mj-lt`) isn't one.
fn typefaces_used(pkg: &Package) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for xml in pkg.xml_parts() {
        let mut pieces = xml.split("typeface=\"");
        let mut before = pieces.next().unwrap_or("");
        for chunk in pieces {
            // A bullet's font isn't one the person chose.
            let bullet = before.ends_with("<a:buFont ");
            before = chunk;
            if bullet {
                continue;
            }
            let name = chunk.split('"').next().unwrap_or("");
            if !name.is_empty() && !name.starts_with('+') {
                names.insert(unescape(name));
            }
        }
    }
    names
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// Carries the fonts the slides use: each family of the talk's or the app's own fonts that is
/// named in a part gets its styles stored once as `/ppt/fonts/fontN.fntdata` (Embedded OpenType)
/// and related from the presentation. A family that can't be embedded is left as the name it
/// has, and said so in `notices`.
fn embed_fonts(pkg: &mut Package, fonts: &Fonts, notices: &mut Vec<String>) -> Vec<EmbeddedFont> {
    let mut embedded: Vec<EmbeddedFont> = Vec::new();
    let mut stored = 0;
    for name in typefaces_used(pkg) {
        let faces = fonts.embeddable_faces(&name);
        let Some(family) = faces.first().map(|f| f.family.clone()) else {
            notices.push(format!("{name} can't be embedded, so PowerPoint may show another font"));
            continue;
        };
        if embedded.iter().any(|e| e.typeface == family) {
            continue;
        }
        let mut font = EmbeddedFont { typeface: family, ..EmbeddedFont::default() };
        let mut failed = false;
        for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
            let Some(face) = faces.iter().find(|f| f.bold == bold && f.italic == italic) else { continue };
            let Ok(eot) = eot_from_ttf(&face.data) else {
                failed = true;
                continue;
            };
            stored += 1;
            let part = format!("/ppt/fonts/font{stored}.fntdata");
            pkg.add_media(&part, eot);
            let r_id = Some(pkg.rel(PRES, &part, REL::FONT, false));
            match (bold, italic) {
                (false, false) => font.regular = r_id,
                (true, false) => font.bold = r_id,
                (false, true) => font.italic = r_id,
                (true, true) => font.bold_italic = r_id,
            }
        }
        if failed {
            notices.push(format!("{name} can't be embedded, so PowerPoint may show another font"));
        }
        if font.regular.is_some() {
            embedded.push(font);
        }
    }
    embedded
}

/// Writes the talk as a PowerPoint file with one slide per slide of the talk. A picture the talk
/// names as a PNG, JPEG or GIF is read through `files`, bounded as every read of a talk's files
/// is; anything else is stored as the PNG it was drawn from.
pub fn write_pptx(deck: &mut Deck, files: &dyn TalkFiles) -> Result<PptxOutput, String> {
    let pkg: SharedPackage = Rc::new(RefCell::new(Package::default()));
    let mut media = MediaStore::default();
    let theme = deck.theme();
    let title = deck.title();
    let geometry = Geometry::new(SLIDE_WIDTH_IN, SLIDE_HEIGHT_IN, MARGIN_X_IN);
    let slide_count = deck.slide_count();
    let notes: Vec<Option<String>> = (0..slide_count).map(|i| deck.notes(i).filter(|n| !n.trim().is_empty())).collect();
    let mut next_master_id = FIRST_MASTER_ID;

    let (master_id, master_r_id, layout_parts) = {
        let mut p = pkg.borrow_mut();
        p.rel("", PRES, REL::OFFICE_DOCUMENT, false);
        p.rel("", "/docProps/core.xml", REL::CORE_PROPS, false);
        p.rel("", "/docProps/app.xml", REL::EXT_PROPS, false);
        p.add_xml(THEME, CT::THEME, &theme_xml(title.as_deref().unwrap_or("Deck"), &theme));
        let master_r_id = p.rel(PRES, MASTER, REL::SLIDE_MASTER, false);
        let master_id = next_master_id;
        next_master_id += 1;
        p.rel(MASTER, THEME, REL::THEME, false);
        let mut layout_ids = Vec::new();
        let mut layout_parts: HashMap<&'static str, String> = HashMap::new();
        for (i, def) in layout_defs(&geometry).iter().enumerate() {
            let part = format!("/ppt/slideLayouts/slideLayout{}.xml", i + 1);
            p.add_xml(&part, CT::SLIDE_LAYOUT, &layout_xml(def, LANG));
            p.rel(&part, MASTER, REL::SLIDE_MASTER, false);
            layout_ids.push(LayoutId { id: next_master_id, r_id: p.rel(MASTER, &part, REL::SLIDE_LAYOUT, false) });
            next_master_id += 1;
            layout_parts.insert(def.key, part);
        }
        p.add_xml(MASTER, CT::SLIDE_MASTER, &master_xml(&geometry, LANG, &layout_ids));
        (master_id, master_r_id, layout_parts)
    };

    // The notes master, with a theme of its own as every master needs one, only exists if some
    // slide has notes.
    let notes_master_r_id = notes.iter().any(Option::is_some).then(|| {
        let mut p = pkg.borrow_mut();
        p.add_xml(NOTES_THEME, CT::THEME, &theme_xml("Notes", &theme));
        let mut ctx = TextCtx { lang: LANG.into(), link: Box::new(|_| String::new()) };
        p.add_xml(NOTES_MASTER, CT::NOTES_MASTER, &notes_master_xml(&mut ctx));
        p.rel(NOTES_MASTER, NOTES_THEME, REL::THEME, false);
        p.rel(PRES, NOTES_MASTER, REL::NOTES_MASTER, false)
    });

    let mut notices = Vec::new();
    let mut slide_refs = Vec::new();
    let mut notes_count = 0;
    for (index, note) in notes.iter().enumerate() {
        notices.extend(deck.notices_whole(index).into_iter().map(|n| format!("Slide {}: {n}", index + 1)));
        let timeline = deck.timeline(index);
        notices.extend(
            deck.varying_tags(index)
                .into_iter()
                .map(|tag| format!("Slide {}: {tag} changes as the talk plays, so PowerPoint shows it in its last state", index + 1)),
        );
        let screens = terminal_pictures(deck, index);
        let specs = deck.specs(index);
        notices.extend(left_out_links(specs).into_iter().map(|url| format!("Slide {}: the link {url:?} was left out: only http, https and mailto links are kept", index + 1)));
        let part = format!("/ppt/slides/slide{}.xml", index + 1);
        let title_at = title_of(specs);
        let layout = layout_parts[if title_at.is_some() { "titleOnly" } else { "blank" }].clone();
        pkg.borrow_mut().rel(&part, &layout, REL::SLIDE_LAYOUT, false);

        let link_pkg = pkg.clone();
        let link_part = part.clone();
        let link = Box::new(move |url: &str| link_pkg.borrow_mut().rel(&link_part, url, REL::HYPERLINK, true));
        let mut env = SlideEnv { pkg: &pkg, media: &mut media, files, part: &part, ctx: TextCtx { lang: LANG.into(), link } };

        let (mut shapes, mut effects, mut next_id) = (String::new(), Vec::new(), 2u32);
        for (layer_at, spec) in specs.iter().enumerate() {
            let pictured = screens.iter().filter(|(at, _)| *at == layer_at).map(|(_, element)| element);
            for (element_at, element) in spec.native.iter().chain(pictured).enumerate() {
                let id = next_id;
                next_id += 1;
                let is_title = title_at == Some((layer_at, element_at));
                let Some((xml, is_text)) = env.element_xml(id, spec, element, is_title) else { continue };
                shapes.push_str(&xml);
                if let Some(click) = spec.click {
                    effects.push(Effect { spid: id, is_text, has_fill: has_fill(&element.content), click, enter: spec.enter });
                }
            }
        }
        drop(env);

        let background = background_xml(theme.background, None);
        let xml = slide_xml_shown_or_not(&shapes, &background, &animation_timing_xml(&effects), &transition_xml(timeline.transition), deck.is_hidden(index));
        {
            let mut p = pkg.borrow_mut();
            p.add_xml(&part, CT::SLIDE, &xml);
            if let Some(text) = note {
                let notes_part = format!("/ppt/notesSlides/notesSlide{}.xml", index + 1);
                let mut ctx = TextCtx { lang: LANG.into(), link: Box::new(|_| String::new()) };
                p.add_xml(&notes_part, CT::NOTES_SLIDE, &notes_slide_xml(text, &mut ctx));
                p.rel(&notes_part, NOTES_MASTER, REL::NOTES_MASTER, false);
                p.rel(&notes_part, &part, REL::SLIDE, false);
                p.rel(&part, &notes_part, REL::NOTES_SLIDE, false);
                notes_count += 1;
            }
            slide_refs.push(SlideRef { id: FIRST_SLIDE_ID + index as u32, r_id: p.rel(PRES, &part, REL::SLIDE, false) });
        }
    }

    let mut p = pkg.borrow_mut();
    p.rel(PRES, THEME, REL::THEME, false);
    for (name, content_type, kind, xml) in [
        ("/ppt/presProps.xml", CT::PRES_PROPS, REL::PRES_PROPS, pres_props_xml()),
        ("/ppt/viewProps.xml", CT::VIEW_PROPS, REL::VIEW_PROPS, view_props_xml()),
        ("/ppt/tableStyles.xml", CT::TABLE_STYLES, REL::TABLE_STYLES, table_styles_xml()),
    ] {
        p.add_xml(name, content_type, &xml);
        p.rel(PRES, name, kind, false);
    }
    let embedded_fonts = embed_fonts(&mut p, deck.fonts_mut(), &mut notices);
    let presentation = Presentation {
        master_id,
        master_r_id: &master_r_id,
        notes_master_r_id: notes_master_r_id.as_deref(),
        slides: &slide_refs,
        width: SLIDE_WIDTH_IN,
        height: SLIDE_HEIGHT_IN,
        embedded_fonts: &embedded_fonts,
    };
    p.add_xml(PRES, CT::PRESENTATION, &presentation_xml(&presentation));
    p.add_xml("/docProps/core.xml", CT::CORE_PROPS, &core_xml(title.as_deref().unwrap_or(""), "", None));
    p.add_xml("/docProps/app.xml", CT::EXT_PROPS, &app_xml(slide_count, notes_count));
    let bytes = p.to_bytes().map_err(|e| e.to_string())?;
    Ok(PptxOutput { bytes, notices })
}

#[cfg(test)]
mod tests {
    use super::*;
    use slides_engine::{Enter, FolderFiles, PathStep};
    use toolkit::Color;

    /// A deck can't be given a component that draws a path from here, so the slide's element
    /// writer is handed one directly.
    #[test]
    fn a_slide_with_a_path_exports_a_freeform() {
        let steps = vec![PathStep::Move(0.0, 0.0), PathStep::Line(10.0, 10.0), PathStep::Close];
        let element = NativeElement {
            rect: Rect { x: 5.0, y: 5.0, w: 10.0, h: 10.0 },
            content: NativeContent::Path { steps, fill: Some(Color::rgb(255, 0, 0)), stroke: Some(Color::rgb(0, 0, 255)), stroke_width: 3.0 },
        };
        let layer = LayerSpec {
            source: None,
            rect: Rect { x: 100.0, y: 100.0, w: 50.0, h: 50.0 },
            rotate: 0.0,
            items: Vec::new().into(),
            click: None,
            enter: Enter::default(),
            native: vec![element.clone()],
            is_title: false,
            varies: false,
            video: None,
            model: None,
            terminal: None,
            button: None,
            component: None,
            id: None,
        };
        let pkg: SharedPackage = Rc::new(RefCell::new(Package::default()));
        let mut media = MediaStore::default();
        let files = FolderFiles::new(std::env::temp_dir());
        let mut env = SlideEnv {
            pkg: &pkg,
            media: &mut media,
            files: &files,
            part: "/ppt/slides/slide1.xml",
            ctx: TextCtx { lang: "en-US".into(), link: Box::new(|_| String::new()) },
        };
        let (xml, is_text) = env.element_xml(2, &layer, &element, false).expect("a path is written");
        assert!(xml.contains("<a:custGeom>") && !xml.contains("<p:pic>"), "{xml}");
        assert!(xml.contains(r#"<a:off x="666750" y="666750"/>"#), "{xml}");
        assert!(xml.contains(r#"<a:ln w="19050">"#), "{xml}");
        assert!(!is_text);
        assert!(has_fill(&element.content));
    }
}
