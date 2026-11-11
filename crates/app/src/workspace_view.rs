//! The window's content: the title bar, the slide, its notices and the talk's Markdown, and
//! the bar that offers to build the talk's components.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, TitleBar, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use slides_components::{Bundle, Diagnostic};
use slides_engine::{ComponentState, Fonts};

use crate::builds::{self, BuildOutcome, Compile};
use crate::cargo_file::{self, write_cargo_file};
use crate::document::{Document, OpenProblem, Reload, SLIDES_FILE, TalkPlace};
use crate::export::{self, ExportKind, run_export, stagepack_executable};
use crate::markdown_panel::MarkdownPanel;
use crate::permissions::BuildPermissions;
use crate::present_view::{self, Presentation, start_show};
use crate::slide_canvas::{ScaleChanged, SlideCanvas};
use crate::watcher::{self, Change};

pub const CANVAS_ID: &str = "canvas";
pub const TITLE_ID: &str = "title";
pub const NOTICES_ID: &str = "notices";
pub const STATUS_ID: &str = "status";
pub const PROBLEM_ID: &str = "problem";
pub const MARKDOWN_ID: &str = "markdown";
pub const FILE_MENU_ID: &str = "file-menu";
pub const EXPORT_MENU_ID: &str = "export-menu";
pub const PRESENT_ID: &str = "present";
pub const OPEN_IN_STAGEPACK_ID: &str = "open-in-stagepack";
pub const BUILD_BAR_ID: &str = "build-bar";
pub const BUILD_ONCE_ID: &str = "build-once";
pub const BUILD_ALWAYS_ID: &str = "build-always";
pub const COMPONENT_PROBLEMS_ID: &str = "component-problems";

/// The key context of the window's content, where its shortcuts apply.
pub const KEY_CONTEXT: &str = "Workspace";

/// How the desktop tells this app's windows from another's.
pub(crate) const APP_ID: &str = "stagepack-studio";

/// How long the file has to stay quiet before it's read, as one save can be several writes.
const RELOAD_DELAY: Duration = Duration::from_millis(100);

const MARKDOWN_WIDTH: Pixels = px(380.);

const SLIDEV_TALK: &str =
    "This is a Slidev talk made for Stagepack, so this app can't open it yet.";

const PACKED_EXTENSION: &str = "sptx";

const NEEDS_BUILDING: &str =
    "Some components in this talk need building to show as they're written.";
const BUILDING: &str = "Building components…";
const NO_COMPILER: &str = "This copy of the app has no compiler, so components can't be built.";

actions!(
    stagepack_studio,
    [
        NextSlide,
        PreviousSlide,
        ToggleMarkdown,
        OpenFolder,
        OpenPacked,
        SavePacked,
        SaveFolder,
        SetUpForCodeEditor,
        ExportPowerPoint,
        ExportPdf,
        Present,
        Rehearse,
    ]
);

/// The window's shortcuts, and a show's. Call once, after `gpui_kit::init`.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("right", NextSlide, Some(KEY_CONTEXT)),
        KeyBinding::new("pagedown", NextSlide, Some(KEY_CONTEXT)),
        KeyBinding::new("left", PreviousSlide, Some(KEY_CONTEXT)),
        KeyBinding::new("pageup", PreviousSlide, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-m", ToggleMarkdown, Some(KEY_CONTEXT)),
        KeyBinding::new("f5", Present, Some(KEY_CONTEXT)),
        KeyBinding::new("shift-f5", Rehearse, Some(KEY_CONTEXT)),
    ]);
    present_view::bind_keys(cx);
}

