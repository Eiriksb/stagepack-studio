//! Reads a talk's pictures and draws icons, as pixmaps.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::io::Cursor;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;

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

/// The most an SVG can need while it's drawn, in pixels of 4 bytes.
const MAX_SVG_PIXELS: u64 = 256 * 1024 * 1024 / 4;
/// How much resvg pads a layer by, past the edges of what's in it.
const LAYER_MARGIN: u32 = 4;

const CARBON_ICONS: &str = include_str!("../assets/icons/carbon.json");
/// The side of the square every Carbon icon is drawn in.
const ICON_VIEW_BOX: f32 = 32.0;

const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";

/// What tells a file from a later version of itself: its stamp, or a hash of its bytes
/// where there's no stamp to go by.
#[derive(PartialEq)]
enum FileVersion {
    Stamp(u64, SystemTime),
    Hash(u64),
}

impl FileVersion {
    fn of_bytes(bytes: &[u8]) -> FileVersion {
        let mut hasher = DefaultHasher::new();
        hasher.write(bytes);
        FileVersion::Hash(hasher.finish())
    }

    /// The version of `path` as it is now, or `None` if it can't be read.
    fn of_file(path: &str, files: &dyn TalkFiles) -> Option<FileVersion> {
        match files.stamp(path) {
            Some((length, modified)) => Some(FileVersion::Stamp(length, modified)),
            None => files.read(path).map(|bytes| FileVersion::of_bytes(&bytes)),
        }
    }
}

struct CachedPicture {
    picture: Arc<Pixmap>,
    version: FileVersion,
}

/// The pictures read so far, by their path in the talk. One that couldn't be read isn't
/// kept, so it appears the next time its slide is laid out after the file is fixed.
#[derive(Default)]
pub struct PictureCache {
    pictures: HashMap<String, CachedPicture>,
}

impl PictureCache {
    pub(crate) fn get(&mut self, path: &str, files: &dyn TalkFiles) -> Option<Arc<Pixmap>> {
        if let Some(cached) = self.pictures.get(path) {
            return Some(cached.picture.clone());
        }
        // The stamp comes before the read, so a file that changes in between looks changed
        // the next time it's checked, not current.
        let stamp = files.stamp(path);
        let bytes = files.read(path)?;
        let version = match stamp {
            Some((length, modified)) => FileVersion::Stamp(length, modified),
            None => FileVersion::of_bytes(&bytes),
        };
        let picture = Arc::new(decode(path, &bytes)?);
        self.pictures.insert(path.to_string(), CachedPicture { picture: picture.clone(), version });
        Some(picture)
    }

