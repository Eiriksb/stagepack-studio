//! What the open talk shows: its slides, their layers, notes and timelines, and its text.

use super::*;

impl Document {
    pub fn current(&self) -> usize {
        self.current
    }

    pub fn slide_count(&self) -> usize {
        self.deck.slide_count()
    }

    /// How many slides a show goes through: all but the hidden ones.
    pub fn shown_count(&self) -> usize {
        self.deck.shown_slides().len()
    }

    /// The talk's own index of the slide a show calls `position`, counting only the slides it
    /// shows. This is the one place a show's slides are mapped to the talk's.
    pub fn shown_slide(&self, position: usize) -> usize {
        let shown = self.deck.shown_slides();
        shown.get(position).or(shown.last()).copied().unwrap_or(0)
    }

    pub fn next(&mut self) {
        if self.current + 1 < self.slide_count() {
            self.go_to(self.current + 1);
        }
    }

    pub fn previous(&mut self) {
        self.go_to(self.current.saturating_sub(1));
    }

    /// Makes slide `index` the current one, or the last if there's no such slide.
    pub fn go_to(&mut self, index: usize) {
        let index = index.min(self.slide_count() - 1);
        if index != self.current {
            // What's typed is in the talk already; only what typing left empty is tidied.
            if let Err(refused) = self.end_text() {
                self.say(refused);
            }
            self.current = index;
            self.typing_notes = None;
        }
    }

