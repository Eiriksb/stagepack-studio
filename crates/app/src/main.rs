use gpui_kit::*;
use stagepack_studio::workspace_view::WorkspaceView;

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            let options = WorkspaceView::window_options(cx);
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| WorkspaceView::new(window, cx))
            })
            .expect("Failed to open the window");
            cx.activate(true);
        });
}
