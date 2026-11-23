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
    /// Its absolute path, which is valid text.
    pub path: PathBuf,
    /// Whether the talk chose the file: the word is written as a path, which a talk can aim
    /// anywhere, or the file is inside the talk's folder.
    pub in_talk: bool,
}

/// Why a name isn't a program that can be run: one plain sentence.
fn missing(word: &str) -> String {
    format!("There's no program called \"{word}\" here.")
}

/// Whether `path`, already canonical, is a file that is run as itself. On Windows that is a
/// program (`.exe` or `.com`) and nothing else: a script (`.bat`, `.cmd`) is run through
/// `cmd.exe`, which reads its arguments again, so what is shown wouldn't be what runs.
fn is_program(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() || path.to_str().is_none() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(windows)]
    {
        path.extension().and_then(OsStr::to_str).is_some_and(|extension| extension.eq_ignore_ascii_case("exe") || extension.eq_ignore_ascii_case("com"))
    }
    #[cfg(not(any(unix, windows)))]
    {
        true
    }
}

/// Whether `word` is written as a path. On Windows a colon makes it one (`C:foo` is a place on a
/// drive, not a name).
fn is_path(word: &str) -> bool {
    word.contains('/') || (cfg!(windows) && (word.contains('\\') || word.contains(':')))
}

/// `given` taken from `folder`, part by part, so that `.` and `..` are settled in the words and
/// not left to the system (a verbatim Windows path doesn't settle them). A path that names a
/// drive or a root of its own isn't relative.
fn from_folder(folder: &Path, given: &Path) -> Option<PathBuf> {
    use std::path::Component;
    let mut full = folder.to_path_buf();
    for part in given.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                full.pop();
            }
            Component::Normal(name) => full.push(name),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(full)
}

/// The program `word` names, for a talk whose folder is `folder` (canonical) if it has one, with
/// `path_var` as the PATH. A word written as a path is a path, taken from the talk's folder if it
/// isn't absolute; a bare name is looked for in the absolute entries of the PATH only, none of
/// which may be inside the talk.
pub fn resolve_program(word: &str, folder: Option<&Path>, path_var: Option<&OsStr>) -> Result<Program, String> {
    let inside = |path: &Path| folder.is_some_and(|folder| path.starts_with(folder));
    if is_path(word) {
        let given = PathBuf::from(word);
        let full = if given.is_absolute() {
            given
        } else {
            from_folder(folder.ok_or_else(|| missing(word))?, &given).ok_or_else(|| missing(word))?
        };
        let path = full.canonicalize().map_err(|_| missing(word))?;
        if !path.is_absolute() || !is_program(&path) {
            return Err(missing(word));
        }
        return Ok(Program { in_talk: true, path });
    }
    if word.is_empty() {
        return Err(missing(word));
    }
    let comes_with_talk = || {
        format!("The program \"{word}\" would be one that comes with this talk, not one of yours, so it isn't run.")
    };
    for entry in std::env::split_paths(path_var.unwrap_or_default()) {
        if !entry.is_absolute() {
            continue;
        }
        // A folder of the PATH inside the talk, or a link to one, is the talk's, whatever is in it.
        if entry.canonicalize().is_ok_and(|real| inside(&real)) {
            return Err(comes_with_talk());
        }
        let mut names = vec![word.to_string()];
        if cfg!(windows) {
            names.push(format!("{word}.exe"));
        }
        for name in names {
            let found = entry.join(name);
            let Ok(real) = found.canonicalize() else {
                continue;
            };
            if !found.is_absolute() || !is_program(&real) {
                continue;
            }
            if inside(&real) {
                return Err(comes_with_talk());
            }
            if found.to_str().is_none() {
                return Err(missing(word));
            }
            return Ok(Program { path: found, in_talk: false });
        }
    }
    Err(missing(word))
}
