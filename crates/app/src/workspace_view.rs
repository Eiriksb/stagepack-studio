//! The window's content: the title bar, the canvas and the inspector slot.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, TitleBar, h_flex, h_resizable,
    resizable_panel, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::canvas::Canvas;

pub const CANVAS_ID: &str = "canvas";
pub const INSPECTOR_ID: &str = "inspector";
pub const TITLE_ID: &str = "title";

pub const INSPECTOR_WIDTH: Pixels = px(280.);

pub struct WorkspaceView {
    /// `None` in headless tests, which can't host a web view.
    canvas: Option<Entity<Canvas>>,
    file_name: SharedString,
    dirty: bool,
}

impl WorkspaceView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let canvas = cx.new(|cx| Canvas::new(window, cx));
        Self::with_canvas(Some(canvas))
    }

    /// The same layout with a plain div where the web view goes.
    pub fn for_test(_cx: &mut Context<Self>) -> Self {
        Self::with_canvas(None)
    }

    fn with_canvas(canvas: Option<Entity<Canvas>>) -> Self {
        Self {
            canvas,
            file_name: "Untitled".into(),
            dirty: false,
        }
    }

    pub fn canvas(&self) -> Option<&Entity<Canvas>> {
        self.canvas.as_ref()
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

    fn render_title_bar(&self) -> impl IntoElement {
        let action = |id: &'static str, label: &'static str| {
            Button::new(id).ghost().small().label(label).disabled(true)
        };

        TitleBar::new()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .id(TITLE_ID)
                            .test_support()
                            .aria_label(self.file_name.clone())
                            .text_sm()
                            .child(self.file_name.clone()),
                    )
                    .child(
                        div()
                            .id("dirty")
                            .test_support()
                            .aria_label("Unsaved changes")
                            .text_sm()
                            .when(!self.dirty, |this| this.invisible())
                            .child("•"),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .pr_2()
                    .items_center()
                    .child(action("undo", "Undo"))
                    .child(action("redo", "Redo"))
                    .child(action("save", "Save")),
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
            .when_some(self.canvas.clone(), |this, canvas| this.child(canvas));

        let inspector = div()
            .id(INSPECTOR_ID)
            .test_support()
            .size_full()
            .bg(cx.theme().background);

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar())
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
