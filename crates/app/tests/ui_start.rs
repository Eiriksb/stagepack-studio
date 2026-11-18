//! The start screen: what a window shows with no talk in it. The templates a new presentation
//! starts as, the two ways to open a talk, and the new presentations not saved last time.

#[path = "support/typing.rs"]
mod typing;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use typing::*;

use gpui_kit::test::{TestQueryExt as _, TestWindowExt as _};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Entity, Point, TestAppContext, VisualTestContext, WindowBounds,
    WindowOptions, px, size,
};
use stagepack_studio::builds::Compile;
use stagepack_studio::document::{TalkPlace, UntitledId};
use stagepack_studio::permissions::BuildPermissions;
use stagepack_studio::recovery::{Recovery, hash_text};
use stagepack_studio::ribbon::SAVE_ID;
use stagepack_studio::slide_strip::strip_row_id;
use stagepack_studio::start_screen::{
    CANCEL_DISCARD_ID, CONFIRM_DISCARD_ID, DISCARD_QUESTION, START_OPEN_FILE_ID, START_OPEN_FOLDER_ID,
    START_PROBLEM_ID, UNREADABLE_ID, template_picture_id, template_tile_id, tile_picture, unsaved_discard_id,
    unsaved_open_id, unsaved_row_id, when_words,
};
use stagepack_studio::templates::{template, templates};
use stagepack_studio::workspace_view::{
    self, BUILD_BAR_ID, BUILD_ONCE_ID, CANVAS_ID, DISCARD_RECOVERY_ID, FILE_MENU_ID, OPEN_IN_STAGEPACK_ID, PROBLEM_ID,
    RECOVERY_ID, TITLE_ID,
    UNSAVED_MARK, WorkspaceView,
};

/// Long enough for a working copy to be kept after the last edit.
const KEEP_PAUSE: u64 = 5_000;

/// Opens a start window whose working copies, Documents folder and build permissions are all
/// under `root`, as [`UntitledPlaces::under`] has them, and draws it until it's settled.
fn open_start_under(cx: &mut TestAppContext, root: &Path) -> Typing {
    let typing = open_start_unsettled(cx, root, |_| {});
    typing.settle(cx);
    typing
}

/// Opens a start window as [`open_start_under`] does, set up by `set_up` too, and leaves it
/// before anything has run.
fn open_start_unsettled(
    cx: &mut TestAppContext,
    root: &Path,
    set_up: impl FnOnce(&mut WorkspaceView) + 'static,
) -> Typing {
    let places = UntitledPlaces::under(root);
    let scratch = tempfile::tempdir().unwrap();
    let file = scratch.path().join("no file yet");
    let (handle, view) = cx.update(|cx: &mut App| {
        gpui_kit::init(cx);
        workspace_view::init(cx);
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1280.), px(800.)),
        };
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx| {
                    let mut view = WorkspaceView::for_start(Some(Recovery::new(places.recovery)), window, cx);
                    view.set_permissions_for_test(BuildPermissions::load(places.permissions));
                    view.set_documents_folder_for_test(places.documents);
                    view.set_no_watching_for_test();
                    set_up(&mut view);
                    view
                })
            },
        )
        .expect("open test window")
    });
    Typing {
        _folder: scratch,
        file,
        handle,
        view,
    }
}

/// A start window with a scratch folder of its own for all it keeps.
fn open_start(cx: &mut TestAppContext) -> (Typing, UntitledPlaces, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    let places = UntitledPlaces::under(root.path());
    let typing = open_start_under(cx, root.path());
    (typing, places, root)
}

fn shows(typing: &Typing, cx: &mut TestAppContext, id: impl Into<gpui_kit::ElementId>) -> bool {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some_and(|found| found.visible())
    })
}

fn label_of(typing: &Typing, cx: &mut TestAppContext, id: impl Into<gpui_kit::ElementId>) -> Option<String> {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(id).and_then(|found| found.label().map(str::to_string))
    })
}

fn has_label(typing: &Typing, cx: &mut TestAppContext, label: &str) -> bool {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        !window.find_all_by_label(label).is_empty()
    })
}

/// The labels inside the element `id`.
fn labels_within(typing: &Typing, cx: &mut TestAppContext, id: impl Into<gpui_kit::ElementId>) -> Vec<String> {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window
            .within(id)
            .elements()
            .iter()
            .filter_map(|element| element.label().map(str::to_string))
            .collect()
    })
}

fn has_keyboard(typing: &Typing, cx: &mut TestAppContext, id: impl Into<gpui_kit::ElementId>) -> bool {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.find(id).focused() == Some(true)
    })
}

fn title(typing: &Typing, cx: &mut TestAppContext) -> String {
    label_of(typing, cx, TITLE_ID).expect("the title has a label")
}

fn has_talk(typing: &Typing, cx: &TestAppContext) -> bool {
    cx.read(|cx| typing.view.read(cx).document().is_some())
}

fn is_start_screen(typing: &Typing, cx: &mut TestAppContext) -> bool {
    !has_talk(typing, cx) && shows(typing, cx, START_OPEN_FILE_ID) && shows(typing, cx, template_tile_id("light"))
}

