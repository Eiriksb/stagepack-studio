//! Stagepack Studio: a native editor for Markdown slide talks.

pub mod added_files;
pub mod addons;
pub mod ai_tool;
pub mod animate;
pub mod animate_tools;
pub mod app_assets;
pub mod arrange;
pub mod arrange_tools;
pub mod builds;
pub mod cargo_file;
pub mod clipboard;
pub mod clipboard_tools;
pub mod document;
pub mod export;
pub mod history;
pub mod icon_picker;
pub mod insert;
pub mod insert_tools;
mod interactive;
pub mod large_files;
pub mod live_terminal;
pub mod inspector;
pub mod layout_picker;
pub mod markdown_panel;
pub mod model_view;
pub mod notes_panel;
pub mod palette;
pub mod permissions;
pub mod present;
pub mod present_view;
pub mod recovery;
pub mod replay;
pub mod ribbon;
pub mod save_flow;
pub mod saving;
pub mod selection;
pub mod slide_canvas;
pub mod slide_editor;
pub mod slide_strip;
pub mod snap;
pub mod start_screen;
pub mod table_tools;
pub mod templates;
pub mod text_editing;
pub mod text_tools;
pub mod video_player;
pub mod watcher;
pub mod workspace_view;
pub mod working_copies;

/// What both programs do: `stagepack-studio` (a windows-subsystem program when shipped on Windows) and
/// `stagepack-studio-console` (always a console program, staged on Windows as `stagepack-studio.com`).
/// Runs a command and exits, or opens the app.
pub fn run_main() {
    // `stagepack-studio outline TALK` and the other commands print and end, with no window.
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if let Some(status) = crate::ai_tool::run_command(&args) {
        // A drawer a picture or a check started is let go of its device before the end.
        crate::model_view::close_drawer(std::time::Duration::from_secs(10));
        crate::model_view::wait_closed();
        std::process::exit(status);
    }
    // `stagepack-studio <folder or .sptx file>` opens that talk; with no argument, the start
    // screen.
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let recovery = recovery::Recovery::default_root().map(recovery::Recovery::new);
    if let Some(recovery) = recovery.clone() {
        // Working copies of talks not opened again in a long while.
        std::thread::spawn(move || recovery.sweep(recovery::KEEP_FOR));
    }

    gpui_kit::application()
        .with_assets(app_assets::AppAssets)
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
    crate::model_view::wait_closed();
}
