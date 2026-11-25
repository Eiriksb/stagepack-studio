//! A talk and its drawn layers, kept so that an edit only draws again what it changed.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;
use std::sync::{Arc, Weak};

use tiny_skia::Pixmap;

use slides_components::Diagnostic;
use slides_format::{Settings, Talk, Target, is_hidden, parse_talk};
use toolkit::{ComponentSchema, Registry};

use crate::builtins::DEFAULT_ICON_SIZE;
use crate::pictures::picture_size;
use crate::slide::{EmptyRegion, MAX_SIZE, SLIDE_WIDTH, empty_region_layer, layout_slide_placed, placed_at, placed_tag, placed_tag_layer};
use crate::text_geometry::{LayerText, RegionBox, TextLayout};
use crate::timeline::MAX_CLICK;
use crate::render::device_offset;
use crate::{
    ComponentAsks, ComponentInfo, DisplayItem, Fonts, Layer, LayerSpec, LayerTiming, LayoutEnv, Moment, MadeStill, PictureCache, Placing,
    SlideTimeline, SchemaLoad, ButtonLayer, ComponentAnswer, ComponentFeed, ComponentLayer, MeasureVideo, StillJob, StillWanted, TalkComponents, TalkFiles, TerminalLayer, Transition, VideoLayer, DrawModel, ModelLayer, builtin_registry, compose, render_layer, talk_theme,
};

/// How many scales of one slide are kept drawn. A zoom or a resize asks for many in a row.
const KEPT_SCALES: usize = 4;

/// What the drawn layers of a whole deck can add up to, in pixel bytes, unless it's told otherwise.
const DEFAULT_MEMORY_BUDGET: usize = 512 * 1024 * 1024;

/// The widest a thumbnail can be, which is more than a list of slides will ask for.
const MAX_THUMBNAIL_WIDTH: u32 = 4096;

/// What makes a drawn layer the same as one drawn before: the text it came from, the
/// look of the talk and the slide's layout, where it sits and how it turns, and the
/// pictures and components of the talk at that moment.
#[derive(Clone, PartialEq, Eq, Hash)]
struct LayerKey {
    text: String,
    place: [u32; 5],
    /// Where and how big what the layer draws is. Text moves inside a layer when something
    /// elsewhere changes it, such as a list's indent, with the layer's text just as it was.
    drawing: u64,
    files_generation: u64,
    /// The step and the bits of the time it was drawn at, for a layer that changes with
    /// them. `drawing` doesn't see colours, so it can't tell those layers apart.
    moment: Option<(u32, u32)>,
    /// The terminal recording it's the last screen of, if it is one.
    terminal: Option<String>,
    /// The state an interactive component's layer was drawn from: 0 for any other layer.
    state: u64,
}

impl LayerKey {
    /// Whether the two are the same layer but for where it sits: the same picture, moved.
    fn moved_from(&self, other: &LayerKey) -> bool {
        self.place[2..] == other.place[2..]
            && self.drawing == other.drawing
            && self.files_generation == other.files_generation
            && self.moment == other.moment
            && self.state == other.state
            && self.text == other.text
    }
}

/// Where a layer sits and how it turns, as a key holds it.
fn place_bits(spec: &LayerSpec) -> [u32; 5] {
    let rect = spec.rect;
    [rect.x, rect.y, rect.w, rect.h, spec.rotate].map(f32::to_bits)
}

/// Where a pixmap was drawn: the place on the slide it was drawn for, in slide units, and the
/// device pixel it sits at there. A pixmap only moved is always placed from here, so that
/// moving it again and again adds up no rounding.
#[derive(Clone, Copy)]
struct DrawnAt {
    x: f32,
    y: f32,
    left: i32,
    top: i32,
}

impl DrawnAt {
    fn of(layer: &Layer, x: f32, y: f32) -> DrawnAt {
        DrawnAt { x, y, left: layer.x, top: layer.y }
    }

    /// `pixmap`, drawn here, shown at (`x`, `y`) on the slide, at `scale`.
    fn shown_at(&self, pixmap: &Arc<Pixmap>, x: f32, y: f32, scale: f32) -> Arc<Layer> {
        let at = |from: i32, by: f32| device_offset(from as f32 + (by * scale).round());
        Arc::new(Layer { pixmap: pixmap.clone(), x: at(self.left, x - self.x), y: at(self.top, y - self.y) })
    }
}

/// A layer of `drawn` that is `key` but for where it sits, shown where `key` sits at `scale`,
/// and where its pixmap was drawn. The slide's own layers are looked at first.
fn moved_layer(drawn: &Drawn, key: &LayerKey, scale: f32) -> Option<(Arc<Layer>, DrawnAt)> {
    let place = |key: &LayerKey| (f32::from_bits(key.place[0]), f32::from_bits(key.place[1]));
    let own = drawn.by_key.iter().find(|(other, _)| key.moved_from(other)).map(|(other, layer)| {
        let (x, y) = place(other);
        (layer, DrawnAt::of(layer, x, y))
    });
    let placed = || drawn.placed_by_key.iter().find(|(other, _)| key.moved_from(other)).map(|(_, (layer, at))| (layer, *at));
    let (layer, at) = own.or_else(placed)?;
    let (x, y) = place(key);
    Some((at.shown_at(&layer.pixmap, x, y, scale), at))
}

/// A slide laid out. The keys are made when the slide is, because `LayerSpec::source`
/// points into the text of that moment, which later edits move.
#[derive(Clone)]
struct Laid {
    specs: Vec<LayerSpec>,
    keys: Vec<LayerKey>,
    notices: Vec<String>,
    transition: Transition,
    /// Whether it shows a picture from a file of the talk, and so holds that picture in memory.
    holds_pictures: bool,
    /// The videos whose stills it shows, or the panel shown while one is made.
    stills: Vec<String>,
    asks: ComponentAsks,
    /// Where each layer's text is, beside `specs`.
    texts: Vec<LayerText>,
    /// The regions of the layout with nothing in them.
    empty_regions: Vec<EmptyRegion>,
    /// Every region of the layout, as placed.
    regions: Vec<RegionBox>,
    /// The most steps a region shown a piece at a time takes.
    each_steps: u32,
    /// Each region shown a piece at a time, by name, and the steps it takes.
    each_regions: Vec<(String, u32)>,
    /// Where the slide's editable text is, once it's been asked for, and the state of the
    /// talk's loaded modules it was worked out for: which of them take content.
    text_layout: Option<(u64, Arc<TextLayout>)>,
}

impl Laid {
    /// When each of its layers appears, in order.
    fn timings(&self) -> Vec<LayerTiming> {
        self.specs.iter().map(|spec| LayerTiming { click: spec.click, enter: spec.enter }).collect()
    }

    /// The most clicks its layers or components ask for. A 3D model takes a click for each of
    /// its views, after the click it appears on, and a terminal recording one for each of its
    /// stops.
    fn steps(&self) -> u32 {
        crate::slide::steps_of(&self.specs, self.asks.steps)
    }
}

/// A number that differs when a layer's items sit or measure differently. A terminal recording's
/// screen of many items is told by the items it shares with what's kept of the recording, which
/// are the same as long as its box is, and by the version of the file it was made from, `version`,
/// as the items of a version since may lie where those of the one before did: a screen has too
/// many to go through each time it's laid out, and with its box, which the key holds, and its
/// file, they're all there is to it.
fn drawing_fingerprint(spec: &LayerSpec, version: Option<&crate::pictures::FileVersion>) -> u64 {
    let mut hasher = DefaultHasher::new();
    if spec.terminal.is_some() && spec.items.len() > 64 {
        (3u8, spec.items.as_ptr() as usize, spec.items.len(), version).hash(&mut hasher);
        return hasher.finish();
    }
    for item in spec.items.iter() {
        match item {
            DisplayItem::Fill { path, .. } | DisplayItem::Stroke { path, .. } => {
                let b = path.bounds();
                (0u8, [b.left(), b.top(), b.right(), b.bottom()].map(f32::to_bits)).hash(&mut hasher);
            }
            DisplayItem::Text { text, x, y } => {
                (1u8, [*x, *y, text.width, text.height].map(f32::to_bits)).hash(&mut hasher);
                // Where each line starts, which aligning the text moves and nothing above sees.
                for line in text.buffer.layout_runs() {
                    line.glyphs.first().map(|glyph| glyph.x.to_bits()).hash(&mut hasher);
                }
            }
            DisplayItem::Picture { rect, .. } => (2u8, [rect.x, rect.y, rect.w, rect.h].map(f32::to_bits)).hash(&mut hasher),
        }
    }
    hasher.finish()
}

#[derive(Default)]
struct Drawn {
    /// The layers that came from the slide's text. The background isn't one of them.
    by_key: HashMap<LayerKey, Arc<Layer>>,
    /// The layers in order at a moment, as long as nothing in the slide has changed.
    ordered: Option<(Moment, Vec<Arc<Layer>>)>,
    /// What the editor shows in the slide's empty regions, as long as nothing in it has changed.
    empty_regions: Option<Vec<Arc<Layer>>>,
    /// The layers shown with the deck's placing that the slide's own layers don't have.
    placed_by_key: HashMap<LayerKey, (Arc<Layer>, DrawnAt)>,
    /// The layers in order with the deck's placing, as long as it stays.
    placed_ordered: Option<Vec<Arc<Layer>>>,
    /// What the editor shows in the slide's empty regions with the deck's placing.
    placed_empty_regions: Option<Vec<Arc<Layer>>>,
    /// The value of `Deck::layers_asked_for` when these were last asked for.
    asked: u64,
    /// What stands in for terminal recordings' layers being drawn aside, or whose screens are
    /// being shaped aside, by their keys: each made once at this scale.
    standing: HashMap<LayerKey, Arc<Layer>>,
}

impl Drawn {
    /// Forgets what's kept for the slide as it was, keeping the layers by key.
    fn slide_changed(&mut self) {
        self.ordered = None;
        self.empty_regions = None;
        self.placing_cleared();
    }

    /// Forgets what's shown with a placing that has changed, keeping its layers by key.
    fn placing_changed(&mut self) {
        self.placed_ordered = None;
        self.placed_empty_regions = None;
    }

    /// Forgets everything shown with a placing.
    fn placing_cleared(&mut self) {
        self.placing_changed();
        self.placed_by_key.clear();
    }

    /// Every layer it holds, some of them perhaps more than once.
    fn layers(&self) -> impl Iterator<Item = &Arc<Layer>> {
        let empty = self.empty_regions.iter().chain(&self.placed_empty_regions).flatten();
        self.by_key.values().chain(self.placed_by_key.values().map(|(layer, _)| layer)).chain(empty).chain(self.standing.values())
    }
}

