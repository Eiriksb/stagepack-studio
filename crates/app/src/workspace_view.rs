//! The window's content: the title bar, the canvas and the inspector slot.

use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, TitleBar, h_flex, h_resizable,
    resizable_panel, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::canvas::Canvas;
use crate::document_view::DocumentView;
use crate::inspector::{Inspector, InspectorEvent};
use studio_protocol::CommandBody;

pub const CANVAS_ID: &str = "canvas";
pub const INSPECTOR_ID: &str = "inspector";
pub const TITLE_ID: &str = "title";

pub const INSPECTOR_WIDTH: Pixels = px(280.);

/// The key context of the window's content, where its shortcuts apply.
pub const KEY_CONTEXT: &str = "Workspace";

actions!(stagepack_studio, [Save]);

/// The window's shortcuts. Call once, after `gpui_kit::init`.
pub fn bind_keys(cx: &mut App) {
    // `KeyBinding::new` takes explicit modifiers only, not `secondary-`.
    let save = if cfg!(target_os = "macos") {
        "cmd-s"
    } else {
        "ctrl-s"
    };
    cx.bind_keys([KeyBinding::new(save, Save, Some(KEY_CONTEXT))]);
}

pub struct WorkspaceView {
    /// `None` in headless tests, which can't host a web view.
    canvas: Option<Entity<Canvas>>,
    /// The open talk. `None` for `STUDIO_DEV_URL`, which only loads a page.
    document: Option<Entity<DocumentView>>,
    inspector: Entity<Inspector>,
    /// What would have gone to the page, in tests that have no canvas.
    sent: Vec<CommandBody>,
    focus_handle: FocusHandle,
    _observe: Option<Subscription>,
    _inspector_events: Subscription,
}

