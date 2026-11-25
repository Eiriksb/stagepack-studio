//! The window's content: the title bar, the tools on their tabs, the list of slides, the
//! slide with its notes and notices, the talk's Markdown, the bar that offers to build the
//! talk's components, and saving the talk.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input;
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::resizable::{ResizableState, h_resizable, resizable_panel, v_resizable};
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, TitleBar, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use slides_components::{Bundle, Diagnostic};
use slides_engine::{ComponentState, Fonts};
use slides_format::DEFAULT_LAYOUT;

use crate::animate::Choice;
use crate::animate_tools::{ChooseAnimation, ChooseEntrance, ChooseTransition};
use crate::arrange_tools::{AlignThings, DistributeThings, ReorderThing};
use crate::builds::{self, BuildOutcome, Compile, NEEDS_BUILDING, NO_COMPILER};
use crate::cargo_file::{self, write_cargo_file};
use crate::clipboard::ClipboardOp;
use crate::clipboard_tools::ClipboardTools;
use crate::document::{Document, EditRefused, OpenProblem, Reload, SLIDES_FILE, TalkPlace, UNREADABLE_FILE, UntitledId};
use crate::export::{self, ExportKind, stagepack_executable};
use crate::icon_picker::{self, IconPicked, IconPicker};
use crate::palette::InsertComponent;
use crate::insert_tools::{InsertButton, InsertModel, InsertPicture, InsertShape, InsertTerminal, InsertTextBox, InsertVideo};
use crate::layout_picker::SetLayout;
use crate::inspector::{self, Inspector};
use crate::markdown_panel::MarkdownPanel;
use crate::notes_panel::{
    NOTES_HEIGHT, NOTES_MAX_HEIGHT, NOTES_MIN_HEIGHT, NotesEdited, NotesFocused, NotesPanel,
};
use crate::live_terminal::{Decision, LiveTerminals, Plan, RunAnswer, RunPermissions, RunQuestion, SessionYes, ask_to_run, may_run, plan};
use crate::permissions::BuildPermissions;
use crate::present_view::{self, Presentation};
use crate::recovery::{Found, NotTaken, Recovery};
use crate::ribbon::{Panels, Ribbon, RibbonTab};
use crate::save_flow::{CloseAnswer, Effect, SaveFlow, SaveOutcome};
use crate::saving::{SaveError, SaveJob, SaveTarget, Saved};
use crate::slide_canvas::{ScaleChanged, SlideCanvas};
use crate::video_player::StillsTaken;
use crate::slide_editor::{self, EditorEvent, SlideEditor};
use crate::table_tools::{
    InsertTable, TableColumnLeft, TableColumnRight, TableDeleteColumn, TableDeleteRow, TableRowAbove, TableRowBelow,
};
use crate::text_tools::TextTools;
use crate::start_screen::{self, StartEvent, StartScreen};
use crate::templates;
use crate::slide_strip::{
    self, STRIP_CONTEXT, STRIP_MAX_WIDTH, STRIP_MIN_WIDTH, STRIP_WIDTH, SlideStrip,
};
use crate::watcher::{self, Change};
use crate::working_copies::{KEEP_FAILED, KeepFailed, WorkingCopies};

pub use running::{BUTTON_TERMINAL_ENDED, BUTTON_TERMINAL_FULL, BUTTON_TERMINAL_REFUSED, BUTTON_TEXT_REFUSED, LiveStep};
pub(crate) use running::RunPressed;

mod editing;
mod inserting;
mod running;
mod saving;
mod starting;

pub use inserting::{DIALOG_FAILED, ONLY_PICTURES, ONLY_THE_FIRST, READING_RECORDING, StopsFraming, hold_recording_checks_for_tests, recording_checks_running, stop_framing_after};
pub use starting::{open_start_window, open_untitled_ids, refresh_start_screens};

pub const CANVAS_ID: &str = "canvas";
pub const TITLE_ID: &str = "title";
pub const NOTICES_ID: &str = "notices";
pub const STATUS_ID: &str = "status";
/// The slide's column: the slide on its desk, and the notes under it.
pub const SLIDE_COLUMN_ID: &str = "slide-column";

/// The narrowest the window can be: room for the slide list, the slide, the Markdown and the
/// inspector, each at its narrowest or more.
pub const MIN_WINDOW_WIDTH: Pixels = px(1024.);
/// The narrowest the slide's column is ever made by the panels beside it.
pub const SLIDE_COLUMN_MIN_WIDTH: Pixels = px(240.);
pub const PROBLEM_ID: &str = "problem";
pub const MARKDOWN_ID: &str = "markdown";
pub const FILE_MENU_ID: &str = "file-menu";
pub const EXPORT_MENU_ID: &str = "export-menu";
pub const OPEN_IN_STAGEPACK_ID: &str = "open-in-stagepack";
pub const BUILD_BAR_ID: &str = "build-bar";
pub const BUILD_ONCE_ID: &str = "build-once";
pub const BUILD_ALWAYS_ID: &str = "build-always";
pub const COMPONENT_PROBLEMS_ID: &str = "component-problems";
pub const OUTSIDE_CHANGE_ID: &str = "outside-change";
pub const KEEP_MINE_ID: &str = "keep-mine";
pub const LOAD_THEIRS_ID: &str = "load-theirs";
pub const SAVE_AS_FOLDER_ID: &str = "save-as-folder";
pub const SAVE_AS_PACKED_ID: &str = "save-as-packed";
pub const SAVE_AS_CANCEL_ID: &str = "save-as-cancel";
pub const CLOSE_SAVE_ID: &str = "close-save";
pub const CLOSE_DONT_SAVE_ID: &str = "close-dont-save";
pub const CLOSE_CANCEL_ID: &str = "close-cancel";
pub const RECOVERY_ID: &str = "recovery";
pub const RECOVER_ID: &str = "recover";
pub const DISCARD_RECOVERY_ID: &str = "discard-recovery";
/// The status line while a large file is copied in, saved or pasted: a bar, and how far it's got.
pub const PROGRESS_ID: &str = "progress";
/// The status line's Cancel, which stops what it shows the progress of.
pub const CANCEL_PROGRESS_ID: &str = "cancel-progress";
/// The answers to a paste that asks before copying large files into the talk.
pub const COPY_LARGE_ID: &str = "copy-large";
pub const DONT_COPY_LARGE_ID: &str = "dont-copy-large";

/// The key context of the window's content, where its shortcuts apply.
pub const KEY_CONTEXT: &str = "Workspace";

/// The key context of the kit's text fields.
const INPUT_CONTEXT: &str = "Input";

/// How the desktop tells this app's windows from another's.
pub(crate) const APP_ID: &str = "stagepack-studio";

/// How long the file has to stay quiet before it's read, as one save can be several writes.
const RELOAD_DELAY: Duration = Duration::from_millis(100);

/// How long "Saved" stays on the status line.
const STATUS_FADE: Duration = Duration::from_secs(5);

const MARKDOWN_WIDTH: Pixels = px(380.);
/// The Markdown panel gives way down to this before the slide does.
const MARKDOWN_MIN_WIDTH: Pixels = px(240.);
const MARKDOWN_MAX_WIDTH: Pixels = px(800.);

pub const SLIDEV_TALK: &str =
    "This is a Slidev talk made for Stagepack, so this app can't open it yet.";

/// What Export ▸ Slidev… says while the Slidev add-on isn't installed.
pub const SLIDEV_NOT_INSTALLED: &str = "Install the Slidev add-on from File ▸ Add-ons";
/// What the window says while a Slidev talk made for Stagepack is imported.
pub const IMPORTING_SLIDEV: &str = "Reading the Slidev talk…";
/// The button that opens the Add-ons dialog, beside a Slidev talk this app can't open yet.
pub const OFFER_ADD_ONS_ID: &str = "offer-add-ons";
/// Export ▸ Slidev….
pub const EXPORT_SLIDEV_ITEM_ID: &str = "export-slidev";
/// The report on what an import couldn't carry over.
pub const IMPORT_REPORT_ID: &str = "import-report";
/// The report on what an export to Slidev couldn't carry over as it was.
pub const EXPORT_REPORT_ID: &str = "export-report";

const PACKED_EXTENSION: &str = "sptx";

const BUILDING: &str = "Building components…";

pub const OUTSIDE_CHANGE: &str = "This talk was changed outside the app.";
const UNSAVED_ON_CLOSE: &str = "This talk has changes that haven't been saved.";

pub const RECOVERY_OFFER: &str = "Changes from last time weren't saved.";
pub const RECOVERY_CHANGED_SINCE: &str = "The talk has also changed since then.";

/// What the title shows before the talk's name while it has unsaved changes.
pub const UNSAVED_MARK: &str = "• ";

actions!(
    stagepack_studio,
    [
        NextSlide,
        PreviousSlide,
        ToggleMarkdown,
        NewPresentation,
        OpenFolder,
        OpenPacked,
        Save,
        SaveAs,
        SetUpForCodeEditor,
        ExportPowerPoint,
        ExportPdf,
        ExportSlidev,
        ShowAddOns,
        Present,
        Rehearse,
        NewSlide,
        DuplicateSlide,
        DeleteSlide,
        ToggleHidden,
        Undo,
        Redo,
        MoveSlideUp,
        MoveSlideDown,
        ToggleNotes,
        ToggleSlideList,
        ToggleInspector,
        CopyToClipboard,
        CutToClipboard,
        PasteFromClipboard,
    ]
);

/// Adds a slide with the layout called `layout`, by the engine's name for it, after the
/// current one.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = stagepack_studio, no_json)]
pub struct AddSlide {
    pub layout: &'static str,
}

/// Shows slide `index`.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = stagepack_studio, no_json)]
pub struct SelectSlide {
    pub index: usize,
}

/// Moves slide `from` so that it's slide `to` afterwards.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = stagepack_studio, no_json)]
pub struct MoveSlideTo {
    pub from: usize,
    pub to: usize,
}

/// How long a text field's caret blinks after the last key, click or focus, before it holds
/// still. Each blink draws the whole window, so a caret left blinking keeps an idle editor
/// busy. GTK stops after the same time.
pub const CARET_BLINKS_FOR: Duration = Duration::from_secs(10);

/// The window's settings and shortcuts, and a show's. Call once, after `gpui_kit::init`.
pub fn init(cx: &mut App) {
    gpui_kit::component::Theme::update(cx, |theme| {
        theme.cursor_blink = theme.cursor_blink.with_idle_timeout(CARET_BLINKS_FOR);
    });
    bind_keys(cx);
    // The drawer of 3D models lets go of its graphics device before the program ends.
    crate::model_view::close_on_quit(cx);
}