struct SlideState {
    text: String,
    /// The slide laid out at `Moment::START`.
    laid: Option<Laid>,
    /// The slide laid out at the one other moment it was last asked for, which differs from the start.
    at_moment: Option<(Moment, Laid)>,
    /// The slide laid out in its whole state with the deck's placing, while it has one here.
    placed: Option<Laid>,
    drawn: HashMap<u32, Drawn>,
    /// The whole slide on one small pixmap, and the width it was made for. It lasts until the
    /// slide changes, and counts against the memory budget with the drawn layers.
    thumbnail: Option<(u32, Arc<Pixmap>)>,
    /// The value of `Deck::layers_asked_for` when this slide's layers were last asked for.
    last_used: u64,
    /// The value of `Deck::layers_asked_for` when this slide's thumbnail was last asked for.
    /// It's apart from `last_used`, so that a list of thumbnails scrolled through doesn't make
    /// the slides in it seem more wanted than the one on screen.
    thumbnail_used: u64,
    /// Differs from every other slide's, and from what this one had before it last changed:
    /// what a picture of the slide was drawn from.
    version: u64,
}

pub struct Deck {
    files: Rc<dyn TalkFiles>,
    fonts: Fonts,
    registry: Registry,
    components: TalkComponents,
    pictures: PictureCache,
    /// Goes up each time a picture in the talk is found to have been replaced, or its
    /// components to have changed.
    files_generation: u64,
    talk: Talk,
    /// The talk's look, as text that can be part of a key.
    theme: String,
    slides: Vec<SlideState>,
    /// The layers with no source, by theme and scale. Nothing on a slide changes them, so
    /// every slide shares one.
    backgrounds: HashMap<(String, u32), Arc<Layer>>,
    memory_budget: usize,
    layers_asked_for: u64,
    /// The slide whose layers were asked for last.
    last_asked: Option<usize>,
    /// The slides whose `laid` holds pictures, which are the ones that can give memory back.
    slides_holding_pictures: BTreeSet<usize>,
    /// The slides a show goes through, worked out when the talk is read.
    shown: Vec<usize>,
    /// The last version given to a slide.
    versions_made: u64,
    /// Where things on one slide are shown that their source doesn't say yet, and the slide.
    placing: Option<(usize, Vec<Placing>)>,
    /// Layers wanted drawn aside, as they take too long to draw here, and those being drawn: by
    /// slide, scale and key, each with what its job holds, which is gone once the job is dropped
    /// undrawn, or what it drew is dropped untaken.
    draw_jobs: Vec<DrawJob>,
    drawing: HashMap<(usize, u32, LayerKey), Weak<()>>,
}

/// The most glyphs and rectangles a terminal recording's layer has that are drawn on the thread
/// asking for it, when stills are made aside: one with more is drawn aside, about a millisecond
/// and a half of it.
const MOST_PIECES_DRAWN_HERE: usize = 4_000;

/// A layer to be drawn on another thread, with fonts of its own: a terminal recording's, whose
/// screen takes too long to draw where it's asked for. See [`Deck::draw_jobs`].
pub struct DrawJob {
    slide: usize,
    scale: u32,
    key: LayerKey,
    /// Cheap to copy: its items are shared with the slide's layout.
    spec: LayerSpec,
    fonts: crate::FontsSeed,
    /// Held until what it drew is taken in: the deck waits for it only while it's held.
    held: Arc<()>,
}

impl DrawJob {
    /// Draws the layer: for a thread other than the window's.
    pub fn draw(self) -> DrawnLayer {
        let mut fonts = self.fonts.fonts();
        let layer = Arc::new(render_layer(&self.spec, f32::from_bits(self.scale), &mut fonts));
        DrawnLayer { slide: self.slide, scale: self.scale, key: self.key, layer, _held: self.held }
    }
}

/// A layer drawn aside, for [`Deck::take_drawn`] to take in.
pub struct DrawnLayer {
    slide: usize,
    scale: u32,
    key: LayerKey,
    layer: Arc<Layer>,
    _held: Arc<()>,
}

/// How many glyphs and rectangles of a terminal recording's layer are drawn at `scale`: the glyphs
/// of rows tall enough for text to be drawn, and each cell's fill and underline.
fn pieces_drawn(spec: &LayerSpec, scale: f32) -> usize {
    let shapes = |path: &tiny_skia::Path| path.segments().filter(|segment| matches!(segment, tiny_skia::PathSegment::MoveTo(_))).count();
    spec.items
        .iter()
        .map(|item| match item {
            DisplayItem::Text { text, .. } if text.buffer.metrics().line_height * scale >= crate::render::TERMINAL_TEXT_ROW => {
                text.buffer.layout_runs().map(|run| run.glyphs.len()).sum::<usize>()
            }
            DisplayItem::Fill { path, .. } | DisplayItem::Stroke { path, .. } => shapes(path),
            _ => 0,
        })
        .sum()
}

/// A layer drawn, and its key.
type Picture = (LayerKey, Arc<Layer>);

/// What's shown of a terminal recording's layer while it's drawn aside, or while its screen is
/// shaped and placed aside: what stood in for it at this scale already, or a picture of the
/// recording, `own`, drawn at this scale, moved or stretched to where it is now, or else one of
/// `others`, at the nearest scale below this one, or the nearest above, stretched, or else what
/// stood in for it at any scale, stretched, or else its plain panel. Gives as well the picture at
/// this scale it was made from, to keep until the layer is drawn.
fn standing_in(own: &Drawn, others: &HashMap<u32, Drawn>, key: &LayerKey, spec: &LayerSpec, scale: f32, fonts: &mut Fonts) -> (Arc<Layer>, Option<Picture>) {
    let from = || drawn_of(own, key).map(|(key, layer)| (key.clone(), layer.clone()));
    if let Some(layer) = own.standing.get(key) {
        return (layer.clone(), from());
    }
    if let Some((from, layer, _)) = resized_layer(own, key, spec, scale) {
        return (layer, Some(from));
    }
    // Positive floats order as their bits do.
    let nearest = |(at, _): &(u32, &Arc<Layer>)| {
        let at = f32::from_bits(*at);
        if at <= scale { (false, (scale - at).to_bits()) } else { (true, (at - scale).to_bits()) }
    };
    let pictures = others.iter().filter_map(|(at, drawn)| Some((*at, drawn_of(drawn, key)?.1)));
    let this = scale.to_bits();
    let stood_in = std::iter::once((&this, own)).chain(others).filter_map(|(at, drawn)| Some((*at, standing_of(drawn, key)?)));
    if let Some((_, picture)) = pictures.min_by_key(nearest).or_else(|| stood_in.min_by_key(nearest)) {
        return (Arc::new(crate::render::stretched_layer(picture, spec, scale)), None);
    }
    let panel = LayerSpec { items: spec.items.iter().take(1).cloned().collect(), ..spec.clone() };
    (Arc::new(render_layer(&panel, scale, fonts)), None)
}

/// What stood in for a layer of the terminal recording whose layer `key` is, in `drawn`.
fn standing_of<'a>(drawn: &'a Drawn, key: &LayerKey) -> Option<&'a Arc<Layer>> {
    key.terminal.as_ref()?;
    drawn.standing.iter().find(|(other, _)| other.terminal == key.terminal && other.files_generation == key.files_generation).map(|(_, layer)| layer)
}

/// Wants slide `index`'s layer of `key`, `spec`, drawn aside at `scale`, among `jobs`, unless it's
/// `drawing` already, and says whether it's newly wanted. It's drawn once, at the scale wanted
/// last: a job not yet taken for it at another scale is let go of.
fn want_drawn(
    jobs: &mut Vec<DrawJob>,
    drawing: &mut HashMap<(usize, u32, LayerKey), Weak<()>>,
    fonts: &mut Fonts,
    (index, scale, key, spec): (usize, f32, &LayerKey, &LayerSpec),
) -> bool {
    let at = (index, scale.to_bits(), key.clone());
    if drawing.get(&at).is_some_and(|held| held.strong_count() > 0) {
        return false;
    }
    jobs.retain(|job| !(job.slide == index && job.key == *key));
    drawing.retain(|_, held| held.strong_count() > 0);
    let held = Arc::new(());
    drawing.insert(at, Arc::downgrade(&held));
    jobs.push(DrawJob { slide: index, scale: scale.to_bits(), key: key.clone(), spec: spec.clone(), fonts: fonts.seed(), held });
    true
}

/// Makes room in `drawn`, a slide's layers by scale, for those at `scale`: of as many scales as are
/// kept, the one asked for longest ago is let go of.
fn make_room(drawn: &mut HashMap<u32, Drawn>, scale: u32) {
    if drawn.len() >= KEPT_SCALES && !drawn.contains_key(&scale) {
        let oldest = drawn.iter().min_by_key(|(_, drawn)| drawn.asked).map(|(scale, _)| *scale);
        drawn.retain(|kept, _| Some(*kept) != oldest);
    }
}

/// The talk's settings, as far as they decide how a slide looks: not where they sit.
fn settings_pairs(settings: Option<&Settings>) -> Vec<(&str, &str)> {
    settings.map(|s| s.entries.iter().map(|e| (e.key.as_str(), e.value.as_str())).collect()).unwrap_or_default()
}

fn slide_texts(talk: &Talk) -> Vec<String> {
    talk.slides.iter().map(|slide| talk.source[slide.range.clone()].to_string()).collect()
}

fn theme_key(talk: &Talk) -> String {
    format!("{:?}", talk_theme(talk))
}

/// All but the hidden slides of `talk`, or all of them if every slide is hidden, so that a
/// show is never empty.
fn shown_slides_of(talk: &Talk) -> Vec<usize> {
    let shown: Vec<usize> = (0..talk.slides.len()).filter(|&index| !is_hidden(&talk.slides[index])).collect();
    if shown.is_empty() { (0..talk.slides.len()).collect() } else { shown }
}

/// How the slides of a talk just taken are matched to the ones it had, to keep what's known of each.
enum Matching {
    /// Each to the one that was at its place.
    ByPlace,
    /// From the start, and then from the end: slides after one added or taken out are still found.
    FromBothEnds,
}

impl Deck {
    pub fn open(files: Box<dyn TalkFiles>, fonts: Fonts, source: String) -> Deck {
        Deck::with_registry(files, fonts, source, builtin_registry())
    }

    /// As [`Self::open`], with the talk's text already read.
    pub fn open_parsed(files: Box<dyn TalkFiles>, fonts: Fonts, talk: Talk) -> Deck {
        Deck::of_talk(files, fonts, talk, builtin_registry())
    }

    /// A deck whose talk can use the components in `registry`, as well as its own.
    pub fn with_registry(files: Box<dyn TalkFiles>, fonts: Fonts, source: String, registry: Registry) -> Deck {
        Deck::of_talk(files, fonts, parse_talk(&source), registry)
    }

    fn of_talk(files: Box<dyn TalkFiles>, fonts: Fonts, talk: Talk, registry: Registry) -> Deck {
        let files: Rc<dyn TalkFiles> = Rc::from(files);
        let components = TalkComponents::scan_for(files.clone(), &registry);
        let slides: Vec<SlideState> = slide_texts(&talk)
            .into_iter()
            .zip(1..)
            .map(|(text, version)| SlideState {
                text,
                laid: None,
                at_moment: None,
                placed: None,
                drawn: HashMap::new(),
                thumbnail: None,
                last_used: 0,
                thumbnail_used: 0,
                version,
            })
            .collect();
        Deck {
            files,
            fonts,
            registry,
            components,
            pictures: PictureCache::default(),
            files_generation: 0,
            theme: theme_key(&talk),
            shown: shown_slides_of(&talk),
            talk,
            backgrounds: HashMap::new(),
            memory_budget: DEFAULT_MEMORY_BUDGET,
            layers_asked_for: 0,
            last_asked: None,
            slides_holding_pictures: BTreeSet::new(),
            versions_made: slides.len() as u64,
            slides,
            placing: None,
            draw_jobs: Vec::new(),
            drawing: HashMap::new(),
        }
    }

