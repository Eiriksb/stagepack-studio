//! `package linux`: wraps the staged app folder with the install scripts into a `.tar.zst`.

use crate::stage::{copy_tree, repo_root};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The files in `packaging/linux` that go beside the app in the package (the scripts are made executable).
const PACKAGING_FILES: [&str; 4] = ["install.sh", "uninstall.sh", "stagepack-studio.desktop.in", "stagepack-studio-mime.xml"];

/// The app's version, from `crates/app/Cargo.toml`.
pub fn version() -> Result<String, String> {
    let manifest = repo_root().join("crates/app/Cargo.toml");
    let text = std::fs::read_to_string(&manifest).map_err(|e| format!("can't read {}: {e}", manifest.display()))?;
    // The package's own `version = "..."` line comes first; dependencies' versions sit inside braces.
    text.lines()
        .find_map(|line| line.strip_prefix("version = \"")?.split('"').next().map(str::to_string))
        .ok_or_else(|| format!("no version in {}", manifest.display()))
}

/// The package's file name for this version.
pub fn tarball_name(version: &str) -> String {
    format!("stagepack-studio-{version}-linux-x86_64.tar.zst")
}

/// Makes the package's folder `dest` from the staged app: its files (hard-linked where the disk allows,
/// to save time and space) plus the install scripts, the menu entry template and the file type.
pub fn assemble(staged: &Path, dest: &Path) -> Result<(), String> {
    if !staged.join("stagepack-studio").is_file() {
        return Err(format!("{} isn't a staged app folder (run `package stage` first)", staged.display()));
    }
    if dest.exists() {
        std::fs::remove_dir_all(dest).map_err(|e| format!("can't clear {}: {e}", dest.display()))?;
    }
    std::fs::create_dir_all(dest.parent().ok_or("no parent folder")?).map_err(|e| e.to_string())?;
    let linked = Command::new("cp").arg("-al").arg(staged).arg(dest).status().is_ok_and(|s| s.success());
    if !linked {
        let _ = std::fs::remove_dir_all(dest);
        copy_tree(staged, dest)?;
    }
    let packaging = repo_root().join("packaging/linux");
    for file in PACKAGING_FILES {
        let target = dest.join(file);
        let _ = std::fs::remove_file(&target); // never write through a hard link into the staged folder
        std::fs::copy(packaging.join(file), &target).map_err(|e| format!("can't copy {file}: {e}"))?;
        #[cfg(unix)]
        if file.ends_with(".sh") {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Packs the staged app into `out_dir/stagepack-studio-<version>-linux-x86_64.tar.zst`, one top folder
/// `stagepack-studio-<version>`, and returns the file's path.
pub fn pack(staged: &Path, out_dir: &Path) -> Result<PathBuf, String> {
    let version = version()?;
    let out_dir = std::path::absolute(out_dir).map_err(|e| e.to_string())?;
    let top = format!("stagepack-studio-{version}");
    let work = out_dir.join("work");
    assemble(staged, &work.join(&top))?;
    let tarball = out_dir.join(tarball_name(&version));
    let _ = std::fs::remove_file(&tarball);
    let status = Command::new("tar")
        .args(["--owner=0", "--group=0", "--numeric-owner", "--use-compress-program=zstd -19 -T0", "-cf"])
        .arg(&tarball)
        .arg("-C")
        .arg(&work)
        .arg(&top)
        .status()
        .map_err(|e| format!("couldn't run tar: {e}"))?;
    let _ = std::fs::remove_dir_all(&work);
    if !status.success() {
        let _ = std::fs::remove_file(&tarball);
        return Err(format!("tar failed ({status})"));
    }
    Ok(tarball)
}
