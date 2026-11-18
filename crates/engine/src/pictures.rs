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

use crate::{ReadSeek, TalkFiles};

/// The longest edge a picture is kept at.
const MAX_EDGE: u32 = 4096;

/// The longest edge a video's still is kept at: as large as a slide is shown, and no larger.
const MAX_STILL_EDGE: u32 = 1920;

/// What the pictures of a talk can add up to in memory, in pixel bytes, unless it's told otherwise.
const DEFAULT_MEMORY_BUDGET: usize = 512 * 1024 * 1024;

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
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum FileVersion {
    Stamp(u64, SystemTime),
    Hash(u64),
}

impl FileVersion {
    pub(crate) fn of_bytes(bytes: &[u8]) -> FileVersion {
        let mut hasher = DefaultHasher::new();
        hasher.write(bytes);
        FileVersion::Hash(hasher.finish())
    }

    /// The version of `path` as it is now, or `None` if it can't be read.
    pub(crate) fn of_file(path: &str, files: &dyn TalkFiles) -> Option<FileVersion> {
        match files.stamp(path) {
            Some((length, modified)) => Some(FileVersion::Stamp(length, modified)),
            None => files.read(path).map(|bytes| FileVersion::of_bytes(&bytes)),
        }
    }
}

struct CachedPicture {
    /// `None` once it has been let go. What its file was stays, so that a change to the file
    /// still shows.
    picture: Option<Arc<Pixmap>>,
    version: FileVersion,
    /// The value of `PictureCache::asked_for` when it was last asked for.
    last_used: u64,
}

impl CachedPicture {
    fn bytes(&self) -> usize {
        self.picture.as_ref().map_or(0, |picture| picture.data().len())
    }
}

/// The pictures read so far, by their path in the talk. One that couldn't be read isn't
/// kept, so it appears the next time its slide is laid out after the file is fixed.
///
/// Past its budget it lets go of the pictures asked for longest ago. A picture that something
/// else still holds, such as a laid-out slide, stays: letting go of it would free nothing, and
/// it keeps every picture in memory counted here, once each.
pub struct PictureCache {
    pictures: HashMap<String, CachedPicture>,
    memory_budget: usize,
    /// The pixel bytes of the pictures held.
    bytes: usize,
    asked_for: u64,
    /// The stills of the talk's videos, by the video's path in the talk.
    stills: HashMap<String, CachedStill>,
    /// The videos whose stills were asked for since [`Self::take_stills_asked`].
    stills_asked: Vec<String>,
    /// Where stills are decoded off the caller's thread, if they are.
    aside: Option<Aside>,
}

impl Default for PictureCache {
    fn default() -> PictureCache {
        PictureCache {
            pictures: HashMap::new(),
            memory_budget: DEFAULT_MEMORY_BUDGET,
            bytes: 0,
            asked_for: 0,
            stills: HashMap::new(),
            stills_asked: Vec::new(),
            aside: None,
        }
    }
}

/// What a video shows while it isn't playing.
#[derive(Clone)]
pub(crate) enum Still {
    /// Its first picture, made no larger than a slide, and the video's own width and height.
    Frame(Arc<Pixmap>, (u32, u32)),
    /// Why it can't be played.
    Cannot(slides_video::CantPlay),
    /// Being made on another thread, with the video's own width and height if they're known.
    Coming(Option<(u32, u32)>),
}

impl Still {
    fn bytes(&self) -> usize {
        match self {
            Still::Frame(frame, _) => frame.data().len(),
            _ => 0,
        }
    }
}

struct CachedStill {
    still: Still,
    /// What its file was when it was asked for, if the files can say.
    version: Option<FileVersion>,
    /// The value of `PictureCache::asked_for` when it was last asked for.
    last_used: u64,
}

/// Told, on the thread laying a slide out, each time a still is wanted that's to be made aside.
pub type StillWanted = Arc<dyn Fn() + Send + Sync>;

/// Says how wide and high the video at a path in the talk is, from what its file says of itself,
/// decoding nothing; `None` if that can't be known now.
pub type MeasureVideo = Arc<dyn Fn(&str) -> Option<(u32, u32)> + Send + Sync>;

