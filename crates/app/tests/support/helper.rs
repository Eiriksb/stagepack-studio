//! Finding the live terminal's helper for the tests that run it.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The helper, found in the target folder beside the test's own program, or built there once.
pub fn helper_program() -> PathBuf {
    static HELPER: OnceLock<PathBuf> = OnceLock::new();
    HELPER
        .get_or_init(|| {
            let exe = std::env::current_exe().unwrap();
            let target = exe.parent().and_then(Path::parent).unwrap().to_path_buf();
            let helper = target.join("stagepack-terminal");
            let release = target.file_name().is_some_and(|name| name == "release");
            let mut build = std::process::Command::new(std::env!("CARGO"));
            build.args(["build", "-p", "slides-terminal-helper"]);
            if release {
                build.arg("--release");
            }
            let status = build.status().expect("cargo can be run");
            assert!(status.success(), "the helper couldn't be built");
            assert!(helper.is_file(), "no helper at {}", helper.display());
            helper
        })
        .clone()
}
