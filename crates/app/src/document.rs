//! An open talk, and which of its slides is showing.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use slides_components::Diagnostic;
use slides_engine::{
    Align, ComponentInfo, Deck, FolderFiles, Fonts, Layer, LayerTiming, Moment, PackError, Pixmap,
    SlideTimeline, TalkFiles, TextLayout, Transition, layout_by_name, open_packed, text_align,
    read_talk_text,
};
use slides_format::{
    Change, FlowPlace, MAX_SLIDE_BYTES, Para, Pos, Selection, SourceEdit, Talk, TextEdit, parse_talk, reparse, set_align,
    split_slides, storable, text_flows, write_flow, write_into_region,
};

use crate::export::ExportInput;
use crate::history::{History, RunId};
use crate::present::TalkShape;
use crate::saving::{SaveJob, SaveTarget, Saved};
use crate::text_editing::{
    ALIGN_TEXT, COULD_NOT_TYPE, CUT, Composing, EditingText, FORMAT_TEXT, FlowKey, Kind, PASTE, TYPING, TextMark,
    holds_no_words, holds_something, region_has_text,
};

pub(crate) const SLIDES_FILE: &str = "slides.md";
const FONTS_FOLDER: &str = "assets/fonts";

/// The most bytes of fonts a talk can bring. A small packed file can unpack to far more than
/// that, and the fonts of an open talk stay in memory.
const TALK_FONT_BYTES: usize = 256 * 1024 * 1024;

/// What came of reading the talk's file again.
pub enum Reload {
    /// The talk took the new text, or a picture it shows was replaced. These are the
    /// slides to show again.
    Changed(Vec<usize>),
    /// The file says what it said before, and its pictures are as they were.
    Unchanged,
    /// The read gave nothing to use, so the talk stays as it was.
    KeptLastGood,
    /// The file changed while the talk has edits it doesn't, so nothing was taken: whoever is
    /// editing it is asked which to keep. [`Document::outside_change`] is what the file holds.
    Conflict,
    /// Reading what the file holds went wrong, so the talk stays as it was.
    Unreadable,
}

/// What the window says when a new read of the talk's file can't be taken in.
pub const UNREADABLE_FILE: &str =
    "The talk's file changed, but it couldn't be read, so the talk is shown as it was.";

/// Why an edit wasn't made. Nothing of it was applied, and there's nothing to undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditRefused {
    /// A slide that could be read would have become one that can't.
    WouldBreakASlide,
    /// The talk would have ended up with more or fewer slides than the edit was meant to give.
    WouldChangeTheSlides,
    /// An edit to one slide would have changed another.
    WouldChangeAnotherSlide,
    /// Working the change out went wrong.
    CouldNotBeMade,
    /// What was typed on a slide couldn't be written as it was typed.
    CouldNotBeTyped,
    /// Text that typing left with no words couldn't be taken away.
    CouldNotTidy,
}

impl std::fmt::Display for EditRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            EditRefused::WouldBreakASlide => {
                "That change would have broken a slide, so it wasn't made."
            }
            EditRefused::WouldChangeTheSlides => {
                "That change would have joined or split slides, so it wasn't made."
            }
            EditRefused::WouldChangeAnotherSlide => {
                "That change would have changed another slide, so it wasn't made."
            }
            EditRefused::CouldNotBeMade => "That change couldn't be made, so nothing was changed.",
            EditRefused::CouldNotBeTyped => COULD_NOT_TYPE,
            EditRefused::CouldNotTidy => {
                "The emptied text couldn't be taken away, so it's left on the slide."
            }
        })
    }
}

impl std::error::Error for EditRefused {}

/// Whether an edit to one slide joins the run of edits open in the history, so that one undo
/// takes the run back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditRun {
    /// The edit is a step of its own, which opens a run that later edits can join.
    New,
    /// The edit joins this run, while it's open. Once it isn't, the edit is a new step.
    Continue(RunId),
}

/// Which step of the history an edit's change goes into.
#[derive(Clone, Copy)]
enum Join {
    /// A step of its own, which opens no run.
    Alone,
    /// A step of its own that opens a run, or the run's step, as [`EditRun`] says.
    Run(EditRun),
    /// The newest step, if [`History::fold`] can take it in; otherwise a step of its own.
    Last,
}

/// How many slides an edit is meant to leave, and which of them it may change.
#[derive(Clone, Copy)]
enum Holds {
    /// The number of slides changes by this, and any slide may change.
    Count(isize),
    /// Only this slide may change, and the number of slides stays the same.
    Slide(usize),
    /// A slide is added at this index, and every other slide keeps its text, but for the line
    /// ending a slide that ended the file without one gains.
    Inserted(usize),
}

/// What Undo shows for each kind of edit.
const ADD_SLIDE: &str = "Add slide";
const DUPLICATE_SLIDE: &str = "Duplicate slide";
const DELETE_SLIDE: &str = "Delete slide";
const CUT_SLIDE: &str = "Cut slide";
const PASTE_SLIDE: &str = "Paste slide";
const MOVE_SLIDE: &str = "Move slide";
const HIDE_SLIDE: &str = "Hide slide";
const SHOW_SLIDE: &str = "Show slide";
const EDIT_NOTES: &str = "Edit notes";
const RECOVER_CHANGES: &str = "Recover changes";
const LOAD_CHANGED_FILE: &str = "Load the changed file";

/// What the window says when the current slide couldn't be laid out for the editor.
pub const LAYOUT_FAILED: &str =
    "This slide couldn't be laid out, so it can't be shown or typed on just now.";

thread_local! {
    /// Whether the next change worked out, or text read, on this thread panics, for tests.
    static PANIC_IN_NEXT_CHANGE: Cell<bool> = const { Cell::new(false) };
    /// Whether the next layout of a slide for the editor on this thread panics, for tests.
    static PANIC_IN_NEXT_LAYOUT: Cell<bool> = const { Cell::new(false) };
}

/// Makes the next change a [`Document`] command works out on this thread panic, or the next
/// text it reads, as a fault in the format crate would, so tests can see that it's refused.
/// Only for tests.
#[doc(hidden)]
pub fn panic_in_next_change() {
    PANIC_IN_NEXT_CHANGE.with(|flag| flag.set(true));
}

/// Makes the next layout of a slide for the editor, its layers or where its text is, panic on
/// this thread, as a fault in the engine would, so tests can see that it's survived. Only for
/// tests.
#[doc(hidden)]
pub fn panic_in_next_layout() {
    PANIC_IN_NEXT_LAYOUT.with(|flag| flag.set(true));
}

/// What `work`, the engine's layout of a slide for the editor, gives, or `None` if it panics.
fn laid_out<T>(work: impl FnOnce() -> T) -> Option<T> {
    guarded(|| {
        if PANIC_IN_NEXT_LAYOUT.with(|flag| flag.replace(false)) {
            panic!("a layout that panics, for a test");
        }
        work()
    })
    .ok()
}

/// What `work` gives, or [`EditRefused::CouldNotBeMade`] if it panics. It's the format crate's
/// work on a talk, which reads only what it's given, so nothing is left half done.
fn guarded<T>(work: impl FnOnce() -> T) -> Result<T, EditRefused> {
    catch_unwind(AssertUnwindSafe(work)).map_err(|_| EditRefused::CouldNotBeMade)
}

/// What `work`, some of the format crate's work, gives, guarded as [`guarded`] is.
fn worked_out<T>(work: impl FnOnce() -> T) -> Result<T, EditRefused> {
    guarded(|| {
        if PANIC_IN_NEXT_CHANGE.with(|flag| flag.replace(false)) {
            panic!("a change that panics, for a test");
        }
        work()
    })
}

/// What `build` makes of `talk`: the change a command works out, guarded as [`guarded`] is.
fn built<T>(talk: &Talk, build: impl FnOnce(&Talk) -> T) -> Result<T, EditRefused> {
    worked_out(|| build(talk))
}

