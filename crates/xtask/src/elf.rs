//! Which glibc the Linux package's programs and libraries need, read from their dynamic symbols.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The newest glibc a shipped file may need: Ubuntu 22.04's, which Debian 12 (2.36) also meets.
pub const GLIBC_CEILING: (u32, u32) = (2, 35);

/// The highest `GLIBC_x.y` version named in `objdump -T` output, or `None` if it names none.
/// Only a `GLIBC_` followed by a digit counts, so `GLIBCXX_` (libstdc++) and `GLIBC_PRIVATE` don't.
pub fn parse_glibc_needs(objdump_output: &str) -> Option<(u32, u32)> {
    let mut highest = None;
    for (at, _) in objdump_output.match_indices("GLIBC_") {
        let rest = &objdump_output[at + "GLIBC_".len()..];
        let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
        let mut parts = rest[..end].split('.');
        let (Some(Ok(major)), Some(Ok(minor))) = (parts.next().map(str::parse::<u32>), parts.next().map(str::parse::<u32>)) else {
            continue;
        };
        highest = highest.max(Some((major, minor)));
    }
    highest
}

/// The highest glibc version `path` needs, from `objdump -T`, or `None` if it needs none.
pub fn max_glibc(path: &Path) -> Result<Option<(u32, u32)>, String> {
    let output = Command::new("objdump").arg("-T").arg(path).output().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => "objdump isn't installed (it comes with binutils: apt install binutils)".to_string(),
        _ => format!("couldn't run objdump: {e}"),
    })?;
    if !output.status.success() {
        return Err(format!("objdump -T {} failed: {}", path.display(), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(parse_glibc_needs(&String::from_utf8_lossy(&output.stdout)))
}

/// Every regular file under `folder` that starts with `\x7fELF`, sorted. Links are not followed.
pub fn elf_files(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() && starts_with_elf_magic(&entry.path()) {
                found.push(entry.path());
            }
        }
    }
    found.sort();
    found
}

fn starts_with_elf_magic(path: &Path) -> bool {
    let mut magic = [0u8; 4];
    std::fs::File::open(path).and_then(|mut file| file.read_exact(&mut magic)).is_ok() && magic == *b"\x7fELF"
}

/// A version as `2.35`.
pub fn version_text((major, minor): (u32, u32)) -> String {
    format!("{major}.{minor}")
}
