//! A new presentation in its window: untitled until its first save, never lost on the way, and
//! an ordinary talk once it has a place.

#[path = "support/typing.rs"]
mod typing;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use typing::*;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{AnyWindowHandle, AppContext as _, TestAppContext, VisualTestContext};
use slides_engine::{Fonts, TalkFiles as _, open_packed};
use stagepack_studio::added_files::FileKind;
use stagepack_studio::builds::Compile;
use stagepack_studio::clipboard::{self, AppData, CopiedFile, PastedFiles};
use stagepack_studio::document::{Document, TalkPlace, UntitledId};
use stagepack_studio::permissions::BuildPermissions;
use stagepack_studio::recovery::{NotTaken, Recovery};
use stagepack_studio::ribbon::SAVE_ID;
use stagepack_studio::workspace_view::{
    BUILD_ALWAYS_ID, CLOSE_DONT_SAVE_ID, CLOSE_SAVE_ID, ExportPdf, ExportPowerPoint, SAVE_AS_CANCEL_ID, SAVE_AS_FOLDER_ID,
    SAVE_AS_PACKED_ID, Save, TITLE_ID,
};

/// Long enough for a working copy to be kept after the last edit.
const KEEP_PAUSE: u64 = 5_000;

const SCORE_SOURCE: &str = include_str!("../../components/tests/fixtures/v1/score.rs");
const SCORE_BUILD: &[u8] = include_bytes!("../../components/tests/fixtures/v1/score.wasm");
const SCORE: &str = "<Score team=\"Reds\" points=\"3\" x=\"300\" y=\"300\" w=\"600\" h=\"300\" />";

/// Score as a copy of it from another talk brings it: its source, and its build as built from
/// the source with the hash `built_from`.
fn copied_score(built_from: &str) -> (String, PastedFiles) {
    let copied = [
        CopiedFile { path: "components/score.rs".into(), bytes: SCORE_SOURCE.as_bytes().into(), built_from: None },
        CopiedFile { path: "build/score.wasm".into(), bytes: SCORE_BUILD.into(), built_from: Some(built_from.into()) },
    ];
    let entry = clipboard::element_entry(SCORE, &copied).unwrap();
    match clipboard::read_app_data(entry.metadata.as_deref()) {
        AppData::Element(carried) => (carried.markdown, carried.files),
        _ => panic!("element data"),
    }
}

/// Builds with a compiler that only counts its calls.
fn count_builds(typing: &Typing, cx: &mut TestAppContext) -> Arc<AtomicUsize> {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let compile: Compile = Arc::new(move |_: &str, _: &str| {
        counted.fetch_add(1, Ordering::SeqCst);
        Err(Vec::new())
    });
    cx.update(|cx| typing.view.update(cx, |view, _| view.set_compile_for_test(compile)));
    calls
}

fn title(typing: &Typing, cx: &mut TestAppContext) -> String {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.find(TITLE_ID).label().map(str::to_string).expect("the title has a label")
    })
}

fn is_open(cx: &TestAppContext, handle: AnyWindowHandle) -> bool {
    cx.read(|cx| cx.windows().iter().any(|window| window.window_id() == handle.window_id()))
}

/// Tries to close the window, as its close button does, and says whether it closed at once.
fn try_to_close(cx: &mut TestAppContext, handle: AnyWindowHandle) -> bool {
    let closed = VisualTestContext::from_window(handle, cx).simulate_close();
    cx.run_until_parked();
    closed
}

/// Asks the window to close, and says whether it went without asking anything: at once, or
/// once the changes to its working copies were made.
fn closes_without_asking(cx: &mut TestAppContext, handle: AnyWindowHandle) -> bool {
    try_to_close(cx, handle) || !is_open(cx, handle)
}

/// Clicks `id` in the window, which may close because of it.
fn click_in(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn shows(typing: &Typing, cx: &mut TestAppContext, id: &'static str) -> bool {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some_and(|found| found.visible())
    })
}

fn add_slide(typing: &Typing, cx: &mut TestAppContext) {
    typing.press(cx, "ctrl-m");
}

