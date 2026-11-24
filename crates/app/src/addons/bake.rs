//! The parts of a talk a Slidev export shows as pictures, drawn by the app: every tag but a box,
//! a shape, text and a picture, as an icon, a chart, a component, a video's still. Each is drawn
//! as the slide draws it, at twice its size, and listed in `baked/index.json` with its place.
//!
//! The add-on finds each by its slide, counted from 1, and its `n`: the tags of a slide that are
//! drawn here, counted from 0 in the order they're written, those inside others too.
//!
//! A part is drawn as the layer it's in, cut to its box, as the engine has no way to draw one
//! part of a layer alone. So what else of that layer lies under the box, the fill of a box it's
//! in or text running past it, is in its picture too, and a part beside it that changes on a
//! click makes it look as though it does. Slidev draws those again under it, which matches as
//! long as Slidev lays them out as the app does.

use addon_abi::{Bundle, Notice, NoticeKind};
use serde::Serialize;
use slides_engine::{Deck, LayerSpec, Moment, Pixmap, Rect, render_layer};
use slides_format::{Block, Tag};

/// Where the list of the parts drawn goes in the bundle.
pub const INDEX: &str = "baked/index.json";
/// How many times its size on the slide a part is drawn.
const SCALE: f32 = 2.0;
/// The most pixels a part's picture has: one larger is drawn smaller than twice its size.
const MAX_PIXELS: f32 = 32_000_000.0;
/// The tags the add-on writes as Slidev's own HTML, which aren't drawn here.
const DRAWN_BY_SLIDEV: [&str; 4] = ["Box", "Shape", "Text", "Picture"];
/// The most clicks of a slide a part is drawn at, one picture each.
const MOST_STEPS: u32 = 99;
/// How far into a slide a part that changes on its own is looked at again, to tell that it does.
const LATER: f32 = 0.75;
/// The most the pictures of parts that change on clicks may come to, a picture for each click,
/// across the talk. Past it, a part is one picture, of how it ends up.
pub const MOST_STEPPED_BYTES: u64 = 96 * 1024 * 1024;
/// The most pictures of parts that change on clicks, across the talk.
pub const MOST_STEPPED_FILES: usize = 2_000;

/// One part drawn, as `baked/index.json` lists it: where it is on the slide, in slide units
/// before anything it's in is turned, the clicks it changes on, and whether it changes on its
/// own, so that one picture can't show it.
#[derive(Clone, Debug, Serialize)]
pub struct Baked {
    pub slide: usize,
    pub n: usize,
    pub tag: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub rotate: f32,
    pub steps: Vec<u32>,
    pub animated: bool,
}

/// Whether the tag `name` is drawn here for an export.
pub fn is_baked(name: &str) -> bool {
    !DRAWN_BY_SLIDEV.contains(&name)
}

/// The tags of `blocks` drawn here, at any depth, in the order they're written.
fn baked_tags<'a>(blocks: &'a [Block], found: &mut Vec<&'a Tag>) {
    for block in blocks {
        match block {
            Block::Tag(tag) => {
                if is_baked(&tag.name) {
                    found.push(tag);
                }
                baked_tags(&tag.children, found);
            }
            Block::List { items, .. } => items.iter().for_each(|item| baked_tags(item, found)),
            _ => {}
        }
    }
}

/// The pictures of every part of `deck`'s talk drawn for a Slidev export, as
/// `baked/<slide>/<n>.png`, or `<n>@<step>.png` for one that changes on clicks, and
/// `baked/index.json`. A part that isn't drawn on its slide, as one in a slide that can't be
/// read, is left out, and the add-on says so.
pub fn bake(deck: &mut Deck) -> Bundle {
    bake_within(deck, MOST_STEPPED_BYTES, MOST_STEPPED_FILES).0
}