/// A video's still the deck wants made, which can be made on any thread: see
/// [`crate::Deck::still_jobs`].
pub struct StillJob {
    path: String,
    version: Option<FileVersion>,
}

impl StillJob {
    /// The path in the talk of the video whose still it is.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Makes the still from what `reader` reads, the video's file, or, with none, says it
    /// couldn't be read. It decodes, so it's for a thread other than the window's.
    pub fn make(self, reader: Option<Box<dyn ReadSeek>>) -> MadeStill {
        MadeStill { path: self.path, version: self.version, still: make_still(reader) }
    }
}

/// A still made aside, for [`crate::Deck::take_still`] to take in.
pub struct MadeStill {
    path: String,
    version: Option<FileVersion>,
    still: Still,
}

/// Stills to be made aside, as the window's thread mustn't decode a video.
struct Aside {
    wanted: StillWanted,
    measure: MeasureVideo,
    jobs: Vec<StillJob>,
}

/// The version of a large file, found without reading it: its stamp, or else its length.
fn large_file_version(path: &str, files: &dyn TalkFiles) -> Option<FileVersion> {
    match files.stamp(path) {
        Some((length, modified)) => Some(FileVersion::Stamp(length, modified)),
        None => files.len(path).map(|length| FileVersion::Stamp(length, SystemTime::UNIX_EPOCH)),
    }
}

thread_local! {
    static STILLS_MADE_HERE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many videos' stills have been decoded on this thread, for tests that none is on the
/// window's.
#[doc(hidden)]
pub fn stills_decoded_on_this_thread() -> usize {
    STILLS_MADE_HERE.with(std::cell::Cell::get)
}

/// The still of the video `reader` reads, or why there's none: its first picture.
fn make_still(reader: Option<Box<dyn ReadSeek>>) -> Still {
    STILLS_MADE_HERE.with(|made| made.set(made.get() + 1));
    let Some(reader) = reader else {
        return Still::Cannot(slides_video::CantPlay::CouldNotRead);
    };
    let frame = slides_video::open(Box::new(reader)).and_then(|mut video| video.first_frame());
    match frame {
        Ok(frame) => match frame_pixmap(&frame) {
            Some(pixmap) => Still::Frame(Arc::new(pixmap), (frame.width, frame.height)),
            None => Still::Cannot(slides_video::CantPlay::Damaged),
        },
        Err(why) => Still::Cannot(why),
    }
}

/// A video's picture as a pixmap, made smaller to [`MAX_STILL_EDGE`] if it's larger.
fn frame_pixmap(frame: &slides_video::Frame) -> Option<Pixmap> {
    let image = image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra.clone())?;
    let mut image = image::DynamicImage::ImageRgba8(image);
    if frame.width.max(frame.height) > MAX_STILL_EDGE {
        image = image.thumbnail(MAX_STILL_EDGE, MAX_STILL_EDGE);
    }
    let image = image.to_rgba8();
    let mut pixmap = Pixmap::new(image.width(), image.height())?;
    // The picture is blue, green, red and opaque, so it needs no premultiplying.
    for (pixel, bgra) in pixmap.pixels_mut().iter_mut().zip(image.pixels()) {
        *pixel = ColorU8::from_rgba(bgra[2], bgra[1], bgra[0], 255).premultiply();
    }
    Some(pixmap)
}

impl PictureCache {
    /// How many pixel bytes the pictures can add up to, from the next picture read on.
    pub fn set_memory_budget(&mut self, bytes: usize) {
        self.memory_budget = bytes;
    }

    /// Lets go of every picture, keeping the budget: they're read again when next asked for.
    /// The stills of videos whose files in `files`, the talk's files now, are as they were stay:
    /// a still takes decoding to make again.
    pub(crate) fn files_replaced(&mut self, files: &dyn TalkFiles) {
        self.pictures.clear();
        self.stills.retain(|path, cached| large_file_version(path, files) == cached.version);
        self.bytes = self.stills.values().map(|cached| cached.still.bytes()).sum();
    }