    /// Takes the talk's new text and returns the slides that changed: those whose text
    /// differs from what was at the same place, or all of them if the talk's settings did,
    /// one of its picture files has been replaced or its components have changed, even with
    /// the text just as it was.
    /// A changed slide is laid out again when next asked for, but keeps the pixels of
    /// any element that stayed the same.
    pub fn update(&mut self, source: String) -> Vec<usize> {
        self.update_parsed(parse_talk(&source))
    }

    /// As [`Self::update`], with the talk's new text already read.
    pub fn update_parsed(&mut self, talk: Talk) -> Vec<usize> {
        let pictures_replaced = self.pictures.drop_replaced(&*self.files);
        let components_changed = self.components.rescan(&self.registry);
        let files_changed = pictures_replaced || components_changed;
        if files_changed {
            self.files_generation += 1;
        }
        self.take_talk(talk, files_changed, Matching::ByPlace)
    }

    /// Takes the talk as an edit made in the app left it, and returns the slides whose text
    /// differs from what was at the same place, or all of them if the talk's settings did. The
    /// talk's files are as they were, so none are looked at.
    ///
    /// Each slide is matched to the old ones by its text, from the start and then from the
    /// end, so that a slide added or taken out in the middle leaves every other slide its
    /// version, layout and drawn layers.
    pub fn update_text(&mut self, talk: Talk) -> Vec<usize> {
        self.take_talk(talk, false, Matching::FromBothEnds)
    }

    fn take_talk(&mut self, talk: Talk, files_changed: bool, matching: Matching) -> Vec<usize> {
        self.clear_placing();
        let same_look = !files_changed
            && settings_pairs(self.talk.slides.first().and_then(|s| s.settings.as_ref()))
                == settings_pairs(talk.slides.first().and_then(|s| s.settings.as_ref()));
        let texts = slide_texts(&talk);
        let changed: Vec<usize> = (0..texts.len())
            .filter(|&index| !same_look || self.slides.get(index).is_none_or(|old| old.text != texts[index]))
            .collect();
        let old_starts: Vec<usize> = self.talk.slides.iter().map(|slide| slide.range.start).collect();
        let (old_count, new_count) = (self.slides.len(), texts.len());
        // Matched from the end too, a slide comes from the old slide as far from the end as it is.
        let from_end = match matching {
            Matching::FromBothEnds if same_look => {
                let most = old_count.min(new_count);
                let from_start = (0..most).take_while(|&index| self.slides[index].text == texts[index]).count();
                (0..most - from_start).take_while(|&back| self.slides[old_count - 1 - back].text == texts[new_count - 1 - back]).count()
            }
            _ => 0,
        };
        let mut old: Vec<Option<SlideState>> = std::mem::take(&mut self.slides).into_iter().map(Some).collect();
        let mut slides = Vec::with_capacity(new_count);
        for (index, text) in texts.into_iter().enumerate() {
            let came_from = if index >= new_count - from_end {
                Some(index + old_count - new_count)
            } else {
                Some(index).filter(|&index| index < old_count - from_end)
            };
            let previous = came_from.and_then(|from| Some((from, old[from].take()?)));
            let start = talk.slides[index].range.start;
            match previous {
                Some((from, mut previous)) if same_look && previous.text == text => {
                    previous.moved(old_starts[from], start);
                    slides.push(previous);
                }
                previous => {
                    let previous = previous.map(|(_, previous)| previous);
                    let (last_used, thumbnail_used) = previous.as_ref().map_or((0, 0), |p| (p.last_used, p.thumbnail_used));
                    let mut drawn = previous.map(|p| p.drawn).unwrap_or_default();
                    for scale in drawn.values_mut() {
                        scale.slide_changed();
                    }
                    self.versions_made += 1;
                    let version = self.versions_made;
                    slides.push(SlideState { text, laid: None, at_moment: None, placed: None, drawn, thumbnail: None, last_used, thumbnail_used, version });
                }
            }
        }
        self.theme = theme_key(&talk);
        self.shown = shown_slides_of(&talk);
        self.talk = talk;
        self.slides_holding_pictures = slides
            .iter()
            .enumerate()
            .filter(|(_, slide)| slide.holds_pictures())
            .map(|(index, _)| index)
            .collect();
        self.slides = slides;
        changed
    }

    /// Looks at the talk's components again, and returns the slides to show again: all of
    /// them if anything about the components has changed, and none if not.
    pub fn refresh_components(&mut self) -> Vec<usize> {
        if !self.components.rescan(&self.registry) {
            return Vec::new();
        }
        self.files_generation += 1;
        for slide in &mut self.slides {
            slide.laid = None;
            slide.at_moment = None;
            slide.placed = None;
            slide.thumbnail = None;
            for scale in slide.drawn.values_mut() {
                scale.slide_changed();
            }
            self.versions_made += 1;
            slide.version = self.versions_made;
        }
        self.slides_holding_pictures.clear();
        (0..self.slides.len()).collect()
    }

    /// Reads the talk's files from `files` from now on, as when the talk has been saved
    /// somewhere new. Every slide is laid out again when next asked for, as nothing drawn
    /// from the old files is kept.
    pub fn set_files(&mut self, files: Box<dyn TalkFiles>) {
        self.clear_placing();
        self.files = Rc::from(files);
        self.components = TalkComponents::scan_for(self.files.clone(), &self.registry);
        self.pictures.files_replaced(&*self.files);
        self.files_generation += 1;
        for slide in &mut self.slides {
            slide.laid = None;
            slide.at_moment = None;
            slide.thumbnail = None;
            slide.drawn.clear();
            self.versions_made += 1;
            slide.version = self.versions_made;
        }
        self.slides_holding_pictures.clear();
    }

    /// Changes whenever any slide's text does, or how it's drawn: a slide's text changing always
    /// gives it a new version. For telling cheaply whether anything worked out from the text is
    /// still right.
    pub fn text_version(&self) -> u64 {
        self.versions_made
    }

    /// Takes in that files have been added to the talk's files, and returns the slides to show
    /// again: all of them, as any could name one, and a picture that wasn't there leaves no trace
    /// to find it by. Its components are looked at again too. Pictures read already are kept.
    pub fn files_added(&mut self) -> Vec<usize> {
        self.components.rescan(&self.registry);
        self.pictures.drop_replaced_stills(&*self.files);
        self.files_generation += 1;
        let talk = self.talk.clone();
        self.take_talk(talk, true, Matching::ByPlace)
    }

    /// The names of every component the talk can use: the built-ins and its own, sorted, for
    /// a palette to offer.
    pub fn available_components(&self) -> Vec<String> {
        let own = self.components.infos().iter().map(|info| info.tag.clone());
        let mut names: Vec<String> = self.registry.names().into_iter().map(String::from).chain(own).collect();
        names.sort();
        names.dedup();
        names
    }

    /// Whether `tag` is the name of a component every talk has, or of a tag the format itself
    /// reads, which a talk's own component can't take.
    pub fn is_built_in(&self, tag: &str) -> bool {
        crate::talk_components::is_reserved(tag, &self.registry)
    }

    /// The talk's own components.
    pub fn components(&self) -> &[ComponentInfo] {
        self.components.infos()
    }

    /// How many slides hold a layout at a moment other than their start.
    #[doc(hidden)]
    pub fn moments_held(&self) -> usize {
        self.slides.iter().filter(|slide| slide.at_moment.is_some()).count()
    }

    /// How many of the talk's assets are remembered as read by its components.
    #[doc(hidden)]
    pub fn assets_remembered(&self) -> usize {
        self.components.assets_remembered()
    }

    /// What's wrong with the talk's own components, as found when they were last looked at.
    pub fn component_problems(&self) -> &[Diagnostic] {
        self.components.problems()
    }

    /// What the component `tag` takes, as far as is known without loading anything: a
    /// built-in's, or one of the talk's own whose build is loaded already.
    pub fn loaded_schema(&self, tag: &str) -> SchemaLoad {
        if let Some(component) = self.registry.get(tag) {
            return SchemaLoad::Loaded((component.schema)());
        }
        match self.components.loaded_schema(tag) {
            None => SchemaLoad::NotLoaded,
            Some(None) => SchemaLoad::Failed,
            Some(Some(schema)) => SchemaLoad::Loaded(schema.clone()),
        }
    }

    /// Whether `tag` is one of the talk's components whose build is loaded and working and
    /// takes content, so that what's written between its tags is a box of text of its own.
    /// Nothing is loaded to find out: a component not loaded yet, with no build, that
    /// doesn't work, or no component at all, takes none. The text layout and whoever edits
    /// the talk read its flows with this, as [`slides_format::text_flows_with`] does.
    pub fn takes_content(&self, tag: &str) -> bool {
        self.registry.get(tag).is_none() && self.components.takes_content(tag)
    }

    /// How many of the talk's modules have been loaded, or tried, for tests.
    #[doc(hidden)]
    pub fn modules_loaded(&self) -> usize {
        self.components.loads()
    }

    /// What the component `tag` takes, for a form: a built-in, or one of the talk's own.
    pub fn component_schema(&mut self, tag: &str) -> Option<ComponentSchema> {
        match self.registry.get(tag) {
            Some(component) => Some((component.schema)()),
            None => self.components.schema(tag),
        }
    }

    /// The size, in slide units, a new `tag` with `attrs` is given: a picture its own size,
    /// made smaller to fit in half the slide, an icon the size it asks for, and anything
    /// else a box of a third of the slide's width.
    pub fn default_size(&mut self, tag: &str, attrs: &[(&str, &str)]) -> (f32, f32) {
        const ROOM: (f64, f64) = (960.0, 540.0);
        const OTHERWISE: (f32, f32) = (640.0, 360.0);
        let attr = |name: &str| attrs.iter().find(|(key, _)| *key == name).map(|(_, value)| *value);
        match tag {
            "Picture" => {
                let size = attr("src").and_then(|src| picture_size(src, &self.files.read(src)?));
                let Some((w, h)) = size.map(|(w, h)| (f64::from(w), f64::from(h))) else { return OTHERWISE };
                let (w, h) = if w <= ROOM.0 && h <= ROOM.1 {
                    (w, h)
                } else if w * ROOM.1 >= h * ROOM.0 {
                    (ROOM.0, h * ROOM.0 / w)
                } else {
                    (w * ROOM.1 / h, ROOM.1)
                };
                (w as f32, h as f32)
            }
            "Icon" => {
                let size = attr("size").and_then(|size| size.trim().parse::<f32>().ok()).filter(|size| size.is_finite() && *size > 0.0);
                let size = size.unwrap_or(DEFAULT_ICON_SIZE).min(MAX_SIZE);
                (size, size)
            }
            _ => OTHERWISE,
        }
    }