fn is_untitled(typing: &Typing, cx: &TestAppContext) -> bool {
    typing.document(cx).read_with(cx, |document, _| document.is_untitled())
}

fn place(typing: &Typing, cx: &TestAppContext) -> TalkPlace {
    typing.document(cx).read_with(cx, |document, _| document.place().clone())
}

fn save_is_off(typing: &Typing, cx: &mut TestAppContext) -> bool {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.find(SAVE_ID).disabled() == Some(true)
    })
}

/// Answers the file dialog that has just been asked for with `chosen`, and lets what comes of
/// it finish.
fn choose(cx: &mut TestAppContext, chosen: Option<PathBuf>) {
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path(), "a Save dialog is showing");
    cx.simulate_new_path_selection(move |_| chosen);
    cx.run_until_parked();
}

/// Saves the untitled talk with Ctrl+S, in the form `form` is the button of, at `at`.
fn first_save(typing: &Typing, cx: &mut TestAppContext, form: &'static str, at: &Path) {
    typing.press(cx, "ctrl-s");
    click_in(cx, typing.handle, form);
    choose(cx, Some(at.to_path_buf()));
}

#[gpui_kit::test]
fn an_untitled_talk_is_never_lost(cx: &mut TestAppContext) {
    // With no edits, it closes without asking.
    let (typing, _places) = open_untitled(cx, "light");
    assert_eq!(title(&typing, cx), "Untitled presentation · 1 / 1");
    assert!(closes_without_asking(cx, typing.handle), "nothing to lose");

    // With edits, Don't save lets them go.
    let (typing, _places) = open_untitled(cx, "dark");
    add_slide(&typing, cx);
    assert!(!try_to_close(cx, typing.handle));
    click_in(cx, typing.handle, CLOSE_DONT_SAVE_ID);
    assert!(!is_open(cx, typing.handle));

    // Save asks where, and a cancelled dialog leaves the window, the talk untitled and its
    // edits there, and says why.
    let (typing, places) = open_untitled(cx, "warm");
    add_slide(&typing, cx);
    let edited = typing.source(cx);
    assert!(!try_to_close(cx, typing.handle));
    click_in(cx, typing.handle, CLOSE_SAVE_ID);
    assert!(shows(&typing, cx, SAVE_AS_PACKED_ID), "Save on an untitled talk is Save As");
    click_in(cx, typing.handle, SAVE_AS_PACKED_ID);
    choose(cx, None);
    assert!(is_open(cx, typing.handle));
    assert!(is_untitled(&typing, cx));
    assert_eq!(typing.source(cx), edited);
    let said = typing.status(cx).expect("it says why the window stayed");
    assert!(said.contains("wasn't saved"), "{said}");
    // So does the first dialog's Cancel.
    assert!(!try_to_close(cx, typing.handle));
    click_in(cx, typing.handle, CLOSE_SAVE_ID);
    click_in(cx, typing.handle, SAVE_AS_CANCEL_ID);
    assert!(is_open(cx, typing.handle));
    assert!(typing.status(cx).is_some_and(|said| said.contains("wasn't saved")));

    // A save that fails says so on the status line, and the window stays.
    let not_empty = places.documents.join("taken");
    std::fs::create_dir(&not_empty).unwrap();
    std::fs::write(not_empty.join("theirs.txt"), "theirs").unwrap();
    assert!(!try_to_close(cx, typing.handle));
    click_in(cx, typing.handle, CLOSE_SAVE_ID);
    click_in(cx, typing.handle, SAVE_AS_FOLDER_ID);
    choose(cx, Some(not_empty.clone()));
    assert!(is_open(cx, typing.handle));
    assert!(is_untitled(&typing, cx));
    let said = typing.status(cx).expect("it says what happened");
    assert!(said.starts_with("Couldn't save"), "{said}");
    assert_eq!(std::fs::read_dir(&not_empty).unwrap().count(), 1, "nothing was written there");
    wait(cx, KEEP_PAUSE);
    let untitled = place(&typing, cx);
    let kept = Recovery::new(places.recovery.clone()).find(&untitled).expect("the edits are still kept");
    assert_eq!(kept.text, edited);

    // Save, and a place chosen: it's written, and the window closes.
    let out = places.documents.join("Kept.sptx");
    assert!(!try_to_close(cx, typing.handle));
    click_in(cx, typing.handle, CLOSE_SAVE_ID);
    click_in(cx, typing.handle, SAVE_AS_PACKED_ID);
    choose(cx, Some(out.clone()));
    assert_eq!(open_packed(&out).unwrap().slides().unwrap(), edited);
    assert!(!is_open(cx, typing.handle));

    // Ctrl+S and the Save tool open Save As.
    let (typing, _places) = open_untitled(cx, "ocean");
    typing.press(cx, "ctrl-s");
    assert!(shows(&typing, cx, SAVE_AS_FOLDER_ID) && shows(&typing, cx, SAVE_AS_PACKED_ID));
    click_in(cx, typing.handle, SAVE_AS_CANCEL_ID);
    assert!(!shows(&typing, cx, SAVE_AS_FOLDER_ID));
    assert_eq!(typing.status(cx), None, "nothing to say when nothing was waiting on it");
    click_in(cx, typing.handle, SAVE_ID);
    assert!(shows(&typing, cx, SAVE_AS_PACKED_ID));
    click_in(cx, typing.handle, SAVE_AS_CANCEL_ID);
    cx.update_window(typing.handle, |_, window, cx| window.dispatch_action(Box::new(Save), cx)).unwrap();
    cx.run_until_parked();
    assert!(shows(&typing, cx, SAVE_AS_PACKED_ID), "the menu's Save does too");
}