/// Opens the talk at `path`, a folder or a packed file, in a window of its own.
pub fn open_talk_window(path: PathBuf, cx: &mut App) -> Result<()> {
    let path = std::path::absolute(&path).unwrap_or(path);
    let options = WorkspaceView::window_options(cx);
    gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| WorkspaceView::open(path, window, cx))
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
const SAVING: Wording = Wording {
    doing: "Saving",
    done: "Saved",
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
    markdown_panel: Entity<MarkdownPanel>,
    show_markdown: bool,
    focus_handle: FocusHandle,
    /// Keeps the talk's folder watched for as long as the window is open.
    watcher: Option<watcher::TalkWatcher>,
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
    _scale_changed: Subscription,
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
                    view.follow_saves(path, cx);
                    view
                }
                Err(problem) => Self::build(None, Some(problem.into()), window, cx),
            };
        }
        match Document::open_packed(&path, fonts) {
            Ok(document) => Self::open_with(document, window, cx),
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
        let slide_canvas = cx.new(|_| SlideCanvas::default());
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
            markdown_panel,
            show_markdown: false,
            focus_handle,
            watcher: None,
            watch_problem: None,
            pending_reload: None,
            show: None,
            talk_folder,
            compile: builds::bundled_compiler(),
            bundle: Bundle::find(),
            permissions,
            always_build: false,
            building: false,
            build_again: false,
            build_problems: Vec::new(),
            pending_components: None,
            _scale_changed: scale_changed,
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

    /// Keeps which folders are always built in `permissions`, in place of the app's own.
    pub fn set_permissions_for_test(&mut self, permissions: BuildPermissions) {
        self.permissions = Some(permissions);
        self.always_build = self.is_always_built();
    }

    fn is_always_built(&self) -> bool {
        let permissions = self.permissions.as_ref();
        let folder = self.talk_folder.as_deref();
        folder.is_some_and(|folder| permissions.is_some_and(|p| p.always(folder)))
    }

    pub fn slide_canvas(&self) -> &Entity<SlideCanvas> {
        &self.slide_canvas
    }

    /// Whether the talk's folder is being watched for saves.
    pub fn follows_saves(&self) -> bool {
        self.watcher.is_some()
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

    fn go(&mut self, step: fn(&mut Document), cx: &mut Context<Self>) {
        if let Some(document) = self.document.clone() {
            document.update(cx, |document, _| step(document));
            self.show_slide(cx);
        }
    }

    /// Watches `folder`, and reloads the talk from its file whenever that changes.
    pub fn follow_saves(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        let (changed, mut changes) = mpsc::unbounded();
        let watcher = watcher::watch(&folder, move |change| {
            changed.unbounded_send(change).ok();
        });
        match watcher {
            Ok(watcher) => self.watcher = Some(watcher),
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
            let contents = cx
                .background_spawn(async move { slides_engine::read_talk_text(&file).ok() })
                .await;
            this.update(cx, |this, cx| this.reload(contents, cx)).ok();
        }));
    }

    fn reload(&mut self, contents: Option<String>, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let reload = document.update(cx, |document, cx| {
            let reload = document.reload(contents);
            if matches!(reload, Reload::Changed(_)) {
                // A show of the talk has to know.
                cx.notify();
            }
            reload
        });
        let Reload::Changed(slides) = reload else {
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
    /// builds them if the folder is always built. A change elsewhere never builds.
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
        cx.notify();
    }

    /// Asks which talk to open, and opens it in a window of its own.
    fn ask_to_open(&mut self, directories: bool, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: !directories,
            directories,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn(async move |this, cx| {
            let opened = match chosen.await {
                Ok(Ok(Some(mut paths))) if !paths.is_empty() => cx
                    .update(|cx| open_talk_window(paths.remove(0), cx))
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

    fn save_packed(&mut self, cx: &mut Context<Self>) {
        self.write_out(
            Some(PACKED_EXTENSION),
            SAVING,
            |document| {
                let place = document.place().clone();
                move |out: &Path| place.save_packed(out)
            },
            cx,
        );
    }

    fn save_folder(&mut self, cx: &mut Context<Self>) {
        self.write_out(
            None,
            SAVING,
            |document| {
                let place = document.place().clone();
                move |out: &Path| place.save_folder(out).map(|()| Vec::new())
            },
            cx,
        );
    }

    /// Writes a `Cargo.toml` for the talk's components, which only this asks for.
    fn set_up_for_code_editor(&mut self, cx: &mut Context<Self>) {
        let (Some(folder), Some(bundle)) = (&self.talk_folder, &self.bundle) else {
            return;
        };
        let said = match write_cargo_file(folder, bundle) {
            Ok(_) => cargo_file::WRITTEN.to_string(),
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
            let menu = menu
                .menu("Open folder…", Box::new(OpenFolder))
                .menu("Open packed talk…", Box::new(OpenPacked));
            match packed {
                Some(false) => menu
                    .separator()
                    .menu("Save as packed talk…", Box::new(SavePacked))
                    .when(for_code_editor, |menu| {
                        menu.separator()
                            .menu("Set up for a code editor", Box::new(SetUpForCodeEditor))
                    }),
                Some(true) => menu
                    .separator()
                    .menu("Save as folder…", Box::new(SaveFolder)),
                None => menu,
            }
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
        let lines: Vec<SharedString> = document
            .component_problems()
            .iter()
            .chain(&self.build_problems)
            .map(problem_line)
            .collect();
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
        let component_problems = self.component_problems(cx);
        let document = self.document.as_ref().map(|document| document.read(cx));
        let title: SharedString = document
            .map_or_else(|| "Untitled".to_string(), Document::title)
            .into();
        let packed = document.map(Document::is_packed);
        let muted_text = cx.theme().muted_foreground;
        let notices: Vec<&str> = self
            .notices
            .iter()
            .map(String::as_str)
            .chain(self.watch_problem.as_deref())
            .collect();
        let offers_stagepack = self.stagepack_offer.is_some();
        v_flex()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &NextSlide, _, cx| this.go(Document::next, cx)))
            .on_action(
                cx.listener(|this, _: &PreviousSlide, _, cx| this.go(Document::previous, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleMarkdown, _, cx| {
                this.show_markdown = !this.show_markdown;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &OpenFolder, _, cx| this.ask_to_open(true, cx)))
            .on_action(cx.listener(|this, _: &OpenPacked, _, cx| this.ask_to_open(false, cx)))
            .on_action(cx.listener(|this, _: &SavePacked, _, cx| this.save_packed(cx)))
            .on_action(cx.listener(|this, _: &SaveFolder, _, cx| this.save_folder(cx)))
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
                            Button::new(PRESENT_ID)
                                .ghost()
                                .small()
                                .label("Present")
                                .disabled(packed.is_none())
                                .on_click(cx.listener(|this, _, _, cx| this.present(false, cx))),
                        )
                        .child(
                            div()
                                .id(TITLE_ID)
                                .test_support()
                                .aria_label(title.clone())
                                .text_sm()
                                .child(title),
                        ),
                ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .items_stretch()
                    .child(
                        div()
                            .id(CANVAS_ID)
                            .test_support()
                            .relative()
                            .flex_1()
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
                            }),
                    )
                    .when(self.show_markdown, |this| {
                        this.child(
                            div()
                                .id(MARKDOWN_ID)
                                .test_support()
                                .flex_none()
                                .w(MARKDOWN_WIDTH)
                                .border_l_1()
                                .border_color(cx.theme().border)
                                .child(self.markdown_panel.clone()),
                        )
                    }),
            )
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
