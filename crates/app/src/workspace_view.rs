//! The window's content: the title bar, the tools on their tabs, the list of slides, the
//! slide with its notes and notices, the talk's Markdown, the bar that offers to build the
//! talk's components, and saving the talk.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input;
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, TitleBar, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use slides_components::{Bundle, Diagnostic};
use slides_engine::{ComponentState, Fonts};

use crate::builds::{self, BuildOutcome, Compile};
use crate::cargo_file::{self, write_cargo_file};
use crate::document::{Document, EditRefused, OpenProblem, Reload, SLIDES_FILE, TalkPlace, UNREADABLE_FILE};
use crate::export::{self, ExportKind, run_export, stagepack_executable};
use crate::markdown_panel::MarkdownPanel;
use crate::notes_panel::{
    NOTES_HEIGHT, NOTES_MAX_HEIGHT, NOTES_MIN_HEIGHT, NotesEdited, NotesPanel,
};
use crate::permissions::BuildPermissions;
use crate::present_view::{self, Presentation, start_show};
use crate::recovery::{Found, Recovery, hash_text};
use crate::ribbon::{Panels, Ribbon};
use crate::saving::{SaveError, SaveJob, SaveTarget, Saved};
use crate::slide_canvas::{ScaleChanged, SlideCanvas};
use crate::slide_strip::{
    self, STRIP_CONTEXT, STRIP_MAX_WIDTH, STRIP_MIN_WIDTH, STRIP_WIDTH, SlideStrip,
};
use crate::watcher::{self, Change};

mod editing;
mod saving;

use saving::Kept;

pub const CANVAS_ID: &str = "canvas";
pub const TITLE_ID: &str = "title";
pub const NOTICES_ID: &str = "notices";
pub const STATUS_ID: &str = "status";
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

/// The key context of the window's content, where its shortcuts apply.
pub const KEY_CONTEXT: &str = "Workspace";

/// The key context of the kit's text fields.
const INPUT_CONTEXT: &str = "Input";

/// How the desktop tells this app's windows from another's.
pub(crate) const APP_ID: &str = "stagepack-studio";

/// How long the file has to stay quiet before it's read, as one save can be several writes.
const RELOAD_DELAY: Duration = Duration::from_millis(100);

/// How long after the last edit the working copy of unsaved changes is brought up to date.
const WORKING_COPY_DELAY: Duration = Duration::from_secs(1);
/// The longest edits that never stop for [`WORKING_COPY_DELAY`] go without being kept, from
/// the first of them.
const WORKING_COPY_MOST_WAIT: Duration = Duration::from_secs(3);
/// How long "Saved" stays on the status line.
const STATUS_FADE: Duration = Duration::from_secs(5);
/// How long after a working copy couldn't be written it's tried again.
const KEEP_RETRY_DELAY: Duration = Duration::from_secs(10);

const MARKDOWN_WIDTH: Pixels = px(380.);
const MARKDOWN_MIN_WIDTH: Pixels = px(200.);
const MARKDOWN_MAX_WIDTH: Pixels = px(800.);

const SLIDEV_TALK: &str =
    "This is a Slidev talk made for Stagepack, so this app can't open it yet.";

const PACKED_EXTENSION: &str = "sptx";

const NEEDS_BUILDING: &str =
    "Some components in this talk need building to show as they're written.";
const BUILDING: &str = "Building components…";
const NO_COMPILER: &str = "This copy of the app has no compiler, so components can't be built.";

pub const OUTSIDE_CHANGE: &str = "This talk was changed outside the app.";
const UNSAVED_ON_CLOSE: &str = "This talk has changes that haven't been saved.";

pub const RECOVERY_OFFER: &str = "Changes from last time weren't saved.";
pub const RECOVERY_CHANGED_SINCE: &str = "The talk has also changed since then.";
const KEEP_FAILED: &str =
    "Unsaved changes couldn't be kept safe in case the app stops. The app will try again shortly.";

/// What the title shows before the talk's name while it has unsaved changes.
pub const UNSAVED_MARK: &str = "• ";