/// The talk `source` reads as, the text of `talk` once changed, guarded as [`guarded`] is.
/// Only the slides whose text changed are read again.
fn read_after(talk: &Talk, source: String) -> Result<Talk, EditRefused> {
    worked_out(|| reparse(talk, source))
}

/// The layout a slide has when its settings name none.
const DEFAULT_LAYOUT: &str = "content";

/// Why a packed file didn't open.
#[derive(Debug, PartialEq, Eq)]
pub enum OpenProblem {
    /// One of Stagepack's Slidev talks, which this app can't show yet.
    SlidevTalk,
    Other(String),
}

impl From<PackError> for OpenProblem {
    fn from(error: PackError) -> OpenProblem {
        match error {
            PackError::SlidevTalk => OpenProblem::SlidevTalk,
            other => OpenProblem::Other(other.to_string()),
        }
    }
}

/// Where a talk is kept: what saving it works from. It can be handed to a background task, as a
/// [`Document`] can't.
#[derive(Clone)]
pub enum TalkPlace {
    Folder(PathBuf),
    Packed(PathBuf),
}

impl TalkPlace {
    /// The folder this talk sits in, where anything saved from it starts out.
    pub fn directory(&self) -> &Path {
        let (TalkPlace::Folder(path) | TalkPlace::Packed(path)) = self;
        path.parent().unwrap_or(path)
    }

    /// What to call a talk with no title setting.
    pub(crate) fn name(&self) -> String {
        match self {
            TalkPlace::Folder(folder) => folder_name(folder),
            TalkPlace::Packed(file) => file.file_stem().map_or_else(
                || file.display().to_string(),
                |stem| stem.to_string_lossy().into_owned(),
            ),
        }
    }
}

pub struct Document {
    deck: Deck,
    place: TalkPlace,
    /// The talk's files, which exports read from their own thread.
    files: Arc<dyn TalkFiles + Send + Sync>,
    /// The talk's own fonts, for exports to load into decks of their own.
    talk_fonts: Vec<Arc<Vec<u8>>>,
    /// What to call the talk if it has no title setting.
    place_name: String,
    current: usize,
    history: History,
    /// The slide whose notes are being typed into, and the history's run they're typed in:
    /// more typing there joins that run while it's open.
    typing_notes: Option<(usize, RunId)>,
    /// What the talk's file held when the app last wrote or read it, so that the app's own
    /// save isn't taken for someone else's change.
    on_disk: String,
    /// What the file holds after it changed under unsaved edits, until that's answered.
    outside: Option<String>,
    /// The text being typed on a slide, if any.
    editing: Option<EditingText>,
    /// A colour chosen for the selection of the text being typed, not yet given to it.
    held_color: Option<HeldColor>,
    /// The number the next time of typing on a slide gets.
    next_session: u64,
    /// What the window should say about something that went wrong where nothing was refused
    /// to whoever asked, such as a tidy-up before a slide change, until it's taken.
    message: Option<String>,
}

impl Document {
    /// Opens the talk kept in `folder`, along with any fonts it carries.
    pub fn open(folder: &Path, fonts: Fonts) -> Result<Document, String> {
        let source = read_talk_text(&folder.join(SLIDES_FILE)).map_err(|error| {
            format!(
                "Could not read {SLIDES_FILE} in {}: {error}",
                folder.display()
            )
        })?;
        let files = FolderFiles::new(folder.to_path_buf());
        let talk_fonts = read_talk_fonts(font_names_in_folder(folder), &files);
        Document::new(
            TalkPlace::Folder(folder.to_path_buf()),
            Arc::new(files),
            talk_fonts,
            source,
            fonts,
        )
        .map_err(|_| {
            format!(
                "Could not read {SLIDES_FILE} in {}: something in it can't be read.",
                folder.display()
            )
        })
    }

    /// Opens the packed talk in the file at `path`, along with any fonts it carries.
    pub fn open_packed(path: &Path, fonts: Fonts) -> Result<Document, OpenProblem> {
        if !path.exists() {
            return Err(OpenProblem::Other(format!(
                "There's no talk at {}.",
                path.display()
            )));
        }
        let packed = open_packed(path)?;
        let source = packed.slides()?;
        let names = packed
            .names()
            .into_iter()
            .filter(|name| is_talk_font(name))
            .collect();
        let talk_fonts = read_talk_fonts(names, &packed);
        Document::new(
            TalkPlace::Packed(path.to_path_buf()),
            Arc::new(packed),
            talk_fonts,
            source,
            fonts,
        )
        .map_err(|_| {
            OpenProblem::Other(format!(
                "Could not read the talk in {}: something in it can't be read.",
                path.display()
            ))
        })
    }

