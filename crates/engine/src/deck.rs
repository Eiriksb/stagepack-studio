//! A talk and its drawn layers, kept so that an edit only draws again what it changed.

use std::collections::{BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;

use tiny_skia::Pixmap;

use slides_components::Diagnostic;
use slides_format::{Settings, Talk, is_hidden, parse_talk};
use toolkit::{ComponentSchema, Registry};

use crate::slide::SLIDE_WIDTH;
use crate::timeline::MAX_CLICK;
use crate::{
    ComponentAsks, ComponentInfo, DisplayItem, Fonts, Layer, LayerSpec, LayerTiming, LayoutEnv, Moment, PictureCache, SlideTimeline,
    TalkComponents, TalkFiles, Transition, builtin_registry, compose, layout_slide, render_layer, talk_theme,
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
}

/// A slide laid out. The keys are made when the slide is, because `LayerSpec::source`
/// points into the text of that moment, which later edits move.
struct Laid {
    specs: Vec<LayerSpec>,
    keys: Vec<LayerKey>,
    notices: Vec<String>,
    transition: Transition,
    /// Whether it shows a picture from a file of the talk, and so holds that picture in memory.
    holds_pictures: bool,
    asks: ComponentAsks,
}

impl Laid {
    /// When each of its layers appears, in order.
    fn timings(&self) -> Vec<LayerTiming> {
        self.specs.iter().map(|spec| LayerTiming { click: spec.click, enter: spec.enter }).collect()
    }

    /// The most clicks its layers or components ask for.
    fn steps(&self) -> u32 {
        let clicks = self.specs.iter().filter_map(|spec| spec.click).max().unwrap_or(0);
        clicks.max(self.asks.steps).min(MAX_CLICK)
    }
}

/// A number that differs when a layer's items sit or measure differently.
fn drawing_fingerprint(items: &[DisplayItem]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for item in items {
        match item {
            DisplayItem::Fill { path, .. } | DisplayItem::Stroke { path, .. } => {
                let b = path.bounds();
                (0u8, [b.left(), b.top(), b.right(), b.bottom()].map(f32::to_bits)).hash(&mut hasher);
            }
            DisplayItem::Text { text, x, y } => (1u8, [*x, *y, text.width, text.height].map(f32::to_bits)).hash(&mut hasher),
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
}

struct SlideState {
    text: String,
    /// The slide laid out at `Moment::START`.
    laid: Option<Laid>,
    /// The slide laid out at the one other moment it was last asked for, which differs from the start.
    at_moment: Option<(Moment, Laid)>,
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

impl Deck {
    pub fn open(files: Box<dyn TalkFiles>, fonts: Fonts, source: String) -> Deck {
        Deck::with_registry(files, fonts, source, builtin_registry())
    }

    /// A deck whose talk can use the components in `registry`, as well as its own.
    pub fn with_registry(files: Box<dyn TalkFiles>, fonts: Fonts, source: String, registry: Registry) -> Deck {
        let files: Rc<dyn TalkFiles> = Rc::from(files);
        let components = TalkComponents::scan_for(files.clone(), &registry);
        let talk = parse_talk(&source);
        let slides: Vec<SlideState> = slide_texts(&talk)
            .into_iter()
            .zip(1..)
            .map(|(text, version)| SlideState {
                text,
                laid: None,
                at_moment: None,
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
        }
    }

    /// Takes the talk's new text and returns the slides that changed: those whose text
    /// differs from what was at the same place, or all of them if the talk's settings did,
    /// one of its picture files has been replaced or its components have changed, even with
    /// the text just as it was.
    /// A changed slide is laid out again when next asked for, but keeps the pixels of
    /// any element that stayed the same.
    pub fn update(&mut self, source: String) -> Vec<usize> {
        let pictures_replaced = self.pictures.drop_replaced(&*self.files);
        let components_changed = self.components.rescan(&self.registry);
        let files_changed = pictures_replaced || components_changed;
        if files_changed {
            self.files_generation += 1;
        }
        let talk = parse_talk(&source);
        let same_look = !files_changed
            && settings_pairs(self.talk.slides.first().and_then(|s| s.settings.as_ref()))
                == settings_pairs(talk.slides.first().and_then(|s| s.settings.as_ref()));
        let mut old = std::mem::take(&mut self.slides);
        let mut changed = Vec::new();
        let mut slides = Vec::new();
        for (index, text) in slide_texts(&talk).into_iter().enumerate() {
            let previous = old.get_mut(index);
            match previous {
                Some(previous) if same_look && previous.text == text => {
                    slides.push(SlideState {
                        text,
                        laid: previous.laid.take(),
                        at_moment: previous.at_moment.take(),
                        drawn: std::mem::take(&mut previous.drawn),
                        thumbnail: previous.thumbnail.take(),
                        last_used: previous.last_used,
                        thumbnail_used: previous.thumbnail_used,
                        version: previous.version,
                    });
                }
                previous => {
                    changed.push(index);
                    let (last_used, thumbnail_used) = previous.as_ref().map_or((0, 0), |p| (p.last_used, p.thumbnail_used));
                    let mut drawn = previous.map(|p| std::mem::take(&mut p.drawn)).unwrap_or_default();
                    for scale in drawn.values_mut() {
                        scale.ordered = None;
                    }
                    self.versions_made += 1;
                    let version = self.versions_made;
                    slides.push(SlideState { text, laid: None, at_moment: None, drawn, thumbnail: None, last_used, thumbnail_used, version });
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
            slide.thumbnail = None;
            for scale in slide.drawn.values_mut() {
                scale.ordered = None;
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
        self.files = Rc::from(files);
        self.components = TalkComponents::scan_for(self.files.clone(), &self.registry);
        self.pictures.clear();
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

    /// The names of every component the talk can use: the built-ins and its own, sorted, for
    /// a palette to offer.
    pub fn available_components(&self) -> Vec<String> {
        let own = self.components.infos().iter().map(|info| info.tag.clone());
        let mut names: Vec<String> = self.registry.names().into_iter().map(String::from).chain(own).collect();
        names.sort();
        names.dedup();
        names
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

    /// What the component `tag` takes, for a form: a built-in, or one of the talk's own.
    pub fn component_schema(&mut self, tag: &str) -> Option<ComponentSchema> {
        match self.registry.get(tag) {
            Some(component) => Some((component.schema)()),
            None => self.components.schema(tag),
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

    /// The pixel bytes of the talk's pictures in memory. A picture many slides show counts once.
    pub fn picture_bytes(&self) -> usize {
        self.pictures.bytes()
    }

    /// The pixel bytes the deck is holding in drawn layers. A layer the slides share counts once.
    pub fn drawn_bytes(&self) -> usize {
        let slide_layers = self.slides.iter().flat_map(|slide| slide.drawn.values()).flat_map(|drawn| drawn.by_key.values());
        let layers: usize = slide_layers.chain(self.backgrounds.values()).map(|layer| layer.pixmap.data().len()).sum();
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

    /// What each layer of the slide draws in its whole state, before it is drawn at any scale.
    pub fn specs(&mut self, index: usize) -> &[LayerSpec] {
        let moment = self.ensure_whole(index);
        &self.slides[index].laid_at(moment).specs
    }

    /// A slide's specs in its whole state, with what they are drawn from: the fonts, and the talk's files.
    pub fn specs_fonts_and_files(&mut self, index: usize) -> (&[LayerSpec], &mut Fonts, &dyn TalkFiles) {
        let moment = self.ensure_whole(index);
        (&self.slides[index].laid_at(moment).specs, &mut self.fonts, &*self.files)
    }

    /// The talk's look.
    pub fn theme(&self) -> toolkit::Theme {
        talk_theme(&self.talk)
    }

    pub fn fonts_mut(&mut self) -> &mut Fonts {
        &mut self.fonts
    }

    /// The slide's layers in its whole state, first to last, drawn at `scale`: its last step, at time 0.
    pub fn layers(&mut self, index: usize, scale: f32) -> Vec<Arc<Layer>> {
        let whole = self.ensure_whole(index);
        self.layers_at(index, whole, scale)
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
        if slide.drawn.len() >= KEPT_SCALES && !slide.drawn.contains_key(&scale.to_bits()) {
            slide.drawn.clear();
        }
        let SlideState { laid, at_moment, drawn, .. } = slide;
        let laid = match at_moment {
            Some((at, laid)) if *at == moment => &*laid,
            _ => laid.as_ref().expect("the slide was just laid out"),
        };
        let drawn = drawn.entry(scale.to_bits()).or_default();
        if let Some((at, ordered)) = &drawn.ordered
            && *at == moment
        {
            return ordered.clone();
        }
        let mut by_key = HashMap::new();
        let mut ordered = Vec::with_capacity(laid.specs.len());
        for (spec, key) in laid.specs.iter().zip(&laid.keys) {
            let layer = if spec.source.is_some() {
                let layer = by_key
                    .get(key)
                    .or_else(|| drawn.by_key.get(key))
                    .cloned()
                    .unwrap_or_else(|| Arc::new(render_layer(spec, scale, &mut self.fonts)));
                by_key.insert(key.clone(), layer.clone());
                layer
            } else {
                self.backgrounds
                    .entry((self.theme.clone(), scale.to_bits()))
                    .or_insert_with(|| Arc::new(render_layer(spec, scale, &mut self.fonts)))
                    .clone()
            };
            ordered.push(layer);
        }
        drawn.by_key = by_key;
        drawn.ordered = Some((moment, ordered.clone()));
        ordered
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
        Moment { step, time }
    }

    /// Lays out slide `index` in its whole state, and gives the moment that is.
    fn ensure_whole(&mut self, index: usize) -> Moment {
        self.ensure_laid(index);
        let steps = self.slides[index].laid.as_ref().expect("the slide was just laid out").steps();
        self.ensure_laid_at(index, Moment { step: steps, time: 0.0 })
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
        let mut env = LayoutEnv {
            fonts: &mut self.fonts,
            registry: &self.registry,
            files: &*self.files,
            pictures: &mut self.pictures,
            components: &mut self.components,
            moment,
            asks: ComponentAsks::default(),
        };
        let laid = layout_slide(&self.talk, index, &mut env);
        let source = &self.talk.source;
        let theme = &self.theme;
        let layout = self.talk.slides[index]
            .settings
            .as_ref()
            .and_then(|settings| settings.get("layout"))
            .unwrap_or_default();
        let keys = laid
            .layers
            .iter()
            .map(|spec| {
                let text = spec.source.clone().map(|range| source[range].to_string()).unwrap_or_default();
                let rect = spec.rect;
                LayerKey {
                    text: format!("{theme}\n{layout}\n{}\n{text}", spec.source.is_some()),
                    place: [rect.x, rect.y, rect.w, rect.h, spec.rotate].map(f32::to_bits),
                    drawing: drawing_fingerprint(&spec.items),
                    files_generation: self.files_generation,
                    moment: spec.varies.then_some((moment.step, moment.time.to_bits())),
                }
            })
            .collect();
        let notices = laid.notices.into_iter().map(|notice| notice.0).collect();
        let holds_pictures = laid
            .layers
            .iter()
            .flat_map(|spec| &spec.items)
            .any(|item| matches!(item, DisplayItem::Picture { src: Some(_), .. }));
        Laid { specs: laid.layers, keys, notices, transition: laid.transition, holds_pictures, asks: laid.asks }
    }
}

impl SlideState {
    /// Whether either of its layouts shows a picture from a file of the talk.
    fn holds_pictures(&self) -> bool {
        let at_moment = self.at_moment.as_ref().map(|(_, laid)| laid);
        self.laid.iter().chain(at_moment).any(|laid| laid.holds_pictures)
    }

    /// The layout at `moment`, which `Deck::ensure_laid_at` made sure of.
    fn laid_at(&self, moment: Moment) -> &Laid {
        match &self.at_moment {
            Some((at, laid)) if *at == moment => laid,
            _ => self.laid.as_ref().expect("the slide was just laid out"),
        }
    }
}