fn windows(cx: &TestAppContext) -> Vec<AnyWindowHandle> {
    cx.read(|cx| cx.windows())
}

/// The view of the window `handle`.
fn view_of(cx: &TestAppContext, handle: AnyWindowHandle) -> Entity<WorkspaceView> {
    cx.read(|cx| {
        let root = handle.downcast::<gpui_kit::base::Root>().expect("a kit window").read(cx).unwrap();
        root.view().clone().downcast::<WorkspaceView>().expect("a window on a talk")
    })
}

/// The window opened since `before` were.
fn the_new_window(cx: &TestAppContext, before: &[AnyWindowHandle]) -> Typing {
    let opened: Vec<AnyWindowHandle> = windows(cx)
        .into_iter()
        .filter(|window| before.iter().all(|old| old.window_id() != window.window_id()))
        .collect();
    assert_eq!(opened.len(), 1, "one window opened");
    let handle = opened[0];
    let scratch = tempfile::tempdir().unwrap();
    let file = scratch.path().join("no file yet");
    Typing {
        _folder: scratch,
        file,
        handle,
        view: view_of(cx, handle),
    }
}

/// Answers the Open dialog that has just been asked for with `chosen`, after checking it asks
/// for folders if `folders`, or files.
fn answer_open(typing: &Typing, cx: &mut TestAppContext, folders: bool, chosen: Option<PathBuf>) {
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths(), "an Open dialog is showing");
    cx.simulate_path_prompt_response(move |options| {
        assert_eq!(options.directories, folders);
        assert_eq!(options.files, !folders);
        assert!(!options.multiple);
        chosen.map(|path| vec![path])
    });
    cx.run_until_parked();
    typing.settle(cx);
}

fn pick(typing: &Typing, cx: &mut TestAppContext, id: &str) {
    click_on(typing, cx, template_tile_id(id));
}

fn keep_untitled(root: &Path, name: &str, text: &str) -> UntitledId {
    let id = UntitledId::random();
    Recovery::new(root.join("recovery")).keep_untitled(id, name, text, &[], &[]).unwrap();
    id
}

/// The folder `id`'s copy is kept in under the recovery root `recovery`.
fn untitled_slot(recovery: &Path, id: UntitledId) -> PathBuf {
    let hex = id.to_string();
    for key in std::fs::read_dir(recovery).unwrap().flatten() {
        let Ok(slots) = std::fs::read_dir(key.path()) else { continue };
        for slot in slots.flatten() {
            let about = std::fs::read_to_string(slot.path().join("about.json")).unwrap_or_default();
            if about.contains(&hex) {
                return slot.path();
            }
        }
    }
    panic!("no copy of {hex}");
}

fn set_kept_at(recovery: &Path, id: UntitledId, at: SystemTime) {
    let about = untitled_slot(recovery, id).join("about.json");
    let mut json: serde_json::Value = serde_json::from_slice(&std::fs::read(&about).unwrap()).unwrap();
    json["kept_at"] = at.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs().into();
    std::fs::write(&about, json.to_string()).unwrap();
}

fn spoil(recovery: &Path, id: UntitledId) {
    let about = untitled_slot(recovery, id).join("about.json");
    let bytes = std::fs::read(&about).unwrap();
    std::fs::write(&about, &bytes[..bytes.len() / 2]).unwrap();
}

/// Every file under `root`, and what it holds.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.insert(path.clone(), Vec::new());
                folders.push(path);
            } else {
                files.insert(path.clone(), std::fs::read(&path).unwrap());
            }
        }
    }
    files
}

fn file_menu_has(typing: &Typing, cx: &mut TestAppContext, item: &str) -> bool {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.click(FILE_MENU_ID, cx);
        let found = window.within("popup-menu").try_find_by_label(item).is_some();
        window.click(FILE_MENU_ID, cx);
        found
    })
}

#[gpui_kit::test]
fn the_start_screen_shows_five_templates_and_two_ways_to_open(cx: &mut TestAppContext) {
    let (typing, _places, _root) = open_start(cx);
    assert_eq!(title(&typing, cx), "Stagepack Studio");
    assert!(has_label(&typing, cx, "New presentation"));
    assert_eq!(templates().len(), 5);
    let mut lefts = Vec::new();
    for each in templates() {
        assert!(shows(&typing, cx, template_tile_id(each.id)), "{}", each.id);
        let inside = labels_within(&typing, cx, template_tile_id(each.id));
        assert!(inside.iter().any(|label| label == each.name), "{}: {inside:?}", each.id);
        assert!(inside.iter().any(|label| label == each.says), "{}: {inside:?}", each.id);
        lefts.push(bounds_of(&typing, cx, template_tile_id(each.id)).unwrap().left());
    }
    assert!(lefts.windows(2).all(|pair| pair[0] < pair[1]), "in a row, in order: {lefts:?}");
    assert_eq!(label_of(&typing, cx, START_OPEN_FILE_ID).as_deref(), Some("Open a talk file…"));
    assert_eq!(label_of(&typing, cx, START_OPEN_FOLDER_ID).as_deref(), Some("Open a talk folder…"));
    // Nothing kept from last time, so no list, and nothing wrong.
    assert!(!has_label(&typing, cx, "Not saved last time"));
    assert!(!shows(&typing, cx, START_PROBLEM_ID));
    // No slide list, notes or slide.
    assert!(!has_talk(&typing, cx));
    assert!(!shows(&typing, cx, strip_row_id(0)));
    assert!(!shows(&typing, cx, CANVAS_ID));
}

