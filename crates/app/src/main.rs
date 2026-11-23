use std::path::PathBuf;

use stagepack_studio::app_assets::AppAssets;
use stagepack_studio::recovery::{KEEP_FOR, Recovery};
use stagepack_studio::workspace_view;

fn main() {
    // `stagepack-studio outline TALK` and the other commands print and end, with no window.
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if let Some(status) = stagepack_studio::ai_tool::run_command(&args) {
        stagepack_studio::model_view::wait_closed();
        std::process::exit(status);
    }
    // `stagepack-studio <folder or .sptx file>` opens that talk; with no argument, the start
    // screen.
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
            workspace_view::init(cx);
            let opened = match path {
                Some(path) => workspace_view::open_talk_window(path, recovery, cx),
                None => workspace_view::open_start_window(recovery, cx),
            };
            opened.expect("Failed to open the window");
        });
    // The windows are gone: the drawer of 3D models is let go of its device before the end.
    stagepack_studio::model_view::wait_closed();
}
