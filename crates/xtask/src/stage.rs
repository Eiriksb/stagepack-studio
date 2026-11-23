//! `package stage`: the installed app's folder, made from shipped release builds.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The most the installed folder may take (the design's budget), in megabytes of 1024 x 1024 bytes.
pub const BUDGET_MB: u64 = 600;

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

/// The repository's folder.
pub fn repo_root() -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("the repository folder exists");
    // Windows spells a canonical path with a `\\?\` prefix that the compiler never prints, so a remap of it would match nothing.
    match path.to_str().and_then(|p| p.strip_prefix(r"\\?\")) {
        Some(plain) if cfg!(windows) => PathBuf::from(plain),
        _ => path,
    }
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
    // In a target folder of their own, with the paths of the machine that builds them written
    // out of them, so that nothing of the builder's home ends up in a shipped program.
    // On Linux cargo runs in a memory-limited scope; Windows has no such wrapper, so the cargo that ran `package` builds.
    let cargo = if cfg!(windows) { std::env::var_os("CARGO").map_or_else(|| PathBuf::from("cargo"), PathBuf::from) } else { repo.join("scripts/cargo-safe.sh") };
    let target = repo.join("target/package-build");
    let flags = path_free_flags(&repo);
    let build = |package: &str, features: &[&str]| {
        let mut command = Command::new(&cargo);
        command
            .current_dir(&repo)
            .env("CARGO_ENCODED_RUSTFLAGS", &flags)
            .args(["build", "--release", "-p", package, "--target-dir"])
            .arg(&target)
            .args(features);
        command
    };
    // On Windows the console twin is built with it; on Linux the one program is both.
    let app = if cfg!(windows) {
        build("stagepack-studio", &["--features", "shipped,windows-console", "--bin", "stagepack-studio", "--bin", "stagepack-studio-console"])
    } else {
        build("stagepack-studio", &["--features", "shipped"])
    };
    run(&mut { app }, "building the app")?;
    run(&mut build("slides-terminal-helper", &[]), "building the helper")?;
    let release = target.join("release");
    for name in ["stagepack-studio", "stagepack-terminal"] {
        let file = format!("{name}{EXE}");
        std::fs::copy(release.join(&file), out.join(&file)).map_err(|e| format!("can't copy {file}: {e}"))?;
    }

    if cfg!(windows) {
        // What a shell waits for: a console program of the same name that the shell finds first (PATHEXT tries `.COM` before `.EXE`).
        std::fs::copy(release.join("stagepack-studio-console.exe"), out.join("stagepack-studio.com")).map_err(|e| format!("can't copy stagepack-studio.com: {e}"))?;
    }

    match bundle {
        Some(made) => copy_tree(made, &out.join("bundle"))?,
        None => run(&mut bundle_command(&repo, &out.join("bundle")), "making the bundle")?,
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

/// The command that makes the compiler bundle in `dest`: `make-bundle.sh` on Linux, `make-bundle.ps1` on Windows
/// (run by PowerShell 7 if it is installed, else the PowerShell that comes with Windows).
fn bundle_command(repo: &Path, dest: &Path) -> Command {
    let mut command;
    if cfg!(windows) {
        let shell = if Command::new("pwsh").arg("-Version").output().is_ok() { "pwsh" } else { "powershell" };
        command = Command::new(shell);
        command.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]).arg(repo.join("scripts/make-bundle.ps1")).arg("-Out").arg(dest);
    } else {
        command = Command::new("bash");
        command.arg(repo.join("scripts/make-bundle.sh")).arg(dest);
    }
    command.current_dir(repo);
    command
}

/// Rust flags that write the builder's own folders out of what's built: the cargo home, where
/// the libraries' sources are, the repository, and the toolchain's home, each as a short stand-in.
pub fn path_free_flags(repo: &Path) -> String {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    let cargo_home = std::env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".cargo")));
    let rustup_home = std::env::var_os("RUSTUP_HOME").map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".rustup")));
    let mut flags = Vec::new();
    for (from, to) in [(cargo_home, "/cargo"), (rustup_home, "/rustup"), (Some(repo.to_path_buf()), "/studio")] {
        if let Some(from) = from {
            flags.push(format!("--remap-path-prefix={}={to}", from.display()));
        }
    }
    flags.join("\x1f")
}
