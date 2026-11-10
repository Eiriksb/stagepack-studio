use std::path::PathBuf;

use gpui_kit::*;
use stagepack_studio::workspace_view;

fn main() {
    // `stagepack-studio <folder or .sptx file>` opens that talk; with no argument, ask.
    let path = std::env::args_os().nth(1).map(PathBuf::from);

    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            workspace_view::bind_keys(cx);
            match path {
                Some(path) => open_talk(path, cx),
                None => ask_for_folder(cx),
            }
        });
}

fn open_talk(path: PathBuf, cx: &mut App) {
    workspace_view::open_talk_window(path, cx).expect("Failed to open the window");
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
