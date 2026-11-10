//! A whole `.pptx` from a laid-out talk: the part wiring of `writePptx` in `slidev-pptx-export`,
//! over the writers of the other modules. This file owns part names, relationships and ids; the
//! XML of each part comes from the module for it.
//!
//! The order of the package calls is the original's, so the relationship ids come out the same:
//! the theme, the layouts, the master, the notes master, each slide with its notes, the small
//! package parts, then the presentation itself.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use tiny_skia::Pixmap;
use slides_engine::{Deck, LayerSpec, NativeContent, NativeElement, Rect, TalkFiles};

use crate::master::{Geometry, LayoutId, layout_defs, layout_xml, master_xml};
use crate::notes::{notes_master_xml, notes_slide_xml};
use crate::package::{CT, Package, REL};
use crate::presentation::{
    Presentation, SlideRef, app_xml, core_xml, pres_props_xml, presentation_xml, view_props_xml,
};
use crate::shapes::{
    Ph, background_xml, graphic_frame_xml, picture_xml_rotated, placeholder_xml, shape_xml_rotated, slide_xml,
};
use crate::table::{TABLE_URI, table_styles_xml, table_xml};
use crate::text::{BodyOptions, TextCtx};
use crate::theme::theme_xml;
use crate::timing::{Effect, animation_timing_xml, transition_xml};

const PRES: &str = "/ppt/presentation.xml";
const MASTER: &str = "/ppt/slideMasters/slideMaster1.xml";
const THEME: &str = "/ppt/theme/theme1.xml";
const NOTES_THEME: &str = "/ppt/theme/theme2.xml";
const NOTES_MASTER: &str = "/ppt/notesMasters/notesMaster1.xml";
const LANG: &str = "en-US";

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
            NativeContent::Text { paragraphs, .. } if title => {
                let ph = Ph { kind: Some("title"), idx: None };
                Some((placeholder_xml(id, "Title 1", &ph, Some(rect), paragraphs, &mut self.ctx, &BodyOptions::default()), true))
            }
            NativeContent::Text { .. } | NativeContent::Shape { .. } => {
                Some((shape_xml_rotated(id, rect, rotate, &element.content, &mut self.ctx)?, true))
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
        }
    }
}

fn has_fill(content: &NativeContent) -> bool {
    match content {
        NativeContent::Text { fill, .. } => fill.is_some(),
        NativeContent::Shape { fill, outline, outline_width, .. } => {
            fill.is_some() || (outline.is_some() && outline_width.is_finite() && *outline_width > 0.0)
        }
        NativeContent::Table { .. } | NativeContent::Picture { .. } => false,
    }
}

/// The layer and element that become the slide's title placeholder: the first text of the title
/// layer. A turned title is a text box, as a placeholder can't be turned.
fn title_of(specs: &[LayerSpec]) -> Option<(usize, usize)> {
    let (layer, spec) = specs.iter().enumerate().find(|(_, spec)| spec.is_title && spec.rotate == 0.0)?;
    let element = spec.native.iter().position(|e| matches!(e.content, NativeContent::Text { .. }))?;
    Some((layer, element))
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
        notices.extend(deck.notices(index).into_iter().map(|n| format!("Slide {}: {n}", index + 1)));
        let timeline = deck.timeline(index);
        let specs = deck.specs(index);
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
            for (element_at, element) in spec.native.iter().enumerate() {
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
        let xml = slide_xml(&shapes, &background, &animation_timing_xml(&effects), &transition_xml(timeline.transition));
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
    let presentation = Presentation {
        master_id,
        master_r_id: &master_r_id,
        notes_master_r_id: notes_master_r_id.as_deref(),
        slides: &slide_refs,
        width: SLIDE_WIDTH_IN,
        height: SLIDE_HEIGHT_IN,
        embedded_fonts: &[],
    };
    p.add_xml(PRES, CT::PRESENTATION, &presentation_xml(&presentation));
    p.add_xml("/docProps/core.xml", CT::CORE_PROPS, &core_xml(title.as_deref().unwrap_or(""), "", None));
    p.add_xml("/docProps/app.xml", CT::EXT_PROPS, &app_xml(slide_count, notes_count));
    let bytes = p.to_bytes().map_err(|e| e.to_string())?;
    Ok(PptxOutput { bytes, notices })
}
