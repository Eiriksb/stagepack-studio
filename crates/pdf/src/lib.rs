//! Writes a talk as a PDF: one 960 by 540 pt page per slide, with every layer drawn
//! as it is after its last click. Text stays text, in subset fonts.

use std::collections::HashMap;

use krilla::color::rgb;
use krilla::geom::{Path, PathBuilder, Point, Size, Transform};
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, Stroke};
use krilla::surface::Surface;
use krilla::text::{Font, GlyphId, KrillaGlyph};
use krilla::image::Image;
use krilla::Document;
use cosmic_text::{fontdb, LayoutGlyph};
use slides_engine::{Deck, DisplayItem, Fonts, LayerSpec, Rect, ShapedText};
use tiny_skia::PathSegment;
use toolkit::Color;

/// The page size in points. Slides are 1920 by 1080 units, so a unit is half a point.
const PAGE_WIDTH: f32 = 960.0;
const PAGE_HEIGHT: f32 = 540.0;
const POINTS_PER_UNIT: f32 = 0.5;

/// Krilla fonts, made once per face so a file holds one copy of each. `None` marks a face
/// that couldn't be read.
type FontCache = HashMap<fontdb::ID, Option<Font>>;

/// Writes the whole talk. What can't be drawn, such as a glyph from a missing font, is left
/// out; only a failure of the PDF writer is an error.
pub fn write_pdf(deck: &mut Deck) -> Result<Vec<u8>, String> {
    let mut document = Document::new();
    if let Some(title) = deck.title() {
        document.set_metadata(krilla::metadata::Metadata::new().title(title));
    }
    let mut cache = FontCache::new();
    for index in 0..deck.slide_count() {
        let settings = PageSettings::from_wh(PAGE_WIDTH, PAGE_HEIGHT).ok_or("the page size is invalid")?;
        let mut page = document.start_page_with(settings);
        let mut surface = page.surface();
        surface.push_transform(&Transform::from_scale(POINTS_PER_UNIT, POINTS_PER_UNIT));
        let (specs, fonts) = deck.specs_and_fonts(index);
        for spec in specs {
            draw_layer(&mut surface, spec, fonts, &mut cache);
        }
        surface.pop();
        surface.finish();
        page.finish();
    }
    document.finish().map_err(|e| format!("{e:?}"))
}

/// Places the layer the way `render_layer` does: turned around the centre of its rect.
fn draw_layer(surface: &mut Surface, spec: &LayerSpec, fonts: &mut Fonts, cache: &mut FontCache) {
    let Rect { x, y, w, h } = spec.rect;
    if ![x, y, w, h, spec.rotate].into_iter().all(f32::is_finite) {
        return;
    }
    surface.push_transform(&Transform::from_translate(x, y));
    surface.push_transform(&Transform::from_rotate_at(spec.rotate, w / 2.0, h / 2.0));
    for item in &spec.items {
        match item {
            DisplayItem::Fill { path, color } => {
                if let Some(path) = convert_path(path) {
                    surface.set_stroke(None);
                    surface.set_fill(Some(fill(*color)));
                    surface.draw_path(&path);
                }
            }
            DisplayItem::Stroke { path, color, width } => {
                if let Some(path) = convert_path(path) {
                    surface.set_fill(None);
                    surface.set_stroke(Some(stroke(*color, *width)));
                    surface.draw_path(&path);
                }
            }
            DisplayItem::Text { text, x, y } => draw_text(surface, text, *x, *y, fonts, cache),
            DisplayItem::Picture { pixmap, rect } => draw_picture(surface, pixmap, rect),
        }
    }
    surface.pop();
    surface.pop();
}

fn opacity(alpha: u8) -> NormalizedF32 {
    NormalizedF32::new(f32::from(alpha) / 255.0).unwrap_or(NormalizedF32::ONE)
}

fn fill(color: Color) -> Fill {
    Fill { paint: rgb::Color::new(color.r, color.g, color.b).into(), opacity: opacity(color.a), rule: Default::default() }
}

fn stroke(color: Color, width: f32) -> Stroke {
    Stroke {
        paint: rgb::Color::new(color.r, color.g, color.b).into(),
        width: if width.is_finite() { width.max(0.0) } else { 0.0 },
        opacity: opacity(color.a),
        ..Stroke::default()
    }
}

