//! The yes a talk needs before it runs a program: asked once a session, remembered only for one
//! folder and one command, never kept in a talk, shown in full, and impossible to go round.

#[path = "support/typing.rs"]
mod typing;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use typing::*;

use gpui_kit::component::WindowExt as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, TestAppContext};
use slides_engine::Fonts;
use stagepack_studio::document::{Document, TalkPlace, UntitledId};
use stagepack_studio::export::{ExportKind, run_export};
use stagepack_studio::live_terminal::{
    Decision, HELPER_ENV, RUN_ALWAYS_ID, RUN_ASK_ID, RUN_IN_NEW_FOLDER, RUN_LINE_ID, RUN_NO_ID, RUN_QUESTION, RUN_YES_ID, RunPermissions,
    SessionYes, TalkFolder, may_run, start_after_yes,
};
use stagepack_studio::present_view::start_show;

fn words(command: &[&str]) -> Vec<String> {
    command.iter().map(|word| word.to_string()).collect()
}

fn permissions_in(dir: &Path) -> RunPermissions {
    RunPermissions::load(dir.join("terminal-permissions.json"))
}

#[test]
fn the_yes_is_asked_once_per_session() {
    let folder = tempfile::tempdir().unwrap();
    let talk = TalkPlace::Folder(folder.path().to_path_buf());
    let scratch = tempfile::tempdir().unwrap();
    let remembered = permissions_in(scratch.path());
    let bash = words(&["bash"]);
    let mut session = SessionYes::new();
    assert_eq!(may_run(&talk, &bash, &session, &remembered), Decision::Ask);
    session.allow(&bash);
    assert_eq!(may_run(&talk, &bash, &session, &remembered), Decision::Run);
    assert_eq!(may_run(&talk, &words(&["bash", "-l"]), &session, &remembered), Decision::Ask, "another command asks again");
    assert_eq!(may_run(&talk, &words(&["Bash"]), &session, &remembered), Decision::Ask, "a byte different asks again");
    session.refuse(&bash);
    assert!(session.refused(&bash) && !session.allowed(&bash));
    assert_eq!(may_run(&talk, &bash, &session, &remembered), Decision::Refused);
    assert_eq!(may_run(&talk, &bash, &SessionYes::new(), &remembered), Decision::Ask, "a new session starts over");
    // A yes for a packed or untitled talk is only for the session.
    for place in [TalkPlace::Packed(folder.path().join("a.sptx")), TalkPlace::Untitled(UntitledId(7))] {
        assert_eq!(may_run(&place, &bash, &SessionYes::new(), &remembered), Decision::Ask);
    }
}

#[test]
fn a_command_that_cant_run_is_refused_whatever_has_been_said() {
    let folder = tempfile::tempdir().unwrap();
    let talk = TalkPlace::Folder(folder.path().to_path_buf());
    let scratch = tempfile::tempdir().unwrap();
    let remembered = permissions_in(scratch.path());
    let mut session = SessionYes::new();
    let too_many: Vec<String> = (0..65).map(|n| format!("w{n}")).collect();
    let too_long = vec!["x".repeat(4097)];
    for bad in [Vec::new(), vec![String::new()], too_many, too_long] {
        session.allow(&bad);
        assert_eq!(may_run(&talk, &bad, &session, &remembered), Decision::Refused, "{:?}", bad.len());
    }
}