#[gpui_kit::test]
fn escape_on_the_save_as_dialog_lets_the_close_go(cx: &mut TestAppContext) {
    let (typing, places) = open_untitled(cx, "light");
    add_slide(&typing, cx);
    assert!(!try_to_close(cx, typing.handle));
    click_in(cx, typing.handle, CLOSE_SAVE_ID);
    assert!(shows(&typing, cx, SAVE_AS_PACKED_ID));
    typing.press(cx, "escape");
    assert!(!shows(&typing, cx, SAVE_AS_PACKED_ID));
    assert!(typing.status(cx).is_some_and(|said| said.contains("wasn't saved")));
    // A save after it doesn't close the window.
    first_save(&typing, cx, SAVE_AS_PACKED_ID, &places.documents.join("Later.sptx"));
    assert!(places.documents.join("Later.sptx").is_file());
    assert!(is_open(cx, typing.handle));
}

#[gpui_kit::test]
fn two_untitled_talks_have_their_own_names_and_copies(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let first = open_untitled_under(cx, "light", 1, root.path());
    let second = open_untitled_under(cx, "dark", 2, root.path());
    assert_eq!(title(&first, cx), "Untitled presentation · 1 / 1");
    assert_eq!(title(&second, cx), "Untitled presentation 2 · 1 / 1");
    add_slide(&first, cx);
    add_slide(&second, cx);
    add_slide(&second, cx);
    wait(cx, KEEP_PAUSE);
    let recovery = Recovery::new(UntitledPlaces::under(root.path()).recovery);
    let (one, two) = (place(&first, cx), place(&second, cx));
    assert!(!matches!((&one, &two), (TalkPlace::Untitled(a), TalkPlace::Untitled(b)) if a == b));
    assert_eq!(recovery.find(&one).expect("the first one's copy").text, first.source(cx));
    assert_eq!(recovery.find(&two).expect("the second one's copy").text, second.source(cx));
}

#[gpui_kit::test]
fn an_untitled_talk_keeps_a_copy_only_once_edited(cx: &mut TestAppContext) {
    let (typing, places) = open_untitled(cx, "tour");
    wait(cx, KEEP_PAUSE);
    let recovery = Recovery::new(places.recovery.clone());
    assert!(recovery.find(&place(&typing, cx)).is_none(), "no edits, no copy");
    add_slide(&typing, cx);
    wait(cx, KEEP_PAUSE);
    let found = recovery.find(&place(&typing, cx)).expect("a copy of the edits");
    assert_eq!(found.text, typing.source(cx));
    assert!(found.files.iter().any(|(path, _)| path == "assets/photo.png"), "with the template's picture");

    // Don't save lets the copy go too.
    assert!(!try_to_close(cx, typing.handle));
    let untitled = place(&typing, cx);
    click_in(cx, typing.handle, CLOSE_DONT_SAVE_ID);
    assert!(recovery.find(&untitled).is_none());
}

