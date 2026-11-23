//! The yes a talk needs before it runs a program: asked once a session, remembered only for one
//! folder and one command, never kept in a talk, shown in full, and impossible to go round.

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
    Decision, HELPER_ENV, RUN_ALWAYS_ID, RUN_ASK_ID, RUN_IN_NEW_FOLDER, RUN_LINE_ID, RUN_NO_ID, RUN_PROGRAM_IN_TALK, RUN_QUESTION, RUN_YES_ID,
    RunPermissions, SessionYes, may_run, may_run_with_path, permit, plan, plan_with_path, start_after_yes,
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
    session.allow(&key);
    assert_eq!(may_run(&talk, &sh, &session, &remembered), Decision::Run);
    assert_eq!(may_run(&talk, &words(&["sh", "-l"]), &session, &remembered), Decision::Ask, "another command asks again");
    assert_eq!(may_run(&talk, &words(&["ls"]), &session, &remembered), Decision::Ask);
    session.refuse(&key);
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
    session.allow(&key_of(&packed, &["sh"]));
    // A pack that is saved as a folder, or a talk moved to another folder, is asked about again.
    assert_eq!(may_run(&packed, &words(&["sh"]), &session, &remembered), Decision::Run);
    assert_eq!(may_run(&in_a, &words(&["sh"]), &session, &remembered), Decision::Ask);
    session.allow(&key_of(&in_a, &["sh"]));
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
    ];
    for word in hiding {
        let command = words(&["echo", word]);
        // Even a yes already given isn't one for it.
        let mut session = SessionYes::new();
        if let Ok(plan) = plan(&talk, &command) {
            session.allow(plan.key());
        }
        assert_eq!(may_run(&talk, &command, &session, &remembered), Decision::Refused, "{word:?}");
    }
    // An empty argument, one single space, and plain words are what a command is.
    for fine in [vec!["echo", ""], vec!["echo", "two words"], vec!["echo", "-n", "x y z"]] {
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
    remembered.remember(kept.folder().unwrap(), &kept.key().command).unwrap();
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
    remembered.remember(kept.folder().unwrap(), &kept.key().command).unwrap();
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
    assert!(again.always(&a, &kept.key().command) && !again.always(&b, &kept.key().command));
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
    let real = plan(&TalkPlace::Folder(talk.path().to_path_buf()), &sh).unwrap().key().command.clone();
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
    own.remember(kept.folder().unwrap(), &kept.key().command).unwrap();
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
    session.allow(plan(&place, &command).unwrap().key());
    let granted = permit(&place, &command, &session, &remembered, (40, 6)).unwrap();
    let spec = granted.spec();
    assert_eq!(spec.cwd, canonical);
    assert!(Path::new(&spec.program).is_absolute() && spec.program.ends_with("/sh"), "{}", spec.program);
    assert_eq!(spec.args, words(&["-c", "true"]));
    let names: Vec<&str> = spec.env.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["TERM", "COLUMNS", "LINES"]);
    // A pack runs in a new empty folder, removed when it's let go of.
    let packed = TalkPlace::Packed(folder.path().join("t.sptx"));
    session.allow(plan(&packed, &command).unwrap().key());
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
        "in".to_string(),
        format!("\"{}\"", folder_of(&typing).display()),
    ];
    assert_eq!(lines(&typing, cx), expected, "each word quoted on its own line, the program as the file that runs, then where");
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
    assert_eq!(shown.len(), 60 + 2, "every word, and where");
    for (n, line) in shown.iter().enumerate().take(60).skip(1) {
        assert_eq!(line, &format!("\"argument-{n}\""));
    }
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
    assert_eq!(shown.last().map(String::as_str), Some(RUN_PROGRAM_IN_TALK));
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
    assert!(permissions_in(scratch.path()).always(&folder_of(&typing), &kept.key().command));
    assert!(!permissions_in(scratch.path()).always(&folder_of(&typing), &plan(&place, &words(&["sh"])).unwrap().key().command));
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
    let shown = lines(&typing, cx);
    assert_eq!(shown.last().map(String::as_str), Some(RUN_IN_NEW_FOLDER));
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

/// The real helper, found in the target folder beside the test's own program, or built there once.
#[cfg(unix)]
fn real_helper() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let target = exe.parent().and_then(Path::parent).unwrap().to_path_buf();
    let helper = target.join("stagepack-terminal");
    let release = target.file_name().is_some_and(|name| name == "release");
    let mut build = std::process::Command::new(std::env!("CARGO"));
    build.args(["build", "-p", "slides-terminal-helper"]);
    if release {
        build.arg("--release");
    }
    assert!(build.status().expect("cargo can be run").success(), "the helper couldn't be built");
    assert!(helper.is_file());
    helper
}

/// A second window on the folder `path`, as another window of the same talk.
fn second_window_on(cx: &mut TestAppContext, path: &Path) -> AnyWindowHandle {
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
        .0
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
    let second = second_window_on(cx, &path);
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
    refused.refuse(plan(&place, &command).unwrap().key());
    assert!(matches!(start_after_yes(&place, &command, &refused, &remembered, &helper, (40, 6)), Err(Decision::Refused)));
    empty("the one door, shut");

    // Controls: with a yes the same door does start the recording helper, and the real one runs
    // the command, so what was checked above could have failed.
    let mut yes = SessionYes::new();
    yes.allow(plan(&place, &command).unwrap().key());
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

fn rust_files(folder: &Path, found: &mut Vec<(String, String)>, root: &Path) {
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, found, root);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let text: String = std::fs::read_to_string(&path)
                .unwrap()
                .lines()
                .map(|line| line.split("//").next().unwrap_or(""))
                .collect::<Vec<_>>()
                .join("\n");
            found.push((path.strip_prefix(root).unwrap().display().to_string(), text));
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
        files.iter().filter(|(file, text)| text.contains(name) && !allowed.contains(&file.as_str())).map(|(file, _)| file.clone()).collect()
    };
    // Starting a session, in any spelling: only the door (and the definition).
    for spelling in ["Session::start", "Session :: start", "Session as"] {
        let places = outside(spelling, &["live_terminal/session.rs", "live_terminal/permission.rs"]);
        assert!(places.is_empty(), "{spelling}: {places:?}");
    }
    assert!(outside("unchecked_for_tests", &["live_terminal/permission.rs"]).is_empty());
    assert!(outside("Permit {", &["live_terminal/permission.rs"]).is_empty());
    // The helper is found and run only by the live terminal's own files.
    assert!(outside("Command::new", &["live_terminal/session.rs"]).iter().all(|file| !file.starts_with("live_terminal/")), "a second way to run a program");
    for name in ["find_helper", "HELPER_ENV"] {
        let places = outside(name, &["live_terminal/helper.rs", "live_terminal/mod.rs", "live_terminal/permission.rs"]);
        assert!(places.is_empty(), "{name}: {places:?}");
    }
    // The door asks first.
    let door = &files.iter().find(|(file, _)| file == "live_terminal/permission.rs").unwrap().1;
    let body = &door[door.find("pub fn start_after_yes").unwrap()..];
    assert!(body.contains("permit(") && body.contains("Session::start("));
    let made = &door[door.find("pub fn permit").unwrap()..door.find("pub fn start_after_yes").unwrap()];
    assert!(made.find("decide(").unwrap() < made.find("Permit {").unwrap());
}
