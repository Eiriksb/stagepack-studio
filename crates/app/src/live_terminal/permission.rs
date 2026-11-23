//! The yes a talk needs before it runs a program. The app keeps it, never the talk: a talk is
//! untrusted, and can't give itself permission. A yes is for one exact command, for the session
//! of one talk in one window; or, for a talk kept in a folder only, remembered for that folder
//! and that command in the app's own data folder.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use slides_engine::write_atomically;
use slides_terminal::{MOST_COMMAND_BYTES, MOST_COMMAND_WORDS};
use slides_terminal_helper::wire::StartSpec;

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

/// What this talk, in this window, has been told about commands: held by the window, not by the
/// talk's files. A talk opened in a window gets a new one, and nothing carries over between
/// windows.
#[derive(Default)]
pub struct SessionYes {
    allowed: Vec<Vec<String>>,
    refused: Vec<Vec<String>>,
}

impl SessionYes {
    pub fn new() -> SessionYes {
        SessionYes::default()
    }

    /// Whether `command`, exactly, has been said yes to.
    pub fn allowed(&self, command: &[String]) -> bool {
        self.allowed.iter().any(|one| one == command)
    }

    /// Says yes to `command`, exactly, for the rest of the session.
    pub fn allow(&mut self, command: &[String]) {
        self.refused.retain(|one| one != command);
        if !self.allowed(command) {
            self.allowed.push(command.to_vec());
        }
    }

    /// Says no to `command`, exactly, for the rest of the session: it isn't asked again.
    pub fn refuse(&mut self, command: &[String]) {
        self.allowed.retain(|one| one != command);
        if !self.refused(command) {
            self.refused.push(command.to_vec());
        }
    }

    /// Whether `command`, exactly, has been said no to.
    pub fn refused(&self, command: &[String]) -> bool {
        self.refused.iter().any(|one| one == command)
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

/// Whether the words of a command are ones that can be run at all, as `split_command` gives them.
fn runnable(command: &[String]) -> bool {
    let bytes = command.iter().map(String::len).sum::<usize>() + command.len().saturating_sub(1);
    !command.is_empty() && command.len() <= MOST_COMMAND_WORDS && bytes <= MOST_COMMAND_BYTES && !command[0].is_empty()
}

/// Whether `command`, the words a talk's `run` splits into, may run for `talk` now. `Run` only
/// from a yes given in this session, or, for a talk kept in a folder only, a permission
/// remembered for that folder and those exact words. Everything that starts a program goes
/// through this.
pub fn may_run(talk: &TalkPlace, command: &[String], session: &SessionYes, remembered: &RunPermissions) -> Decision {
    if !runnable(command) || session.refused(command) {
        return Decision::Refused;
    }
    if session.allowed(command) {
        return Decision::Run;
    }
    match TalkFolder::of(talk) {
        Some(folder) if remembered.always(folder.path(), command) => Decision::Run,
        _ => Decision::Ask,
    }
}

/// Starts `command` with the helper at `helper`, in the folder and on the screen `place` says,
/// only if [`may_run`] says `Run`; otherwise what it says, and nothing is started. What runs is
/// `command`, whatever `place` has as its program and arguments. This is the one place a program
/// is started, so nothing can start one without a yes.
pub fn start_after_yes(
    talk: &TalkPlace,
    command: &[String],
    session: &SessionYes,
    remembered: &RunPermissions,
    helper: &Path,
    place: StartSpec,
) -> Result<Session, Decision> {
    match may_run(talk, command, session, remembered) {
        Decision::Run => {}
        other => return Err(other),
    }
    let spec = StartSpec { program: command[0].clone(), args: command[1..].to_vec(), ..place };
    Ok(Session::start(helper, spec))
}