#[gpui_kit::test]
fn once_saved_it_behaves_as_an_opened_talk(cx: &mut TestAppContext) {
    let (typing, places) = open_untitled(cx, "tour");
    let untitled = place(&typing, cx);
    add_slide(&typing, cx);
    wait(cx, KEEP_PAUSE);
    let recovery = Recovery::new(places.recovery.clone());
    assert!(recovery.find(&untitled).is_some());

    let folder = places.documents.join("My talk");
    first_save(&typing, cx, SAVE_AS_FOLDER_ID, &folder);
    assert_eq!(std::fs::read_to_string(folder.join("slides.md")).unwrap(), typing.source(cx));
    assert_eq!(title(&typing, cx), "My talk · 2 / 9", "its new name, and nothing unsaved");
    assert_eq!(typing.status(cx).as_deref(), Some("Saved My talk."));
    // Save is off until an edit.
    assert!(save_is_off(&typing, cx));
    // The untitled copy is gone.
    wait(cx, KEEP_PAUSE);
    assert!(recovery.find(&untitled).is_none(), "no copy is left under the untitled id");

    // An edit to its file from outside is taken in.
    let followed = cx.read(|cx| typing.view.read(cx).followed_folder().map(Path::to_path_buf));
    assert_eq!(followed.as_deref(), Some(folder.as_path()), "its folder's saves are followed");
    let theirs = format!("{}\n---\n\n# From outside\n", typing.source(cx));
    std::fs::write(folder.join("slides.md"), &theirs).unwrap();
    let file = folder.join("slides.md");
    cx.update(|cx| typing.view.update(cx, |view, cx| view.reload_soon(file, cx)));
    wait(cx, 1_000);
    assert_eq!(typing.source(cx), theirs);

    // Its working copy is under its new place.
    add_slide(&typing, cx);
    assert!(!save_is_off(&typing, cx));
    wait(cx, KEEP_PAUSE);
    let now = place(&typing, cx);
    assert!(matches!(&now, TalkPlace::Folder(at) if *at == folder));
    assert_eq!(recovery.find(&now).expect("a copy under the folder").text, typing.source(cx));
    assert!(recovery.find(&untitled).is_none());

    // Save As again, to the other form, works.
    let packed = places.documents.join("My talk.sptx");
    typing.press(cx, "ctrl-shift-s");
    click_in(cx, typing.handle, SAVE_AS_PACKED_ID);
    choose(cx, Some(packed.clone()));
    assert_eq!(open_packed(&packed).unwrap().slides().unwrap(), typing.source(cx));
    assert!(typing.document(cx).read_with(cx, |document, _| document.is_packed()));
    // And with nothing unsaved, closing doesn't ask.
    assert!(closes_without_asking(cx, typing.handle));
}

#[gpui_kit::test]
fn a_folder_saved_from_the_app_is_always_built(cx: &mut TestAppContext) {
    let (typing, places) = open_untitled(cx, "light");
    let folder = places.documents.join("Built");
    first_save(&typing, cx, SAVE_AS_FOLDER_ID, &folder);
    assert!(BuildPermissions::load(places.permissions.clone()).always(&folder));
    assert!(!is_untitled(&typing, cx));
}

#[gpui_kit::test]
fn a_packed_talk_is_not(cx: &mut TestAppContext) {
    let (typing, places) = open_untitled(cx, "light");
    add_slide(&typing, cx);
    wait(cx, KEEP_PAUSE);
    let untitled = place(&typing, cx);
    let recovery = Recovery::new(places.recovery.clone());
    assert!(recovery.find(&untitled).is_some());
    let out = places.documents.join("Packed.sptx");
    first_save(&typing, cx, SAVE_AS_PACKED_ID, &out);
    assert!(out.is_file());
    wait(cx, KEEP_PAUSE);
    assert!(recovery.find(&untitled).is_none(), "the untitled copy is cleared");
    assert!(!places.permissions.exists(), "nothing is said about a packed talk");
    let followed = cx.read(|cx| typing.view.read(cx).follows_saves());
    assert!(!followed, "nothing edits a packed file in place");
}

