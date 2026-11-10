//! Bundled fonts, shaping in slide units, and drawing through a transform.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use cosmic_text::{
    fontdb, Align as TextAlign, Attrs, Buffer, Color as TextColor, Command, Family, FontSystem,
    Metrics, Shaping, Style, SwashCache, SwashContent, Weight,
};
use tiny_skia::{
    Color, ColorU8, FillRule, FilterQuality, Paint, Path, PathBuilder, Pixmap, PixmapMut,
    PixmapPaint, Rect, Shader, Transform,
};
use toolkit::Color as ToolkitColor;

const BUNDLED: [&[u8]; 8] = [
    include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../assets/fonts/Inter-Italic.ttf"),
    include_bytes!("../assets/fonts/Inter-BoldItalic.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Italic.ttf"),
    include_bytes!("../assets/fonts/NotoColorEmoji.ttf"),
];

/// Line height as a multiple of the font size.
const LINE_HEIGHT: f32 = 1.25;

/// The range of text sizes, in slide units. A talk sets the size, and outside this range
/// the font code misbehaves: size 0 means unscaled font units, and huge sizes overflow.
pub(crate) const MIN_TEXT_SIZE: f32 = 1.0;
pub(crate) const MAX_TEXT_SIZE: f32 = 512.0;

pub(crate) fn limited_size(size: f32) -> f32 {
    if size.is_finite() {
        size.clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE)
    } else {
        MIN_TEXT_SIZE
    }
}

/// Something the user should hear about, such as a character no font can draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice(pub String);

pub struct Fonts {
    pub system: FontSystem,
    pub cache: SwashCache,
    /// Fonts that ship with the app or the talk. Any other font came from this computer.
    known: HashSet<fontdb::ID>,
}

impl Fonts {
    fn build(with_system: bool) -> Fonts {
        let mut db = fontdb::Database::new();
        let mut known = HashSet::new();
        for data in BUNDLED {
            known.extend(db.load_font_source(fontdb::Source::Binary(Arc::new(data))));
        }
        db.set_sans_serif_family("Inter");
        db.set_monospace_family("JetBrains Mono");
        if with_system {
            db.load_system_fonts();
        }
        Fonts {
            system: FontSystem::new_with_locale_and_db("en-US".to_string(), db),
            cache: SwashCache::new(),
            known,
        }
    }

    /// Only the fonts in this crate, so a talk lays out the same on every machine.
    pub fn bundled_only() -> Fonts {
        Fonts::build(false)
    }

    /// The bundled fonts first, then the fonts of this computer for what they lack.
    pub fn with_system_fallback() -> Fonts {
        Fonts::build(true)
    }

    /// A font that travels inside the talk file.
    pub fn load_talk_font(&mut self, data: Vec<u8>) {
        let ids = self
            .system
            .db_mut()
            .load_font_source(fontdb::Source::Binary(Arc::new(data)));
        self.known.extend(ids);
    }
}

#[derive(Clone)]
pub struct TextRun {
    pub text: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: ToolkitColor,
    pub family: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

pub struct ShapedText {
    pub buffer: Arc<Buffer>,
    pub width: f32,
    pub height: f32,
    /// `(x, y, width, color)` of each underlined run on each line, in slide units.
    pub underlines: Vec<(f32, f32, f32, ToolkitColor)>,
    pub notices: Vec<Notice>,
}

fn run_attrs(run: &TextRun, index: usize) -> Attrs<'_> {
    let size = limited_size(run.size);
    Attrs::new()
        .family(Family::Name(&run.family))
        .weight(if run.bold { Weight::BOLD } else { Weight::NORMAL })
        .style(if run.italic { Style::Italic } else { Style::Normal })
        .color(TextColor::rgba(run.color.r, run.color.g, run.color.b, run.color.a))
        .metrics(Metrics::new(size, size * LINE_HEIGHT))
        .metadata(index)
}