#[gpui_kit::test]
fn the_first_tile_has_the_keyboard_and_enter_makes_it(cx: &mut TestAppContext) {
    let (typing, _places, _root) = open_start(cx);
    assert!(has_keyboard(&typing, cx, template_tile_id("light")));
    typing.press(cx, "enter");
    assert!(has_talk(&typing, cx));
    assert_eq!(typing.source(cx), template("light").unwrap().slides);
}

#[gpui_kit::test]
fn left_and_right_move_along_the_tiles(cx: &mut TestAppContext) {
    let (typing, _places, _root) = open_start(cx);
    typing.press(cx, "right");
    assert!(has_keyboard(&typing, cx, template_tile_id("dark")));
    assert!(!has_keyboard(&typing, cx, template_tile_id("light")));
    for _ in 0..4 {
        typing.press(cx, "right");
    }
    assert!(has_keyboard(&typing, cx, template_tile_id("tour")), "the last stays the last");
    typing.press(cx, "left");
    assert!(has_keyboard(&typing, cx, template_tile_id("ocean")));
    for _ in 0..5 {
        typing.press(cx, "left");
    }
    assert!(has_keyboard(&typing, cx, template_tile_id("light")), "the first stays the first");
    typing.press(cx, "right");
    typing.press(cx, "right");
    typing.press(cx, "enter");
    assert_eq!(typing.source(cx), template("warm").unwrap().slides);
}

#[gpui_kit::test]
fn picking_a_template_shows_it_in_this_window_ready_to_type(cx: &mut TestAppContext) {
    let (typing, _places, _root) = open_start(cx);
    let before = windows(cx).len();
    pick(&typing, cx, "light");
    assert_eq!(windows(cx).len(), before, "in this window");
    assert!(has_talk(&typing, cx));
    assert_eq!(title(&typing, cx), "Untitled presentation · 1 / 1");
    assert!(shows(&typing, cx, strip_row_id(0)));
    assert!(!shows(&typing, cx, strip_row_id(1)), "one slide");
    assert!(!shows(&typing, cx, START_OPEN_FILE_ID), "the start screen has gone");
    assert!(typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.find(SAVE_ID).disabled() != Some(true)
    }), "the Save tool is on");
    let at = typing.placeholder_point(cx, "body");
    typing.click(cx, at);
    assert!(typing.caret(cx).is_some(), "typing in Click to add text");
    typing.type_text(cx, "Hello there");
    assert!(typing.source(cx).contains("Hello there"), "{}", typing.source(cx));
    assert!(title(&typing, cx).starts_with(UNSAVED_MARK));
}

#[gpui_kit::test]
fn each_template_can_be_picked(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    for each in templates() {
        let typing = open_start_under(cx, root.path());
        pick(&typing, cx, each.id);
        assert_eq!(typing.source(cx), each.slides, "{}", each.id);
        let untitled = typing.document(cx).read_with(cx, |document, _| document.is_untitled());
        assert!(untitled, "{}", each.id);
    }
}

#[gpui_kit::test]
fn a_tile_shows_its_slides_picture_once_drawn(cx: &mut TestAppContext) {
    let (typing, _places, _root) = open_start(cx);
    for each in templates() {
        assert!(shows(&typing, cx, template_picture_id(each.id)), "{}", each.id);
        let picture = cx.read(|cx| tile_picture(each.id, cx)).expect("drawn");
        let tile = bounds_of(&typing, cx, template_picture_id(each.id)).unwrap();
        let scale = typing.with_window(cx, |window, _| window.scale_factor());
        let drawn = picture.size(0);
        assert_eq!(drawn.width.0 as f32, (f32::from(tile.size.width) * scale).round(), "{}: at the tile's size", each.id);
    }
    // Each in its own colours.
    let light = cx.read(|cx| tile_picture("light", cx)).unwrap();
    let dark = cx.read(|cx| tile_picture("dark", cx)).unwrap();
    assert_ne!(light.as_bytes(0).unwrap()[..4], dark.as_bytes(0).unwrap()[..4]);
    // A later start screen has them at once.
    let again = open_start_unsettled(cx, _root.path(), |_| {});
    assert!(shows(&again, cx, template_picture_id("tour")));
}