    /// Leaves videos' stills to be made aside from now on, telling `wanted` each time one is
    /// wanted: it's then among [`Self::still_jobs`], and until it's taken in, with
    /// [`Self::take_still`], the video shows a plain panel, as large as `measure` says it is.
    /// Stills already wanted stay wanted.
    pub(crate) fn make_stills_aside(&mut self, wanted: StillWanted, measure: MeasureVideo) {
        let jobs = self.aside.take().map(|aside| aside.jobs).unwrap_or_default();
        self.aside = Some(Aside { wanted, measure, jobs });
    }

    /// The stills wanted to be made aside since last asked.
    pub(crate) fn still_jobs(&mut self) -> Vec<StillJob> {
        self.aside.as_mut().map(|aside| std::mem::take(&mut aside.jobs)).unwrap_or_default()
    }

    /// The still of the video at `path`, made once for each version of its file.
    pub(crate) fn still(&mut self, path: &str, files: &dyn TalkFiles) -> Still {
        self.stills_asked.push(path.to_string());
        self.asked_for += 1;
        let asked_for = self.asked_for;
        if let Some(cached) = self.stills.get_mut(path) {
            cached.last_used = asked_for;
            return cached.still.clone();
        }
        let version = large_file_version(path, files);
        let still = match &mut self.aside {
            // A file that isn't there is known not to be read without decoding anything.
            Some(_) if version.is_none() && files.len(path).is_none() => Still::Cannot(slides_video::CantPlay::CouldNotRead),
            Some(aside) => {
                aside.jobs.push(StillJob { path: path.to_string(), version: version.clone() });
                (aside.wanted)();
                Still::Coming((aside.measure)(path))
            }
            None => make_still(files.open(path)),
        };
        self.bytes += still.bytes();
        self.stills.insert(path.to_string(), CachedStill { still: still.clone(), version, last_used: asked_for });
        self.let_go_of_unused();
        still
    }

    /// Why the video at `path` can't be played, if its still has been made and it can't.
    pub(crate) fn still_problem(&self, path: &str) -> Option<slides_video::CantPlay> {
        match self.stills.get(path)?.still {
            Still::Cannot(why) => Some(why),
            _ => None,
        }
    }

    /// Takes in a still made aside, and gives the path of its video if it was still wanted:
    /// one whose file has changed since it was asked for isn't, and is asked for again.
    pub(crate) fn take_still(&mut self, made: MadeStill) -> Option<String> {
        let cached = self.stills.get_mut(&made.path)?;
        if !matches!(cached.still, Still::Coming(_)) || cached.version != made.version {
            return None;
        }
        self.bytes += made.still.bytes();
        cached.still = made.still;
        self.let_go_of_unused();
        Some(made.path)
    }

    /// Forgets every still whose video's file is no longer what it was, as when files are added:
    /// one that couldn't be read may be there now.
    pub(crate) fn drop_replaced_stills(&mut self, files: &dyn TalkFiles) {
        let held = &mut self.bytes;
        self.stills.retain(|path, cached| {
            let same = large_file_version(path, files) == cached.version;
            if !same {
                *held -= cached.still.bytes();
            }
            same
        });
    }

    /// Whether any still is still being made aside.
    pub(crate) fn stills_coming(&self) -> bool {
        self.stills.values().any(|cached| matches!(cached.still, Still::Coming(_)))
    }

    /// The videos whose stills were asked for since this was last asked.
    pub(crate) fn take_stills_asked(&mut self) -> Vec<String> {
        let mut asked = std::mem::take(&mut self.stills_asked);
        asked.sort();
        asked.dedup();
        asked
    }

