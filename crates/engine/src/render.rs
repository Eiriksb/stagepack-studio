//! Draws a `LayerSpec` into its own pixmap, and puts layers together.

use tiny_skia::{
    ColorU8, FillRule, FilterQuality, Paint, Pixmap, PixmapPaint, Point, Shader, Stroke, Transform,
};
use toolkit::Color;

use crate::{DisplayItem, Fonts, Layer, LayerSpec, draw_text};

fn paint(color: Color) -> Paint<'static> {
    Paint {
        shader: Shader::SolidColor(tiny_skia::Color::from_rgba8(color.r, color.g, color.b, color.a)),
        anti_alias: true,
        ..Paint::default()
    }
}

/// Draws one element. Rotation is baked in, so moving the layer later is a plain blit.
pub fn render_layer(spec: &LayerSpec, scale: f32, fonts: &mut Fonts) -> Layer {
    let (x, y) = (spec.rect.x * scale, spec.rect.y * scale);
    let (w, h) = (spec.rect.w * scale, spec.rect.h * scale);
    let place = Transform::from_translate(x, y).pre_concat(Transform::from_rotate_at(spec.rotate, w / 2.0, h / 2.0));
    let mut corners = [
        Point::from_xy(0.0, 0.0),
        Point::from_xy(w, 0.0),
        Point::from_xy(w, h),
        Point::from_xy(0.0, h),
    ];
    place.map_points(&mut corners);
    let min_x = corners.iter().map(|p| p.x).fold(f32::MAX, f32::min).floor();
    let min_y = corners.iter().map(|p| p.y).fold(f32::MAX, f32::min).floor();
    let max_x = corners.iter().map(|p| p.x).fold(f32::MIN, f32::max).ceil();
    let max_y = corners.iter().map(|p| p.y).fold(f32::MIN, f32::max).ceil();
    let mut pixmap = Pixmap::new((max_x - min_x) as u32 + 1, (max_y - min_y) as u32 + 1)
        .expect("layer size must be non-zero");

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
            DisplayItem::Text { text, x, y } => {
                draw_text(&mut target, text, fonts, at.pre_translate(*x, *y));
            }
            DisplayItem::Picture { pixmap: picture, rect } => {
                let fit = at
                    .pre_translate(rect.x, rect.y)
                    .pre_scale(rect.w / picture.width() as f32, rect.h / picture.height() as f32);
                let picture_paint = PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
                target.draw_pixmap(0, 0, (**picture).as_ref(), &picture_paint, fit, None);
            }
        }
    }
    Layer { pixmap, x: min_x as i32, y: min_y as i32 }
}

/// Draws the layers over `target`, first to last.
pub fn compose(target: &mut Pixmap, layers: &[Layer]) {
    for layer in layers {
        target.draw_pixmap(layer.x, layer.y, layer.pixmap.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    }
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