actions!(
    stagepack_studio,
    [
        NextSlide,
        PreviousSlide,
        ToggleMarkdown,
        OpenFolder,
        OpenPacked,
        Save,
        SaveAs,
        SetUpForCodeEditor,
        ExportPowerPoint,
        ExportPdf,
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
        KeyBinding::new("ctrl-s", Save, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-shift-s", SaveAs, Some(KEY_CONTEXT)),
        KeyBinding::new("f5", Present, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-f5", Rehearse, Some(KEY_CONTEXT)),
    ]);
    slide_strip::bind_keys(cx);
    present_view::bind_keys(cx);
}

/// Opens the talk at `path`, a folder or a packed file, in a window of its own. Its unsaved
/// changes are kept safe in `recovery`, if there's one.
pub fn open_talk_window(path: PathBuf, recovery: Option<Recovery>, cx: &mut App) -> Result<()> {
    let path = std::path::absolute(&path).unwrap_or(path);
    let options = WorkspaceView::window_options(cx);
    gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| {
            let mut view = WorkspaceView::open(path, window, cx);
            if let Some(recovery) = recovery {
                view.use_recovery(recovery, cx);
            }
            view
        })
    })?;
    cx.activate(true);
    Ok(())
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
/// A file this app can't show but an installed Stagepack can.
struct StagepackOffer {
    file: PathBuf,
    executable: PathBuf,
}

pub struct WorkspaceView {
    document: Option<Entity<Document>>,
    /// Why no talk is showing, when one was asked for.
    problem: Option<SharedString>,
    stagepack_offer: Option<StagepackOffer>,
    slide_canvas: Entity<SlideCanvas>,
    notices: Vec<String>,
    /// What came of the last thing asked of the File or Export menu.
    status: Option<SharedString>,
    /// The wait before a status that only stays a while goes, while it's showing.
    status_fading: Option<Task<()>>,
    markdown_panel: Entity<MarkdownPanel>,
    show_markdown: bool,
    ribbon: Entity<Ribbon>,
    slide_strip: Entity<SlideStrip>,
    show_slide_list: bool,
    notes_panel: Entity<NotesPanel>,
    show_notes: bool,
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
    /// Whether this talk's folder is one of them.
    always_build: bool,
    /// Whether a build is running.
    building: bool,
    /// Whether another build was asked for while one ran.
    build_again: bool,
    /// What the last build couldn't do.
    build_problems: Vec<Diagnostic>,
    /// The look at the components waiting for them to go quiet.
    pending_components: Option<Task<()>>,
    /// Whether a save is running.
    saving: bool,
    /// Whether another save was asked for while one ran.
    save_again: bool,
    /// Whether a read of the file came in while a save ran. It's read again once the save
    /// is done: what came in may be older than what the save wrote.
    reload_after_save: bool,
    /// Whether the window closes once the running save is done.
    close_after_save: bool,
    /// Whether the window may close with unsaved changes, as asked.
    close_anyway: bool,
    /// Where working copies of unsaved changes are kept, if anywhere.
    recovery: Option<Recovery>,
    /// The working copy this window last wrote, while it's there.
    kept: Option<Kept>,
    /// The update of the working copy waiting for the edits to stop for a moment.
    pending_working_copy: Option<Task<()>>,
    /// When the first edit the working copy doesn't hold yet was made, while an update waits.
    unkept_since: Option<Instant>,
    /// Unsaved changes from last time, while nobody has said whether to bring them back.
    offered: Option<Found>,
    /// The copy from last time that was brought back, until this window's own copy or a save
    /// holds what it did.
    adopted: saving::Adopted,
    /// The last change to the working copies, which the next one waits for.
    working_copy_chain: Option<Task<()>>,
    /// How many clears of this window's copy have been asked for.
    clears: Arc<AtomicU64>,
    /// The window this is the content of.
    window_handle: AnyWindowHandle,
    _scale_changed: Subscription,
    _document_changed: Option<Subscription>,
    _notes_edited: Subscription,
}

impl WorkspaceView {
    /// Opens the talk at `path`, a folder or a packed file. A folder's saves are followed, as
    /// nothing edits a packed file in place. If it can't be opened, the window says why.
    pub fn open(path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let fonts = Fonts::with_system_fallback();
        if path.is_dir() {
            return match Document::open(&path, fonts) {
                Ok(document) => {
                    let mut view = Self::open_with(document, window, cx);
                    view.opened_from_path = true;
                    view.follow_saves(path, cx);
                    view
                }
                Err(problem) => Self::build(None, Some(problem.into()), window, cx),
            };
        }
        match Document::open_packed(&path, fonts) {
            Ok(document) => {
                let mut view = Self::open_with(document, window, cx);
                view.opened_from_path = true;
                view
            }
            Err(OpenProblem::SlidevTalk) => {
                let stagepack =
                    stagepack_executable(|name| std::env::var(name).ok(), Path::is_file);
                Self::for_slidev_talk(path, stagepack, window, cx)
            }
            Err(OpenProblem::Other(problem)) => Self::build(None, Some(problem.into()), window, cx),
        }
    }

