//! Draws a `LayerSpec` into its own pixmap, and puts layers together.

use std::sync::Arc;

use tiny_skia::{
    ColorU8, FillRule, FilterQuality, Paint, Pixmap, PixmapMut, PixmapPaint, Point, Shader, Stroke, Transform,
};
use toolkit::Color;

use crate::{DisplayItem, Enter, Fonts, Layer, LayerSpec, Rect, ShapedText, draw_text};

fn paint(color: Color) -> Paint<'static> {
    Paint {
        shader: Shader::SolidColor(tiny_skia::Color::from_rgba8(color.r, color.g, color.b, color.a)),
        anti_alias: true,
        ..Paint::default()
    }
}

/// The longest side a layer's pixmap can have. What doesn't fit isn't drawn.
const MAX_SIDE: u32 = 16384;

/// The most pixels a layer's pixmap can have, as two long sides could otherwise ask for a
/// gibibyte. What doesn't fit isn't drawn.
const MAX_PIXELS: u64 = 64_000_000;

/// How far from the slide a layer can sit. `compose` adds a layer's size to its position,
/// and that sum has to fit in an `i32`.
const MAX_OFFSET: f32 = 1_000_000_000.0;

/// A device pixel position, held to where a layer can sit.
pub(crate) fn device_offset(at: f32) -> i32 {
    at.clamp(-MAX_OFFSET, MAX_OFFSET) as i32
}

/// `(left, top, right, bottom)` of the box around the rect and everything drawn, before
/// rotation, with the rect's top-left corner as the origin.
fn extent(spec: &LayerSpec) -> (f32, f32, f32, f32) {
    let (mut left, mut top, mut right, mut bottom) = (0.0_f32, 0.0_f32, spec.rect.w, spec.rect.h);
    for item in &spec.items {
        let (l, t, r, b) = match item {
            DisplayItem::Fill { path, .. } => {
                let b = path.bounds();
                (b.left(), b.top(), b.right(), b.bottom())
            }
            DisplayItem::Stroke { path, width, .. } => {
                let b = path.bounds();
                let half = width / 2.0;
                (b.left() - half, b.top() - half, b.right() + half, b.bottom() + half)
            }
            DisplayItem::Text { text, x, y } => (*x, *y, x + text.width, y + text.height),
            DisplayItem::Picture { rect, .. } => (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h),
        };
        left = left.min(l);
        top = top.min(t);
        right = right.max(r);
        bottom = bottom.max(b);
    }
    (left, top, right, bottom)
}

/// What a layer whose place makes no sense draws: nothing.
fn blank_layer() -> Layer {
    Layer { pixmap: Arc::new(Pixmap::new(1, 1).expect("1 by 1 is a valid size")), x: 0, y: 0 }
}

fn pixmap_side(from: f32, to: f32) -> u32 {
    ((to - from) as u32).saturating_add(1).min(MAX_SIDE)
}

/// A size with no more than `MAX_PIXELS`, in the same proportions as the one asked for.
/// Both sides are at least a few thousand by the time this has anything to do.
fn within_max_pixels(width: u32, height: u32) -> (u32, u32) {
    let pixels = u64::from(width) * u64::from(height);
    if pixels <= MAX_PIXELS {
        return (width, height);
    }
    let shrink = (MAX_PIXELS as f64 / pixels as f64).sqrt();
    ((f64::from(width) * shrink) as u32, (f64::from(height) * shrink) as u32)
}

/// Draws one element. Rotation is baked in, so moving the layer later is a plain blit.
/// The pixmap holds everything the items draw, even outside `spec.rect`, though the layer
/// still turns around the centre of `spec.rect`. A talk decides these numbers, so no
/// rect, however absurd, may panic here.
pub fn render_layer(spec: &LayerSpec, scale: f32, fonts: &mut Fonts) -> Layer {
    draw_layer(spec, scale, |target, text, at| draw_text(target, text, fonts, at))
}

/// One of a video's pictures, `width` by `height` in straight BGRA, drawn as the layer of a
/// video placed at `rect` and turned by `rotate` would show it, at `scale`: for a turned video,
/// which the window can't turn. `None` if the picture isn't that size.
/// `from` is the pixmap the last picture was read into, filled again if it's this one's size.
pub fn frame_layer(width: u32, height: u32, bgra: &[u8], rect: Rect, rotate: f32, scale: f32, from: &mut Option<Pixmap>) -> Option<Layer> {
    turned_picture_layer(width, height, bgra, true, rect, rotate, scale, from)
}

/// A 3D model's picture, `width` by `height` in straight BGRA, clear where the model isn't,
/// drawn as [`frame_layer`] draws a video's: for a model in a turned box.
pub fn picture_layer(width: u32, height: u32, bgra: &[u8], rect: Rect, rotate: f32, scale: f32, from: &mut Option<Pixmap>) -> Option<Layer> {
    turned_picture_layer(width, height, bgra, false, rect, rotate, scale, from)
}