#[gpui_kit::test]
fn save_as_starts_in_documents_with_the_untitled_name(cx: &mut TestAppContext) {
    let (typing, places) = open_untitled(cx, "light");
    for (form, name) in [(SAVE_AS_PACKED_ID, "Untitled presentation.sptx"), (SAVE_AS_FOLDER_ID, "Untitled presentation")] {
        typing.press(cx, "ctrl-s");
        // The packed form is still the first choice: the button the dialog ends with.
        let (packed, folder) = (bounds_of(&typing, cx, SAVE_AS_PACKED_ID).unwrap(), bounds_of(&typing, cx, SAVE_AS_FOLDER_ID).unwrap());
        assert!(packed.origin.x > folder.origin.x);
        let packed_form = form == SAVE_AS_PACKED_ID;
        let suggested = cx.read(|cx| typing.view.read(cx).save_as_suggestion(packed_form, cx));
        assert_eq!(suggested, (places.documents.clone(), name.to_string()));
        click_in(cx, typing.handle, form);
        cx.run_until_parked();
        let documents = places.documents.clone();
        cx.simulate_new_path_selection(move |directory| {
            assert_eq!(directory, documents);
            None
        });
        cx.run_until_parked();
    }
    // A second one suggests its own name.
    let root = tempfile::tempdir().unwrap();
    let second = open_untitled_under(cx, "dark", 2, root.path());
    let suggested = cx.read(|cx| second.view.read(cx).save_as_suggestion(true, cx));
    assert_eq!(suggested.1, "Untitled presentation 2.sptx");
}

#[gpui_kit::test]
fn export_works_before_the_first_save(cx: &mut TestAppContext) {
    let (typing, places) = open_untitled(cx, "dark");
    let elsewhere = tempfile::tempdir().unwrap();
    for (pdf, name) in [(true, "out.pdf"), (false, "out.pptx")] {
        if pdf {
            cx.dispatch_action(typing.handle, ExportPdf);
        } else {
            cx.dispatch_action(typing.handle, ExportPowerPoint);
        }
        cx.run_until_parked();
        let documents = places.documents.clone();
        let out = elsewhere.path().join(name);
        let chosen = out.clone();
        cx.simulate_new_path_selection(move |directory| {
            assert_eq!(directory, documents, "an export suggests the Documents folder");
            Some(chosen)
        });
        cx.run_until_parked();
        let bytes = std::fs::read(&out).unwrap();
        assert!(bytes.starts_with(if pdf { b"%PDF" } else { b"PK" }), "{name}");
    }
    assert!(is_untitled(&typing, cx), "an export isn't a save");
}

#[gpui_kit::test]
fn the_save_tool_is_on_for_an_untitled_talk_and_off_once_saved(cx: &mut TestAppContext) {
    let (typing, places) = open_untitled(cx, "tour");
    assert!(!save_is_off(&typing, cx), "on from the start");
    let out = places.documents.join("Tour.sptx");
    first_save(&typing, cx, SAVE_AS_PACKED_ID, &out);
    assert!(save_is_off(&typing, cx), "nothing unsaved now");
    assert_eq!(title(&typing, cx), "Tour · 1 / 8");
    assert!(open_packed(&out).unwrap().read("assets/photo.png").is_some(), "the template's picture is in it");
}

