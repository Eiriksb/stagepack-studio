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
    AddedSet, Align, ComponentInfo, ComponentState, Deck, SchemaLoad, ElementBox, Rect, FolderFiles, Fonts, Layer, LayerTiming, Moment, PackError, Pixmap,
    SlideTimeline, TalkFiles, TextLayout, Transition, WithAdded, layout_by_name, open_packed, text_align,
    read_talk_text,
};
use slides_format::{
    Change, FlowPlace, TextFlow, MAX_SLIDE_BYTES, Para, Pos, Selection, SourceEdit, Talk, TextEdit, is_visible_char, parse_talk, reparse, set_align,
    Block, NewTag, Tag, block_text, delete_block, find_tag, insert_block, set_tag_attr, set_tag_body, split_slides, storable,
    tag_text, text_flows_with, write_flow_with, write_into_region, MAX_TABLE_COLUMNS, MAX_TABLE_ROWS, new_table, table_size,
    Target, ValueEdit, set_values,
};
use toolkit::{ComponentSchema, Theme};

use crate::added_files::{self, FileKind, MAX_ADDED_BYTES, NotAdded, Placed, talk_uses};
use crate::clipboard::{CopiedFile, MAX_CLIPBOARD_FILES, MAX_CLIPBOARD_FILES_BYTES, PastedFile, PastedFiles};
use crate::export::ExportInput;
use crate::history::{History, RunId, one_edit};
use crate::insert::{self, INSERT_COMPONENT, INSERT_PICTURE, INSERT_SHAPE, INSERT_TABLE, INSERT_TEXT_BOX, TEXT_BOX_SIZE};
use crate::palette::{self, COMPONENT_SIZE, LISTED_BUILT_INS};
pub use crate::palette::PaletteEntry;
use crate::present::TalkShape;
use crate::saving::{SaveJob, SaveTarget, Saved};
use crate::table_tools::{CLICK_IN_A_CELL, HEADING_ROW_STAYS, LONG_ROW, MOST_COLUMNS, MOST_ROWS, ONE_COLUMN_STAYS};
use crate::selection::{self, SelectMark};
pub use crate::selection::{Selected, Thing};
use crate::text_editing::{
    ALIGN_TEXT, COULD_NOT_TYPE, CUT, Composing, EditingText, FORMAT_TEXT, FlowKey, Kind, PASTE, TYPING, TextMark,
    holds_no_words, holds_something, region_has_text,
};

mod animating;
mod arranging;
mod clipboard;
mod components;
mod core;
mod ending_text;
mod errors;
mod files;
mod inserting;
mod open;
mod reading;
mod saving;
mod selecting;
mod slides;
mod tables;
mod typing;

pub use animating::{ANIMATE, AnimateHere, StepBadge, TRANSITION};
pub use arranging::{ALIGN, ArrangeHere, DISTRIBUTE, MOVE, MOVE_OUT, NUDGE, ORDER, RESIZE, TURN};
pub use clipboard::Brought;
pub use self::core::EditRun;
use self::core::{Holds, Join, others_alike};
pub use errors::{AREA_STAYS, DRAG_OUT_FIRST, EditRefused, InsertFailed, LAYOUT_FAILED, OpenProblem, panic_in_next_change, panic_in_next_layout};
use errors::{built, guarded, laid_out, read_after, worked_out};
pub use open::TalkPlace;
pub use reading::talk_settings;
pub use saving::{Reload, UNREADABLE_FILE};
pub use slides::CHANGE_LAYOUT;
pub use tables::{Beside, CellMove, TableHere, TableTool};
use selecting::made_in_turn;
use tables::tables_on;
use typing::{HeldColor, flows_in, from_first_visible, joined, new_box_change, tag_starting_at, waits_for_text, write_flow};

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
const PUT_BACK: &str = "Put back";
const ADD_ROW: &str = "Add row";
const ADD_COLUMN: &str = "Add column";
const DELETE_ROW: &str = "Delete row";
const DELETE_COLUMN: &str = "Delete column";

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
    /// The build index the last save wrote into the pack the talk is kept in, when the pack
    /// couldn't be opened again after it: what the next save's index starts from.
    index_kept_in_pack: Option<Vec<u8>>,
    /// The things picked on a slide, if any: the editor's view of the talk, not part of it.
    selection: Option<selection::Selection>,
    /// The one block picked, while that's all that's picked: what [`Self::selected`] gives.
    selected: Option<Selected>,
    /// Where the slide the things are picked on is in the talk's text, which an area picked is
    /// followed through an edit by.
    selection_slide: Option<Range<usize>>,
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
