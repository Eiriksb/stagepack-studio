//! An open talk, and which of its slides is showing.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use slides_components::{BuildEntry, BuildIndex, Diagnostic, INDEX_FILE, source_hash};
use slides_engine::{
    AddedSet, Align, ComponentInfo, ComponentState, Deck, SchemaLoad, ElementBox, FolderFiles, Fonts, Layer, LayerTiming, Moment, PackError, Pixmap,
    SlideTimeline, TalkFiles, TextLayout, Transition, WithAdded, layout_by_name, open_packed, text_align,
    read_talk_text,
};
use slides_format::{
    Change, FlowPlace, TextFlow, MAX_SLIDE_BYTES, Para, Pos, Selection, SourceEdit, Talk, TextEdit, is_visible_char, parse_talk, reparse, set_align,
    Block, NewTag, Tag, block_text, delete_block, find_tag, insert_block, set_tag_attr, set_tag_body, split_slides, storable,
    tag_text, text_flows_with, write_flow_with, write_into_region, MAX_TABLE_COLUMNS, MAX_TABLE_ROWS, new_table, table_size,
};
use toolkit::{ComponentSchema, Theme};

use crate::added_files::{self, FileKind, MAX_ADDED_BYTES, NotAdded, Placed, talk_uses};
use crate::clipboard::{self, CopiedFile, MAX_CLIPBOARD_FILES, MAX_CLIPBOARD_FILES_BYTES, PastedFile, PastedFiles};
use crate::export::ExportInput;
use crate::history::{History, RunId, one_edit};
use crate::insert::{self, INSERT_COMPONENT, INSERT_PICTURE, INSERT_SHAPE, INSERT_TABLE, INSERT_TEXT_BOX, TEXT_BOX_SIZE};
use crate::palette::{self, COMPONENT_SIZE, LISTED_BUILT_INS};
pub use crate::palette::PaletteEntry;
use crate::present::TalkShape;
use crate::saving::{SaveJob, SaveTarget, Saved};
use crate::table_tools::{CLICK_IN_A_CELL, HEADING_ROW_STAYS, LONG_ROW, MOST_COLUMNS, MOST_ROWS, ONE_COLUMN_STAYS};
use crate::selection::{self, SelectMark};
pub use crate::selection::Selected;
use crate::text_editing::{
    ALIGN_TEXT, COULD_NOT_TYPE, CUT, Composing, EditingText, FORMAT_TEXT, FlowKey, Kind, PASTE, TYPING, TextMark,
    holds_no_words, holds_something, region_has_text,
};

pub(crate) const SLIDES_FILE: &str = "slides.md";
/// What tells one state of the talk's text and its added files from another.
type FilesKey = (u64, usize, u64, usize);
/// What tells apart what the table tools are about: the state of the text, the text being typed
/// and the thing selected.
type TableKey = (u64, Option<(usize, FlowKey)>, Option<Selected>);
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
    /// The thing selected couldn't be taken away without changing what's around it.
    CouldNotBeRemoved,
    /// A table's heading row can't be taken away, nor a row put above it.
    KeepsHeadingRow,
    /// A table's only column can't be taken away.
    KeepsOneColumn,
    /// A table has as many columns as it can have.
    TooManyColumns,
    /// A table has as many rows as it can have.
    TooManyRows,
    /// A table with a row of more cells than its heading row can't have its columns changed.
    RowTooLong,
    /// A table selected with no caret in it has no row or column of its own to act on.
    NeedsACell,
    /// A component's build couldn't be loaded, so it couldn't be put on the slide.
    ComponentNotLoaded,
    /// What was pasted isn't one thing a slide can hold.
    NotOneThing,
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
            EditRefused::CouldNotBeRemoved => {
                "That couldn't be taken away without changing what's around it."
            }
            EditRefused::KeepsHeadingRow => HEADING_ROW_STAYS,
            EditRefused::KeepsOneColumn => ONE_COLUMN_STAYS,
            EditRefused::TooManyColumns => MOST_COLUMNS,
            EditRefused::TooManyRows => MOST_ROWS,
            EditRefused::RowTooLong => LONG_ROW,
            EditRefused::NeedsACell => CLICK_IN_A_CELL,
            EditRefused::ComponentNotLoaded => "That component couldn't be loaded, so it wasn't added.",
            EditRefused::NotOneThing => clipboard::NOT_ONE_THING,
        })
    }
}

impl std::error::Error for EditRefused {}

/// Why a picture wasn't put on a slide. Nothing of it was added, and there's nothing to undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertFailed {
    /// The file couldn't be added to the talk.
    File(NotAdded),
    /// The file was fine, but the slide couldn't take the picture.
    Edit(EditRefused),
}

impl std::fmt::Display for InsertFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InsertFailed::File(why) => why.fmt(f),
            InsertFailed::Edit(why) => why.fmt(f),
        }
    }
}

impl std::error::Error for InsertFailed {}

/// What a paste that brought files with it has to say of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Brought {
    /// Some of them couldn't be added, or the text couldn't be made to name them.
    pub left_out: bool,
    /// They were too big to come with it.
    pub too_large: bool,
    /// What to say of a component the talk has a different one of, by the same name.
    pub different: Option<String>,
    /// A pasted component had the name of one every talk has.
    pub built_in: bool,
    /// A component the slide shows came as a build without its source, so it couldn't be brought.
    pub component_left_out: bool,
    /// The stems of the builds that came without their sources.
    builds_alone: Vec<String>,
}

impl Brought {
    /// Marks a component left out if the current slide of `document`, as pasted, shows one missing
    /// that came only as a build.
    fn look_for_lone_builds(&mut self, document: &mut Document) {
        if self.builds_alone.is_empty() {
            return;
        }
        let notices = document.notices();
        let missing = clipboard::missing_components(&notices);
        self.component_left_out = missing.iter().any(|name| self.builds_alone.iter().any(|stem| stem.eq_ignore_ascii_case(name)));
    }

    /// The one thing to say of it, if any: a different component first, then one with a
    /// built-in's name, then files too big, then pictures left out, then a component left out.
    pub fn said(&self) -> Option<String> {
        let said = |is: bool, sentence: &str| is.then(|| sentence.to_string());
        self.different
            .clone()
            .or_else(|| said(self.built_in, clipboard::BUILT_IN_NAME))
            .or_else(|| said(self.too_large, clipboard::FILES_TOO_LARGE))
            .or_else(|| said(self.left_out, clipboard::MISSING_PICTURES))
            .or_else(|| said(self.component_left_out, clipboard::MISSING_COMPONENT))
    }
}

/// The files a paste carries, as a talk's files, to tell which of them its text uses.
struct CarriedFiles<'a>(&'a [PastedFile]);

impl CarriedFiles<'_> {
    fn find(&self, path: &str) -> Option<&PastedFile> {
        let path = slides_engine::normalise(path)?;
        self.0.iter().find(|file| file.path == path)
    }
}