#[gpui_kit::test]
fn a_folder_holding_a_pasted_component_is_not_always_built(cx: &mut TestAppContext) {
    let (typing, places) = open_untitled(cx, "light");
    let calls = count_builds(&typing, cx);
    // Pasted from someone else's talk, with a build that isn't of its source.
    let (markdown, files) = copied_score(&"0".repeat(64));
    typing.document(cx).update(cx, |document, cx| {
        document.paste_element(&markdown, files).unwrap();
        cx.notify();
    });
    typing.settle(cx);
    let folder = places.documents.join("Pasted");
    first_save(&typing, cx, SAVE_AS_FOLDER_ID, &folder);
    assert!(folder.join("components/score.rs").is_file());
    assert!(!BuildPermissions::load(places.permissions.clone()).always(&folder));
    assert!(!places.permissions.exists(), "nothing is written down");
    wait(cx, KEEP_PAUSE);
    assert!(shows(&typing, cx, BUILD_ALWAYS_ID), "the bar asks, as for any folder");
    assert_eq!(calls.load(Ordering::SeqCst), 0, "nothing is built without a yes");
}

#[gpui_kit::test]
fn a_folder_of_a_copy_brought_back_with_a_component_is_not_always_built(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let places = UntitledPlaces::under(root.path());
    let talk = Document::untitled_with(
        UntitledId::random(),
        format!("# Back\n\n{SCORE}\n"),
        vec![("components/score.rs".to_string(), Arc::from(SCORE_SOURCE.as_bytes()))],
        Vec::new(),
        Fonts::bundled_only(),
    )
    .unwrap();
    let typing = open_untitled_talk(cx, talk, root.path());
    let calls = count_builds(&typing, cx);
    let folder = places.documents.join("Back");
    first_save(&typing, cx, SAVE_AS_FOLDER_ID, &folder);
    assert!(folder.join("components/score.rs").is_file());
    assert!(!BuildPermissions::load(places.permissions.clone()).always(&folder));
    wait(cx, KEEP_PAUSE);
    assert!(shows(&typing, cx, BUILD_ALWAYS_ID));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[gpui_kit::test]
fn closing_while_a_save_runs_waits_for_it(cx: &mut TestAppContext) {
    // A window keeping no working copies, which would make it wait for those anyway.
    let typing = open(cx, "# One\n");
    std::fs::remove_file(&typing.file).unwrap();
    // Nothing unsaved, and a save asked for and left running, as the first save of a new
    // presentation can be when its window is closed.
    cx.update_window(typing.handle, |_, window, cx| window.dispatch_action(Box::new(Save), cx)).unwrap();
    assert!(!VisualTestContext::from_window(typing.handle, cx).simulate_close(), "not while it writes");
    cx.run_until_parked();
    assert!(typing.file.is_file(), "the save finished");
    assert!(!is_open(cx, typing.handle), "and then the window went");
}

/// Closes the window as a crash would: without asking, and without anything cleared.
fn crash(cx: &mut TestAppContext, typing: Typing) {
    let handle = typing.handle;
    drop(typing);
    cx.update_window(handle, |_, window, _| window.remove_window()).unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn an_untitled_talk_keeps_its_copy_as_it_is_edited(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let places = UntitledPlaces::under(root.path());
    let typing = open_untitled_under(cx, "tour", 2, root.path());
    let untitled = place(&typing, cx);
    let TalkPlace::Untitled(id) = untitled else { panic!("untitled") };
    let recovery = Recovery::new(places.recovery.clone());
    wait(cx, KEEP_PAUSE);
    assert!(recovery.find(&untitled).is_none(), "no edits, no copy");

    add_slide(&typing, cx);
    wait(cx, KEEP_PAUSE);
    let edited = typing.source(cx);
    assert_eq!(recovery.find(&untitled).expect("a copy of the edits").text, edited);
    assert_eq!(recovery.untitled(), (Vec::new(), 0), "the window holds it, so it isn't listed");
    assert_eq!(recovery.take_untitled(id).unwrap_err(), NotTaken::Held);

    crash(cx, typing);
    let (listed, skipped) = recovery.untitled();
    assert_eq!(skipped, 0);
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].id, id);
    assert_eq!(listed[0].name, "Untitled presentation 2", "what it was called when it was kept");
    assert_eq!(listed[0].first_words, "Welcome");
    let found = recovery.take_untitled(id).unwrap();
    assert_eq!(found.text, edited);
    assert!(found.files.iter().any(|(path, _)| path == "assets/photo.png"), "with the template's picture");
}