/// A picture drawn turned with its box, `opaque` as a video's is, or with the alpha it has.
#[allow(clippy::too_many_arguments)]
fn turned_picture_layer(width: u32, height: u32, bgra: &[u8], opaque: bool, rect: Rect, rotate: f32, scale: f32, from: &mut Option<Pixmap>) -> Option<Layer> {
    if bgra.len() as u64 != u64::from(width) * u64::from(height) * 4 {
        return None;
    }
    let mut picture = match from.take() {
        Some(kept) if (kept.width(), kept.height()) == (width, height) => kept,
        _ => Pixmap::new(width, height)?,
    };
    for (pixel, bgra) in picture.pixels_mut().iter_mut().zip(bgra.as_chunks::<4>().0) {
        let alpha = if opaque { 255 } else { bgra[3] };
        *pixel = ColorU8::from_rgba(bgra[2], bgra[1], bgra[0], alpha).premultiply();
    }
    let picture = Arc::new(picture);
    let item = DisplayItem::Picture { pixmap: picture.clone(), src: None, rect: Rect { x: 0.0, y: 0.0, w: rect.w, h: rect.h } };
    let spec = LayerSpec {
        source: None,
        rect,
        rotate,
        items: vec![item],
        click: None,
        enter: Enter::Appear,
        native: Vec::new(),
        is_title: false,
        varies: false,
        video: None,
        model: None,
    };
    let layer = draw_layer(&spec, scale, |_, _, _| {});
    drop(spec);
    *from = Arc::into_inner(picture);
    Some(layer)
}

/// Draws `spec` at `scale`, its text with `text`.
fn draw_layer(spec: &LayerSpec, scale: f32, mut text: impl FnMut(&mut PixmapMut, &ShapedText, Transform)) -> Layer {
    let (x, y) = (spec.rect.x * scale, spec.rect.y * scale);
    let (w, h) = (spec.rect.w * scale, spec.rect.h * scale);
    let place = Transform::from_translate(x, y).pre_concat(Transform::from_rotate_at(spec.rotate, w / 2.0, h / 2.0));
    let (left, top, right, bottom) = extent(spec);
    let mut corners = [
        Point::from_xy(left * scale, top * scale),
        Point::from_xy(right * scale, top * scale),
        Point::from_xy(right * scale, bottom * scale),
        Point::from_xy(left * scale, bottom * scale),
    ];
    place.map_points(&mut corners);
    if ![x, y, w, h].into_iter().chain(corners.iter().flat_map(|p| [p.x, p.y])).all(f32::is_finite) {
        return blank_layer();
    }
    let min_x = corners.iter().map(|p| p.x).fold(f32::MAX, f32::min).floor();
    let min_y = corners.iter().map(|p| p.y).fold(f32::MAX, f32::min).floor();
    let max_x = corners.iter().map(|p| p.x).fold(f32::MIN, f32::max).ceil();
    let max_y = corners.iter().map(|p| p.y).fold(f32::MIN, f32::max).ceil();
    let (width, height) = within_max_pixels(pixmap_side(min_x, max_x), pixmap_side(min_y, max_y));
    let Some(mut pixmap) = Pixmap::new(width, height) else {
        return blank_layer();
    };

    // Items are in slide units, so the item transform scales as well as places.
    let at = Transform::from_translate(-min_x, -min_y).pre_concat(place).pre_scale(scale, scale);
    let mut target = pixmap.as_mut();
    for item in &spec.items {
        match item {
            DisplayItem::Fill { path, color } => {
                target.fill_path(path, &paint(*color), FillRule::Winding, at, None);
            }
            DisplayItem::Stroke { path, color, width } => {
                let stroke = Stroke { width: *width, ..Stroke::default() };
                target.stroke_path(path, &paint(*color), &stroke, at, None);
            }
            DisplayItem::Text { text: shaped, x, y } => {
                text(&mut target, shaped, at.pre_translate(*x, *y));
            }
            DisplayItem::Picture { pixmap: picture, rect, .. } => {
                let fit = at
                    .pre_translate(rect.x, rect.y)
                    .pre_scale(rect.w / picture.width() as f32, rect.h / picture.height() as f32);
                let picture_paint = PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
                target.draw_pixmap(0, 0, (**picture).as_ref(), &picture_paint, fit, None);
            }
        }
    }
    Layer {
        pixmap: Arc::new(pixmap),
        x: device_offset(min_x),
        y: device_offset(min_y),
    }
}

/// Draws the layers over `target`, first to last.
pub fn compose(target: &mut Pixmap, layers: &[Layer]) {
    for layer in layers {
        target.draw_pixmap(layer.x, layer.y, Pixmap::as_ref(&layer.pixmap), &PixmapPaint::default(), Transform::identity(), None);
    }
}

/// A picture `width` by `height` already in straight BGRA, as the window's image type holds
/// pixels, or `None` if it isn't that size.
pub fn bgra_frame(width: u32, height: u32, bgra: Vec<u8>) -> Option<image::Frame> {
    Some(image::Frame::new(image::RgbaImage::from_raw(width, height, bgra)?))
}

/// Straight-alpha BGRA, the way the window's image type holds pixels.
pub fn to_bgra_frame(pixmap: &Pixmap) -> image::Frame {
    let mut bytes = Vec::with_capacity(pixmap.data().len());
    for pixel in pixmap.pixels() {
        let c: ColorU8 = pixel.demultiply();
        bytes.extend_from_slice(&[c.blue(), c.green(), c.red(), c.alpha()]);
    }
    image::Frame::new(image::RgbaImage::from_raw(pixmap.width(), pixmap.height(), bytes).expect("buffer matches size"))
}
