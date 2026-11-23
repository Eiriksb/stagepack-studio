//! The yes a talk needs before it runs a program. The app keeps it, never the talk: a talk is
//! untrusted, and can't give itself permission. A yes is for one exact command, for the session
//! of one talk in one window; or, for a talk kept in a folder only, remembered for that folder
//! and that command in the app's own data folder.

use std::ffi::OsStr;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use slides_engine::write_atomically;
use slides_terminal::{MOST_COMMAND_BYTES, MOST_COMMAND_WORDS};
use slides_terminal_helper::wire::StartSpec;

use super::resolve::{Program, resolve_program};
use super::Session;
use crate::document::TalkPlace;

/// The file's name in the app's data folder.
const FILE_NAME: &str = "terminal-permissions.json";

/// One thing remembered: this command, in this folder.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Remembered {
    /// The folder by its canonical path.
    folder: PathBuf,
    /// The command's words, exactly.
    command: Vec<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct Stored {
    always_run: Vec<Remembered>,
}

/// A talk's folder, for a talk kept in one: the only place a yes can be remembered for. There is
/// none for a packed or untitled talk, so asking to remember one can't be written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TalkFolder(PathBuf);

impl TalkFolder {
    /// The folder of `talk`, by its canonical path, if it's kept in one.
    pub fn of(talk: &TalkPlace) -> Option<TalkFolder> {
        match talk {
            TalkPlace::Folder(folder) => folder.canonicalize().ok().map(TalkFolder),
            TalkPlace::Packed(_) | TalkPlace::Untitled(_) => None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

/// The commands the user has said can always run, each in one folder.
pub struct RunPermissions {
    file: PathBuf,
    always_run: Vec<Remembered>,
}

impl RunPermissions {
    /// The permissions kept in `file`. A file that is missing or can't be read counts as no
    /// permissions at all.
    pub fn load(file: PathBuf) -> RunPermissions {
        RunPermissions { always_run: read(&file).always_run, file }
    }

    /// The file these permissions are kept in.
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// Where the app keeps its permissions: in its own data folder, if the system has one.
    pub fn default_file() -> Option<PathBuf> {
        Some(crate::permissions::BuildPermissions::app_folder()?.join(FILE_NAME))
    }

    /// Whether `command`, exactly, is remembered for `folder`, which is the same folder however
    /// it's written.
    pub fn always(&self, folder: &Path, command: &[String]) -> bool {
        folder.canonicalize().is_ok_and(|folder| self.always_run.iter().any(|one| one.folder == folder && one.command == command))
    }

    /// Says from now on that `command` runs in `folder` without asking, and writes that down.
    pub fn remember(&mut self, folder: &TalkFolder, command: &[String]) -> io::Result<()> {
        let one = Remembered { folder: folder.0.canonicalize()?, command: command.to_vec() };
        // Another window may have added one since this one read the file.
        let mut stored = read(&self.file);
        for known in &self.always_run {
            if !stored.always_run.contains(known) {
                stored.always_run.push(known.clone());
            }
        }
        if !stored.always_run.contains(&one) {
            stored.always_run.push(one);
        }
        let json = serde_json::to_vec_pretty(&stored).map_err(io::Error::other)?;
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_atomically(&self.file, |file| file.write_all(&json))?;
        self.always_run = stored.always_run;
        Ok(())
    }
}

fn read(file: &Path) -> Stored {
    std::fs::read(file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// What a yes is for: the words of a command with the program as the file it resolves to, and the
/// folder it runs in (none for a new empty folder). A command that resolves elsewhere, or is run
/// somewhere else, is another thing to say yes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunKey {
    pub command: Vec<String>,
    pub folder: Option<PathBuf>,
}

/// What this talk, in this window, has been told about commands: held by the window, not by the
/// talk's files. A talk opened in a window gets a new one, and nothing carries over between
/// windows.
#[derive(Default)]
pub struct SessionYes {
    allowed: Vec<RunKey>,
    refused: Vec<RunKey>,
}

impl SessionYes {
    pub fn new() -> SessionYes {
        SessionYes::default()
    }

    /// Whether `key`, exactly, has been said yes to.
    pub fn allowed(&self, key: &RunKey) -> bool {
        self.allowed.contains(key)
    }

    /// Says yes to `key`, exactly, for the rest of the session.
    pub fn allow(&mut self, key: &RunKey) {
        self.refused.retain(|one| one != key);
        if !self.allowed(key) {
            self.allowed.push(key.clone());
        }
    }

    /// Says no to `key`, exactly, for the rest of the session: it isn't asked again.
    pub fn refuse(&mut self, key: &RunKey) {
        self.allowed.retain(|one| one != key);
        if !self.refused(key) {
            self.refused.push(key.clone());
        }
    }

    /// Whether `key`, exactly, has been said no to.
    pub fn refused(&self, key: &RunKey) -> bool {
        self.refused.contains(key)
    }
}

/// What may be done about running a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// It has a yes: it may run.
    Run,
    /// It has no yes yet: ask.
    Ask,
    /// It must not run, and isn't asked about: it was said no to, or it isn't a command that can
    /// be run.
    Refused,
}

/// Whether `c` is one that can hide: a control, a format character (bidirectional marks and
/// overrides, zero-width characters, the byte order mark), or a space of any kind but the plain one.
pub(super) fn hides(c: char) -> bool {
    c.is_control()
        || (c.is_whitespace() && c != ' ')
        || matches!(c as u32,
            0xAD | 0x600..=0x605 | 0x61C | 0x6DD | 0x70F | 0x8E2 | 0x180E | 0x200B..=0x200F | 0x2028..=0x202E
            | 0x2060..=0x2064 | 0x2066..=0x206F | 0xFEFF | 0xFFF9..=0xFFFB | 0x110BD | 0x1D173..=0x1D17A
            | 0xE0001 | 0xE0020..=0xE007F)
}

/// Why the words of a command can't be run, if they can't: it must be what it looks like, so no
/// word may hold anything that hides, and a space may only stand singly between visible characters.
fn unrunnable(command: &[String]) -> Option<String> {
    let bytes = command.iter().map(String::len).sum::<usize>() + command.len().saturating_sub(1);
    if command.is_empty() || command.len() > MOST_COMMAND_WORDS || bytes > MOST_COMMAND_BYTES || command[0].is_empty() {
        return Some("The command to run can't be run.".to_string());
    }
    for word in command {
        let spaced = word.starts_with(' ') || word.ends_with(' ') || word.contains("  ");
        if word.chars().any(hides) || spaced {
            return Some(
                "The command to run has a space or a character that can't be seen, so it isn't run. Write it plainly.".to_string(),
            );
        }
    }
    None
}

/// A command worked out for a talk: what it runs, where, and what a yes is a yes to.
#[derive(Clone, Debug)]
pub struct Plan {
    key: RunKey,
    program: Program,
    folder: Option<TalkFolder>,
}

impl Plan {
    /// What the yes is for.
    pub fn key(&self) -> &RunKey {
        &self.key
    }

    /// The talk's folder, if it's kept in one: where it runs, and where *Always* is kept.
    pub fn folder(&self) -> Option<&TalkFolder> {
        self.folder.as_ref()
    }

    /// Whether the program is a file that comes with the talk.
    pub fn program_in_talk(&self) -> bool {
        self.program.in_talk
    }

    /// The words as the question shows them: the program as the file that runs, as `<talk>/name`
    /// if the talk brings it, then the arguments as the command has them.
    pub fn shown_words(&self) -> Vec<String> {
        let first = match (&self.folder, self.program.in_talk) {
            (Some(folder), true) => {
                let inside = self.program.path.strip_prefix(folder.path()).unwrap_or(&self.program.path);
                format!("<talk>/{}", inside.display())
            }
            _ => self.program.path.display().to_string(),
        };
        std::iter::once(first).chain(self.key.command[1..].iter().cloned()).collect()
    }
}

/// Works out `command`, the words of a talk's `run`, for `talk`, with `path_var` as the PATH. The
/// error is why it can't be run, in one plain sentence.
pub fn plan_with_path(talk: &TalkPlace, command: &[String], path_var: Option<&OsStr>) -> Result<Plan, String> {
    if let Some(why) = unrunnable(command) {
        return Err(why);
    }
    let folder = match talk {
        TalkPlace::Folder(_) => Some(TalkFolder::of(talk).ok_or("This talk's folder can't be found, so nothing is run.")?),
        TalkPlace::Packed(_) | TalkPlace::Untitled(_) => None,
    };
    let program = resolve_program(&command[0], folder.as_ref().map(TalkFolder::path), path_var)?;
    let mut words = command.to_vec();
    words[0] = program.path.display().to_string();
    let key = RunKey { command: words, folder: folder.as_ref().map(|folder| folder.path().to_path_buf()) };
    Ok(Plan { key, program, folder })
}

/// [`plan_with_path`] with the app's own PATH.
pub fn plan(talk: &TalkPlace, command: &[String]) -> Result<Plan, String> {
    plan_with_path(talk, command, std::env::var_os("PATH").as_deref())
}

fn decide(plan: &Plan, session: &SessionYes, remembered: &RunPermissions) -> Decision {
    if session.refused(&plan.key) {
        return Decision::Refused;
    }
    if session.allowed(&plan.key) {
        return Decision::Run;
    }
    match &plan.folder {
        Some(folder) if remembered.always(folder.path(), &plan.key.command) => Decision::Run,
        _ => Decision::Ask,
    }
}

/// Whether `command`, the words a talk's `run` splits into, may run for `talk` now, with `path_var`
/// as the PATH. `Run` only from a yes given in this session, or, for a talk kept in a folder only,
/// a permission remembered for that folder and that program and those exact words.
pub fn may_run_with_path(
    talk: &TalkPlace,
    command: &[String],
    path_var: Option<&OsStr>,
    session: &SessionYes,
    remembered: &RunPermissions,
) -> Decision {
    match plan_with_path(talk, command, path_var) {
        Ok(plan) => decide(&plan, session, remembered),
        Err(_) => Decision::Refused,
    }
}

/// [`may_run_with_path`] with the app's own PATH.
pub fn may_run(talk: &TalkPlace, command: &[String], session: &SessionYes, remembered: &RunPermissions) -> Decision {
    may_run_with_path(talk, command, std::env::var_os("PATH").as_deref(), session, remembered)
}

/// A folder made for a program to run in, removed when this goes.
struct Scratch(PathBuf);

impl Scratch {
    fn make() -> io::Result<Scratch> {
        static MADE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let count = MADE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_nanos());
        let folder = std::env::temp_dir().join(format!("stagepack-terminal-{}-{nanos}-{count}", std::process::id()));
        std::fs::create_dir(&folder)?;
        Ok(Scratch(folder))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The right to start one program, which only a yes makes: what runs, where, and how, as that yes
/// was given. [`Session::start`](super::Session::start) wants one, and only this file can make one.
pub struct Permit {
    spec: StartSpec,
    scratch: Option<Scratch>,
}

impl Permit {
    pub(super) fn into_parts(self) -> (StartSpec, Option<ScratchFolder>) {
        (self.spec, self.scratch.map(ScratchFolder))
    }

    /// What this permit starts: its program, its arguments, where and on what screen.
    pub fn spec(&self) -> &StartSpec {
        &self.spec
    }

    /// A permit for a test of the session itself, which runs whatever it's given. Nothing else may
    /// call this: the source is searched for it.
    #[doc(hidden)]
    pub fn unchecked_for_tests(spec: StartSpec) -> Permit {
        Permit { spec, scratch: None }
    }
}

/// The folder a session runs its program in, if it made one, held for as long as the session lives.
pub struct ScratchFolder(#[allow(dead_code)] Scratch);

/// What the app's own environment gives a program besides its own: the terminal it runs in.
fn terminal_env(cols: u16, rows: u16) -> Vec<(String, String)> {
    vec![
        ("TERM".to_string(), "xterm-256color".to_string()),
        ("COLUMNS".to_string(), cols.to_string()),
        ("LINES".to_string(), rows.to_string()),
    ]
}

/// The right to start `command` for `talk`, on a screen `cols` by `rows`, only if [`may_run`] says
/// `Run`; otherwise what it says. Where it runs and what it's given are worked out here from the
/// talk, never taken from the caller: the talk's folder for a folder talk, a new empty folder for
/// the others, the program as the file the yes was for, and the app's own environment with the
/// terminal's.
pub fn permit(
    talk: &TalkPlace,
    command: &[String],
    session: &SessionYes,
    remembered: &RunPermissions,
    (cols, rows): (u16, u16),
) -> Result<Permit, Decision> {
    let plan = plan(talk, command).map_err(|_| Decision::Refused)?;
    match decide(&plan, session, remembered) {
        Decision::Run => {}
        other => return Err(other),
    }
    let (cwd, scratch) = match &plan.folder {
        Some(folder) => (folder.path().to_path_buf(), None),
        None => {
            let scratch = Scratch::make().map_err(|_| Decision::Refused)?;
            (scratch.0.clone(), Some(scratch))
        }
    };
    let spec = StartSpec {
        program: plan.program.path.display().to_string(),
        args: plan.key.command[1..].to_vec(),
        cwd,
        cols,
        rows,
        env: terminal_env(cols, rows),
    };
    Ok(Permit { spec, scratch })
}

/// Starts `command` with the helper at `helper`, if there's a yes for it: the one place a program
/// is started.
pub fn start_after_yes(
    talk: &TalkPlace,
    command: &[String],
    session: &SessionYes,
    remembered: &RunPermissions,
    helper: &Path,
    size: (u16, u16),
) -> Result<Session, Decision> {
    permit(talk, command, session, remembered, size).map(|permit| Session::start(helper, permit))
}