#[gpui_kit::test]
fn a_tile_can_be_picked_before_its_picture(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let typing = open_start_unsettled(cx, root.path(), |_| {});
    assert!(!shows(&typing, cx, template_picture_id("dark")), "not drawn yet");
    assert!(shows(&typing, cx, template_tile_id("dark")));
    assert!(labels_within(&typing, cx, template_tile_id("dark")).iter().any(|label| label == "Dark"));
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.click(template_tile_id("dark"), cx);
    });
    assert!(has_talk(&typing, cx));
    typing.settle(cx);
    assert_eq!(typing.source(cx), template("dark").unwrap().slides);
}

#[gpui_kit::test]
fn opening_a_folder_from_the_start_screen_uses_this_window(cx: &mut TestAppContext) {
    let (typing, _places, _root) = open_start(cx);
    let talk = tempfile::tempdir().unwrap();
    std::fs::write(talk.path().join("slides.md"), "# From a folder\n").unwrap();
    let before = windows(cx).len();
    click_on(&typing, cx, START_OPEN_FOLDER_ID);
    answer_open(&typing, cx, true, Some(talk.path().to_path_buf()));
    assert_eq!(windows(cx).len(), before, "in this window");
    assert!(has_talk(&typing, cx));
    assert_eq!(typing.source(cx), "# From a folder\n");
    let followed = cx.read(|cx| typing.view.read(cx).followed_folder().map(Path::to_path_buf));
    assert_eq!(followed.as_deref(), Some(talk.path()), "its saves are followed");
    let name = talk.path().file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(title(&typing, cx), format!("{name} · 1 / 1"));

    // From a window with a talk, Open opens another window. (A file that isn't a talk, as a
    // test can't have a folder watched for real.)
    let before = windows(cx);
    typing.press(cx, "ctrl-o");
    let other = tempfile::tempdir().unwrap();
    std::fs::write(other.path().join("notes.txt"), "Just notes").unwrap();
    answer_open(&typing, cx, false, Some(other.path().join("notes.txt")));
    let new = the_new_window(cx, &before);
    new.settle(cx);
    assert!(shows(&new, cx, PROBLEM_ID), "it says why there");
    assert_eq!(typing.source(cx), "# From a folder\n");
}

#[gpui_kit::test]
fn a_cancelled_dialog_leaves_the_start_screen(cx: &mut TestAppContext) {
    let (typing, _places, _root) = open_start(cx);
    click_on(&typing, cx, START_OPEN_FILE_ID);
    answer_open(&typing, cx, false, None);
    assert!(is_start_screen(&typing, cx));
    assert!(!shows(&typing, cx, START_PROBLEM_ID));
    // And the keys ask too.
    typing.press(cx, "ctrl-shift-o");
    answer_open(&typing, cx, true, None);
    typing.press(cx, "ctrl-o");
    answer_open(&typing, cx, false, None);
    assert!(is_start_screen(&typing, cx));
}

#[gpui_kit::test]
fn a_talk_that_cannot_be_opened_says_why_and_stays(cx: &mut TestAppContext) {
    let (typing, _places, _root) = open_start(cx);
    let no_talk = tempfile::tempdir().unwrap();
    std::fs::write(no_talk.path().join("notes.txt"), "Just notes").unwrap();
    click_on(&typing, cx, START_OPEN_FOLDER_ID);
    answer_open(&typing, cx, true, Some(no_talk.path().to_path_buf()));
    assert!(is_start_screen(&typing, cx));
    assert!(!shows(&typing, cx, PROBLEM_ID));
    assert_eq!(label_of(&typing, cx, START_PROBLEM_ID).as_deref(), Some("That folder has no talk in it."));
    // Under the buttons.
    let problem = bounds_of(&typing, cx, START_PROBLEM_ID).unwrap();
    assert!(problem.top() >= bounds_of(&typing, cx, START_OPEN_FILE_ID).unwrap().bottom());

    click_on(&typing, cx, START_OPEN_FILE_ID);
    answer_open(&typing, cx, false, Some(no_talk.path().join("notes.txt")));
    assert!(is_start_screen(&typing, cx));
    let said = label_of(&typing, cx, START_PROBLEM_ID).expect("it says why");
    assert!(said.contains("isn't a talk"), "{said}");
    assert!(!said.contains(&no_talk.path().display().to_string()), "in words, not a path: {said}");

    // The screen still works.
    pick(&typing, cx, "light");
    assert!(has_talk(&typing, cx));
}