#[test]
fn always_is_for_one_folder_and_one_command() {
    let root = tempfile::tempdir().unwrap();
    let (a, b) = (root.path().join("a"), root.path().join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let mut remembered = permissions_in(scratch.path());
    let bash = words(&["bash"]);
    remembered.remember(&TalkFolder::of(&TalkPlace::Folder(a.clone())).unwrap(), &bash).unwrap();
    let session = SessionYes::new();
    let decide = |place: &TalkPlace, command: &[String]| may_run(place, command, &session, &remembered);
    assert_eq!(decide(&TalkPlace::Folder(a.clone()), &bash), Decision::Run);
    assert_eq!(decide(&TalkPlace::Folder(a.clone()), &words(&["bash", "-l"])), Decision::Ask);
    assert_eq!(decide(&TalkPlace::Folder(b.clone()), &bash), Decision::Ask);
    // Written another way, or by a link, it's the same folder.
    assert_eq!(decide(&TalkPlace::Folder(a.join("..").join("a")), &bash), Decision::Run);
    #[cfg(unix)]
    {
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&a, &link).unwrap();
        assert_eq!(decide(&TalkPlace::Folder(link), &bash), Decision::Run);
    }
    // A packed copy of it, or one kept nowhere, never uses what a folder has.
    assert_eq!(decide(&TalkPlace::Packed(a.join("copy.sptx")), &bash), Decision::Ask);
    assert_eq!(decide(&TalkPlace::Packed(a.clone()), &bash), Decision::Ask);
    assert_eq!(decide(&TalkPlace::Untitled(UntitledId(1)), &bash), Decision::Ask);
    // It's kept: a window that reads the file afresh has it.
    let again = permissions_in(scratch.path());
    assert!(again.always(&a, &bash) && !again.always(&b, &bash));
    // A folder that is gone, or renamed, is another folder.
    let renamed = root.path().join("renamed");
    std::fs::rename(&a, &renamed).unwrap();
    assert_eq!(may_run(&TalkPlace::Folder(renamed), &bash, &session, &again), Decision::Ask);
    assert_eq!(may_run(&TalkPlace::Folder(a), &bash, &session, &again), Decision::Ask);
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
    let bash = words(&["bash"]);
    let canonical = talk.path().canonicalize().unwrap();
    // A file the talk carries, in the form the app keeps, in the places it might be looked for.
    let claim = format!("{{\"always_run\": [{{\"folder\": {:?}, \"command\": [\"bash\"]}}]}}", canonical.display().to_string());
    std::fs::write(talk.path().join("terminal-permissions.json"), &claim).unwrap();
    std::fs::create_dir_all(talk.path().join("stagepack-studio")).unwrap();
    std::fs::write(talk.path().join("stagepack-studio/terminal-permissions.json"), &claim).unwrap();
    std::fs::write(talk.path().join("slides.md"), "# T\n").unwrap();
    // The only file that counts is the app's own, in its own data folder.
    let own = permissions_in(scratch.path());
    let place = TalkPlace::Folder(talk.path().to_path_buf());
    assert_eq!(may_run(&place, &bash, &SessionYes::new(), &own), Decision::Ask);
    // Opening the talk reads nothing from it either.
    let document = Document::open(talk.path(), Fonts::bundled_only()).unwrap();
    assert_eq!(may_run(document.place(), &bash, &SessionYes::new(), &own), Decision::Ask);
    // And what the app writes goes only where it was told.
    let mut own = own;
    own.remember(&TalkFolder::of(&place).unwrap(), &bash).unwrap();
    assert!(scratch.path().join("terminal-permissions.json").is_file());
    assert_eq!(std::fs::read_to_string(talk.path().join("terminal-permissions.json")).unwrap(), claim);
}

// The dialog.

const TALK: &str = "# T\n\n<Terminal run=\"sh -c 'touch ran'\" x=\"100\" y=\"100\" w=\"640\" h=\"360\" />\n";

fn ask(typing: &Typing, cx: &mut TestAppContext, command: &[&str]) {
    let command = words(command);
    cx.update_window(typing.handle, |_, window, cx| {
        typing.view.update(cx, |view, cx| view.ask_to_run(command, window, cx));
    })
    .unwrap();
    typing.settle(cx);
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

/// A window on a folder talk whose commands are remembered in a scratch file of its own.
fn window(cx: &mut TestAppContext) -> (Typing, tempfile::TempDir) {
    let scratch = tempfile::tempdir().unwrap();
    let typing = open(cx, TALK);
    let remembered = permissions_in(scratch.path());
    typing.view.update(cx, |view, _| view.set_run_permissions_for_test(remembered));
    (typing, scratch)
}

#[gpui_kit::test]
fn the_dialog_shows_the_whole_command(cx: &mut TestAppContext) {
    let (typing, _scratch) = window(cx);
    let command = ["tool", "--name", "two words", "-x"];
    ask(&typing, cx, &command);
    assert!(showing(&typing, cx, RUN_ASK_ID));
    let shown = lines(&typing, cx);
    let folder = typing.file.parent().unwrap().display().to_string();
    let expected: Vec<String> = command.iter().map(|word| word.to_string()).chain(["in".to_string(), folder]).collect();
    assert_eq!(shown, expected, "each word of the command on its own line, then where");
    assert!(typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.find(RUN_ASK_ID).label() == Some(RUN_QUESTION)
    }));
    for id in [RUN_YES_ID, RUN_ALWAYS_ID, RUN_NO_ID] {
        assert!(showing(&typing, cx, id), "{id}");
    }
}