    /// What the talk is called: its title setting, or else the name of its folder or file.
    pub fn name(&self) -> String {
        self.deck
            .title()
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| self.place_name.clone())
    }

    /// The talk's name and where you are in it.
    pub fn title(&self) -> String {
        format!(
            "{} · {} / {}",
            self.name(),
            self.current + 1,
            self.slide_count()
        )
    }

    /// The current slide's layers, first to last, drawn at `scale`.
    pub fn layers(&mut self, scale: f32) -> Vec<Arc<Layer>> {
        self.layers_of(self.current, scale)
    }

    /// The layers of slide `index`, which must be one of the talk's, first to last, drawn at
    /// `scale`.
    pub fn layers_of(&mut self, index: usize, scale: f32) -> Vec<Arc<Layer>> {
        self.deck.layers(index, scale)
    }

    /// The current slide's layers as the editor shows them, first to last, drawn at `scale`:
    /// with an offer to add text in each region that has none.
    ///
    /// If laying the slide out goes wrong, it has none, and [`Self::take_message`] says so.
    pub fn editor_layers(&mut self, scale: f32) -> Vec<Arc<Layer>> {
        let (deck, current) = (&mut self.deck, self.current);
        laid_out(|| deck.editor_layers(current, scale)).unwrap_or_else(|| {
            self.layout_failed();
            Vec::new()
        })
    }

    /// Where slide `index`'s editable text is drawn, for a caret, a selection and a click.
    ///
    /// If laying the slide out goes wrong, it has no text, any typing ends, and
    /// [`Self::take_message`] says so.
    pub fn text_layout(&mut self, index: usize) -> Arc<TextLayout> {
        let deck = &mut self.deck;
        laid_out(|| deck.text_layout(index)).unwrap_or_else(|| {
            self.layout_failed();
            Arc::new(TextLayout::default())
        })
    }

    /// A slide couldn't be laid out for the editor: typing on it can't go on, and it's said.
    fn layout_failed(&mut self) {
        self.editing = None;
        self.held_color = None;
        self.message = Some(LAYOUT_FAILED.to_string());
    }

    /// Notes `refused`, which nobody was told of, for the window to say.
    pub(super) fn say(&mut self, refused: EditRefused) {
        self.message = Some(refused.to_string());
    }

    /// What the window should say about something that went wrong where no edit was refused
    /// to whoever asked: a tidy-up of typing before something else, or a slide that couldn't
    /// be laid out. Each is given once.
    pub fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    /// The layers of slide `index` at `moment`, first to last, drawn at `scale`.
    ///
    /// If laying the slide out goes wrong, it has none: a show goes on.
    pub fn layers_at(&mut self, index: usize, moment: Moment, scale: f32) -> Vec<Arc<Layer>> {
        let deck = &mut self.deck;
        laid_out(|| deck.layers_at(index, moment, scale)).unwrap_or_default()
    }

    /// When each layer of slide `index` at `moment` appears, in the order of [`Self::layers_at`]
    /// at that moment.
    pub fn timings_at(&mut self, index: usize, moment: Moment) -> Vec<LayerTiming> {
        self.deck.timings_at(index, moment)
    }

    /// When each layer of slide `index` appears, in the order of [`Self::layers_of`].
    pub fn timeline_of(&mut self, index: usize) -> SlideTimeline {
        self.deck.timeline(index)
    }

    /// The notes of slide `index`, if it has any. A slide that isn't in the talk has none.
    pub fn notes_of(&self, index: usize) -> Option<String> {
        (index < self.slide_count())
            .then(|| self.deck.notes(index))
            .flatten()
    }

    /// Whether slide `index` is left out of shows.
    pub fn is_hidden(&self, index: usize) -> bool {
        self.deck.is_hidden(index)
    }

    /// Slide `index` drawn `width` device pixels wide, to stand for it in a list, or `None` if
    /// the talk has no such slide, or drawing it goes wrong.
    pub fn thumbnail(&mut self, index: usize, width: u32) -> Option<Arc<Pixmap>> {
        let deck = &mut self.deck;
        laid_out(|| deck.thumbnail(index, width)).flatten()
    }

    /// What slide `index` is at the moment: a number that changes whenever anything that
    /// decides how it looks does. `None` if the talk has no such slide.
    pub fn slide_version(&self, index: usize) -> Option<u64> {
        self.deck.slide_version(index)
    }

    /// How many pixel bytes the talk's drawn slides and thumbnails can add up to.
    pub fn set_memory_budget(&mut self, bytes: usize) {
        self.deck.set_memory_budget(bytes);
    }

    /// Slide `index`'s thumbnail `width` wide, if it's drawn already: asking draws nothing.
    pub fn drawn_thumbnail(&self, index: usize, width: u32) -> Option<Arc<Pixmap>> {
        self.deck.drawn_thumbnail(index, width)
    }

    /// What the user should hear about the current slide. If laying it out goes wrong, there
    /// are none, and [`Self::take_message`] says so, as for [`Self::editor_layers`].
    pub fn notices(&mut self) -> Vec<String> {
        let (deck, current) = (&mut self.deck, self.current);
        // The window shows the slide in its whole state, so these are its notices.
        laid_out(|| deck.notices_whole(current)).unwrap_or_else(|| {
            self.layout_failed();
            Vec::new()
        })
    }

    pub fn source(&self) -> &str {
        self.deck.source()
    }

    /// The talk as it is now: its slides, and where each sits in the text.
    pub fn talk(&self) -> &Talk {
        self.deck.talk()
    }

    /// The talk's theme, as the engine reads it.
    pub fn theme(&self) -> Theme {
        self.deck.theme()
    }
}

/// A show's slides are the talk's shown ones, so `slide` here is a position among those.
impl TalkShape for Document {
    fn slide_count(&self) -> usize {
        self.shown_count()
    }

    fn steps(&mut self, slide: usize) -> u32 {
        self.timeline_of(self.shown_slide(slide)).steps
    }

    fn transition(&mut self, slide: usize) -> Transition {
        self.timeline_of(self.shown_slide(slide)).transition
    }
}

/// The talk's own settings, as the first slide's settings block holds them: each key of
/// [`slides_format::TALK_SETTINGS`] it has, with its value.
pub fn talk_settings(talk: &Talk) -> Vec<(&'static str, String)> {
    let settings = talk.slides.first().and_then(|slide| slide.settings.as_ref());
    slides_format::TALK_SETTINGS
        .iter()
        .filter_map(|&key| Some((key, settings?.get(key)?.to_string())))
        .collect()
}