/// The window's shortcuts, and a show's. [`init`] binds them.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("right", NextSlide, Some(KEY_CONTEXT)),
        KeyBinding::new("pagedown", NextSlide, Some(KEY_CONTEXT)),
        KeyBinding::new("left", PreviousSlide, Some(KEY_CONTEXT)),
        KeyBinding::new("pageup", PreviousSlide, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-m", ToggleMarkdown, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-m", NewSlide, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-d", DuplicateSlide, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-z", Undo, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-z", Redo, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-y", Redo, Some(KEY_CONTEXT)),
        // A text field takes Ctrl+Z and Ctrl+Y for its own text. This is its Redo too, so that
        // it never reaches the window's.
        KeyBinding::new("ctrl-shift-z", input::Redo, Some(INPUT_CONTEXT)),
        KeyBinding::new("up", PreviousSlide, Some(STRIP_CONTEXT)),
        KeyBinding::new("down", NextSlide, Some(STRIP_CONTEXT)),
        // The last bound is the one menus show.
        KeyBinding::new("backspace", DeleteSlide, Some(STRIP_CONTEXT)),
        KeyBinding::new("delete", DeleteSlide, Some(STRIP_CONTEXT)),
        KeyBinding::new("ctrl-up", MoveSlideUp, Some(STRIP_CONTEXT)),
        KeyBinding::new("ctrl-down", MoveSlideDown, Some(STRIP_CONTEXT)),
        KeyBinding::new("ctrl-c", CopyToClipboard, Some(STRIP_CONTEXT)),
        KeyBinding::new("ctrl-x", CutToClipboard, Some(STRIP_CONTEXT)),
        KeyBinding::new("ctrl-v", PasteFromClipboard, Some(STRIP_CONTEXT)),
        KeyBinding::new("ctrl-n", NewPresentation, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-o", OpenPacked, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-o", OpenFolder, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-s", Save, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-s", SaveAs, Some(KEY_CONTEXT)),
        KeyBinding::new("f5", Present, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-f5", Rehearse, Some(KEY_CONTEXT)),
    ]);
    slide_strip::bind_keys(cx);
    slide_editor::bind_keys(cx);
    inspector::bind_keys(cx);
    icon_picker::bind_keys(cx);
    present_view::bind_keys(cx);
    start_screen::bind_keys(cx);
}

/// Opens the talk at `path`, a folder or a packed file, in a window of its own. Its unsaved
/// changes are kept safe in `recovery`, if there's one.
pub fn open_talk_window(path: PathBuf, recovery: Option<Recovery>, cx: &mut App) -> Result<()> {
    open_talk_window_with(path, recovery, BuildPermissions::app_folder(), cx)
}

/// [`open_talk_window`], with add-ons looked for in `app_folder`.
fn open_talk_window_with(path: PathBuf, recovery: Option<Recovery>, app_folder: Option<PathBuf>, cx: &mut App) -> Result<()> {
    let path = std::path::absolute(&path).unwrap_or(path);
    let options = WorkspaceView::window_options(cx);
    gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| {
            let mut view = WorkspaceView::open_with_addons(path, app_folder, window, cx);
            if let Some(recovery) = recovery {
                view.use_recovery(recovery, cx);
            }
            view
        })
    })?;
    cx.activate(true);
    Ok(())
}

/// What writing a file out has to say: a few notices, for the status line, or a report on what
/// couldn't be carried over, for a dialog of its own.
enum Written {
    Said(Vec<String>),
    Report(addon_abi::Report),
}

/// The files of the talks open in the app's windows, `this` window's included: a packed talk's
/// file, and a folder talk's Markdown.
fn open_talk_files(this: &WorkspaceView, cx: &Context<WorkspaceView>) -> Vec<PathBuf> {
    let file_of = |document: &Entity<Document>| match document.read(cx).place() {
        TalkPlace::Packed(file) => Some(file.clone()),
        TalkPlace::Folder(folder) => Some(folder.join(SLIDES_FILE)),
        TalkPlace::Untitled(_) => None,
    };
    let mut files: Vec<PathBuf> = this.document.as_ref().and_then(file_of).into_iter().collect();
    // The other windows' views: this one is being updated, so it is read as `this`.
    for view in starting::open_views(cx).into_iter().filter(|view| view.entity_id() != cx.entity_id()) {
        files.extend(view.read(cx).document.as_ref().and_then(file_of));
    }
    files
}

/// Whether `a` and `b` are the one file, a link to it included, as far as the system can tell.
fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if let (Ok(a), Ok(b)) = (std::fs::metadata(a), std::fs::metadata(b)) {
            return a.dev() == b.dev() && a.ino() == b.ino();
        }
    }
    matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// How the status line talks about a kind of writing.
struct Wording {
    doing: &'static str,
    done: &'static str,
}

const EXPORTING: Wording = Wording {
    doing: "Exporting",
    done: "Exported",
};
/// How far work that moves a large file has got, shared with the background task doing it.
#[derive(Default)]
pub(crate) struct Meter {
    done: std::sync::atomic::AtomicU64,
    total: std::sync::atomic::AtomicU64,
    stop: std::sync::atomic::AtomicBool,
}

impl Meter {
    /// Takes in how far it's got: `done` bytes of `total`.
    pub(crate) fn tell(&self, done: u64, total: u64) {
        use std::sync::atomic::Ordering::Relaxed;
        self.total.store(total, Relaxed);
        self.done.store(done, Relaxed);
    }

    /// What's set to stop it.
    pub(crate) fn stop(&self) -> &std::sync::atomic::AtomicBool {
        &self.stop
    }
}

/// Work that moves a large file, while the status line shows how far it's got.
struct Progressing {
    /// What it's doing, such as *Copying video*.
    doing: SharedString,
    /// Whether it's stopped when the window closes, as a copy into the talk is, which the talk
    /// closing has no use for. A save goes on.
    stops_on_close: bool,
    meter: std::sync::Arc<Meter>,
    /// Draws the line again while it goes on.
    _ticker: Task<()>,
}

/// What the status line says while the window waits for a save to end before it closes.
pub const CLOSING_AFTER_SAVE: &str = "Closing once the save is done; changes made now won't be kept.";

/// What the status line says of work asked to stop, until it has.
pub const STOPPING: &str = "Stopping…";

/// Background work, started on the window's thread.
type Start = Box<dyn FnOnce(&mut WorkspaceView, &mut Context<WorkspaceView>)>;

/// Background work a test holds back, to do things while it would be running.
struct HeldWork {
    saves: bool,
    copies: bool,
    held: Vec<Start>,
}

/// How often the status line is drawn again while it shows how far work has got.
const PROGRESS_TICK: Duration = Duration::from_millis(100);

/// A file this app can't show but an installed Stagepack can.
struct StagepackOffer {
    file: PathBuf,
    executable: PathBuf,
}

pub struct WorkspaceView {
    document: Option<Entity<Document>>,
    /// Why no talk is showing, when one was asked for.
    problem: Option<SharedString>,
    /// What the window shows while it has no talk, if it was opened to start from.
    start_screen: Option<Entity<StartScreen>>,
    /// Where working copies are kept, as the window was given it, for whatever is opened from
    /// it.
    recovery: Option<Recovery>,
    stagepack_offer: Option<StagepackOffer>,
    /// Whether the window shows a Slidev talk it can't open, which the Slidev add-on would.
    offers_add_ons: bool,
    /// The app's data folder, where add-ons are kept, if the window has one. Only the app's
    /// own windows look there, so a test's never touches the person's.
    app_folder: Option<PathBuf>,
    /// The Slidev add-on, as last looked at.
    addon_status: crate::addons::Status,
    slide_canvas: Entity<SlideCanvas>,
    /// The slide as it's typed on, over the canvas.
    slide_editor: Entity<SlideEditor>,
    notices: Vec<String>,
    /// What came of the last thing asked of the File or Export menu.
    status: Option<SharedString>,
    /// Work that moves a large file, while it goes on, the newest last.
    progressing: Vec<Progressing>,
    /// Videos and 3D models dropped, pasted or chosen, waiting to be copied in, one at a time in
    /// turn, and whether one is being copied now.
    large_waiting: std::collections::VecDeque<inserting::WaitingLarge>,
    large_copying: bool,
    /// Terminal recordings dropped, pasted or chosen, waiting to be checked before they're added,
    /// one at a time in turn, and whether one is being checked now.
    recordings_waiting: std::collections::VecDeque<inserting::WaitingRecording>,
    recording_checking: bool,
    /// What to say once the files waiting are all in, as that more files were dropped than go on.
    said_after_large: Option<&'static str>,
    /// Whether the window is to close once the copies it stopped as it was closed have ended.
    close_when_idle: bool,
    /// How many saves are running, which a window closing waits for before it lets anything of
    /// the working copy go: a save may be reading a video from it.
    saves_running: usize,
    /// Whether the window is to finish closing once nothing it waits for runs.
    finish_closing: bool,
    /// Whether the window has begun to close for good: its working copy is let go of, and it
    /// goes once that's done. It does so once.
    closing_for_good: bool,
    /// Where a test hears, in order, what a save and a close did.
    order_log: Option<crate::working_copies::OrderLog>,
    /// Whether it was asked to close again while copies it stopped hadn't ended, and closes
    /// without waiting for them.
    close_anyway: bool,
    /// For tests: which kinds of background work are held back, and those held.
    held_work: Option<HeldWork>,
    /// The wait before a status that only stays a while goes, while it's showing.
    status_fading: Option<Task<()>>,
    markdown_panel: Entity<MarkdownPanel>,
    show_markdown: bool,
    ribbon: Entity<Ribbon>,
    slide_strip: Entity<SlideStrip>,
    show_slide_list: bool,
    notes_panel: Entity<NotesPanel>,
    show_notes: bool,
    /// The form for the thing selected, at the right.
    inspector: Entity<Inspector>,
    /// Whether the form shows, while a talk is open.
    show_inspector: bool,
    /// How wide the slide list, the slide's column and the Markdown are.
    columns: Entity<ResizableState>,
    focus_handle: FocusHandle,
    /// Keeps the talk's folder watched for as long as the window is open.
    watcher: Option<watcher::TalkWatcher>,
    /// Whether the window was opened from a path, as the app opens one, and so follows its
    /// talk's folder wherever Save As puts it.
    opened_from_path: bool,
    /// The folder whose saves are followed, if any.
    followed: Option<PathBuf>,
    /// Whether following a folder starts a watch on it. A test's scheduler can't have a
    /// watcher's thread waking its tasks.
    really_watch: bool,
    /// Why saved changes won't show up, if the folder couldn't be watched.
    watch_problem: Option<String>,
    /// The reload waiting for the file to go quiet.
    pending_reload: Option<Task<()>>,
    /// The show of this talk, if one has been started.
    show: Option<WeakEntity<Presentation>>,
    /// Where the talk is kept, if it's kept in a folder. A packed talk is never built.
    talk_folder: Option<PathBuf>,
    /// What builds the talk's components, if this copy of the app has a compiler.
    compile: Option<Compile>,
    /// The compiler and toolkit that came with the app, for a code editor to be pointed at.
    bundle: Option<Bundle>,
    /// Where code editor projects are written, if the app has somewhere to keep them.
    editor_projects: Option<PathBuf>,
    /// Which folders are built whenever their components change, if the app has somewhere
    /// to keep that.
    permissions: Option<BuildPermissions>,
    /// The commands remembered as always allowed to run, in which folders, if the app has
    /// somewhere to keep that.
    run_permissions: Option<RunPermissions>,
    /// The commands this talk has been told yes or no to in this window. A talk opened in the
    /// window gets a new one.
    run_yes: SessionYes,
    /// The live terminals running in this window, for its show and its editor.
    live: LiveTerminals,
    /// What looks at them now and then.
    live_watch: Option<Task<()>>,
    /// The terminal *Start* asked about, to be started if the answer is yes.
    start_when_yes: Option<(usize, slides_engine::TerminalLayer)>,
    /// The button pressed last in the show that waits for a yes before it types.
    type_when_yes: Option<running::ButtonRun>,
    /// Whether this talk's folder is one of them.
    always_build: bool,
    /// Where an untitled talk is first saved, in place of the person's Documents folder.
    documents: Option<PathBuf>,
    /// Where Stagepack is, if anywhere, in place of where it's installed.
    stagepack: Option<Option<PathBuf>>,
    /// Whether a build is running.
    building: bool,
    /// Whether another build was asked for while one ran.
    build_again: bool,
    /// What the last build couldn't do.
    build_problems: Vec<Diagnostic>,
    /// The look at the components waiting for them to go quiet.
    pending_components: Option<Task<()>>,
    /// Whether a save is running, and what's to happen once it's done.
    save_flow: SaveFlow,
    /// The working copies of the talk's unsaved changes.
    working_copies: Entity<WorkingCopies>,
    /// The window this is the content of.
    window_handle: AnyWindowHandle,
    _scale_changed: Subscription,
    /// Ends the window's live terminals when the window is closed.
    _window_closed: Subscription,
    _document_changed: Option<Subscription>,
    _notes_edited: Subscription,
    _editor_events: Subscription,
    _working_copies_changed: Subscription,
    _keep_failed: Subscription,
    _icon_picked: Subscription,
    _list_focused: Subscription,
    _notes_focused: Subscription,
    /// A size chosen from the Insert tab's grid of tables.
    _table_picked: Subscription,
    /// The slide shows the steps things come in on while the Animate tab is open.
    _tab_changed: Subscription,
    _component_picked: Subscription,
    _start_asked: Option<Subscription>,
}