    /// How many pixel bytes the drawn layers of the whole deck can add up to, from the
    /// next call to `layers` on.
    pub fn set_memory_budget(&mut self, bytes: usize) {
        self.memory_budget = bytes;
    }

    /// How many pixel bytes the pictures of the talk can add up to, from the next slide laid
    /// out on. The slide being asked for keeps its pictures, however much they take alone.
    pub fn set_picture_budget(&mut self, bytes: usize) {
        self.pictures.set_memory_budget(bytes);
    }

    /// Leaves videos' stills to be made aside from now on, as the window's thread mustn't decode
    /// a video: `wanted` is told, on the thread laying a slide out, each time one is wanted, and
    /// [`Self::still_jobs`] then gives it, to be made on another thread and taken in with
    /// [`Self::take_still`]. Until then a video shows a plain panel, as large as `measure`, which
    /// decodes nothing, says the video is. Without this, a still is made on the thread that lays
    /// its slide out.
    pub fn make_stills_aside(&mut self, wanted: StillWanted, measure: MeasureVideo) {
        self.pictures.make_stills_aside(wanted, measure);
    }

    /// The stills wanted to be made aside since last asked.
    pub fn still_jobs(&mut self) -> Vec<StillJob> {
        self.pictures.still_jobs()
    }

    /// Takes in a still made aside, and returns the slides showing it, which are laid out again
    /// when next asked for.
    pub fn take_still(&mut self, made: MadeStill) -> Vec<usize> {
        let Some(made) = self.pictures.take_still(made) else {
            return Vec::new();
        };
        let mut changed = Vec::new();
        for (index, slide) in self.slides.iter_mut().enumerate() {
            let at_moment = slide.at_moment.as_ref().map(|(_, laid)| laid);
            let shows = slide.laid.iter().chain(at_moment).chain(&slide.placed).any(|laid| laid.stills.contains(&made));
            if !shows {
                continue;
            }
            slide.laid = None;
            slide.at_moment = None;
            slide.placed = None;
            slide.thumbnail = None;
            for scale in slide.drawn.values_mut() {
                scale.slide_changed();
            }
            self.versions_made += 1;
            slide.version = self.versions_made;
            changed.push(index);
        }
        for index in &changed {
            self.slides_holding_pictures.remove(index);
        }
        changed
    }

    /// Whether any video's still is being made aside.
    pub fn stills_coming(&self) -> bool {
        self.pictures.stills_coming()
    }

    /// The plain sentence saying why the video at `source` on slide `index` can't be played, if
    /// it's a video and its still has been tried and it can't. It's among the slide's notices too.
    pub fn video_problem(&mut self, index: usize, source: &std::ops::Range<usize>) -> Option<String> {
        if index >= self.slides.len() {
            return None;
        }
        let src = self.specs(index).iter().find(|spec| spec.source.as_ref() == Some(source))?.video.as_ref()?.src.clone();
        self.pictures.still_problem(&src).map(|why| why.to_string())
    }

    /// Draws the talk's 3D models' stills with `draw` from now on: the engine draws no 3D of its
    /// own. Until it's given something to draw with, a model is a panel saying it can't be drawn
    /// here. With stills made aside, `draw` is called by [`StillJob::make`], on whatever thread
    /// makes them.
    pub fn draw_models_with(&mut self, draw: DrawModel) {
        self.pictures.draw_models_with(draw);
    }

    /// The plain sentence saying why the 3D model at `source` on slide `index` can't be drawn, if
    /// it's a model and its still has been tried and it can't. It's among the slide's notices too.
    pub fn model_problem(&mut self, index: usize, source: &std::ops::Range<usize>) -> Option<String> {
        if index >= self.slides.len() {
            return None;
        }
        let on = self.specs(index).iter().find(|spec| spec.source.as_ref() == Some(source))?.model.clone()?;
        if on.src.is_empty() {
            return Some(crate::MODEL_NO_FILE.to_string());
        }
        let size = crate::pictures::model_still_size(on.rect.w, on.rect.h);
        self.pictures.model_problem(&on.src, on.view, size).map(|why| why.to_string())
    }

    /// The plain sentence saying why the terminal recording at `source` on slide `index` can't be
    /// played, if it's a recording and it's been read and can't be, or it has no file. It's among
    /// the slide's notices too.
    pub fn terminal_problem(&mut self, index: usize, source: &std::ops::Range<usize>) -> Option<String> {
        if index >= self.slides.len() {
            return None;
        }
        let on = self.specs(index).iter().find(|spec| spec.source.as_ref() == Some(source))?.terminal.clone()?;
        let src = on.src;
        // One that names no recording is a mistake if it has no usable command either, and has
        // no file to be read if it has.
        if src.is_empty() {
            return crate::terminal_complaint(&src, on.run.as_deref(), on.cols, on.rows);
        }
        self.pictures.terminal_problem(&src).map(|why| why.to_string())
    }

    /// The plain sentence saying why the command or screen of the terminal at `source` on slide
    /// `index` is wrong, if it is. A terminal that can't run is still shown as its recording or
    /// panel, so this is apart from [`Self::terminal_problem`], which is about showing it.
    pub fn terminal_run_problem(&mut self, index: usize, source: &std::ops::Range<usize>) -> Option<String> {
        if index >= self.slides.len() {
            return None;
        }
        let on = self.specs(index).iter().find(|spec| spec.source.as_ref() == Some(source))?.terminal.clone()?;
        crate::terminal_screen::terminal_run_complaint(on.run.as_deref(), on.cols, on.rows)
    }

    /// Whether a terminal recording on slide `index` is still being read aside: until it's read,
    /// it's a plain panel, and its stops aren't among the slide's steps.
    pub fn terminal_pending(&mut self, index: usize) -> bool {
        if index >= self.slides.len() {
            return false;
        }
        let sources: Vec<String> = self.specs(index).iter().filter_map(|spec| Some(spec.terminal.as_ref()?.src.clone())).collect();
        sources.iter().any(|src| self.pictures.terminal_coming(src))
    }

    /// The terminal recordings on slide `index` in its whole state, among its layers as `specs`
    /// and `layers` give them, with the deck's placing.
    pub fn terminals(&mut self, index: usize) -> Vec<TerminalLayer> {
        if index >= self.slides.len() {
            return Vec::new();
        }
        terminals_of(self.specs(index))
    }

    /// The terminal recordings on slide `index` at `moment`, among its layers as `layers_at`
    /// gives them then.
    pub fn terminals_at(&mut self, index: usize, moment: Moment) -> Vec<TerminalLayer> {
        if index >= self.slides.len() {
            return Vec::new();
        }
        let moment = self.ensure_laid_at(index, moment);
        terminals_of(&self.slides[index].laid_at(moment).specs)
    }

    /// The 3D models on slide `index` in its whole state, among its layers as `specs` and
    /// `layers` give them, with the deck's placing.
    pub fn models(&mut self, index: usize) -> Vec<ModelLayer> {
        if index >= self.slides.len() {
            return Vec::new();
        }
        models_of(self.specs(index))
    }

    /// The 3D models on slide `index` at `moment`, among its layers as `layers_at` gives them then.
    pub fn models_at(&mut self, index: usize, moment: Moment) -> Vec<ModelLayer> {
        if index >= self.slides.len() {
            return Vec::new();
        }
        let moment = self.ensure_laid_at(index, moment);
        models_of(&self.slides[index].laid_at(moment).specs)
    }

    /// The buttons on slide `index` at `moment`, among its layers as `layers_at` gives them then.
    pub fn buttons_at(&mut self, index: usize, moment: Moment) -> Vec<ButtonLayer> {
        if index >= self.slides.len() {
            return Vec::new();
        }
        let moment = self.ensure_laid_at(index, moment);
        buttons_of(&self.slides[index].laid_at(moment).specs)
    }

    /// The interactive components on slide `index` at `moment`, among its layers as `layers_at`
    /// gives them then.
    pub fn components_at(&mut self, index: usize, moment: Moment) -> Vec<ComponentLayer> {
        if index >= self.slides.len() {
            return Vec::new();
        }
        let moment = self.ensure_laid_at(index, moment);
        components_of(&self.slides[index].laid_at(moment).specs)
    }

    /// What each interactive component, by where its tag is written, is given when it's drawn at a
    /// moment whose state isn't 0. A moment with other feeds has another state.
    pub fn feed_components(&mut self, feeds: HashMap<std::ops::Range<usize>, ComponentFeed>) {
        self.components.set_feeds(feeds);
    }

    /// What came of drawing interactive components from their feeds since this was last asked.
    pub fn take_component_answers(&mut self) -> Vec<ComponentAnswer> {
        self.components.take_answers()
    }

    /// The plain sentence saying why the button at `source` on slide `index` does nothing, or
    /// what it names that isn't there, if it's a button and either is so. It's among the slide's
    /// notices too.
    pub fn button_problem(&mut self, index: usize, source: &std::ops::Range<usize>) -> Option<String> {
        if index >= self.slides.len() {
            return None;
        }
        let moment = self.ensure_whole(index);
        let laid = self.slides[index].laid_at(moment);
        let on = laid.specs.iter().find(|spec| spec.source.as_ref() == Some(source))?.button.as_ref()?;
        match &on.action {
            Err(fault) => Some(crate::button_fault_notice(*fault).to_string()),
            Ok(_) => crate::slide::button_target_problem(on, &laid.specs, laid.steps()).map(str::to_string),
        }
    }

    /// Whether the video `src`, with the still `poster` names if it names one, is drawn as a
    /// picture once its slide is laid out: its first picture, or that still, could be shown.
    /// What laying the slide out found is what's said: ask once it's laid out. A still made and
    /// let go of since, to keep within the budget, was a picture.
    pub fn video_shows_a_picture(&mut self, src: &str, poster: Option<&str>) -> bool {
        let src = src.trim();
        let made = !src.is_empty() && self.pictures.still_problem(src).is_none();
        let poster = poster.map(str::trim).filter(|poster| !poster.is_empty());
        made || poster.is_some_and(|poster| self.pictures.get(poster, &*self.files).is_some())
    }

    /// The videos on slide `index` in its whole state, among its layers as `specs` and `layers`
    /// give them, with the deck's placing.
    pub fn videos(&mut self, index: usize) -> Vec<VideoLayer> {
        if index >= self.slides.len() {
            return Vec::new();
        }
        videos_of(self.specs(index))
    }

    /// The videos on slide `index` at `moment`, among its layers as `layers_at` gives them then.
    pub fn videos_at(&mut self, index: usize, moment: Moment) -> Vec<VideoLayer> {
        if index >= self.slides.len() {
            return Vec::new();
        }
        let moment = self.ensure_laid_at(index, moment);
        videos_of(&self.slides[index].laid_at(moment).specs)
    }

    /// The pixel bytes of the talk's pictures in memory. A picture many slides show counts once.
    pub fn picture_bytes(&self) -> usize {
        self.pictures.bytes()
    }

