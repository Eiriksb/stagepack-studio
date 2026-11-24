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

/// Why the file system mustn't be asked about `path` at all, if it mustn't: asking about a network
/// path makes the computer connect to it, and on Windows send the owner's credentials, before
/// anyone has said yes. On Windows only a path on a drive is touched; on macOS the automounted
/// network folders are left alone, whatever their letters' case. `path` is absolute and settled
/// (see [`settle`]), so `..` can't lead round this.
fn unsafe_to_touch(path: &Path) -> bool {
    #[cfg(feature = "test-hooks")]
    if FORBIDDEN_FOR_TESTS.with(|forbidden| forbidden.borrow().iter().any(|base| within(path, base))) {
        return true;
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        !matches!(
            path.components().next(),
            Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))
        )
    }
    #[cfg(target_os = "macos")]
    {
        use std::path::Component;
        let mut parts = path.components();
        parts.next() == Some(Component::RootDir)
            && parts.next().is_some_and(|first| first.as_os_str().eq_ignore_ascii_case("net") || first.as_os_str().eq_ignore_ascii_case("Network"))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = path;
        false
    }
}

#[cfg(feature = "test-hooks")]
thread_local! {
    static FORBIDDEN_FOR_TESTS: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Makes the folders `forbidden` count as network folders on this thread, for tests, which have
/// no network folder to stand in for one. Only a build with the `test-hooks` feature has it.
#[cfg(feature = "test-hooks")]
#[doc(hidden)]
pub fn forbid_for_tests(forbidden: Vec<PathBuf>) {
    FORBIDDEN_FOR_TESTS.with(|list| *list.borrow_mut() = forbidden);
}

/// The most links one path may go through, as many as Linux allows.
const MOST_LINKS: usize = 40;

/// Why a path couldn't be settled.
#[derive(Debug, PartialEq, Eq)]
enum Unsettled {
    /// It is, or leads to, a place not to touch.
    Network,
    /// It isn't there, goes through too many links, or is written in a way that isn't followed.
    Missing,
}

/// One step of a path being settled.
enum Step {
    /// Start again from this root (`/`, or a drive's root on Windows).
    Root(PathBuf),
    Up,
    Name(std::ffi::OsString),
}

/// The steps of `path`, with `.` and `..` settled in the words first (`a/../b` is `b`, whatever
/// `a` is), or none if it's written relative to a drive or a root it doesn't name (`C:foo` or
/// `\foo` on Windows), which isn't followed. Only a relative path can start with `..`, which is
/// then taken from where it's found.
fn steps(path: &Path) -> Option<Vec<Step>> {
    use std::path::Component;
    let mut steps = Vec::new();
    let mut parts = path.components().peekable();
    if let Some(Component::Prefix(prefix)) = parts.peek().copied() {
        parts.next();
        if parts.next() != Some(Component::RootDir) {
            return None;
        }
        let mut root = PathBuf::from(prefix.as_os_str());
        root.push(Component::RootDir.as_os_str());
        steps.push(Step::Root(root));
    }
    for part in parts {
        match part {
            Component::RootDir if steps.is_empty() && !cfg!(windows) => steps.push(Step::Root(PathBuf::from("/"))),
            Component::RootDir | Component::Prefix(_) => return None,
            Component::CurDir => {}
            Component::ParentDir => match steps.last() {
                Some(Step::Name(_)) => {
                    steps.pop();
                }
                // Above the root is the root.
                Some(Step::Root(_)) => {}
                Some(Step::Up) | None => steps.push(Step::Up),
            },
            Component::Normal(name) => steps.push(Step::Name(name.to_os_string())),
        }
    }
    Some(steps)
}

/// `path`, which is absolute, as the place it really is, worked out one part at a time: `.` and
/// `..` are settled in the words, and each link is read and followed by hand, at most
/// [`MOST_LINKS`] of them. Before anything is asked of the file system, the place it would be
/// asked about is checked with [`unsafe_to_touch`], and so is each link's target before it's
/// followed, so a link or a `..` can't lead to a network folder. This is what both the network
/// check and the talk's-folder check are made on.
fn settle(path: &Path) -> Result<PathBuf, Unsettled> {
    if !path.is_absolute() {
        return Err(Unsettled::Missing);
    }
    let mut todo: std::collections::VecDeque<Step> = steps(path).ok_or(Unsettled::Missing)?.into();
    let mut here = PathBuf::new();
    let mut links = 0;
    while let Some(step) = todo.pop_front() {
        match step {
            Step::Root(root) => {
                if unsafe_to_touch(&root) {
                    return Err(Unsettled::Network);
                }
                here = root;
            }
            // At the root, `..` is the root.
            Step::Up => {
                here.pop();
            }
            Step::Name(name) => {
                if here.as_os_str().is_empty() {
                    return Err(Unsettled::Missing);
                }
                let next = here.join(&name);
                if unsafe_to_touch(&next) {
                    return Err(Unsettled::Network);
                }
                let meta = std::fs::symlink_metadata(&next).map_err(|_| Unsettled::Missing)?;
                if !meta.file_type().is_symlink() {
                    here = next;
                    continue;
                }
                links += 1;
                if links > MOST_LINKS {
                    return Err(Unsettled::Missing);
                }
                let target = std::fs::read_link(&next).map_err(|_| Unsettled::Missing)?;
                if target.is_absolute() && unsafe_to_touch(&target) {
                    return Err(Unsettled::Network);
                }
                // A relative target is taken from the link's own folder, which is `here`.
                for step in steps(&target).ok_or(Unsettled::Missing)?.into_iter().rev() {
                    todo.push_front(step);
                }
            }
        }
    }
    Ok(here)
}

/// Whether `path` is `folder` or inside it, both settled. On macOS and Windows, where a name is
/// the same whatever its letters' case, the case doesn't count, and on Windows a drive is the same
/// drive written either way (`C:` or `\\?\C:`).
pub(crate) fn within(path: &Path, folder: &Path) -> bool {
    let mut parts = path.components();
    folder.components().all(|part| parts.next().is_some_and(|other| same_part(part, other)))
}

fn same_part(one: std::path::Component, other: std::path::Component) -> bool {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let drive = |part: Component| match part {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => Some(letter.to_ascii_uppercase()),
                _ => None,
            },
            _ => None,
        };
        if let (Some(one), Some(other)) = (drive(one), drive(other)) {
            return one == other;
        }
    }
    if cfg!(any(windows, target_os = "macos")) {
        one.as_os_str().eq_ignore_ascii_case(other.as_os_str())
    } else {
        one == other
    }
}

