// The shipped program on Windows has no console window of its own, so that opening a talk from Explorer shows no terminal.
// A command run from a terminal attaches to that terminal (see `ai_tool::attach_parent_console`); `stagepack-studio.com`
// (the console twin, `src/bin/stagepack-studio-console.rs`) is what a shell waits for and takes the exit status from.
#![cfg_attr(all(windows, feature = "shipped"), windows_subsystem = "windows")]

fn main() {
    stagepack_studio::run_main();
}
