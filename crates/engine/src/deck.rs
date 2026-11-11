//! A talk and its drawn layers, kept so that an edit only draws again what it changed.

use std::collections::{BTreeSet, HashMap};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;

use slides_components::Diagnostic;
use slides_format::{Settings, Talk, parse_talk};
use toolkit::{ComponentSchema, Registry};

use crate::{
    ComponentInfo, DisplayItem, Fonts, Layer, LayerSpec, LayerTiming, LayoutEnv, PictureCache, SlideTimeline, TalkComponents, TalkFiles,
    Transition, builtin_registry, layout_slide, render_layer, talk_theme,
};

/// How many scales of one slide are kept drawn. A zoom or a resize asks for many in a row.
const KEPT_SCALES: usize = 4;

/// What the drawn layers of a whole deck can add up to, in pixel bytes, unless it's told otherwise.
const DEFAULT_MEMORY_BUDGET: usize = 512 * 1024 * 1024;

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
    /// The layers in order, as long as nothing in the slide has changed.
    ordered: Option<Vec<Arc<Layer>>>,
}

struct SlideState {
    text: String,
    laid: Option<Laid>,
    drawn: HashMap<u32, Drawn>,
    /// The value of `Deck::layers_asked_for` when this slide's layers were last asked for.
    last_used: u64,
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
    /// The slides whose `laid` holds pictures, which are the ones that can give memory back.
    slides_holding_pictures: BTreeSet<usize>,
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

impl Deck {
    pub fn open(files: Box<dyn TalkFiles>, fonts: Fonts, source: String) -> Deck {
        Deck::with_registry(files, fonts, source, builtin_registry())
    }

