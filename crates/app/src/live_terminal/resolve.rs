//! Which program a command really runs. The app works this out itself, before asking, so that the
//! yes is for the file that will run and not for a name: a bare name is searched for only in the
//! absolute folders of the PATH (never in a relative one, which would be inside the talk), and a
//! name that resolves inside the talk's own folder is refused. What is sent to the helper is the
//! absolute path, so nothing is searched for again.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The program a command's first word names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// Its absolute path.
    pub path: PathBuf,
    /// Whether it's a file that comes with the talk, inside the talk's folder.
    pub in_talk: bool,
}

/// Why a name isn't a program that can be run: one plain sentence.
fn missing(word: &str) -> String {
    format!("There's no program called \"{word}\" here.")
}

fn is_program(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn has_separator(word: &str) -> bool {
    word.contains('/') || (cfg!(windows) && word.contains('\\'))
}

/// The program `word` names, for a talk whose folder is `folder` (canonical) if it has one, with
/// `path_var` as the PATH. A word with a separator is a path, taken from the talk's folder if it
/// isn't absolute; a bare name is looked for in the absolute entries of the PATH only.
pub fn resolve_program(word: &str, folder: Option<&Path>, path_var: Option<&OsStr>) -> Result<Program, String> {
    let inside = |path: &Path| folder.is_some_and(|folder| path.starts_with(folder));
    if has_separator(word) {
        let given = PathBuf::from(word);
        let full = if given.is_absolute() {
            given
        } else {
            folder.ok_or_else(|| missing(word))?.join(given)
        };
        let path = full.canonicalize().map_err(|_| missing(word))?;
        if !is_program(&path) {
            return Err(missing(word));
        }
        return Ok(Program { in_talk: inside(&path), path });
    }
    if word.is_empty() {
        return Err(missing(word));
    }
    for entry in std::env::split_paths(path_var.unwrap_or_default()) {
        if !entry.is_absolute() {
            continue;
        }
        let mut names = vec![word.to_string()];
        if cfg!(windows) {
            names.push(format!("{word}.exe"));
        }
        for name in names {
            let found = entry.join(name);
            if !is_program(&found) {
                continue;
            }
            let real = found.canonicalize().map_err(|_| missing(word))?;
            if inside(&real) {
                return Err(format!(
                    "The program \"{word}\" would be one that comes with this talk, not one of yours, so it isn't run."
                ));
            }
            return Ok(Program { path: found, in_talk: false });
        }
    }
    Err(missing(word))
}