    fn new(
        place: TalkPlace,
        files: Arc<dyn TalkFiles + Send + Sync>,
        talk_fonts: Vec<Arc<Vec<u8>>>,
        source: String,
        mut fonts: Fonts,
    ) -> Result<Document, EditRefused> {
        let talk = worked_out(|| parse_talk(&source))?;
        for font in &talk_fonts {
            fonts.load_talk_font(font.clone());
        }
        Ok(Document {
            on_disk: source,
            outside: None,
            deck: Deck::open_parsed(Box::new(files.clone()), fonts, talk),
            place_name: place.name(),
            place,
            files,
            talk_fonts,
            current: 0,
            history: History::new(),
            typing_notes: None,
            editing: None,
            held_color: None,
            next_session: 0,
            message: None,
        })
    }

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
            self.select(self.current + 1);
        }
    }

    pub fn previous(&mut self) {
        self.select(self.current.saturating_sub(1));
    }

    /// Makes slide `index` the current one, or the last if there's no such slide.
    pub fn select(&mut self, index: usize) {
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
    fn say(&mut self, refused: EditRefused) {
        self.message = Some(refused.to_string());
    }

    /// What the window should say about something that went wrong where no edit was refused
    /// to whoever asked: a tidy-up of typing before something else, or a slide that couldn't
    /// be laid out. Each is given once.
    pub fn take_message(&mut self) -> Option<String> {
        self.message.take()
    }

    /// The layers of slide `index` at `moment`, first to last, drawn at `scale`.
    pub fn layers_at(&mut self, index: usize, moment: Moment, scale: f32) -> Vec<Arc<Layer>> {
        self.deck.layers_at(index, moment, scale)
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

    /// Makes `change` to the talk's text, named `label` for Undo, and returns the slides to
    /// show again. `slides` is how the number of slides is meant to change: 0, 1 or -1.
    ///
    /// The change is refused, and nothing applied or recorded, if it would leave a slide that
    /// could be read as one that can't, or change the number of slides by anything else. An
    /// empty change does nothing.
    pub fn edit(
        &mut self,
        label: &'static str,
        change: Change,
        slides: isize,
    ) -> Result<Vec<usize>, EditRefused> {
        let current = self.current;
        self.make(label, change, Holds::Count(slides), current, current, None)
    }

    /// Makes `change` to slide `index`'s text, named `label` for Undo, joining the run `run`
    /// says, makes `index` the current slide, and returns the slides to show again.
    ///
    /// The change is refused, and nothing applied or recorded, unless every other slide's
    /// bytes are the same after it as before and the talk has as many slides, or if it would
    /// leave a slide that could be read as one that can't. An empty change does nothing.
    pub fn edit_slide(
        &mut self,
        label: &'static str,
        index: usize,
        change: Change,
        run: EditRun,
    ) -> Result<Vec<usize>, EditRefused> {
        self.make(label, change, Holds::Slide(index), index, index, Some(run))
    }

    /// The run of edits that's open in the history, which an edit can join with
    /// [`EditRun::Continue`].
    pub fn open_run(&self) -> Option<RunId> {
        self.history.open_run()
    }

    /// The change `build` works out from the talk as it is, for one of the commands. Text being
    /// typed on a slide is finished first, so the change is worked out from the talk as tidied.
    /// A command that's refused ends any typing in the notes.
    fn build<T>(&mut self, build: impl FnOnce(&Talk) -> T) -> Result<T, EditRefused> {
        let built = self
            .finish_before_edit()
            .and_then(|_| built(self.talk(), build));
        if built.is_err() {
            self.typing_notes = None;
        }
        built
    }

    /// Makes `change` as [`Self::edit`] does, held to what `holds` says. `before` is the slide
    /// it's about in the text as it is, and `after` the one it's about once made, which becomes
    /// the current slide. With `run`, the step opens a run, or joins the one given; without, it
    /// opens none.
    ///
    /// Every edit made to the talk's text comes this way, or through [`Self::build`] first, but
    /// typing on a slide itself and what the history or the file gives back: undo, redo,
    /// recovering and reloading. Text being typed is finished before it; if that tidied the
    /// talk, `change`, worked out from the talk as it was, no longer fits it, and is refused.
    fn make(
        &mut self,
        label: &'static str,
        change: Change,
        holds: Holds,
        before: usize,
        after: usize,
        run: Option<EditRun>,
    ) -> Result<Vec<usize>, EditRefused> {
        let empty = change.is_empty();
        let join = run.map_or(Join::Alone, Join::Run);
        let made = match self.finish_before_edit() {
            Ok(false) => self.try_make(label, change, holds, before, after, join, None),
            Ok(true) => Err(EditRefused::CouldNotBeMade),
            Err(refused) => Err(refused),
        };
        if made.is_err() {
            self.typing_notes = None;
        }
        if made.is_ok() && !empty {
            // The text being typed may have moved, or gone: it's typed into again once clicked.
            self.editing = None;
            self.held_color = None;
        }
        made
    }

    /// Finishes the text being typed on a slide, before another edit, and says whether tidying
    /// what typing left empty changed the talk. Emptied text that can't be taken away is left
    /// where it is, said, and doesn't stop the edit.
    fn finish_before_edit(&mut self) -> Result<bool, EditRefused> {
        if self.editing.is_none() {
            return Ok(false);
        }
        match self.finish_text() {
            Ok((_, removed)) => Ok(removed.is_some()),
            Err(EditRefused::CouldNotTidy) => {
                self.message = Some(EditRefused::CouldNotTidy.to_string());
                Ok(false)
            }
            Err(refused) => Err(refused),
        }
    }

    /// [`Self::make`], but for ending the typing, into the step `join` says. With `check`, the
    /// talk the change gives is refused unless `check` holds for it, before anything is taken.
    #[allow(clippy::too_many_arguments)]
    fn try_make(
        &mut self,
        label: &'static str,
        change: Change,
        holds: Holds,
        before: usize,
        after: usize,
        join: Join,
        check: Option<&dyn Fn(&Talk) -> bool>,
    ) -> Result<Vec<usize>, EditRefused> {
        if change.is_empty() {
            return Ok(Vec::new());
        }
        let old = self.source();
        let old_count = self.slide_count();
        let broken_before = guarded(|| broken_bodies(self.talk()))?;
        // The candidate is worked out and read before the deck takes it, so a refusal, or a
        // panic, leaves the deck untouched.
        let (undo, text) = guarded(|| (change.inverse(old), change.apply(old)))?;
        let candidate = read_after(self.talk(), text)?;
        let slides = match holds {
            Holds::Count(slides) => slides,
            Holds::Slide(_) => 0,
            Holds::Inserted(_) => 1,
        };
        if old_count.checked_add_signed(slides) != Some(candidate.slides.len()) {
            return Err(EditRefused::WouldChangeTheSlides);
        }
        if let Holds::Slide(index) = holds
            && !others_alike(self.talk(), &candidate, index)
        {
            return Err(EditRefused::WouldChangeAnotherSlide);
        }
        if let Holds::Inserted(index) = holds
            && !others_kept(self.talk(), &candidate, index)
        {
            return Err(EditRefused::WouldChangeAnotherSlide);
        }
        if guarded(|| broken_bodies(&candidate))?
            .iter()
            .any(|body| !broken_before.contains(body))
        {
            return Err(EditRefused::WouldBreakASlide);
        }
        if let Some(check) = check
            && !guarded(|| check(&candidate))?
        {
            return Err(EditRefused::CouldNotBeTyped);
        }
        let old = old.to_string();
        let mut changed = self.deck.update_text(candidate);
        let joined = match join {
            Join::Run(EditRun::Continue(run)) => {
                self.history.extend(run, &old, self.deck.source())
            }
            Join::Last => self.history.fold(&old, self.deck.source()),
            Join::Run(EditRun::New) | Join::Alone => false,
        };
        if !joined {
            let opens_run = matches!(join, Join::Run(_));
            self.history.record(label, undo, before, after, opens_run);
        }
        // Typing in the notes goes on only through set_notes, which says so itself.
        self.typing_notes = None;
        self.show(after, &mut changed);
        Ok(changed)
    }

    /// Makes `index` the current slide after an edit, and adds it to `changed` if it isn't
    /// there: its text may be as it was, but it's now the slide to show.
    fn show(&mut self, index: usize, changed: &mut Vec<usize>) {
        let index = index.min(self.slide_count() - 1);
        if index != self.current {
            self.current = index;
            if !changed.contains(&index) {
                changed.push(index);
            }
        }
    }

    /// Takes the last edit back, and returns the slides to show again: `None` if there was
    /// nothing to undo. If reading the text it gives goes wrong, it's refused, and the talk
    /// and its history stay as they were.
    pub fn undo(&mut self) -> Result<Option<Vec<usize>>, EditRefused> {
        let Some(change) = self.history.next_undo() else {
            return Ok(None);
        };
        let talk = self.read_changed(change)?;
        let step = self
            .history
            .undo(self.deck.source())
            .expect("there's a step to undo");
        let changed = self.take_step(step.current, talk);
        self.resume_text(step.mark, true);
        Ok(Some(changed))
    }

    /// Makes the last undone edit again, and returns the slides to show again: `None` if
    /// there was nothing to redo. It's refused as [`Self::undo`] is.
    pub fn redo(&mut self) -> Result<Option<Vec<usize>>, EditRefused> {
        let Some(change) = self.history.next_redo() else {
            return Ok(None);
        };
        let talk = self.read_changed(change)?;
        let step = self
            .history
            .redo(self.deck.source())
            .expect("there's a step to redo");
        let changed = self.take_step(step.current, talk);
        self.resume_text(step.mark, false);
        Ok(Some(changed))
    }

    /// The talk as `change` leaves it.
    fn read_changed(&self, change: &Change) -> Result<Talk, EditRefused> {
        let text = guarded(|| change.apply(self.source()))?;
        read_after(self.talk(), text)
    }

    fn take_step(&mut self, current: usize, talk: Talk) -> Vec<usize> {
        let mut changed = self.deck.update_text(talk);
        self.typing_notes = None;
        self.editing = None;
        self.held_color = None;
        self.show(current, &mut changed);
        changed
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// What Undo would take back, such as "Add slide".
    pub fn undo_label(&self) -> Option<&'static str> {
        self.history.undo_label()
    }

    /// What Redo would make again.
    pub fn redo_label(&self) -> Option<&'static str> {
        self.history.redo_label()
    }

    /// Whether the talk has edits that its file doesn't.
    pub fn has_unsaved_changes(&self) -> bool {
        !self.history.is_saved()
    }

    /// Takes `text`, the working copy of edits that weren't saved, as the talk's text, and
    /// returns the slides to show again. It's taken as it is, whatever it does to the slides,
    /// and one undo gives back the text as it was. Nothing is written until the talk is saved.
    /// If reading it goes wrong, it's refused, and the talk stays as it was.
    pub fn recover(&mut self, text: String) -> Result<Vec<usize>, EditRefused> {
        self.replace_all(RECOVER_CHANGES, text)
    }

    /// Takes `text` as the talk's whole text, as one step named `label`, and returns the
    /// slides to show again. A whole-text step's ranges fit any text, so the steps before it
    /// can still be undone.
    fn replace_all(
        &mut self,
        label: &'static str,
        text: String,
    ) -> Result<Vec<usize>, EditRefused> {
        let old = self.source().to_string();
        if text == old {
            return Ok(Vec::new());
        }
        let talk = read_after(self.talk(), text.clone())?;
        let change = Change::single(SourceEdit {
            range: 0..old.len(),
            text,
        });
        let undo = change.inverse(&old);
        // Taken from outside the app, so its pictures are looked at again too.
        let mut changed = self.deck.update_parsed(talk);
        let at = self.current;
        self.history.record(label, undo, at, at, false);
        self.typing_notes = None;
        self.editing = None;
        self.held_color = None;
        self.show(at, &mut changed);
        Ok(changed)
    }

    /// Adds a slide with the layout called `layout` after the current one, with nothing in
    /// it, so each of its places offers to be typed in, and makes it the current slide. A
    /// name that isn't a layout's is taken as the default one.
    pub fn add_slide(&mut self, layout: &str) -> Result<Vec<usize>, EditRefused> {
        let layout = layout_by_name(layout).map_or(DEFAULT_LAYOUT, |layout| layout.name);
        let text = if layout == DEFAULT_LAYOUT {
            "\n".to_string()
        } else {
            format!("layout: {layout}\n---\n\n")
        };
        let at = self.current;
        let change = self.build(|talk| slides_format::insert_slide(talk, Some(at), &text))?;
        self.make(ADD_SLIDE, change, Holds::Count(1), at, at + 1, None)
    }

    /// Puts a copy of the current slide straight after it, and makes the copy current.
    pub fn duplicate_current(&mut self) -> Result<Vec<usize>, EditRefused> {
        let at = self.current;
        let change = self.build(|talk| slides_format::duplicate_slide(talk, at))?;
        self.make(DUPLICATE_SLIDE, change, Holds::Count(1), at, at + 1, None)
    }

    /// Removes the current slide, unless it's the only one. The one after it becomes
    /// current, or the one before when it was last.
    pub fn delete_current(&mut self) -> Result<Vec<usize>, EditRefused> {
        let at = self.current;
        let Some(change) = self.build(|talk| slides_format::delete_slide(talk, at))? else {
            return Ok(Vec::new());
        };
        let after = at.min(self.slide_count() - 2);
        self.make(DELETE_SLIDE, change, Holds::Count(-1), at, after, None)
    }

    /// Takes the current slide out, as [`Self::delete_current`] does, named for Undo as a cut.
    pub fn cut_current(&mut self) -> Result<Vec<usize>, EditRefused> {
        let at = self.current;
        let Some(change) = self.build(|talk| slides_format::delete_slide(talk, at))? else {
            return Ok(Vec::new());
        };
        let after = at.min(self.slide_count() - 2);
        self.make(CUT_SLIDE, change, Holds::Count(-1), at, after, None)
    }

    /// Whether the talk has a file at `path`, relative to it, as a picture's is written.
    pub fn has_file(&self, path: &str) -> bool {
        self.files.len(path).is_some()
    }

    /// The current slide's own text, to copy it: see [`slides_format::slide_text_for_copy`].
    /// Text being typed on it is in the talk already. `None` if working it out goes wrong.
    pub fn current_for_copy(&self) -> Option<String> {
        let at = self.current;
        worked_out(|| slides_format::slide_text_for_copy(self.talk(), at)).ok()
    }

    /// Adds a slide holding `text`, a slide's own text as [`Self::current_for_copy`] gives it,
    /// perhaps from another talk, after the current one, and makes it current. It's refused,
    /// with nothing changed, unless it makes exactly one slide there, every other slide keeping
    /// its text: a text that would split into several slides, or take in the slide after it,
    /// isn't pasted.
    ///
    /// Whatever `text` is, it's first made a slide's own text by [`crate::clipboard::one_slide`],
    /// with none of the talk's settings, so a pasted slide never sets them; text that doesn't
    /// read as one slide is refused.
    ///
    /// An empty talk, whose text is nothing but whitespace and perhaps a byte order mark, has one
    /// slide with nothing on it: the pasted slide takes its place as the talk's only slide, after
    /// the byte order mark, with the talk's line endings, and a break line before it if it starts
    /// with settings of its own, which only then are read as its settings block. It's refused
    /// unless that makes one slide with none of the talk's settings.
    pub fn paste_slide(&mut self, text: &str) -> Result<Vec<usize>, EditRefused> {
        let Some(slide) = guarded(|| crate::clipboard::one_slide(text))? else {
            return Err(EditRefused::WouldChangeTheSlides);
        };
        let text = slide.text;
        let source = self.source();
        let bom = if source.starts_with('\u{feff}') { "\u{feff}" } else { "" };
        if self.slide_count() == 1 && source[bom.len()..].trim().is_empty() {
            let eol = if source.contains("\r\n") { "\r\n" } else { "\n" };
            let first = guarded(|| if parse_talk(&text).slides.len() == 1 { text.clone() } else { format!("---\n{text}") })?;
            let new = format!("{bom}{}", first.replace('\n', eol));
            let sets_nothing = guarded(|| {
                let talk = parse_talk(&new);
                talk.slides.len() == 1 && talk_settings(&talk).is_empty()
            })?;
            if !sets_nothing {
                return Err(EditRefused::WouldChangeTheSlides);
            }
            let change = Change::single(SourceEdit { range: 0..source.len(), text: new });
            return self.make(PASTE_SLIDE, change, Holds::Count(0), 0, 0, None);
        }
        let at = self.current;
        let change = self.build(|talk| slides_format::insert_slide(talk, Some(at), &text))?;
        self.make(PASTE_SLIDE, change, Holds::Inserted(at + 1), at, at + 1, None)
    }

    /// Moves slide `from` so that `to` is its place afterwards, and makes it current.
    pub fn move_slide(&mut self, from: usize, to: usize) -> Result<Vec<usize>, EditRefused> {
        let change = self.build(|talk| slides_format::move_slide(talk, from, to))?;
        self.make(MOVE_SLIDE, change, Holds::Count(0), from, to, None)
    }

    /// Leaves the current slide out of shows, or puts it back in.
    pub fn set_current_hidden(&mut self, hidden: bool) -> Result<Vec<usize>, EditRefused> {
        let at = self.current;
        let change = self.build(|talk| slides_format::set_hidden(talk, at, hidden))?;
        let label = if hidden { HIDE_SLIDE } else { SHOW_SLIDE };
        self.edit_slide(label, at, change, EditRun::New)
    }

    /// Sets the notes of slide `index`, and makes it current. Notes typed into one slide
    /// with nothing else done in between are one step to undo.
    pub fn set_notes(&mut self, index: usize, notes: &str) -> Result<Vec<usize>, EditRefused> {
        let change = self.build(|talk| slides_format::set_notes(talk, index, notes))?;
        if change.is_empty() {
            // Nothing typed: the notes typing, wherever it is, goes on as it was.
            return Ok(Vec::new());
        }
        let run = self
            .typing_notes
            .filter(|&(slide, _)| slide == index)
            .map_or(EditRun::New, |(_, run)| EditRun::Continue(run));
        let changed = self.edit_slide(EDIT_NOTES, index, change, run)?;
        self.typing_notes = self.open_run().map(|run| (index, run));
        Ok(changed)
    }

    /// Whether `bytes` more can go on the slide whose text is being typed: a slide that holds
    /// more than [`MAX_SLIDE_BYTES`] can't be read. With no text being typed, there's room.
    pub fn has_room_for(&self, bytes: usize) -> bool {
        let slide = self.editing.as_ref().and_then(|editing| self.talk().slides.get(editing.slide));
        slide.is_none_or(|slide| slide.range.len().saturating_add(bytes) <= MAX_SLIDE_BYTES)
    }

    /// The text being typed on a slide, if any.
    pub fn text(&self) -> Option<&EditingText> {
        self.editing.as_ref()
    }

    /// Which of its slide's `text_flows` the text being typed is, if it's in the talk yet: an
    /// empty region's isn't until something is typed in it.
    pub fn text_flow(&self) -> Option<usize> {
        let editing = self.editing.as_ref()?;
        let flows = guarded(|| text_flows(self.talk(), editing.slide)).ok()?;
        editing.flow.find(&flows)
    }

    /// The number for a new time of typing.
    fn new_session(&mut self) -> u64 {
        self.next_session += 1;
        self.next_session
    }

    /// Starts typing in flow `flow` of slide `slide`, an index into its `text_flows`, with the
    /// caret at `pos`, and makes `slide` the current slide. Text being typed elsewhere ends
    /// first, and `flow` is taken as it was before that. In the flow already being typed in,
    /// it only moves the caret. Says whether the slide has such a flow.
    pub fn begin_text(&mut self, slide: usize, flow: usize, pos: Pos) -> bool {
        if slide >= self.slide_count() {
            return false;
        }
        let Ok(flows) = guarded(|| text_flows(self.talk(), slide)) else {
            return false;
        };
        let Some(key) = FlowKey::of(&flows, flow) else {
            return false;
        };
        if self
            .editing
            .as_ref()
            .is_some_and(|editing| editing.slide == slide && editing.flow == key)
        {
            self.apply_text(|edit| edit.move_to(pos, false)).ok();
            return true;
        }
        let ended = match self.finish_text() {
            Ok((_, removed)) => removed,
            Err(refused) => {
                self.say(refused);
                None
            }
        };
        let flow = match ended {
            Some((on, removed)) if on == slide && removed == flow => return false,
            Some((on, removed)) if on == slide && removed < flow => flow - 1,
            _ => flow,
        };
        self.select(slide);
        let Ok(flows) = guarded(|| text_flows(self.talk(), slide)) else {
            return false;
        };
        let Some(key) = FlowKey::of(&flows, flow) else {
            return false;
        };
        let edit = TextEdit::new(flows[flow].paras.clone()).move_to(pos, false);
        self.editing = Some(EditingText {
            slide,
            flow: key,
            edit,
            run: None,
            session: self.new_session(),
            composing: None,
        });
        true
    }

    /// Starts typing in region `region` of slide `slide`, which has nothing in it, and makes
    /// `slide` the current slide. Nothing is written until something is typed. A region with
    /// text in it is typed into at the end of its first run of text. Says whether the slide's
    /// layout has such a region to type in.
    pub fn begin_text_in_region(&mut self, slide: usize, region: &str) -> bool {
        if slide >= self.slide_count() {
            return false;
        }
        if let Err(refused) = self.end_text() {
            self.say(refused);
        }
        self.select(slide);
        let Ok(flows) = guarded(|| text_flows(self.talk(), slide)) else {
            return false;
        };
        let place = slides_format::FlowPlace::Region(region.to_string());
        if let Some(index) = flows.iter().position(|flow| flow.place == place) {
            let end = TextEdit::new(flows[index].paras.clone()).select_all().selection.end();
            return self.begin_text(slide, index, end);
        }
        // A region of the layout with no text in it offers to add some.
        if self.text_layout(slide).placeholder(region).is_none() {
            return false;
        }
        self.editing = Some(EditingText {
            session: self.new_session(),
            ..EditingText::in_empty_region(slide, region)
        });
        true
    }

    /// Makes `op` to the text being typed, writes what it gives to the talk, and returns the
    /// slides to show again. A run of typing in one flow is one step to undo, until the caret
    /// is moved, the text is styled, or [`Self::end_typing_run`] or anything else happens.
    ///
    /// What's typed always reads as the talk holds it: the flow its key finds holds exactly
    /// its paragraphs, or, before anything is written in an empty region, the region has no
    /// text and it holds one empty paragraph. If that doesn't hold before `op`, typing ends and
    /// it's refused, with nothing written. What's written must read back as the paragraphs `op`
    /// gave, with the slide's other text as it was. If it wouldn't, or it can't be written
    /// there, it's refused, and the talk and the text being typed stay as they were.
    pub fn apply_text(
        &mut self,
        op: impl FnOnce(&TextEdit) -> TextEdit,
    ) -> Result<Vec<usize>, EditRefused> {
        self.write_text(op, None)
    }

    /// Makes `op`, a paste, to the text being typed, as [`Self::apply_text`] does, but as a step
    /// of its own to undo, named "Paste": the run of typing before it ends, and typing after it
    /// starts another.
    pub fn paste_text(
        &mut self,
        op: impl FnOnce(&TextEdit) -> TextEdit,
    ) -> Result<Vec<usize>, EditRefused> {
        self.text_step(PASTE, op)
    }

    /// Deletes the selection of the text being typed, once it's been copied, as a step of its
    /// own to undo, named "Cut", as [`Self::paste_text`] is.
    pub fn cut_text(&mut self) -> Result<Vec<usize>, EditRefused> {
        self.text_step(CUT, TextEdit::delete)
    }

    fn text_step(
        &mut self,
        label: &'static str,
        op: impl FnOnce(&TextEdit) -> TextEdit,
    ) -> Result<Vec<usize>, EditRefused> {
        self.end_typing_run();
        let made = self.write_text(op, Some(label));
        self.end_typing_run();
        made
    }

    /// [`Self::apply_text`], the step named `label` if it's given. A colour held for the
    /// selection is given to it first.
    fn write_text(
        &mut self,
        op: impl FnOnce(&TextEdit) -> TextEdit,
        label: Option<&'static str>,
    ) -> Result<Vec<usize>, EditRefused> {
        let given = self.give_held_first();
        Ok(joined(given, self.write_now(op, label)?))
    }

    /// Keeps `color` to give the selection of the text being typed: the selection as it is,
    /// in the text as it is. It's given when [`Self::give_held_color`] is asked, or before
    /// anything else is done to the text or typing ends; by then the selection may have moved,
    /// and it's that selection that's coloured all the same. If the text itself has changed,
    /// it's dropped instead.
    pub fn hold_color(&mut self, color: String) {
        self.held_color = self.editing.as_ref().map(|editing| HeldColor {
            color,
            slide: editing.slide,
            flow: editing.flow.clone(),
            selection: editing.edit.selection,
            paras: editing.edit.paras.clone(),
        });
    }

    /// Gives the colour held for a selection to it, as one step, leaving the selection as it
    /// is now, and returns the slides to show again.
    pub fn give_held_color(&mut self) -> Result<Vec<usize>, EditRefused> {
        let Some(held) = self.held_color.take() else {
            return Ok(Vec::new());
        };
        let Some(editing) = &self.editing else {
            return Ok(Vec::new());
        };
        if editing.slide != held.slide || editing.flow != held.flow || editing.edit.paras != held.paras {
            // The text it was chosen for isn't there as it was.
            return Ok(Vec::new());
        }
        let (then, now) = (held.selection, editing.edit.selection);
        if then.is_caret() && then != now {
            // A colour for what was to be typed there, which wasn't.
            return Ok(Vec::new());
        }
        let color = held.color;
        self.write_now(
            |edit| {
                let colored = edit
                    .move_to(then.anchor, false)
                    .move_to(then.head, true)
                    .set_color(Some(color));
                if then == now {
                    colored
                } else {
                    colored.move_to(now.anchor, false).move_to(now.head, true)
                }
            },
            None,
        )
    }

    /// [`Self::give_held_color`], before something else is done: what it refuses is said.
    fn give_held_first(&mut self) -> Vec<usize> {
        self.give_held_color().unwrap_or_else(|refused| {
            self.say(refused);
            Vec::new()
        })
    }

    /// [`Self::write_text`], with no colour held given first.
    fn write_now(
        &mut self,
        op: impl FnOnce(&TextEdit) -> TextEdit,
        label: Option<&'static str>,
    ) -> Result<Vec<usize>, EditRefused> {
        // Anything done to the text drops what the system was composing: a commit types it.
        self.compose(None);
        let Some(editing) = self.editing.clone() else {
            return Ok(Vec::new());
        };
        let slide = editing.slide;
        let flows = (slide < self.slide_count())
            .then(|| guarded(|| text_flows(self.talk(), slide)).ok())
            .flatten();
        let Some(flows) = flows.filter(|flows| reads_as_typed(flows, &editing)) else {
            // The talk and what's typed have parted: nothing can be written safely.
            self.editing = None;
            self.held_color = None;
            return Err(EditRefused::CouldNotBeTyped);
        };
        let after = guarded(|| op(&editing.edit)).map_err(|_| EditRefused::CouldNotBeTyped)?;
        let kind = Kind::of(&editing.edit, &after);
        match kind {
            Kind::Nothing => return Ok(Vec::new()),
            Kind::Moved => {
                self.keep_typed(after, None);
                return Ok(Vec::new());
            }
            Kind::Typed | Kind::Formatted => {}
        }
        let found = editing.flow.find(&flows);
        let change = match (found, editing.flow.region()) {
            (Some(index), _) => write_flow(self.talk(), slide, &flows[index], &after.paras),
            // An empty region's first paragraph is written once it holds something.
            (None, Some(_)) if !holds_something(&after.paras) => {
                self.keep_typed(after, editing.run);
                return Ok(Vec::new());
            }
            (None, Some(region)) => write_into_region(self.talk(), slide, region, &after.paras),
            (None, None) => None,
        };
        let Some(change) = change else {
            return Err(EditRefused::CouldNotBeTyped);
        };
        if change.is_empty() {
            self.keep_typed(after, editing.run);
            return Ok(Vec::new());
        }
        let typed = kind == Kind::Typed;
        let run = match editing.run {
            Some(run) if typed && self.history.is_open(run) => EditRun::Continue(run),
            _ => EditRun::New,
        };
        let expected = storable(&after.paras);
        // Writing into an empty region adds its text; anything else keeps the slide's count.
        let count = flows.len() + usize::from(found.is_none());
        let key = editing.flow.clone();
        let reads_back = |candidate: &Talk| {
            let flows = text_flows(candidate, slide);
            flows.len() == count
                && key
                    .find(&flows)
                    .is_some_and(|index| flows[index].paras == expected)
        };
        let label = label.unwrap_or(if typed { TYPING } else { FORMAT_TEXT });
        let changed = self.make_text(label, slide, change, Join::Run(run), &reads_back)?;
        self.note_step(
            run,
            TextMark {
                slide,
                flow: editing.flow,
                before: editing.edit.selection,
                after: Some(after.selection),
                session: editing.session,
            },
        );
        let run = if typed { self.history.open_run() } else { None };
        self.keep_typed(after, run);
        Ok(changed)
    }

    /// Aligns the text being typed `align`, and returns the slides to show again. It's the whole
    /// region the text is in that's aligned, or the whole box, as one step to undo, and typing
    /// goes on where it was: the paragraphs typed in are as they were, only where they're drawn
    /// changes. Aligning as it's set to be aligned already changes nothing and makes no step.
    pub fn align_text(&mut self, align: Align) -> Result<Vec<usize>, EditRefused> {
        self.compose(None);
        let given = self.give_held_first();
        let Some(editing) = self.editing.clone() else {
            return Ok(given);
        };
        let slide = editing.slide;
        if slide >= self.slide_count() {
            return Ok(given);
        }
        let written = match align {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        };
        let change = guarded(|| {
            let talk = self.talk();
            let place = typed_place(talk, &editing)?;
            if text_align(talk, slide, &place) == Some(align) {
                return Some(Change { edits: Vec::new() });
            }
            set_align(talk, slide, &place, written)
        })?
        .ok_or(EditRefused::CouldNotBeMade)?;
        if change.is_empty() {
            return Ok(given);
        }
        let depth = self.history.depth();
        // Not through `make`: the typing isn't finished first, as it goes on.
        let changed =
            self.try_make(ALIGN_TEXT, change, Holds::Slide(slide), slide, slide, Join::Alone, None)?;
        if self.history.depth() > depth {
            let selection = editing.edit.selection;
            self.history.mark_last(TextMark {
                slide,
                flow: editing.flow,
                before: selection,
                after: Some(selection),
                session: editing.session,
            });
        }
        // What's typed next is a step of its own.
        self.end_typing_run();
        Ok(joined(given, changed))
    }

    /// How the text being typed is set to be aligned: its region's or its box's alignment, if
    /// any text is being typed.
    pub fn text_align(&self) -> Option<Align> {
        let editing = self.editing.as_ref()?;
        let talk = self.talk();
        guarded(|| text_align(talk, editing.slide, &typed_place(talk, editing)?))
            .ok()
            .flatten()
    }

    /// Shows `composing` in place of the selection of the text being typed, as the system's
    /// input method composes it, or, with `None`, drops what it was composing. Nothing is
    /// written to the talk: committing it is typing it, through [`Self::apply_text`].
    pub fn compose(&mut self, composing: Option<Composing>) {
        if let Some(editing) = &mut self.editing {
            editing.composing = composing;
        }
    }

    /// Ends the run of typing, so that what's typed next is a step of its own, as after a pause.
    pub fn end_typing_run(&mut self) {
        if let Some(editing) = &mut self.editing {
            editing.run = None;
        }
    }

    /// Stops typing on the slide, and returns the slides to show again. Everything typed is in
    /// the talk already; text left with no words at all is taken away, so that its region
    /// offers to add text again. Typing ends even if that's refused.
    pub fn end_text(&mut self) -> Result<Vec<usize>, EditRefused> {
        self.finish_text().map(|(changed, _)| changed)
    }

    /// Leaves the talk tidy while typing goes on, as before a save, a show or an export, and
    /// returns the slides to show again. Text that typing left with no words at all is taken
    /// away, as [`Self::end_text`] does; typing then goes on in its region, empty again, or
    /// ends if the region still has other text. Any other typing goes on as it was.
    pub fn settle_text(&mut self) -> Result<Vec<usize>, EditRefused> {
        // What the system was composing isn't text yet, and isn't kept.
        self.compose(None);
        let given = self.give_held_first();
        let Some(editing) = self.editing.clone() else {
            return Ok(given);
        };
        let (changed, removed) = self.tidy(&editing)?;
        let changed = joined(given, changed);
        if removed.is_some() {
            let region = editing.flow.region().unwrap_or_default();
            let flows = guarded(|| text_flows(self.talk(), editing.slide)).ok();
            self.editing = flows
                .filter(|flows| !region_has_text(flows, region))
                .map(|_| EditingText {
                    session: editing.session,
                    ..EditingText::in_empty_region(editing.slide, region)
                });
        }
        Ok(changed)
    }

    /// [`Self::end_text`], which also gives the slide and the flow it took away, if it did.
    fn finish_text(&mut self) -> Result<EndedText, EditRefused> {
        let given = self.give_held_first();
        let Some(editing) = self.editing.take() else {
            return Ok((given, None));
        };
        let (changed, removed) = self.tidy(&editing)?;
        Ok((joined(given, changed), removed))
    }

    /// Takes away the region text `editing` is typing in, if it has no words at all, and gives
    /// the slides to show again, and the slide and flow taken away. That's no step of its own:
    /// it joins the step before it typed in the same time of typing, where the history can
    /// take it, and if that leaves the step doing nothing, the step goes. A box of text is
    /// never taken away, as an empty box would be no text at all.
    fn tidy(&mut self, editing: &EditingText) -> Result<EndedText, EditRefused> {
        let slide = editing.slide;
        if editing.flow.region().is_none() || slide >= self.slide_count() {
            return Ok((Vec::new(), None));
        }
        let (change, index, count) = {
            let talk = self.talk();
            let flows = guarded(|| text_flows(talk, slide))?;
            let Some(index) = editing.flow.find(&flows) else {
                return Ok((Vec::new(), None));
            };
            if !holds_no_words(&flows[index].paras) {
                return Ok((Vec::new(), None));
            }
            (write_flow(talk, slide, &flows[index], &[]), index, flows.len())
        };
        let join = match editing.run {
            Some(run) if self.history.is_open(run) => Join::Run(EditRun::Continue(run)),
            _ if self
                .history
                .last_mark()
                .is_some_and(|mark| mark.session == editing.session) =>
            {
                Join::Last
            }
            _ => Join::Alone,
        };
        // Where this typing's own step put the text there, taking it away gives back the
        // slide's bytes from before it, so that the two together are no change at all.
        let change = match join {
            Join::Alone => change,
            _ => self.before_typed_in(editing, index).or(change),
        };
        let change = change.ok_or(EditRefused::CouldNotTidy)?;
        if change.is_empty() {
            return Ok((Vec::new(), None));
        }
        let depth = self.history.depth();
        let emptied = |candidate: &Talk| text_flows(candidate, slide).len() + 1 == count;
        let changed = self
            .make_text(TYPING, slide, change, join, &emptied)
            .map_err(|_| EditRefused::CouldNotTidy)?;
        // Made again, the step leaves nothing to type in.
        let now = self.history.depth();
        if now > depth {
            self.history.mark_last(TextMark {
                slide,
                flow: editing.flow.clone(),
                before: editing.edit.selection,
                after: None,
                session: editing.session,
            });
        } else if now == depth
            && let Some(last) = self.history.last_mark_mut()
        {
            last.after = None;
        }
        Ok((changed, Some((slide, index))))
    }

    /// The change back to the talk from before the newest step, if that step is what put
    /// flow `index` of `editing`'s slide there: before it, the slide had all its other text as
    /// it is now, and not that flow.
    fn before_typed_in(&self, editing: &EditingText, index: usize) -> Option<Change> {
        let undo = self.history.next_undo()?;
        let slide = editing.slide;
        let alike = guarded(|| {
            let before = reparse(self.talk(), undo.apply(self.source()));
            let mut now = text_flows(self.talk(), slide);
            now.remove(index);
            let then = text_flows(&before, slide);
            before.slides.len() == self.slide_count()
                && others_alike(self.talk(), &before, slide)
                && then.len() == now.len()
                && then
                    .iter()
                    .zip(&now)
                    .all(|(then, now)| same_place(&then.place, &now.place) && then.paras == now.paras)
        });
        alike.unwrap_or(false).then(|| undo.clone())
    }

    /// Takes `edit` as the text being typed, in the run `run`.
    fn keep_typed(&mut self, edit: TextEdit, run: Option<RunId>) {
        if let Some(editing) = &mut self.editing {
            editing.edit = edit;
            editing.run = run;
        }
    }

    /// Makes `change` to slide `slide`, typed on it, into the step `join` says, unless the talk
    /// it gives fails `check`. The text being typed goes on.
    fn make_text(
        &mut self,
        label: &'static str,
        slide: usize,
        change: Change,
        join: Join,
        check: &dyn Fn(&Talk) -> bool,
    ) -> Result<Vec<usize>, EditRefused> {
        self.try_make(label, change, Holds::Slide(slide), slide, slide, join, Some(check))
            .map_err(|_| EditRefused::CouldNotBeTyped)
    }

    /// Notes where the step just made by typing in run `run` was typed: on a step of its own,
    /// `mark`; on a run's step, how far the typing has gone.
    fn note_step(&mut self, run: EditRun, mark: TextMark) {
        match (run, self.history.open_run()) {
            (EditRun::Continue(run), Some(open)) if run == open => {
                if let Some(last) = self.history.last_mark_mut() {
                    last.after = mark.after;
                }
            }
            (_, Some(_)) => self.history.mark_last(mark),
            // The run came back to where it started, and its step went with it.
            (_, None) => {}
        }
    }

    /// Goes back to typing where `mark` says, after an undo, with `before`, or a redo of the
    /// step typed there. A step that took the text away leaves nothing to type in once it's
    /// made again, and text found where only an empty region was is someone else's: in both,
    /// typing stays ended.
    fn resume_text(&mut self, mark: Option<TextMark>, before: bool) {
        let Some(mark) = mark else {
            return;
        };
        let selection = if before { Some(mark.before) } else { mark.after };
        let Some(selection) = selection else {
            return;
        };
        let slide = mark.slide;
        if slide >= self.slide_count() {
            return;
        }
        let Ok(flows) = guarded(|| text_flows(self.talk(), slide)) else {
            return;
        };
        self.editing = match (mark.flow.find(&flows), mark.flow.region()) {
            (Some(index), _) => {
                let edit = TextEdit::new(flows[index].paras.clone())
                    .move_to(selection.anchor, false)
                    .move_to(selection.head, true);
                Some(EditingText {
                    slide,
                    flow: mark.flow,
                    edit,
                    run: None,
                    session: mark.session,
                    composing: None,
                })
            }
            (None, Some(region)) if !region_has_text(&flows, region) => Some(EditingText {
                session: mark.session,
                ..EditingText::in_empty_region(slide, region)
            }),
            _ => None,
        };
    }

    /// The talk's own components, and whether each is built.
    pub fn components(&self) -> &[ComponentInfo] {
        self.deck.components()
    }

    /// What's wrong with the talk's own components, as found when they were last looked at.
    pub fn component_problems(&self) -> &[Diagnostic] {
        self.deck.component_problems()
    }

    /// Looks at the talk's components again, after a build or a change to one of them.
    pub fn reload_components(&mut self) -> Reload {
        let changed = self.deck.refresh_components();
        if changed.is_empty() {
            Reload::Unchanged
        } else {
            Reload::Changed(changed)
        }
    }

    /// Whether this talk came from a packed file, rather than a folder.
    pub fn is_packed(&self) -> bool {
        matches!(self.place, TalkPlace::Packed(_))
    }

    pub fn place(&self) -> &TalkPlace {
        &self.place
    }

    /// A save of the talk as it is now, where it's kept: for a folder, only its `slides.md`.
    pub fn save_job(&self) -> SaveJob {
        SaveJob::new(
            self.source().to_string(),
            self.on_disk.clone(),
            self.place.clone(),
            None,
        )
    }

    /// A save of the talk as it is now, somewhere new.
    pub fn save_as_job(&self, target: SaveTarget) -> SaveJob {
        SaveJob::new(
            self.source().to_string(),
            self.on_disk.clone(),
            self.place.clone(),
            Some(target),
        )
    }

    /// Takes in a save that worked. The talk is saved if its text is still what was written,
    /// and after a Save As it's kept in the new place from now on.
    pub fn saved(&mut self, saved: Saved) {
        if let Some((place, files)) = saved.moved_to {
            self.place_name = place.name();
            self.place = place;
            self.files = files.clone();
            self.deck.set_files(Box::new(files));
        }
        if self.source() == saved.written {
            self.history.mark_saved();
        } else {
            // Edited while it saved: no step leads to what the file holds now.
            self.history.forget_saved();
        }
        self.on_disk = saved.written;
        self.outside = None;
    }

    /// What the talk's file held when the app last wrote or read it.
    pub fn stamp_on_disk(&self) -> &str {
        &self.on_disk
    }

    /// What the file holds, when it changed under unsaved edits and nobody has said yet whether
    /// to keep those or load it.
    pub fn outside_change(&self) -> Option<&str> {
        self.outside.as_deref()
    }

    /// Loads `text`, what the file holds after a change from outside, in place of the edits
    /// made here. It's one step, which brings the edits back when it's undone. If reading it
    /// goes wrong, it's refused, and the talk and the question about the change stay as they were.
    pub fn take_theirs(&mut self, text: String) -> Result<(), EditRefused> {
        if text.is_empty() {
            self.outside = None;
            return Ok(());
        }
        self.replace_all(LOAD_CHANGED_FILE, text.clone())?;
        self.outside = None;
        self.on_disk = text;
        // What the file holds is what's here now.
        self.history.mark_saved();
        Ok(())
    }

    /// Keeps the edits made here over the file's change, which isn't asked about again. The
    /// file holds neither, so the talk has unsaved changes until it's saved.
    pub fn keep_mine(&mut self) {
        if let Some(theirs) = self.outside.take() {
            self.on_disk = theirs;
            self.history.forget_saved();
        }
    }

    /// What an export needs to know about the talk as it is now.
    pub fn export_input(&self) -> ExportInput {
        ExportInput {
            source: self.source().to_string(),
            files: self.files.clone(),
            talk_fonts: self.talk_fonts.clone(),
        }
    }

    /// Takes a new read of the talk's file: its text, or `None` if it couldn't be read. An
    /// empty read is an editor caught halfway through a save, not an empty talk.
    ///
    /// A file that holds what the app last wrote or read brings nothing new. A change made
    /// while the talk has unsaved edits isn't taken, but asked about. If reading what it holds
    /// goes wrong, the talk stays as it was.
    pub fn reload(&mut self, contents: Option<String>) -> Reload {
        let Some(source) = contents.filter(|source| !source.is_empty()) else {
            return Reload::KeptLastGood;
        };
        let holds_mine = source == self.source();
        if holds_mine || source == self.on_disk {
            let Ok(talk) = read_after(self.talk(), self.source().to_string()) else {
                return Reload::Unreadable;
            };
            return self.take_same(source, holds_mine, talk);
        }
        if self.has_unsaved_changes() {
            self.outside = Some(source);
            return Reload::Conflict;
        }
        let Ok(talk) = read_after(self.talk(), source.clone()) else {
            return Reload::Unreadable;
        };
        self.take(source, talk)
    }

    /// Takes a read of the file that holds what's here, `holds_mine`, or what the app last
    /// wrote or read. `talk` is what's here, read again.
    fn take_same(&mut self, source: String, holds_mine: bool, talk: Talk) -> Reload {
        if holds_mine {
            // The file holds what's here, however it came to: nothing is unsaved.
            self.history.mark_saved();
        }
        self.on_disk = source;
        // The app's own save, or changed back: a picture the talk shows may still have been
        // replaced.
        self.outside = None;
        let changed = self.deck.update_parsed(talk);
        if changed.is_empty() {
            Reload::Unchanged
        } else {
            Reload::Changed(changed)
        }
    }

    /// Takes `source`, what the file holds, as the talk's text. `talk` is what it reads as.
    fn take(&mut self, source: String, talk: Talk) -> Reload {
        self.on_disk = source.clone();
        let same_text = source == self.source();
        // Even the same text goes through: a picture it shows may have been replaced.
        let mut changed = self.deck.update_parsed(talk);
        if !same_text {
            // Its steps are ranges of the text it had, which no longer fit.
            self.history = History::new();
            self.typing_notes = None;
            // Nor does the text being typed: what was typed is in the talk, or the file.
            self.editing = None;
            self.held_color = None;
        }
        if same_text && changed.is_empty() {
            return Reload::Unchanged;
        }
        let last = self.slide_count() - 1;
        if self.current > last {
            self.current = last;
            // Its text may be as it was, but this is now the slide to show.
            if !changed.contains(&last) {
                changed.push(last);
            }
        }
        Reload::Changed(changed)
    }
}

