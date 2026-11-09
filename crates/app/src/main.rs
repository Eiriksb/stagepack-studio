use std::path::PathBuf;

use gpui_kit::*;
use stagepack_studio::workspace_view::{self, WorkspaceView};

fn main() {
    // `stagepack-studio <folder>` opens the talk in that folder; with no argument, ask.
    let folder = std::env::args_os().nth(1).map(PathBuf::from);

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            workspace_view::bind_keys(cx);
            match folder {
                Some(folder) => open_talk(folder, cx),
                None => ask_for_folder(cx),
            }
        });
}

fn open_talk(folder: PathBuf, cx: &mut App) {
    let options = WorkspaceView::window_options(cx);
    gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| WorkspaceView::open(folder, window, cx))
    })
    .expect("Failed to open the window");
    cx.activate(true);
}

fn ask_for_folder(cx: &mut App) {
    let chosen = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Open".into()),
    });
    cx.spawn(async move |cx| {
        let folder = match chosen.await {
            Ok(Ok(Some(mut paths))) if !paths.is_empty() => Some(paths.remove(0)),
            Ok(Err(err)) => {
                eprintln!("stagepack-studio: could not show the Open dialog: {err}");
                None
            }
            _ => None,
        };
        cx.update(|cx| match folder {
            Some(folder) => open_talk(folder, cx),
            None => cx.quit(),
        });
    })
    .detach();
}