impl WorkspaceView {
    /// Opens the talk at `path`, a folder or a packed file. A folder's saves are followed, as
    /// nothing edits a packed file in place. If it can't be opened, the window says why, in the
    /// start screen's plain words.
    pub fn open(path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::open_with_addons(path, None, window, cx)
    }

    /// [`Self::open`], with add-ons looked for in `app_folder`: a Slidev talk made for Stagepack
    /// is read through the Slidev add-on if it's installed there.
    pub fn open_with_addons(path: PathBuf, app_folder: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        match read_talk(&path, Fonts::with_system_fallback()) {
            Ok(document) => {
                let mut view = Self::open_with(document, window, cx);
                view.set_app_folder(app_folder, cx);
                view.opened_from_path = true;
                if path.is_dir() {
                    view.follow_saves(path, cx);
                }
                view
            }
            Err(OpenProblem::SlidevTalk) => {
                let mut view = Self::for_slidev_talk(path.clone(), installed_stagepack(), window, cx);
                view.set_app_folder(app_folder, cx);
                if view.slidev_export_ready() {
                    view.problem = Some(IMPORTING_SLIDEV.into());
                    view.stagepack_offer = None;
                    view.import_slidev_talk(path, window, cx);
                }
                view
            }
            Err(OpenProblem::Other(_)) => {
                let mut view = Self::build(None, Some(starting::start_problem(&path).into()), window, cx);
                view.app_folder = app_folder;
                view
            }
        }
    }

    /// Shows a talk that is already open.
    pub fn open_with(document: Document, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(Some(document), None, window, cx)
    }

