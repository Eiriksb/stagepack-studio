//! `package stage`: the installed app's folder, made from shipped release builds.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The most the installed folder may take (the design's budget), in megabytes of 1024 x 1024 bytes.
pub const BUDGET_MB: u64 = 600;

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

/// The repository's folder.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("the repository folder exists")
}

fn run(command: &mut Command, what: &str) -> Result<(), String> {
    let status = command.status().map_err(|e| format!("couldn't run {what}: {e}"))?;
    status.success().then_some(()).ok_or_else(|| format!("{what} failed ({status})"))
}

/// Copies a folder with everything in it: links stay links, and files keep their permissions.
pub fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    let fail = |what: &str, path: &Path, e: std::io::Error| format!("can't {what} {}: {e}", path.display());
    std::fs::create_dir_all(to).map_err(|e| fail("create", to, e))?;
    for entry in std::fs::read_dir(from).map_err(|e| fail("read", from, e))? {
        let entry = entry.map_err(|e| fail("read", from, e))?;
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type().map_err(|e| fail("read", &source, e))?;
        if kind.is_dir() {
            copy_tree(&source, &target)?;
        } else if kind.is_symlink() {
            #[cfg(unix)]
            {
                let link = std::fs::read_link(&source).map_err(|e| fail("read", &source, e))?;
                std::os::unix::fs::symlink(link, &target).map_err(|e| fail("link", &target, e))?;
            }
            #[cfg(not(unix))]
            std::fs::copy(&source, &target).map_err(|e| fail("copy", &source, e))?;
        } else {
            std::fs::copy(&source, &target).map_err(|e| fail("copy", &source, e))?;
        }
    }
    Ok(())
}

/// The bytes in the files under `folder` (a link counts as the link itself).
pub fn folder_bytes(folder: &Path) -> Result<u64, String> {
    let mut total = 0;
    for entry in std::fs::read_dir(folder).map_err(|e| format!("can't read {}: {e}", folder.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        total += if kind.is_dir() {
            folder_bytes(&entry.path())?
        } else {
            entry.metadata().map_err(|e| e.to_string())?.len()
        };
    }
    Ok(total)
}

/// The sentence for a folder that is too big.
pub fn over_budget(bytes: u64) -> String {
    format!("The app folder is {} MB, over the {BUDGET_MB} MB budget.", bytes.div_ceil(1024 * 1024))
}

/// Builds the shipped app and its helper in release, and gathers the folder in `out`: the two
/// programs, the compiler bundle (made here unless `bundle` names one already made), the guides,
/// the licence, the notices and the icons. Returns the folder's size in bytes, or says it's over
/// the budget.
pub fn stage(out: &Path, bundle: Option<&Path>) -> Result<u64, String> {
    let repo = repo_root();
    let out = std::path::absolute(out).map_err(|e| format!("can't resolve {}: {e}", out.display()))?;
    // The folder is replaced, so make sure it's one of ours.
    if std::fs::read_dir(&out).is_ok_and(|mut d| d.next().is_some()) && !out.join(format!("stagepack-studio{EXE}")).exists() {
        return Err(format!("{} has files in it and isn't an app folder, so it's been left alone", out.display()));
    }
    if out.exists() {
        std::fs::remove_dir_all(&out).map_err(|e| format!("can't clear {}: {e}", out.display()))?;
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("can't create {}: {e}", out.display()))?;

    // Built one at a time, so that the helper's pseudo-terminal code isn't linked into the app.
    let cargo = repo.join("scripts/cargo-safe.sh");
    run(
        Command::new(&cargo).current_dir(&repo).args(["build", "--release", "-p", "stagepack-studio", "--features", "shipped"]),
        "building the app",
    )?;
    run(Command::new(&cargo).current_dir(&repo).args(["build", "--release", "-p", "slides-terminal-helper"]), "building the helper")?;
    let release = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| repo.join("target")).join("release");
    for name in ["stagepack-studio", "stagepack-terminal"] {
        let file = format!("{name}{EXE}");
        std::fs::copy(release.join(&file), out.join(&file)).map_err(|e| format!("can't copy {file}: {e}"))?;
    }

    match bundle {
        Some(made) => copy_tree(made, &out.join("bundle"))?,
        None => run(Command::new("bash").current_dir(&repo).arg(repo.join("scripts/make-bundle.sh")).arg(out.join("bundle")), "making the bundle")?,
    }

    std::fs::create_dir_all(out.join("guide")).map_err(|e| e.to_string())?;
    for guide in ["components.md", "writing-talks-for-ai.md"] {
        std::fs::copy(repo.join("docs/guide").join(guide), out.join("guide").join(guide)).map_err(|e| format!("can't copy {guide}: {e}"))?;
    }
    std::fs::copy(repo.join("LICENSE"), out.join("LICENSE")).map_err(|e| format!("can't copy LICENSE: {e}"))?;
    crate::notices::write_notices(&out.join("NOTICES.txt"))?;
    crate::icon::write_icons(&out.join("icons"))?;

    let bytes = folder_bytes(&out)?;
    if bytes > BUDGET_MB * 1024 * 1024 {
        return Err(over_budget(bytes));
    }
    Ok(bytes)
}