    /// The pixel bytes the deck is holding in drawn layers. A layer the slides share counts once.
    /// A pixmap that's moved, and so shown by two layers, counts once too.
    pub fn drawn_bytes(&self) -> usize {
        let slide_layers = self.slides.iter().flat_map(|slide| slide.drawn.values()).flat_map(Drawn::layers);
        let mut counted = HashSet::new();
        let layers: usize = slide_layers
            .chain(self.backgrounds.values())
            .filter(|layer| counted.insert(Arc::as_ptr(&layer.pixmap)))
            .map(|layer| layer.pixmap.data().len())
            .sum();
        let thumbnails: usize = self.slides.iter().filter_map(|slide| slide.thumbnail.as_ref()).map(|(_, pixmap)| pixmap.data().len()).sum();
        layers + thumbnails
    }

    pub fn slide_count(&self) -> usize {
        self.slides.len()
    }

    /// Whether slide `index` is left out of shows and PDFs. A slide that isn't in the talk isn't.
    pub fn is_hidden(&self, index: usize) -> bool {
        self.talk.slides.get(index).is_some_and(is_hidden)
    }

    /// The slides a show goes through, in order: all but the hidden ones, or all of them if
    /// every slide is hidden, so that a show is never empty. It's worked out as the talk is
    /// read, so asking costs nothing.
    pub fn shown_slides(&self) -> &[usize] {
        &self.shown
    }

    /// The whole slide, its last step, drawn onto one pixmap `width` wide and 9/16 as tall, to
    /// stand for the slide in a list, or `None` if the talk has no slide `index`. It is kept
    /// until the slide changes, and the slides asked for longest ago give theirs up when the
    /// deck is over its memory budget.
    pub fn thumbnail(&mut self, index: usize, width: u32) -> Option<Arc<Pixmap>> {
        if index >= self.slides.len() {
            return None;
        }
        let width = width.clamp(1, MAX_THUMBNAIL_WIDTH);
        self.layers_asked_for += 1;
        self.slides[index].thumbnail_used = self.layers_asked_for;
        if let Some((made_for, pixmap)) = &self.slides[index].thumbnail
            && *made_for == width
        {
            return Some(pixmap.clone());
        }
        let moment = self.ensure_whole(index);
        let height = (u64::from(width) * 9 / 16).max(1) as u32;
        let scale = width as f32 / SLIDE_WIDTH;
        let mut pixmap = Pixmap::new(width, height).expect("a thumbnail has an area and a modest size");
        // Each layer is drawn and put on at once rather than kept: a thumbnail is small, and the
        // layers of the slide at the size the editor shows it are not to be pushed out by it.
        for spec in &self.slides[index].laid_at(moment).specs {
            compose(&mut pixmap, &[render_layer(spec, scale, &mut self.fonts)]);
        }
        let pixmap = Arc::new(pixmap);
        self.slides[index].thumbnail = Some((width, pixmap.clone()));
        self.keep_within_budget(index);
        Some(pixmap)
    }

    /// What slide `index` is at the moment, as a number that changes whenever anything that
    /// decides how it looks does, or `None` if the talk has no such slide. A picture of the
    /// slide made at one version shows it as long as the version stays.
    pub fn slide_version(&self, index: usize) -> Option<u64> {
        self.slides.get(index).map(|slide| slide.version)
    }

    /// Slide `index`'s thumbnail `width` wide, if one has been drawn since the slide last
    /// changed. Asking draws nothing, so a list can find what it still has to draw.
    pub fn drawn_thumbnail(&self, index: usize, width: u32) -> Option<Arc<Pixmap>> {
        let width = width.clamp(1, MAX_THUMBNAIL_WIDTH);
        match &self.slides.get(index)?.thumbnail {
            Some((made_for, pixmap)) if *made_for == width => Some(pixmap.clone()),
            _ => None,
        }
    }

    pub fn source(&self) -> &str {
        &self.talk.source
    }

    /// The talk as it was last read: its slides, and where each sits in the text.
    pub fn talk(&self) -> &Talk {
        &self.talk
    }

    pub fn title(&self) -> Option<String> {
        self.talk.slides.first()?.settings.as_ref()?.get("title").map(str::to_string)
    }

    /// What the slide says about itself at its start.
    pub fn notices(&mut self, index: usize) -> Vec<String> {
        self.ensure_laid(index);
        self.slides[index].laid.as_ref().map(|laid| laid.notices.clone()).unwrap_or_default()
    }

    /// What the slide says about itself in its whole state, which `specs` and `layers` draw.
    pub fn notices_whole(&mut self, index: usize) -> Vec<String> {
        let moment = self.ensure_whole(index);
        self.slides[index].laid_at(moment).notices.clone()
    }

    /// The slide's notes, if it has any.
    pub fn notes(&self, index: usize) -> Option<String> {
        self.talk.slides[index].notes.as_ref().map(|notes| notes.text.clone())
    }

    /// When each layer of the slide appears, in the order of `layers`, and how the slide is arrived at.
    pub fn timeline(&mut self, index: usize) -> SlideTimeline {
        self.ensure_laid(index);
        let laid = self.slides[index].laid.as_ref().expect("the slide was just laid out");
        SlideTimeline { steps: laid.steps(), layers: laid.timings(), transition: laid.transition, animated: laid.asks.animated }
    }

    /// When each layer of the slide at `moment` appears, in the order of `layers_at` at that
    /// moment. A component can give a layer at one step and none at another, so the layers at a
    /// moment needn't be those of `timeline`.
    pub fn timings_at(&mut self, index: usize, moment: Moment) -> Vec<LayerTiming> {
        let moment = self.ensure_laid_at(index, moment);
        self.slides[index].laid_at(moment).timings()
    }

    /// The components on the slide that change as the talk plays, once each.
    pub fn varying_tags(&mut self, index: usize) -> Vec<String> {
        self.ensure_laid(index);
        self.slides[index].laid.as_ref().expect("the slide was just laid out").asks.tags.clone()
    }

    /// What each layer of the slide draws in its whole state, before it is drawn at any scale,
    /// with the deck's placing. [`Self::specs_fonts_and_files`] never shows a placing.
    pub fn specs(&mut self, index: usize) -> &[LayerSpec] {
        if self.ensure_placed(index) {
            return &self.slides[index].placed.as_ref().expect("the slide was just laid out").specs;
        }
        let moment = self.ensure_whole(index);
        &self.slides[index].laid_at(moment).specs
    }

    /// Shows the things and regions `placing` names on slide `slide` where it says, as if their
    /// source said so, until it's cleared: in `specs`, `layers`, `editor_layers` and
    /// `text_layout`. The source, the slide's version, its thumbnail, `layers_at` and what's
    /// exported don't change. A placing for anything that isn't a placed tag at the top level
    /// of the slide or a region of its layout is ignored. A layer only moved isn't drawn again:
    /// its pixmap is shown at the new place.
    pub fn set_placing(&mut self, slide: usize, placing: Vec<Placing>) {
        if self.placing.as_ref().is_some_and(|(at, now)| *at == slide && *now == placing) {
            return;
        }
        match self.placing.take() {
            Some((at, _)) if at != slide => self.forget_placing(at),
            _ => {}
        }
        let Some(state) = self.slides.get_mut(slide) else { return };
        state.placed = None;
        for drawn in state.drawn.values_mut() {
            drawn.placing_changed();
        }
        self.placing = Some((slide, placing));
    }

    /// Shows every slide as its source says again.
    pub fn clear_placing(&mut self) {
        if let Some((slide, _)) = self.placing.take() {
            self.forget_placing(slide);
        }
    }

    /// The slide that has a placing, and the placing.
    pub fn placing(&self) -> Option<(usize, &[Placing])> {
        self.placing.as_ref().map(|(slide, placing)| (*slide, placing.as_slice()))
    }

    fn forget_placing(&mut self, slide: usize) {
        if let Some(state) = self.slides.get_mut(slide) {
            state.placed = None;
            for drawn in state.drawn.values_mut() {
                drawn.placing_cleared();
            }
            if !state.holds_pictures() {
                self.slides_holding_pictures.remove(&slide);
            }
        }
    }

    /// The steps slide `slide` takes that no `click` can renumber: those of a region shown a
    /// piece at a time, and of a component that steps. 0 for a slide the talk doesn't have.
    pub fn fixed_steps(&mut self, slide: usize) -> u32 {
        if slide >= self.slides.len() {
            return 0;
        }
        self.ensure_laid(slide);
        let laid = self.slides[slide].laid.as_ref().expect("the slide was just laid out");
        laid.each_steps.max(laid.asks.steps).min(MAX_CLICK)
    }

    /// The steps region `region` of slide `slide` takes, shown a piece at a time from click 1:
    /// one for a region turned, which shows whole on click 1. `None` for a region not shown
    /// that way, one with nothing in it, or a slide the talk doesn't have.
    pub fn each_steps(&mut self, slide: usize, region: &str) -> Option<u32> {
        if slide >= self.slides.len() {
            return None;
        }
        self.ensure_laid(slide);
        let laid = self.slides[slide].laid.as_ref().expect("the slide was just laid out");
        laid.each_regions.iter().find(|(name, _)| name == region).map(|(_, steps)| *steps)
    }

    /// A slide's specs in its whole state, with what they are drawn from: the fonts, and the talk's
    /// files. They're always as the source says: unlike [`Self::specs`], never with a placing.
    pub fn specs_fonts_and_files(&mut self, index: usize) -> (&[LayerSpec], &mut Fonts, &dyn TalkFiles) {
        let moment = self.ensure_whole(index);
        (&self.slides[index].laid_at(moment).specs, &mut self.fonts, &*self.files)
    }

    /// A slide's specs at `moment`, as [`Self::layers_at`] draws them, with the fonts they are
    /// drawn with: for drawing one part of the slide as it is at a click or a time. Never with a
    /// placing.
    pub fn specs_and_fonts_at(&mut self, index: usize, moment: Moment) -> (&[LayerSpec], &mut Fonts) {
        let moment = self.ensure_laid_at(index, moment);
        (&self.slides[index].laid_at(moment).specs, &mut self.fonts)
    }

    /// The talk's look.
    pub fn theme(&self) -> toolkit::Theme {
        talk_theme(&self.talk)
    }

    pub fn fonts_mut(&mut self) -> &mut Fonts {
        &mut self.fonts
    }

    /// The slide's layers in its whole state, first to last, drawn at `scale`: its last step, at
    /// time 0, with the deck's placing. Under a placing they needn't be as many as
    /// `timeline().layers`, which is the source's: a region shown a piece at a time and turned by
    /// the placing is one layer. So the two are only to be paired with no placing.
    pub fn layers(&mut self, index: usize, scale: f32) -> Vec<Arc<Layer>> {
        if self.ensure_placed(index) {
            return self.placed_layers(index, scale);
        }
        let whole = self.ensure_whole(index);
        self.layers_at(index, whole, scale)
    }

