//! A new presentation in its window: untitled until its first save, never lost on the way, and
//! an ordinary talk once it has a place.

#[path = "support/typing.rs"]
mod typing;

use std::path::{Path, PathBuf};
use std::time::Duration;

use typing::*;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{AnyWindowHandle, AppContext as _, TestAppContext, VisualTestContext};
use slides_engine::{TalkFiles as _, open_packed};
use stagepack_studio::document::TalkPlace;
use stagepack_studio::permissions::BuildPermissions;
use stagepack_studio::recovery::Recovery;
use stagepack_studio::ribbon::SAVE_ID;
use stagepack_studio::workspace_view::{
    CLOSE_DONT_SAVE_ID, CLOSE_SAVE_ID, ExportPdf, ExportPowerPoint, SAVE_AS_CANCEL_ID, SAVE_AS_FOLDER_ID,
    SAVE_AS_PACKED_ID, Save, TITLE_ID,
};

/// Long enough for a working copy to be kept after the last edit.
const KEEP_PAUSE: u64 = 5_000;

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
    let out = places.documents.join("Packed.sptx");
    first_save(&typing, cx, SAVE_AS_PACKED_ID, &out);
    assert!(out.is_file());
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
    cx.executor().advance_clock(Duration::from_secs(1));
}