    /// The pixel bytes of the pictures in memory.
    pub(crate) fn bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) fn is_over_budget(&self) -> bool {
        self.bytes > self.memory_budget
    }

    pub(crate) fn get(&mut self, path: &str, files: &dyn TalkFiles) -> Option<Arc<Pixmap>> {
        self.asked_for += 1;
        let asked_for = self.asked_for;
        if let Some(CachedPicture { picture: Some(picture), last_used, .. }) = self.pictures.get_mut(path) {
            *last_used = asked_for;
            return Some(picture.clone());
        }
        // The stamp comes before the read, so a file that changes in between looks changed
        // the next time it's checked, not current.
        let stamp = files.stamp(path);
        let bytes = files.read(path)?;
        let picture = Arc::new(decode(path, &bytes)?);
        self.bytes += picture.data().len();
        match self.pictures.get_mut(path) {
            // It keeps the version it was first read as: if the file has changed since, layers
            // drawn from the old picture are still around, and the next check has to find them.
            Some(let_go) => {
                let_go.picture = Some(picture.clone());
                let_go.last_used = asked_for;
            }
            None => {
                let version = match stamp {
                    Some((length, modified)) => FileVersion::Stamp(length, modified),
                    None => FileVersion::of_bytes(&bytes),
                };
                self.pictures.insert(path.to_string(), CachedPicture { picture: Some(picture.clone()), version, last_used: asked_for });
            }
        }
        self.let_go_of_unused();
        Some(picture)
    }

    /// While over budget, lets go of the pictures only this cache holds, oldest first.
    /// A video's still only this holds is let go of by being forgotten: it's made again when
    /// next asked for.
    pub(crate) fn let_go_of_unused(&mut self) {
        if !self.is_over_budget() {
            return;
        }
        let pictures = self
            .pictures
            .iter()
            .filter(|(_, cached)| cached.picture.as_ref().is_some_and(|picture| Arc::strong_count(picture) == 1))
            .map(|(path, cached)| (cached.last_used, false, path.clone()));
        let stills = self
            .stills
            .iter()
            .filter(|(_, cached)| matches!(&cached.still, Still::Frame(frame, _) if Arc::strong_count(frame) == 1))
            .map(|(path, cached)| (cached.last_used, true, path.clone()));
        let mut unused: Vec<(u64, bool, String)> = pictures.chain(stills).collect();
        unused.sort_unstable();
        for (_, still, path) in unused {
            if self.bytes <= self.memory_budget {
                break;
            }
            if still {
                if let Some(cached) = self.stills.remove(&path) {
                    self.bytes -= cached.still.bytes();
                }
            } else if let Some(cached) = self.pictures.get_mut(&path) {
                self.bytes -= cached.bytes();
                cached.picture = None;
            }
        }
    }

    /// Forgets every picture whose file is no longer the one that was read, and says
    /// whether there were any.
    pub(crate) fn drop_replaced(&mut self, files: &dyn TalkFiles) -> bool {
        let before = (self.pictures.len(), self.stills.len());
        let held = &mut self.bytes;
        self.pictures.retain(|path, cached| {
            let same = FileVersion::of_file(path, files).as_ref() == Some(&cached.version);
            if !same {
                *held -= cached.bytes();
            }
            same
        });
        self.stills.retain(|path, cached| {
            let same = large_file_version(path, files) == cached.version;
            if !same {
                *held -= cached.still.bytes();
            }
            same
        });
        (self.pictures.len(), self.stills.len()) != before
    }
}

/// What kind of picture a file holds, told from its bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PictureFormat {
    Png,
    Jpeg,
    Webp,
    Gif,
    Bmp,
    Tiff,
    Svg,
    /// Another kind the engine reads, such as an icon file.
    Other,
}

/// What kind of picture `bytes` hold, if they're a picture the engine draws, decoded with the
/// same limits as when it's shown. Told from the bytes alone: a raster picture says what it is
/// in its first bytes, and anything else is read as an SVG.
pub fn picture_format(bytes: &[u8]) -> Option<PictureFormat> {
    let format = image::guess_format(bytes).ok();
    let drawn = match format {
        Some(_) => decode("", bytes),
        None => decode("picture.svg", bytes),
    };
    drawn?;
    Some(match format {
        None => PictureFormat::Svg,
        Some(image::ImageFormat::Png) => PictureFormat::Png,
        Some(image::ImageFormat::Jpeg) => PictureFormat::Jpeg,
        Some(image::ImageFormat::WebP) => PictureFormat::Webp,
        Some(image::ImageFormat::Gif) => PictureFormat::Gif,
        Some(image::ImageFormat::Bmp) => PictureFormat::Bmp,
        Some(image::ImageFormat::Tiff) => PictureFormat::Tiff,
        Some(_) => PictureFormat::Other,
    })
}