#[gpui_kit::test]
fn the_first_save_clears_the_untitled_copy(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let places = UntitledPlaces::under(root.path());
    let typing = open_untitled_under(cx, "light", 1, root.path());
    let untitled = place(&typing, cx);
    let TalkPlace::Untitled(id) = untitled else { panic!("untitled") };
    add_slide(&typing, cx);
    wait(cx, KEEP_PAUSE);
    let recovery = Recovery::new(places.recovery.clone());
    assert!(recovery.find(&untitled).is_some());

    first_save(&typing, cx, SAVE_AS_PACKED_ID, &places.documents.join("First.sptx"));
    wait(cx, KEEP_PAUSE);
    assert!(!is_untitled(&typing, cx));
    assert!(recovery.find(&untitled).is_none(), "the save holds it now");
    crash(cx, typing);
    assert_eq!(recovery.untitled(), (Vec::new(), 0));
    assert_eq!(recovery.take_untitled(id).unwrap_err(), NotTaken::Gone);
}

#[gpui_kit::test]
fn closing_with_dont_save_discards_the_copy(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let places = UntitledPlaces::under(root.path());
    let typing = open_untitled_under(cx, "dark", 1, root.path());
    let untitled = place(&typing, cx);
    add_slide(&typing, cx);
    wait(cx, KEEP_PAUSE);
    let recovery = Recovery::new(places.recovery.clone());
    assert!(recovery.find(&untitled).is_some());

    let handle = typing.handle;
    assert!(!try_to_close(cx, handle));
    click_in(cx, handle, CLOSE_DONT_SAVE_ID);
    assert!(!is_open(cx, handle));
    drop(typing);
    cx.run_until_parked();
    assert!(recovery.find(&untitled).is_none());
    assert_eq!(recovery.untitled(), (Vec::new(), 0));
    assert!(!places.recovery.exists() || std::fs::read_dir(&places.recovery).unwrap().count() == 0, "nothing left behind");
}

#[gpui_kit::test]
fn a_picture_undone_before_its_copy_is_kept_leaves_nothing_behind(cx: &mut TestAppContext) {
    // Closed once the pause is over, and closed within it.
    for waits in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let places = UntitledPlaces::under(root.path());
        let typing = open_untitled_under(cx, "light", 1, root.path());
        let photo = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/showcase/assets/photo.png")).unwrap();
        let document = typing.document(cx);
        document.update(cx, |document, cx| {
            let path = document.add_file(FileKind::Picture, "photo.png", photo).unwrap();
            document.insert_tag("Insert picture", "Picture", &[("src", &path)], None).unwrap();
            cx.notify();
        });
        cx.run_until_parked();
        // The picture is kept the moment it's added, before any text is.
        let slots = || -> Vec<PathBuf> {
            std::fs::read_dir(&places.recovery)
                .map(|keys| keys.flatten().flat_map(|key| std::fs::read_dir(key.path()).into_iter().flatten().flatten()).map(|slot| slot.path()).collect())
                .unwrap_or_default()
        };
        assert_eq!(slots().len(), 1, "waits {waits}: the picture's slot");
        assert!(!slots()[0].join("slides.md").exists(), "waits {waits}: no text kept yet");
        // Undone within the pause before the text would be kept.
        document.update(cx, |document, cx| {
            document.undo().unwrap();
            cx.notify();
        });
        cx.run_until_parked();
        assert!(!document.read_with(cx, |document, _| document.has_unsaved_changes()));
        if waits {
            wait(cx, KEEP_PAUSE);
        }

        assert!(closes_without_asking(cx, typing.handle), "waits {waits}");
        drop(typing);
        cx.run_until_parked();
        assert_eq!(slots(), Vec::<PathBuf>::new(), "waits {waits}: nothing left behind");
        assert_eq!(Recovery::new(places.recovery.clone()).untitled(), (Vec::new(), 0));
    }
}