#[gpui_kit::test]
fn unsaved_talks_are_listed_newest_first_with_their_words(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let recovery = root.path().join("recovery");
    let older = keep_untitled(root.path(), "Untitled presentation", "# Older plans\n");
    let three_days_ago = SystemTime::now() - Duration::from_secs(3 * 24 * 60 * 60);
    set_kept_at(&recovery, older, three_days_ago);
    let newer = keep_untitled(root.path(), "Untitled presentation 2", "# Newer plans\n\n---\n\n# Two\n");
    let typing = open_start_under(cx, root.path());
    assert!(has_label(&typing, cx, "Not saved last time"));
    let (newer_row, older_row) = (
        bounds_of(&typing, cx, unsaved_row_id(newer)).expect("listed"),
        bounds_of(&typing, cx, unsaved_row_id(older)).expect("listed"),
    );
    assert!(newer_row.top() < older_row.top(), "newest first");
    let newer_says = labels_within(&typing, cx, unsaved_row_id(newer));
    for words in ["Untitled presentation 2", "Newer plans", "2 slides"] {
        assert!(newer_says.iter().any(|label| label == words), "{words}: {newer_says:?}");
    }
    let listed = Recovery::new(recovery.clone()).untitled().0;
    let newer_when = listed.iter().find(|copy| copy.id == newer).unwrap().when;
    let newer_when_words = when_words(newer_when, SystemTime::now());
    assert!(newer_when_words.starts_with("Today ") || newer_when_words == "Yesterday", "{newer_when_words}");
    assert!(newer_says.contains(&newer_when_words), "{newer_says:?}");
    let older_says = labels_within(&typing, cx, unsaved_row_id(older));
    assert!(older_says.iter().any(|label| label == "1 slide"), "{older_says:?}");
    assert!(older_says.contains(&when_words(three_days_ago, SystemTime::now())), "{older_says:?}");
    assert!(shows(&typing, cx, unsaved_open_id(newer)));
    assert!(shows(&typing, cx, unsaved_discard_id(newer)));
    assert!(!shows(&typing, cx, UNREADABLE_ID));
}

#[test]
fn when_reads_as_a_day_and_a_time() {
    let now = SystemTime::now();
    let today = when_words(now, now);
    assert!(today.starts_with("Today "), "{today}");
    let time = &today["Today ".len()..];
    assert!(time.len() == 5 && time.as_bytes()[2] == b':', "{today}");
    assert_eq!(when_words(now - Duration::from_secs(24 * 60 * 60), now), "Yesterday");
    let long_ago = when_words(now - Duration::from_secs(40 * 24 * 60 * 60), now);
    let months = [
        "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November",
        "December",
    ];
    assert!(months.iter().any(|month| long_ago.contains(month)), "{long_ago}");
    assert!(long_ago.chars().next().unwrap().is_ascii_digit(), "the day first: {long_ago}");
    let years_ago = when_words(now - Duration::from_secs(800 * 24 * 60 * 60), now);
    assert!(years_ago.ends_with(|c: char| c.is_ascii_digit()), "with its year: {years_ago}");
}

#[gpui_kit::test]
fn open_brings_one_back_unsaved_and_takes_it_off_other_start_screens(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let text = "# Plans for the spring\n\n---\n\n# Budget\n";
    let id = keep_untitled(root.path(), "Untitled presentation", text);
    let here = open_start_under(cx, root.path());
    let there = open_start_under(cx, root.path());
    assert!(shows(&here, cx, unsaved_row_id(id)));
    assert!(shows(&there, cx, unsaved_row_id(id)));
    let before = windows(cx).len();
    click_on(&here, cx, unsaved_open_id(id));
    cx.run_until_parked();
    here.settle(cx);
    there.settle(cx);
    assert_eq!(windows(cx).len(), before, "in this window");
    assert!(has_talk(&here, cx));
    assert_eq!(here.source(cx), text);
    assert!(here.document(cx).read_with(cx, |document, _| document.is_untitled()));
    assert!(title(&here, cx).starts_with(UNSAVED_MARK), "unsaved: {}", title(&here, cx));
    assert!(!shows(&there, cx, unsaved_row_id(id)), "taken off the other start screen");
    wait(cx, KEEP_PAUSE);
    there.settle(cx);
    assert!(!shows(&there, cx, unsaved_row_id(id)));
    // Kept again as the window's own, and last time's copy gone.
    let place = here.document(cx).read_with(cx, |document, _| document.place().clone());
    assert_eq!(place, TalkPlace::Untitled(id));
    assert_eq!(Recovery::new(root.path().join("recovery")).untitled().0, Vec::new(), "only the open window's copy");
    // A new start screen doesn't list it either.
    let later = open_start_under(cx, root.path());
    assert!(!shows(&later, cx, unsaved_row_id(id)));
}

#[gpui_kit::test]
fn discard_asks_and_then_removes_it(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let recovery = root.path().join("recovery");
    let gone = keep_untitled(root.path(), "Untitled presentation", "# Let go\n");
    let stays = keep_untitled(root.path(), "Untitled presentation 2", "# Kept\n");
    let typing = open_start_under(cx, root.path());
    let other = open_start_under(cx, root.path());
    click_on(&typing, cx, unsaved_discard_id(gone));
    assert!(has_label(&typing, cx, DISCARD_QUESTION));
    assert_eq!(DISCARD_QUESTION, "Discard this presentation? It can't be brought back.");
    assert_eq!(label_of(&typing, cx, CONFIRM_DISCARD_ID).as_deref(), Some("Discard"));
    assert_eq!(label_of(&typing, cx, CANCEL_DISCARD_ID).as_deref(), Some("Cancel"));
    click_on(&typing, cx, CANCEL_DISCARD_ID);
    cx.run_until_parked();
    assert!(!has_label(&typing, cx, DISCARD_QUESTION));
    assert!(shows(&typing, cx, unsaved_row_id(gone)), "Cancel keeps it");
    assert_eq!(Recovery::new(recovery.clone()).untitled().0.len(), 2);

    click_on(&typing, cx, unsaved_discard_id(gone));
    click_on(&typing, cx, CONFIRM_DISCARD_ID);
    cx.run_until_parked();
    typing.settle(cx);
    other.settle(cx);
    assert!(!shows(&typing, cx, unsaved_row_id(gone)));
    assert!(!shows(&other, cx, unsaved_row_id(gone)), "gone from every start screen");
    assert!(shows(&typing, cx, unsaved_row_id(stays)));
    let left: Vec<UntitledId> = Recovery::new(recovery).untitled().0.iter().map(|copy| copy.id).collect();
    assert_eq!(left, vec![stays]);
    assert!(is_start_screen(&typing, cx));
}

