//! Where a shipped build looks for the compiler bundle and the live terminal's helper.
//! Run with and without `--features shipped`.

use std::sync::Mutex;

use stagepack_studio::builds::find_bundle;
use stagepack_studio::live_terminal::find_helper;

/// The tests change the environment, so they take turns.
static TURN: Mutex<()> = Mutex::new(());

/// A folder that looks like a bundle (a `bin/rustc` file) and a helper file, in a scratch folder.
fn fakes() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let bundle = dir.path().join("bundle");
    std::fs::create_dir_all(bundle.join("bin")).unwrap();
    let rustc = if cfg!(windows) { "rustc.exe" } else { "rustc" };
    std::fs::write(bundle.join("bin").join(rustc), "").unwrap();
    let helper = dir.path().join("helper");
    std::fs::write(&helper, "").unwrap();
    (dir, bundle, helper)
}

#[cfg(feature = "shipped")]
#[test]
fn a_shipped_build_ignores_the_variables() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let (_dir, bundle, helper) = fakes();
    // SAFETY: the other tests in this file hold the same lock while they use the environment.
    unsafe {
        std::env::set_var("STUDIO_BUNDLE", &bundle);
        std::env::set_var("STUDIO_TERMINAL_HELPER", &helper);
    }
    // Nothing is beside the test program, so a shipped build finds nothing.
    assert_eq!(find_bundle(), None);
    assert_eq!(find_helper(), None);
    unsafe {
        std::env::remove_var("STUDIO_BUNDLE");
        std::env::remove_var("STUDIO_TERMINAL_HELPER");
    }
}

#[cfg(not(feature = "shipped"))]
#[test]
fn a_development_build_still_reads_the_variables() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let (_dir, bundle, helper) = fakes();
    // SAFETY: the other tests in this file hold the same lock while they use the environment.
    unsafe {
        std::env::set_var("STUDIO_BUNDLE", &bundle);
        std::env::set_var("STUDIO_TERMINAL_HELPER", &helper);
    }
    assert_eq!(find_bundle().map(|b| b.root), Some(bundle));
    assert_eq!(find_helper(), Some(helper));
    unsafe {
        std::env::remove_var("STUDIO_BUNDLE");
        std::env::remove_var("STUDIO_TERMINAL_HELPER");
    }
}