/// [`bake`], with the pictures of parts that change on clicks coming to no more than
/// `most_bytes` and `most_files`, and a notice for each part shown as one picture instead.
pub fn bake_within(deck: &mut Deck, most_bytes: u64, most_files: usize) -> (Bundle, Vec<Notice>) {
    let mut bundle = Bundle::new();
    let mut notices = Vec::new();
    let (mut stepped_bytes, mut stepped_files) = (0u64, 0usize);
    let mut index = Vec::new();
    for slide in 0..deck.slide_count() {
        if deck.talk().slides[slide].error.is_some() {
            continue;
        }
        let mut tags = Vec::new();
        baked_tags(&deck.talk().slides[slide].blocks, &mut tags);
        let tags: Vec<(usize, Tag)> = tags.into_iter().cloned().enumerate().collect();
        if tags.is_empty() {
            continue;
        }
        let layout = deck.text_layout(slide);
        let timeline = deck.timeline(slide);
        let varying = deck.varying_tags(slide);
        for (n, tag) in tags {
            let Some(element) = layout.element(&tag.range) else { continue };
            // The layout gives a part inside a turned box where it ends up once the box is
            // turned; the add-on wants it where it is before, as the box turns it in Slidev too.
            let rect = if element.placed || element.rotate == 0.0 {
                element.rect
            } else {
                let (specs, _, _) = deck.specs_fonts_and_files(slide);
                match holding_layer(specs, &tag) {
                    Some(holder) => unturned(element.rect, holder.rect, element.rotate),
                    None => element.rect,
                }
            };
            if !(rect.w > 0.0 && rect.h > 0.0) {
                continue;
            }
            let scale = SCALE.min((MAX_PIXELS / (rect.w * rect.h)).sqrt());
            let picture_at = |deck: &mut Deck, moment: Option<Moment>| -> Option<Vec<u8>> { picture(deck, slide, &tag, rect, scale, moment) };
            let Some(whole) = picture_at(deck, None) else { continue };
            let changes = varying.contains(&tag.name);
            let animated = changes && timeline.animated && {
                let start = picture_at(deck, Some(Moment { step: 0, time: 0.0 }));
                start != picture_at(deck, Some(Moment { step: 0, time: LATER }))
            };
            let mut steps = Vec::new();
            if changes && !animated && timeline.steps > 0 {
                let mut pictures: Vec<(u32, Vec<u8>)> = Vec::new();
                for step in 0..=timeline.steps.min(MOST_STEPS) {
                    let Some(drawn) = picture_at(deck, Some(Moment { step, time: 0.0 })) else { continue };
                    if pictures.last().is_none_or(|(_, last)| *last != drawn) {
                        pictures.push((step, drawn));
                    }
                }
                let bytes = pictures.iter().map(|(_, png)| png.len() as u64).sum::<u64>();
                let fits = stepped_bytes + bytes <= most_bytes && stepped_files + pictures.len() <= most_files;
                if pictures.len() > 1 && !fits {
                    notices.push(Notice {
                        slide: Some(slide + 1),
                        kind: NoticeKind::Frozen,
                        text: format!(
                            "The {} changes on clicks, and is one picture of how it ends up, as a picture for each click would make the export too large.",
                            slides_components::shortened(&tag.name, 40)
                        ),
                    });
                } else if pictures.len() > 1 {
                    stepped_bytes += bytes;
                    stepped_files += pictures.len();
                    for (step, png) in pictures {
                        bundle.push((format!("baked/{}/{n}@{step}.png", slide + 1), png));
                        steps.push(step);
                    }
                }
            }
            if steps.is_empty() {
                bundle.push((format!("baked/{}/{n}.png", slide + 1), whole));
            }
            // A part with a place of its own is turned on its own; one inside another is
            // turned with what it's in, which the add-on turns.
            let rotate = if element.placed { element.rotate } else { 0.0 };
            index.push(Baked { slide: slide + 1, n, tag: tag.name.clone(), x: rect.x, y: rect.y, w: rect.w, h: rect.h, rotate, steps, animated });
        }
    }
    bundle.push((INDEX.to_string(), serde_json::to_vec(&index).expect("the list of parts drawn is always JSON")));
    (bundle, notices)
}

/// The picture of the part `tag` of slide `slide`, which is drawn at `rect` on it, as a PNG
/// `scale` times its size: at `moment`, or in the slide's whole state. It's the layer it's in,
/// drawn alone and standing up, cut to its box.
fn picture(deck: &mut Deck, slide: usize, tag: &Tag, rect: Rect, scale: f32, moment: Option<Moment>) -> Option<Vec<u8>> {
    let (specs, fonts) = match moment {
        Some(moment) => deck.specs_and_fonts_at(slide, moment),
        None => {
            let (specs, fonts, _) = deck.specs_fonts_and_files(slide);
            (specs, fonts)
        }
    };
    let holder = holding_layer(specs, tag)?;
    let standing = LayerSpec { rotate: 0.0, ..holder.clone() };
    let layer = render_layer(&standing, scale, fonts);
    let (width, height) = ((rect.w * scale).round().max(1.0) as u32, (rect.h * scale).round().max(1.0) as u32);
    let left = (rect.x * scale).round() as i64 - i64::from(layer.x);
    let top = (rect.y * scale).round() as i64 - i64::from(layer.y);
    cut(&layer.pixmap, left, top, width, height)?.encode_png().ok()
}

/// `rect`, turned by `degrees` about the centre of `around`, as it was before it was turned.
fn unturned(rect: Rect, around: Rect, degrees: f32) -> Rect {
    let (sin, cos) = (-f64::from(degrees)).to_radians().sin_cos();
    let pivot = (f64::from(around.x) + f64::from(around.w) / 2.0, f64::from(around.y) + f64::from(around.h) / 2.0);
    let centre = (f64::from(rect.x) + f64::from(rect.w) / 2.0 - pivot.0, f64::from(rect.y) + f64::from(rect.h) / 2.0 - pivot.1);
    let back = (pivot.0 + centre.0 * cos - centre.1 * sin, pivot.1 + centre.0 * sin + centre.1 * cos);
    Rect { x: (back.0 - f64::from(rect.w) / 2.0) as f32, y: (back.1 - f64::from(rect.h) / 2.0) as f32, ..rect }
}

/// The layer `tag` is drawn in: the last of the slide's, but its background, whose source holds it.
fn holding_layer<'a>(specs: &'a [LayerSpec], tag: &Tag) -> Option<&'a LayerSpec> {
    specs
        .iter()
        .skip(1)
        .rev()
        .find(|spec| spec.source.as_ref().is_some_and(|source| source.start <= tag.range.start && tag.range.end <= source.end))
}

/// The `width` by `height` part of `from` whose top left corner is at (`left`, `top`) in it,
/// clear where it reaches past `from`.
fn cut(from: &Pixmap, left: i64, top: i64, width: u32, height: u32) -> Option<Pixmap> {
    let mut to = Pixmap::new(width, height)?;
    let (from_width, from_height) = (i64::from(from.width()), i64::from(from.height()));
    let row_bytes = width as usize * 4;
    let (source, target) = (from.data(), to.data_mut());
    for row in 0..i64::from(height) {
        let y = top + row;
        if !(0..from_height).contains(&y) {
            continue;
        }
        let (start, end) = (left.max(0), (left + i64::from(width)).min(from_width));
        if start >= end {
            continue;
        }
        let from_at = ((y * from_width + start) * 4) as usize;
        let to_at = row as usize * row_bytes + ((start - left) * 4) as usize;
        let len = ((end - start) * 4) as usize;
        target[to_at..to_at + len].copy_from_slice(&source[from_at..from_at + len]);
    }
    Some(to)
}
