//! The window's content: the title bar, the slide and its notices.

use std::path::PathBuf;

use gpui_kit::component::{ActiveTheme as _, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use slides_engine::Fonts;

use crate::document::Document;
use crate::slide_canvas::{ScaleChanged, SlideCanvas};

pub const CANVAS_ID: &str = "canvas";
pub const TITLE_ID: &str = "title";
pub const NOTICES_ID: &str = "notices";
pub const PROBLEM_ID: &str = "problem";

/// The key context of the window's content, where its shortcuts apply.
pub const KEY_CONTEXT: &str = "Workspace";

actions!(stagepack_studio, [NextSlide, PreviousSlide]);

/// The window's shortcuts. Call once, after `gpui_kit::init`.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("right", NextSlide, Some(KEY_CONTEXT)),
        KeyBinding::new("pagedown", NextSlide, Some(KEY_CONTEXT)),
        KeyBinding::new("left", PreviousSlide, Some(KEY_CONTEXT)),
        KeyBinding::new("pageup", PreviousSlide, Some(KEY_CONTEXT)),
    ]);
}

pub struct WorkspaceView {
    document: Option<Document>,
    /// Why no talk is showing, when one was asked for.
    problem: Option<SharedString>,
    slide_canvas: Entity<SlideCanvas>,
    notices: Vec<String>,
    focus_handle: FocusHandle,
    _scale_changed: Subscription,
}

impl WorkspaceView {
    /// Opens the talk in `folder`. If it can't be, the window says why.
    pub fn open(folder: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        match Document::open(&folder, Fonts::with_system_fallback()) {
            Ok(document) => Self::open_with(document, window, cx),
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
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        let notices = document.as_mut().map(Document::notices).unwrap_or_default();
        Self {
            document,
            problem,
            slide_canvas,
            notices,
            focus_handle,
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
}

impl Render for WorkspaceView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title: SharedString = self
            .document
            .as_ref()
            .map_or_else(|| "Untitled".to_string(), Document::title)
            .into();
        let muted_text = cx.theme().muted_foreground;
        v_flex()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &NextSlide, _, cx| this.go(Document::next, cx)))
            .on_action(
                cx.listener(|this, _: &PreviousSlide, _, cx| this.go(Document::previous, cx)),
            )
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
                div()
                    .id(CANVAS_ID)
                    .test_support()
                    .relative()
                    .flex_1()
                    .w_full()
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
            .when(!self.notices.is_empty(), |this| {
                let line: SharedString = self.notices.join(" · ").into();
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
