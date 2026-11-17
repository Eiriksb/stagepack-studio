//! Why an edit or an opening didn't happen, and the guards that turn a panic in the
//! format crate or the engine into a refusal.

use super::*;
use crate::clipboard;

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
    /// Only a layout's areas are selected, and an area stays where its layout puts it.
    AreaStays,
    /// Fewer than three things are selected to space out.
    NeedsThree,
    /// A thing selected is written in the slide's text, with no place of its own.
    InTheText,
    /// A layout's area can't be put in front of or behind anything.
    AreaOrder,
    /// Several things are selected, and only one at a time is put in front or behind.
    OneToOrder,
    /// Nothing is selected on the slide, nor an area typed in, for the Animate tab to act on.
    NothingToAnimate,
    /// The slide has as many steps as it can have, so nothing more can come in on one.
    TooManySteps,
    /// The area chosen holds one block and no list, so it can't be shown a piece at a time.
    NotAList,
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
            EditRefused::AreaStays => AREA_STAYS,
            EditRefused::NeedsThree => "Select three or more things to space them out.",
            EditRefused::InTheText => DRAG_OUT_FIRST,
            EditRefused::AreaOrder => "A layout's area always sits behind the things on the slide.",
            EditRefused::OneToOrder => "Select one thing to bring it forward or send it back.",
            EditRefused::NothingToAnimate => "Select something on the slide first.",
            EditRefused::TooManySteps => "This slide has as many clicks as it can have.",
            EditRefused::NotAList => "Select an area that holds a list to show it one item at a time.",
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

/// What's said when a thing written in the slide's text, with no place of its own, is moved.
pub const DRAG_OUT_FIRST: &str = "Drag it out of the text first.";
/// What's said when only a layout's areas are selected and Delete, Cut, Copy or Duplicate is asked.
pub const AREA_STAYS: &str = "A layout's area can't be deleted or copied. Choose another layout to change the areas.";

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
pub(super) fn laid_out<T>(work: impl FnOnce() -> T) -> Option<T> {
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
pub(super) fn guarded<T>(work: impl FnOnce() -> T) -> Result<T, EditRefused> {
    catch_unwind(AssertUnwindSafe(work)).map_err(|_| EditRefused::CouldNotBeMade)
}

/// What `work`, some of the format crate's work, gives, guarded as [`guarded`] is.
pub(super) fn worked_out<T>(work: impl FnOnce() -> T) -> Result<T, EditRefused> {
    guarded(|| {
        if PANIC_IN_NEXT_CHANGE.with(|flag| flag.replace(false)) {
            panic!("a change that panics, for a test");
        }
        work()
    })
}

/// What `build` makes of `talk`: the change a command works out, guarded as [`guarded`] is.
pub(super) fn built<T>(talk: &Talk, build: impl FnOnce(&Talk) -> T) -> Result<T, EditRefused> {
    worked_out(|| build(talk))
}

/// The talk `source` reads as, the text of `talk` once changed, guarded as [`guarded`] is.
/// Only the slides whose text changed are read again.
pub(super) fn read_after(talk: &Talk, source: String) -> Result<Talk, EditRefused> {
    worked_out(|| reparse(talk, source))
}

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
