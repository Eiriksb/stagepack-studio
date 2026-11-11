use std::path::PathBuf;

use gpui_kit::assets::{Assets, icon_assets};
use gpui_kit::*;
use stagepack_studio::recovery::{KEEP_FOR, Recovery};
use stagepack_studio::workspace_view;

// The editor's tools show these, beyond the icons that come with the components.
icon_assets!(ToolIcons, [SquarePlus, CopyPlus, Trash, Presentation, Timer, CodeXml]);

/// The components' icons and the tools'.
struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if let Some(bytes) = ToolIcons.load(path)? {
            return Ok(Some(bytes));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = Assets.list(path)?;
        paths.extend(ToolIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

fn main() {
    // `stagepack-studio <folder or .sptx file>` opens that talk; with no argument, ask.
    let path = std::env::args_os().nth(1).map(PathBuf::from);
    let recovery = Recovery::default_root().map(Recovery::new);
    if let Some(recovery) = recovery.clone() {
        // Working copies of talks not opened again in a long while.
        std::thread::spawn(move || recovery.sweep(KEEP_FOR));
    }

    gpui_kit::application()
        .with_assets(AppAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            workspace_view::bind_keys(cx);
            match path {
                Some(path) => open_talk(path, recovery, cx),
                None => ask_for_folder(recovery, cx),
            }
        });
}

fn open_talk(path: PathBuf, recovery: Option<Recovery>, cx: &mut App) {
    workspace_view::open_talk_window(path, recovery, cx).expect("Failed to open the window");
}

fn ask_for_folder(recovery: Option<Recovery>, cx: &mut App) {
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
            Some(folder) => open_talk(folder, recovery, cx),
            None => cx.quit(),
        });
    })
    .detach();
}
