//! `package windows-folder`: the installed app's folder on Windows, which Inno Setup then wraps.

use std::path::Path;

/// The sentence for asking for the Windows folder anywhere but Windows.
pub const NOT_WINDOWS: &str = "The Windows folder is staged on Windows.";

/// Stages the folder exactly as `stage` does (the program names take `.exe` and the bundle is made
/// by `make-bundle.ps1` when this runs on Windows); anywhere else it refuses. Returns the folder's size in bytes.
pub fn windows_folder(out: &Path, bundle: Option<&Path>) -> Result<u64, String> {
    if !cfg!(windows) {
        return Err(NOT_WINDOWS.into());
    }
    crate::stage::stage(out, bundle)
}