    /// Shows a talk that is already open.
    pub fn open_with(document: Document, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(Some(document), None, window, cx)
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
        let panels = Panels {
            notes: true,
            slide_list: true,
            markdown: false,
        };
        let ribbon = cx.new(|cx| Ribbon::new(document.clone(), panels, cx));
        let slide_strip = cx.new(|cx| SlideStrip::new(document.clone(), cx));
        let notes_panel = cx.new(|cx| NotesPanel::new(document.clone(), window, cx));
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
        // The title says whether there are unsaved changes, whatever made them, and the
        // working copy follows them.
        let document_changed = document.as_ref().map(|document| {
            cx.observe(document, |this, _, cx| {
                this.keep_working_copy_soon(cx);
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
            .and_then(|document| match document.read(cx).place() {
                TalkPlace::Folder(folder) => Some(folder.clone()),
                TalkPlace::Packed(_) => None,
            });
        let permissions = BuildPermissions::default_file().map(BuildPermissions::load);
        let mut view = Self {
            document,
            problem,
            stagepack_offer: None,
            slide_canvas,
            notices,
            status: None,
            status_fading: None,
            markdown_panel,
            show_markdown: panels.markdown,
            ribbon,
            slide_strip,
            show_slide_list: panels.slide_list,
            notes_panel,
            show_notes: panels.notes,
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
            bundle: Bundle::find(),
            editor_projects: cargo_file::default_projects_root(),
            permissions,
            always_build: false,
            building: false,
            build_again: false,
            build_problems: Vec::new(),
            pending_components: None,
            saving: false,
            save_again: false,
            reload_after_save: false,
            close_after_save: false,
            close_anyway: false,
            recovery: None,
            kept: None,
            pending_working_copy: None,
            unkept_since: None,
            offered: None,
            adopted: saving::Adopted::default(),
            working_copy_chain: None,
            clears: Arc::new(AtomicU64::new(0)),
            window_handle: window.window_handle(),
            _scale_changed: scale_changed,
            _document_changed: document_changed,
            _notes_edited: notes_edited,
        };
        view.always_build = view.is_always_built();
        view
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

    pub fn ribbon(&self) -> &Entity<Ribbon> {
        &self.ribbon
    }

    pub fn slide_strip(&self) -> &Entity<SlideStrip> {
        &self.slide_strip
    }

    pub fn notes_panel(&self) -> &Entity<NotesPanel> {
        &self.notes_panel
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
            window_min_size: Some(size(px(800.), px(500.))),
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
        let (notices, layers) = document.update(cx, |document, _| {
            (
                document.notices(),
                scale.map(|scale| document.layers(scale)),
            )
        });
        self.notices = notices;
        if let Some(layers) = layers {
            self.slide_canvas
                .update(cx, |canvas, cx| canvas.set_layers(layers, cx));
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
        if self.saving {
            self.reload_after_save = true;
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
        let this = cx.entity().downgrade();
        // Once this window is free again, as the show asks it which screen it's on.
        cx.defer(move |cx| {
            let show = start_show(document, rehearsing, cx);
            this.update(cx, |this, cx| {
                if show.read(cx).has_ended() {
                    this.say("Could not open a window for the show.", cx);
                }
                this.show = Some(show.downgrade());
            })
            .ok();
        });
    }

    /// Puts `status` on the status line.
    fn say(&mut self, status: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status = Some(status.into());
        self.status_fading = None;
        cx.notify();
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

    /// Asks which talk to open, and opens it in a window of its own.
    fn ask_to_open(&mut self, directories: bool, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: !directories,
            directories,
            multiple: false,
            prompt: Some("Open".into()),
        });
        let recovery = self.recovery.clone();
        cx.spawn(async move |this, cx| {
            let opened = match chosen.await {
                Ok(Ok(Some(mut paths))) if !paths.is_empty() => cx
                    .update(|cx| open_talk_window(paths.remove(0), recovery, cx))
                    .map_err(|error| format!("Could not open a window for that talk: {error}")),
                Ok(Err(error)) => Err(format!("Could not show the Open dialog: {error}")),
                _ => Ok(()),
            };
            if let Err(problem) = opened {
                this.update(cx, |this, cx| this.say(problem, cx)).ok();
            }
        })
        .detach();
    }

    /// Asks where to put what `job` will write, then runs it on a background task and says how
    /// it went. `job` is made once the place is chosen, so it has the talk as it is by then.
    fn write_out<Job>(
        &mut self,
        extension: Option<&str>,
        wording: Wording,
        job: impl FnOnce(&Document) -> Job + 'static,
        cx: &mut Context<Self>,
    ) where
        Job: FnOnce(&Path) -> Result<Vec<String>, String> + Send + 'static,
    {
        let Some(document) = &self.document else {
            return;
        };
        let document = document.read(cx);
        let stem = export::file_stem_for(&document.name());
        let suggested = extension.map_or_else(|| stem.clone(), |ext| format!("{stem}.{ext}"));
        let chosen = cx.prompt_for_new_path(document.place().directory(), Some(&suggested));
        cx.spawn(async move |this, cx| {
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
                this.say(format!("{} {name}…", wording.doing), cx);
                this.document
                    .as_ref()
                    .map(|document| job(document.read(cx)))
            });
            let Ok(Some(job)) = job else {
                return;
            };
            let outcome = cx.background_spawn(async move { job(&out) }).await;
            let said = match outcome {
                Ok(notices) if notices.is_empty() => format!("{} {name}.", wording.done),
                Ok(notices) => format!("{} {name}. {}", wording.done, notices.join(" · ")),
                Err(problem) => problem,
            };
            this.update(cx, |this, cx| this.say(said, cx)).ok();
        })
        .detach();
    }

    fn export(&mut self, kind: ExportKind, cx: &mut Context<Self>) {
        self.write_out(
            Some(kind.extension()),
            EXPORTING,
            move |document| {
                let input = document.export_input();
                move |out: &Path| run_export(input, kind, out)
            },
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

    fn open_in_stagepack(&mut self, cx: &mut Context<Self>) {
        let Some(offer) = &self.stagepack_offer else {
            return;
        };
        if let Err(error) = export::open_in_stagepack(&offer.executable, &offer.file) {
            self.say(format!("Could not start Stagepack: {error}"), cx);
        }
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
            menu.menu("Open talk folder…", Box::new(OpenFolder))
                .menu("Open talk file…", Box::new(OpenPacked))
                .separator()
                .menu_with_disabled("Save", Box::new(Save), no_talk)
                .menu_with_disabled("Save As…", Box::new(SaveAs), no_talk)
                .when(for_code_editor && packed == Some(false), |menu| {
                    menu.separator()
                        .menu("Set up for a code editor", Box::new(SetUpForCodeEditor))
                })
        })
}

fn export_menu(has_talk: bool) -> impl IntoElement {
    Button::new(EXPORT_MENU_ID)
        .ghost()
        .small()
        .label("Export")
        .dropdown_menu(move |menu, _, _| {
            menu.menu_with_disabled("PowerPoint…", Box::new(ExportPowerPoint), !has_talk)
                .menu_with_disabled("PDF…", Box::new(ExportPdf), !has_talk)
        })
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let focus_handle = self.focus_handle.clone();
        let canvas = div()
            .id(CANVAS_ID)
            .test_support()
            .relative()
            .size_full()
            .bg(cx.theme().muted)
            .child(self.slide_canvas.clone())
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
                        .when(offers_stagepack, |this| {
                            this.child(
                                Button::new(OPEN_IN_STAGEPACK_ID)
                                    .primary()
                                    .label("Open in Stagepack")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.open_in_stagepack(cx)
                                    })),
                            )
                        }),
                )
            })
            // A click on the slide gives it the keyboard, for its keys.
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                focus_handle.focus(window, cx)
            });
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
                this.select(|document| document.select(index), window, cx)
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
            .on_action(cx.listener(|this, _: &NewSlide, _, cx| this.add_slide("content", cx)))
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
            .on_action(cx.listener(|this, _: &Undo, _, cx| this.step_history(Document::undo, cx)))
            .on_action(cx.listener(|this, _: &Redo, _, cx| this.step_history(Document::redo, cx)))
            .on_action(cx.listener(|this, _: &OpenFolder, _, cx| this.ask_to_open(true, cx)))
            .on_action(cx.listener(|this, _: &OpenPacked, _, cx| this.ask_to_open(false, cx)))
            .on_action(cx.listener(|this, _: &Save, _, cx| this.save(cx)))
            .on_action(cx.listener(|this, _: &SaveAs, window, cx| this.ask_to_save_as(window, cx)))
            .on_action(
                cx.listener(|this, _: &SetUpForCodeEditor, _, cx| this.set_up_for_code_editor(cx)),
            )
            .on_action(cx.listener(|this, _: &ExportPowerPoint, _, cx| {
                this.export(ExportKind::PowerPoint, cx)
            }))
            .on_action(cx.listener(|this, _: &ExportPdf, _, cx| this.export(ExportKind::Pdf, cx)))
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
                        .child(file_menu(packed, self.bundle.is_some()))
                        .child(export_menu(packed.is_some()))
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
            .child(self.editor_area(canvas, bars, cx))
            .children(component_problems)
            .children(build_bar)
            .when(!notices.is_empty(), |this| {
                this.child(footer_line(
                    NOTICES_ID,
                    notices.join(" · ").into(),
                    muted_text,
                ))
            })
            .when_some(self.status.clone(), |this, status| {
                this.child(footer_line(STATUS_ID, status, muted_text))
            })
    }
}