#[gpui_kit::test]
fn the_templates_picture_taken_away_before_the_first_save_comes_back_with_undo(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let places = UntitledPlaces::under(root.path());
    let typing = open_untitled_under(cx, "tour", 1, root.path());
    let document = typing.document(cx);
    document.update(cx, |document, cx| {
        let at = document.source().find("<Picture src=\"assets/photo.png\"").unwrap();
        let end = at + document.source()[at..].find("/>").unwrap() + 2;
        assert!((0..document.slide_count()).any(|slide| document.select(slide, at..end)));
        document.delete_selected().unwrap();
        cx.notify();
    });
    cx.run_until_parked();
    let folder = places.documents.join("Tour");
    first_save(&typing, cx, SAVE_AS_FOLDER_ID, &folder);
    assert!(!folder.join("assets/photo.png").exists(), "the text didn't use it");
    let recovery = Recovery::new(places.recovery.clone());
    // The picture the save didn't write isn't kept on its own, with nothing unsaved.
    let nothing_kept = || !places.recovery.exists() || std::fs::read_dir(&places.recovery).unwrap().count() == 0;
    cx.run_until_parked();
    assert!(nothing_kept(), "not even for a moment");
    wait(cx, KEEP_PAUSE);
    assert!(nothing_kept());

    // Undone, it's unsaved again, kept with its picture, and the next save writes it.
    document.update(cx, |document, cx| {
        document.undo().unwrap();
        cx.notify();
    });
    wait(cx, KEEP_PAUSE);
    let now = place(&typing, cx);
    let found = recovery.find(&now).expect("a copy of the undo");
    assert!(found.files.iter().any(|(path, _)| path == "assets/photo.png"), "with the template's picture");
    typing.press(cx, "ctrl-s");
    cx.run_until_parked();
    assert!(folder.join("assets/photo.png").is_file(), "written by the next save");
    assert_eq!(std::fs::read_to_string(folder.join("slides.md")).unwrap(), typing.source(cx));
    wait(cx, KEEP_PAUSE);
    assert!(recovery.find(&now).is_none());
    assert!(closes_without_asking(cx, typing.handle));
}

#[gpui_kit::test]
fn a_talk_brought_back_is_kept_before_its_old_copy_goes(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let places = UntitledPlaces::under(root.path());
    let recovery = Recovery::new(places.recovery.clone());
    let typing = open_untitled_under(cx, "tour", 1, root.path());
    let TalkPlace::Untitled(id) = place(&typing, cx) else { panic!("untitled") };
    add_slide(&typing, cx);
    wait(cx, KEEP_PAUSE);
    let edited = typing.source(cx);
    crash(cx, typing);

    // Brought back, and gone again with nothing edited.
    let found = recovery.take_untitled(id).unwrap();
    let typing = open_brought_back(cx, id, found, 2, root.path());
    assert_eq!(typing.source(cx), edited);
    assert_eq!(title(&typing, cx), "• Untitled presentation 2 · 1 / 9");
    wait(cx, KEEP_PAUSE);
    crash(cx, typing);
    let (listed, skipped) = recovery.untitled();
    assert_eq!((listed.len(), skipped), (1, 0), "{listed:?}");
    assert_eq!(listed[0].id, id);
    assert_eq!(listed[0].name, "Untitled presentation 2");
    let found = recovery.take_untitled(id).unwrap();
    assert_eq!(found.text.as_bytes(), edited.as_bytes());
    assert!(found.files.iter().any(|(path, _)| path == "assets/photo.png"), "with its picture");

    // Brought back and edited: one copy, under the same id, the old one gone.
    let typing = open_brought_back(cx, id, found, 1, root.path());
    add_slide(&typing, cx);
    wait(cx, KEEP_PAUSE);
    let edited = typing.source(cx);
    crash(cx, typing);
    assert_eq!(std::fs::read_dir(&places.recovery).unwrap().count(), 1, "one talk's folder");
    let slots = std::fs::read_dir(&places.recovery).unwrap().next().unwrap().unwrap().path();
    assert_eq!(std::fs::read_dir(&slots).unwrap().count(), 1, "one copy in it");
    let (listed, _) = recovery.untitled();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id);
    assert_eq!(recovery.take_untitled(id).unwrap().text, edited);
}