#[gpui_kit::test]
fn some_that_cannot_be_read_are_counted(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let recovery = root.path().join("recovery");
    let fine = keep_untitled(root.path(), "Untitled presentation", "# Fine\n");
    for text in ["# One\n", "# Two\n"] {
        let id = keep_untitled(root.path(), "Untitled presentation", text);
        spoil(&recovery, id);
    }
    let typing = open_start_under(cx, root.path());
    assert!(shows(&typing, cx, unsaved_row_id(fine)));
    assert_eq!(label_of(&typing, cx, UNREADABLE_ID).as_deref(), Some("2 couldn't be read."));

    // With only those, the heading and the count still show.
    let root = tempfile::tempdir().unwrap();
    let id = keep_untitled(root.path(), "Untitled presentation", "# One\n");
    spoil(&root.path().join("recovery"), id);
    let typing = open_start_under(cx, root.path());
    assert_eq!(label_of(&typing, cx, UNREADABLE_ID).as_deref(), Some("1 couldn't be read."));
}

#[gpui_kit::test]
fn with_no_recovery_root_there_is_no_list(cx: &mut TestAppContext) {
    let (handle, view) = cx.update(|cx: &mut App| {
        gpui_kit::init(cx);
        workspace_view::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| WorkspaceView::for_start(None, window, cx))
        })
        .unwrap()
    });
    let scratch = tempfile::tempdir().unwrap();
    let typing = Typing { file: scratch.path().join("none"), _folder: scratch, handle, view };
    typing.settle(cx);
    assert!(is_start_screen(&typing, cx));
    assert!(!has_label(&typing, cx, "Not saved last time"));
    assert!(!shows(&typing, cx, UNREADABLE_ID));
}

#[gpui_kit::test]
fn ctrl_n_opens_a_start_screen_in_a_new_window(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let id = keep_untitled(root.path(), "Untitled presentation", "# From before\n");
    let typing = open_start_under(cx, root.path());
    let before = windows(cx);
    typing.press(cx, "ctrl-n");
    cx.run_until_parked();
    let new = the_new_window(cx, &before);
    new.settle(cx);
    assert!(is_start_screen(&new, cx));
    assert_eq!(title(&new, cx), "Stagepack Studio");
    assert!(shows(&new, cx, unsaved_row_id(id)), "it lists from the same place");
    assert!(is_start_screen(&typing, cx), "the first stays as it was");

    // From a window with a talk too.
    pick(&typing, cx, "warm");
    let before = windows(cx);
    typing.press(cx, "ctrl-n");
    cx.run_until_parked();
    let third = the_new_window(cx, &before);
    third.settle(cx);
    assert!(is_start_screen(&third, cx));
    assert!(has_keyboard(&third, cx, template_tile_id("light")));
    assert_eq!(typing.source(cx), template("warm").unwrap().slides);
}

#[gpui_kit::test]
fn the_file_menu_has_new_presentation_and_shows_the_keys(cx: &mut TestAppContext) {
    let (typing, _places, _root) = open_start(cx);
    for item in ["New presentation…", "Open talk folder…", "Open talk file…"] {
        assert!(file_menu_has(&typing, cx, item), "{item}");
    }
    let first = typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.click(FILE_MENU_ID, cx);
        window.render_frame(cx);
        let menu = window.within("popup-menu");
        let new = menu.find_by_label("New presentation…").bounds();
        let folder = menu.find_by_label("Open talk folder…").bounds();
        new.top() < folder.top()
    });
    assert!(first, "New presentation comes first");
    let mut visual = VisualTestContext::from_window(typing.handle, cx);
    for key in ["kbd:ctrl-n", "kbd:ctrl-o", "kbd:ctrl-shift-o"] {
        assert!(visual.debug_bounds(key).is_some(), "{key} shows beside its item");
    }

    // Picking it opens a start screen in a new window.
    let before = windows(cx);
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let menu = window.within("popup-menu");
        assert_eq!(menu.find(0usize).label(), Some("New presentation…"));
        window.within("popup-menu").click(0usize, cx);
    });
    cx.run_until_parked();
    let new = the_new_window(cx, &before);
    new.settle(cx);
    assert!(is_start_screen(&new, cx));
}