    /// A window on the new presentation `id`, brought back from `found`, its working copy from
    /// last time, taken with [`Recovery::take_untitled`], and numbered `number` among those
    /// open. It's kept in `recovery` from the start as this window's own, and last time's copy
    /// goes only once it is, so a crash before anything is edited loses nothing.
    pub fn bring_back_untitled(
        id: UntitledId,
        found: Found,
        number: usize,
        fonts: Fonts,
        recovery: Recovery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Self, String> {
        let mut document =
            Document::untitled_with(id, found.text.clone(), found.files.clone(), found.builds.clone(), fonts)?;
        document.set_untitled_number(number);
        let mut view = Self::build(Some(document), None, window, cx);
        view.recovery = Some(recovery.clone());
        view.working_copies
            .update(cx, |copies, cx| copies.use_recovery_bringing_back(recovery, found, cx));
        Ok(view)
    }

    /// Says that `file` is one of Stagepack's Slidev talks, and offers to open it in
    /// `stagepack`, if that's where Stagepack is.
    pub fn for_slidev_talk(
        file: PathBuf,
        stagepack: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut view = Self::build(None, Some(SLIDEV_TALK.into()), window, cx);
        view.stagepack_offer = stagepack.map(|executable| StagepackOffer { file, executable });
        view.offers_add_ons = true;
        view
    }

    /// The window with no talk, for headless tests.
    pub fn for_test(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(None, None, window, cx)
    }

    fn build(
        document: Option<Document>,
        problem: Option<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let slide_canvas = cx.new(|_| SlideCanvas::framed());
        let scale_changed = cx.subscribe(&slide_canvas, |this, _, _: &ScaleChanged, cx| {
            this.show_slide(cx)
        });
        let markdown_panel = cx.new(|cx| {
            let mut panel = MarkdownPanel::default();
            if let Some(document) = &document {
                panel.set_source(document.source(), cx);
            }
            panel
        });
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        let document = document.map(|document| cx.new(|_| document));
        // What holds the text tools, which can have the keyboard while typing goes on.
        let tools_focus = cx.focus_handle();
        let slide_editor = cx.new(|cx| {
            let canvas = slide_canvas.clone();
            let (away, tools) = (focus_handle.clone(), tools_focus.clone());
            SlideEditor::new(document.clone(), canvas, away, tools, window, cx)
        });
        let text_tools = cx.new(|cx| {
            TextTools::new(document.clone(), slide_editor.clone(), tools_focus, window, cx)
        });
        let editor_events =
            cx.subscribe_in(&slide_editor, window, |this, _, event: &EditorEvent, window, cx| match event {
                EditorEvent::Edited => this.edited(cx),
                EditorEvent::Said(said) => this.say(said.clone(), cx),
                EditorEvent::Unsaid(said) => this.unsay(said, cx),
                EditorEvent::StartTerminal => this.start_selected_terminal(window, cx),
            });
        let panels = Panels {
            notes: true,
            slide_list: true,
            markdown: false,
            inspector: true,
        };
        let clipboard_tools = cx.new(|cx| ClipboardTools::new(document.clone(), &slide_editor, cx));
        let icon_picker = cx.new(|cx| IconPicker::new(window, cx));
        // Typing in the picker's search leaves a text box waiting for its first character as it is.
        let search_focus = icon_picker.read(cx).search_focus(cx);
        slide_editor.update(cx, |editor, cx| editor.keep_typing_with(search_focus, window, cx));
        let icon_picked = cx.subscribe_in(&icon_picker, window, |this, _, picked: &IconPicked, window, cx| {
            this.insert_icon(picked.0, cx);
            this.keyboard_to_the_slide(window, cx);
        });
        let slide_focus = slide_editor.read(cx).focus_handle().clone();
        let ribbon = cx.new(|cx| {
            Ribbon::new(document.clone(), clipboard_tools.clone(), text_tools, icon_picker, slide_focus, panels, cx)
        });
        // Typing in an area goes on while the Animate tab's tools, which act on it, have the keyboard.
        let animate_focus = ribbon.read(cx).animate_focus().clone();
        slide_editor.update(cx, |editor, cx| editor.keep_typing_with(animate_focus, window, cx));
        let editor = slide_editor.clone();
        let tab_changed = cx.observe(&ribbon, move |_, ribbon, cx| {
            let animate = ribbon.read(cx).tab() == RibbonTab::Animate;
            editor.update(cx, |editor, cx| editor.show_steps(animate, cx));
        });
        let table_grid = ribbon.read(cx).table_grid().clone();
        let table_picked = cx.subscribe_in(&table_grid, window, |this, _, picked: &InsertTable, window, cx| {
            let InsertTable { columns, rows } = *picked;
            this.edit(|document| document.insert_table(columns, rows), cx);
            this.keyboard_to_the_slide(window, cx);
        });
        let component_list = ribbon.read(cx).component_list().clone();
        let component_picked = cx.subscribe_in(&component_list, window, |this, _, picked: &InsertComponent, window, cx| {
            this.edit(|document| document.insert_component(&picked.0), cx);
            this.keyboard_to_the_slide(window, cx);
        });
        let slide_strip = cx.new(|cx| SlideStrip::new(document.clone(), cx));
        // A thing is shown as selected only while the list of slides hasn't the keyboard, where
        // Delete and the clipboard act on whole slides: the list taking the keyboard lets go of
        // it (at once on a press, and by the keyboard as the window next draws), and a thing
        // selected while the list has it takes the keyboard for the slide.
        let list_focus = slide_strip.read(cx).focus_handle().clone();
        slide_editor.update(cx, |editor, _| editor.take_keyboard_from(list_focus.clone()));
        // Typing on the slide ends then too, as Escape ends it: no caret shows, and no new text
        // box waits, while the list has the keyboard.
        let editor = slide_editor.downgrade();
        slide_strip.update(cx, |strip, _| strip.end_typing_of(editor));
        let strip = slide_strip.clone();
        let list_focused = cx.on_focus_in(&list_focus, window, move |_, window, cx| {
            strip.update(cx, |strip, cx| strip.let_go_of_selection(window, cx));
        });
        let notes_panel = cx.new(|cx| NotesPanel::new(document.clone(), window, cx));
        let workspace = cx.weak_entity();
        let inspector = cx.new(|cx| Inspector::new(document.clone(), workspace, cx));
        let notes_focus = notes_panel.read(cx).field().focus_handle(cx);
        // The notes taking the keyboard let go of what's selected on the slide, and end typing
        // there, as the list of slides does; and nothing is selected while they have it.
        slide_editor.update(cx, |editor, _| editor.give_way_to_notes(notes_focus.clone()));
        let strip = slide_strip.clone();
        let notes_focused = cx.subscribe_in(&notes_panel, window, move |_, _, _: &NotesFocused, window, cx| {
            strip.update(cx, |strip, cx| strip.let_go_of_selection(window, cx));
        });
        clipboard_tools.update(cx, |tools, cx| tools.follow_notes(notes_focus, window, cx));
        let notes_edited = cx.subscribe_in(
            &notes_panel,
            window,
            |this, notes_panel, edited: &NotesEdited, window, cx| {
                let NotesEdited { index, text } = edited;
                let index = *index;
                this.edit(|document| document.set_notes(index, text), cx);
                // Notes that weren't taken as typed show as the talk has them.
                notes_panel.update(cx, |panel, cx| panel.follow_slide(window, cx));
            },
        );
        let working_copies = cx.new(|_| WorkingCopies::new(document.clone()));
        // The bar offering back last time's changes comes and goes.
        let working_copies_changed = cx.observe(&working_copies, |_, _, cx| cx.notify());
        let keep_failed = cx.subscribe(&working_copies, |this, _, _: &KeepFailed, cx| {
            this.say(KEEP_FAILED, cx)
        });
        // The title says whether there are unsaved changes, whatever made them, and the
        // working copy follows them.
        let document_changed = document.as_ref().map(|document| {
            cx.observe(document, |this, _, cx| {
                this.working_copies
                    .update(cx, |copies, cx| copies.talk_changed(cx));
                // A tag taken away, or a `run` changed, ends what ran.
                this.sync_live(cx);
                cx.notify();
            })
        });
        let view = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            view.update(cx, |view, cx| view.should_close(window, cx))
                .unwrap_or(true)
        });
        let notices = document
            .as_ref()
            .map(|document| document.update(cx, |document, _| document.notices()))
            .unwrap_or_default();
        let talk_folder = document
            .as_ref()
            .and_then(|document| talk_folder_of(document.read(cx).place()));
        let permissions = BuildPermissions::default_file().map(BuildPermissions::load);
        let run_permissions = RunPermissions::default_file().map(RunPermissions::load);
        let live = LiveTerminals::new();
        slide_editor.update(cx, |editor, cx| editor.set_live(live.clone(), cx));
        let window_closed = Self::end_live_with_window(window, live.clone(), cx);
        let mut view = Self {
            document,
            problem,
            start_screen: None,
            recovery: None,
            stagepack_offer: None,
            offers_add_ons: false,
            app_folder: None,
            addon_status: crate::addons::Status::NotInstalled,
            slide_canvas,
            slide_editor,
            notices,
            status: None,
            progressing: Vec::new(),
            large_waiting: std::collections::VecDeque::new(),
            large_copying: false,
            recordings_waiting: std::collections::VecDeque::new(),
            recording_checking: false,
            said_after_large: None,
            close_when_idle: false,
            saves_running: 0,
            finish_closing: false,
            closing_for_good: false,
            order_log: None,
            close_anyway: false,
            held_work: None,
            status_fading: None,
            markdown_panel,
            show_markdown: panels.markdown,
            ribbon,
            slide_strip,
            show_slide_list: panels.slide_list,
            notes_panel,
            show_notes: panels.notes,
            inspector,
            show_inspector: panels.inspector,
            columns: cx.new(|_| ResizableState::default()),
            focus_handle,
            watcher: None,
            opened_from_path: false,
            followed: None,
            really_watch: true,
            watch_problem: None,
            pending_reload: None,
            show: None,
            talk_folder,
            compile: builds::bundled_compiler(),
            bundle: builds::find_bundle(),
            editor_projects: cargo_file::default_projects_root(),
            permissions,
            run_permissions,
            run_yes: SessionYes::new(),
            live,
            live_watch: None,
            start_when_yes: None,
            type_when_yes: None,
            documents: None,
            stagepack: None,
            always_build: false,
            building: false,
            build_again: false,
            build_problems: Vec::new(),
            pending_components: None,
            save_flow: SaveFlow::default(),
            working_copies,
            window_handle: window.window_handle(),
            _scale_changed: scale_changed,
            _window_closed: window_closed,
            _document_changed: document_changed,
            _notes_edited: notes_edited,
            _editor_events: editor_events,
            _working_copies_changed: working_copies_changed,
            _keep_failed: keep_failed,
            _icon_picked: icon_picked,
            _list_focused: list_focused,
            _notes_focused: notes_focused,
            _table_picked: table_picked,
            _tab_changed: tab_changed,
            _component_picked: component_picked,
            _start_asked: None,
        };
        view.always_build = view.is_always_built();
        view.make_stills_aside(cx);
        view
    }

    /// Has the talk's videos' stills made, and its slide drawn again as each comes.
    fn make_stills_aside(&mut self, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        crate::video_player::make_stills(&document, cx);
        cx.subscribe(&document, |this, _, _: &StillsTaken, cx| this.show_slide(cx)).detach();
    }

    pub fn document(&self) -> Option<&Entity<Document>> {
        self.document.as_ref()
    }

    /// Builds with `compile` in place of the app's own compiler.
    pub fn set_compile_for_test(&mut self, compile: Compile) {
        self.compile = Some(compile);
    }

    /// Uses `bundle`, or none, in place of the one that came with the app.
    pub fn set_bundle_for_test(&mut self, bundle: Option<Bundle>) {
        self.bundle = bundle;
    }

    /// Writes code editor projects in `folder`, in place of the app's data folder.
    pub fn set_editor_projects_for_test(&mut self, folder: PathBuf) {
        self.editor_projects = Some(folder);
    }

    /// Keeps which folders are always built in `permissions`, in place of the app's own.
    pub fn set_permissions_for_test(&mut self, permissions: BuildPermissions) {
        self.permissions = Some(permissions);
        self.always_build = self.is_always_built();
    }

    /// Keeps which commands are always run in `permissions`, in place of the app's own.
    pub fn set_run_permissions_for_test(&mut self, permissions: RunPermissions) {
        self.run_permissions = Some(permissions);
    }

    /// What may be done about running `command`, the words of a talk's `run`, for this talk now.
    pub fn run_decision(&self, command: &[String], cx: &App) -> Decision {
        let Some(document) = &self.document else {
            return Decision::Refused;
        };
        let none = RunPermissions::load(PathBuf::new());
        let remembered = self.run_permissions.as_ref().unwrap_or(&none);
        may_run(document.read(cx).place(), command, &self.run_yes, remembered)
    }

    /// Asks whether `command` may run, if it hasn't been said yes or no to. The answer is kept
    /// for the session, and for *Always for this folder* in the app's own permissions. A command
    /// that can't be run is not asked about: the status line says why. This starts nothing: what
    /// starts a program asks [`Self::run_decision`] again. It says whether the question was put:
    /// while another dialog is showing it isn't, and nothing is recorded, so it can be asked again.
    pub fn ask_to_run(&mut self, command: Vec<String>, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(document) = &self.document else {
            return false;
        };
        let plan = match plan(document.read(cx).place(), &command) {
            Ok(plan) => plan,
            Err(why) => {
                self.say(why, cx);
                return false;
            }
        };
        if self.run_decision(&command, cx) != Decision::Ask {
            return false;
        }
        let question = RunQuestion {
            command: plan.shown_words(),
            program_in_talk: plan.program_in_talk(),
            program_by_path: plan.program_named_by_path(),
            folder: plan.folder().map(|folder| crate::live_terminal::spelled_plainly(folder.path())),
            typed: None,
        };
        let view = cx.weak_entity();
        ask_to_run(window, cx, question, move |answer, window, cx| {
            view.update(cx, |view, cx| view.run_answered(&plan, answer, window, cx)).ok();
        })
    }

    /// Takes in what was answered about running what `plan` says.
    fn run_answered(&mut self, plan: &Plan, answer: RunAnswer, window: &mut Window, cx: &mut Context<Self>) {
        let key = plan.key();
        match answer {
            RunAnswer::DontRun => self.run_yes.refuse(key),
            RunAnswer::Run => self.run_yes.allow(key),
            RunAnswer::Always => {
                self.run_yes.allow(key);
                let kept = match (plan.folder(), &mut self.run_permissions) {
                    (Some(folder), Some(permissions)) => permissions.remember(folder, &key.command).is_ok(),
                    _ => false,
                };
                // A permission that can't be written is still a yes for this session.
                if !kept {
                    self.say("The permission couldn't be kept, so it's only for now.".to_string(), cx);
                }
            }
        }
        // *Start* asked: a yes starts what it asked about, which looks at the yes again.
        if let Some((_, terminal)) = self.start_when_yes.take()
            && answer != RunAnswer::DontRun
            && let Some(document) = self.document.clone()
        {
            // The terminal as it is now: if the talk changed under the question, nothing starts.
            let (slide, now) = document.update(cx, |document, _| (document.current(), document.selected_terminal()));
            if let Some(now) = now.filter(|now| now.source == terminal.source && now.on.run == terminal.on.run) {
                self.start_live(slide, &now, true, cx);
            }
        }
        // A button asked: a yes goes on to its text.
        self.button_run_answered(window, cx);
        cx.notify();
    }

    /// Saves an untitled talk, and anything made from it, in `folder` first, in place of the
    /// person's Documents folder.
    pub fn set_documents_folder_for_test(&mut self, folder: PathBuf) {
        self.documents = Some(folder);
    }

    /// The folder anything saved from `document` starts out in: the one its talk sits in, or
    /// for an untitled talk the Documents folder.
    fn save_folder(&self, document: &Document) -> PathBuf {
        if let (TalkPlace::Untitled(_), Some(documents)) = (document.place(), &self.documents) {
            return documents.clone();
        }
        document.place().directory()
    }

    /// Looks for add-ons in `app_folder`, in place of the app's data folder.
    pub fn set_app_folder_for_test(&mut self, app_folder: PathBuf, cx: &mut Context<Self>) {
        self.set_app_folder(Some(app_folder), cx);
    }

    /// Takes Stagepack to be at `stagepack`, or nowhere, in place of where it's installed.
    pub fn set_stagepack_for_test(&mut self, stagepack: Option<PathBuf>) {
        self.stagepack = Some(stagepack);
    }

    /// Follows a folder's saves without watching it for real, so a test can see which folder
    /// is followed.
    pub fn set_no_watching_for_test(&mut self) {
        self.really_watch = false;
    }

    /// Keeps working copies in `recovery`, in place of the app's data folder.
    pub fn set_recovery_for_test(&mut self, recovery: Recovery, cx: &mut Context<Self>) {
        self.use_recovery(recovery, cx);
    }

    fn is_always_built(&self) -> bool {
        let permissions = self.permissions.as_ref();
        let folder = self.talk_folder.as_deref();
        folder.is_some_and(|folder| permissions.is_some_and(|p| p.always(folder)))
    }

    pub fn slide_canvas(&self) -> &Entity<SlideCanvas> {
        &self.slide_canvas
    }

    pub fn slide_editor(&self) -> &Entity<SlideEditor> {
        &self.slide_editor
    }

    pub fn ribbon(&self) -> &Entity<Ribbon> {
        &self.ribbon
    }

    pub fn slide_strip(&self) -> &Entity<SlideStrip> {
        &self.slide_strip
    }

    pub fn notes_panel(&self) -> &Entity<NotesPanel> {
        &self.notes_panel
    }

    pub fn inspector(&self) -> &Entity<Inspector> {
        &self.inspector
    }

    /// Whether the talk's folder is being watched for saves.
    pub fn follows_saves(&self) -> bool {
        self.followed.is_some()
    }

    /// The folder whose saves are followed, if any.
    pub fn followed_folder(&self) -> Option<&Path> {
        self.followed.as_deref()
    }

    /// Window options for a window whose content is a `WorkspaceView`.
    pub fn window_options(cx: &App) -> WindowOptions {
        WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1280.), px(800.)), cx)),
            titlebar: Some(TitlebarOptions {
                title: Some("Stagepack Studio".into()),
                ..TitleBar::title_bar_options()
            }),
            window_min_size: Some(size(MIN_WINDOW_WIDTH, px(500.))),
            app_id: Some(APP_ID.into()),
            ..TitleBar::window_options()
        }
    }

    /// Brings the canvas and the notices up to date with the current slide. Before the
    /// canvas has measured itself there's no scale to draw at, and its first paint brings
    /// us back here.
    fn show_slide(&mut self, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let scale = self.slide_canvas.read(cx).scale();
        let (notices, layers, message) = document.update(cx, |document, _| {
            let layers = scale.map(|scale| document.editor_layers(scale));
            (document.notices(), layers, document.take_message())
        });
        if let Some(message) = message {
            self.say(message, cx);
        }
        self.notices = notices;
        if let Some(layers) = layers {
            let (videos, models, terminals) = document.update(cx, |document, _| (document.videos_here(), document.models_here(), document.terminals_here()));
            self.slide_canvas.update(cx, |canvas, cx| {
                canvas.set_layers(layers, cx);
                canvas.place_videos(&videos, cx);
                canvas.place_models(&models, cx);
                canvas.place_recordings(&terminals, cx);
            });
        }
        cx.notify();
    }

    /// Watches `folder`, and reloads the talk from its file whenever that changes.
    pub fn follow_saves(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        if !self.really_watch {
            self.followed = Some(folder);
            return;
        }
        let (changed, mut changes) = mpsc::unbounded();
        let watcher = watcher::watch(&folder, move |change| {
            changed.unbounded_send(change).ok();
        });
        match watcher {
            Ok(watcher) => {
                self.watcher = Some(watcher);
                self.followed = Some(folder.clone());
            }
            Err(error) => {
                self.watch_problem = Some(format!(
                    "Saved changes won't show here: could not watch {}: {error}",
                    folder.display()
                ));
                cx.notify();
                return;
            }
        }
        let file = folder.join(SLIDES_FILE);
        cx.spawn(async move |this, cx| {
            while let Some(change) = changes.next().await {
                let file = file.clone();
                let followed = this.update(cx, |this, cx| match change {
                    Change::Slides => this.reload_soon(file, cx),
                    Change::Component => this.components_changed_soon(cx),
                });
                if followed.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Reloads the talk from `file` once it has been quiet for a moment.
    pub fn reload_soon(&mut self, file: PathBuf, cx: &mut Context<Self>) {
        // Replacing the task cancels the one waiting, so the wait starts again from now.
        self.pending_reload = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RELOAD_DELAY).await;
            let read = file.clone();
            let contents = cx
                .background_spawn(async move { slides_engine::read_talk_text(&read).ok() })
                .await;
            this.update(cx, |this, cx| {
                // A file the talk was saved away from says nothing about it now.
                let talks_file = this.talk_folder.as_ref().map(|folder| folder.join(SLIDES_FILE));
                if talks_file.as_ref() == Some(&file) {
                    this.reload(contents, cx);
                }
            })
            .ok();
        }));
    }

    /// Takes `contents` as a read of the talk's file that has just come in.
    pub fn reload_for_test(&mut self, contents: Option<String>, cx: &mut Context<Self>) {
        self.reload(contents, cx);
    }

    fn reload(&mut self, contents: Option<String>, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        // A read that comes in while a save runs is held back, and the file read again after.
        if !self.save_flow.read_arrived().contains(&Effect::TakeRead) {
            return;
        }
        let reload = document.update(cx, |document, cx| {
            let reload = document.reload(contents);
            if matches!(reload, Reload::Changed(_)) {
                // A show of the talk has to know.
                cx.notify();
            }
            reload
        });
        if matches!(reload, Reload::Unreadable) {
            self.say(UNREADABLE_FILE, cx);
            return;
        }
        let Reload::Changed(slides) = reload else {
            // The bar about a change from outside may have come or gone.
            cx.notify();
            return;
        };
        let (showing_changed, source) = {
            let document = document.read(cx);
            (
                slides.contains(&document.current()),
                document.source().to_string(),
            )
        };
        self.markdown_panel
            .update(cx, |panel, cx| panel.set_source(&source, cx));
        if showing_changed {
            self.show_slide(cx);
        } else {
            // The title counts the slides, and the talk may have gained some.
            cx.notify();
        }
    }

    /// Looks at the talk's components again once they have been quiet for a moment, and
    /// builds them if the folder is always built. A change to an asset comes this way too.
    pub fn components_changed_soon(&mut self, cx: &mut Context<Self>) {
        // Replacing the task cancels the one waiting, so the wait starts again from now.
        self.pending_components = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RELOAD_DELAY).await;
            this.update(cx, |this, cx| {
                this.refresh_components(cx);
                if this.always_build {
                    this.build_components(cx);
                }
            })
            .ok();
        }));
    }

    /// Shows the talk's components as they are now on disk.
    fn refresh_components(&mut self, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let reload = document.update(cx, |document, cx| {
            let reload = document.reload_components();
            if matches!(reload, Reload::Changed(_)) {
                // A show of the talk has to know.
                cx.notify();
            }
            reload
        });
        // A component that's built now has nothing left to say about its last build.
        let document = document.read(cx);
        let built: Vec<String> = document
            .components()
            .iter()
            .filter(|info| info.state == ComponentState::UpToDate)
            .map(|info| info.file.clone())
            .collect();
        self.build_problems
            .retain(|problem| !built.contains(&problem.file));
        let current = document.current();
        if let Reload::Changed(slides) = reload
            && slides.contains(&current)
        {
            self.show_slide(cx);
        }
        cx.notify();
    }

    /// Builds the talk's components that need it, on a background task, unless the talk is
    /// packed or there's no compiler. A build asked for while one runs follows it.
    fn build_components(&mut self, cx: &mut Context<Self>) {
        let (Some(folder), Some(compile)) = (self.talk_folder.clone(), self.compile.clone()) else {
            return;
        };
        if self.building {
            self.build_again = true;
            return;
        }
        self.building = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_spawn(async move { builds::build_components(&folder, &compile) })
                .await;
            this.update(cx, |this, cx| this.finish_build(outcome, cx))
                .ok();
        })
        .detach();
    }

    fn finish_build(&mut self, outcome: BuildOutcome, cx: &mut Context<Self>) {
        self.building = false;
        self.build_problems = outcome.problems;
        self.refresh_components(cx);
        if std::mem::take(&mut self.build_again) {
            self.build_components(cx);
        }
    }

    /// Remembers that this folder is always built, and builds it.
    fn build_always(&mut self, cx: &mut Context<Self>) {
        let Some(folder) = self.talk_folder.clone() else {
            return;
        };
        let remembered = match &mut self.permissions {
            Some(permissions) => permissions
                .set_always(&folder)
                .map_err(|error| format!("This folder couldn't be remembered: {error}")),
            None => Err("This computer has no place for the app to remember this folder.".into()),
        };
        match remembered {
            Ok(()) => self.always_build = true,
            Err(problem) => self.say(
                format!("{problem} Its components are built just this once."),
                cx,
            ),
        }
        self.build_components(cx);
    }

    /// Starts a show of the talk, unless one is running already.
    fn present(&mut self, rehearsing: bool, cx: &mut Context<Self>) {
        self.settle_typing(cx);
        let Some(document) = self.document.clone() else {
            return;
        };
        let running = self
            .show
            .as_ref()
            .and_then(WeakEntity::upgrade)
            .is_some_and(|show| !show.read(cx).has_ended());
        if running {
            return;
        }
        // The show plays the talk's videos and turns its models: none does here meanwhile.
        self.slide_editor.update(cx, |editor, cx| {
            editor.stop_video(cx);
            editor.stop_recording(cx);
            editor.stop_turning(cx);
        });
        let this = cx.entity().downgrade();
        let owner = (this.clone(), self.live.clone());
        // Once this window is free again, as the show asks it which screen it's on.
        cx.defer(move |cx| {
            let show = present_view::start_show_for(document, rehearsing, Some(owner), cx);
            this.update(cx, |this, cx| {
                if show.read(cx).has_ended() {
                    this.say("Could not open a window for the show.", cx);
                }
                this.show = Some(show.downgrade());
            })
            .ok();
        });
    }

    /// Puts `status` on the status line, for the show.
    pub(crate) fn tell(&mut self, status: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.say(status, cx);
    }

    /// Puts `status` on the status line.
    fn say(&mut self, status: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status = Some(status.into());
        self.status_fading = None;
        cx.notify();
    }

    /// Takes `status` off the status line, if it's still what the line says.
    fn unsay(&mut self, status: &SharedString, cx: &mut Context<Self>) {
        if self.status.as_ref() == Some(status) {
            self.status = None;
            self.status_fading = None;
            cx.notify();
        }
    }

    /// Shows how far work that moves a large file has got on the status line, `doing` it, with a
    /// Cancel, until [`Self::end_progress`]. What it gives is told how far it's got, and says when
    /// to stop. Work that `stops_on_close` is stopped when the window closes, which waits for it
    /// to end. While more than one runs, the newest is shown, saying how many there are.
    pub(crate) fn start_progress(&mut self, doing: impl Into<SharedString>, stops_on_close: bool, cx: &mut Context<Self>) -> std::sync::Arc<Meter> {
        let meter = std::sync::Arc::new(Meter::default());
        let ticking = meter.clone();
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PROGRESS_TICK).await;
                let going = this.update(cx, |this, cx| {
                    cx.notify();
                    this.progressing.iter().any(|progressing| std::sync::Arc::ptr_eq(&progressing.meter, &ticking))
                });
                if !matches!(going, Ok(true)) {
                    break;
                }
            }
        });
        self.progressing.push(Progressing { doing: doing.into(), stops_on_close, meter: meter.clone(), _ticker: ticker });
        cx.notify();
        meter
    }

    /// The work `meter` is told of is done: the status line no longer shows it. If the window was
    /// closed while it ran, and nothing it stopped runs any more, it closes now.
    pub(crate) fn end_progress(&mut self, meter: &std::sync::Arc<Meter>, cx: &mut Context<Self>) {
        self.progressing.retain(|progressing| !std::sync::Arc::ptr_eq(&progressing.meter, meter));
        cx.notify();
        self.closing_may_go_on(cx);
        if self.close_when_idle && !self.stops_running() {
            self.close_when_idle = false;
            let (window, view) = (self.window_handle, cx.weak_entity());
            cx.defer(move |cx| {
                window
                    .update(cx, |_, window, cx| {
                        let closes = view.update(cx, |view, cx| view.should_close(window, cx)).unwrap_or(true);
                        if closes {
                            window.remove_window();
                        }
                    })
                    .ok();
            });
        }
    }

    /// The work the status line shows: the newest not yet stopped, whose Cancel stops it, or else
    /// the newest, while it stops.
    fn progress_shown(&self) -> Option<&Progressing> {
        let stopped = |progressing: &&Progressing| progressing.meter.stop.load(std::sync::atomic::Ordering::SeqCst);
        self.progressing.iter().rev().find(|progressing| !stopped(progressing)).or(self.progressing.last())
    }

    /// Holds back saves, if `saves`, and copies into the talk, if `copies`, as they're started,
    /// until [`Self::release_held_work_for_test`]: they count as running meanwhile. For tests,
    /// whose background work otherwise runs to its end at once.
    #[doc(hidden)]
    pub fn hold_work_for_test(&mut self, saves: bool, copies: bool) {
        self.held_work = Some(HeldWork { saves, copies, held: Vec::new() });
    }

    /// Starts the work held back, and holds back no more.
    #[doc(hidden)]
    pub fn release_held_work_for_test(&mut self, cx: &mut Context<Self>) {
        for start in self.held_work.take().map(|held| held.held).unwrap_or_default() {
            start(self, cx);
        }
    }

    /// Starts `start`, background work that's a save if `save` and else a copy into the talk,
    /// unless a test holds that kind back.
    pub(crate) fn start_work(&mut self, save: bool, start: impl FnOnce(&mut Self, &mut Context<Self>) + 'static, cx: &mut Context<Self>) {
        match &mut self.held_work {
            Some(held) if (save && held.saves) || (!save && held.copies) => held.held.push(Box::new(start)),
            _ => start(self, cx),
        }
    }

    /// Whether the window closing has to wait: for a save running, which may be reading what the
    /// working copy holds, and for copies it stopped, unless it was asked to close anyway.
    pub(super) fn close_waits(&self) -> bool {
        self.save_waits() || (self.stops_running() && !self.close_anyway)
    }

    /// Whether a save is running, or not yet taken in: the window closing waits for the talk to
    /// be marked saved, as the order of a save is the write, then the talk marked saved, then the
    /// working copy let go.
    pub(super) fn save_waits(&self) -> bool {
        self.saves_running > 0 || self.save_flow.is_saving()
    }

    /// Whether the window is closing: waiting to, or going. No save starts then.
    pub(super) fn is_closing_window(&self) -> bool {
        self.finish_closing || self.closing_for_good
    }

    /// Tells `log`, in order, when a save's write is done, the talk is marked saved, the working
    /// copy is cleared as the window closes, and the window has closed. For tests.
    #[doc(hidden)]
    pub fn log_order_for_test(&mut self, log: crate::working_copies::OrderLog, cx: &mut Context<Self>) {
        self.order_log = Some(log.clone());
        self.working_copies.update(cx, |copies, _| copies.log_order_for_test(log));
    }

    /// Tells the test listening, if one is, that `what` has happened.
    pub(super) fn log_order(&self, what: &'static str) {
        if let Some(log) = &self.order_log {
            log.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(what);
        }
    }

    /// Finishes closing the window, if it's waiting to and nothing it waits for runs any more.
    pub(super) fn closing_may_go_on(&mut self, cx: &mut Context<Self>) {
        if self.finish_closing && !self.close_waits() {
            self.finish_closing = false;
            self.close_after_working_copies(cx);
        }
    }

    /// Whether any work runs that the window closing stops.
    fn stops_running(&self) -> bool {
        self.progressing.iter().any(|progressing| progressing.stops_on_close)
    }

    /// Stops every copy running, as the window is closing, and closes it once they've ended: they
    /// copy into a working copy the close may be about to clear. Whether any was running.
    pub(super) fn stop_before_closing(&mut self) -> bool {
        if !self.stops_running() {
            return false;
        }
        if self.close_when_idle || self.finish_closing {
            // Asked again while they stop: a copy blocked on a disk that went away may never get
            // to its next stop. The window closes without them; what they've written is in files
            // under names of their own beside where they'd go, which a copy that ends takes away,
            // and the next start's sweep takes away with the slot otherwise.
            self.close_anyway = true;
            self.close_when_idle = false;
            return false;
        }
        for progressing in self.progressing.iter().filter(|progressing| progressing.stops_on_close) {
            progressing.meter.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        self.close_when_idle = true;
        true
    }

    /// The status line while work that moves a large file goes on: what it's doing, how far it's
    /// got as a bar and in words, and a Cancel.
    fn progress_line(&self, progressing: &Progressing, cx: &Context<Self>) -> impl IntoElement + use<> {
        use std::sync::atomic::Ordering::Relaxed;
        let (done, total) = (progressing.meter.done.load(Relaxed), progressing.meter.total.load(Relaxed));
        let stopping = progressing.meter.stop.load(std::sync::atomic::Ordering::SeqCst);
        let mut words = if stopping { STOPPING.to_string() } else { crate::large_files::progress_words(&progressing.doing, done, total) };
        if self.finish_closing && self.save_waits() {
            words = format!("{CLOSING_AFTER_SAVE} {words}");
        }
        if self.progressing.len() > 1 {
            words = format!("{words} · {} running", self.progressing.len());
        }
        let words: SharedString = words.into();
        let fraction = if total == 0 { 0.0 } else { (done as f64 / total as f64).clamp(0.0, 1.0) as f32 };
        let meter = progressing.meter.clone();
        h_flex()
            .id(PROGRESS_ID)
            .test_support()
            .aria_label(words.clone())
            .w_full()
            .gap_2()
            .px_2()
            .items_center()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(
                div()
                    .w(px(160.))
                    .h(px(6.))
                    .rounded_full()
                    .bg(cx.theme().secondary)
                    .child(div().h_full().rounded_full().bg(cx.theme().primary).w(relative(fraction))),
            )
            .child(div().truncate().child(words))
            .child(
                Button::new(CANCEL_PROGRESS_ID)
                    .xsmall()
                    .ghost()
                    .label("Cancel")
                    .on_click(move |_, _, _| meter.stop.store(true, std::sync::atomic::Ordering::SeqCst)),
            )
    }

    /// Puts `status` on the status line until [`STATUS_FADE`] has passed, or the next edit.
    fn say_for_a_while(&mut self, status: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.say(status, cx);
        self.status_fading = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(STATUS_FADE).await;
            this.update(cx, |this, cx| this.clear_fading_status(cx))
                .ok();
        }));
    }

    /// Takes away a status that was only to stay a while.
    fn clear_fading_status(&mut self, cx: &mut Context<Self>) {
        if self.status_fading.take().is_some() {
            self.status = None;
            cx.notify();
        }
    }

    /// Asks which talk to open, and opens it: in this window from the start screen, and in one
    /// of its own from a window with a talk.
    fn ask_to_open(&mut self, directories: bool, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: !directories,
            directories,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let path = match chosen.await {
                Ok(Ok(Some(mut paths))) if !paths.is_empty() => paths.remove(0),
                Ok(Err(error)) => {
                    let problem = format!("Could not show the Open dialog: {error}");
                    this.update(cx, |this, cx| this.say_on_start(problem, false, cx)).ok();
                    return;
                }
                _ => return,
            };
            this.update_in(cx, |this, window, cx| {
                if this.shows_start_screen() {
                    this.open_here(path, window, cx);
                } else if let Err(error) = open_talk_window_with(path, this.recovery.clone(), this.app_folder.clone(), cx) {
                    this.say(format!("Could not open a window for that talk: {error}"), cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Asks where to put what `job` will write, then runs it on a background task and says how
    /// it went. `job` is made once the place is chosen, so it has the talk as it is by then,
    /// with typing done since settled too. A place that is the file of a talk open in a window,
    /// this one's included, is refused: writing there would take the talk away.
    fn write_out<Job>(
        &mut self,
        suggest: impl FnOnce(&str) -> String,
        wording: Wording,
        job: impl FnOnce(&Document) -> Job + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) where
        Job: FnOnce(&Path) -> Result<Written, String> + Send + 'static,
    {
        self.settle_typing(cx);
        let Some(document) = &self.document else {
            return;
        };
        let document = document.read(cx);
        let suggested = suggest(&document.name());
        let chosen = cx.prompt_for_new_path(&self.save_folder(document), Some(&suggested));
        cx.spawn_in(window, async move |this, cx| {
            let out = match chosen.await {
                Ok(Ok(Some(out))) => out,
                Ok(Err(error)) => {
                    let problem = format!("Could not show the Save dialog: {error}");
                    this.update(cx, |this, cx| this.say(problem, cx)).ok();
                    return;
                }
                _ => return,
            };
            let name = out.file_name().map_or_else(
                || out.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
            let job = this.update(cx, |this, cx| {
                if open_talk_files(this, cx).iter().any(|open| same_file(open, &out)) {
                    this.say(format!("{name} is a talk open in a window, so it wasn't written over."), cx);
                    return None;
                }
                // Typing may have gone on while the dialog was up.
                this.settle_typing(cx);
                this.say(format!("{} {name}…", wording.doing), cx);
                this.document
                    .as_ref()
                    .map(|document| job(document.read(cx)))
            });
            let Ok(Some(job)) = job else {
                return;
            };
            let outcome = cx.background_spawn(async move { job(&out) }).await;
            this.update_in(cx, |this, window, cx| {
                let said = match outcome {
                    Ok(Written::Said(notices)) if notices.is_empty() => format!("{} {name}.", wording.done),
                    Ok(Written::Said(notices)) => format!("{} {name}. {}", wording.done, notices.join(" · ")),
                    Ok(Written::Report(report)) if report.notices.is_empty() => format!("{} {name}.", wording.done),
                    Ok(Written::Report(report)) => {
                        let count = report.notices.len();
                        starting::show_report(EXPORT_REPORT_ID, "Exported to Slidev", "What couldn't be carried over as it was:", report, window, cx);
                        if count == 1 {
                            format!("{} {name}, with 1 note.", wording.done)
                        } else {
                            format!("{} {name}, with {count} notes.", wording.done)
                        }
                    }
                    Err(problem) => problem,
                };
                this.say(said, cx);
            })
            .ok();
        })
        .detach();
    }

    fn export(&mut self, kind: ExportKind, window: &mut Window, cx: &mut Context<Self>) {
        self.write_out(
            move |name| format!("{}.{}", export::file_stem_for(name), kind.extension()),
            EXPORTING,
            move |document| {
                let (input, readers) = (document.export_input(), document.readers());
                // After a save replacing the pack that's running, and keeping one from replacing
                // it until it's done.
                move |out: &Path| export::run_export_reading_with(input, kind, out, |make| readers.while_reading(make)).map(Written::Said)
            },
            window,
            cx,
        );
    }

    /// Writes a Cargo project for the talk's components, in the app's data folder, which only
    /// this asks for.
    fn set_up_for_code_editor(&mut self, cx: &mut Context<Self>) {
        let (Some(folder), Some(bundle)) = (&self.talk_folder, &self.bundle) else {
            return;
        };
        let Some(projects) = self.editor_projects.clone() else {
            self.say("This system has no data folder to keep a code editor project in.".to_string(), cx);
            return;
        };
        let said = match write_cargo_file(folder, bundle, &projects) {
            Ok(project) => cargo_file::written(&project),
            Err(problem) => problem,
        };
        self.say(said, cx);
    }

    /// Looks for add-ons in `app_folder` from now on, and at what's there now.
    pub(crate) fn set_app_folder(&mut self, app_folder: Option<PathBuf>, cx: &mut Context<Self>) {
        self.app_folder = app_folder;
        self.look_at_addons(cx);
    }

    /// Looks again at whether the Slidev add-on is installed, and draws what that changes.
    fn look_at_addons(&mut self, cx: &mut Context<Self>) {
        self.addon_status = self.app_folder.as_deref().map_or(crate::addons::Status::NotInstalled, crate::addons::status);
        cx.notify();
    }

    /// Whether the Slidev add-on is installed, as last looked at, and so Export ▸ Slidev… is on.
    pub fn slidev_export_ready(&self) -> bool {
        matches!(self.addon_status, crate::addons::Status::Installed(_))
    }

    /// Opens the Add-ons dialog.
    fn show_add_ons(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        crate::addons::dialog::open(self.app_folder.clone(), window, cx);
    }

    /// Exports the talk to Slidev through the Slidev add-on, as a file Stagepack opens. Without
    /// the add-on, it says where to install it.
    fn export_slidev(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(app_folder) = self.app_folder.clone().filter(|_| self.slidev_export_ready()) else {
            self.say(SLIDEV_NOT_INSTALLED, cx);
            return;
        };
        self.write_out(
            export::slidev_file_name,
            EXPORTING,
            move |document| {
                let (input, readers) = (document.export_input(), document.readers());
                move |out: &Path| {
                    // Loaded here, off the window's thread, and checked again as it is; and held
                    // in use until the export is done, so that no update swaps it meanwhile.
                    let (_in_use, loaded) = crate::addons::load_in_use(&app_folder);
                    let addon = match loaded {
                        Some(crate::addons::Installed { state: crate::addons::InstalledState::Ready(addon), .. }) => addon,
                        Some(crate::addons::Installed { state: crate::addons::InstalledState::Damaged(sentence), .. }) => {
                            return Err(format!("The Slidev add-on can't be used: {sentence}"));
                        }
                        None => return Err(format!("{SLIDEV_NOT_INSTALLED}.")),
                    };
                    let id = uuid::Uuid::new_v4().to_string();
                    export::export_slidev_reading_with(input, &addon, out, chrono::Utc::now(), &id, |make| readers.while_reading(make))
                        .map(Written::Report)
                }
            },
            window,
            cx,
        );
    }

    fn open_in_stagepack(&mut self, cx: &mut Context<Self>) {
        let Some(offer) = &self.stagepack_offer else {
            return;
        };
        if let Err(error) = export::open_in_stagepack(&offer.executable, &offer.file) {
            self.say(format!("Could not start Stagepack: {error}"), cx);
        }
    }
}

/// Reads the talk at `path`, a folder or a packed file.
fn read_talk(path: &Path, fonts: Fonts) -> Result<Document, OpenProblem> {
    if path.is_dir() {
        Document::open(path, fonts).map_err(OpenProblem::Other)
    } else {
        Document::open_packed(path, fonts)
    }
}

/// Has every window look again at whether the Slidev add-on is installed, as it has just been
/// installed or taken away.
pub fn refresh_addon_status(cx: &mut App) {
    for view in starting::open_views(cx) {
        view.update(cx, |view, cx| view.look_at_addons(cx));
    }
}

/// Where Stagepack is installed, if it is.
fn installed_stagepack() -> Option<PathBuf> {
    stagepack_executable(|name| std::env::var(name).ok(), Path::is_file)
}

/// The folder of the talk at `place`, if it's kept in one: the folder that's watched and built.
fn talk_folder_of(place: &TalkPlace) -> Option<PathBuf> {
    match place {
        TalkPlace::Folder(folder) => Some(folder.clone()),
        TalkPlace::Packed(_) | TalkPlace::Untitled(_) => None,
    }
}

/// The File menu. `packed` says which form the open talk is in, and is `None` with no talk.
/// `for_code_editor` says whether it offers to set a folder talk up for a code editor.
fn file_menu(packed: Option<bool>, for_code_editor: bool) -> impl IntoElement {
    Button::new(FILE_MENU_ID)
        .ghost()
        .small()
        .label("File")
        .dropdown_menu(move |menu, _, _| {
            let no_talk = packed.is_none();
            // The system's Open dialog picks files or folders, never both at once.
            menu.menu("New presentation…", Box::new(NewPresentation))
                .separator()
                .menu("Open talk folder…", Box::new(OpenFolder))
                .menu("Open talk file…", Box::new(OpenPacked))
                .separator()
                .menu_with_disabled("Save", Box::new(Save), no_talk)
                .menu_with_disabled("Save As…", Box::new(SaveAs), no_talk)
                .separator()
                .menu("Add-ons…", Box::new(ShowAddOns))
                .when(for_code_editor && packed == Some(false), |menu| {
                    menu.separator()
                        .menu("Set up for a code editor", Box::new(SetUpForCodeEditor))
                })
        })
}

/// The Export menu. `slidev_ready` says whether the Slidev add-on is installed, without which
/// Export ▸ Slidev… is off, and says why.
fn export_menu(has_talk: bool, slidev_ready: bool) -> impl IntoElement {
    Button::new(EXPORT_MENU_ID)
        .ghost()
        .small()
        .label("Export")
        .dropdown_menu(move |menu, _, _| {
            menu.menu_with_disabled("PowerPoint…", Box::new(ExportPowerPoint), !has_talk)
                .menu_with_disabled("PDF…", Box::new(ExportPdf), !has_talk)
                .menu_element_with_disabled(Box::new(ExportSlidev), !(has_talk && slidev_ready), move |_, _| {
                    let words: SharedString = if slidev_ready {
                        "Slidev…".into()
                    } else {
                        format!("Slidev… ({SLIDEV_NOT_INSTALLED})").into()
                    };
                    div()
                        .id(EXPORT_SLIDEV_ITEM_ID)
                        .test_support()
                        .aria_label(words)
                        .child("Slidev…")
                        .when(!slidev_ready, |item| {
                            item.tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new(SLIDEV_NOT_INSTALLED).build(window, cx))
                        })
                })
        })
}

/// A footer line with nothing on it, as tall as one with something.
fn blank_footer_line() -> impl IntoElement {
    div().w_full().px_2().py_1().text_xs().child("\u{a0}")
}

/// One line of small print under the slide.
fn footer_line(id: &'static str, line: SharedString, color: Hsla) -> impl IntoElement {
    div()
        .id(id)
        .test_support()
        .aria_label(line.clone())
        .w_full()
        .px_2()
        .py_1()
        .text_xs()
        .text_color(color)
        .truncate()
        .child(line)
}

/// How a problem with a component reads: its file, its line if it has one, and what's wrong.
/// The most problems the panel lists, before it says how many more there are.
const MAX_PROBLEM_LINES: usize = 20;

fn problem_line(problem: &Diagnostic) -> SharedString {
    if problem.line == 0 {
        format!("{}: {}", problem.file, problem.message).into()
    } else {
        format!("{}:{}: {}", problem.file, problem.line, problem.message).into()
    }
}

impl WorkspaceView {
    /// The bar that offers to build the talk's components, when any need it.
    fn build_bar(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        self.talk_folder.as_ref()?;
        let needs_building = self
            .document
            .as_ref()?
            .read(cx)
            .components()
            .iter()
            .any(|info| info.state != ComponentState::UpToDate);
        if !(needs_building || self.building) {
            return None;
        }
        let sentence = |text: &'static str| {
            div()
                .id(text)
                .test_support()
                .aria_label(text)
                .text_sm()
                .child(text)
        };
        let bar = h_flex()
            .id(BUILD_BAR_ID)
            .test_support()
            .w_full()
            .gap_2()
            .px_2()
            .py_1()
            .items_center()
            .bg(cx.theme().secondary)
            .border_t_1()
            .border_color(cx.theme().border);
        Some(if self.building {
            bar.child(sentence(BUILDING))
        } else if self.compile.is_none() {
            bar.child(sentence(NO_COMPILER))
        } else {
            bar.child(sentence(NEEDS_BUILDING))
                .child(
                    Button::new(BUILD_ONCE_ID)
                        .small()
                        .primary()
                        .label("Build components")
                        .on_click(cx.listener(|this, _, _, cx| this.build_components(cx))),
                )
                .when(!self.always_build, |bar| {
                    bar.child(
                        Button::new(BUILD_ALWAYS_ID)
                            .small()
                            .ghost()
                            .label("Always build this folder")
                            .on_click(cx.listener(|this, _, _, cx| this.build_always(cx))),
                    )
                })
        })
    }

    /// What's wrong with the talk's components, one line each.
    fn component_problems(&self, cx: &App) -> Option<impl IntoElement> {
        let document = self.document.as_ref()?.read(cx);
        let problems: Vec<&Diagnostic> = document.component_problems().iter().chain(&self.build_problems).collect();
        let mut lines: Vec<SharedString> = problems.iter().take(MAX_PROBLEM_LINES).map(|problem| problem_line(problem)).collect();
        if problems.len() > MAX_PROBLEM_LINES {
            lines.push(format!("and {} more", problems.len() - MAX_PROBLEM_LINES).into());
        }
        if lines.is_empty() {
            return None;
        }
        Some(
            v_flex()
                .id(COMPONENT_PROBLEMS_ID)
                .test_support()
                .w_full()
                .max_h(px(120.))
                .overflow_y_scroll()
                .px_2()
                .py_1()
                .border_t_1()
                .border_color(cx.theme().border)
                .text_xs()
                .text_color(cx.theme().danger)
                .children(lines.into_iter().enumerate().map(|(index, line)| {
                    div()
                        .id(("component-problem", index))
                        .test_support()
                        .aria_label(line.clone())
                        .child(line)
                })),
        )
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(start) = self.start_screen().cloned() {
            return self.render_start(start, cx).into_any_element();
        }
        let build_bar = self.build_bar(cx);
        // The bars that need an answer.
        let bars: Vec<AnyElement> = self
            .recovery_bar(cx)
            .map(IntoElement::into_any_element)
            .into_iter()
            .chain(self.outside_change_bar(cx).map(IntoElement::into_any_element))
            .collect();
        let component_problems = self.component_problems(cx);
        let document = self.document.as_ref().map(|document| document.read(cx));
        let unsaved = document.is_some_and(Document::has_unsaved_changes);
        let title: SharedString = document
            .map_or_else(|| "Untitled".to_string(), Document::title)
            .into();
        let title: SharedString = if unsaved {
            format!("{UNSAVED_MARK}{title}").into()
        } else {
            title
        };
        let packed = document.map(Document::is_packed);
        let muted_text = cx.theme().muted_foreground;
        let notices: Vec<&str> = self
            .notices
            .iter()
            .map(String::as_str)
            .chain(self.watch_problem.as_deref())
            .collect();
        let offers_stagepack = self.stagepack_offer.is_some();
        let offers_add_ons = self.offers_add_ons && !self.slidev_export_ready();
        let focus_handle = self.focus_handle.clone();
        let canvas = div()
            .id(CANVAS_ID)
            .test_support()
            .relative()
            .size_full()
            .bg(cx.theme().muted)
            .child(self.slide_editor.clone())
            .when_some(self.problem.clone(), |this, problem| {
                this.child(
                    v_flex()
                        .id(PROBLEM_ID)
                        .test_support()
                        .aria_label(problem.clone())
                        .absolute()
                        .size_full()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .p_4()
                        .child(problem)
                        .when(offers_stagepack || offers_add_ons, |this| {
                            this.child(
                                h_flex()
                                    .gap_2()
                                    .when(offers_stagepack, |this| {
                                        this.child(
                                            Button::new(OPEN_IN_STAGEPACK_ID)
                                                .primary()
                                                .label("Open in Stagepack")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.open_in_stagepack(cx)
                                                })),
                                        )
                                    })
                                    .when(offers_add_ons, |this| {
                                        this.child(
                                            Button::new(OFFER_ADD_ONS_ID)
                                                .label("Add-ons…")
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.show_add_ons(window, cx)
                                                })),
                                        )
                                    }),
                            )
                        }),
                )
            })
            // A click on the slide gives it the keyboard, for its keys.
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                focus_handle.focus(window, cx)
            })
            // Pictures dropped on it from outside the app go on the slide.
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                this.dropped(paths.paths().to_vec(), cx);
                this.keyboard_to_the_slide(window, cx);
            }));
        v_flex()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &NextSlide, window, cx| {
                this.go(Document::next, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PreviousSlide, window, cx| {
                this.go(Document::previous, window, cx)
            }))
            .on_action(cx.listener(|this, action: &SelectSlide, window, cx| {
                let index = action.index;
                this.select(|document| document.go_to(index), window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleMarkdown, window, cx| {
                this.toggle_panel(|this| &mut this.show_markdown, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleNotes, window, cx| {
                this.toggle_panel(|this| &mut this.show_notes, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleSlideList, window, cx| {
                this.toggle_panel(|this| &mut this.show_slide_list, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleInspector, window, cx| {
                this.toggle_panel(|this| &mut this.show_inspector, window, cx)
            }))
            .on_action(cx.listener(|this, action: &AlignThings, _, cx| {
                let to = action.to;
                this.edit(|document| document.align_things(to), cx)
            }))
            .on_action(cx.listener(|this, action: &DistributeThings, _, cx| {
                let across = action.across;
                this.edit(|document| document.distribute_things(across), cx)
            }))
            .on_action(cx.listener(|this, action: &ChooseAnimation, _, cx| {
                let choice = action.choice;
                this.edit(|document| document.animate(choice), cx)
            }))
            .on_action(cx.listener(|this, action: &ChooseEntrance, _, cx| match action.enter {
                Some(enter) => this.edit(|document| document.set_entrance(enter), cx),
                None => this.edit(|document| document.animate(Choice::Remove), cx),
            }))
            .on_action(cx.listener(|this, action: &ChooseTransition, _, cx| {
                let (transition, all) = (action.transition, action.all);
                this.edit(|document| document.set_slide_transition(transition, all), cx)
            }))
            .on_action(cx.listener(|this, action: &ReorderThing, _, cx| {
                let order = action.order;
                this.edit(|document| document.reorder_selected(order), cx)
            }))
            .on_action(cx.listener(|this, action: &SetLayout, _, cx| this.set_layout(action.layout, cx)))
            .on_action(cx.listener(|this, _: &NewSlide, _, cx| this.add_slide(DEFAULT_LAYOUT, cx)))
            .on_action(cx.listener(|this, action: &AddSlide, _, cx| {
                this.add_slide(action.layout, cx)
            }))
            .on_action(cx.listener(|this, _: &DuplicateSlide, _, cx| {
                this.edit(Document::duplicate_current, cx)
            }))
            .on_action(cx.listener(|this, _: &DeleteSlide, _, cx| this.delete_slide(cx)))
            .on_action(cx.listener(|this, _: &ToggleHidden, _, cx| this.toggle_hidden(cx)))
            .on_action(cx.listener(|this, _: &MoveSlideUp, _, cx| this.move_slide(false, cx)))
            .on_action(cx.listener(|this, _: &MoveSlideDown, _, cx| this.move_slide(true, cx)))
            .on_action(cx.listener(|this, action: &MoveSlideTo, _, cx| {
                this.move_slide_to(action.from, action.to, cx)
            }))
            .on_action(cx.listener(|this, _: &CopyToClipboard, window, cx| {
                this.clipboard(ClipboardOp::Copy, window, cx)
            }))
            .on_action(cx.listener(|this, _: &CutToClipboard, window, cx| {
                this.clipboard(ClipboardOp::Cut, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PasteFromClipboard, window, cx| {
                this.clipboard(ClipboardOp::Paste, window, cx)
            }))
            .on_action(cx.listener(|this, _: &InsertPicture, window, cx| this.ask_for_pictures(window, cx)))
            .on_action(cx.listener(|this, _: &InsertVideo, window, cx| this.ask_for_videos(window, cx)))
            .on_action(cx.listener(|this, _: &InsertModel, window, cx| this.ask_for_models(window, cx)))
            .on_action(cx.listener(|this, _: &InsertTerminal, window, cx| this.ask_for_recordings(window, cx)))
            .on_action(cx.listener(|this, action: &InsertShape, window, cx| {
                this.insert_shape(action.kind, cx);
                this.keyboard_to_the_slide(window, cx);
            }))
            .on_action(cx.listener(|this, _: &InsertTextBox, _, cx| this.insert_text_box(cx)))
            .on_action(cx.listener(|this, _: &InsertButton, window, cx| {
                this.edit(Document::insert_button, cx);
                this.keyboard_to_the_slide(window, cx);
            }))
            .on_action(cx.listener(|this, _: &TableRowAbove, _, cx| this.edit(|document| document.add_row(false), cx)))
            .on_action(cx.listener(|this, _: &TableRowBelow, _, cx| this.edit(|document| document.add_row(true), cx)))
            .on_action(cx.listener(|this, _: &TableColumnLeft, _, cx| this.edit(|document| document.add_column(false), cx)))
            .on_action(cx.listener(|this, _: &TableColumnRight, _, cx| this.edit(|document| document.add_column(true), cx)))
            .on_action(cx.listener(|this, _: &TableDeleteRow, _, cx| this.edit(Document::remove_row, cx)))
            .on_action(cx.listener(|this, _: &TableDeleteColumn, _, cx| this.edit(Document::remove_column, cx)))
            .on_action(cx.listener(|this, _: &Undo, _, cx| this.step_history(Document::undo, cx)))
            .on_action(cx.listener(|this, _: &Redo, _, cx| this.step_history(Document::redo, cx)))
            .on_action(cx.listener(|this, _: &NewPresentation, _, cx| this.new_presentation(cx)))
            .on_action(cx.listener(|this, _: &OpenFolder, window, cx| this.ask_to_open(true, window, cx)))
            .on_action(cx.listener(|this, _: &OpenPacked, window, cx| this.ask_to_open(false, window, cx)))
            .on_action(cx.listener(|this, _: &Save, window, cx| this.save(window, cx)))
            .on_action(cx.listener(|this, _: &SaveAs, window, cx| this.ask_to_save_as(window, cx)))
            .on_action(
                cx.listener(|this, _: &SetUpForCodeEditor, _, cx| this.set_up_for_code_editor(cx)),
            )
            .on_action(cx.listener(|this, _: &ExportPowerPoint, window, cx| {
                this.export(ExportKind::PowerPoint, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ExportPdf, window, cx| this.export(ExportKind::Pdf, window, cx)))
            .on_action(cx.listener(|this, _: &ExportSlidev, window, cx| this.export_slidev(window, cx)))
            .on_action(cx.listener(|this, _: &ShowAddOns, window, cx| this.show_add_ons(window, cx)))
            .on_action(cx.listener(|this, _: &Present, _, cx| this.present(false, cx)))
            .on_action(cx.listener(|this, _: &Rehearse, _, cx| this.present(true, cx)))
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(file_menu(packed, self.bundle.is_some() && self.talk_folder.is_some()))
                        .child(export_menu(packed.is_some(), self.slidev_export_ready()))
                        .child(
                            div()
                                .id(TITLE_ID)
                                .test_support()
                                .aria_label(title.clone())
                                .ml_4()
                                .text_sm()
                                .child(title),
                        ),
                ),
            )
            .child(self.ribbon.clone())
            .child(self.editor_area(canvas, bars, window.viewport_size().width, cx))
            .children(component_problems)
            .children(build_bar)
            .when(!notices.is_empty(), |this| {
                this.child(footer_line(
                    NOTICES_ID,
                    notices.join(" · ").into(),
                    muted_text,
                ))
            })
            .map(|this| match (self.progress_shown(), self.status.clone()) {
                (Some(progressing), _) => this.child(self.progress_line(progressing, cx)),
                (None, Some(status)) => this.child(footer_line(STATUS_ID, status, muted_text)),
                // The line keeps its room with nothing on it, so the slide stays put as what's
                // said comes and goes, as it does with each click that selects a thing.
                (None, None) if self.document.is_some() => this.child(blank_footer_line()),
                (None, None) => this,
            })
            .into_any_element()
    }
}
