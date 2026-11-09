//! The window's content: the title bar and the canvas area.

use gpui_kit::component::{ActiveTheme as _, TitleBar, h_flex, v_flex};
use gpui_kit::*;

pub const CANVAS_ID: &str = "canvas";
pub const TITLE_ID: &str = "title";

pub struct WorkspaceView;

impl WorkspaceView {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self
    }

    /// The same view for headless tests.
    pub fn for_test(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::new(window, cx)
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
}

impl Render for WorkspaceView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title: SharedString = "Untitled".into();
        v_flex()
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
                    .flex_1()
                    .w_full()
                    .bg(cx.theme().muted),
            )
    }
}