#[gpui_kit::test]
fn a_second_untitled_talk_is_numbered_two(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let first = open_start_under(cx, root.path());
    let second = open_start_under(cx, root.path());
    pick(&first, cx, "light");
    assert_eq!(title(&first, cx), "Untitled presentation · 1 / 1");
    pick(&second, cx, "dark");
    assert_eq!(title(&second, cx), "Untitled presentation 2 · 1 / 1");
    // The lowest not in use: with the first closed, the next is 1 again.
    assert!(VisualTestContext::from_window(first.handle, cx).simulate_close(), "unedited, it closes");
    cx.update_window(first.handle, |_, window, _| window.remove_window()).unwrap();
    cx.run_until_parked();
    let third = open_start_under(cx, root.path());
    pick(&third, cx, "ocean");
    assert_eq!(title(&third, cx), "Untitled presentation · 1 / 1");
    let fourth = open_start_under(cx, root.path());
    pick(&fourth, cx, "warm");
    assert_eq!(title(&fourth, cx), "Untitled presentation 3 · 1 / 1");
}

#[gpui_kit::test]
fn the_start_screen_does_nothing_while_idle(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    keep_untitled(root.path(), "Untitled presentation", "# From before\n");
    let typing = open_start_under(cx, root.path());
    assert!(shows(&typing, cx, template_picture_id("light")), "the tiles are drawn");
    let start = cx.read(|cx| typing.view.read(cx).start_screen().cloned()).expect("a start screen");
    let (view_redraws, _watching_view) = redraws(cx, &typing.view);
    let (start_redraws, _watching_start) = redraws(cx, &start);
    wait(cx, 20_000);
    typing.settle(cx);
    assert_eq!(view_redraws.get(), 0);
    assert_eq!(start_redraws.get(), 0);
}

#[gpui_kit::test]
fn it_writes_nothing(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let _places = UntitledPlaces::under(root.path());
    keep_untitled(root.path(), "Untitled presentation", "# From before\n");
    let id = keep_untitled(root.path(), "Untitled presentation", "# Spoilt\n");
    spoil(&root.path().join("recovery"), id);
    let before = snapshot(root.path());
    let typing = open_start_under(cx, root.path());
    typing.press(cx, "right");
    wait(cx, 20_000);
    typing.settle(cx);
    assert_eq!(snapshot(root.path()), before);
}

#[gpui_kit::test]
fn nothing_on_the_start_screen_shows_code(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    keep_untitled(root.path(), "Untitled presentation", "---\nlayout: two-columns\n---\n\n# <Box /> plans\n");
    let id = keep_untitled(root.path(), "Untitled presentation", "# Spoilt\n");
    spoil(&root.path().join("recovery"), id);
    let typing = open_start_under(cx, root.path());
    let no_talk = tempfile::tempdir().unwrap();
    click_on(&typing, cx, START_OPEN_FOLDER_ID);
    answer_open(&typing, cx, true, Some(no_talk.path().to_path_buf()));
    let said: Vec<String> = typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let labels = window.elements().into_iter().filter(|e| e.visible()).filter_map(|e| e.label().map(str::to_string));
        let texts = gpui_kit::base::test_support::texts(window)
            .into_iter()
            .filter(|text| text.visible())
            .map(|text| text.text().to_string());
        labels.chain(texts).collect()
    });
    assert!(said.iter().any(|words| words == "Not saved last time"), "{said:?}");
    let hyphenated: Vec<&str> =
        slides_format::LAYOUT_NAMES.iter().map(|layout| layout.name).filter(|name| name.contains('-')).collect();
    for words in &said {
        for code in ["<", ".md", "assets/", "slides.md"].iter().chain(&hyphenated) {
            assert!(!words.contains(code), "{words:?} shows {code:?}");
        }
    }
}

/// Builds with a compiler that only counts its calls.
fn counting_compile() -> (Compile, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let compile: Compile = Arc::new(move |_: &str, _: &str| {
        counted.fetch_add(1, Ordering::SeqCst);
        Err(Vec::new())
    });
    (compile, calls)
}