/// The same outline as a krilla path. `None` if the path has nothing drawable in it.
fn convert_path(path: &tiny_skia::Path) -> Option<Path> {
    let mut b = PathBuilder::new();
    for segment in path.segments() {
        match segment {
            PathSegment::MoveTo(p) => b.move_to(p.x, p.y),
            PathSegment::LineTo(p) => b.line_to(p.x, p.y),
            PathSegment::QuadTo(c, p) => b.quad_to(c.x, c.y, p.x, p.y),
            PathSegment::CubicTo(c1, c2, p) => b.cubic_to(c1.x, c1.y, c2.x, c2.y, p.x, p.y),
            PathSegment::Close => b.close(),
        }
    }
    b.finish()
}

fn draw_picture(surface: &mut Surface, pixmap: &tiny_skia::Pixmap, rect: &Rect) {
    let Ok(png) = pixmap.encode_png() else { return };
    let Ok(image) = Image::from_png(png.into(), true) else { return };
    let Some(size) = Size::from_wh(rect.w, rect.h) else { return };
    surface.push_transform(&Transform::from_translate(rect.x, rect.y));
    surface.draw_image(image, size);
    surface.pop();
}

fn font_for(id: fontdb::ID, fonts: &mut Fonts, cache: &mut FontCache) -> Option<Font> {
    cache
        .entry(id)
        .or_insert_with(|| {
            let (data, index) = fonts.face_bytes(id)?;
            Font::new(data.as_ref().clone().into(), index)
        })
        .clone()
}

/// What makes neighbouring glyphs one `draw_glyphs` call.
#[derive(PartialEq)]
struct Style {
    font: fontdb::ID,
    size: u32,
    color: (u8, u8, u8, u8),
    baseline: u32,
}

fn draw_text(surface: &mut Surface, text: &ShapedText, x: f32, y: f32, fonts: &mut Fonts, cache: &mut FontCache) {
    if !x.is_finite() || !y.is_finite() {
        return;
    }
    surface.push_transform(&Transform::from_translate(x, y));
    for line in text.buffer.layout_runs() {
        let mut start = 0;
        while start < line.glyphs.len() {
            let style = |i: usize| {
                let g = &line.glyphs[i];
                let c = g.color_opt.map_or((0, 0, 0, 255), |c| (c.r(), c.g(), c.b(), c.a()));
                Style { font: g.font_id, size: g.font_size.to_bits(), color: c, baseline: (g.y - g.y_offset * g.font_size).to_bits() }
            };
            let first = style(start);
            let mut end = start + 1;
            while end < line.glyphs.len() && style(end) == first {
                end += 1;
            }
            draw_glyph_group(surface, &line.glyphs[start..end], line.text, line.line_y, &first, fonts, cache);
            start = end;
        }
    }
    let size = text.buffer.metrics().font_size;
    let thickness = (size / 16.0).max(1.0);
    for &(ux, uy, width, color) in &text.underlines {
        let mut b = PathBuilder::new();
        let middle = uy + thickness / 2.0;
        b.move_to(ux, middle);
        b.line_to(ux + width, middle);
        if let Some(line) = b.finish() {
            surface.set_fill(None);
            surface.set_stroke(Some(stroke(color, thickness)));
            surface.draw_path(&line);
        }
    }
    surface.pop();
}

fn draw_glyph_group(
    surface: &mut Surface,
    glyphs: &[LayoutGlyph],
    line_text: &str,
    line_y: f32,
    style: &Style,
    fonts: &mut Fonts,
    cache: &mut FontCache,
) {
    let size = f32::from_bits(style.size);
    let Some(font) = font_for(style.font, fonts, cache) else { return };
    if !(size.is_finite() && size > 0.0) {
        return;
    }
    // Each glyph's advance is the distance to the next one, so the layout's own positions
    // (kerning, wrapping, alignment) are kept exactly.
    let krilla_glyphs: Vec<KrillaGlyph> = glyphs
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let here = g.x + g.x_offset * g.font_size;
            let next = glyphs.get(i + 1).map_or(here + g.w, |n| n.x + n.x_offset * n.font_size);
            let range = if line_text.get(g.start..g.end).is_some() { g.start..g.end } else { 0..0 };
            KrillaGlyph::new(GlyphId::new(u32::from(g.glyph_id)), (next - here) / size, 0.0, 0.0, 0.0, range, None)
        })
        .collect();
    let first = &glyphs[0];
    let origin = Point::from_xy(first.x + first.x_offset * first.font_size, line_y + f32::from_bits(style.baseline));
    let (r, g, b, a) = style.color;
    surface.set_stroke(None);
    surface.set_fill(Some(fill(Color { r, g, b, a })));
    surface.draw_glyphs(origin, &krilla_glyphs, font, line_text, size, false);
}