/// Shapes `runs` in slide units. With a width the text wraps at it.
pub fn shape_text(fonts: &mut Fonts, runs: &[TextRun], width: Option<f32>, align: Align) -> ShapedText {
    let biggest = runs.iter().map(|r| limited_size(r.size)).fold(MIN_TEXT_SIZE, f32::max);
    let mut buffer = Buffer::new(&mut fonts.system, Metrics::new(biggest, biggest * LINE_HEIGHT));
    buffer.set_size(width, None);
    let alignment = match align {
        Align::Left => TextAlign::Left,
        Align::Center => TextAlign::Center,
        Align::Right => TextAlign::Right,
    };
    buffer.set_rich_text(
        runs.iter().enumerate().map(|(i, run)| (run.text.as_str(), run_attrs(run, i))),
        &Attrs::new().family(Family::Name("Inter")),
        Shaping::Advanced,
        Some(alignment),
    );
    buffer.shape_until_scroll(&mut fonts.system, false);

    let mut shaped_width = 0.0_f32;
    let mut height = 0.0_f32;
    let mut underlines = Vec::new();
    let mut missing = BTreeSet::new();
    let mut borrowed = BTreeSet::new();
    for line in buffer.layout_runs() {
        shaped_width = shaped_width.max(line.line_w);
        height = height.max(line.line_top + line.line_height);
        // Glyphs of one run are contiguous on a line, so one span per run index.
        let mut spans: Vec<(usize, f32, f32)> = Vec::new();
        for glyph in line.glyphs {
            let text = line.text.get(glyph.start..glyph.end).unwrap_or("");
            if glyph.glyph_id == 0 {
                missing.extend(text.chars().filter(|c| !c.is_whitespace()));
            } else if !fonts.known.contains(&glyph.font_id) {
                borrowed.extend(text.chars().filter(|c| !c.is_whitespace()));
            }
            if runs.get(glyph.metadata).is_some_and(|r| r.underline) {
                match spans.last_mut() {
                    Some(span) if span.0 == glyph.metadata => span.2 = glyph.x + glyph.w,
                    _ => spans.push((glyph.metadata, glyph.x, glyph.x + glyph.w)),
                }
            }
        }
        for (index, from, to) in spans {
            let run = &runs[index];
            underlines.push((from, line.line_y + limited_size(run.size) * 0.12, to - from, run.color));
        }
    }

    let mut notices = Vec::new();
    for c in missing {
        notices.push(Notice(format!("No font has the character {c}")));
    }
    for c in borrowed {
        notices.push(Notice(format!("Uses a font on this computer for {c}")));
    }
    ShapedText { buffer: Arc::new(buffer), width: shaped_width, height, underlines, notices }
}

fn outline_path(commands: &[Command]) -> Option<Path> {
    let mut b = PathBuilder::new();
    for command in commands {
        match *command {
            Command::MoveTo(p) => b.move_to(p.x, -p.y),
            Command::LineTo(p) => b.line_to(p.x, -p.y),
            Command::QuadTo(c, p) => b.quad_to(c.x, -c.y, p.x, -p.y),
            Command::CurveTo(c1, c2, p) => b.cubic_to(c1.x, -c1.y, c2.x, -c2.y, p.x, -p.y),
            Command::Close => b.close(),
        }
    }
    b.finish()
}

fn solid(r: u8, g: u8, b: u8, a: u8) -> Paint<'static> {
    Paint {
        shader: Shader::SolidColor(Color::from_rgba8(r, g, b, a)),
        anti_alias: true,
        ..Paint::default()
    }
}

/// Draws shaped text with slide units mapped through `transform`, so outlines stay
/// sharp at any zoom or rotation. Glyphs with no outline (colour emoji) are drawn
/// from their bitmap, placed and scaled by the same transform.
pub fn draw_text(target: &mut PixmapMut, text: &ShapedText, fonts: &mut Fonts, transform: Transform) {
    for run in text.buffer.layout_runs() {
        for glyph in run.glyphs.iter() {
            let physical = glyph.physical((0.0, 0.0), 1.0);
            let at = transform.pre_translate(glyph.x, run.line_y + physical.y as f32);
            let path = fonts
                .cache
                .get_outline_commands(&mut fonts.system, physical.cache_key)
                .filter(|commands| !commands.is_empty())
                .and_then(outline_path);
            if let Some(path) = path {
                let c = glyph.color_opt.unwrap_or(TextColor::rgb(0, 0, 0));
                target.fill_path(&path, &solid(c.r(), c.g(), c.b(), c.a()), FillRule::Winding, at, None);
                continue;
            }
            let Some(image) = fonts.cache.get_image(&mut fonts.system, physical.cache_key) else {
                continue;
            };
            if image.content != SwashContent::Color || image.placement.width == 0 {
                continue;
            }
            let Some(mut bitmap) = Pixmap::new(image.placement.width, image.placement.height) else {
                continue;
            };
            for (pixel, rgba) in bitmap.pixels_mut().iter_mut().zip(image.data.chunks_exact(4)) {
                *pixel = ColorU8::from_rgba(rgba[0], rgba[1], rgba[2], rgba[3]).premultiply();
            }
            let paint = PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
            let place = at.pre_translate(image.placement.left as f32, -image.placement.top as f32);
            target.draw_pixmap(0, 0, bitmap.as_ref(), &paint, place, None);
        }
    }
    let thickness = (text.buffer.metrics().font_size / 16.0).max(1.0);
    for &(x, y, width, color) in &text.underlines {
        if let Some(rect) = Rect::from_xywh(x, y, width, thickness) {
            target.fill_rect(rect, &solid(color.r, color.g, color.b, color.a), transform, None);
        }
    }
}
