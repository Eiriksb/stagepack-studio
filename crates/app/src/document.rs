//! An open talk, and which of its slides is showing.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use slides_components::Diagnostic;
use slides_engine::{
    ComponentInfo, Deck, FolderFiles, Fonts, Layer, LayerTiming, Moment, PackError, Pixmap,
    SlideTimeline, TalkFiles, TextLayout, Transition, layout_by_name, open_packed,
    read_talk_text,
};
use slides_format::{
    Change, Pos, SourceEdit, Talk, TextEdit, parse_talk, reparse, split_slides, storable,
    text_flows, write_flow, write_into_region,
};

use crate::export::ExportInput;
use crate::history::{History, RunId};
use crate::present::TalkShape;
use crate::saving::{SaveJob, SaveTarget, Saved};
use crate::text_editing::{
    COULD_NOT_TYPE, EditingText, FORMAT_TEXT, FlowKey, Kind, TYPING, TextMark, holds_something,
    only_empty_lines,
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

/// How many slides an edit is meant to leave, and which of them it may change.
#[derive(Clone, Copy)]
enum Holds {
    /// The number of slides changes by this, and any slide may change.
    Count(isize),
    /// Only this slide may change, and the number of slides stays the same.
    Slide(usize),
}

/// What Undo shows for each kind of edit.
const ADD_SLIDE: &str = "Add slide";
const DUPLICATE_SLIDE: &str = "Duplicate slide";
const DELETE_SLIDE: &str = "Delete slide";
const MOVE_SLIDE: &str = "Move slide";
const HIDE_SLIDE: &str = "Hide slide";
const SHOW_SLIDE: &str = "Show slide";
const EDIT_NOTES: &str = "Edit notes";
const RECOVER_CHANGES: &str = "Recover changes";
const LOAD_CHANGED_FILE: &str = "Load the changed file";

thread_local! {
    /// Whether the next change worked out, or text read, on this thread panics, for tests.
    static PANIC_IN_NEXT_CHANGE: Cell<bool> = const { Cell::new(false) };
}

/// Makes the next change a [`Document`] command works out on this thread panic, or the next
/// text it reads, as a fault in the format crate would, so tests can see that it's refused.
/// Only for tests.
#[doc(hidden)]
pub fn panic_in_next_change() {
    PANIC_IN_NEXT_CHANGE.with(|flag| flag.set(true));
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
            self.end_text().ok();
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
    pub fn editor_layers(&mut self, scale: f32) -> Vec<Arc<Layer>> {
        self.deck.editor_layers(self.current, scale)
    }

    /// Where slide `index`'s editable text is drawn, for a caret, a selection and a click.
    pub fn text_layout(&mut self, index: usize) -> Arc<TextLayout> {
        self.deck.text_layout(index)
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
    /// the talk has no such slide.
    pub fn thumbnail(&mut self, index: usize, width: u32) -> Option<Arc<Pixmap>> {
        self.deck.thumbnail(index, width)
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

    /// What the user should hear about the current slide.
    pub fn notices(&mut self) -> Vec<String> {
        // The window shows the slide in its whole state, so these are its notices.
        self.deck.notices_whole(self.current)
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

    /// The change `build` works out from the talk as it is, for one of the commands. A command
    /// that's refused ends any typing in the notes.
    fn build<T>(&mut self, build: impl FnOnce(&Talk) -> T) -> Result<T, EditRefused> {
        let built = built(self.talk(), build);
        if built.is_err() {
            self.typing_notes = None;
        }
        built
    }

    /// Makes `change` as [`Self::edit`] does, held to what `holds` says. `before` is the slide
    /// it's about in the text as it is, and `after` the one it's about once made, which becomes
    /// the current slide. With `run`, the step opens a run, or joins the one given; without, it
    /// opens none.
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
        let made = self.try_make(label, change, holds, before, after, run, None);
        if made.is_err() {
            self.typing_notes = None;
        }
        if made.is_ok() && !empty {
            // The text being typed may have moved, or gone: it's typed into again once clicked.
            self.editing = None;
        }
        made
    }

    /// [`Self::make`], but for ending the typing when it's refused. With `check`, the talk the
    /// change gives is refused unless `check` holds for it, before anything is taken.
    #[allow(clippy::too_many_arguments)]
    fn try_make(
        &mut self,
        label: &'static str,
        change: Change,
        holds: Holds,
        before: usize,
        after: usize,
        run: Option<EditRun>,
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
        };
        if old_count.checked_add_signed(slides) != Some(candidate.slides.len()) {
            return Err(EditRefused::WouldChangeTheSlides);
        }
        if let Holds::Slide(index) = holds
            && !others_alike(self.talk(), &candidate, index)
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
        let joined = match run {
            Some(EditRun::Continue(run)) => self.history.extend(run, &old, self.deck.source()),
            _ => false,
        };
        if !joined {
            self.history.record(label, undo, before, after, run.is_some());
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
        self.resume_text(step.mark.map(|mark| (mark.slide, mark.flow, mark.before)));
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
        self.resume_text(step.mark.map(|mark| (mark.slide, mark.flow, mark.after)));
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
        self.show(at, &mut changed);
        Ok(changed)
    }

    /// Adds a slide with the layout called `layout` after the current one, with an empty
    /// heading, and makes it the current slide. A name that isn't a layout's is taken as the
    /// default one.
    pub fn add_slide(&mut self, layout: &str) -> Result<Vec<usize>, EditRefused> {
        let layout = layout_by_name(layout).map_or(DEFAULT_LAYOUT, |layout| layout.name);
        let text = if layout == DEFAULT_LAYOUT {
            "# \n".to_string()
        } else {
            format!("layout: {layout}\n---\n\n# \n")
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
        let ended = self.finish_text().ok().and_then(|(_, removed)| removed);
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
        });
        true
    }

    /// Starts typing in region `region` of slide `slide`, which has nothing in it, and makes
    /// `slide` the current slide. Nothing is written until something is typed. A region with
    /// text in it is typed into at the end of its first run of text.
    pub fn begin_text_in_region(&mut self, slide: usize, region: &str) -> bool {
        if slide >= self.slide_count() {
            return false;
        }
        self.end_text().ok();
        self.select(slide);
        let Ok(flows) = guarded(|| text_flows(self.talk(), slide)) else {
            return false;
        };
        let place = slides_format::FlowPlace::Region(region.to_string());
        if let Some(index) = flows.iter().position(|flow| flow.place == place) {
            let end = TextEdit::new(flows[index].paras.clone()).select_all().selection.end();
            return self.begin_text(slide, index, end);
        }
        self.editing = Some(EditingText::in_empty_region(slide, region));
        true
    }

    /// Makes `op` to the text being typed, writes what it gives to the talk, and returns the
    /// slides to show again. A run of typing in one flow is one step to undo, until the caret
    /// is moved, the text is styled, or [`Self::end_typing_run`] or anything else happens.
    ///
    /// What's written must read back as the paragraphs `op` gave. If it wouldn't, or it can't
    /// be written there, it's refused, and the talk and the text being typed stay as they were.
    pub fn apply_text(
        &mut self,
        op: impl FnOnce(&TextEdit) -> TextEdit,
    ) -> Result<Vec<usize>, EditRefused> {
        let Some(editing) = self.editing.clone() else {
            return Ok(Vec::new());
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
        let slide = editing.slide;
        let change = {
            let talk = self.talk();
            let flows = guarded(|| text_flows(talk, slide)).map_err(|_| EditRefused::CouldNotBeTyped)?;
            match (editing.flow.find(&flows), editing.flow.region()) {
                (Some(index), _) => write_flow(talk, slide, &flows[index], &after.paras),
                // An empty region's first paragraph is written once it holds something.
                (None, Some(_)) if !holds_something(&after.paras) => {
                    self.keep_typed(after, editing.run);
                    return Ok(Vec::new());
                }
                (None, Some(region)) => write_into_region(talk, slide, region, &after.paras),
                (None, None) => None,
            }
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
        let key = editing.flow.clone();
        let reads_back = |candidate: &Talk| {
            let flows = text_flows(candidate, slide);
            key.find(&flows).is_some_and(|index| flows[index].paras == expected)
        };
        let label = if typed { TYPING } else { FORMAT_TEXT };
        let changed = self.make_text(label, slide, change, run, &reads_back)?;
        self.note_step(
            run,
            TextMark {
                slide,
                flow: editing.flow,
                before: editing.edit.selection,
                after: after.selection,
            },
        );
        let run = if typed { self.history.open_run() } else { None };
        self.keep_typed(after, run);
        Ok(changed)
    }

    /// Ends the run of typing, so that what's typed next is a step of its own, as after a pause.
    pub fn end_typing_run(&mut self) {
        if let Some(editing) = &mut self.editing {
            editing.run = None;
        }
    }

    /// Stops typing on the slide, and returns the slides to show again. Everything typed is in
    /// the talk already; a region left with nothing but empty lines is emptied, so that it
    /// offers to add text again.
    pub fn end_text(&mut self) -> Result<Vec<usize>, EditRefused> {
        self.finish_text().map(|(changed, _)| changed)
    }

    /// [`Self::end_text`], which also gives the slide and the flow it took away, if it did.
    fn finish_text(&mut self) -> Result<EndedText, EditRefused> {
        let Some(editing) = self.editing.take() else {
            return Ok((Vec::new(), None));
        };
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
            if !only_empty_lines(&flows[index].paras) {
                return Ok((Vec::new(), None));
            }
            (write_flow(talk, slide, &flows[index], &[]), index, flows.len())
        };
        let Some(change) = change.filter(|change| !change.is_empty()) else {
            return Ok((Vec::new(), None));
        };
        let run = match editing.run {
            Some(run) if self.history.is_open(run) => EditRun::Continue(run),
            _ => EditRun::New,
        };
        let emptied = |candidate: &Talk| text_flows(candidate, slide).len() + 1 == count;
        let changed = self.make_text(TYPING, slide, change, run, &emptied)?;
        let selection = editing.edit.selection;
        self.note_step(
            run,
            TextMark {
                slide,
                flow: editing.flow,
                before: selection,
                after: selection,
            },
        );
        Ok((changed, Some((slide, index))))
    }

    /// Takes `edit` as the text being typed, in the run `run`.
    fn keep_typed(&mut self, edit: TextEdit, run: Option<RunId>) {
        if let Some(editing) = &mut self.editing {
            editing.edit = edit;
            editing.run = run;
        }
    }

    /// Makes `change` to slide `slide`, typed on it, unless the talk it gives fails `check`.
    /// The text being typed goes on.
    fn make_text(
        &mut self,
        label: &'static str,
        slide: usize,
        change: Change,
        run: EditRun,
        check: &dyn Fn(&Talk) -> bool,
    ) -> Result<Vec<usize>, EditRefused> {
        self.try_make(label, change, Holds::Slide(slide), slide, slide, Some(run), Some(check))
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

    /// Goes back to typing at `at`, a slide, one of its flows and a selection in it, after an
    /// undo or a redo of a step typed there.
    fn resume_text(&mut self, at: Option<(usize, FlowKey, slides_format::Selection)>) {
        let Some((slide, key, selection)) = at else {
            return;
        };
        if slide >= self.slide_count() {
            return;
        }
        let Ok(flows) = guarded(|| text_flows(self.talk(), slide)) else {
            return;
        };
        self.editing = match (key.find(&flows), key.region()) {
            (Some(index), _) => {
                let edit = TextEdit::new(flows[index].paras.clone())
                    .move_to(selection.anchor, false)
                    .move_to(selection.head, true);
                Some(EditingText {
                    slide,
                    flow: key,
                    edit,
                    run: None,
                })
            }
            (None, Some(region)) => Some(EditingText::in_empty_region(slide, region)),
            (None, None) => None,
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