impl TalkFiles for CarriedFiles<'_> {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.find(path).map(|file| file.bytes.clone())
    }

    fn len(&self, path: &str) -> Option<u64> {
        self.find(path).map(|file| file.bytes.len() as u64)
    }

    fn names_in(&self, folder: &str) -> Vec<String> {
        let prefix = format!("{folder}/");
        self.0.iter().filter(|file| file.path.starts_with(&prefix)).map(|file| file.path.clone()).collect()
    }
}

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
const DELETE: &str = "Delete";
const DUPLICATE: &str = "Duplicate";
const CHANGE_SETTING: &str = "Change setting";
const ADD_ROW: &str = "Add row";
const ADD_COLUMN: &str = "Add column";
const DELETE_ROW: &str = "Delete row";
const DELETE_COLUMN: &str = "Delete column";

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
    /// The talk's files, with those added to it, which exports read from their own thread.
    files: Arc<dyn TalkFiles + Send + Sync>,
    /// The files added to the talk while it's open, which [`Self::files`] reads first.
    added: Arc<AddedSet>,
    /// The talk's files where it's kept, without those added: what a save writes over.
    own_files: Arc<dyn TalkFiles + Send + Sync>,
    /// The files added that no save has written yet.
    pending: BTreeSet<String>,
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
    /// The thing picked on a slide, if any: the editor's view of the talk, not part of it.
    selected: Option<Selected>,
    /// Counts the times a thing was picked or let go of, rather than followed through an edit.
    picks: u64,
    /// The number the next time of typing on a slide gets.
    next_session: u64,
    /// What the window should say about something that went wrong where nothing was refused
    /// to whoever asked, such as a tidy-up before a slide change, until it's taken.
    message: Option<String>,
    /// Whether a save would write a file added to the talk, for the text, files added and files
    /// still to save it was worked out for.
    files_to_save: Cell<Option<(FilesKey, bool)>>,
    files_to_save_walks: Cell<u64>,
    /// The table the table tools are about, for the state of the text, the text being typed and
    /// the thing selected it was worked out for.
    table_here: RefCell<Option<(TableKey, Option<TableHere>)>>,
    /// What the component palette offers, worked out again whenever the talk's components change.
    palette: Vec<PaletteEntry>,
    /// The palette's components not loaded in this opening of it, which it doesn't try again.
    palette_given_up: BTreeSet<String>,
    /// The longest a step of loading the palette has taken.
    palette_longest_step: Duration,
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
        let added = Arc::new(AddedSet::new());
        let own_files = files.clone();
        let files: Arc<dyn TalkFiles + Send + Sync> = Arc::new(WithAdded::new(files, added.clone()));
        let mut document = Document {
            own_files,
            on_disk: source,
            outside: None,
            deck: Deck::open_parsed(Box::new(files.clone()), fonts, talk),
            place_name: place.name(),
            place,
            files,
            added,
            pending: BTreeSet::new(),
            talk_fonts,
            current: 0,
            history: History::new(),
            typing_notes: None,
            editing: None,
            held_color: None,
            selected: None,
            picks: 0,
            next_session: 0,
            message: None,
            files_to_save: Cell::new(None),
            files_to_save_walks: Cell::new(0),
            table_here: RefCell::new(None),
            palette: Vec::new(),
            palette_given_up: BTreeSet::new(),
            palette_longest_step: Duration::ZERO,
        };
        document.components_looked_at();
        Ok(document)
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
        self.follow(&change);
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
        let changed = self.take_step(step.current, &step.change, talk);
        self.resume_text(step.mark, true);
        self.select_marked(step.selects.map(|selects| selects.before));
        self.keep_selection_with_typing();
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
        let changed = self.take_step(step.current, &step.change, talk);
        self.resume_text(step.mark, false);
        self.select_marked(step.selects.map(|selects| selects.after));
        self.keep_selection_with_typing();
        Ok(Some(changed))
    }

    /// The talk as `change` leaves it.
    fn read_changed(&self, change: &Change) -> Result<Talk, EditRefused> {
        let text = guarded(|| change.apply(self.source()))?;
        read_after(self.talk(), text)
    }

    fn take_step(&mut self, current: usize, change: &Change, talk: Talk) -> Vec<usize> {
        let mut changed = self.deck.update_text(talk);
        self.follow(change);
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
    /// A file added to the talk that a save would write counts too, as the file holds neither.
    pub fn has_unsaved_changes(&self) -> bool {
        !self.history.is_saved() || self.has_files_to_save()
    }

    /// Whether a save would write a file added to the talk. Worked out once for each version of
    /// the text and of the files added, as it's asked on every frame and keystroke.
    fn has_files_to_save(&self) -> bool {
        if self.pending.is_empty() {
            return false;
        }
        let key = (self.deck.text_version(), self.source().len(), self.added.stamp(), self.pending.len());
        if let Some((known, answer)) = self.files_to_save.get()
            && known == key
        {
            return answer;
        }
        self.files_to_save_walks.set(self.files_to_save_walks.get() + 1);
        let talk = self.talk();
        let answer = self.pending.iter().any(|path| talk_uses(talk, &*self.files, path));
        self.files_to_save.set(Some((key, answer)));
        answer
    }

    /// How many times whether there are files to save has been worked out, for its tests.
    #[doc(hidden)]
    pub fn files_to_save_walks(&self) -> u64 {
        self.files_to_save_walks.get()
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
        self.follow(&one_edit(&old, self.deck.source()));
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

    /// Adds `bytes` to the talk as a file of `kind`, named as like `suggested` as is safe, and
    /// returns its path in the talk, as the text names it. A file the talk has with the same
    /// bytes, saved or added, is that file. A name it has already is numbered, but a component's
    /// never is, as its name is the component's. Nothing is written until a save, which writes
    /// it if the text uses it then; till then it's kept with the talk's unsaved changes, and a
    /// slide that shows it shows it at once.
    pub fn add_file(&mut self, kind: FileKind, suggested: &str, bytes: Vec<u8>) -> Result<String, NotAdded> {
        self.add_file_avoiding(kind, suggested, bytes, &BTreeSet::new())
    }

    /// [`Self::add_file`], never giving a new file a path in `avoid`, in lower case.
    fn add_file_avoiding(&mut self, kind: FileKind, suggested: &str, bytes: Vec<u8>, avoid: &BTreeSet<String>) -> Result<String, NotAdded> {
        let taken = |path: &str| self.on_disk(path) || avoid.contains(&path.to_ascii_lowercase());
        let (path, bytes) = match added_files::place(&*self.files, kind, suggested, bytes, taken)? {
            Placed::There(path) => return Ok(path),
            Placed::New(path, bytes) => (path, bytes),
        };
        let added = self.added.add(&path, Arc::from(bytes));
        debug_assert!(added, "a name that's free");
        if !added {
            return Err(NotAdded::NameTaken);
        }
        self.pending.insert(path.clone());
        // A slide may name it already, and a component is found by its files.
        if kind == FileKind::ComponentSource || kind == FileKind::ComponentBuild || talk_uses(self.talk(), &*self.files, &path) {
            self.deck.files_added();
            self.components_looked_at();
        }
        Ok(path)
    }

    /// The bytes of the file at `path`, outside the talk, to add to it with [`Self::add_file`].
    /// A folder, a link to anything but a file, and a file over the limit are refused.
    pub fn read_outside_file(path: &Path) -> Result<Vec<u8>, NotAdded> {
        added_files::read_outside(path)
    }

    /// The files added to the talk that its text uses and no save has written yet, for a save.
    pub fn added_files(&self) -> Vec<(String, Arc<[u8]>)> {
        let talk = self.talk();
        self.pending
            .iter()
            .filter(|path| talk_uses(talk, &*self.files, path))
            .filter_map(|path| Some((path.clone(), self.added.get(path)?)))
            .collect()
    }

    /// The files added to the talk that no save has written, used or not, for its working copy.
    pub fn unsaved_files(&self) -> Vec<(String, Arc<[u8]>)> {
        self.pending.iter().filter_map(|path| Some((path.clone(), self.added.get(path)?))).collect()
    }

    /// Changes each time a file is added to the talk.
    pub fn files_added_stamp(&self) -> u64 {
        self.added.stamp()
    }

    /// Takes `text`, the working copy of edits that weren't saved, as [`Self::recover`] does,
    /// with `files`, the files added to the talk that were kept with it, as added again. One the
    /// talk has already is left as it is, and one that isn't a name the app gives is left out.
    ///
    /// One whose name the talk now gives a different file comes back under another name, or is
    /// the file the talk has with the same bytes, and every use of it in `text`, a picture's
    /// source or a tag's attribute, is made to name that instead before it's taken: the slides
    /// show what they showed. If one of those uses can't be rewritten exactly, or the file can't
    /// be brought back at all, `text` is taken as it is, and the window says so, once.
    pub fn recover_with_files(&mut self, text: String, files: Vec<(String, Arc<[u8]>)>) -> Result<Vec<usize>, EditRefused> {
        self.recover_with_builds(text, files, Vec::new())
    }

    /// Takes `text` and `files` back as [`Self::recover_with_files`] does, with `builds`, the
    /// builds a paste had entered in the talk's index, kept with them. Each is entered again only
    /// as a paste would enter it: its source and build are files brought back here, the tag is
    /// the one the app finds in that source, it isn't a built-in's, and the talk's index has no
    /// entry for that component or file. The hash it records is compared, by the engine, with
    /// the source's, as for any build. Any other is let go, and that component needs building.
    pub fn recover_with_builds(
        &mut self,
        text: String,
        files: Vec<(String, Arc<[u8]>)>,
        builds: Vec<BuildEntry>,
    ) -> Result<Vec<usize>, EditRefused> {
        let changed = self.recover_files_and_text(text, files)?;
        let mut entered = false;
        for entry in builds {
            let (source, build) = (format!("components/{}.rs", entry.file), format!("build/{}.wasm", entry.file));
            let hash_like = entry.source_hash.len() == 64 && entry.source_hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
            let brought_back = slides_components::safe_stem(&entry.file) && self.pending.contains(&source) && self.pending.contains(&build);
            let tag = brought_back.then(|| self.added.get(&source)).flatten().and_then(|bytes| {
                std::str::from_utf8(&bytes).ok().and_then(slides_components::component_tag)
            });
            let indexed = added_files::build_index(&*self.files)
                .is_some_and(|index| index.components.iter().any(|kept| kept.tag == entry.tag || kept.file == entry.file));
            if hash_like && tag.as_deref() == Some(entry.tag.as_str()) && !self.deck.is_built_in(&entry.tag) && !indexed {
                self.added.enter_build(entry);
                entered = true;
            }
        }
        if entered {
            self.deck.files_added();
            self.components_looked_at();
        }
        Ok(changed)
    }

    fn recover_files_and_text(&mut self, text: String, files: Vec<(String, Arc<[u8]>)>) -> Result<Vec<usize>, EditRefused> {
        let (renames, mut clashed) = self.bring_back_files(files);
        let text = if renames.is_empty() {
            text
        } else {
            match guarded(|| added_files::rename_uses(&text, &renames)).ok().flatten() {
                Some(renamed) => renamed,
                None => {
                    clashed = true;
                    text
                }
            }
        };
        if clashed {
            self.message = Some(added_files::KEPT_FILE_CLASH.to_string());
        }
        self.recover(text)
    }

    /// Adds `files` back as [`Self::recover_with_files`] says, and gives the new path of each
    /// that couldn't keep its own, `(old, new)`, and whether any couldn't be brought back.
    fn bring_back_files(&mut self, files: Vec<(String, Arc<[u8]>)>) -> (Vec<(String, String)>, bool) {
        let mut renames = Vec::new();
        let mut left_out = false;
        let mut any = false;
        for (path, bytes) in files {
            let Some(kind) = added_files::kind_of_path(&path) else {
                continue;
            };
            let same = self.files.len(&path) == Some(bytes.len() as u64) && self.files.read(&path).is_some_and(|held| *held == *bytes);
            if same {
                continue;
            }
            let free = self.files.len(&path).is_none() && !self.on_disk(&path);
            let new = if free {
                path
            } else {
                let (_, name) = path.split_once('/').expect("a path from kind_of_path");
                match added_files::place(&*self.files, kind, name, bytes.to_vec(), |path| self.on_disk(path)) {
                    Ok(Placed::New(new, _)) => {
                        renames.push((path, new.clone()));
                        new
                    }
                    // The same bytes under another name already.
                    Ok(Placed::There(there)) => {
                        renames.push((path, there));
                        continue;
                    }
                    Err(_) => {
                        left_out = true;
                        continue;
                    }
                }
            };
            if self.added.add(&new, bytes) {
                self.pending.insert(new);
                any = true;
            }
        }
        if any {
            self.deck.files_added();
            self.components_looked_at();
        }
        (renames, left_out)
    }

    /// Whether anything is at `path` in a folder talk's folder, a link or a folder included,
    /// which the talk's files don't list.
    fn on_disk(&self, path: &str) -> bool {
        match &self.place {
            TalkPlace::Folder(folder) => std::fs::symlink_metadata(folder.join(path)).is_ok(),
            TalkPlace::Packed(_) => false,
        }
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

    /// Pastes the slide `text` as [`Self::paste_slide`] does, with `files`, the files it uses
    /// from the talk it was copied from: they're added first, as [`Self::bring_files`] says, and
    /// each path in its text that had to change is made to name the file as added. A slide that
    /// isn't one is refused before anything is added.
    pub fn paste_slide_with_files(&mut self, text: &str, files: PastedFiles) -> Result<(Vec<usize>, Brought), EditRefused> {
        if guarded(|| crate::clipboard::one_slide(text))?.is_none() {
            return Err(EditRefused::WouldChangeTheSlides);
        }
        let (text, mut brought) = self.bring_files_for(text, files);
        let changed = self.paste_slide(&text)?;
        brought.look_for_lone_builds(self);
        Ok((changed, brought))
    }

    /// Adds those of `files` that `text` uses, as [`Self::bring_files`] says, and gives `text`
    /// naming them as added. If it can't be made to name them exactly, it's given as it is, and
    /// that's left out.
    fn bring_files_for(&mut self, text: &str, files: PastedFiles) -> (String, Brought) {
        let (renames, mut brought) = self.bring_files(text, files);
        if renames.is_empty() {
            return (text.to_string(), brought);
        }
        match guarded(|| added_files::rename_uses(text, &renames)).ok().flatten() {
            Some(renamed) => (renamed, brought),
            None => {
                brought.left_out = true;
                (text.to_string(), brought)
            }
        }
    }

    /// Adds the files of `files`, pasted from another talk with `text`, that `text` uses, as
    /// [`talk_uses`] finds them among the files pasted: a picture or data file a picture or a
    /// tag's attribute names, and a component's source and build when a tag in `text` is the
    /// component the app finds in that source. Which are used is decided before anything is
    /// added, and the rest are dropped: nothing unused is added, named, kept or offered. Each is
    /// added as [`Self::add_file`] adds a dropped one, and the paths that had to change are given,
    /// `(as it was, as added)`, with what there is to say of them. One that can't be added is
    /// left out. A new file never takes the name of a file `text` names that wasn't pasted, so a
    /// name it had to be given is never one the text already meant for another.
    ///
    /// A component's source and build are added only if the talk has no component of that name,
    /// and it isn't the name of one every talk has. If it has one with the same source, that
    /// one is used as it is; with different source, nothing of the pasted one is added, and
    /// that's said. A build is added only beside its source, only to a free name, and only if the
    /// talk's build index has no entry for that component or file: an entry there is never
    /// replaced. It's entered in the index, kept with the files added until a save, named for
    /// the component the app finds in that source, and recorded as built from it only if the copy
    /// says it was built from exactly those bytes, by their hash as the app works it out; if it
    /// says another, that's recorded, and the engine shows it out of date. Nothing is built, and
    /// no component's source or build is ever replaced.
    ///
    /// The hash a copy says a build was built from can't be checked without building it: a
    /// clipboard that says the right one has its module shown as up to date for the pasted
    /// source, exactly as a talk's own index does when the talk is opened. The module runs
    /// sandboxed, as any component does, and is used only if it says it is that component.
    fn bring_files(&mut self, text: &str, files: PastedFiles) -> (Vec<(String, String)>, Brought) {
        // Only what's used is spoken of: a file that came and failed its checks is missing from the
        // slide, if the slide uses it, which is said once it's pasted.
        let mut brought = Brought { too_large: files.too_large, ..Brought::default() };
        let stem_of = |path: &str| path.split_once('/').and_then(|(_, name)| name.rsplit_once('.')).map(|(stem, _)| stem.to_string());
        // A build carried without its source was the copied talk's, but can't be paired here:
        // said once pasted, if the slide shows that component missing.
        let sources: BTreeSet<String> =
            files.files.iter().filter(|file| file.kind == FileKind::ComponentSource).filter_map(|file| stem_of(&file.path)).collect();
        brought.builds_alone = files
            .files
            .iter()
            .filter(|file| file.kind == FileKind::ComponentBuild)
            .filter_map(|file| stem_of(&file.path))
            .filter(|stem| !sources.contains(stem))
            .collect();
        let Ok(pasted) = guarded(|| parse_talk(text)) else {
            brought.left_out = true;
            return (Vec::new(), brought);
        };
        let carried = CarriedFiles(&files.files);
        let used: Vec<bool> = files.files.iter().map(|file| guarded(|| talk_uses(&pasted, &carried, &file.path)).unwrap_or(false)).collect();
        let files: Vec<PastedFile> = files.files.into_iter().zip(used).filter_map(|(file, used)| used.then_some(file)).collect();
        // What `text` names that wasn't pasted, which nothing added may come to be.
        let avoid: BTreeSet<String> = added_files::written_names(&pasted)
            .into_iter()
            .filter_map(slides_engine::normalise)
            .filter(|name| !files.iter().any(|file| file.path == *name))
            .map(|name| name.to_ascii_lowercase())
            .collect();
        let mut renames = Vec::new();
        let (components, others): (Vec<PastedFile>, Vec<PastedFile>) = files.into_iter().partition(|file| file.kind.is_component());
        for file in others {
            let name = file.path.split_once('/').map_or(file.path.as_str(), |(_, name)| name).to_string();
            match self.add_file_avoiding(file.kind, &name, file.bytes, &avoid) {
                Ok(path) if path != file.path => renames.push((file.path, path)),
                Ok(_) => {}
                Err(_) => brought.left_out = true,
            }
        }
        let (sources, mut builds): (Vec<PastedFile>, Vec<PastedFile>) =
            components.into_iter().partition(|file| file.kind == FileKind::ComponentSource);
        for source in sources {
            let Some(stem) = stem_of(&source.path) else { continue };
            let build = builds.iter().position(|build| stem_of(&build.path).as_deref() == Some(stem.as_str())).map(|at| builds.swap_remove(at));
            let Some(tag) = std::str::from_utf8(&source.bytes).ok().and_then(slides_components::component_tag) else {
                brought.left_out = true;
                continue;
            };
            if self.deck.is_built_in(&tag) {
                brought.built_in = true;
                continue;
            }
            if let Some(info) = self.components().iter().find(|info| info.tag == tag) {
                if !self.files.read(&info.file).is_some_and(|held| held == source.bytes) {
                    brought.different = Some(clipboard::different_component(&palette::label(&tag)));
                }
                continue;
            }
            let hash = source_hash(&source.bytes);
            if self.add_file(FileKind::ComponentSource, &format!("{stem}.rs"), source.bytes).is_err() {
                brought.left_out = true;
                continue;
            }
            // Without its build, it needs building, as any source does.
            let Some(build) = build else { continue };
            let build_path = format!("build/{stem}.wasm");
            let indexed = added_files::build_index(&*self.files)
                .is_some_and(|index| index.components.iter().any(|entry| entry.tag == tag || entry.file == stem));
            // A build or an entry already there is the talk's: the pasted one needs building.
            if indexed || self.files.len(&build_path).is_some() || self.on_disk(&build_path) {
                continue;
            }
            let built_from = build.built_from.clone();
            if self.add_file(FileKind::ComponentBuild, &format!("{stem}.wasm"), build.bytes).is_err() {
                brought.left_out = true;
                continue;
            }
            let source_hash = match built_from {
                Some(said) if said == hash => hash,
                Some(other) => other,
                // Built from what, nobody said: it needs building.
                None => continue,
            };
            self.added.enter_build(BuildEntry { tag, file: stem, source_hash });
            self.deck.files_added();
            self.components_looked_at();
        }
        (renames, brought)
    }

    /// The files of the talk's that `text`, a slide's own text or a thing's, uses, as
    /// [`talk_uses`] finds them, for a copy to carry: a picture or data file in `assets` a tag
    /// or picture names, and for a component of the talk's own, its source and build. A build
    /// says which source it was built from, as the talk's index records. Files are read only
    /// while there's room for them in a copy; past that, the last one read is more than a copy
    /// carries, and it carries none.
    pub fn files_for_copy(&self, text: &str) -> Vec<CopiedFile> {
        let Ok(talk) = worked_out(|| parse_talk(text)) else {
            return Vec::new();
        };
        let files = &*self.files;
        let index = added_files::build_index(files);
        let mut copied: Vec<CopiedFile> = Vec::new();
        let mut total = 0usize;
        for folder in ["assets", "components", "build"] {
            for path in files.names_in(folder) {
                if copied.len() > MAX_CLIPBOARD_FILES || total > MAX_CLIPBOARD_FILES_BYTES {
                    return copied;
                }
                let copyable = crate::clipboard::pasted_kind(&path).is_some()
                    && files.len(&path).is_some_and(|len| len <= MAX_ADDED_BYTES as u64)
                    && guarded(|| talk_uses(&talk, files, &path)).unwrap_or(false);
                let Some(bytes) = copyable.then(|| files.read(&path)).flatten() else {
                    continue;
                };
                let stem = path.strip_prefix("build/").and_then(|name| name.strip_suffix(".wasm"));
                let built_from = stem.and_then(|stem| {
                    let entry = index.as_ref()?.components.iter().find(|entry| entry.file == stem)?;
                    Some(entry.source_hash.clone())
                });
                total = total.saturating_add(bytes.len());
                copied.push(CopiedFile { path, bytes: bytes.into(), built_from });
            }
        }
        copied
    }

    /// The text of the thing selected, as a copy of it gives it, and the files it uses, as
    /// [`Self::files_for_copy`] gives them. `None` with nothing selected.
    pub fn copy_selected(&self) -> Option<(String, Vec<CopiedFile>)> {
        let selected = self.selected.as_ref()?;
        let text = worked_out(|| block_text(self.talk(), selected.slide, &selected.source)).ok().flatten()?;
        let files = self.files_for_copy(&text);
        Some((text, files))
    }

    /// Puts the thing `markdown` holds, perhaps from another talk, on the current slide, with
    /// `files`, the files it uses: see [`Self::put_thing`].
    pub fn paste_element(&mut self, markdown: &str, files: PastedFiles) -> Result<(Vec<usize>, Brought), EditRefused> {
        self.put_thing(PASTE, markdown, files)
    }

    /// Puts a copy of the thing selected on the slide it's on, as a paste of it would, as one
    /// step named "Duplicate", and selects the copy. The clipboard isn't touched.
    pub fn duplicate_selected(&mut self) -> Result<Vec<usize>, EditRefused> {
        // Typing is finished first, which can move the thing; the slide is drawn again if it did.
        let tidied = self.finish_before_edit()?;
        let Some(Selected { slide, source }) = self.selected.clone() else {
            return Ok(if tidied { vec![self.current] } else { Vec::new() });
        };
        let text = built(self.talk(), |talk| block_text(talk, slide, &source))?.ok_or(EditRefused::CouldNotBeMade)?;
        self.go_to(slide);
        let (mut changed, _) = self.put_thing(DUPLICATE, &text, PastedFiles::default())?;
        if tidied && !changed.contains(&slide) {
            changed.push(slide);
        }
        Ok(changed)
    }

    /// Puts the thing `markdown` holds on the current slide, as one step named `label`, and
    /// selects it. It must be one thing, as [`clipboard::one_element`] says, or it's refused,
    /// with nothing added. Typing ends first, and a new text box waiting for its first letter
    /// is dropped. `files` are added first, as [`Self::bring_files`] says, and each path in the
    /// thing that had to change is made to name the file as added. Where a thing already has
    /// its corner at exactly the thing's `x` and `y`, it's moved 40 down and right, as often as
    /// it takes. It's written with [`insert_block`], after the slide's content.
    fn put_thing(&mut self, label: &'static str, markdown: &str, files: PastedFiles) -> Result<(Vec<usize>, Brought), EditRefused> {
        let text = guarded(|| clipboard::one_element(markdown))?.ok_or(EditRefused::NotOneThing)?;
        let tidied = self.finish_before_edit()?;
        self.editing = None;
        let (text, brought) = self.bring_files_for(&text, files);
        let text = self.stepped_aside(&text);
        let changed = self.insert_text(label, &text, tidied)?;
        let mut brought = brought;
        brought.look_for_lone_builds(self);
        Ok((changed, brought))
    }

    /// `text`, one tag, with its `x` and `y` moved aside from the corners of the things on the
    /// current slide, as [`insert::step_aside`] moves them. As it is if it has no place written
    /// as numbers, or nothing is there.
    fn stepped_aside(&mut self, text: &str) -> String {
        let layout = self.text_layout(self.current);
        let taken: Vec<(f32, f32)> = layout.elements().iter().map(|element| (element.rect.x, element.rect.y)).collect();
        let first_tag = |talk: &Talk| match talk.slides.first()?.blocks.first()? {
            Block::Tag(tag) => Some(tag.clone()),
            _ => None,
        };
        let moved = guarded(|| {
            let talk = parse_talk(text);
            let tag = first_tag(&talk)?;
            let place = |name: &str| tag.attrs.iter().find(|attr| attr.name == name)?.value.trim().parse::<f32>().ok().filter(|at| at.is_finite());
            let at = (place("x")?, place("y")?);
            let to = insert::step_aside(at, &taken);
            if to == at {
                return None;
            }
            let text = set_tag_attr(&talk, 0, &tag.range, "x", Some(&insert::whole(to.0)))?.apply(text);
            let talk = parse_talk(&text);
            let tag = first_tag(&talk)?;
            Some(set_tag_attr(&talk, 0, &tag.range, "y", Some(&insert::whole(to.1)))?.apply(&text))
        });
        moved.ok().flatten().unwrap_or_else(|| text.to_string())
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
        let content = self.content_tags();
        let editing = self.editing.as_ref()?;
        let flows = guarded(|| flows_in(self.talk(), editing.slide, &content)).ok()?;
        editing.flow.find(&flows)
    }

    /// The thing selected on a slide, if any. It's the editor's view of the talk, not part of
    /// it: nothing of it is written, and it's no change to save or undo.
    pub fn selected(&self) -> Option<&Selected> {
        self.selected.as_ref()
    }

    /// Selects the thing on slide `slide` whose whole source is `source`, which must be one the
    /// slide's layout can pick, and makes `slide` the current one. Any typing ends first. Says
    /// whether there's such a thing.
    pub fn select(&mut self, slide: usize, source: Range<usize>) -> bool {
        if slide >= self.slide_count() || self.text_layout(slide).element(&source).is_none() {
            return false;
        }
        // Ending the typing can move the thing, and the selection moves with it.
        self.selected = Some(Selected { slide, source });
        self.picks += 1;
        if let Err(refused) = self.end_text() {
            self.say(refused);
        }
        let Some(slide) = self.selected.as_ref().map(|selected| selected.slide) else {
            return false;
        };
        self.go_to(slide);
        true
    }

    /// Lets go of the thing selected, if any.
    pub fn deselect(&mut self) {
        if self.selected.take().is_some() {
            self.picks += 1;
        }
    }

    /// Changes each time a thing is selected or let go of, but not as the thing selected is
    /// followed through an edit: what's shown for one thing is never taken for another's.
    pub fn selection_pick(&self) -> u64 {
        self.picks
    }

    /// What the tag selected takes, from its component: `None` when nothing's selected, the
    /// thing isn't a tag, the talk has no such component, or it has no working build.
    pub fn schema_of_selected(&mut self) -> Option<ComponentSchema> {
        let name = self.selected_tag()?.name.clone();
        let deck = &mut self.deck;
        guarded(|| deck.component_schema(&name)).ok().flatten()
    }

    /// Sets attribute `name` of the tag selected to `value`, or takes it away with `None`, as
    /// one step named "Change setting", and returns the slides to show again. Only that
    /// attribute's bytes change. A value the same as the one there writes nothing and makes no
    /// step. It's refused when the value can't be written, as one holding both kinds of quote
    /// mark or a new line, or when the slide wouldn't read the same but for it.
    pub fn set_selected_attr(&mut self, name: &str, value: Option<&str>) -> Result<Vec<usize>, EditRefused> {
        // Typing is finished first, which can move the thing.
        self.finish_before_edit()?;
        let Some(selected) = self.selected.clone() else {
            return Ok(Vec::new());
        };
        self.set_attr_of(&selected, name, value)
    }

    /// Sets attribute `name` of `thing`, a tag on a slide, whether or not it's the one selected,
    /// as [`Self::set_selected_attr`] does. It's refused if there's no tag at exactly `thing`'s
    /// place, as once typing that ends first has moved it.
    pub fn set_attr_of(&mut self, thing: &Selected, name: &str, value: Option<&str>) -> Result<Vec<usize>, EditRefused> {
        self.finish_before_edit()?;
        let Selected { slide, source } = thing.clone();
        let change = built(self.talk(), |talk| set_tag_attr(talk, slide, &source, name, value))?
            .ok_or(EditRefused::CouldNotBeMade)?;
        let selected = self.selected.as_ref() == Some(thing);
        let depth = self.history.depth();
        let changed = self.make(CHANGE_SETTING, change, Holds::Slide(slide), slide, slide, None)?;
        if selected && self.history.depth() > depth {
            let after = self.selected.as_ref().map(|selected| selected.source.clone());
            self.history.select_mark_last(SelectMark { before: Some(source), after });
        }
        Ok(changed)
    }

    /// The tag selected, if what's selected is one.
    pub fn selected_tag(&self) -> Option<&Tag> {
        let selected = self.selected.as_ref()?;
        guarded(|| find_tag(self.talk(), selected.slide, &selected.source)).ok().flatten()
    }

    /// Where the thing selected is drawn, if anything is.
    pub fn selected_box(&mut self) -> Option<ElementBox> {
        let selected = self.selected.clone()?;
        self.text_layout(selected.slide).element(&selected.source)
    }

    /// Takes the thing selected away, its own lines and one blank line beside them, as one step
    /// named "Delete", and returns the slides to show again. Undone, it's selected again. It's
    /// refused when what's around it would read differently without it.
    pub fn delete_selected(&mut self) -> Result<Vec<usize>, EditRefused> {
        self.remove_selected(DELETE)
    }

    /// Takes the thing selected away, as [`Self::delete_selected`] does, as one step named "Cut".
    pub fn cut_selected(&mut self) -> Result<Vec<usize>, EditRefused> {
        self.remove_selected(CUT)
    }

    fn remove_selected(&mut self, label: &'static str) -> Result<Vec<usize>, EditRefused> {
        // Typing is finished first, which can move the thing.
        self.finish_before_edit()?;
        let Some(Selected { slide, source }) = self.selected.clone() else {
            return Ok(Vec::new());
        };
        let change = built(self.talk(), |talk| delete_block(talk, slide, &source))?
            .ok_or(EditRefused::CouldNotBeRemoved)?;
        let depth = self.history.depth();
        let changed = self.make(label, change, Holds::Slide(slide), slide, slide, None)?;
        if self.history.depth() > depth {
            self.history.select_mark_last(SelectMark { before: Some(source), after: None });
        }
        Ok(changed)
    }

    /// Puts a `name` tag with `attrs`, and `body` inside it if it's given, on the current slide,
    /// after its content and before its notes, as one step named `label`, and selects it. It's
    /// given the size the engine gives a new one, and a place in the middle of the slide that
    /// nothing else on it has its corner at: see [`Self::insert_sized`].
    pub fn insert_tag(
        &mut self,
        label: &'static str,
        name: &str,
        attrs: &[(&str, &str)],
        body: Option<&str>,
    ) -> Result<Vec<usize>, EditRefused> {
        let deck = &mut self.deck;
        let size = guarded(|| deck.default_size(name, attrs))?;
        self.insert_sized(label, name, attrs, size, body)
    }

    /// [`Self::insert_tag`], `size` big: the tag's `x`, `y`, `w` and `h`, whole numbers, come after
    /// `attrs`, at the place [`insert::place_for`] gives among the corners of the things on the
    /// slide. Typing on the slide ends first. Undone, the tag goes and nothing is selected; made
    /// again, it's selected again.
    pub fn insert_sized(
        &mut self,
        label: &'static str,
        name: &str,
        attrs: &[(&str, &str)],
        size: (f32, f32),
        body: Option<&str>,
    ) -> Result<Vec<usize>, EditRefused> {
        // Where things are is worked out from the talk as tidied.
        let tidied = self.finish_before_edit()?;
        let slide = self.current;
        let layout = self.text_layout(slide);
        let taken: Vec<(f32, f32)> = layout.elements().iter().map(|element| (element.rect.x, element.rect.y)).collect();
        let (x, y) = insert::place_for(size, &taken);
        let place = [insert::whole(x), insert::whole(y), insert::whole(size.0.max(1.0)), insert::whole(size.1.max(1.0))];
        let mut all: Vec<(&str, &str)> = attrs.to_vec();
        all.extend(["x", "y", "w", "h"].into_iter().zip(place.iter().map(String::as_str)));
        let text = tag_text(&NewTag { name, attrs: &all, body }).ok_or(EditRefused::CouldNotBeMade)?;
        self.insert_text(label, &text, tidied)
    }

    /// Writes `text`, one block, at the end of the current slide's content, as one step named
    /// `label`, and selects it. `tidied` says whether finishing the typing first changed the
    /// talk. Undone, it goes and nothing is selected; made again, it's selected again.
    fn insert_text(&mut self, label: &'static str, text: &str, tidied: bool) -> Result<Vec<usize>, EditRefused> {
        let slide = self.current;
        let text = text.to_string();
        let change = built(self.talk(), |talk| insert_block(talk, slide, &text))?.ok_or(EditRefused::CouldNotBeMade)?;
        // Where the tag is once it's written, in the talk's line endings.
        let source = change.edits.first().and_then(|edit| {
            let written = if edit.text.contains("\r\n") { text.replace('\n', "\r\n") } else { text.clone() };
            let at = edit.range.start + edit.text.find(&written)?;
            Some(at..at + written.len())
        });
        let depth = self.history.depth();
        let mut changed = self.make(label, change, Holds::Slide(slide), slide, slide, None)?;
        if tidied && !changed.contains(&slide) {
            changed.push(slide);
        }
        if self.history.depth() > depth
            && let Some(source) = source
            && self.select(slide, source.clone())
        {
            self.history.select_mark_last(SelectMark { before: None, after: Some(source) });
        }
        Ok(changed)
    }

    /// Puts a shape of kind `kind`, one of [`insert::SHAPE_KINDS`], on the current slide, as
    /// [`Self::insert_sized`] does: filled with the theme's accent, or, for a line or an arrow,
    /// drawn in its text colour.
    pub fn insert_shape(&mut self, kind: &str) -> Result<Vec<usize>, EditRefused> {
        let theme = self.deck.theme();
        let attrs = insert::shape_attrs(kind, &theme);
        let attrs: Vec<(&str, &str)> = attrs.iter().map(|(name, value)| (*name, value.as_str())).collect();
        self.insert_sized(INSERT_SHAPE, "Shape", &attrs, insert::shape_size(kind), None)
    }

    /// Starts typing in a new text box on the current slide, which isn't written until something
    /// is typed in it: the first character typed writes it, with that character, as one step
    /// named "Insert text box". Till then it's nothing of the talk's: no change, no step, nothing
    /// to save. Anything that ends the typing drops it, writing nothing. It goes in the middle of
    /// the slide, stepped aside from the things on it, and from a new box already waiting there,
    /// which it takes the place of. Typing elsewhere ends first, and nothing stays selected.
    pub fn insert_text_box(&mut self) -> Result<Vec<usize>, EditRefused> {
        let waiting = self
            .new_box()
            .filter(|(slide, _)| *slide == self.current)
            .map(|(_, (x, y, _, _))| (x, y));
        let tidied = self.finish_before_edit()?;
        // Ending typing in a new box drops it; ending any other may tidy the talk.
        self.editing = None;
        self.deselect();
        let slide = self.current;
        let layout = self.text_layout(slide);
        let mut taken: Vec<(f32, f32)> = layout.elements().iter().map(|element| (element.rect.x, element.rect.y)).collect();
        taken.extend(waiting);
        let (x, y) = insert::place_for(TEXT_BOX_SIZE, &taken);
        self.editing = Some(EditingText {
            session: self.new_session(),
            ..EditingText::in_new_box(slide, x.max(0.0) as u32, y.max(0.0) as u32)
        });
        Ok(if tidied { vec![slide] } else { Vec::new() })
    }

    /// The new text box typing has begun in that isn't written yet, if there's one: its slide,
    /// and where it is, `(x, y, w, h)`, in slide units.
    pub fn new_box(&self) -> Option<(usize, (f32, f32, f32, f32))> {
        let editing = self.editing.as_ref()?;
        let (x, y) = editing.flow.new_box()?;
        Some((editing.slide, (x as f32, y as f32, TEXT_BOX_SIZE.0, TEXT_BOX_SIZE.1)))
    }

    /// Writes the new text box `editing` is typing in, at (`x`, `y`), holding `after`'s
    /// paragraphs from their first visible character on, as one step of its own named "Insert
    /// text box", and goes on typing in it, at the end of what's written. Input with no visible
    /// character in it, as Enter, a space or a paste of blank lines, writes nothing, and the box
    /// stays as it was, waiting.
    fn write_new_box(&mut self, editing: &EditingText, (x, y): (u32, u32), after: TextEdit, kind: Kind) -> Result<Vec<usize>, EditRefused> {
        let content = self.content_tags();
        let Some(paras) = from_first_visible(&after.paras) else {
            // A choice of kind of paragraph is kept for what's typed next; blank text isn't.
            let kept = if kind == Kind::Formatted { after } else { editing.edit.clone() };
            self.keep_typed(kept, None);
            return Ok(Vec::new());
        };
        let end = TextEdit::new(paras.clone()).select_all().selection.end();
        let after = TextEdit { typing_style: after.typing_style.clone(), ..TextEdit::new(paras).move_to(end, false) };
        let slide = editing.slide;
        let align = editing.flow.new_box_align();
        let (change, start) = guarded(|| new_box_change(self.talk(), slide, (x, y), align, &after.paras, &content))?
            .ok_or(EditRefused::CouldNotBeTyped)?;
        let count = guarded(|| flows_in(self.talk(), slide, &content).len())? + 1;
        let expected = storable(&after.paras);
        let key = FlowKey::Tag { start };
        let reads_back = |candidate: &Talk| {
            let flows = flows_in(candidate, slide, &content);
            flows.len() == count && key.find(&flows).is_some_and(|index| flows[index].paras == expected)
        };
        let depth = self.history.depth();
        let changed = self
            .try_make(INSERT_TEXT_BOX, change, Holds::Slide(slide), slide, slide, Join::Alone, Some(&reads_back))
            .map_err(|_| EditRefused::CouldNotBeTyped)?;
        if self.history.depth() > depth {
            // Undone, nothing's left to type in; made again, typing goes on after what was typed.
            self.history.mark_last(TextMark {
                slide,
                flow: key.clone(),
                before: Selection::caret(Pos::default()),
                after: Some(after.selection),
                flow_after: None,
                session: editing.session,
            });
        }
        if let Some(editing) = &mut self.editing {
            editing.flow = key;
        }
        self.keep_typed(after, None);
        Ok(changed)
    }

    /// Adds `bytes`, a picture, to the talk as a file named as like `suggested` as is safe, and
    /// puts it on the current slide as [`Self::insert_tag`] does: its own size, made smaller to fit
    /// in half the slide. A picture the talk has already is that one.
    pub fn insert_picture(&mut self, suggested: &str, bytes: Vec<u8>) -> Result<(), InsertFailed> {
        let path = self.add_file(FileKind::Picture, suggested, bytes).map_err(InsertFailed::File)?;
        self.insert_tag(INSERT_PICTURE, "Picture", &[("src", &path)], None).map_err(InsertFailed::Edit)?;
        Ok(())
    }

    /// Puts a table of `columns` columns and `rows` rows under its heading row, every cell empty,
    /// at the end of the current slide's content, as one step named "Insert table", and starts
    /// typing in its first heading cell. Typing on the slide ends first, and nothing stays
    /// selected. Undone, the table goes; made again, it's selected.
    pub fn insert_table(&mut self, columns: usize, rows: usize) -> Result<Vec<usize>, EditRefused> {
        let text = new_table(columns, rows).ok_or(EditRefused::CouldNotBeMade)?;
        let tidied = self.finish_before_edit()?;
        let slide = self.current;
        let change = built(self.talk(), |talk| insert_block(talk, slide, &text))?.ok_or(EditRefused::CouldNotBeMade)?;
        let before = tables_on(self.talk(), slide);
        let moved = change.clone();
        self.deselect();
        let depth = self.history.depth();
        let mut changed = self.make(INSERT_TABLE, change, Holds::Slide(slide), slide, slide, None)?;
        if tidied && !changed.contains(&slide) {
            changed.push(slide);
        }
        // The new table is the one on the slide that isn't one of those it had, moved along.
        let kept: Vec<Range<usize>> = before.iter().filter_map(|table| selection::moved(table, &moved)).collect();
        let new = tables_on(self.talk(), slide).into_iter().find(|table| !kept.contains(table));
        if self.history.depth() > depth
            && let Some(table) = new
        {
            self.history.select_mark_last(SelectMark { before: None, after: Some(table.clone()) });
            self.land_in_cell(slide, table.start, (0, 0), false);
        }
        Ok(changed)
    }

    /// The table the table tools are about, if any: the one whose cell the caret is in, or, with
    /// nothing typed, the table selected, which they only add to at its end. It's worked out once for
    /// each state of the text, the text being typed and the thing selected.
    pub fn table_here(&self) -> Option<TableHere> {
        let key = (
            self.deck.text_version(),
            self.editing.as_ref().map(|editing| (editing.slide, editing.flow.clone())),
            self.selected.clone(),
        );
        if let Some((seen, here)) = &*self.table_here.borrow()
            && *seen == key
        {
            return here.clone();
        }
        let here = guarded(|| self.work_out_table_here()).ok().flatten();
        *self.table_here.borrow_mut() = Some((key, here.clone()));
        here
    }

    fn work_out_table_here(&self) -> Option<TableHere> {
        let content = self.content_tags();
        let talk = self.talk();
        let (slide, table, cell) = match &self.editing {
            Some(editing) => {
                if !editing.is_cell() || editing.slide >= self.slide_count() {
                    return None;
                }
                let flows = flows_in(talk, editing.slide, &content);
                let FlowPlace::Cell { table, row, column } = &flows.get(editing.flow.find(&flows)?)?.place else {
                    return None;
                };
                (editing.slide, table.clone(), Some((*row, *column)))
            }
            None => {
                let selected = self.selected.as_ref()?;
                (selected.slide, selected.source.clone(), None)
            }
        };
        let Some(Block::Table { rows, .. }) = selection::block_at(&talk.slides.get(slide)?.blocks, &table) else {
            return None;
        };
        let (columns, count) = table_size(talk, slide, &table)?;
        let long_row = rows.iter().any(|row| row.len() > columns);
        let (row, column) = cell.unwrap_or((count.saturating_sub(1), columns.saturating_sub(1)));
        Some(TableHere { slide, table, row, column, caret: cell.is_some(), columns, rows: count, long_row })
    }

    /// The table whose cell the caret is in, if it's in one: the table's whole source, and the
    /// cell's row, the heading row being row 0, and column.
    pub fn table_at_caret(&self) -> Option<(Range<usize>, usize, usize)> {
        self.table_here().filter(|here| here.caret).map(|here| (here.table, here.row, here.column))
    }

    /// Adds an empty row to the table the tools are about, under the cell's row, or with `below`
    /// false over it, as one step named "Add row", and returns the slides to show again. The
    /// caret goes to the new row, in the same column. Undone, it goes back where it was.
    pub fn add_row(&mut self, below: bool) -> Result<Vec<usize>, EditRefused> {
        let tool = if below { TableTool::RowBelow } else { TableTool::RowAbove };
        let Some(here) = self.table_for(tool)? else {
            return Ok(Vec::new());
        };
        let after = if below { here.row } else { here.row - 1 };
        let to = (after + 1, here.column);
        self.change_table(ADD_ROW, &here, |talk, slide, table| slides_format::add_row(talk, slide, table, after), to)
    }

    /// Adds an empty column to the table the tools are about, after the cell's column, or with
    /// `right` false before it, as one step named "Add column", the caret going to the new
    /// column, in the same row, as [`Self::add_row`] does.
    pub fn add_column(&mut self, right: bool) -> Result<Vec<usize>, EditRefused> {
        let tool = if right { TableTool::ColumnRight } else { TableTool::ColumnLeft };
        let Some(here) = self.table_for(tool)? else {
            return Ok(Vec::new());
        };
        let after = if right { Some(here.column) } else { here.column.checked_sub(1) };
        let to = (here.row, after.map_or(0, |after| after + 1));
        self.change_table(ADD_COLUMN, &here, |talk, slide, table| slides_format::add_column(talk, slide, table, after), to)
    }

    /// Takes the cell's row away from the table the tools are about, as one step named "Delete
    /// row", the caret going to the row that took its place, or the one above for the last.
    pub fn remove_row(&mut self) -> Result<Vec<usize>, EditRefused> {
        let Some(here) = self.table_for(TableTool::DeleteRow)? else {
            return Ok(Vec::new());
        };
        let row = if here.row + 1 < here.rows { here.row } else { here.row - 1 };
        let to = (row, here.column);
        self.change_table(DELETE_ROW, &here, |talk, slide, table| slides_format::remove_row(talk, slide, table, here.row), to)
    }

    /// Takes the cell's column away from the table the tools are about, as one step named
    /// "Delete column", the caret going to the column that took its place, or the one before for
    /// the last.
    pub fn remove_column(&mut self) -> Result<Vec<usize>, EditRefused> {
        let Some(here) = self.table_for(TableTool::DeleteColumn)? else {
            return Ok(Vec::new());
        };
        let column = if here.column + 1 < here.columns { here.column } else { here.column.saturating_sub(1) };
        let to = (here.row, column);
        self.change_table(DELETE_COLUMN, &here, |talk, slide, table| slides_format::remove_column(talk, slide, table, here.column), to)
    }

    /// The table the tools are about, if there's one and `tool` can be used on it.
    fn table_for(&self, tool: TableTool) -> Result<Option<TableHere>, EditRefused> {
        let Some(here) = self.table_here() else {
            return Ok(None);
        };
        match here.why_not(tool) {
            Some(refused) => Err(refused),
            None => Ok(Some(here)),
        }
    }

    /// Makes the change `change` works out for the table `here` is about, as one step named
    /// `label`. If the caret was in one of its cells, it goes to the cell at `to`, or one near
    /// it, and the step goes back to where it was once undone; if the table was selected, it
    /// stays selected.
    fn change_table(
        &mut self,
        label: &'static str,
        here: &TableHere,
        change: impl FnOnce(&Talk, usize, &Range<usize>) -> Option<Change>,
        to: (usize, usize),
    ) -> Result<Vec<usize>, EditRefused> {
        let slide = here.slide;
        let was = self
            .editing
            .as_ref()
            .filter(|_| here.caret)
            .map(|editing| (editing.flow.clone(), editing.edit.selection, editing.session));
        // Typing is finished before the change is worked out: that can write a colour held for
        // the cell, which moves what's after it. The table is found again where it starts.
        self.finish_before_edit()?;
        let table = table_starting_at(self.talk(), slide, here.table.start).ok_or(EditRefused::CouldNotBeMade)?;
        let change = built(self.talk(), |talk| change(talk, slide, &table))?.ok_or(EditRefused::CouldNotBeMade)?;
        let depth = self.history.depth();
        let changed = self.make(label, change, Holds::Slide(slide), slide, slide, None)?;
        let made = self.history.depth() > depth;
        let start = here.table.start;
        match was {
            Some((flow, before, session)) => {
                if self.land_in_cell(slide, start, to, false)
                    && made
                    && let Some(editing) = &self.editing
                {
                    let mark = TextMark {
                        slide,
                        flow,
                        before,
                        after: Some(editing.edit.selection),
                        flow_after: Some(editing.flow.clone()),
                        session,
                    };
                    self.history.mark_last(mark);
                }
            }
            None => {
                if let Some(after) = table_starting_at(self.talk(), slide, start) {
                    self.selected = Some(Selected { slide, source: after.clone() });
                    if made {
                        self.history.select_mark_last(SelectMark { before: Some(table), after: Some(after) });
                    }
                }
            }
        }
        Ok(changed)
    }

    /// Starts typing in the cell at `(row, column)` of the table that starts at byte `start` of
    /// slide `slide`, at its start, or with `end` at its end. A cell there that can't be typed
    /// in, or that isn't there, gives way to the nearest of that row that can, or else the
    /// table's first. Says whether typing started.
    fn land_in_cell(&mut self, slide: usize, start: usize, (row, column): (usize, usize), end: bool) -> bool {
        let content = self.content_tags();
        let Ok(flows) = guarded(|| flows_in(self.talk(), slide, &content)) else {
            return false;
        };
        let cells: Vec<(usize, usize, usize)> = flows
            .iter()
            .enumerate()
            .filter_map(|(index, flow)| match &flow.place {
                FlowPlace::Cell { table, row, column } if table.start == start => Some((index, *row, *column)),
                _ => None,
            })
            .collect();
        let found = cells
            .iter()
            .find(|cell| (cell.1, cell.2) == (row, column))
            .or_else(|| cells.iter().filter(|cell| cell.1 == row).min_by_key(|cell| cell.2.abs_diff(column)))
            .or_else(|| cells.first());
        let Some(&(index, ..)) = found else {
            return false;
        };
        let pos = if end { end_of(&flows[index].paras) } else { Pos::default() };
        self.begin_text(slide, index, pos)
    }

    /// Moves the caret on from the cell it's in, as Tab does with `Next`, Shift+Tab with
    /// `Previous` and Enter with `Below`, to the end of what that cell says: cells that can't be
    /// typed in, and those a short row doesn't have, are gone past. Tab from the table's last
    /// cell, and Enter from its last row, add a row, as one step named "Add row", and go into it.
    pub fn move_in_table(&mut self, step: CellMove) -> Result<Vec<usize>, EditRefused> {
        let content = self.content_tags();
        let Some(here) = self.table_here().filter(|here| here.caret) else {
            return Ok(Vec::new());
        };
        let beside = match step {
            CellMove::Next => Beside::After,
            CellMove::Previous => Beside::Before,
            CellMove::Below => Beside::Below,
        };
        if let Some(index) = self.cell_beside(beside) {
            let flows = guarded(|| flows_in(self.talk(), here.slide, &content))?;
            let end = end_of(&flows[index].paras);
            self.begin_text(here.slide, index, end);
            return Ok(Vec::new());
        }
        let (after, column) = match step {
            CellMove::Next => (here.rows - 1, 0),
            CellMove::Below if here.row + 1 == here.rows => (here.row, here.column),
            CellMove::Below | CellMove::Previous => return Ok(Vec::new()),
        };
        if here.rows >= MAX_TABLE_ROWS {
            return Err(EditRefused::TooManyRows);
        }
        self.change_table(ADD_ROW, &here, |talk, slide, table| slides_format::add_row(talk, slide, table, after), (after + 1, column))
    }

    /// The flow of the cell `beside` the caret's in its table, if there's one to type in: the
    /// one before or after it in the order they're written, or the nearest above or below it in
    /// its column.
    pub fn cell_beside(&self, beside: Beside) -> Option<usize> {
        let content = self.content_tags();
        let editing = self.editing.as_ref().filter(|editing| editing.is_cell())?;
        let flows = guarded(|| flows_in(self.talk(), editing.slide, &content)).ok()?;
        let at = editing.flow.find(&flows)?;
        let FlowPlace::Cell { table, row, column } = &flows[at].place else {
            return None;
        };
        let (row, column) = (*row, *column);
        let mut cells = flows.iter().enumerate().filter_map(|(index, flow)| match &flow.place {
            FlowPlace::Cell { table: other, row, column } if other == table => Some((index, *row, *column)),
            _ => None,
        });
        let found = match beside {
            Beside::Before => cells.rfind(|cell| cell.0 < at),
            Beside::After => cells.find(|cell| cell.0 > at),
            Beside::Above => cells.filter(|cell| cell.2 == column && cell.1 < row).max_by_key(|cell| cell.1),
            Beside::Below => cells.filter(|cell| cell.2 == column && cell.1 > row).min_by_key(|cell| cell.1),
        };
        found.map(|cell| cell.0)
    }

    /// Whether the thing selected takes typing: a shape, a box, or one of the talk's own
    /// components that takes content, with nothing in it, or with only text in it.
    pub fn selected_takes_text(&self) -> bool {
        let content = self.content_tags();
        let Some(selected) = &self.selected else {
            return false;
        };
        let Some(tag) = self.selected_tag() else {
            return false;
        };
        takes_text(tag, &content)
            && (tag.children.is_empty()
                || guarded(|| flows_in(self.talk(), selected.slide, &content))
                    .is_ok_and(|flows| flows.iter().any(|flow| flow.place == FlowPlace::Tag(tag.range.clone()))))
    }

    /// Starts typing in the thing selected, if it takes typing: at the end of its text, or, with
    /// none, in it empty, where nothing is written until something is typed. Says whether it did.
    pub fn begin_text_in_selected(&mut self) -> bool {
        if let Err(refused) = self.end_text() {
            self.say(refused);
        }
        if !self.selected_takes_text() {
            return false;
        }
        let Some(Selected { slide, source }) = self.selected.clone() else {
            return false;
        };
        self.begin_text_in_tag(slide, source.start)
    }

    /// Starts typing in the shape, box or component taking content whose tag starts at byte
    /// `start` of slide `slide`: at the end of its text, or, with none, in it empty. Says whether
    /// it did.
    fn begin_text_in_tag(&mut self, slide: usize, start: usize) -> bool {
        let content = self.content_tags();
        if slide >= self.slide_count() {
            return false;
        }
        let Ok(flows) = guarded(|| flows_in(self.talk(), slide, &content)) else {
            return false;
        };
        let key = FlowKey::Tag { start };
        if let Some(index) = key.find(&flows) {
            let end = TextEdit::new(flows[index].paras.clone()).select_all().selection.end();
            return self.begin_text(slide, index, end);
        }
        if !tag_starting_at(self.talk(), slide, start).is_some_and(|tag| waits_for_text(tag, &content)) {
            return false;
        }
        if let Err(refused) = self.end_text() {
            self.say(refused);
        }
        self.go_to(slide);
        self.editing = Some(EditingText {
            session: self.new_session(),
            ..EditingText::in_empty_tag(slide, start)
        });
        self.keep_selection_with_typing();
        true
    }

    /// Moves the selection along with `change`, just made to the talk's text: to where its
    /// thing's text went, or away if that went.
    fn follow(&mut self, change: &Change) {
        // The box being typed in moves as the thing selected does; if it can't be followed,
        // typing there ends.
        if let Some(editing) = &mut self.editing {
            match editing.flow.moved(change) {
                Some(flow) => editing.flow = flow,
                None => self.editing = None,
            }
        }
        let Some(selected) = self.selected.take() else {
            return;
        };
        let talk = self.talk();
        self.selected = selection::moved(&selected.source, change).and_then(|source| {
            let slide = selection::slide_of(talk, &source)?;
            selection::has_block(talk, slide, &source).then_some(Selected { slide, source })
        });
    }

    /// Lets go of the thing selected if text is being typed on its slide outside it: typing
    /// in the thing keeps it selected, and typing on another slide leaves it be. Whatever starts
    /// or brings back typing, a click, an undo or a redo, ends with this.
    fn keep_selection_with_typing(&mut self) {
        let (Some(editing), Some(selected)) = (&self.editing, &self.selected) else {
            return;
        };
        if editing.slide != selected.slide {
            return;
        }
        if editing.flow.tag_start() == Some(selected.source.start) {
            return;
        }
        let para = editing.edit.selection.head.para;
        let inside = self
            .text_flow()
            .is_some_and(|flow| selection::holds_para(&self.text_flows(self.talk(), editing.slide), flow, para, &selected.source));
        if !inside {
            self.selected = None;
        }
    }

    /// Selects what a step of the history marked, once it's undone or made again: the thing at
    /// `source`, or nothing.
    fn select_marked(&mut self, source: Option<Option<Range<usize>>>) {
        let Some(source) = source else {
            return;
        };
        let talk = self.talk();
        let selected = source.and_then(|source| {
            let slide = selection::slide_of(talk, &source)?;
            selection::has_block(talk, slide, &source).then_some(Selected { slide, source })
        });
        if selected != self.selected {
            self.selected = selected;
            self.picks += 1;
        }
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
        let content = self.content_tags();
        if slide >= self.slide_count() {
            return false;
        }
        let Ok(flows) = guarded(|| flows_in(self.talk(), slide, &content)) else {
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
        self.go_to(slide);
        let Ok(flows) = guarded(|| flows_in(self.talk(), slide, &content)) else {
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
        self.keep_selection_with_typing();
        true
    }

    /// Starts typing in region `region` of slide `slide`, which has nothing in it, and makes
    /// `slide` the current slide. Nothing is written until something is typed. A region with
    /// text in it is typed into at the end of its first run of text. Says whether the slide's
    /// layout has such a region to type in.
    pub fn begin_text_in_region(&mut self, slide: usize, region: &str) -> bool {
        let content = self.content_tags();
        if slide >= self.slide_count() {
            return false;
        }
        if let Err(refused) = self.end_text() {
            self.say(refused);
        }
        self.go_to(slide);
        let Ok(flows) = guarded(|| flows_in(self.talk(), slide, &content)) else {
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
        self.keep_selection_with_typing();
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
        let content = self.content_tags();
        // Anything done to the text drops what the system was composing: a commit types it.
        self.compose(None);
        let Some(editing) = self.editing.clone() else {
            return Ok(Vec::new());
        };
        let slide = editing.slide;
        let flows = (slide < self.slide_count())
            .then(|| guarded(|| flows_in(self.talk(), slide, &content)).ok())
            .flatten();
        let Some(flows) = flows.filter(|flows| reads_as_typed(self.talk(), &content, flows, &editing)) else {
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
        if let Some(place) = editing.flow.new_box() {
            // Typed or pasted, what first goes in a new box writes it, as "Insert text box".
            return self.write_new_box(&editing, place, after, kind);
        }
        let found = editing.flow.find(&flows);
        let change = match (found, editing.flow.region()) {
            (Some(index), _) => write_flow(self.talk(), slide, &flows[index], &after.paras, &content),
            // An empty region's first paragraph is written once it holds something.
            (None, Some(_)) if !holds_something(&after.paras) => {
                self.keep_typed(after, editing.run);
                return Ok(Vec::new());
            }
            (None, Some(region)) => write_into_region(self.talk(), slide, region, &after.paras),
            // So is an empty box's.
            (None, None) if !holds_something(&after.paras) => {
                self.keep_typed(after, editing.run);
                return Ok(Vec::new());
            }
            (None, None) => match editing.flow.tag_start() {
                Some(start) => guarded(|| write_into_tag(self.talk(), &content, slide, start, &after.paras))?,
                None => None,
            },
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
            let flows = flows_in(candidate, slide, &content);
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
                flow_after: None,
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
        if let FlowKey::NewBox { x, y, .. } = editing.flow {
            // Kept for the box, which is written with it.
            let align = match align {
                Align::Left => None,
                Align::Center => Some("center"),
                Align::Right => Some("right"),
            };
            if let Some(editing) = &mut self.editing {
                editing.flow = FlowKey::NewBox { x, y, align };
            }
            return Ok(given);
        }
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
            let place = typed_place(talk, &editing, &self.content_tags())?;
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
                flow_after: None,
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
        if editing.flow.new_box().is_some() {
            return Some(match editing.flow.new_box_align() {
                Some("center") => Align::Center,
                Some("right") => Align::Right,
                _ => Align::Left,
            });
        }
        let talk = self.talk();
        guarded(|| text_align(talk, editing.slide, &typed_place(talk, editing, &self.content_tags())?))
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
        let content = self.content_tags();
        // What the system was composing isn't text yet, and isn't kept.
        self.compose(None);
        let given = self.give_held_first();
        let Some(editing) = self.editing.clone() else {
            return Ok(given);
        };
        if editing.flow.new_box().is_some() {
            // A new box nothing is typed in yet is dropped, so that nothing of it is saved,
            // shown or exported.
            self.editing = None;
            return Ok(given);
        }
        let (changed, removed) = self.tidy(&editing)?;
        let changed = joined(given, changed);
        if removed.is_some() {
            let region = editing.flow.region().unwrap_or_default();
            let flows = guarded(|| flows_in(self.talk(), editing.slide, &content)).ok();
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
        let content = self.content_tags();
        let slide = editing.slide;
        if editing.flow.region().is_none() || slide >= self.slide_count() {
            return Ok((Vec::new(), None));
        }
        let (change, index, count) = {
            let talk = self.talk();
            let flows = guarded(|| flows_in(talk, slide, &content))?;
            let Some(index) = editing.flow.find(&flows) else {
                return Ok((Vec::new(), None));
            };
            if !holds_no_words(&flows[index].paras) {
                return Ok((Vec::new(), None));
            }
            (write_flow(talk, slide, &flows[index], &[], &content), index, flows.len())
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
        let emptied = |candidate: &Talk| flows_in(candidate, slide, &content).len() + 1 == count;
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
                flow_after: None,
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
        let content = self.content_tags();
        let undo = self.history.next_undo()?;
        let slide = editing.slide;
        let alike = guarded(|| {
            let before = reparse(self.talk(), undo.apply(self.source()));
            let mut now = flows_in(self.talk(), slide, &content);
            now.remove(index);
            let then = flows_in(&before, slide, &content);
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
        let content = self.content_tags();
        let Some(mark) = mark else {
            return;
        };
        let selection = if before { Some(mark.before) } else { mark.after };
        let Some(selection) = selection else {
            return;
        };
        let mut mark = mark;
        if !before && let Some(flow) = mark.flow_after.take() {
            mark.flow = flow;
        }
        let slide = mark.slide;
        if slide >= self.slide_count() {
            return;
        }
        let Ok(flows) = guarded(|| flows_in(self.talk(), slide, &content)) else {
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
            (None, None) => mark
                .flow
                .tag_start()
                .filter(|&start| tag_starting_at(self.talk(), slide, start).is_some_and(|tag| waits_for_text(tag, &content)))
                .map(|start| EditingText {
                    session: mark.session,
                    ..EditingText::in_empty_tag(slide, start)
                }),
            _ => None,
        };
    }

    /// The tags of the talk's components whose content is a box of text of its own as things
    /// stand: those whose build is loaded, works and says it takes content. Nothing is loaded
    /// to find out, so this is what the engine's text layout reads the slides with too.
    fn content_tags(&self) -> Vec<String> {
        self.deck.components().iter().filter(|info| self.deck.takes_content(&info.tag)).map(|info| info.tag.clone()).collect()
    }

    /// The flows of slide `slide` of `talk`, as the slide's text layout counts them.
    pub fn text_flows(&self, talk: &Talk, slide: usize) -> Vec<TextFlow> {
        flows_in(talk, slide, &self.content_tags())
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
            self.components_looked_at();
            Reload::Changed(changed)
        }
    }

    /// What the component palette offers: a chart first, then the talk's own components in name
    /// order, each with how built it is. The tags with tools of their own aren't in it. It's
    /// worked out as the talk's components change, not each time it's asked for.
    pub fn palette(&mut self) -> Vec<PaletteEntry> {
        self.palette.clone()
    }

    /// Lists the palette again from the talk's components as they are now: which there are and
    /// how built each is. Nothing is loaded: what each takes is known only once its build is.
    fn components_looked_at(&mut self) {
        let deck = &self.deck;
        let listed = guarded(|| {
            let own: Vec<String> = deck.components().iter().map(|info| info.tag.clone()).collect();
            let theirs = deck.available_components().into_iter().filter(|tag| own.contains(tag) && !LISTED_BUILT_INS.contains(&tag.as_str()));
            LISTED_BUILT_INS.iter().map(|tag| tag.to_string()).chain(theirs).collect::<Vec<_>>()
        });
        let entries: Vec<PaletteEntry> = listed.unwrap_or_default().iter().map(|tag| self.palette_entry(tag)).collect();
        self.palette = entries;
    }

    /// The palette's entry for `tag`, as far as is known without loading anything.
    fn palette_entry(&self, tag: &str) -> PaletteEntry {
        let state = self.deck.components().iter().find(|info| info.tag == tag).map(|info| info.state);
        let (takes_content, loading) = match self.deck.loaded_schema(tag) {
            SchemaLoad::Loaded(schema) => (schema.children, false),
            SchemaLoad::Failed => (false, false),
            _ => (false, state != Some(ComponentState::NeedsBuilding) && !self.palette_given_up.contains(tag)),
        };
        PaletteEntry { tag: tag.to_string(), label: palette::label(tag), ours: state.is_some(), state, takes_content, loading }
    }

    /// Starts an opening of the palette: each of its components whose build isn't loaded yet is
    /// to be, by [`Self::palette_step`], with a budget of its own.
    pub fn begin_palette(&mut self) {
        self.palette_given_up.clear();
        self.components_looked_at();
    }

    /// Loads the build of the first of the palette's components still loading, and fills in its
    /// entry: one module at a time, which the sandbox holds to a quarter of a second of running.
    /// Says whether any is still loading.
    pub fn palette_step(&mut self) -> bool {
        let Some(at) = self.palette.iter().position(|entry| entry.loading) else {
            return false;
        };
        let tag = self.palette[at].tag.clone();
        let deck = &mut self.deck;
        let started = Instant::now();
        let _ = guarded(|| deck.component_schema(&tag));
        self.palette_longest_step = self.palette_longest_step.max(started.elapsed());
        if let SchemaLoad::NotLoaded = self.deck.loaded_schema(&tag) {
            // Nothing could be loaded for it: it isn't tried again this opening.
            self.palette_given_up.insert(tag.clone());
        }
        self.palette[at] = self.palette_entry(&tag);
        self.palette.iter().any(|entry| entry.loading)
    }

    /// Stops loading for this opening of the palette: what isn't loaded yet is listed as it is,
    /// to be loaded if it's chosen.
    pub fn give_up_palette(&mut self) {
        let loading: Vec<String> = self.palette.iter().filter(|entry| entry.loading).map(|entry| entry.tag.clone()).collect();
        self.palette_given_up.extend(loading);
        self.components_looked_at();
    }

    /// The longest one [`Self::palette_step`] has taken, for tests.
    #[doc(hidden)]
    pub fn palette_longest_step(&self) -> Duration {
        self.palette_longest_step
    }

    /// How many of the talk's modules have been loaded, or tried, for tests.
    #[doc(hidden)]
    pub fn modules_loaded(&self) -> usize {
        self.deck.modules_loaded()
    }

    /// Puts the component `tag` from the palette on the current slide, [`COMPONENT_SIZE`] big,
    /// as one step named "Insert component", and selects it: written with what
    /// [`palette::default_attrs`] gives, and, if it takes content, an empty body. One the
    /// palette doesn't offer, or one with no build, puts nothing there. One whose build can't
    /// be loaded is refused. Nothing is built.
    pub fn insert_component(&mut self, tag: &str) -> Result<Vec<usize>, EditRefused> {
        let listed = self.palette.iter().any(|entry| entry.tag == tag && entry.state != Some(ComponentState::NeedsBuilding));
        if !listed {
            return Ok(Vec::new());
        }
        let deck = &mut self.deck;
        let schema = guarded(|| deck.component_schema(tag))?;
        if let Some(at) = self.palette.iter().position(|entry| entry.tag == tag) {
            self.palette[at] = self.palette_entry(tag);
        }
        let schema = schema.ok_or(EditRefused::ComponentNotLoaded)?;
        let attrs = palette::default_attrs(&schema, &self.deck.theme());
        let attrs: Vec<(&str, &str)> = attrs.iter().map(|(name, value)| (name.as_str(), value.as_str())).collect();
        let body = schema.children.then_some("");
        self.insert_sized(INSERT_COMPONENT, tag, &attrs, COMPONENT_SIZE, body)
    }

    /// The talk's theme, as the engine reads it.
    pub fn theme(&self) -> Theme {
        self.deck.theme()
    }

    /// Whether this talk came from a packed file, rather than a folder.
    pub fn is_packed(&self) -> bool {
        matches!(self.place, TalkPlace::Packed(_))
    }

    pub fn place(&self) -> &TalkPlace {
        &self.place
    }

    /// A save of the talk as it is now, where it's kept: for a folder, only its `slides.md` and
    /// the files added that it uses.
    pub fn save_job(&self) -> SaveJob {
        SaveJob::new(
            self.source().to_string(),
            self.on_disk.clone(),
            self.place.clone(),
            None,
            self.added_files(),
            self.index_for_save(),
        )
    }

    /// A save of the talk as it is now, somewhere new.
    pub fn save_as_job(&self, target: SaveTarget) -> SaveJob {
        SaveJob::new(
            self.source().to_string(),
            self.on_disk.clone(),
            self.place.clone(),
            Some(target),
            self.added_files(),
            self.index_for_save(),
        )
    }

    /// The builds entered in the talk's index with the files added to it, of builds its text
    /// uses, for a save to enter in the index it writes.
    pub fn builds_to_enter(&self) -> Vec<BuildEntry> {
        let talk = self.talk();
        self.added
            .builds_entered()
            .into_iter()
            .filter(|entry| talk_uses(talk, &*self.files, &format!("build/{}.wasm", entry.file)))
            .collect()
    }

    /// The builds a save enters in the talk's index, and the index with them entered in it, as a
    /// save anywhere but the talk's own folder writes it. It's the talk's own index as it's kept,
    /// every entry as it is, with an entry added for each component pasted whose source and build
    /// this save writes, as [`Self::builds_to_enter`] gives them, but for one the index has an
    /// entry for the same component or file: that's the talk's, and never replaced. An entry
    /// whose files this save doesn't write stays to be entered. `None` for the index if the
    /// talk's own can't be read, which a save leaves as it is.
    fn index_for_save(&self) -> (Vec<BuildEntry>, Option<Vec<u8>>) {
        let writing: BTreeSet<String> = self.added_files().into_iter().map(|(path, _)| path).collect();
        let own = &*self.own_files;
        let index = match own.len(INDEX_FILE) {
            None if own.read(INDEX_FILE).is_none() => Some(BuildIndex { format: 1, components: Vec::new() }),
            _ => added_files::build_index(own),
        };
        let builds: Vec<BuildEntry> = self
            .builds_to_enter()
            .into_iter()
            .filter(|entry| writing.contains(&format!("build/{}.wasm", entry.file)) && writing.contains(&format!("components/{}.rs", entry.file)))
            .filter(|entry| index.as_ref().is_none_or(|index| !index.components.iter().any(|kept| kept.tag == entry.tag || kept.file == entry.file)))
            .collect();
        if builds.is_empty() {
            return (builds, None);
        }
        let index = index.and_then(|mut index| {
            index.components.extend(builds.iter().cloned());
            index.format = index.format.max(1);
            serde_json::to_vec_pretty(&index).ok()
        });
        (builds, index)
    }

    /// Takes in a save that worked. The talk is saved if its text is still what was written,
    /// and after a Save As it's kept in the new place from now on.
    pub fn saved(&mut self, saved: Saved) {
        // Part of the talk now, wherever it's kept.
        for path in &saved.files_written {
            self.pending.remove(path);
        }
        // In the index where the talk is kept now, which is read as it is from here on.
        self.added.forget_builds(&saved.builds_written);
        if let Some(files) = saved.reopened {
            self.own_files = files.clone();
            self.files = Arc::new(WithAdded::new(files, self.added.clone()));
            self.deck.set_files(Box::new(self.files.clone()));
            self.components_looked_at();
        }
        if let Some((place, files)) = saved.moved_to {
            self.place_name = place.name();
            self.place = place;
            self.own_files = files.clone();
            self.files = Arc::new(WithAdded::new(files, self.added.clone()));
            self.deck.set_files(Box::new(self.files.clone()));
            self.components_looked_at();
        }
        // Read from where the talk is kept from here on, as its own files are, so that a change
        // to one there is seen as a change to any other is. One that isn't there to be read, as
        // in a pack that wasn't opened again, stays in memory.
        let written: Vec<String> = saved.files_written.iter().filter(|path| self.own_files.len(path).is_some()).cloned().collect();
        if self.added.release(&written) {
            self.deck.files_added();
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
        let old = (!same_text).then(|| self.source().to_string());
        // Even the same text goes through: a picture it shows may have been replaced.
        let mut changed = self.deck.update_parsed(talk);
        if let Some(old) = old {
            self.follow(&one_edit(&old, self.deck.source()));
        }
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
fn reads_as_typed(talk: &Talk, content: &[String], flows: &[slides_format::TextFlow], editing: &EditingText) -> bool {
    match (editing.flow.find(flows), editing.flow.region()) {
        (Some(index), _) => flows[index].paras == storable(&editing.edit.paras),
        (None, Some(region)) => {
            !region_has_text(flows, region) && !holds_something(&editing.edit.paras)
        }
        (None, None) if editing.flow.new_box().is_some() => !holds_something(&editing.edit.paras),
        (None, None) => {
            let start = editing.flow.tag_start();
            start.is_some_and(|start| tag_starting_at(talk, editing.slide, start).is_some_and(|tag| waits_for_text(tag, content)))
                && !holds_something(&editing.edit.paras)
        }
    }
}

/// The flows of slide `slide` of `talk`, with the tags in `content` taking content, as the
/// engine's text layout reads them: see [`Document::content_tags`].
fn flows_in(talk: &Talk, slide: usize, content: &[String]) -> Vec<TextFlow> {
    text_flows_with(talk, slide, &|tag| content.iter().any(|name| name == tag))
}

/// [`write_flow_with`], with the tags in `content` taking content.
fn write_flow(talk: &Talk, slide: usize, flow: &TextFlow, paras: &[Para], content: &[String]) -> Option<Change> {
    write_flow_with(talk, slide, flow, paras, &|tag| content.iter().any(|name| name == tag))
}

/// Whether `tag` is one typing on it puts words in: a shape, a box, or one of the talk's own
/// components that draws what's written between its tags, whose tags are `content`.
fn takes_text(tag: &Tag, content: &[String]) -> bool {
    matches!(tag.name.as_str(), "Shape" | "Box") || content.contains(&tag.name)
}

/// Whether `tag` takes typing and has nothing in it yet.
fn waits_for_text(tag: &Tag, content: &[String]) -> bool {
    takes_text(tag, content) && tag.children.is_empty()
}

/// The tag on slide `slide` of `talk`, at any depth, that starts at byte `start`.
fn tag_starting_at(talk: &Talk, slide: usize, start: usize) -> Option<&Tag> {
    fn within(blocks: &[Block], start: usize) -> Option<&Tag> {
        blocks.iter().find_map(|block| match block {
            Block::Tag(tag) if tag.range.start == start => Some(tag),
            Block::Tag(tag) => within(&tag.children, start),
            Block::List { items, .. } => items.iter().find_map(|item| within(item, start)),
            _ => None,
        })
    }
    let slide = talk.slides.get(slide).filter(|slide| slide.error.is_none())?;
    within(&slide.blocks, start)
}

/// `paras` from their first visible character on ([`is_visible_char`]): the paragraphs before
/// it, and what's before it in its own, left out. `None` if there's none.
fn from_first_visible(paras: &[Para]) -> Option<Vec<Para>> {
    let visible = |para: &Para| para.spans.iter().any(|span| span.text.chars().any(is_visible_char));
    let first = paras.iter().position(visible)?;
    let mut out = paras[first..].to_vec();
    let spans = &mut out[0].spans;
    while let Some(span) = spans.first_mut() {
        let rest = span.text.trim_start_matches(|c: char| !is_visible_char(c));
        if rest.is_empty() {
            spans.remove(0);
        } else {
            span.text = rest.to_string();
            break;
        }
    }
    Some(out)
}

/// The change that writes a new text box, at (`x`, `y`) and [`TEXT_BOX_SIZE`], holding `paras`,
/// at the end of slide `slide`'s content, as one edit, and where its tag starts once it's made:
/// the box is written holding one empty paragraph, and that then written as `paras`. `None` if
/// it can't be.
fn new_box_change(talk: &Talk, slide: usize, (x, y): (u32, u32), align: Option<&str>, paras: &[Para], content: &[String]) -> Option<(Change, usize)> {
    let place = [x.to_string(), y.to_string(), insert::whole(TEXT_BOX_SIZE.0), insert::whole(TEXT_BOX_SIZE.1)];
    let mut attrs: Vec<(&str, &str)> = ["x", "y", "w", "h"].into_iter().zip(place.iter().map(String::as_str)).collect();
    attrs.extend(align.map(|align| ("align", align)));
    let text = tag_text(&NewTag { name: "Box", attrs: &attrs, body: Some("&nbsp;") })?;
    let opened = insert_block(talk, slide, &text)?;
    let edit = opened.edits.first()?;
    let written = if edit.text.contains("\r\n") { text.replace('\n', "\r\n") } else { text.clone() };
    let start = edit.range.start + edit.text.find(&written)?;
    let held = reparse(talk, opened.apply(&talk.source));
    let flows = flows_in(&held, slide, content);
    let index = FlowKey::Tag { start }.find(&flows)?;
    let filled = write_flow(&held, slide, &flows[index], paras, content)?;
    Some((one_edit(&talk.source, &filled.apply(&held.source)), start))
}

/// The change that puts `paras` in the shape, box or component taking content, one of `content`,
/// with nothing in it whose tag starts at byte `start` of slide `slide`: the tag given a body of
/// one empty paragraph, and that written as `paras`, as one edit inside the tag. `None` if it
/// can't be.
fn write_into_tag(talk: &Talk, content: &[String], slide: usize, start: usize, paras: &[Para]) -> Option<Change> {
    let tag = tag_starting_at(talk, slide, start).filter(|tag| waits_for_text(tag, content))?;
    let opened = set_tag_body(talk, slide, &tag.range, "&nbsp;")?;
    let held = reparse(talk, opened.apply(&talk.source));
    let flows = flows_in(&held, slide, content);
    let index = FlowKey::Tag { start }.find(&flows)?;
    let written = write_flow(&held, slide, &flows[index], paras, content)?;
    let text = written.apply(&held.source);
    Some(one_edit(&talk.source, &text))
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
fn typed_place(talk: &Talk, editing: &EditingText, content: &[String]) -> Option<FlowPlace> {
    match &editing.flow {
        FlowKey::Region { name, .. } => Some(FlowPlace::Region(name.clone())),
        FlowKey::Cell { .. } => {
            let flows = flows_in(talk, editing.slide, content);
            editing.flow.find(&flows).map(|index| flows[index].place.clone())
        }
        FlowKey::Tag { start } => {
            tag_starting_at(talk, editing.slide, *start).map(|tag| FlowPlace::Tag(tag.range.clone()))
        }
        FlowKey::NewBox { .. } => None,
    }
}

/// Whether `a` and `b` are the same kind of place: the same region, both boxes, or the same
/// cell of a table, whose ranges move with the text before them.
fn same_place(a: &slides_format::FlowPlace, b: &slides_format::FlowPlace) -> bool {
    use slides_format::FlowPlace::{Cell, Region, Tag};
    match (a, b) {
        (Region(a), Region(b)) => a == b,
        (Tag(_), Tag(_)) => true,
        (Cell { row: r1, column: c1, .. }, Cell { row: r2, column: c2, .. }) => (r1, c1) == (r2, c2),
        _ => false,
    }
}

/// The table the table tools are about, and the cell of it they act on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableHere {
    pub slide: usize,
    /// The table's whole source.
    pub table: Range<usize>,
    /// The cell's row, the heading row being row 0, and its column: the caret's, or, for a table
    /// selected with nothing typed, its last row and column, where Row below and Column right
    /// add at its end.
    pub row: usize,
    pub column: usize,
    /// Whether the caret is in that cell.
    pub caret: bool,
    /// How many columns the heading row has, and how many rows there are, the heading row with
    /// them.
    pub columns: usize,
    pub rows: usize,
    /// Whether a row has more cells than the heading row, which are in no column.
    pub long_row: bool,
}

/// One of the tools that grow and shrink a table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableTool {
    RowAbove,
    RowBelow,
    ColumnLeft,
    ColumnRight,
    DeleteRow,
    DeleteColumn,
}

impl TableHere {
    /// Why `tool` can't be used on this table, at this cell, if it can't.
    pub fn why_not(&self, tool: TableTool) -> Option<EditRefused> {
        use TableTool::*;
        match tool {
            // A table selected, with no caret, is only added to at its end.
            RowAbove | ColumnLeft | DeleteRow | DeleteColumn if !self.caret => Some(EditRefused::NeedsACell),
            RowAbove | DeleteRow if self.row == 0 => Some(EditRefused::KeepsHeadingRow),
            RowAbove | RowBelow if self.rows >= MAX_TABLE_ROWS => Some(EditRefused::TooManyRows),
            ColumnLeft | ColumnRight | DeleteColumn if self.long_row => Some(EditRefused::RowTooLong),
            ColumnLeft | ColumnRight if self.columns >= MAX_TABLE_COLUMNS => Some(EditRefused::TooManyColumns),
            DeleteColumn if self.columns <= 1 => Some(EditRefused::KeepsOneColumn),
            _ => None,
        }
    }
}

/// Where the caret goes from a cell of a table: on, as Tab takes it, back, as Shift+Tab does,
/// or down, as Enter does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellMove {
    Next,
    Previous,
    Below,
}

/// Which cell beside a cell of a table: the one written before it or after it, or the one above
/// or below it in its column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Beside {
    Before,
    After,
    Above,
    Below,
}

/// The whole source of the table on slide `slide` of `talk`, at any depth, that starts at byte
/// `start`.
fn table_starting_at(talk: &Talk, slide: usize, start: usize) -> Option<Range<usize>> {
    fn within(blocks: &[Block], start: usize) -> Option<Range<usize>> {
        blocks.iter().find_map(|block| match block {
            Block::Table { range, .. } if range.start == start => Some(range.clone()),
            Block::Tag(tag) => within(&tag.children, start),
            Block::List { items, .. } => items.iter().find_map(|item| within(item, start)),
            _ => None,
        })
    }
    let slide = talk.slides.get(slide).filter(|slide| slide.error.is_none())?;
    within(&slide.blocks, start)
}

/// The whole source of each table on slide `slide` of `talk`, at any depth.
fn tables_on(talk: &Talk, slide: usize) -> Vec<Range<usize>> {
    fn within(blocks: &[Block], found: &mut Vec<Range<usize>>) {
        for block in blocks {
            match block {
                Block::Table { range, .. } => found.push(range.clone()),
                Block::Tag(tag) => within(&tag.children, found),
                Block::List { items, .. } => items.iter().for_each(|item| within(item, found)),
                _ => {}
            }
        }
    }
    let mut found = Vec::new();
    if let Some(slide) = talk.slides.get(slide).filter(|slide| slide.error.is_none()) {
        within(&slide.blocks, &mut found);
    }
    found
}

/// Where the end of `paras` is.
fn end_of(paras: &[Para]) -> Pos {
    TextEdit::new(paras.to_vec()).select_all().selection.end()
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