/// Why `word` isn't run, for a path that couldn't be settled.
fn unsettled(word: &str, why: Unsettled) -> String {
    match why {
        Unsettled::Network => format!("The program \"{word}\" is on a network folder, so it isn't run."),
        Unsettled::Missing => missing(word),
    }
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
    let inside = |path: &Path| folder.is_some_and(|folder| within(path, folder));
    if is_path(word) {
        let given = PathBuf::from(word);
        let full = if given.is_absolute() {
            given
        } else {
            from_folder(folder.ok_or_else(|| missing(word))?, &given).ok_or_else(|| missing(word))?
        };
        let path = settle(&full).map_err(|why| unsettled(word, why))?;
        if !is_program(&path) {
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
        // A folder that can't be settled, or leads to a network folder, is passed over.
        let Ok(real_entry) = settle(&entry) else {
            continue;
        };
        // A folder of the PATH inside the talk, or a link to one, is the talk's, whatever is in it.
        if inside(&real_entry) {
            return Err(comes_with_talk());
        }
        let mut names = vec![word.to_string()];
        if cfg!(windows) {
            names.push(format!("{word}.exe"));
        }
        for name in names {
            let found = entry.join(name);
            let Ok(real) = settle(&found) else {
                continue;
            };
            if !is_program(&real) {
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
