//! Finding the helper program.
use std::path::PathBuf;

/// The environment variable that names the helper, for tests and for those who keep it elsewhere.
pub const HELPER_ENV: &str = "STUDIO_TERMINAL_HELPER";

/// What the helper's file is called.
const HELPER_NAME: &str = if cfg!(windows) { "stagepack-terminal.exe" } else { "stagepack-terminal" };

/// Where the helper is: the file `STUDIO_TERMINAL_HELPER` names if it's set (and nowhere else if
/// that isn't a file), else beside the app's own program. It is never fetched.
///
/// A shipped build (the `shipped` feature) looks beside the app only and ignores the variable.
pub fn find_helper() -> Option<PathBuf> {
    if cfg!(feature = "shipped") {
        find_helper_beside_exe()
    } else {
        find_helper_anywhere()
    }
}

fn find_helper_anywhere() -> Option<PathBuf> {
    if let Some(named) = std::env::var_os(HELPER_ENV).filter(|named| !named.is_empty()) {
        let named = PathBuf::from(named);
        return named.is_file().then_some(named);
    }
    find_helper_beside_exe()
}

fn find_helper_beside_exe() -> Option<PathBuf> {
    let beside = std::env::current_exe().ok()?.parent()?.join(HELPER_NAME);
    beside.is_file().then_some(beside)
}
