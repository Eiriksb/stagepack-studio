//! Reads a talk's pictures and draws icons, as pixmaps.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use image::{ImageReader, Limits};
use resvg::usvg;
use tiny_skia::{ColorU8, Pixmap, Transform};
use toolkit::Color;

use crate::TalkFiles;

/// The longest edge a picture is kept at.
const MAX_EDGE: u32 = 4096;

// What a picture file can ask for while it is decoded. A file can claim any dimensions,
// so one that claims too much has to fail before anything is allocated for it.
const MAX_DECODED_SIDE: u32 = 16384;
const MAX_DECODED_BYTES: u64 = 256 * 1024 * 1024;

const CARBON_ICONS: &str = include_str!("../assets/icons/carbon.json");
/// The side of the square every Carbon icon is drawn in.
const ICON_VIEW_BOX: f32 = 32.0;

const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

/// The pictures read so far, by their path in the talk. One that couldn't be read isn't
/// kept, so it appears the next time its slide is laid out after the file is fixed.
#[derive(Default)]
pub struct PictureCache {
    pictures: HashMap<String, Arc<Pixmap>>,
}

impl PictureCache {
    pub(crate) fn get(&mut self, path: &str, files: &dyn TalkFiles) -> Option<Arc<Pixmap>> {
        if let Some(picture) = self.pictures.get(path) {
            return Some(picture.clone());
        }
        let bytes = files.read(path)?;
        let picture = Arc::new(decode(path, &bytes)?);
        self.pictures.insert(path.to_string(), picture.clone());
        Some(picture)
    }
}

fn decode(path: &str, bytes: &[u8]) -> Option<Pixmap> {
    // The file is untrusted and the decoders are large, so a bug in one of them shows as
    // a bad picture instead of taking the app down.
    std::panic::catch_unwind(|| if is_svg(path) { decode_svg(bytes) } else { decode_raster(bytes) }).ok().flatten()
}

fn is_svg(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
}

fn decode_raster(bytes: &[u8]) -> Option<Pixmap> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODED_SIDE);
    limits.max_image_height = Some(MAX_DECODED_SIDE);
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?;
    reader.limits(limits);
    let mut image = reader.decode().ok()?;
    // Shrunk before it becomes RGBA, which can be four times what the limits allowed.
    if image.width().max(image.height()) > MAX_EDGE {
        image = image.thumbnail(MAX_EDGE, MAX_EDGE);
    }
    let image = image.to_rgba8();
    let mut pixmap = Pixmap::new(image.width(), image.height())?;
    for (pixel, rgba) in pixmap.pixels_mut().iter_mut().zip(image.pixels()) {
        *pixel = ColorU8::from_rgba(rgba[0], rgba[1], rgba[2], rgba[3]).premultiply();
    }
    Some(pixmap)
}

fn decode_svg(bytes: &[u8]) -> Option<Pixmap> {
    // From text, because `Tree::from_data` would unpack a gzipped file of any size.
    draw_svg(std::str::from_utf8(bytes).ok()?, |size| (MAX_EDGE as f32 / size.width().max(size.height())).min(1.0))
}

/// Draws SVG markup at the scale `scale_for` picks from the size the markup declares.
fn draw_svg(markup: &str, scale_for: impl FnOnce(usvg::Size) -> f32) -> Option<Pixmap> {
    let options = usvg::Options {
        // A linked picture could be any file on the computer, which a talk must never reach,
        // and an embedded one would be decoded without the limits above.
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_str(markup, &options).ok()?;
    let scale = scale_for(tree.size());
    let side = |length: f32| ((length * scale).round() as u32).clamp(1, MAX_EDGE);
    let mut pixmap = Pixmap::new(side(tree.size().width()), side(tree.size().height()))?;
    resvg::render(&tree, Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Some(pixmap)
}

fn svg_color(color: Color) -> String {
    format!("rgba({},{},{},{})", color.r, color.g, color.b, f32::from(color.a) / 255.0)
}

/// The markup of each icon, by name.
fn icons() -> &'static HashMap<String, String> {
    static ICONS: OnceLock<HashMap<String, String>> = OnceLock::new();
    ICONS.get_or_init(|| {
        let set: serde_json::Value = serde_json::from_str(CARBON_ICONS).expect("the bundled icon set is valid JSON");
        let icons = set["icons"].as_object().expect("the bundled icon set has icons");
        icons
            .iter()
            .map(|(name, icon)| (name.clone(), icon["body"].as_str().expect("every icon has a body").to_string()))
            .collect()
    })
}

/// Draws icon `name`, `size` units square, or `None` if there's no such icon.
pub(crate) fn icon_pixmap(name: &str, size: f32, color: Color) -> Option<Arc<Pixmap>> {
    let body = icons().get(name)?.replace("currentColor", &svg_color(color));
    let markup = format!("<svg xmlns=\"{SVG_NAMESPACE}\" viewBox=\"0 0 {ICON_VIEW_BOX} {ICON_VIEW_BOX}\">{body}</svg>");
    draw_svg(&markup, |_| size / ICON_VIEW_BOX).map(Arc::new)
}

/// Draws SVG path `data` in a box of the given size, or `None` if it can't be drawn.
pub(crate) fn path_pixmap(data: &str, width: f32, height: f32, color: Color) -> Option<Arc<Pixmap>> {
    // `data` comes from a component and goes inside markup, where these could end the attribute.
    if data.contains(['<', '>', '&', '"', '\'']) {
        return None;
    }
    let fill = svg_color(color);
    let markup = format!("<svg xmlns=\"{SVG_NAMESPACE}\" width=\"{width}\" height=\"{height}\"><path d=\"{data}\" fill=\"{fill}\"/></svg>");
    draw_svg(&markup, |_| 1.0).map(Arc::new)
}