    /// Forgets every picture whose file is no longer the one that was read, and says
    /// whether there were any.
    pub(crate) fn drop_replaced(&mut self, files: &dyn TalkFiles) -> bool {
        let before = self.pictures.len();
        self.pictures.retain(|path, cached| FileVersion::of_file(path, files).as_ref() == Some(&cached.version));
        self.pictures.len() != before
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
    let canvas = (side(tree.size().width()), side(tree.size().height()));
    if !fits_in_memory(&tree, scale, canvas) {
        return None;
    }
    let mut pixmap = Pixmap::new(canvas.0, canvas.1)?;
    resvg::render(&tree, Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Some(pixmap)
}

/// Whether drawing `tree` at `scale` on a canvas of `canvas` pixels stays within
/// `MAX_SVG_PIXELS`. resvg allocates a buffer for every pattern tile, layer, clip, mask and
/// filter step, sized by what the SVG says, and running out of memory aborts the app. This
/// follows the steps resvg's drawing would take, without its recursion, and adds up those
/// buffers as if they were all held at once.
fn fits_in_memory(tree: &usvg::Tree, scale: f32, canvas: (u32, u32)) -> bool {
    // resvg's own limit on the side of a layer.
    let bounds = (canvas.0.saturating_mul(5), canvas.1.saturating_mul(5));
    let root = DrawStep::Children { group: tree.root(), transform: Transform::from_scale(scale, scale), bounds };
    let mut count = BufferCount { pixels: pixel_count(canvas.0, canvas.1), steps: vec![root] };
    // Anything drawn more than once costs at least a pixel each time, so this ends even
    // when patterns refer to patterns many times over.
    while count.pixels <= MAX_SVG_PIXELS {
        let Some(step) = count.steps.pop() else { return true };
        count.follow(step);
    }
    false
}

fn pixel_count(width: u32, height: u32) -> u64 {
    u64::from(width) * u64::from(height)
}

/// Something resvg would draw, still to be counted.
enum DrawStep<'a> {
    /// A group's children, through `transform`, with layers up to `bounds` pixels a side.
    Children { group: &'a usvg::Group, transform: Transform, bounds: (u32, u32) },
    /// A clip path over a layer of `layer` pixels.
    Clip { clip: &'a usvg::ClipPath, layer: u64 },
    /// A group inside a clip path over a layer of `layer` pixels.
    ClipChildren { group: &'a usvg::Group, layer: u64 },
}

struct BufferCount<'a> {
    pixels: u64,
    steps: Vec<DrawStep<'a>>,
}

impl<'a> BufferCount<'a> {
    fn add(&mut self, pixels: u64) {
        self.pixels = self.pixels.saturating_add(pixels);
    }

    fn follow(&mut self, step: DrawStep<'a>) {
        match step {
            DrawStep::Children { group, transform, bounds } => {
                for child in group.children() {
                    match child {
                        usvg::Node::Group(group) => self.group(group, transform, bounds),
                        usvg::Node::Path(path) => {
                            let paints = [path.fill().map(usvg::Fill::paint), path.stroke().map(usvg::Stroke::paint)];
                            for paint in paints.into_iter().flatten() {
                                if let usvg::Paint::Pattern(pattern) = paint {
                                    self.pattern(pattern, transform, bounds);
                                }
                            }
                        }
                        usvg::Node::Text(text) => self.group(text.flattened(), transform, bounds),
                        // `draw_svg` turns pictures off, so an SVG has none of these.
                        usvg::Node::Image(_) => {}
                    }
                }
            }
            DrawStep::Clip { clip, layer } => {
                self.add(layer);
                self.steps.extend(clip.clip_path().map(|clip| DrawStep::Clip { clip, layer }));
                self.steps.push(DrawStep::ClipChildren { group: clip.root(), layer });
            }
            DrawStep::ClipChildren { group, layer } => {
                for child in group.children() {
                    match child {
                        usvg::Node::Group(group) => {
                            if let Some(clip) = group.clip_path() {
                                self.add(layer);
                                self.steps.push(DrawStep::Clip { clip, layer });
                            }
                            self.steps.push(DrawStep::ClipChildren { group, layer });
                        }
                        usvg::Node::Text(text) => self.steps.push(DrawStep::ClipChildren { group: text.flattened(), layer }),
                        _ => {}
                    }
                }
            }
        }
    }

    fn group(&mut self, group: &'a usvg::Group, transform: Transform, bounds: (u32, u32)) {
        let transform = transform.pre_concat(group.transform());
        if group.should_isolate() {
            let Some(area) = group.layer_bounding_box().transform(transform) else { return };
            let side = |length: f32, bound: u32| (length.ceil() as u32).saturating_add(LAYER_MARGIN).min(bound);
            let layer = pixel_count(side(area.width(), bounds.0), side(area.height(), bounds.1));
            self.add(layer);
            for filter in group.filters() {
                self.filter(filter, transform, layer);
            }
            self.steps.extend(group.clip_path().map(|clip| DrawStep::Clip { clip, layer }));
            let mut mask = group.mask();
            while let Some(this_mask) = mask {
                self.add(layer);
                self.steps.push(DrawStep::Children { group: this_mask.root(), transform, bounds });
                mask = this_mask.mask();
            }
        }
        self.steps.push(DrawStep::Children { group, transform, bounds });
    }

    /// A pattern is drawn to a tile of its own, as big as one repeat at this scale.
    fn pattern(&mut self, pattern: &'a usvg::Pattern, transform: Transform, bounds: (u32, u32)) {
        let (scale_x, scale_y) = transform.pre_concat(pattern.transform()).get_scale();
        let tile = pixel_count((pattern.rect().width() * scale_x).round() as u32, (pattern.rect().height() * scale_y).round() as u32);
        if tile > 0 {
            self.add(tile);
            self.steps.push(DrawStep::Children { group: pattern.root(), transform: Transform::from_scale(scale_x, scale_y), bounds });
        }
    }

    /// A filter works on its own region, which nothing keeps to the size of its layer, and
    /// holds on to the result of every step until it's done.
    fn filter(&mut self, filter: &'a usvg::filter::Filter, transform: Transform, layer: u64) {
        let Some(region) = filter.rect().transform(transform) else { return };
        let (width, height) = (region.width().ceil() as u32, region.height().ceil() as u32);
        let results = filter.primitives().len() as u64 + 1;
        self.add(pixel_count(width, height).max(layer).saturating_mul(results));
        let (scale_x, scale_y) = transform.get_scale();
        for primitive in filter.primitives() {
            if let usvg::filter::Kind::Image(image) = primitive.kind() {
                let transform = Transform::from_scale(scale_x, scale_y);
                self.steps.push(DrawStep::Children { group: image.root(), transform, bounds: (width, height) });
            }
        }
    }
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
