use std::path::PathBuf;

use gpui_kit::*;
use stagepack_studio::document_view;
use stagepack_studio::workspace_view::{self, WorkspaceView};

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
    // `stagepack-studio <file.sptx>` opens that talk; with no argument, ask.
    let path = std::env::args_os().nth(1).map(PathBuf::from);

    app.with_assets(gpui_kit::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        workspace_view::bind_keys(cx);

        // One window per run. Closing it quits, but only once its sidecar has
        // stopped, so serve can stop Slidev and clean up first.
        cx.set_quit_mode(QuitMode::Explicit);
        cx.on_window_closed(|cx, _| {
            #[cfg(target_os = "linux")]
            stagepack_studio::canvas::window_closing_on_x11();
            if cx.windows().is_empty() {
                document_view::quit_when_stopped(cx);
            }
        })
        .detach();

        match (dev_url, path) {
            (Some(url), _) => open_dev_url(&url, cx),
            (None, Some(path)) => open_talk(path, cx),
            (None, None) => ask_for_talk(cx),
        }
    });
}

fn open_dev_url(url: &str, cx: &mut App) {
    let options = WorkspaceView::window_options(cx);
    gpui_kit::open_window(options, cx, |window, cx| {
        let view = cx.new(|cx| WorkspaceView::new(window, cx));
        if let Some(canvas) = view.read(cx).canvas().cloned() {
            canvas.update(cx, |canvas, cx| canvas.load_url(url, cx));
        }
        view
    })
    .expect("Failed to open the window");
    cx.activate(true);
}

fn open_talk(path: PathBuf, cx: &mut App) {
    let path = std::path::absolute(&path).unwrap_or(path);
    let options = WorkspaceView::window_options(cx);
    gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| WorkspaceView::open(path, window, cx))
    })
    .expect("Failed to open the window");
    cx.activate(true);
}

/// File > Open. The portal's picker takes no file-type filter, so any file
/// can be chosen and serve refuses what isn't a talk.
fn ask_for_talk(cx: &mut App) {
    let chosen = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Open".into()),
    });
    cx.spawn(async move |cx| {
        let path = match chosen.await {
            Ok(Ok(Some(mut paths))) if !paths.is_empty() => Some(paths.remove(0)),
            Ok(Err(err)) => {
                eprintln!("stagepack-studio: could not show the Open dialog: {err}");
                None
            }
            _ => None,
        };
        cx.update(|cx| match path {
            Some(path) => open_talk(path, cx),
            None => cx.quit(),
        });
    })
    .detach();
}