    /// The slide's layers in its whole state, as `layers` gives them, and then, for the editor
    /// alone, what it shows in each region of the layout with nothing in it: an offer to add
    /// a title or text there. A show, a thumbnail and an export never have those. Like `layers`,
    /// under a placing they needn't match `timeline().layers`.
    pub fn editor_layers(&mut self, index: usize, scale: f32) -> Vec<Arc<Layer>> {
        if index >= self.slides.len() {
            return Vec::new();
        }
        let mut layers = self.layers(index, scale);
        let empty = self.empty_region_layers(index, scale);
        let empty = if self.ensure_placed(index) { self.placed_empty_region_layers(index, scale, empty) } else { empty };
        layers.extend(empty);
        layers
    }

    /// The slide's layers as `editor_layers` gives them, but with its interactive components drawn
    /// from what they're fed, at `state`, as the editor draws what it tries. With a state of 0, or
    /// with the deck's placing here, just `editor_layers`.
    pub fn editor_layers_in(&mut self, index: usize, state: u64, scale: f32) -> Vec<Arc<Layer>> {
        if state == 0 || index >= self.slides.len() || self.ensure_placed(index) {
            return self.editor_layers(index, scale);
        }
        let whole = self.ensure_whole(index);
        let mut layers = self.layers_at(index, Moment { state, ..whole }, scale);
        layers.extend(self.empty_region_layers(index, scale));
        layers
    }

    /// What the editor shows in slide `index`'s empty regions, as its source has them, drawn at `scale`.
    fn empty_region_layers(&mut self, index: usize, scale: f32) -> Vec<Arc<Layer>> {
        let drawn = self.slides[index].drawn.entry(scale.to_bits()).or_default();
        if let Some(empty) = &drawn.empty_regions {
            return empty.clone();
        }
        let moment = self.ensure_whole(index);
        let theme = talk_theme(&self.talk);
        let regions = &self.slides[index].laid_at(moment).empty_regions;
        let empty: Vec<Arc<Layer>> = regions
            .iter()
            .map(|region| Arc::new(render_layer(&empty_region_layer(region, &theme, &mut self.fonts), scale, &mut self.fonts)))
            .collect();
        self.slides[index].drawn.entry(scale.to_bits()).or_default().empty_regions = Some(empty.clone());
        self.keep_within_budget(index);
        empty
    }

    /// What the editor shows in slide `index`'s empty regions with the deck's placing, drawn at
    /// `scale`, from `own`, what it shows as the source has them: one that hasn't moved is as it
    /// was, one only moved is shown at its new place, and any other is drawn again.
    fn placed_empty_region_layers(&mut self, index: usize, scale: f32, own: Vec<Arc<Layer>>) -> Vec<Arc<Layer>> {
        if let Some(empty) = &self.slides[index].drawn.entry(scale.to_bits()).or_default().placed_empty_regions {
            return empty.clone();
        }
        let moment = self.ensure_whole(index);
        let theme = talk_theme(&self.talk);
        let slide = &self.slides[index];
        let (base, placed) = (&slide.laid_at(moment).empty_regions, &slide.placed.as_ref().expect("the slide was just laid out").empty_regions);
        let mut empty = Vec::with_capacity(placed.len());
        for region in placed {
            let was = base.iter().zip(&own).find(|(was, _)| was.name == region.name);
            let layer = match was {
                Some((was, layer)) if was.rect == region.rect && was.rotate == region.rotate => layer.clone(),
                Some((was, layer)) if (was.rect.w, was.rect.h, was.rotate) == (region.rect.w, region.rect.h, region.rotate) => {
                    DrawnAt::of(layer, was.rect.x, was.rect.y).shown_at(&layer.pixmap, region.rect.x, region.rect.y, scale)
                }
                _ => Arc::new(render_layer(&empty_region_layer(region, &theme, &mut self.fonts), scale, &mut self.fonts)),
            };
            empty.push(layer);
        }
        self.slides[index].drawn.entry(scale.to_bits()).or_default().placed_empty_regions = Some(empty.clone());
        self.keep_within_budget(index);
        empty
    }

    /// What `editor_layers` adds to the slide's own layers, before it's drawn.
    #[doc(hidden)]
    pub fn placeholder_specs(&mut self, index: usize) -> Vec<LayerSpec> {
        if index >= self.slides.len() {
            return Vec::new();
        }
        let moment = self.ensure_whole(index);
        let theme = talk_theme(&self.talk);
        let regions = self.slides[index].laid_at(moment).empty_regions.clone();
        regions.iter().map(|region| empty_region_layer(region, &theme, &mut self.fonts)).collect()
    }

    /// Where slide `index`'s editable text is drawn in its whole state, as `layers` draws it:
    /// for a caret, a selection and a click. It's kept with the slide's layout, and made
    /// again only once the slide has changed. A slide the talk doesn't have has no text.
    pub fn text_layout(&mut self, index: usize) -> Arc<TextLayout> {
        if index >= self.slides.len() {
            return Arc::new(TextLayout::default());
        }
        let placed = self.ensure_placed(index);
        let moment = self.ensure_whole(index);
        let (talk, components) = (&self.talk, &self.components);
        let generation = components.loaded_generation();
        let slide = &mut self.slides[index];
        let laid = if placed { slide.placed.as_mut().expect("the slide was just laid out") } else { slide.laid_at_mut(moment) };
        if let Some((built_for, layout)) = &laid.text_layout
            && *built_for == generation
        {
            return layout.clone();
        }
        let takes_content = |tag: &str| components.takes_content(tag);
        let layout = Arc::new(TextLayout::build(talk, index, &laid.specs, &laid.texts, &laid.empty_regions, &laid.regions, &takes_content));
        laid.text_layout = Some((generation, layout.clone()));
        layout
    }

    /// Every source range slide `index`'s text geometry is read from, in order, for tests.
    #[doc(hidden)]
    pub fn drawn_text_sources(&mut self, index: usize) -> Vec<std::ops::Range<usize>> {
        if index >= self.slides.len() {
            return Vec::new();
        }
        let moment = self.ensure_whole(index);
        self.slides[index].laid_at(moment).texts.iter().flat_map(LayerText::sources).collect()
    }

    /// The slide's layers at `moment`, first to last, drawn at `scale`.
    pub fn layers_at(&mut self, index: usize, moment: Moment, scale: f32) -> Vec<Arc<Layer>> {
        self.keep_moments_of(index);
        let moment = self.ensure_laid_at(index, moment);
        self.layers_asked_for += 1;
        self.slides[index].last_used = self.layers_asked_for;
        let layers = self.drawn_layers(index, moment, scale);
        self.keep_within_budget(index);
        layers
    }

    /// Forgets the layouts at a moment of every slide but `index` and the one asked for just
    /// before it, so that a show, with its current slide and the next one, holds two.
    fn keep_moments_of(&mut self, index: usize) {
        let before = self.last_asked.replace(index);
        if before == Some(index) {
            return;
        }
        for (other, slide) in self.slides.iter_mut().enumerate() {
            if other != index && Some(other) != before && slide.at_moment.take().is_some() && !slide.holds_pictures() {
                self.slides_holding_pictures.remove(&other);
            }
        }
    }

    /// Forgets what's drawn until the deck is within its budget: first the thumbnails, the one
    /// asked for longest ago first, as they are small and quick to draw again, and then whole
    /// slides' drawn layers, the slide asked for longest ago first. Slide `requested` keeps
    /// what it has, however much it takes alone, and so do the layers of the slide asked for
    /// last, which is the one on screen.
    fn keep_within_budget(&mut self, requested: usize) {
        let on_screen = self.last_asked;
        self.drop_unused_backgrounds();
        while self.drawn_bytes() > self.memory_budget {
            let thumbnails = self
                .slides
                .iter_mut()
                .enumerate()
                .filter(|(index, slide)| *index != requested && slide.thumbnail.is_some());
            let Some((_, oldest)) = thumbnails.min_by_key(|(_, slide)| slide.thumbnail_used) else {
                break;
            };
            oldest.thumbnail = None;
        }
        loop {
            self.drop_unused_backgrounds();
            if self.drawn_bytes() <= self.memory_budget {
                return;
            }
            let others = self
                .slides
                .iter_mut()
                .enumerate()
                .filter(|(index, slide)| *index != requested && Some(*index) != on_screen && !slide.drawn.is_empty());
            let Some((_, oldest)) = others.min_by_key(|(_, slide)| slide.last_used) else {
                return;
            };
            oldest.drawn.clear();
        }
    }

    /// A background is kept while it's the talk's theme and some slide is drawn at its scale.
    fn drop_unused_backgrounds(&mut self) {
        let (slides, theme) = (&self.slides, &self.theme);
        self.backgrounds.retain(|(background_theme, scale), _| {
            background_theme == theme && slides.iter().any(|slide| slide.drawn.contains_key(scale))
        });
    }

    /// The layers of slide `index` at `moment`, which `ensure_laid_at` gave.
    fn drawn_layers(&mut self, index: usize, moment: Moment, scale: f32) -> Vec<Arc<Layer>> {
        let slide = &mut self.slides[index];
        make_room(&mut slide.drawn, scale.to_bits());
        let SlideState { laid, at_moment, drawn: scales, .. } = slide;
        let laid = match at_moment {
            Some((at, laid)) if *at == moment => &*laid,
            _ => laid.as_ref().expect("the slide was just laid out"),
        };
        let drawn = scales.entry(scale.to_bits()).or_default();
        drawn.asked = self.layers_asked_for;
        if let Some((at, ordered)) = &drawn.ordered
            && *at == moment
        {
            return ordered.clone();
        }
        // Apart from the others while it's drawn, as a picture at another scale can stand in.
        let mut drawn = scales.remove(&scale.to_bits()).unwrap_or_default();
        let (mut by_key, mut standing) = (HashMap::new(), HashMap::new());
        let mut ordered = Vec::with_capacity(laid.specs.len());
        let aside = self.pictures.makes_aside();
        let mut wanted = false;
        for (spec, key) in laid.specs.iter().zip(&laid.keys) {
            let layer = if spec.source.is_some() {
                match by_key.get(key).or_else(|| drawn.by_key.get(key)).cloned() {
                    Some(layer) => {
                        by_key.insert(key.clone(), layer.clone());
                        layer
                    }
                    // A screen still shaped and placed aside, or too long to draw here and drawn
                    // aside, has what was drawn of it before stand in, which isn't kept as it; the
                    // picture it's made from is kept until it's drawn.
                    None if spec.terminal.as_ref().is_some_and(|on| on.pending) || (aside && spec.terminal.is_some() && pieces_drawn(spec, scale) > MOST_PIECES_DRAWN_HERE) => {
                        if !spec.terminal.as_ref().is_some_and(|on| on.pending) {
                            wanted |= want_drawn(&mut self.draw_jobs, &mut self.drawing, &mut self.fonts, (index, scale, key, spec));
                        }
                        let (layer, from) = standing_in(&drawn, scales, key, spec, scale, &mut self.fonts);
                        by_key.extend(from);
                        standing.insert(key.clone(), layer.clone());
                        layer
                    }
                    None => {
                        let layer = Arc::new(render_layer(spec, scale, &mut self.fonts));
                        by_key.insert(key.clone(), layer.clone());
                        layer
                    }
                }
            } else {
                self.backgrounds
                    .entry((self.theme.clone(), scale.to_bits()))
                    .or_insert_with(|| Arc::new(render_layer(spec, scale, &mut self.fonts)))
                    .clone()
            };
            ordered.push(layer);
        }
        drawn.by_key = by_key;
        drawn.standing = standing;
        drawn.ordered = Some((moment, ordered.clone()));
        scales.insert(scale.to_bits(), drawn);
        if wanted {
            self.pictures.tell_wanted();
        }
        ordered
    }