/// What ending the text being typed did: the slides to show again, and the slide and flow it
/// took away, if it emptied one.
type EndedText = (Vec<usize>, Option<(usize, usize)>);

/// Whether `editing` reads as the talk holds it in `flows`, its slide's text: the flow its key
/// finds holds exactly its paragraphs, or, in an empty region before anything is written, the
/// region has no text and it holds one empty paragraph.
fn reads_as_typed(flows: &[slides_format::TextFlow], editing: &EditingText) -> bool {
    match (editing.flow.find(flows), editing.flow.region()) {
        (Some(index), _) => flows[index].paras == storable(&editing.edit.paras),
        (None, Some(region)) => {
            !region_has_text(flows, region) && !holds_something(&editing.edit.paras)
        }
        (None, None) => false,
    }
}

/// A colour chosen for a selection of text being typed, and the text it was chosen in.
#[derive(Clone, Debug)]
struct HeldColor {
    color: String,
    slide: usize,
    flow: FlowKey,
    selection: Selection,
    paras: Vec<Para>,
}

/// The slides `a` and `b` say to show again, each once.
fn joined(mut a: Vec<usize>, b: Vec<usize>) -> Vec<usize> {
    for slide in b {
        if !a.contains(&slide) {
            a.push(slide);
        }
    }
    a
}