impl WorkspaceView {
    /// A canvas and no talk, for `STUDIO_DEV_URL`.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let canvas = cx.new(|cx| Canvas::new(window, cx));
        Self::build(Some(canvas), None, None, window, cx)
    }

    /// Opens the talk at `path` through its sidecar.
    pub fn open(path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let canvas = cx.new(|cx| Canvas::new(window, cx));
        let document = cx.new(|cx| DocumentView::open(path, canvas.clone(), window, cx));
        Self::with_document(Some(canvas), document, window, cx)
    }

    /// The same layout with a plain div where the web view goes.
    pub fn for_test(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::build(None, None, None, window, cx)
    }

    /// The layout with a talk that has no sidecar and no web view; see
    /// [`DocumentView::for_test`].
    pub fn for_test_with_document(
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let document = cx.new(|cx| DocumentView::for_test(path, cx));
        Self::with_document(None, document, window, cx)
    }

    fn with_document(
        canvas: Option<Entity<Canvas>>,
        document: Entity<DocumentView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let observe = cx.observe_in(&document, window, |this, document, window, cx| {
            this.show_selection(&document, window, cx);
            cx.notify();
        });
        // The window manager's close button asks the talk first.
        let asked = document.downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            asked
                .update(cx, |document, cx| document.request_close(window, cx))
                .unwrap_or(true)
        });
        Self::build(canvas, Some(document), Some(observe), window, cx)
    }

    fn build(
        canvas: Option<Entity<Canvas>>,
        document: Option<Entity<DocumentView>>,
        observe: Option<Subscription>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let inspector = cx.new(|cx| Inspector::new(window, cx));
        let inspector_events = cx.subscribe(&inspector, |this, _, event: &InspectorEvent, cx| {
            let InspectorEvent::Send(body) = event;
            this.send(body.clone(), cx);
        });
        let focus_handle = cx.focus_handle();
        if document.is_some() {
            focus_handle.focus(window, cx);
        }
        Self {
            canvas,
            document,
            inspector,
            sent: Vec::new(),
            focus_handle,
            _observe: observe,
            _inspector_events: inspector_events,
        }
    }

    /// Sends a command to the page. Without a canvas (headless tests) it is
    /// recorded instead; see [`Self::sent_commands`].
    fn send(&mut self, body: CommandBody, cx: &mut Context<Self>) {
        match &self.canvas {
            Some(canvas) => {
                canvas.update(cx, |canvas, cx| {
                    canvas.send(body, cx);
                });
            }
            None => self.sent.push(body),
        }
    }

    /// The commands a canvas-less workspace would have sent the page.
    pub fn sent_commands(&self) -> &[CommandBody] {
        &self.sent
    }

    fn show_selection(
        &mut self,
        document: &Entity<DocumentView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selection = document.read(cx).document().selection.clone();
        if self.inspector.read(cx).selection() != selection.as_ref() {
            self.inspector.update(cx, |inspector, cx| {
                inspector.set_selection(selection, window, cx)
            });
        }
    }

    pub fn canvas(&self) -> Option<&Entity<Canvas>> {
        self.canvas.as_ref()
    }

    pub fn document_view(&self) -> Option<&Entity<DocumentView>> {
        self.document.as_ref()
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

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(document) = &self.document {
            document.update(cx, |document, cx| document.save(window, cx));
        }
    }

    fn render_title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let action = |id: &'static str, label: &'static str, enabled: bool, body: CommandBody| {
            Button::new(id)
                .ghost()
                .small()
                .label(label)
                .disabled(!enabled)
                .on_click(cx.listener(move |this, _, _, cx| this.send(body.clone(), cx)))
        };
        let (title, dirty, can_save, can_undo, can_redo): (SharedString, bool, bool, bool, bool) =
            match &self.document {
                Some(document) => {
                    let document = document.read(cx).document();
                    (
                        document.title.clone().into(),
                        document.dirty,
                        document.can_save(),
                        document.can_undo,
                        document.can_redo,
                    )
                }
                None => ("Untitled".into(), false, false, false, false),
            };
        // The kit's own close button (client-side decorations) asks the talk too.
        let closing = self.document.clone();

        TitleBar::new()
            .on_close_window(move |_, window, cx| {
                let close_now = closing.as_ref().is_none_or(|document| {
                    document.update(cx, |document, cx| document.request_close(window, cx))
                });
                if close_now {
                    window.remove_window();
                }
            })
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .id(TITLE_ID)
                            .test_support()
                            .aria_label(title.clone())
                            .text_sm()
                            .child(title),
                    )
                    .child(
                        div()
                            .id("dirty")
                            .test_support()
                            .aria_label("Unsaved changes")
                            .text_sm()
                            .when(!dirty, |this| this.invisible())
                            .child("•"),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .pr_2()
                    .items_center()
                    .child(action("undo", "Undo", can_undo, CommandBody::Undo))
                    .child(action("redo", "Redo", can_redo, CommandBody::Redo))
                    .child(
                        Button::new("save")
                            .ghost()
                            .small()
                            .label("Save")
                            .disabled(!can_save)
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
    }
}

impl Render for WorkspaceView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let canvas = div()
            .id(CANVAS_ID)
            .test_support()
            .flex_1()
            .size_full()
            .bg(cx.theme().muted)
            .map(|this| match (&self.document, &self.canvas) {
                (Some(document), _) => this.child(document.clone()),
                (None, Some(canvas)) => this.child(canvas.clone()),
                (None, None) => this,
            });

        let inspector = div()
            .id(INSPECTOR_ID)
            .test_support()
            .size_full()
            .bg(cx.theme().background)
            .child(self.inspector.clone());

        v_flex()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(|this, _: &Save, window, cx| this.save(window, cx)))
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(
                h_resizable("workspace")
                    .child(resizable_panel().child(canvas))
                    .child(
                        resizable_panel()
                            .size(INSPECTOR_WIDTH)
                            .fixed()
                            .child(inspector),
                    ),
            )
    }
}