    /// The layers wanted drawn aside since last asked, as stills are made: those of terminal
    /// recordings whose screens take too long to draw on the thread asking for them. Until each is
    /// taken in, with [`Self::take_drawn`], what it was drawn as before, stretched, or its plain
    /// panel, stands in for it. Only while stills are made aside.
    pub fn draw_jobs(&mut self) -> Vec<DrawJob> {
        std::mem::take(&mut self.draw_jobs)
    }

    /// Takes in a layer drawn aside, and gives the slide it's on if it's still shown there, to be
    /// drawn again.
    pub fn take_drawn(&mut self, drawn: DrawnLayer) -> Vec<usize> {
        let DrawnLayer { slide, scale, key, layer, .. } = drawn;
        self.drawing.remove(&(slide, scale, key.clone()));
        let Some(state) = self.slides.get_mut(slide) else { return Vec::new() };
        let at_moment = state.at_moment.as_ref().map(|(_, laid)| laid);
        if !state.laid.iter().chain(at_moment).chain(&state.placed).any(|laid| laid.keys.contains(&key)) {
            return Vec::new();
        }
        // A scale let go of since isn't brought back.
        let Some(drawn) = state.drawn.get_mut(&scale) else { return Vec::new() };
        drawn.by_key.insert(key, layer);
        drawn.ordered = None;
        drawn.placing_changed();
        vec![slide]
    }

    /// Forgets how the slides furthest from slide `asked` are laid out, until the pictures in
    /// memory are within budget. Each of those is laid out again when next needed. A laid-out
    /// slide holds its pictures, so the cache can't free them by itself.
    fn keep_pictures_within_budget(&mut self, asked: usize) {
        self.pictures.let_go_of_unused();
        while self.pictures.is_over_budget() {
            let ends = [self.slides_holding_pictures.first(), self.slides_holding_pictures.last()];
            let others = ends.into_iter().flatten().copied().filter(|slide| *slide != asked);
            let Some(furthest) = others.max_by_key(|slide| slide.abs_diff(asked)) else {
                return;
            };
            self.slides[furthest].laid = None;
            self.slides[furthest].at_moment = None;
            self.slides[furthest].placed = None;
            self.slides_holding_pictures.remove(&furthest);
            self.pictures.let_go_of_unused();
        }
    }

    /// Lays out slide `index` at `Moment::START` if it isn't already. This and `ensure_laid_at`
    /// are the one place pictures come into memory, so that anything walking the slides stays
    /// within the budget.
    fn ensure_laid(&mut self, index: usize) {
        if self.slides[index].laid.is_some() {
            return;
        }
        let laid = self.lay_out(index, Moment::START);
        self.slides[index].laid = Some(laid);
        self.held_pictures_changed(index);
    }

    /// The moment slide `index` is the same at as at `moment`. In what the slide doesn't vary
    /// by, that's the start: a slide no component steps through is at step 0, and one no
    /// component animates is at time 0. A step past its last is its last.
    fn moment_of(&mut self, index: usize, moment: Moment) -> Moment {
        self.ensure_laid(index);
        let laid = self.slides[index].laid.as_ref().expect("the slide was just laid out");
        let step = if laid.asks.stepped { moment.step.min(laid.steps()) } else { 0 };
        let time = if laid.asks.animated && moment.time.is_finite() { moment.time.max(0.0) } else { 0.0 };
        let state = if laid.specs.iter().any(|spec| spec.component.is_some()) { moment.state } else { 0 };
        Moment { step, time, state }
    }

    /// Lays out slide `index` in its whole state, and gives the moment that is.
    fn ensure_whole(&mut self, index: usize) -> Moment {
        self.ensure_laid(index);
        let steps = self.slides[index].laid.as_ref().expect("the slide was just laid out").steps();
        self.ensure_laid_at(index, Moment::at(steps, 0.0))
    }

    /// Lays out slide `index` at `moment` if it isn't already, and gives the moment it is
    /// laid out at, which `SlideState::laid_at` finds.
    fn ensure_laid_at(&mut self, index: usize, moment: Moment) -> Moment {
        let moment = self.moment_of(index, moment);
        let slide = &self.slides[index];
        let known = moment == Moment::START || slide.at_moment.as_ref().is_some_and(|(at, _)| *at == moment);
        if !known {
            let laid = self.lay_out(index, moment);
            self.slides[index].at_moment = Some((moment, laid));
            self.held_pictures_changed(index);
        }
        moment
    }

    /// Notes whether slide `index` now holds pictures, and lets go of others' if that's over budget.
    fn held_pictures_changed(&mut self, index: usize) {
        if self.slides[index].holds_pictures() {
            self.slides_holding_pictures.insert(index);
        } else {
            self.slides_holding_pictures.remove(&index);
        }
        self.keep_pictures_within_budget(index);
    }

    fn lay_out(&mut self, index: usize, moment: Moment) -> Laid {
        self.lay_out_placed(index, moment, &[])
    }

    /// What makes `spec`, one of slide `index`'s layers at `moment`, the same as a layer drawn before.
    fn layer_key(&self, index: usize, moment: Moment, spec: &LayerSpec) -> LayerKey {
        let layout = self.talk.slides[index].settings.as_ref().and_then(|settings| settings.get("layout")).unwrap_or_default();
        let text = spec.source.clone().map(|range| &self.talk.source[range]).unwrap_or_default();
        LayerKey {
            text: format!("{}\n{layout}\n{}\n{text}", self.theme, spec.source.is_some()),
            place: place_bits(spec),
            drawing: drawing_fingerprint(spec, spec.terminal.as_ref().and_then(|on| self.pictures.terminal_version(&on.src))),
            files_generation: self.files_generation,
            moment: spec.varies.then_some((moment.step, moment.time.to_bits())),
            terminal: spec.terminal.as_ref().map(|on| on.src.clone()),
            state: spec.component.as_ref().map_or(0, |on| on.state),
        }
    }

    /// Runs `lay_out` with the talk and what laying it out at `moment` takes.
    fn with_layout_env<T>(&mut self, moment: Moment, lay_out: impl FnOnce(&Talk, &mut LayoutEnv) -> T) -> T {
        let Deck { fonts, registry, files, pictures, components, talk, .. } = self;
        let mut env = LayoutEnv { fonts, registry, files: &**files, pictures, components, moment, asks: ComponentAsks::default() };
        lay_out(talk, &mut env)
    }

    fn lay_out_placed(&mut self, index: usize, moment: Moment, placing: &[Placing]) -> Laid {
        self.pictures.take_stills_asked();
        let laid = self.with_layout_env(moment, |talk, env| layout_slide_placed(talk, index, env, placing));
        let stills = self.pictures.take_stills_asked();
        let keys = laid.layers.iter().map(|spec| self.layer_key(index, moment, spec)).collect();
        let notices = laid.notices.into_iter().map(|notice| notice.0).collect();
        // A video's still counts as a picture of the talk's.
        let holds_pictures = !stills.is_empty()
            || laid.layers.iter().flat_map(|spec| spec.items.iter()).any(|item| matches!(item, DisplayItem::Picture { src: Some(_), .. }));
        Laid {
            specs: laid.layers,
            keys,
            notices,
            transition: laid.transition,
            holds_pictures,
            stills,
            asks: laid.asks,
            texts: laid.texts,
            empty_regions: laid.empty_regions,
            regions: laid.regions,
            each_steps: laid.each_steps,
            each_regions: laid.each_regions,
            text_layout: None,
        }
    }

    /// Lays out slide `index` with the deck's placing if it has one here and it isn't already,
    /// and says whether it has.
    fn ensure_placed(&mut self, index: usize) -> bool {
        let Some((_, placing)) = self.placing.as_ref().filter(|(slide, _)| *slide == index) else { return false };
        let placing = placing.clone();
        let moment = self.ensure_whole(index);
        if self.slides[index].placed.is_none() {
            let laid = self.placed_layout(index, moment, &placing);
            self.slides[index].placed = Some(laid);
            // Laid out again, it can hold pictures its own layouts don't.
            self.held_pictures_changed(index);
        }
        true
    }

    /// Slide `index` in its whole state, at `moment`, with `placing`. What's only moved is
    /// moved, a placed tag of a new size or turn is laid out again by itself, and the whole
    /// slide is laid out again only for a region of a new size or turn.
    fn placed_layout(&mut self, index: usize, moment: Moment, placing: &[Placing]) -> Laid {
        // The last placing for each target is the one that counts.
        let last: Vec<&Placing> =
            placing.iter().enumerate().filter(|(at, this)| !placing[at + 1..].iter().any(|later| later.target == this.target)).map(|(_, this)| this).collect();
        let base = self.slides[index].laid_at(moment);
        let reshaped = |placing: &Placing| match &placing.target {
            Target::Region(name) => base.regions.iter().find(|region| region.name == *name).is_some_and(|region| {
                let (rect, rotate) = placed_at(region.rect, region.rotate, placing);
                (rect.w, rect.h, rotate) != (region.rect.w, region.rect.h, region.rotate)
            }),
            Target::Tag(_) => false,
        };
        if last.iter().any(|placing| reshaped(placing)) {
            return self.lay_out_placed(index, moment, placing);
        }
        let mut laid = base.clone();
        laid.text_layout = None;
        let mut laid_again = Vec::new();
        for placing in last {
            match &placing.target {
                Target::Region(name) => {
                    let Some(region) = laid.regions.iter_mut().find(|region| region.name == *name) else { continue };
                    let (rect, _) = placed_at(region.rect, region.rotate, placing);
                    let (dx, dy) = (rect.x - region.rect.x, rect.y - region.rect.y);
                    region.rect = rect;
                    for ((spec, text), key) in laid.specs.iter_mut().zip(&mut laid.texts).zip(&mut laid.keys) {
                        // A region shown a piece at a time has a layer for each piece, whose own place this is.
                        let Some((_, area)) = text.region.as_mut().filter(|(region, _)| *region == name.as_str()) else { continue };
                        area.x += dx;
                        area.y += dy;
                        spec.rect.x += dx;
                        spec.rect.y += dy;
                        key.place = place_bits(spec);
                    }
                    for empty in laid.empty_regions.iter_mut().filter(|empty| empty.name == name.as_str()) {
                        empty.rect = rect;
                    }
                }
                Target::Tag(range) => {
                    if placed_tag(&self.talk.slides[index], range).is_none() {
                        continue;
                    }
                    let found = (0..laid.specs.len()).find(|&at| laid.texts[at].whole && laid.specs[at].source.as_ref() == Some(range));
                    let Some(at) = found else { continue };
                    let spec = &mut laid.specs[at];
                    let (rect, rotate) = placed_at(spec.rect, spec.rotate, placing);
                    if (rect.w, rect.h) == (spec.rect.w, spec.rect.h) {
                        spec.rect = rect;
                        spec.rotate = rotate;
                        if let Some(video) = &mut spec.video {
                            video.rect = rect;
                            video.rotate = rotate;
                        }
                        if let Some(model) = &mut spec.model {
                            model.rect = rect;
                            model.rotate = rotate;
                        }
                        if let Some(terminal) = &mut spec.terminal {
                            terminal.rect = rect;
                            terminal.rotate = rotate;
                        }
                        laid.keys[at].place = place_bits(spec);
                    } else {
                        laid_again.push((at, range.clone(), placing.clone()));
                    }
                }
            }
        }
        for (at, range, placing) in laid_again {
            let Some((spec, text)) = self.with_layout_env(moment, |talk, env| placed_tag_layer(talk, index, &range, &placing, env)) else { continue };
            laid.stills.extend(self.pictures.take_stills_asked());
            laid.keys[at] = self.layer_key(index, moment, &spec);
            laid.specs[at] = spec;
            laid.texts[at] = text;
        }
        laid
    }

