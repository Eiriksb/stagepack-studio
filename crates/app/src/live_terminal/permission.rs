//! The yes a talk needs before it runs a program. The app keeps it, never the talk: a talk is
//! untrusted, and can't give itself permission. A yes is for one exact command, for the session
//! of one talk in one window; or, for a talk kept in a folder only, remembered for that folder
//! and that command in the app's own data folder.
//!
//! Text a button types into a live terminal has a yes of its own, kept the same way: for that text,
//! exactly, into a terminal running that command, exactly. A yes to run the command is never one to
//! type into it.

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

/// One text a button may type, remembered: this text, into a terminal running this command, in
/// this folder.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RememberedTyped {
    folder: PathBuf,
    /// The terminal's command, its words exactly as the talk gives them.
    terminal_command: Vec<String>,
    text: String,
}

/// The file as it's written. A key it doesn't know is passed over when it's read, so a file
/// written by a newer app reads here, and the `typed` an older app doesn't know is passed over by
/// it. A key that's missing is empty.
#[derive(Default, Serialize, Deserialize)]
struct Stored {
    #[serde(default)]
    always_run: Vec<Remembered>,
    #[serde(default)]
    typed: Vec<RememberedTyped>,
}

impl Stored {
    /// `self` with what `other` has that it hasn't.
    fn merge(&mut self, other: &Stored) {
        for known in &other.always_run {
            if !self.always_run.contains(known) {
                self.always_run.push(known.clone());
            }
        }
        for known in &other.typed {
            if !self.typed.contains(known) {
                self.typed.push(known.clone());
            }
        }
    }
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

/// How a folder is written to a program and to the person: on Windows a path that `canonicalize` made
/// verbatim (`\\?\C:\talk`) is written plainly (`C:\talk`) when it can be, as `cmd.exe` and other
/// programs refuse the verbatim form as their folder. Elsewhere, and for any path that has no plain
/// form, it is the path as it is. This only changes how the folder is spelled: what is checked and what
/// a yes is for stay the canonical path.
pub fn spelled_plainly(path: &Path) -> PathBuf {
    #[cfg(windows)]
    if let Some(plain) = path.to_str().and_then(plain_drive_path) {
        return PathBuf::from(plain);
    }
    path.to_path_buf()
}

/// `\\?\C:\folder` as `C:\folder`, if it is a verbatim path to a folder on a drive that can be written
/// plainly: no part of it a reserved device name, or ending in a dot or a space, or with a character a
/// plain path can't have, and short enough to be one (under 260 characters). Anything else comes back as
/// `None`.
pub fn plain_drive_path(verbatim: &str) -> Option<String> {
    let rest = verbatim.strip_prefix(r"\\?\")?;
    let mut chars = rest.chars();
    let (drive, colon) = (chars.next()?, chars.next()?);
    if !drive.is_ascii_alphabetic() || colon != ':' {
        return None;
    }
    let after = &rest[2..];
    if !after.is_empty() && !after.starts_with('\\') {
        return None;
    }
    if rest.len() >= 260 {
        return None;
    }
    const RESERVED: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];
    for part in after.split('\\').filter(|part| !part.is_empty()) {
        if part.chars().any(|c| c.is_control() || "<>\"/|?*:".contains(c)) || part.ends_with('.') || part.ends_with(' ') {
            return None;
        }
        let stem = part.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
        let numbered = |name: &str| stem.len() == 4 && stem.starts_with(name) && matches!(stem.as_bytes()[3], b'1'..=b'9');
        if RESERVED.contains(&stem.as_str()) || numbered("COM") || numbered("LPT") {
            return None;
        }
    }
    // A bare drive gets its root.
    Some(if after.is_empty() { format!("{rest}\\") } else { rest.to_string() })
}

/// The commands the user has said can always run, each in one folder, and the texts a button may
/// always type into a terminal, each in one folder.
pub struct RunPermissions {
    file: PathBuf,
    stored: Stored,
}

impl RunPermissions {
    /// The permissions kept in `file`. A file that is missing or can't be read counts as no
    /// permissions at all.
    pub fn load(file: PathBuf) -> RunPermissions {
        RunPermissions { stored: read(&file), file }
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
        folder.canonicalize().is_ok_and(|folder| self.stored.always_run.iter().any(|one| one.folder == folder && one.command == command))
    }

