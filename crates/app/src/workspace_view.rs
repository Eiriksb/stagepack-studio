//! The window's content: the title bar, the slide, its notices and the talk's Markdown.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use notify::RecommendedWatcher;
use slides_engine::Fonts;

use crate::document::{Document, OpenProblem, Reload, SLIDES_FILE};
use crate::export::{self, ExportKind, run_export, stagepack_executable};
use crate::markdown_panel::MarkdownPanel;
use crate::slide_canvas::{ScaleChanged, SlideCanvas};
use crate::watcher;

pub const CANVAS_ID: &str = "canvas";
pub const TITLE_ID: &str = "title";
pub const NOTICES_ID: &str = "notices";
pub const STATUS_ID: &str = "status";
pub const PROBLEM_ID: &str = "problem";
pub const MARKDOWN_ID: &str = "markdown";
pub const FILE_MENU_ID: &str = "file-menu";
pub const EXPORT_MENU_ID: &str = "export-menu";
pub const OPEN_IN_STAGEPACK_ID: &str = "open-in-stagepack";

/// The key context of the window's content, where its shortcuts apply.
pub const KEY_CONTEXT: &str = "Workspace";

/// How long the file has to stay quiet before it's read, as one save can be several writes.
const RELOAD_DELAY: Duration = Duration::from_millis(100);

const MARKDOWN_WIDTH: Pixels = px(380.);

const SLIDEV_TALK: &str =
    "This is a Slidev talk made for Stagepack, so this app can't open it yet.";

const PACKED_EXTENSION: &str = "sptx";

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
        ExportPowerPoint,
        ExportPdf,
    ]
);

/// The window's shortcuts. Call once, after `gpui_kit::init`.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("right", NextSlide, Some(KEY_CONTEXT)),
        KeyBinding::new("pagedown", NextSlide, Some(KEY_CONTEXT)),
        KeyBinding::new("left", PreviousSlide, Some(KEY_CONTEXT)),
        KeyBinding::new("pageup", PreviousSlide, Some(KEY_CONTEXT)),
        KeyBinding::new("ctrl-m", ToggleMarkdown, Some(KEY_CONTEXT)),
    ]);
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
    watcher: Option<RecommendedWatcher>,
    /// Why saved changes won't show up, if the folder couldn't be watched.
    watch_problem: Option<String>,
    /// The reload waiting for the file to go quiet.
    pending_reload: Option<Task<()>>,
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
        Self {
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
            _scale_changed: scale_changed,
        }
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
            app_id: Some("stagepack-studio".into()),
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
        let watcher = watcher::watch(&folder, move || {
            changed.unbounded_send(()).ok();
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
            while changes.next().await.is_some() {
                let file = file.clone();
                if this
                    .update(cx, |this, cx| this.reload_soon(file, cx))
                    .is_err()
                {
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
                .background_spawn(async move { std::fs::read_to_string(file).ok() })
                .await;
            this.update(cx, |this, cx| this.reload(contents, cx)).ok();
        }));
    }

    fn reload(&mut self, contents: Option<String>, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let reload = document.update(cx, |document, _| document.reload(contents));
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
fn file_menu(packed: Option<bool>) -> impl IntoElement {
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
                    .menu("Save as packed talk…", Box::new(SavePacked)),
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

impl Render for WorkspaceView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .on_action(cx.listener(|this, _: &ExportPowerPoint, _, cx| {
                this.export(ExportKind::PowerPoint, cx)
            }))
            .on_action(cx.listener(|this, _: &ExportPdf, _, cx| this.export(ExportKind::Pdf, cx)))
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(file_menu(packed))
                        .child(export_menu(packed.is_some()))
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