#[gpui_kit::test]
fn escape_and_dont_run_refuse(cx: &mut TestAppContext) {
    let (typing, _scratch) = window(cx);
    let bash = ["bash"];
    ask(&typing, cx, &bash);
    typing.press(cx, "escape");
    assert!(!showing(&typing, cx, RUN_ASK_ID));
    assert_eq!(decision(&typing, cx, &bash), Decision::Refused);
    // A refused command isn't asked about again.
    ask(&typing, cx, &bash);
    assert!(!showing(&typing, cx, RUN_ASK_ID));

    ask(&typing, cx, &["other"]);
    click_on(&typing, cx, RUN_NO_ID);
    assert!(!showing(&typing, cx, RUN_ASK_ID));
    assert_eq!(decision(&typing, cx, &["other"]), Decision::Refused);

    // Enter does what the keyboard is on, which is Don't run.
    ask(&typing, cx, &["third"]);
    typing.press(cx, "enter");
    assert!(!showing(&typing, cx, RUN_ASK_ID));
    assert_eq!(decision(&typing, cx, &["third"]), Decision::Refused);
    assert!(typing.with_window(cx, |window, cx| !window.has_active_dialog(cx)));
}

#[gpui_kit::test]
fn a_click_beside_the_dialog_does_nothing(cx: &mut TestAppContext) {
    let (typing, _scratch) = window(cx);
    ask(&typing, cx, &["bash"]);
    let at = typing.at(cx, 1., 1.);
    typing.click(cx, at);
    let corner = gpui_kit::point(gpui_kit::px(3.), gpui_kit::px(3.));
    typing.click(cx, corner);
    assert!(showing(&typing, cx, RUN_ASK_ID), "still asking");
    assert_eq!(decision(&typing, cx, &["bash"]), Decision::Ask, "and nothing was said");
}

#[gpui_kit::test]
fn run_is_for_the_session_and_always_is_kept(cx: &mut TestAppContext) {
    let (typing, scratch) = window(cx);
    ask(&typing, cx, &["bash"]);
    click_on(&typing, cx, RUN_YES_ID);
    assert_eq!(decision(&typing, cx, &["bash"]), Decision::Run);
    assert!(!scratch.path().join("terminal-permissions.json").exists(), "Run is not kept");
    ask(&typing, cx, &["zsh"]);
    click_on(&typing, cx, RUN_ALWAYS_ID);
    assert_eq!(decision(&typing, cx, &["zsh"]), Decision::Run);
    let folder = typing.file.parent().unwrap();
    assert!(permissions_in(scratch.path()).always(folder, &words(&["zsh"])));
    assert!(!permissions_in(scratch.path()).always(folder, &words(&["bash"])));
}