    /// Whether `text`, exactly, may always be typed into a terminal running `terminal_command`,
    /// exactly, in `folder`.
    pub fn always_typed(&self, folder: &Path, text: &str, terminal_command: &[String]) -> bool {
        folder.canonicalize().is_ok_and(|folder| {
            self.stored.typed.iter().any(|one| one.folder == folder && one.text == text && one.terminal_command == terminal_command)
        })
    }

    /// Says from now on that `command` runs in `folder` without asking, and writes that down.
    pub fn remember(&mut self, folder: &TalkFolder, command: &[String]) -> io::Result<()> {
        let one = Remembered { folder: folder.0.canonicalize()?, command: command.to_vec() };
        self.write_with(Stored { always_run: vec![one], typed: Vec::new() })
    }

    /// Says from now on that `text` is typed without asking into a terminal running
    /// `terminal_command` in `folder`, and writes that down.
    pub fn remember_typed(&mut self, folder: &TalkFolder, text: &str, terminal_command: &[String]) -> io::Result<()> {
        let one = RememberedTyped { folder: folder.0.canonicalize()?, terminal_command: terminal_command.to_vec(), text: text.to_string() };
        self.write_with(Stored { always_run: Vec::new(), typed: vec![one] })
    }

    /// Writes the file with what it has, what this has, and `new`.
    fn write_with(&mut self, new: Stored) -> io::Result<()> {
        // Another window may have added one since this one read the file.
        let mut stored = read(&self.file);
        stored.merge(&self.stored);
        stored.merge(&new);
        let json = serde_json::to_vec_pretty(&stored).map_err(io::Error::other)?;
        if let Some(parent) = self.file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_atomically(&self.file, |file| file.write_all(&json))?;
        self.stored = stored;
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
    pub(crate) command: Vec<String>,
    pub(crate) folder: Option<PathBuf>,
}

impl RunKey {
    /// The words, the program as the file it resolves to.
    pub fn command(&self) -> &[String] {
        &self.command
    }
}

/// What this talk, in this window, has been told about commands: held by the window, not by the
/// talk's files. A talk opened in a window gets a new one, and nothing carries over between
/// windows.
#[derive(Default)]
pub struct SessionYes {
    allowed: Vec<RunKey>,
    refused: Vec<RunKey>,
    /// Typed text is said yes or no to on its own: a yes to run a terminal's command is never one
    /// to type into it.
    typed_allowed: Vec<TypeKey>,
    typed_refused: Vec<TypeKey>,
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
    pub(crate) fn allow(&mut self, key: &RunKey) {
        self.refused.retain(|one| one != key);
        if !self.allowed(key) {
            self.allowed.push(key.clone());
        }
    }

    /// Says no to `key`, exactly, for the rest of the session: it isn't asked again.
    pub(crate) fn refuse(&mut self, key: &RunKey) {
        self.allowed.retain(|one| one != key);
        if !self.refused(key) {
            self.refused.push(key.clone());
        }
    }

    /// [`Self::allow`], for tests, which have no one to ask.
    #[cfg(feature = "test-hooks")]
    pub fn allow_for_tests(&mut self, key: &RunKey) {
        self.allow(key);
    }

    /// [`Self::refuse`], for tests.
    #[cfg(feature = "test-hooks")]
    pub fn refuse_for_tests(&mut self, key: &RunKey) {
        self.refuse(key);
    }

    /// Whether `key`, exactly, has been said no to.
    pub fn refused(&self, key: &RunKey) -> bool {
        self.refused.contains(key)
    }

    /// Says yes to typing what `key` is for, exactly, for the rest of the session.
    pub(crate) fn allow_typed(&mut self, key: &TypeKey) {
        self.typed_refused.retain(|one| one != key);
        if !self.typed_allowed.contains(key) {
            self.typed_allowed.push(key.clone());
        }
    }

    /// Says no to typing what `key` is for, exactly, for the rest of the session.
    pub(crate) fn refuse_typed(&mut self, key: &TypeKey) {
        self.typed_allowed.retain(|one| one != key);
        if !self.typed_refused.contains(key) {
            self.typed_refused.push(key.clone());
        }
    }