    /// A deck whose talk can use the components in `registry`, as well as its own.
    pub fn with_registry(files: Box<dyn TalkFiles>, fonts: Fonts, source: String, registry: Registry) -> Deck {
        let files: Rc<dyn TalkFiles> = Rc::from(files);
        let components = TalkComponents::scan_for(files.clone(), &registry);
        let talk = parse_talk(&source);
        let slides = slide_texts(&talk)
            .into_iter()
            .map(|text| SlideState { text, laid: None, drawn: HashMap::new(), last_used: 0 })
            .collect();
        Deck {
            files,
            fonts,
            registry,
            components,
            pictures: PictureCache::default(),
            files_generation: 0,
            theme: theme_key(&talk),
            talk,
            slides,
            backgrounds: HashMap::new(),
            memory_budget: DEFAULT_MEMORY_BUDGET,
            layers_asked_for: 0,
            slides_holding_pictures: BTreeSet::new(),
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
                        drawn: std::mem::take(&mut previous.drawn),
                        last_used: previous.last_used,
                    });
                }
                previous => {
                    changed.push(index);
                    let last_used = previous.as_ref().map_or(0, |p| p.last_used);
                    let mut drawn = previous.map(|p| std::mem::take(&mut p.drawn)).unwrap_or_default();
                    for scale in drawn.values_mut() {
                        scale.ordered = None;
                    }
                    slides.push(SlideState { text, laid: None, drawn, last_used });
                }
            }
        }
        self.theme = theme_key(&talk);
        self.talk = talk;
        self.slides_holding_pictures = slides
            .iter()
            .enumerate()
            .filter(|(_, slide)| slide.laid.as_ref().is_some_and(|laid| laid.holds_pictures))
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
            for scale in slide.drawn.values_mut() {
                scale.ordered = None;
            }
        }
        self.slides_holding_pictures.clear();
        (0..self.slides.len()).collect()
    }

    /// The talk's own components.
    pub fn components(&self) -> &[ComponentInfo] {
        self.components.infos()
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
        slide_layers.chain(self.backgrounds.values()).map(|layer| layer.pixmap.data().len()).sum()
    }

    pub fn slide_count(&self) -> usize {
        self.slides.len()
    }

    pub fn source(&self) -> &str {
        &self.talk.source
    }

    pub fn title(&self) -> Option<String> {
        self.talk.slides.first()?.settings.as_ref()?.get("title").map(str::to_string)
    }

    pub fn notices(&mut self, index: usize) -> Vec<String> {
        self.ensure_laid(index);
        self.slides[index].laid.as_ref().map(|laid| laid.notices.clone()).unwrap_or_default()
    }

    /// The slide's notes, if it has any.
    pub fn notes(&self, index: usize) -> Option<String> {
        self.talk.slides[index].notes.as_ref().map(|notes| notes.text.clone())
    }

    /// When each layer of the slide appears, in the order of `layers`, and how the slide is arrived at.
    pub fn timeline(&mut self, index: usize) -> SlideTimeline {
        self.ensure_laid(index);
        let laid = self.slides[index].laid.as_ref().expect("the slide was just laid out");
        let layers: Vec<LayerTiming> =
            laid.specs.iter().map(|spec| LayerTiming { click: spec.click, enter: spec.enter }).collect();
        let steps = layers.iter().filter_map(|layer| layer.click).max().unwrap_or(0);
        SlideTimeline { steps, layers, transition: laid.transition }
    }

    /// What each layer of the slide draws, before it is drawn at any scale.
    pub fn specs(&mut self, index: usize) -> &[LayerSpec] {
        self.ensure_laid(index);
        &self.slides[index].laid.as_ref().expect("the slide was just laid out").specs
    }

    /// A slide's specs with what they are drawn from: the fonts, and the talk's files.
    pub fn specs_fonts_and_files(&mut self, index: usize) -> (&[LayerSpec], &mut Fonts, &dyn TalkFiles) {
        self.ensure_laid(index);
        let laid = self.slides[index].laid.as_ref().expect("the slide was just laid out");
        (&laid.specs, &mut self.fonts, &*self.files)
    }

    /// The talk's look.
    pub fn theme(&self) -> toolkit::Theme {
        talk_theme(&self.talk)
    }

    pub fn fonts_mut(&mut self) -> &mut Fonts {
        &mut self.fonts
    }

    /// The slide's layers, first to last, drawn at `scale`.
    pub fn layers(&mut self, index: usize, scale: f32) -> Vec<Arc<Layer>> {
        self.ensure_laid(index);
        self.layers_asked_for += 1;
        self.slides[index].last_used = self.layers_asked_for;
        let layers = self.drawn_layers(index, scale);
        self.keep_within_budget(index);
        layers
    }

    /// Forgets whole slides' drawn layers, the slide asked for longest ago first, until the
    /// deck is within its budget. Slide `requested` stays, however much it takes alone.
    fn keep_within_budget(&mut self, requested: usize) {
        loop {
            self.drop_unused_backgrounds();
            if self.drawn_bytes() <= self.memory_budget {
                return;
            }
            let others = self.slides.iter_mut().enumerate().filter(|(index, slide)| *index != requested && !slide.drawn.is_empty());
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

    fn drawn_layers(&mut self, index: usize, scale: f32) -> Vec<Arc<Layer>> {
        let slide = &mut self.slides[index];
        let laid = slide.laid.as_ref().expect("the slide was just laid out");
        if slide.drawn.len() >= KEPT_SCALES && !slide.drawn.contains_key(&scale.to_bits()) {
            slide.drawn.clear();
        }
        let drawn = slide.drawn.entry(scale.to_bits()).or_default();
        if let Some(ordered) = &drawn.ordered {
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
        drawn.ordered = Some(ordered.clone());
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
            self.slides_holding_pictures.remove(&furthest);
            self.pictures.let_go_of_unused();
        }
    }

    /// Lays out slide `index` if it isn't already, which is the one place pictures come into
    /// memory, so that anything walking the slides stays within the budget.
    fn ensure_laid(&mut self, index: usize) {
        if self.slides[index].laid.is_some() {
            return;
        }
        let mut env = LayoutEnv {
            fonts: &mut self.fonts,
            registry: &self.registry,
            files: &*self.files,
            pictures: &mut self.pictures,
            components: &mut self.components,
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
                }
            })
            .collect();
        let notices = laid.notices.into_iter().map(|notice| notice.0).collect();
        let holds_pictures = laid
            .layers
            .iter()
            .flat_map(|spec| &spec.items)
            .any(|item| matches!(item, DisplayItem::Picture { src: Some(_), .. }));
        self.slides[index].laid = Some(Laid { specs: laid.layers, keys, notices, transition: laid.transition, holds_pictures });
        if holds_pictures {
            self.slides_holding_pictures.insert(index);
        }
        self.keep_pictures_within_budget(index);
    }
}