/// Where `editing`'s text is on its slide: its region, or the box it's in.
fn typed_place(talk: &Talk, editing: &EditingText) -> Option<FlowPlace> {
    match &editing.flow {
        FlowKey::Region { name, .. } => Some(FlowPlace::Region(name.clone())),
        FlowKey::Box { .. } => {
            let flows = text_flows(talk, editing.slide);
            editing.flow.find(&flows).map(|index| flows[index].place.clone())
        }
    }
}

/// Whether `a` and `b` are the same kind of place: the same region, or both boxes, whose
/// ranges move with the text before them.
fn same_place(a: &slides_format::FlowPlace, b: &slides_format::FlowPlace) -> bool {
    use slides_format::FlowPlace::{Region, Tag};
    match (a, b) {
        (Region(a), Region(b)) => a == b,
        (Tag(_), Tag(_)) => true,
        _ => false,
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

/// What the slides of `talk` that can't be read hold, past any settings, without the blank
/// lines around it. A slide like that moves, or is copied, as it is, and is still one of these.
fn broken_bodies(talk: &Talk) -> Vec<String> {
    if talk.slides.iter().all(|slide| slide.error.is_none()) {
        return Vec::new();
    }
    split_slides(&talk.source)
        .iter()
        .zip(&talk.slides)
        .filter(|(_, slide)| slide.error.is_some())
        .map(|(raw, _)| talk.source[raw.body.clone()].trim().to_string())
        .collect()
}

/// Whether every slide of `after` but `index` has the bytes it had in `before`, given both
/// have as many slides.
fn others_alike(before: &Talk, after: &Talk, index: usize) -> bool {
    fn text(talk: &Talk, at: usize) -> Option<&str> {
        talk.source.get(talk.slides[at].range.clone())
    }
    before.slides.len() == after.slides.len()
        && (0..before.slides.len())
            .filter(|&at| at != index)
            .all(|at| text(before, at) == text(after, at))
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

/// Whether every slide of `before` keeps its text in `after`, which has a slide added at
/// `index`, but for the line endings at a slide's end, which a slide that ended the file gains
/// once another follows it.
fn others_kept(before: &Talk, after: &Talk, index: usize) -> bool {
    fn text(talk: &Talk, at: usize) -> Option<&str> {
        let range = talk.slides.get(at)?.range.clone();
        Some(talk.source.get(range)?.trim_end_matches(['\r', '\n']))
    }
    after.slides.len() == before.slides.len() + 1
        && (0..before.slides.len()).all(|at| {
            let moved_to = if at < index { at } else { at + 1 };
            text(before, at).is_some() && text(before, at) == text(after, moved_to)
        })
}

/// The folder's own name, even for a path like `talk/..`.
fn folder_name(folder: &Path) -> String {
    folder
        .canonicalize()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| folder.display().to_string())
}

fn is_font_file(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("ttf") || extension.eq_ignore_ascii_case("otf")
        })
}

/// Whether `name`, a path within a talk, is a font sitting right in its fonts folder.
fn is_talk_font(name: &str) -> bool {
    name.strip_prefix(FONTS_FOLDER)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|file| !file.contains('/') && is_font_file(file))
}

fn font_names_in_folder(folder: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(folder.join(FONTS_FOLDER)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_font_file(name))
        .map(|name| format!("{FONTS_FOLDER}/{name}"))
        .collect()
}

/// Reads the talk's fonts in name order, as the order can decide between two similar fonts.
/// They're read through `files`, which stops a link from reaching outside the talk. Past
/// [`TALK_FONT_BYTES`] the rest are left out.
fn read_talk_fonts(mut names: Vec<String>, files: &dyn TalkFiles) -> Vec<Arc<Vec<u8>>> {
    names.sort();
    let mut fonts = Vec::new();
    let mut total = 0;
    for name in names {
        let Some(data) = files.read(&name) else {
            continue;
        };
        total += data.len();
        if total > TALK_FONT_BYTES {
            break;
        }
        fonts.push(Arc::new(data));
    }
    fonts
}