/// The raster picture in `bytes` written again as a PNG, at its own size, if the engine reads
/// it. An animated one gives its first frame.
pub fn picture_as_png(bytes: &[u8]) -> Option<Vec<u8>> {
    std::panic::catch_unwind(|| {
        let image = limited_reader(bytes)?.decode().ok()?;
        let mut png = Vec::new();
        image.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
        Some(png)
    })
    .ok()
    .flatten()
}

/// A reader of the raster picture in `bytes` that refuses one that claims too much.
fn limited_reader(bytes: &[u8]) -> Option<ImageReader<Cursor<&[u8]>>> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DECODED_SIDE);
    limits.max_image_height = Some(MAX_DECODED_SIDE);
    limits.max_alloc = Some(MAX_DECODED_BYTES);
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?;
    reader.limits(limits);
    Some(reader)
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
    let mut image = limited_reader(bytes)?.decode().ok()?;
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

fn svg_options() -> usvg::Options<'static> {
    usvg::Options {
        // A linked picture could be any file on the computer, which a talk must never reach,
        // and an embedded one would be decoded without the limits above.
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    }
}

/// Draws SVG markup at the scale `scale_for` picks from the size the markup declares.
fn draw_svg(markup: &str, scale_for: impl FnOnce(usvg::Size) -> f32) -> Option<Pixmap> {
    let tree = usvg::Tree::from_str(markup, &svg_options()).ok()?;
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

/// The name of every icon, in order.
pub fn icon_names() -> &'static [&'static str] {
    static NAMES: OnceLock<Vec<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names: Vec<&'static str> = icons().keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    })
}

/// Icon `name` drawn `size` pixels square in `color`, as red, green, blue and alpha, for
/// showing it to pick it. `None` if there's no such icon or the size is 0. A size bigger
/// than a picture is kept at is held to that.
pub fn icon_preview(name: &str, size: u32, color: [u8; 4]) -> Option<Pixmap> {
    if size == 0 {
        return None;
    }
    let [r, g, b, a] = color;
    let icon = icon_pixmap(name, size.min(MAX_EDGE) as f32, Color { r, g, b, a })?;
    Some(Arc::unwrap_or_clone(icon))
}

/// The size in pixels the picture at `path`, whose file is `bytes`, would be drawn at,
/// found without drawing it. `None` if it can't be read, or is bigger than can be.
pub(crate) fn picture_size(path: &str, bytes: &[u8]) -> Option<(u32, u32)> {
    std::panic::catch_unwind(|| if is_svg(path) { svg_size(bytes) } else { raster_size(bytes) }).ok().flatten()
}

fn raster_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let (width, height) = ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok()?;
    if width == 0 || height == 0 || width > MAX_DECODED_SIDE || height > MAX_DECODED_SIDE {
        return None;
    }
    // As `decode_raster` shrinks it.
    let scale = (MAX_EDGE as f64 / f64::from(width.max(height))).min(1.0);
    let side = |length: u32| ((f64::from(length) * scale).round() as u32).max(1);
    Some((side(width), side(height)))
}

fn svg_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let size = usvg::Tree::from_str(std::str::from_utf8(bytes).ok()?, &svg_options()).ok()?.size();
    // As `decode_svg` scales it.
    let scale = (MAX_EDGE as f32 / size.width().max(size.height())).min(1.0);
    let side = |length: f32| ((length * scale).round() as u32).clamp(1, MAX_EDGE);
    Some((side(size.width()), side(size.height())))
}

/// Draws icon `name`, `size` units square, or `None` if there's no such icon.
pub(crate) fn icon_pixmap(name: &str, size: f32, color: Color) -> Option<Arc<Pixmap>> {
    let body = icons().get(name)?.replace("currentColor", &svg_color(color));
    let markup = format!("<svg xmlns=\"{SVG_NAMESPACE}\" viewBox=\"0 0 {ICON_VIEW_BOX} {ICON_VIEW_BOX}\">{body}</svg>");
    draw_svg(&markup, |_| size / ICON_VIEW_BOX).map(Arc::new)
}
