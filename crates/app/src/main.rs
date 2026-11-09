use gpui_kit::*;
use stagepack_studio::workspace_view::WorkspaceView;

fn main() {
    // WebKitGTK embeds only into X11 windows, so a Wayland session goes through XWayland.
    #[cfg(target_os = "linux")]
    let app = gpui_kit::platform::linux(WindowingModes::X11);
    #[cfg(not(target_os = "linux"))]
    let app = gpui_kit::application();

    // Debug builds only: load this URL in the canvas and skip everything else.
    let dev_url = if cfg!(debug_assertions) {
        std::env::var("STUDIO_DEV_URL")
            .ok()
            .filter(|url| !url.is_empty())
    } else {
        None
    };

    app.with_assets(gpui_kit::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);

        let options = WorkspaceView::window_options(cx);
        gpui_kit::open_window(options, cx, |window, cx| {
            let view = cx.new(|cx| WorkspaceView::new(window, cx));
            if let Some(url) = &dev_url
                && let Some(canvas) = view.read(cx).canvas().cloned()
            {
                canvas.update(cx, |canvas, cx| canvas.load_url(url, cx));
            }
            view
        })
        .expect("Failed to open the window");
        cx.activate(true);
    });
}
