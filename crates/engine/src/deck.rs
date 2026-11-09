//! A talk and its drawn layers, kept so that an edit only draws again what it changed.

use std::collections::HashMap;
use std::sync::Arc;

use slides_format::{Settings, Talk, parse_talk};
use toolkit::Registry;

use crate::{Fonts, Layer, LayoutEnv, PictureCache, TalkFiles, builtin_registry, layout_slide, render_layer, talk_theme};

/// How many scales of one slide are kept drawn. A zoom or a resize asks for many in a row.
const KEPT_SCALES: usize = 4;

/// What makes a drawn layer the same as one drawn before: the text it came from, the
/// look of the talk and the slide's layout, where it sits and how it turns.
#[derive(Clone, PartialEq, Eq, Hash)]
struct LayerKey {
    text: String,
    place: [u32; 5],
}

/// A slide laid out. The keys are made when the slide is, because `LayerSpec::source`
/// points into the text of that moment, which later edits move.
struct Laid {
    specs: Vec<crate::LayerSpec>,
    keys: Vec<LayerKey>,
    notices: Vec<String>,
}

#[derive(Default)]
struct Drawn {
    by_key: HashMap<LayerKey, Arc<Layer>>,
    /// The layers in order, as long as nothing in the slide has changed.
    ordered: Option<Vec<Arc<Layer>>>,
}

struct SlideState {
    text: String,
    laid: Option<Laid>,
    drawn: HashMap<u32, Drawn>,
}

pub struct Deck {
    files: Box<dyn TalkFiles>,
    fonts: Fonts,
    registry: Registry,
    pictures: PictureCache,
    talk: Talk,
    slides: Vec<SlideState>,
}

/// The talk's settings, as far as they decide how a slide looks: not where they sit.
fn settings_pairs(settings: Option<&Settings>) -> Vec<(&str, &str)> {
    settings.map(|s| s.entries.iter().map(|e| (e.key.as_str(), e.value.as_str())).collect()).unwrap_or_default()
}

fn slide_texts(talk: &Talk) -> Vec<String> {
    talk.slides.iter().map(|slide| talk.source[slide.range.clone()].to_string()).collect()
}

impl Deck {
    pub fn open(files: Box<dyn TalkFiles>, fonts: Fonts, source: String) -> Deck {
        let talk = parse_talk(&source);
        let slides = slide_texts(&talk)
            .into_iter()
            .map(|text| SlideState { text, laid: None, drawn: HashMap::new() })
            .collect();
        Deck { files, fonts, registry: builtin_registry(), pictures: PictureCache::default(), talk, slides }
    }

    /// Takes the talk's new text and returns the slides that changed: those whose text
    /// differs from what was at the same place, or all of them if the talk's settings did.
    /// A changed slide is laid out again when next asked for, but keeps the pixels of
    /// any element that stayed the same.
    pub fn update(&mut self, source: String) -> Vec<usize> {
        let talk = parse_talk(&source);
        let same_look = settings_pairs(self.talk.slides.first().and_then(|s| s.settings.as_ref()))
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
                    });
                }
                previous => {
                    changed.push(index);
                    let mut drawn = previous.map(|p| std::mem::take(&mut p.drawn)).unwrap_or_default();
                    for scale in drawn.values_mut() {
                        scale.ordered = None;
                    }
                    slides.push(SlideState { text, laid: None, drawn });
                }
            }
        }
        self.talk = talk;
        self.slides = slides;
        changed
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
        self.lay_out(index);
        self.slides[index].laid.as_ref().map(|laid| laid.notices.clone()).unwrap_or_default()
    }

    /// The slide's layers, first to last, drawn at `scale`.
    pub fn layers(&mut self, index: usize, scale: f32) -> Vec<Arc<Layer>> {
        self.lay_out(index);
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
            let layer = by_key
                .get(key)
                .or_else(|| drawn.by_key.get(key))
                .cloned()
                .unwrap_or_else(|| Arc::new(render_layer(spec, scale, &mut self.fonts)));
            by_key.insert(key.clone(), layer.clone());
            ordered.push(layer);
        }
        drawn.by_key = by_key;
        drawn.ordered = Some(ordered.clone());
        ordered
    }

    fn lay_out(&mut self, index: usize) {
        if self.slides[index].laid.is_some() {
            return;
        }
        let mut env = LayoutEnv {
            fonts: &mut self.fonts,
            registry: &self.registry,
            files: &*self.files,
            pictures: &mut self.pictures,
        };
        let laid = layout_slide(&self.talk, index, &mut env);
        let source = &self.talk.source;
        let theme = format!("{:?}", talk_theme(&self.talk));
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
                }
            })
            .collect();
        let notices = laid.notices.into_iter().map(|notice| notice.0).collect();
        self.slides[index].laid = Some(Laid { specs: laid.layers, keys, notices });
    }
}