#[gpui_kit::test]
fn a_packed_or_untitled_talk_cant_choose_always(cx: &mut TestAppContext) {
    let (typing, places) = open_untitled(cx, "light");
    let remembered = permissions_in(places.documents.parent().unwrap());
    typing.view.update(cx, |view, _| view.set_run_permissions_for_test(remembered));
    ask(&typing, cx, &["bash"]);
    assert!(showing(&typing, cx, RUN_YES_ID));
    assert!(!showing(&typing, cx, RUN_ALWAYS_ID));
    let shown = lines(&typing, cx);
    assert_eq!(shown.last().map(String::as_str), Some(RUN_IN_NEW_FOLDER));
    click_on(&typing, cx, RUN_YES_ID);
    assert_eq!(decision(&typing, cx, &["bash"]), Decision::Run);
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
    let ran = |folder: &Path| assert!(!folder.join("ran").exists(), "the command ran");

    // A folder talk with the tag, shown in the editor, with its thumbnail.
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), format!("{TALK}\n---\n\n# Two\n")).unwrap();
    let path = folder.path().to_path_buf();
    let typing = open_folder(cx, folder, None);
    typing.settle(cx);
    wait(cx, 1_000);
    empty("opened and shown in the editor");
    // Selecting the terminal, as the editor would to offer Start.
    let terminal = thing(&typing, cx, "<Terminal");
    let at = typing.at(cx, terminal.rect.x + terminal.rect.w / 5., terminal.rect.y + terminal.rect.h / 5.);
    typing.click(cx, at);
    go_to(&typing, cx, 1);
    typing.press(cx, "pageup");
    assert_eq!(typing.current(cx), 0);
    wait(cx, 1_000);
    empty("selected, and the slides' thumbnails made");

    // Every export.
    let out = tempfile::tempdir().unwrap();
    let document = typing.document(cx);
    let input = || cx.read(|cx| document.read(cx).export_input());
    for (kind, name) in [(ExportKind::Pdf, "a.pdf"), (ExportKind::PowerPoint, "a.pptx")] {
        run_export(input(), kind, &out.path().join(name)).unwrap();
    }
    empty("exported as PDF and PowerPoint");

    // The AI tool's commands.
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

    // The presenter's view, and a second window of the same talk.
    let document = typing.document(cx);
    cx.update(|cx| {
        stagepack_studio::workspace_view::bind_keys(cx);
        let show = start_show(document.clone(), true, cx);
        let _ = show;
    });
    wait(cx, 1_000);
    empty("the presenter's view");
    let second = tempfile::tempdir().unwrap();
    std::fs::write(second.path().join("slides.md"), format!("{TALK}\n---\n\n# Two\n")).unwrap();
    let second_window = open_folder(cx, second, None);
    wait(cx, 1_000);
    empty("a second window");

    // Asking, and saying yes, only records the yes: it starts nothing.
    ask(&typing, cx, &["sh", "-c", "touch ran"]);
    click_on(&typing, cx, RUN_YES_ID);
    ask(&second_window, cx, &["sh", "-c", "touch ran"]);
    assert_eq!(decision(&second_window, cx, &["sh", "-c", "touch ran"]), Decision::Ask, "a yes in one window isn't another's");
    wait(cx, 1_000);
    empty("a yes was said");
    ran(&path);

    // The only door: without a yes it opens on nothing, and the helper isn't even run.
    let session = SessionYes::new();
    let remembered = permissions_in(scratch.path());
    let place = TalkPlace::Folder(path.clone());
    let spec = slides_terminal_helper::wire::StartSpec {
        program: String::new(),
        args: Vec::new(),
        cwd: path.clone(),
        cols: 40,
        rows: 6,
        env: Vec::new(),
    };
    let command = words(&["sh", "-c", "touch ran"]);
    for _ in 0..2 {
        assert!(matches!(start_after_yes(&place, &command, &session, &remembered, &helper, spec.clone()), Err(Decision::Ask)));
    }
    let mut refused = SessionYes::new();
    refused.refuse(&command);
    assert!(matches!(start_after_yes(&place, &command, &refused, &remembered, &helper, spec), Err(Decision::Refused)));
    empty("the one door, shut");
    // The record is a real one: a helper that is started does write itself down.
    let control = slides_terminal_helper::wire::StartSpec {
        program: "true".into(),
        args: Vec::new(),
        cwd: path.clone(),
        cols: 40,
        rows: 6,
        env: Vec::new(),
    };
    let started = stagepack_studio::live_terminal::Session::start(&helper, control);
    let began = std::time::Instant::now();
    while std::fs::read(&record).unwrap().is_empty() && began.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!std::fs::read(&record).unwrap().is_empty(), "the record would show a start");
    drop(started);
    unsafe { std::env::remove_var(HELPER_ENV) };
}

/// A program is started in one place only, and that place asks first.
#[test]
fn a_session_is_started_from_one_place_and_it_asks() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    let mut folders = vec![src.clone()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                for (number, line) in text.lines().enumerate() {
                    let code = line.split("//").next().unwrap_or("");
                    if code.contains("Session::start(") {
                        found.push(format!("{}:{}", path.strip_prefix(&src).unwrap().display(), number + 1));
                    }
                }
            }
        }
    }
    // Its own definition's file and the one door, `start_after_yes`, which asks `may_run` first.
    let calls: Vec<&String> = found.iter().filter(|place| !place.starts_with("live_terminal/session.rs")).collect();
    assert_eq!(calls.len(), 1, "{found:?}");
    assert!(calls[0].starts_with("live_terminal/permission.rs"), "{found:?}");
    let door = std::fs::read_to_string(src.join("live_terminal/permission.rs")).unwrap();
    let body = &door[door.find("pub fn start_after_yes").unwrap()..];
    assert!(body.find("may_run(").unwrap() < body.find("Session::start(").unwrap());
}