    /// [`Self::allow_typed`] for `text` into a terminal running `terminal_command` in `talk`, for
    /// tests. It does nothing for a text that can't be typed.
    #[cfg(feature = "test-hooks")]
    pub fn allow_typed_for_tests(&mut self, talk: &TalkPlace, text: &str, terminal_command: &[String]) {
        if let Ok(key) = type_key(talk, text, terminal_command) {
            self.allow_typed(&key);
        }
    }

    /// [`Self::refuse_typed`], for tests.
    #[cfg(feature = "test-hooks")]
    pub fn refuse_typed_for_tests(&mut self, talk: &TalkPlace, text: &str, terminal_command: &[String]) {
        if let Ok(key) = type_key(talk, text, terminal_command) {
            self.refuse_typed(&key);
        }
    }
}

/// What a yes to typed text is for: this text, exactly, into a terminal running this command,
/// exactly, in this folder (none for a packed or untitled talk). Another text, another terminal
/// command or another folder is another thing to say yes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TypeKey {
    text: String,
    terminal_command: Vec<String>,
    folder: Option<PathBuf>,
}

impl TypeKey {
    /// The text, exactly.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// The terminal's command, its words exactly as the talk gives them.
    pub(crate) fn terminal_command(&self) -> &[String] {
        &self.terminal_command
    }
}

/// The most bytes a button may type.
pub const MOST_TYPED_BYTES: usize = 4096;
pub const TYPED_CONTROL: &str = "This button's text has a control character.";
pub const TYPED_TOO_LONG: &str = "This button's text is over 4,096 bytes.";
pub const TYPED_HIDES: &str = "This button's text has a character that can't be seen, so it isn't typed. Write it plainly.";

/// Why `text` can't be typed into a terminal by a button, if it can't: it may hold no control
/// character but a tab, be no more than [`MOST_TYPED_BYTES`], and hold nothing that hides or
/// stacks, as [`unrunnable`] refuses in a command's words.
pub fn untypable(text: &str) -> Option<&'static str> {
    if text.chars().any(|c| c.is_control() && c != '\t') {
        return Some(TYPED_CONTROL);
    }
    if text.len() > MOST_TYPED_BYTES {
        return Some(TYPED_TOO_LONG);
    }
    if text.chars().any(|c| c != '\t' && hides(c)) || stacked(text) {
        return Some(TYPED_HIDES);
    }
    None
}

/// What a yes to typing `text` into a terminal running `terminal_command` for `talk` is for, or
/// why it can't be typed.
pub(crate) fn type_key(talk: &TalkPlace, text: &str, terminal_command: &[String]) -> Result<TypeKey, String> {
    if let Some(why) = untypable(text) {
        return Err(why.to_string());
    }
    if let Some(why) = unrunnable(terminal_command) {
        return Err(why);
    }
    let folder = match talk {
        TalkPlace::Folder(_) => Some(TalkFolder::of(talk).ok_or("This talk's folder can't be found, so nothing is typed.")?.0),
        TalkPlace::Packed(_) | TalkPlace::Untitled(_) => None,
    };
    Ok(TypeKey { text: text.to_string(), terminal_command: terminal_command.to_vec(), folder })
}