#[gpui_kit::test]
fn a_talk_shown_later_is_as_one_the_window_was_built_with(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let places = UntitledPlaces::under(root.path());
    let talk = tempfile::tempdir().unwrap();
    let folder = talk.path().to_path_buf();
    let source = "# One\n\n<Score />\n";
    std::fs::write(folder.join("slides.md"), source).unwrap();
    std::fs::create_dir(folder.join("components")).unwrap();
    std::fs::write(
        folder.join("components/score.rs"),
        "use toolkit::prelude::*;\n\n#[component]\nfn Score() -> Element {\n    text(\"Score\")\n}\n",
    )
    .unwrap();
    // Changes to it from last time, waiting to be offered.
    let mine = format!("{source}\n---\n\n# Mine\n");
    Recovery::new(places.recovery.clone()).keep(&TalkPlace::Folder(folder.clone()), &hash_text(source), &mine).unwrap();
    let (compile, calls) = counting_compile();
    let typing = open_start_unsettled(cx, root.path(), move |view| view.set_compile_for_test(compile));
    typing.settle(cx);
    click_on(&typing, cx, START_OPEN_FOLDER_ID);
    answer_open(&typing, cx, true, Some(folder.clone()));
    assert_eq!(typing.source(cx), source);

    // The keyboard is where opening the talk leaves it: nothing selected or typed in.
    assert!(typing.editing(cx).is_none());
    assert!(selected(&typing, cx).is_none());

    // Recovery's offer.
    assert!(shows(&typing, cx, RECOVERY_ID), "last time's changes are offered");
    click_on(&typing, cx, DISCARD_RECOVERY_ID);
    cx.run_until_parked();
    assert!(!shows(&typing, cx, RECOVERY_ID));

    // The build bar, with the window's own compiler.
    assert!(shows(&typing, cx, BUILD_BAR_ID));
    click_on(&typing, cx, BUILD_ONCE_ID);
    cx.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Typing on the slide.
    typing.double_click_text(cx, 0, pos(0, 3));
    assert!(typing.editing(cx).is_some());
    typing.press(cx, "end");
    typing.type_text(cx, "!");
    assert!(typing.source(cx).starts_with("# One!"), "{}", typing.source(cx));
    typing.press(cx, "escape");

    // Selecting slides in the list.
    typing.press(cx, "ctrl-m");
    assert_eq!(typing.slide_count(cx), 2);
    click_on(&typing, cx, strip_row_id(0));
    assert_eq!(typing.current(cx), 0);
    assert!(title(&typing, cx).starts_with(UNSAVED_MARK));

    // Saving where it's kept.
    typing.press(cx, "ctrl-s");
    cx.run_until_parked();
    typing.settle(cx);
    let saved = std::fs::read_to_string(folder.join("slides.md")).unwrap();
    assert_eq!(saved, typing.source(cx));
    assert!(!title(&typing, cx).starts_with(UNSAVED_MARK));

    // The watcher: a save from outside comes in.
    let followed = cx.read(|cx| typing.view.read(cx).followed_folder().map(Path::to_path_buf));
    assert_eq!(followed.as_deref(), Some(folder.as_path()));
    std::fs::write(folder.join("slides.md"), "# Changed outside\n").unwrap();
    let file = folder.join("slides.md");
    cx.update(|cx| typing.view.update(cx, |view, cx| view.reload_soon(file, cx)));
    wait(cx, 500);
    typing.settle(cx);
    assert_eq!(typing.source(cx), "# Changed outside\n");
}

/// A file holding Stagepack's manifest: one of its Slidev talks.
fn stagepack_file(dir: &Path) -> PathBuf {
    use std::io::Write as _;

    let file = dir.join("slidev.sptx");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&file).unwrap());
    zip.start_file("stagepack.json", zip::write::SimpleFileOptions::default()).unwrap();
    zip.write_all(b"{}").unwrap();
    zip.finish().unwrap();
    file
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_slidev_talk_can_still_be_opened_in_stagepack(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;

    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = stagepack_file(dir.path());
    let said = dir.path().join("said");
    let stagepack = dir.path().join("stagepack");
    std::fs::write(
        &stagepack,
        format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{0}.tmp'\nmv '{0}.tmp' '{0}'\n", said.display()),
    )
    .unwrap();
    std::fs::set_permissions(&stagepack, std::fs::Permissions::from_mode(0o755)).unwrap();

    // With no Stagepack, it says what the file is, and offers nothing.
    let typing = open_start_unsettled(cx, root.path(), |view| view.set_stagepack_for_test(None));
    typing.settle(cx);
    click_on(&typing, cx, START_OPEN_FILE_ID);
    answer_open(&typing, cx, false, Some(file.clone()));
    assert!(is_start_screen(&typing, cx));
    let says = label_of(&typing, cx, START_PROBLEM_ID).expect("it says what the file is");
    assert!(says.contains("Slidev talk made for Stagepack"), "{says}");
    assert!(!shows(&typing, cx, OPEN_IN_STAGEPACK_ID));

    // With one, its button opens the file there.
    let typing = open_start_unsettled(cx, root.path(), move |view| view.set_stagepack_for_test(Some(stagepack)));
    typing.settle(cx);
    click_on(&typing, cx, START_OPEN_FILE_ID);
    answer_open(&typing, cx, false, Some(file.clone()));
    assert!(shows(&typing, cx, OPEN_IN_STAGEPACK_ID));
    let button = bounds_of(&typing, cx, OPEN_IN_STAGEPACK_ID).unwrap();
    assert!(button.top() >= bounds_of(&typing, cx, START_OPEN_FILE_ID).unwrap().bottom(), "under the buttons");
    click_on(&typing, cx, OPEN_IN_STAGEPACK_ID);
    let start = std::time::Instant::now();
    while !said.exists() {
        assert!(start.elapsed() < Duration::from_secs(10), "it never ran");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(std::fs::read_to_string(&said).unwrap(), format!("open\n{}\n", file.display()));
    assert!(is_start_screen(&typing, cx));
}