    /// Slide `index`'s layers with the deck's placing, which `ensure_placed` laid out, drawn at
    /// `scale`. A layer the slide's own layers have is theirs, and one only moved shows the
    /// pixmap it was drawn into at its new place.
    fn placed_layers(&mut self, index: usize, scale: f32) -> Vec<Arc<Layer>> {
        self.keep_moments_of(index);
        self.layers_asked_for += 1;
        self.slides[index].last_used = self.layers_asked_for;
        let slide = &mut self.slides[index];
        make_room(&mut slide.drawn, scale.to_bits());
        let SlideState { placed, drawn: scales, .. } = slide;
        let placed = placed.as_ref().expect("the slide was just laid out");
        let drawn = scales.entry(scale.to_bits()).or_default();
        drawn.asked = self.layers_asked_for;
        if let Some(ordered) = &drawn.placed_ordered {
            return ordered.clone();
        }
        let mut drawn = scales.remove(&scale.to_bits()).unwrap_or_default();
        let mut by_key = HashMap::new();
        let mut ordered = Vec::with_capacity(placed.specs.len());
        let (aside, mut wanted) = (self.pictures.makes_aside(), false);
        for (spec, key) in placed.specs.iter().zip(&placed.keys) {
            let layer = if spec.source.is_none() {
                self.backgrounds
                    .entry((self.theme.clone(), scale.to_bits()))
                    .or_insert_with(|| Arc::new(render_layer(spec, scale, &mut self.fonts)))
                    .clone()
            } else if let Some(layer) = drawn.by_key.get(key) {
                layer.clone()
            } else {
                let (layer, at) = by_key
                    .get(key)
                    .or_else(|| drawn.placed_by_key.get(key))
                    .cloned()
                    .or_else(|| moved_layer(&drawn, key, scale))
                    .or_else(|| spec.terminal.as_ref().and_then(|_| resized_layer(&drawn, key, spec, scale).map(|(_, layer, at)| (layer, at))))
                    .or_else(|| {
                        // A screen still shaped aside, or too long to draw here and drawn aside, is
                        // shown as a picture of it at another scale meanwhile, if there's one.
                        let pending = spec.terminal.as_ref()?.pending;
                        if !pending {
                            if !(aside && pieces_drawn(spec, scale) > MOST_PIECES_DRAWN_HERE) {
                                return None;
                            }
                            wanted |= want_drawn(&mut self.draw_jobs, &mut self.drawing, &mut self.fonts, (index, scale, key, spec));
                        }
                        let (layer, _) = standing_in(&drawn, scales, key, spec, scale, &mut self.fonts);
                        let at = DrawnAt::of(&layer, spec.rect.x, spec.rect.y);
                        Some((layer, at))
                    })
                    .unwrap_or_else(|| {
                        let layer = Arc::new(render_layer(spec, scale, &mut self.fonts));
                        let at = DrawnAt::of(&layer, spec.rect.x, spec.rect.y);
                        (layer, at)
                    });
                by_key.insert(key.clone(), (layer.clone(), at));
                layer
            };
            ordered.push(layer);
        }
        drawn.placed_by_key = by_key;
        drawn.placed_ordered = Some(ordered.clone());
        scales.insert(scale.to_bits(), drawn);
        if wanted {
            self.pictures.tell_wanted();
        }
        self.keep_within_budget(index);
        ordered
    }
}

/// A terminal recording's layer of `drawn`, the slide's own, stretched over where `spec`, the same
/// recording resized, now is: what's shown while it's being resized, as drawing a screen of text
/// again at each step of a drag would take too long. It's drawn again once the drag ends.
/// Gives as well the key and picture it was made from.
fn resized_layer(drawn: &Drawn, key: &LayerKey, spec: &LayerSpec, scale: f32) -> Option<(Picture, Arc<Layer>, DrawnAt)> {
    let (other, own) = drawn_of(drawn, key)?;
    let from = (other.clone(), own.clone());
    // One of the same size and turn is only moved.
    if other.place[2..] == key.place[2..] {
        let (x, y) = (f32::from_bits(other.place[0]), f32::from_bits(other.place[1]));
        let at = DrawnAt::of(own, x, y);
        return Some((from, at.shown_at(&own.pixmap, spec.rect.x, spec.rect.y, scale), at));
    }
    let layer = Arc::new(crate::render::stretched_layer(own, spec, scale));
    let at = DrawnAt::of(&layer, spec.rect.x, spec.rect.y);
    Some((from, layer, at))
}

/// A picture in `drawn` of the terminal recording whose layer `key` is: one from the same text if
/// there is one, or else any of the same recording, as one resized has had its text written again.
fn drawn_of<'a>(drawn: &'a Drawn, key: &LayerKey) -> Option<(&'a LayerKey, &'a Arc<Layer>)> {
    key.terminal.as_ref()?;
    let same = |other: &LayerKey| other.terminal == key.terminal && other.files_generation == key.files_generation && other.moment == key.moment;
    let mut any = None;
    for (other, layer) in drawn.by_key.iter().filter(|(other, _)| same(other)) {
        if other.text == key.text {
            return Some((other, layer));
        }
        any.get_or_insert((other, layer));
    }
    any
}

/// The terminal recordings among `specs`.
fn terminals_of(specs: &[LayerSpec]) -> Vec<TerminalLayer> {
    specs
        .iter()
        .enumerate()
        .filter_map(|(layer, spec)| Some(TerminalLayer { layer, source: spec.source.clone(), click: spec.click, id: spec.id.clone(), on: spec.terminal.clone()? }))
        .collect()
}

/// The 3D models among `specs`.
fn models_of(specs: &[LayerSpec]) -> Vec<ModelLayer> {
    specs
        .iter()
        .enumerate()
        .filter_map(|(layer, spec)| Some(ModelLayer { layer, source: spec.source.clone(), click: spec.click, id: spec.id.clone(), on: spec.model.clone()? }))
        .collect()
}

/// The buttons among `specs`.
fn buttons_of(specs: &[LayerSpec]) -> Vec<ButtonLayer> {
    specs
        .iter()
        .enumerate()
        .filter_map(|(layer, spec)| Some(ButtonLayer { layer, source: spec.source.clone()?, click: spec.click, on: spec.button.clone()? }))
        .collect()
}

/// The interactive components among `specs`.
fn components_of(specs: &[LayerSpec]) -> Vec<ComponentLayer> {
    specs
        .iter()
        .enumerate()
        .filter_map(|(layer, spec)| {
            Some(ComponentLayer { layer, source: spec.source.clone()?, click: spec.click, id: spec.id.clone(), on: spec.component.clone()? })
        })
        .collect()
}

/// The videos among `specs`.
fn videos_of(specs: &[LayerSpec]) -> Vec<VideoLayer> {
    specs
        .iter()
        .enumerate()
        .filter_map(|(layer, spec)| Some(VideoLayer { layer, source: spec.source.clone(), click: spec.click, on: spec.video.clone()? }))
        .collect()
}

impl SlideState {
    /// Moves what the slide was laid out from to where its text now sits: from `from` to `to`.
    fn moved(&mut self, from: usize, to: usize) {
        if from == to {
            return;
        }
        // A placing names things by where they're written, so it's gone by now.
        self.placed = None;
        let at_moment = self.at_moment.as_mut().map(|(_, laid)| laid);
        for laid in self.laid.iter_mut().chain(at_moment) {
            for spec in &mut laid.specs {
                if let Some(source) = &mut spec.source {
                    *source = source.start - from + to..source.end - from + to;
                }
            }
            for text in &mut laid.texts {
                text.moved(from, to);
            }
            // The text layout's elements say where each is written: it's made again from these.
            laid.text_layout = None;
        }
    }

    /// Whether any of its layouts shows a picture from a file of the talk.
    fn holds_pictures(&self) -> bool {
        let at_moment = self.at_moment.as_ref().map(|(_, laid)| laid);
        self.laid.iter().chain(at_moment).chain(&self.placed).any(|laid| laid.holds_pictures)
    }

    /// The layout at `moment`, which `Deck::ensure_laid_at` made sure of.
    fn laid_at(&self, moment: Moment) -> &Laid {
        match &self.at_moment {
            Some((at, laid)) if *at == moment => laid,
            _ => self.laid.as_ref().expect("the slide was just laid out"),
        }
    }

    fn laid_at_mut(&mut self, moment: Moment) -> &mut Laid {
        match &mut self.at_moment {
            Some((at, laid)) if *at == moment => laid,
            _ => self.laid.as_mut().expect("the slide was just laid out"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pictures::FileVersion;
    use crate::{Enter, Rect, TerminalOn};

    #[test]
    fn a_busy_screen_of_another_version_of_its_file_is_told_from_the_one_before() {
        let rect = Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        let fill = || DisplayItem::Fill { path: tiny_skia::PathBuilder::from_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap()), color: toolkit::Color::rgb(1, 2, 3) };
        let terminal = TerminalOn { src: "assets/run.cast".to_string(), run: None, cols: 100, rows: 30, rect, rotate: 0.0, stops: 0, pending: false };
        let spec = LayerSpec {
            source: None,
            rect,
            rotate: 0.0,
            items: (0..65).map(|_| fill()).collect::<Vec<_>>().into(),
            click: None,
            enter: Enter::Appear,
            native: Vec::new(),
            is_title: false,
            varies: false,
            video: None,
            model: None,
            terminal: Some(terminal),
            button: None,
            component: None,
            id: None,
        };
        let (one, two) = (FileVersion::Hash(1), FileVersion::Hash(2));
        assert_eq!(drawing_fingerprint(&spec, Some(&one)), drawing_fingerprint(&spec, Some(&one)));
        assert_ne!(drawing_fingerprint(&spec, Some(&one)), drawing_fingerprint(&spec, Some(&two)));
    }
}