/// Whether a button may type `text` into the live terminal running `terminal_command` for `talk`
/// now. As [`may_run`]: `Run` only from a yes given in this session to this text into this
/// terminal command, or, for a talk kept in a folder only, one remembered for that folder; a yes
/// to run the terminal's command is never one to type into it. A text that can't be typed is
/// `Refused`, and [`untypable`] says why.
pub fn may_type(talk: &TalkPlace, text: &str, terminal_command: &[String], session: &SessionYes, remembered: &RunPermissions) -> Decision {
    let Ok(key) = type_key(talk, text, terminal_command) else {
        return Decision::Refused;
    };
    if session.typed_refused.contains(&key) {
        return Decision::Refused;
    }
    if session.typed_allowed.contains(&key) {
        return Decision::Run;
    }
    match &key.folder {
        Some(folder) if remembered.always_typed(folder, &key.text, &key.terminal_command) => Decision::Run,
        _ => Decision::Ask,
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

/// The Default_Ignorable_Code_Point property: characters that draw as nothing, or as blank space.
/// From DerivedCoreProperties.txt of Unicode 16.0 (the version of the `unicode-properties`
/// tables this app uses), written out as ranges.
const DEFAULT_IGNORABLE: &[(u32, u32)] = &[
    (0x00AD, 0x00AD),
    (0x034F, 0x034F),
    (0x061C, 0x061C),
    (0x115F, 0x1160),
    (0x17B4, 0x17B5),
    (0x180B, 0x180F),
    (0x200B, 0x200F),
    (0x202A, 0x202E),
    (0x2060, 0x206F),
    (0x3164, 0x3164),
    (0xFE00, 0xFE0F),
    (0xFEFF, 0xFEFF),
    (0xFFA0, 0xFFA0),
    (0xFFF0, 0xFFF8),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0xE0000, 0xE0FFF),
];

/// Whether `c` is one that can hide: one that is Default_Ignorable, the blank braille pattern, or
/// by its general category a control, a format character, a line or paragraph separator, any
/// space but the plain one, or a private-use or surrogate code point.
pub(super) fn hides(c: char) -> bool {
    use unicode_properties::{GeneralCategory as Category, UnicodeGeneralCategory as _};
    let code = c as u32;
    if code == 0x2800 || DEFAULT_IGNORABLE.iter().any(|&(first, last)| (first..=last).contains(&code)) {
        return true;
    }
    match c.general_category() {
        Category::Control | Category::Format | Category::LineSeparator | Category::ParagraphSeparator | Category::PrivateUse | Category::Surrogate => true,
        Category::SpaceSeparator => c != ' ',
        _ => c.is_whitespace() && c != ' ',
    }
}

/// Whether `c` is a combining mark that draws over the character before it.
fn combining(c: char) -> bool {
    use unicode_properties::{GeneralCategory as Category, UnicodeGeneralCategory as _};
    matches!(c.general_category(), Category::NonspacingMark | Category::EnclosingMark)
}

/// The most combining marks that may follow one another.
const MOST_COMBINING: usize = 2;

/// Whether `word` has more than [`MOST_COMBINING`] combining marks in a row, which draw over
/// whatever is near.
fn stacked(word: &str) -> bool {
    let mut run = 0;
    for c in word.chars() {
        run = if combining(c) { run + 1 } else { 0 };
        if run > MOST_COMBINING {
            return true;
        }
    }
    false
}

/// Why the words of a command can't be run, if they can't: it must be what it looks like, so no
/// word may hold anything that hides, and a space may only stand singly between visible characters.
fn unrunnable(command: &[String]) -> Option<String> {
    let bytes = command.iter().map(String::len).sum::<usize>() + command.len().saturating_sub(1);
    if command.is_empty() || command.len() > MOST_COMMAND_WORDS || bytes > MOST_COMMAND_BYTES || command[0].is_empty() {
        return Some("The command to run can't be run.".to_string());
    }
    // cmd and the C runtime read a quote differently, so what is shown inside one could run as more.
    if cfg!(windows) && command[1..].iter().any(|word| word.contains('"')) {
        return Some("On Windows an argument can't hold a double quote, so the command isn't run.".to_string());
    }
    for word in command {
        let spaced = word.starts_with(' ') || word.ends_with(' ') || word.contains("  ");
        if word.chars().any(hides) || spaced || stacked(word) {
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

    /// Whether the program is a file inside the talk's folder.
    pub fn program_in_talk(&self) -> bool {
        self.folder.as_ref().is_some_and(|folder| super::resolve::within(&self.program.path, folder.path()))
    }

    /// Whether the talk names the program by a path to a file outside its folder.
    pub fn program_named_by_path(&self) -> bool {
        self.program.in_talk && !self.program_in_talk()
    }

    /// The words as the question shows them: the program as the file that runs, as `<talk>/name`
    /// if the talk brings it, then the arguments as the command has them.
    pub fn shown_words(&self) -> Vec<String> {
        let inside = self.folder.as_ref().and_then(|folder| self.program.path.strip_prefix(folder.path()).ok());
        let first = match inside {
            Some(inside) => format!("<talk>/{}", inside.display()),
            None => self.program.path.display().to_string(),
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
        TalkPlace::Folder(_) => Some(
            TalkFolder::of(talk)
                .filter(|folder| folder.path().to_str().is_some())
                .ok_or("This talk's folder can't be found, so nothing is run.")?,
        ),
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

    /// A permit for a test of the session itself, which runs whatever it's given. It isn't in the
    /// app: only a build with the `test-hooks` feature has it.
    #[cfg(feature = "test-hooks")]
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
        Some(folder) => (spelled_plainly(folder.path()), None),
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
