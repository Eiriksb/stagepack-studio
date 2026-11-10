//! The window's content: the title bar, the slide, its notices and the talk's Markdown.

use std::path::PathBuf;
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::component::{ActiveTheme as _, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use notify::RecommendedWatcher;
use slides_engine::Fonts;

use crate::document::{Document, Reload, SLIDES_FILE};
use crate::markdown_panel::MarkdownPanel;
use crate::slide_canvas::{ScaleChanged, SlideCanvas};
use crate::watcher;

pub const CANVAS_ID: &str = "canvas";
pub const TITLE_ID: &str = "title";
pub const NOTICES_ID: &str = "notices";
pub const PROBLEM_ID: &str = "problem";
pub const MARKDOWN_ID: &str = "markdown";

/// The key context of the window's content, where its shortcuts apply.
pub const KEY_CONTEXT: &str = "Workspace";

/// How long the file has to stay quiet before it's read, as one save can be several writes.
const RELOAD_DELAY: Duration = Duration::from_millis(100);

const MARKDOWN_WIDTH: Pixels = px(380.);

actions!(stagepack_studio, [NextSlide, PreviousSlide, ToggleMarkdown]);

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

pub struct WorkspaceView {
    document: Option<Document>,
    /// Why no talk is showing, when one was asked for.
    problem: Option<SharedString>,
    slide_canvas: Entity<SlideCanvas>,
    notices: Vec<String>,
    markdown_panel: Entity<MarkdownPanel>,
    show_markdown: bool,
    focus_handle: FocusHandle,
    /// Keeps the talk's folder watched for as long as the window is open.
    _watcher: Option<RecommendedWatcher>,
    /// Why saved changes won't show up, if the folder couldn't be watched.
    watch_problem: Option<String>,
    /// The reload waiting for the file to go quiet.
    pending_reload: Option<Task<()>>,
    _scale_changed: Subscription,
}

impl WorkspaceView {
    /// Opens the talk in `folder` and follows its saves. If it can't be, the window says why.
    pub fn open(folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        match Document::open(&folder, Fonts::with_system_fallback()) {
            Ok(document) => {
                let mut view = Self::open_with(document, window, cx);
                view.follow_saves(folder, cx);
                view
            }
            Err(problem) => Self::build(None, Some(problem.into()), window, cx),
        }
    }

    /// Shows a talk that is already open.
    pub fn open_with(document: Document, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(Some(document), None, window, cx)
    }

    /// The window with no talk, for headless tests.
    pub fn for_test(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(None, None, window, cx)
    }

    fn build(
        mut document: Option<Document>,
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
        let notices = document.as_mut().map(Document::notices).unwrap_or_default();
        Self {
            document,
            problem,
            slide_canvas,
            notices,
            markdown_panel,
            show_markdown: false,
            focus_handle,
            _watcher: None,
            watch_problem: None,
            pending_reload: None,
            _scale_changed: scale_changed,
        }
    }

    pub fn slide_canvas(&self) -> &Entity<SlideCanvas> {
        &self.slide_canvas
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
        let Some(document) = &mut self.document else {
            return;
        };
        self.notices = document.notices();
        if let Some(scale) = self.slide_canvas.read(cx).scale() {
            let layers = document.layers(scale);
            self.slide_canvas
                .update(cx, |canvas, cx| canvas.set_layers(layers, cx));
        }
        cx.notify();
    }

    fn go(&mut self, step: fn(&mut Document), cx: &mut Context<Self>) {
        if let Some(document) = &mut self.document {
            step(document);
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
            Ok(watcher) => self._watcher = Some(watcher),
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
        let Some(document) = &mut self.document else {
            return;
        };
        let Reload::Changed(slides) = document.reload(contents) else {
            return;
        };
        let showing_changed = slides.contains(&document.current());
        self.markdown_panel
            .update(cx, |panel, cx| panel.set_source(document.source(), cx));
        if showing_changed {
            self.show_slide(cx);
        } else {
            // The title counts the slides, and the talk may have gained some.
            cx.notify();
        }
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title: SharedString = self
            .document
            .as_ref()
            .map_or_else(|| "Untitled".to_string(), Document::title)
            .into();
        let muted_text = cx.theme().muted_foreground;
        let notices: Vec<&str> = self
            .notices
            .iter()
            .map(String::as_str)
            .chain(self.watch_problem.as_deref())
            .collect();
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
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new().child(
                    h_flex().gap_1().items_center().child(
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
                                    div()
                                        .id(PROBLEM_ID)
                                        .test_support()
                                        .aria_label(problem.clone())
                                        .absolute()
                                        .size_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .p_4()
                                        .child(problem),
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
                let line: SharedString = notices.join(" · ").into();
                this.child(
                    div()
                        .id(NOTICES_ID)
                        .test_support()
                        .aria_label(line.clone())
                        .w_full()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .text_color(muted_text)
                        .truncate()
                        .child(line),
                )
            })
    }
}
