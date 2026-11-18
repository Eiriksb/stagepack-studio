use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, Point, TestAppContext, VisualTestContext,
    WindowBounds, WindowOptions, px, size,
};
use slides_engine::Fonts;
use stagepack_studio::document::{Document, TalkPlace, UntitledId};
use stagepack_studio::notes_panel::NOTES_ID;
use stagepack_studio::recovery::{KEEP_FOR, NotTaken, Recovery, hash_text};
use stagepack_studio::saving::SaveTarget;
use stagepack_studio::ribbon::{HIDE_ID, NEW_SLIDE_ID, RIBBON_ID};
use stagepack_studio::workspace_view::{
    self, CANVAS_ID, CLOSE_DONT_SAVE_ID, DISCARD_RECOVERY_ID, RECOVER_ID, RECOVERY_CHANGED_SINCE,
    RECOVERY_ID, RECOVERY_OFFER, SAVE_AS_FOLDER_ID, STATUS_ID, TITLE_ID, WorkspaceView,
};

const ONE: &str = "---\ntitle: T\n---\n\n# One\n";
const EDITED: &str = "---\ntitle: T\n---\n\n# One\n\n---\n\n# Two\n";

/// A talk folder holding `source`, and an app data folder of its own for the working copies.
struct Setup {
    _app_data: tempfile::TempDir,
    root: PathBuf,
    /// One slot, as one window has.
    recovery: Recovery,
    _dir: tempfile::TempDir,
    folder: PathBuf,
}

impl Setup {
    fn new(source: &str) -> Setup {
        let app_data = tempfile::tempdir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("talk");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("slides.md"), source).unwrap();
        let root = app_data.path().join("recovery");
        Setup {
            recovery: Recovery::new(root.clone()),
            root,
            _app_data: app_data,
            _dir: dir,
            folder,
        }
    }

    fn recovery(&self) -> Recovery {
        self.recovery.clone()
    }

    fn place(&self) -> TalkPlace {
        TalkPlace::Folder(self.folder.clone())
    }

    fn file(&self) -> PathBuf {
        self.folder.join("slides.md")
    }

    /// The talk's folder under the root, the only one there.
    fn key_folder(&self) -> PathBuf {
        only_entry(&self.root)
    }

    /// The working copy's own folder, the only one in the talk's.
    fn kept_folder(&self) -> PathBuf {
        only_entry(&self.key_folder())
    }
}

fn only_entry(folder: &Path) -> PathBuf {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(entries.len(), 1, "{entries:?}");
    entries.remove(0)
}

/// Every working copy's folder under `root`.
fn kept_folders(root: &Path) -> Vec<PathBuf> {
    let Ok(keys) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    keys.flatten()
        .filter(|key| key.path().is_dir())
        .flat_map(|key| std::fs::read_dir(key.path()).unwrap().flatten())
        .map(|slot| slot.path())
        .collect()
}

fn is_hex_name(path: &Path) -> bool {
    let name = path.file_name().unwrap().to_string_lossy();
    name.len() == 16
        && name
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

impl Setup {}

/// Keeps `text` as the working copy of the talk in `setup`, made from what its file holds.
fn keep(setup: &Setup, text: &str) {
    let base = std::fs::read_to_string(setup.file()).unwrap();
    setup
        .recovery()
        .keep(&setup.place(), &hash_text(&base), text)
        .unwrap();
}

#[test]
fn a_working_copy_is_kept_and_found() {
    let setup = Setup::new(ONE);
    let before = SystemTime::now() - Duration::from_secs(2);
    keep(&setup, EDITED);

    let found = setup.recovery().find(&setup.place()).expect("it's kept");
    assert_eq!(found.text, EDITED);
    assert!(found.base_matches);
    assert!(found.when >= before && found.when <= SystemTime::now() + Duration::from_secs(2));

    let kept = setup.kept_folder();
    assert!(is_hex_name(&setup.key_folder()), "{kept:?}");
    assert!(is_hex_name(&kept), "{kept:?}");
    assert_eq!(
        std::fs::read_to_string(kept.join("slides.md")).unwrap(),
        EDITED
    );
    let about: serde_json::Value =
        serde_json::from_slice(&std::fs::read(kept.join("about.json")).unwrap()).unwrap();
    assert_eq!(
        about["talk"].as_str().map(PathBuf::from),
        Some(setup.folder.canonicalize().unwrap())
    );
    assert_eq!(about["base_sha256"].as_str(), Some(hash_text(ONE).as_str()));
    // The talk itself is left as it was.
    assert_eq!(std::fs::read_to_string(setup.file()).unwrap(), ONE);
}

#[cfg(unix)]
#[test]
fn a_working_copy_is_private() {
    use std::os::unix::fs::PermissionsExt as _;

    let setup = Setup::new(ONE);
    keep(&setup, EDITED);
    keep(&setup, ONE);
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let kept = setup.kept_folder();
    assert_eq!(mode(&setup.root), 0o700);
    assert_eq!(mode(&setup.key_folder()), 0o700);
    assert_eq!(mode(&kept), 0o700);
    assert_eq!(mode(&kept.join("slides.md")), 0o600);
    assert_eq!(mode(&kept.join("about.json")), 0o600);
}

#[test]
fn clearing_removes_it() {
    let setup = Setup::new(ONE);
    keep(&setup, EDITED);
    setup.recovery().clear(&setup.place());
    assert!(setup.recovery().find(&setup.place()).is_none());
    assert_eq!(std::fs::read_dir(&setup.root).unwrap().count(), 0);
}

#[test]
fn two_talks_with_the_same_folder_name_do_not_mix() {
    let first = Setup::new(ONE);
    let second = Setup::new(ONE);
    let recovery = first.recovery();
    let base = hash_text(ONE);
    let other = TalkPlace::Folder(second.folder.clone());
    recovery.keep(&first.place(), &base, "# First\n").unwrap();
    recovery.keep(&other, &base, "# Second\n").unwrap();

    assert_eq!(recovery.find(&first.place()).unwrap().text, "# First\n");
    assert_eq!(recovery.find(&other).unwrap().text, "# Second\n");
    recovery.clear(&first.place());
    assert!(recovery.find(&first.place()).is_none());
    assert_eq!(recovery.find(&other).unwrap().text, "# Second\n");
}

#[test]
fn a_talk_written_another_way_is_the_same_talk() {
    let setup = Setup::new(ONE);
    keep(&setup, EDITED);
    let roundabout = TalkPlace::Folder(setup.folder.join("..").join("talk"));
    assert_eq!(setup.recovery().find(&roundabout).unwrap().text, EDITED);
}

#[test]
fn a_changed_talk_does_not_match_its_base() {
    let setup = Setup::new(ONE);
    keep(&setup, EDITED);
    std::fs::write(setup.file(), "# Changed since\n").unwrap();
    let found = setup.recovery().find(&setup.place()).unwrap();
    assert_eq!(found.text, EDITED);
    assert!(!found.base_matches);
}

#[test]
fn a_broken_about_file_is_ignored() {
    let setup = Setup::new(ONE);
    keep(&setup, EDITED);
    let about = setup.kept_folder().join("about.json");
    std::fs::write(&about, "{ not json").unwrap();
    assert!(setup.recovery().find(&setup.place()).is_none());

    // One that names another talk isn't this one's.
    let elsewhere = tempfile::tempdir().unwrap();
    let json = serde_json::json!({
        "talk": elsewhere.path(),
        "base_sha256": hash_text(ONE),
        "kept_at": 1_700_000_000u64,
    });
    std::fs::write(&about, json.to_string()).unwrap();
    assert!(setup.recovery().find(&setup.place()).is_none());

    // A time no clock can hold.
    let json = serde_json::json!({
        "talk": setup.folder.canonicalize().unwrap(),
        "base_sha256": hash_text(ONE),
        "kept_at": 18446744073709551615u64,
    });
    std::fs::write(&about, json.to_string()).unwrap();
    assert!(setup.recovery().find(&setup.place()).is_none());
    setup.recovery().sweep(KEEP_FOR);

    std::fs::remove_file(&about).unwrap();
    assert!(setup.recovery().find(&setup.place()).is_none());
}

#[cfg(unix)]
#[test]
fn a_working_copy_is_never_written_through_a_link() {
    let setup = Setup::new(ONE);
    keep(&setup, EDITED);
    let key = setup.key_folder();
    std::fs::remove_dir_all(&key).unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), &key).unwrap();

    let kept = setup
        .recovery()
        .keep(&setup.place(), &hash_text(ONE), EDITED);

    assert!(kept.is_err());
    assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 0);
}

#[test]
fn each_window_keeps_its_own_copy() {
    let setup = Setup::new(ONE);
    let other = Recovery::new(setup.root.clone());
    let base = hash_text(ONE);
    setup
        .recovery()
        .keep(&setup.place(), &base, "# Mine\n")
        .unwrap();
    // Kept later.
    std::thread::sleep(Duration::from_millis(20));
    other.keep(&setup.place(), &base, "# Theirs\n").unwrap();
    assert_eq!(kept_folders(&setup.root).len(), 2);

    // The newest is found first.
    assert_eq!(
        setup.recovery().find(&setup.place()).unwrap().text,
        "# Theirs\n"
    );
    other.clear(&setup.place());
    assert_eq!(
        setup.recovery().find(&setup.place()).unwrap().text,
        "# Mine\n"
    );

    // Letting a found copy go removes that one.
    let found = other.find(&setup.place()).unwrap();
    other.discard(&found);
    assert!(setup.recovery().find(&setup.place()).is_none());
    assert_eq!(std::fs::read_dir(&setup.root).unwrap().count(), 0);
}

#[test]
fn an_enormous_working_copy_is_ignored() {
    let setup = Setup::new(ONE);
    keep(&setup, EDITED);
    let slides = setup.kept_folder().join("slides.md");
    // Sparse, so it takes no room.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&slides)
        .unwrap();
    file.set_len(64 * 1024 * 1024 + 1).unwrap();
    drop(file);
    assert!(setup.recovery().find(&setup.place()).is_none());
}

#[test]
fn old_working_copies_are_swept() {
    let old = Setup::new(ONE);
    let recent = Setup::new(ONE);
    let recovery = old.recovery();
    let base = hash_text(ONE);
    let recent_place = TalkPlace::Folder(recent.folder.clone());
    recovery.keep(&old.place(), &base, EDITED).unwrap();
    // Something else in the folder is left alone.
    std::fs::write(old.root.join("not-a-working-copy"), "mine").unwrap();
    let about = old.kept_folder_by(&recovery);
    let mut json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&about).unwrap()).unwrap();
    let month_ago = SystemTime::now() - KEEP_FOR - Duration::from_secs(60 * 60);
    json["kept_at"] = month_ago
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .into();
    std::fs::write(&about, json.to_string()).unwrap();
    recovery.keep(&recent_place, &base, EDITED).unwrap();

    recovery.sweep(KEEP_FOR);

    assert!(recovery.find(&old.place()).is_none());
    assert!(!about.exists());
    assert!(
        !about.parent().unwrap().parent().unwrap().exists(),
        "its talk's folder goes too"
    );
    assert_eq!(recovery.find(&recent_place).unwrap().text, EDITED);
    assert!(old.root.join("not-a-working-copy").exists());
}

impl Setup {
    /// The about file of this talk's working copy, among others under the root.
    fn kept_folder_by(&self, recovery: &Recovery) -> PathBuf {
        assert!(recovery.find(&self.place()).is_some());
        let canonical = self.folder.canonicalize().unwrap();
        kept_folders(&self.root)
            .into_iter()
            .map(|slot| slot.join("about.json"))
            .filter(|about| about.is_file())
            .find(|about| {
                let json: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(about).unwrap()).unwrap();
                json["talk"].as_str().map(PathBuf::from) == Some(canonical.clone())
            })
            .expect("its about file")
    }
}

// The window.

fn open_view(cx: &mut TestAppContext, setup: &Setup) -> (AnyWindowHandle, Entity<WorkspaceView>) {
    let talk = Document::open(&setup.folder, Fonts::bundled_only()).unwrap();
    open_view_on(cx, setup, move |window, cx| {
        WorkspaceView::open_with(talk, window, cx)
    })
}

fn open_view_on(
    cx: &mut TestAppContext,
    setup: &Setup,
    build: impl FnOnce(&mut gpui_kit::Window, &mut gpui_kit::Context<WorkspaceView>) -> WorkspaceView
    + 'static,
) -> (AnyWindowHandle, Entity<WorkspaceView>) {
    let recovery = setup.recovery();
    let (handle, view) = cx.update(|cx: &mut App| {
        gpui_kit::init(cx);
        workspace_view::bind_keys(cx);
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
                    let mut view = build(window, cx);
                    view.set_recovery_for_test(recovery, cx);
                    view
                })
            },
        )
        .expect("open test window")
    });
    settle(cx, handle);
    (handle, view)
}

fn settle(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    for _ in 0..2 {
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.run_until_parked();
    }
}

fn wait(cx: &mut TestAppContext, millis: u64) {
    cx.executor().advance_clock(Duration::from_millis(millis));
    cx.run_until_parked();
}

fn document(cx: &TestAppContext, view: &Entity<WorkspaceView>) -> Entity<Document> {
    cx.read(|cx| view.read(cx).document().unwrap().clone())
}

fn add_slide(cx: &mut TestAppContext, view: &Entity<WorkspaceView>) {
    let document = document(cx, view);
    cx.update(|cx| {
        document.update(cx, |document, cx| {
            document.add_slide("content").unwrap();
            cx.notify();
        });
    });
}

fn undo(cx: &mut TestAppContext, view: &Entity<WorkspaceView>) {
    let document = document(cx, view);
    cx.update(|cx| {
        document.update(cx, |document, cx| {
            document.undo().unwrap();
            cx.notify();
        });
    });
}

fn source_of(cx: &TestAppContext, view: &Entity<WorkspaceView>) -> String {
    let document = document(cx, view);
    cx.read(|cx| document.read(cx).source().to_string())
}

fn title(cx: &mut TestAppContext, handle: AnyWindowHandle) -> String {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find(TITLE_ID).label().map(str::to_string).unwrap()
    })
    .unwrap()
}

/// What the recovery bar says, if it's showing.
fn bar(cx: &mut TestAppContext, handle: AnyWindowHandle) -> Option<String> {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window
            .try_find(RECOVERY_ID)
            .map(|bar| bar.label().expect("the bar has a label").to_string())
    })
    .unwrap()
}

fn click(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
    cx.run_until_parked();
    settle(cx, handle);
}

/// Closes the window as a crash would: without asking, and without anything cleared. The
/// caller drops any handle it has on the view first, as nothing of a stopped app is left.
fn crash(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
}

/// A talk edited in a window, its working copy kept, and the window gone without saving.
fn left_unsaved(cx: &mut TestAppContext) -> (Setup, String) {
    let setup = Setup::new(ONE);
    let (handle, view) = open_view(cx, &setup);
    add_slide(cx, &view);
    wait(cx, 1_000);
    let edited = source_of(cx, &view);
    assert!(setup.recovery().find(&setup.place()).is_some());
    crash(cx, handle);
    (setup, edited)
}

#[gpui_kit::test]
fn the_working_copy_follows_edits_after_a_moment(cx: &mut TestAppContext) {
    let setup = Setup::new(ONE);
    let (_handle, view) = open_view(cx, &setup);
    let recovery = setup.recovery();
    wait(cx, 5_000);
    assert!(
        recovery.find(&setup.place()).is_none(),
        "nothing unsaved, nothing kept"
    );

    add_slide(cx, &view);
    wait(cx, 900);
    assert!(
        recovery.find(&setup.place()).is_none(),
        "not on every keystroke"
    );
    wait(cx, 100);
    assert_eq!(
        recovery.find(&setup.place()).unwrap().text,
        source_of(cx, &view)
    );

    add_slide(cx, &view);
    wait(cx, 600);
    add_slide(cx, &view);
    wait(cx, 600);
    let kept = recovery.find(&setup.place()).unwrap().text;
    // The talk's settings, and one new slide's line.
    assert_eq!(
        kept.lines().filter(|line| *line == "---").count(),
        3,
        "each edit starts the wait again: {kept:?}"
    );
    wait(cx, 400);
    assert_eq!(
        recovery.find(&setup.place()).unwrap().text,
        source_of(cx, &view)
    );

    // Back to what's saved, there's nothing to keep.
    for _ in 0..3 {
        undo(cx, &view);
    }
    wait(cx, 1_000);
    assert!(recovery.find(&setup.place()).is_none());
    assert_eq!(std::fs::read_to_string(setup.file()).unwrap(), ONE);
}

#[gpui_kit::test]
fn edits_that_never_stop_are_kept_within_three_seconds(cx: &mut TestAppContext) {
    let setup = Setup::new(ONE);
    let (_handle, view) = open_view(cx, &setup);
    let recovery = setup.recovery();
    add_slide(cx, &view);
    for _ in 0..5 {
        wait(cx, 500);
        add_slide(cx, &view);
    }
    wait(cx, 499);
    assert!(
        recovery.find(&setup.place()).is_none(),
        "each edit within the wait"
    );
    wait(cx, 1);
    assert_eq!(
        recovery.find(&setup.place()).unwrap().text,
        source_of(cx, &view),
        "three seconds after the first edit not kept"
    );

    // The next edit starts the count again.
    add_slide(cx, &view);
    wait(cx, 999);
    assert_ne!(
        recovery.find(&setup.place()).unwrap().text,
        source_of(cx, &view)
    );
    wait(cx, 1);
    assert_eq!(
        recovery.find(&setup.place()).unwrap().text,
        source_of(cx, &view)
    );
}

#[gpui_kit::test]
fn closing_just_after_a_save_leaves_no_copy(cx: &mut TestAppContext) {
    let setup = Setup::new(ONE);
    let (handle, view) = open_view(cx, &setup);
    add_slide(cx, &view);
    wait(cx, 1_000);
    assert!(setup.recovery().find(&setup.place()).is_some());
    let document = document(cx, &view);
    drop(view);

    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(workspace_view::Save), cx)
    })
    .unwrap();
    // Only as far as the save being taken in: the copy's clear is still to come.
    while cx.read(|cx| document.read(cx).has_unsaved_changes()) {
        assert!(cx.executor().tick(), "the save runs");
    }
    if VisualTestContext::from_window(handle, cx).simulate_close() {
        // As the system does with a window that says it may close.
        crash(cx, handle);
    }
    cx.run_until_parked();

    assert!(!is_open(cx, handle));
    assert!(
        kept_folders(&setup.root).is_empty(),
        "{:?}",
        kept_folders(&setup.root)
    );
}

#[gpui_kit::test]
fn edits_made_in_the_window_are_kept_like_any_other(cx: &mut TestAppContext) {
    let setup = Setup::new(ONE);
    let (handle, view) = open_view(cx, &setup);
    let recovery = setup.recovery();

    click(cx, handle, NEW_SLIDE_ID);
    wait(cx, 1_000);
    assert_eq!(
        recovery.find(&setup.place()).unwrap().text,
        source_of(cx, &view)
    );

    click(cx, handle, HIDE_ID);
    wait(cx, 1_000);
    assert_eq!(
        recovery.find(&setup.place()).unwrap().text,
        source_of(cx, &view)
    );

    click(cx, handle, NOTES_ID);
    cx.update_window(handle, |_, window, cx| window.input("Kept", cx))
        .unwrap();
    wait(cx, 1_000);
    let kept = recovery.find(&setup.place()).unwrap().text;
    assert!(kept.contains("Kept"), "{kept}");
    assert_eq!(kept, source_of(cx, &view));
}

#[gpui_kit::test]
fn it_is_cleared_by_a_save(cx: &mut TestAppContext) {
    let setup = Setup::new(ONE);
    let (handle, view) = open_view(cx, &setup);
    add_slide(cx, &view);
    wait(cx, 1_000);
    assert!(setup.recovery().find(&setup.place()).is_some());

    cx.simulate_keystrokes(handle, "ctrl-s");
    cx.run_until_parked();

    assert_eq!(
        std::fs::read_to_string(setup.file()).unwrap(),
        source_of(cx, &view)
    );
    assert!(setup.recovery().find(&setup.place()).is_none());
    wait(cx, 1_000);
    assert!(setup.recovery().find(&setup.place()).is_none());
}

#[gpui_kit::test]
fn and_by_choosing_not_to_save(cx: &mut TestAppContext) {
    let setup = Setup::new(ONE);
    let (handle, view) = open_view(cx, &setup);
    add_slide(cx, &view);
    wait(cx, 1_000);
    assert!(setup.recovery().find(&setup.place()).is_some());

    let closed = VisualTestContext::from_window(handle, cx).simulate_close();
    cx.run_until_parked();
    assert!(!closed, "it asks first");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(CLOSE_DONT_SAVE_ID, cx);
    })
    .unwrap();
    cx.run_until_parked();
    let open = cx.read(|cx| {
        cx.windows()
            .iter()
            .any(|w| w.window_id() == handle.window_id())
    });
    assert!(!open);

    assert!(setup.recovery().find(&setup.place()).is_none());
    assert_eq!(std::fs::read_to_string(setup.file()).unwrap(), ONE);
}

#[gpui_kit::test]
fn unsaved_work_is_offered_back_after_a_crash(cx: &mut TestAppContext) {
    let (setup, edited) = left_unsaved(cx);

    let (handle, view) = open_view(cx, &setup);
    assert_eq!(source_of(cx, &view), ONE);
    assert_eq!(bar(cx, handle).as_deref(), Some(RECOVERY_OFFER));
    assert_eq!(title(cx, handle), "T · 1 / 1");

    click(cx, handle, RECOVER_ID);

    assert_eq!(source_of(cx, &view), edited);
    assert_eq!(bar(cx, handle), None);
    assert_eq!(title(cx, handle), "• T · 1 / 2");
    assert_eq!(
        std::fs::read_to_string(setup.file()).unwrap(),
        ONE,
        "nothing on disk until Save"
    );
    let document = document(cx, &view);
    let undo_label = cx.read(|cx| document.read(cx).undo_label());
    assert_eq!(undo_label, Some("Recover changes"));
    // It's still kept, as it's still unsaved.
    wait(cx, 1_000);
    assert_eq!(setup.recovery().find(&setup.place()).unwrap().text, edited);

    // One step to undo.
    undo(cx, &view);
    assert_eq!(source_of(cx, &view), ONE);
    assert_eq!(title(cx, handle), "T · 1 / 1");
}

#[gpui_kit::test]
fn discard_removes_it(cx: &mut TestAppContext) {
    let (setup, _) = left_unsaved(cx);
    let (handle, view) = open_view(cx, &setup);
    assert!(bar(cx, handle).is_some());

    click(cx, handle, DISCARD_RECOVERY_ID);

    assert_eq!(bar(cx, handle), None);
    assert_eq!(source_of(cx, &view), ONE);
    assert!(setup.recovery().find(&setup.place()).is_none());
    assert_eq!(title(cx, handle), "T · 1 / 1");
}

#[gpui_kit::test]
fn a_talk_changed_since_says_so(cx: &mut TestAppContext) {
    let (setup, edited) = left_unsaved(cx);
    let theirs = "---\ntitle: T\n---\n\n# Theirs\n";
    std::fs::write(setup.file(), theirs).unwrap();

    let (handle, view) = open_view(cx, &setup);
    assert_eq!(
        bar(cx, handle),
        Some(format!("{RECOVERY_OFFER} {RECOVERY_CHANGED_SINCE}"))
    );
    click(cx, handle, RECOVER_ID);
    assert_eq!(source_of(cx, &view), edited);
    undo(cx, &view);
    assert_eq!(source_of(cx, &view), theirs);
}

#[gpui_kit::test]
fn a_kill_mid_save_leaves_the_talk_whole(cx: &mut TestAppContext) {
    let (setup, edited) = left_unsaved(cx);
    // What an interrupted write_atomically leaves: a part-written file beside the talk's.
    let temp = setup.folder.join(format!(
        ".slides.md.{}.00c0ffee00c0ffee.tmp",
        std::process::id()
    ));
    std::fs::write(&temp, &edited.as_bytes()[..edited.len() / 2]).unwrap();

    let (handle, view) = open_view(cx, &setup);
    assert_eq!(source_of(cx, &view), ONE);
    assert_eq!(bar(cx, handle).as_deref(), Some(RECOVERY_OFFER));
    click(cx, handle, RECOVER_ID);
    assert_eq!(source_of(cx, &view), edited);
}

#[gpui_kit::test]
fn a_working_copy_that_is_what_the_file_holds_is_not_offered(cx: &mut TestAppContext) {
    let setup = Setup::new(ONE);
    keep(&setup, ONE);
    let (handle, _view) = open_view(cx, &setup);
    assert_eq!(bar(cx, handle), None);
}

fn is_open(cx: &TestAppContext, handle: AnyWindowHandle) -> bool {
    cx.read(|cx| {
        cx.windows()
            .iter()
            .any(|w| w.window_id() == handle.window_id())
    })
}

fn status(cx: &mut TestAppContext, handle: AnyWindowHandle) -> Option<String> {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window
            .try_find(STATUS_ID)
            .map(|status| status.label().unwrap().to_string())
    })
    .unwrap()
}

fn offered(cx: &mut TestAppContext, setup: &Setup) -> Option<String> {
    let (handle, view) = open_view(cx, setup);
    let offered = bar(cx, handle).map(|_| {
        click(cx, handle, RECOVER_ID);
        source_of(cx, &view)
    });
    crash(cx, handle);
    offered
}

#[gpui_kit::test]
fn the_offered_copy_is_kept_until_answered(cx: &mut TestAppContext) {
    let (setup, edited) = left_unsaved(cx);
    let (handle, view) = open_view(cx, &setup);
    assert!(bar(cx, handle).is_some());
    // Edits before answering, back to saved and saved again: none of it touches last time's.
    add_slide(cx, &view);
    add_slide(cx, &view);
    wait(cx, 1_000);
    let meanwhile = source_of(cx, &view);
    undo(cx, &view);
    undo(cx, &view);
    wait(cx, 1_000);
    add_slide(cx, &view);
    add_slide(cx, &view);
    wait(cx, 1_000);
    // A window that stopped holds nothing.
    drop(view);
    crash(cx, handle);

    // Newest first, and neither is dropped.
    assert_eq!(offered(cx, &setup).as_deref(), Some(meanwhile.as_str()));
    let (handle, _view) = open_view(cx, &setup);
    click(cx, handle, DISCARD_RECOVERY_ID);
    crash(cx, handle);
    assert_eq!(offered(cx, &setup), Some(edited));
}

#[gpui_kit::test]
fn saving_with_the_bar_up_leaves_last_times_copy(cx: &mut TestAppContext) {
    let (setup, edited) = left_unsaved(cx);
    let (handle, view) = open_view(cx, &setup);
    // Not what last time's copy holds, which would have nothing to offer once saved.
    add_slide(cx, &view);
    add_slide(cx, &view);
    wait(cx, 1_000);
    cx.simulate_keystrokes(handle, "ctrl-s");
    cx.run_until_parked();
    crash(cx, handle);
    assert_eq!(offered(cx, &setup), Some(edited));
}

#[gpui_kit::test]
fn bringing_them_back_takes_their_place(cx: &mut TestAppContext) {
    let (setup, edited) = left_unsaved(cx);
    let (handle, _view) = open_view(cx, &setup);
    click(cx, handle, RECOVER_ID);
    // Kept as this window's own straight away, and last time's let go after.
    assert_eq!(kept_folders(&setup.root).len(), 1);
    assert_eq!(setup.recovery().find(&setup.place()).unwrap().text, edited);
    cx.simulate_keystrokes(handle, "ctrl-s");
    cx.run_until_parked();
    assert!(setup.recovery().find(&setup.place()).is_none());
}

#[gpui_kit::test(iterations = 30)]
fn a_save_never_leaves_an_older_copy_behind(cx: &mut TestAppContext) {
    let setup = Setup::new(ONE);
    let (handle, view) = open_view(cx, &setup);
    add_slide(cx, &view);
    // The wait for the working copy is over just as the save starts.
    cx.executor().advance_clock(Duration::from_millis(1_000));
    cx.simulate_keystrokes(handle, "ctrl-s");
    cx.run_until_parked();
    wait(cx, 2_000);
    assert!(
        kept_folders(&setup.root).is_empty(),
        "{:?}",
        kept_folders(&setup.root)
    );
}

#[gpui_kit::test(iterations = 30)]
fn dont_save_never_leaves_a_copy_behind(cx: &mut TestAppContext) {
    let setup = Setup::new(ONE);
    let (handle, view) = open_view(cx, &setup);
    add_slide(cx, &view);
    wait(cx, 1_000);
    add_slide(cx, &view);
    cx.executor().advance_clock(Duration::from_millis(1_000));
    assert!(!VisualTestContext::from_window(handle, cx).simulate_close());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(CLOSE_DONT_SAVE_ID, cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!is_open(cx, handle));
    assert!(
        kept_folders(&setup.root).is_empty(),
        "{:?}",
        kept_folders(&setup.root)
    );
}

#[gpui_kit::test]
fn a_failed_keep_says_so_and_tries_again(cx: &mut TestAppContext) {
    let setup = Setup::new(ONE);
    // Nowhere a folder can be made.
    std::fs::create_dir_all(setup.root.parent().unwrap()).unwrap();
    std::fs::write(&setup.root, "in the way").unwrap();
    let (handle, view) = open_view(cx, &setup);
    add_slide(cx, &view);
    wait(cx, 1_000);
    let said = status(cx, handle).expect("it says so");
    assert!(
        said.starts_with("Unsaved changes couldn't be kept safe"),
        "{said}"
    );
    assert!(!said.contains("error") && !said.contains('('), "{said}");

    std::fs::remove_file(&setup.root).unwrap();
    wait(cx, 9_000);
    assert!(setup.recovery().find(&setup.place()).is_none());
    wait(cx, 1_000);
    assert_eq!(
        setup.recovery().find(&setup.place()).unwrap().text,
        source_of(cx, &view)
    );
}

#[gpui_kit::test]
fn save_as_clears_the_old_places_copy(cx: &mut TestAppContext) {
    let setup = Setup::new(ONE);
    let (handle, view) = open_view(cx, &setup);
    add_slide(cx, &view);
    wait(cx, 1_000);
    assert!(setup.recovery().find(&setup.place()).is_some());
    let elsewhere = tempfile::tempdir().unwrap();
    let out = elsewhere.path().join("copy");

    cx.simulate_keystrokes(handle, "ctrl-shift-s");
    click(cx, handle, SAVE_AS_FOLDER_ID);
    assert!(cx.did_prompt_for_new_path());
    let chosen = out.clone();
    cx.simulate_new_path_selection(move |_| Some(chosen));
    cx.run_until_parked();

    assert_eq!(
        std::fs::read_to_string(out.join("slides.md")).unwrap(),
        source_of(cx, &view)
    );
    assert!(setup.recovery().find(&setup.place()).is_none());
    assert!(setup.recovery().find(&TalkPlace::Folder(out)).is_none());
    assert!(kept_folders(&setup.root).is_empty());
}

#[gpui_kit::test]
fn a_packed_talks_copy_is_found_again(cx: &mut TestAppContext) {
    let setup = Setup::new(ONE);
    let file = setup.folder.parent().unwrap().join("talk.sptx");
    slides_engine::pack_folder(&setup.folder, &file, "test").unwrap();
    let open = |cx: &mut TestAppContext| {
        let file = file.clone();
        open_view_on(cx, &setup, move |window, cx| {
            WorkspaceView::open(file, window, cx)
        })
    };
    let (handle, view) = open(cx);
    add_slide(cx, &view);
    wait(cx, 1_000);
    let edited = source_of(cx, &view);
    drop(view);
    crash(cx, handle);

    let place = TalkPlace::Packed(file.clone());
    let found = setup.recovery().find(&place).expect("kept");
    assert!(
        found.base_matches,
        "the base is what the pack's slides read as"
    );
    let (handle, view) = open(cx);
    assert_eq!(bar(cx, handle).as_deref(), Some(RECOVERY_OFFER));
    click(cx, handle, RECOVER_ID);
    assert_eq!(source_of(cx, &view), edited);
}

#[gpui_kit::test]
fn two_windows_on_the_same_talk_keep_their_own(cx: &mut TestAppContext) {
    let setup = Setup::new(ONE);
    let (first, first_view) = open_view(cx, &setup);
    let (_second, second_view) = open_view(cx, &setup);
    add_slide(cx, &first_view);
    add_slide(cx, &second_view);
    add_slide(cx, &second_view);
    wait(cx, 1_000);
    assert_eq!(kept_folders(&setup.root).len(), 2);

    cx.simulate_keystrokes(first, "ctrl-s");
    cx.run_until_parked();
    wait(cx, 1_000);

    assert_eq!(
        setup.recovery().find(&setup.place()).unwrap().text,
        source_of(cx, &second_view)
    );
}

/// What every working copy under `root` holds.
fn kept_texts(root: &Path) -> Vec<String> {
    kept_folders(root)
        .into_iter()
        .filter_map(|slot| std::fs::read_to_string(slot.join("slides.md")).ok())
        .collect()
}

#[gpui_kit::test]
fn a_live_windows_copy_is_never_offered_or_let_go(cx: &mut TestAppContext) {
    let setup = Setup::new(ONE);
    let (_first, first_view) = open_view(cx, &setup);
    add_slide(cx, &first_view);
    wait(cx, 1_000);
    let live = source_of(cx, &first_view);

    // Another window on the talk is offered nothing.
    let (second, _) = open_view(cx, &setup);
    assert_eq!(bar(cx, second), None);
    crash(cx, second);

    // A copy from an earlier crash is offered instead, and answering it leaves the live one.
    keep(&setup, "# Earlier\n");
    let (third, _) = open_view(cx, &setup);
    assert_eq!(bar(cx, third).as_deref(), Some(RECOVERY_OFFER));
    click(cx, third, DISCARD_RECOVERY_ID);
    crash(cx, third);
    assert_eq!(kept_texts(&setup.root), std::slice::from_ref(&live));

    keep(&setup, "# Earlier\n");
    let (fourth, fourth_view) = open_view(cx, &setup);
    click(cx, fourth, RECOVER_ID);
    assert_eq!(source_of(cx, &fourth_view), "# Earlier\n");
    let mut texts = kept_texts(&setup.root);
    texts.sort();
    assert_eq!(texts, ["# Earlier\n".to_string(), live]);
}

#[gpui_kit::test(iterations = 30)]
fn a_save_just_after_bringing_them_back_leaves_nothing(cx: &mut TestAppContext) {
    let (setup, _) = left_unsaved(cx);
    let (handle, _view) = open_view(cx, &setup);
    // Both asked for in one go, before either has run.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(RECOVER_ID, cx);
    })
    .unwrap();
    cx.simulate_keystrokes(handle, "ctrl-s");
    cx.run_until_parked();
    wait(cx, 2_000);
    assert!(
        kept_folders(&setup.root).is_empty(),
        "{:?}",
        kept_texts(&setup.root)
    );
}

fn bounds_of(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    id: &'static str,
) -> Bounds<gpui_kit::Pixels> {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find(id).bounds()
    })
    .unwrap()
}

#[gpui_kit::test]
fn the_recovery_bar_sits_under_the_ribbon_and_leaves_the_slide_its_size(cx: &mut TestAppContext) {
    let (setup, _) = left_unsaved(cx);
    let (handle, _view) = open_view(cx, &setup);
    assert!(bar(cx, handle).is_some());
    let shown = bounds_of(cx, handle, RECOVERY_ID);
    let ribbon = bounds_of(cx, handle, RIBBON_ID);
    let canvas = bounds_of(cx, handle, CANVAS_ID);
    assert!(shown.top() >= ribbon.bottom(), "{shown:?} {ribbon:?}");
    assert!(shown.bottom() <= canvas.bottom(), "{shown:?} {canvas:?}");

    click(cx, handle, DISCARD_RECOVERY_ID);
    assert_eq!(bar(cx, handle), None);
    assert_eq!(
        bounds_of(cx, handle, CANVAS_ID),
        canvas,
        "the slide keeps its size"
    );
}

// Files added to the talk.

fn red_png() -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::from_pixel(64, 64, image::Rgba([255, 0, 0, 255]))
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

/// Whether the current slide draws with nothing to say and something red on it.
fn draws_red(doc: &mut Document) -> bool {
    let index = doc.current();
    let notices = doc.notices();
    assert!(notices.is_empty(), "{notices:?}");
    let thumbnail = doc.thumbnail(index, 480).expect("the slide draws");
    thumbnail.data().chunks(4).any(|pixel| pixel[0] > 200 && pixel[1] < 40 && pixel[2] < 40)
}

#[test]
fn letting_go_removes_kept_files() {
    let setup = Setup::new(ONE);
    let base = std::fs::read_to_string(setup.file()).unwrap();
    let files = vec![("assets/photo.png".to_string(), std::sync::Arc::<[u8]>::from(red_png()))];
    setup
        .recovery()
        .keep_with_files(&setup.place(), &hash_text(&base), EDITED, &files)
        .unwrap();
    let kept = setup.kept_folder();
    assert_eq!(std::fs::read(kept.join("files/assets/photo.png")).unwrap(), red_png());
    let found = setup.recovery().find(&setup.place()).unwrap();
    assert_eq!(found.files, files);

    setup.recovery().clear(&setup.place());
    assert!(!kept.exists());
    assert!(setup.recovery().find(&setup.place()).is_none());
    assert_eq!(std::fs::read_dir(&setup.root).unwrap().count(), 0);

    // And a copy let go from another window takes its files too.
    setup
        .recovery()
        .keep_with_files(&setup.place(), &hash_text(&base), EDITED, &files)
        .unwrap();
    let found = setup.recovery().find(&setup.place()).unwrap();
    Recovery::new(setup.root.clone()).discard(&found);
    assert_eq!(std::fs::read_dir(&setup.root).unwrap().count(), 0);
}

#[test]
fn only_plain_kept_files_are_brought_back() {
    let setup = Setup::new(ONE);
    let base = std::fs::read_to_string(setup.file()).unwrap();
    let odd = vec![("../outside.png".to_string(), std::sync::Arc::<[u8]>::from(red_png()))];
    assert!(setup.recovery().keep_with_files(&setup.place(), &hash_text(&base), EDITED, &odd).is_err());
    let plain = vec![("assets/photo.png".to_string(), std::sync::Arc::<[u8]>::from(red_png()))];
    setup
        .recovery()
        .keep_with_files(&setup.place(), &hash_text(&base), EDITED, &plain)
        .unwrap();
    let kept = setup.kept_folder();
    std::fs::write(kept.join("files/assets/Not Plain.png"), "x").unwrap();
    std::fs::write(kept.join("files/notes.txt"), "x").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(setup.file(), kept.join("files/assets/link.png")).unwrap();
    assert_eq!(setup.recovery().find(&setup.place()).unwrap().files, plain);
}

#[gpui_kit::test]
fn a_crash_keeps_an_added_file(cx: &mut TestAppContext) {
    let setup = Setup::new(ONE);
    let (handle, view) = open_view(cx, &setup);
    let talk = document(cx, &view);
    cx.update(|cx| {
        talk.update(cx, |document, cx| {
            let path = document
                .add_file(stagepack_studio::added_files::FileKind::Picture, "photo", red_png())
                .unwrap();
            let end = document.source().len();
            let change = slides_format::Change::single(slides_format::SourceEdit {
                range: end..end,
                text: format!("\n![]({path})\n"),
            });
            document.edit("Insert picture", change, 0).unwrap();
            cx.notify();
        });
    });
    wait(cx, 1_000);
    let edited = source_of(cx, &view);
    drop(talk);
    drop(view);
    crash(cx, handle);
    assert!(!setup.folder.join("assets").exists(), "nothing on disk until Save");

    let (handle, view) = open_view(cx, &setup);
    click(cx, handle, RECOVER_ID);

    assert_eq!(source_of(cx, &view), edited);
    let talk = document(cx, &view);
    cx.update(|cx| {
        talk.update(cx, |document, _| {
            assert!(document.has_file("assets/photo.png"));
            assert!(draws_red(document));
            assert_eq!(document.added_files().len(), 1, "a save writes it");
        })
    });
}

#[test]
fn a_kept_file_is_judged_by_its_bytes() {
    let setup = Setup::new(ONE);
    let base = hash_text(&std::fs::read_to_string(setup.file()).unwrap());
    let first = vec![("assets/a.png".to_string(), std::sync::Arc::<[u8]>::from(&b"first"[..]))];
    setup.recovery().keep_with_files(&setup.place(), &base, EDITED, &first).unwrap();
    let other = vec![("assets/a.png".to_string(), std::sync::Arc::<[u8]>::from(&b"other"[..]))];
    setup.recovery().keep_with_files(&setup.place(), &base, EDITED, &other).unwrap();
    assert_eq!(std::fs::read(setup.kept_folder().join("files/assets/a.png")).unwrap(), b"other");
}

/// Adds the picture `suggested` to the talk in `view`, red, without using it, and returns its path.
fn add_red(cx: &mut TestAppContext, view: &Entity<WorkspaceView>, suggested: &str) -> String {
    let talk = document(cx, view);
    cx.update(|cx| {
        talk.update(cx, |document, cx| {
            let path = document
                .add_file(stagepack_studio::added_files::FileKind::Picture, suggested, red_png())
                .unwrap();
            cx.notify();
            path
        })
    })
}

#[gpui_kit::test]
fn an_added_file_is_kept_at_once(cx: &mut TestAppContext) {
    let setup = Setup::new(ONE);
    let (_handle, view) = open_view(cx, &setup);
    let path = add_red(cx, &view, "photo");
    // No time passes: it's written as it's added, though nothing uses it yet.
    cx.run_until_parked();
    assert_eq!(std::fs::read(setup.kept_folder().join("files").join(&path)).unwrap(), red_png());
    assert!(setup.recovery().find(&setup.place()).is_none(), "no changes to offer back");
}

#[gpui_kit::test]
fn a_crash_keeps_an_added_file_the_text_named_already(cx: &mut TestAppContext) {
    let named = "---\ntitle: T\n---\n\n# One\n\n![](assets/late.png)\n";
    let setup = Setup::new(named);
    let (handle, view) = open_view(cx, &setup);
    assert_eq!(add_red(cx, &view, "late"), "assets/late.png");
    cx.run_until_parked();
    let talk = document(cx, &view);
    let unsaved = cx.read(|cx| talk.read(cx).has_unsaved_changes());
    assert!(unsaved, "a save would write it");
    let shown = title(cx, handle);
    assert!(shown.starts_with('•'), "{shown}");
    drop(talk);
    wait(cx, 1_000);
    drop(view);
    crash(cx, handle);

    let (handle, view) = open_view(cx, &setup);
    assert_eq!(bar(cx, handle).as_deref(), Some(RECOVERY_OFFER));
    click(cx, handle, RECOVER_ID);
    assert_eq!(source_of(cx, &view), named);
    let talk = document(cx, &view);
    cx.update(|cx| {
        talk.update(cx, |document, _| {
            assert!(draws_red(document));
            assert!(document.has_unsaved_changes());
        })
    });
}

// New presentations that were never saved.

const UNTITLED_TEXT: &str = "---\nlayout: title\n---\n\n# Plans **for** the spring\nand the summer\n\n---\n\n# Two\n";

/// A recovery folder of its own, in a scratch folder.
fn scratch_root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("recovery");
    (dir, root)
}

/// The folder `id`'s copy is kept in under `root`: the only one whose about file says so.
fn untitled_slot(root: &Path, id: UntitledId) -> PathBuf {
    let hex = id.to_string();
    let mut found: Vec<PathBuf> = kept_folders(root)
        .into_iter()
        .filter(|slot| {
            std::fs::read(slot.join("about.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .is_some_and(|about| about["untitled"]["id"].as_str() == Some(hex.as_str()))
        })
        .collect();
    assert_eq!(found.len(), 1, "{found:?}");
    found.remove(0)
}

/// Says the copy in `slot` was kept `secs` seconds after 1970.
fn set_kept_at(slot: &Path, secs: u64) {
    let about = slot.join("about.json");
    let mut json: serde_json::Value = serde_json::from_slice(&std::fs::read(&about).unwrap()).unwrap();
    json["kept_at"] = secs.into();
    std::fs::write(&about, json.to_string()).unwrap();
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs()
}

#[test]
fn an_untitled_copy_is_kept_listed_taken_and_discarded() {
    let (_dir, root) = scratch_root();
    let recovery = Recovery::new(root.clone());
    let id = UntitledId::random();
    let files = vec![("assets/photo.png".to_string(), std::sync::Arc::<[u8]>::from(red_png()))];
    let before = SystemTime::now() - Duration::from_secs(2);
    recovery.keep_untitled(id, "Untitled presentation 2", UNTITLED_TEXT, &files, &[]).unwrap();
    // A talk with a place isn't listed among them.
    let setup = Setup::new(ONE);
    keep(&setup, EDITED);

    let about: serde_json::Value =
        serde_json::from_slice(&std::fs::read(untitled_slot(&root, id).join("about.json")).unwrap()).unwrap();
    assert_eq!(about["untitled"]["id"].as_str(), Some(id.to_string().as_str()));
    assert_eq!(about["untitled"]["name"].as_str(), Some("Untitled presentation 2"));

    let (listed, skipped) = recovery.untitled();
    assert_eq!(skipped, 0);
    assert_eq!(listed.len(), 1, "{listed:?}");
    let copy = &listed[0];
    assert_eq!(copy.id, id);
    assert_eq!(copy.name, "Untitled presentation 2");
    assert_eq!(copy.slides, 2);
    assert_eq!(copy.first_words, "Plans for the spring", "as a person reads it, its first line");
    assert!(copy.when >= before && copy.when <= SystemTime::now() + Duration::from_secs(2));

    let found = recovery.take_untitled(id).expect("its copy");
    assert_eq!(found.text, UNTITLED_TEXT);
    assert_eq!(found.files, files);
    assert!(found.builds.is_empty());
    assert!(!found.base_matches, "no file holds anything of it");
    // Taking it leaves it there until the window that brings it back keeps its own.
    assert_eq!(recovery.untitled().0.len(), 1);

    recovery.discard_untitled(id);
    assert_eq!(recovery.untitled(), (Vec::new(), 0));
    assert_eq!(recovery.take_untitled(id).unwrap_err(), NotTaken::Gone);
    assert_eq!(recovery.take_untitled(UntitledId::random()).unwrap_err(), NotTaken::Gone);
    assert_eq!(NotTaken::Gone.to_string(), "This presentation isn't kept any more.");
    assert!(setup.recovery().find(&setup.place()).is_some(), "the other talk's copy stays");

    // Kept with no name of its own, as any talk's is, it's called what a new one is.
    let other = UntitledId::random();
    recovery.keep(&TalkPlace::Untitled(other), &hash_text(""), "# Plain\n").unwrap();
    let (listed, _) = recovery.untitled();
    assert_eq!(listed[0].name, "Untitled presentation");
}

#[test]
fn the_first_words_are_short_and_plain() {
    let (_dir, root) = scratch_root();
    let recovery = Recovery::new(root);
    let cases = [
        ("", ""),
        ("---\nlayout: title\n---\n", ""),
        ("# \n\n- *First* point\n- Second\n", "First point"),
        ("```\ncode\n```\n\nAfter   the    code\n", "After the code"),
        (&format!("# {}\n", "word ".repeat(30)), "word word word word word word word word word word word word"),
        ("# Ünïcödé ✓ wörds\n", "Ünïcödé ✓ wörds"),
    ];
    for (text, words) in cases {
        let id = UntitledId::random();
        recovery.keep_untitled(id, "Untitled presentation", text, &[], &[]).unwrap();
        let (listed, _) = recovery.untitled();
        let copy = listed.iter().find(|copy| copy.id == id).unwrap();
        assert_eq!(copy.first_words, words, "{text:?}");
        assert!(copy.first_words.chars().count() <= 60);
        recovery.discard_untitled(id);
    }
}

#[test]
fn a_recovery_folder_in_any_state_is_listed_or_skipped() {
    // No folder at all.
    let (_dir, root) = scratch_root();
    assert_eq!(Recovery::new(root.clone()).untitled(), (Vec::new(), 0));
    std::fs::create_dir_all(&root).unwrap();
    assert_eq!(Recovery::new(root.clone()).untitled(), (Vec::new(), 0));

    // Fifty, all listed, newest first.
    let (_dir, root) = scratch_root();
    let recovery = Recovery::new(root.clone());
    let ids: Vec<UntitledId> = (0..50).map(|_| UntitledId::random()).collect();
    let start = now_secs() - 1_000;
    for (at, id) in ids.iter().enumerate() {
        recovery.keep_untitled(*id, "Untitled presentation", &format!("# Talk {at}\n"), &[], &[]).unwrap();
        set_kept_at(&untitled_slot(&root, *id), start + at as u64);
    }
    let (listed, skipped) = recovery.untitled();
    assert_eq!(skipped, 0);
    let newest_first: Vec<UntitledId> = ids.iter().rev().copied().collect();
    assert_eq!(listed.iter().map(|copy| copy.id).collect::<Vec<_>>(), newest_first);
    assert_eq!(listed[0].first_words, "Talk 49");

    // Each odd state on its own: whether it's listed, and how many are skipped.
    // What's done to a copy, whether it's listed after, and how many are skipped.
    type Spoil = Box<dyn Fn(&Path)>;
    let odd: Vec<(&str, bool, usize, Spoil)> = vec![
        ("text that doesn't read as slides", true, 0, Box::new(|slot: &Path| {
            std::fs::write(slot.join("slides.md"), "<Box x=\"\n---\n---\n<<< }}} \u{0}").unwrap()
        })),
        ("text that isn't UTF-8", false, 1, Box::new(|slot: &Path| {
            std::fs::write(slot.join("slides.md"), [0xff, 0xfe, b'#']).unwrap()
        })),
        ("no text", false, 1, Box::new(|slot: &Path| std::fs::remove_file(slot.join("slides.md")).unwrap())),
        ("no about file", false, 1, Box::new(|slot: &Path| std::fs::remove_file(slot.join("about.json")).unwrap())),
        ("a truncated about file", false, 1, Box::new(|slot: &Path| {
            let about = std::fs::read(slot.join("about.json")).unwrap();
            std::fs::write(slot.join("about.json"), &about[..about.len() / 2]).unwrap()
        })),
        ("an about file from a newer version", true, 0, Box::new(|slot: &Path| {
            let about = slot.join("about.json");
            let mut json: serde_json::Value = serde_json::from_slice(&std::fs::read(&about).unwrap()).unwrap();
            json["colour"] = "teal".into();
            json["untitled"]["pinned"] = true.into();
            std::fs::write(&about, json.to_string()).unwrap()
        })),
        ("an id that points outside", false, 1, Box::new(|slot: &Path| {
            let about = slot.join("about.json");
            let mut json: serde_json::Value = serde_json::from_slice(&std::fs::read(&about).unwrap()).unwrap();
            json["untitled"]["id"] = "../../../../etc/passwd".into();
            std::fs::write(&about, json.to_string()).unwrap()
        })),
        ("an id that isn't its folder's", false, 1, Box::new(|slot: &Path| {
            let about = slot.join("about.json");
            let mut json: serde_json::Value = serde_json::from_slice(&std::fs::read(&about).unwrap()).unwrap();
            let stranger = UntitledId::random();
            json["untitled"]["id"] = stranger.to_string().into();
            json["talk"] = format!("untitled/{stranger}").into();
            std::fs::write(&about, json.to_string()).unwrap()
        })),
        ("a talk path that isn't its own", false, 1, Box::new(|slot: &Path| {
            let about = slot.join("about.json");
            let mut json: serde_json::Value = serde_json::from_slice(&std::fs::read(&about).unwrap()).unwrap();
            json["talk"] = "/etc".into();
            std::fs::write(&about, json.to_string()).unwrap()
        })),
        ("a name no window could show", true, 0, Box::new(|slot: &Path| {
            let about = slot.join("about.json");
            let mut json: serde_json::Value = serde_json::from_slice(&std::fs::read(&about).unwrap()).unwrap();
            json["untitled"]["name"] = format!("Odd\u{7}\n\u{202e}\u{200b}\u{feff}\u{2066}{}", "x".repeat(10_000)).into();
            std::fs::write(&about, json.to_string()).unwrap()
        })),
        ("a time far ahead of the clock", true, 0, Box::new(|slot: &Path| set_kept_at(slot, now_secs() + 10 * 365 * 24 * 60 * 60))),
        ("text too large to bring back", false, 1, Box::new(|slot: &Path| {
            // Sparse, so it takes no room.
            let file = std::fs::OpenOptions::new().write(true).open(slot.join("slides.md")).unwrap();
            file.set_len(64 * 1024 * 1024 + 1).unwrap();
        })),
        ("text larger than is read to list it", true, 0, Box::new(|slot: &Path| {
            let mut text = "# Big\n\n".to_string();
            text.push_str(&"Words on a line.\n".repeat(200_000));
            std::fs::write(slot.join("slides.md"), text).unwrap()
        })),
    ];
    for (what, listed_it, skips, spoil) in odd {
        let (_dir, root) = scratch_root();
        let recovery = Recovery::new(root.clone());
        let id = UntitledId::random();
        recovery.keep_untitled(id, "Untitled presentation", "# Hello\n", &[], &[]).unwrap();
        spoil(&untitled_slot(&root, id));
        let (listed, skipped) = recovery.untitled();
        assert_eq!(listed.iter().any(|copy| copy.id == id), listed_it, "{what}: {listed:?}");
        assert_eq!(skipped, skips, "{what}");
        assert_eq!(recovery.take_untitled(id).is_ok(), listed_it, "{what}");
        if listed_it {
            let copy = listed.iter().find(|copy| copy.id == id).unwrap();
            assert!(copy.name.chars().count() <= 80 && !copy.name.chars().any(char::is_control), "{what}");
            assert!(!copy.name.contains(['\u{202e}', '\u{200b}', '\u{feff}', '\u{2066}']), "{what}: {:?}", copy.name);
            assert!(copy.first_words.chars().count() <= 60, "{what}");
            assert!(copy.when <= SystemTime::now() + Duration::from_secs(60), "{what}: no time ahead of the clock");
            let whole = std::fs::read_to_string(untitled_slot(&root, id).join("slides.md")).unwrap();
            assert_eq!(recovery.take_untitled(id).unwrap().text, whole, "{what}: it comes back whole");
            assert_eq!(copy.slides_at_least, whole.len() > 1024 * 1024, "{what}");
        }
    }

    #[cfg(unix)]
    links_and_locked_folders();
}

#[cfg(unix)]
fn links_and_locked_folders() {
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    // A picture folder that's a link to somewhere else: the copy is listed and brought back,
    // with nothing read through the link.
    let (_dir, root) = scratch_root();
    let recovery = Recovery::new(root.clone());
    let id = UntitledId::random();
    let files = vec![("assets/photo.png".to_string(), std::sync::Arc::<[u8]>::from(red_png()))];
    recovery.keep_untitled(id, "Untitled presentation", "# Hello\n", &files, &[]).unwrap();
    let slot = untitled_slot(&root, id);
    std::fs::remove_dir_all(slot.join("files/assets")).unwrap();
    symlink("/etc", slot.join("files/assets")).unwrap();
    let (listed, skipped) = recovery.untitled();
    assert_eq!((listed.len(), skipped), (1, 0));
    assert!(recovery.take_untitled(id).unwrap().files.is_empty());
    // The files folder itself as a link.
    std::fs::remove_file(slot.join("files/assets")).unwrap();
    std::fs::remove_dir_all(slot.join("files")).unwrap();
    symlink("/etc", slot.join("files")).unwrap();
    assert!(recovery.take_untitled(id).unwrap().files.is_empty());
    // A picture that's a link.
    std::fs::remove_file(slot.join("files")).unwrap();
    std::fs::create_dir_all(slot.join("files/assets")).unwrap();
    symlink("/etc/hostname", slot.join("files/assets/photo.png")).unwrap();
    assert!(recovery.take_untitled(id).unwrap().files.is_empty());

    // Text, an about file, a slot or a talk's folder that are links are never read through.
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("slides.md"), "# Not kept here\n").unwrap();
    for linked in ["slides.md", "about.json", "slot", "key"] {
        let (_dir, root) = scratch_root();
        let recovery = Recovery::new(root.clone());
        let id = UntitledId::random();
        recovery.keep_untitled(id, "Untitled presentation", "# Hello\n", &[], &[]).unwrap();
        let slot = untitled_slot(&root, id);
        let key = slot.parent().unwrap().to_path_buf();
        match linked {
            "slides.md" | "about.json" => {
                if linked == "about.json" {
                    std::fs::copy(slot.join(linked), outside.path().join(linked)).unwrap();
                }
                std::fs::remove_file(slot.join(linked)).unwrap();
                symlink(outside.path().join(linked), slot.join(linked)).unwrap();
            }
            "slot" => {
                let moved = outside.path().join("moved-slot");
                std::fs::rename(&slot, &moved).unwrap();
                symlink(&moved, &slot).unwrap();
            }
            _ => {
                let moved = outside.path().join("moved-key");
                std::fs::rename(&key, &moved).unwrap();
                symlink(&moved, &key).unwrap();
            }
        }
        let (listed, _) = recovery.untitled();
        assert!(listed.is_empty(), "{linked}: {listed:?}");
        assert!(recovery.take_untitled(id).is_err(), "{linked}");
        recovery.discard_untitled(id);
        recovery.sweep(Duration::ZERO);
        assert!(outside.path().join("slides.md").is_file(), "{linked}: nothing outside is removed");
        match linked {
            "slot" => assert!(outside.path().join("moved-slot/slides.md").is_file(), "the slot linked to stays"),
            "key" => assert!(outside.path().join("moved-key").read_dir().unwrap().count() == 1, "the folder linked to stays"),
            "about.json" => assert!(outside.path().join("about.json").is_file()),
            _ => {}
        }
    }

    // Text that's a pipe, a socket or a folder is never read, and nothing waits on it.
    for odd in ["pipe", "socket", "folder"] {
        let (_dir, root) = scratch_root();
        let recovery = Recovery::new(root.clone());
        let id = UntitledId::random();
        let files = vec![("assets/photo.png".to_string(), std::sync::Arc::<[u8]>::from(red_png()))];
        recovery.keep_untitled(id, "Untitled presentation", "# Hello\n", &files, &[]).unwrap();
        let slot = untitled_slot(&root, id);
        let text = slot.join("slides.md");
        let picture = slot.join("files/assets/photo.png");
        std::fs::remove_file(&text).unwrap();
        std::fs::remove_file(&picture).unwrap();
        let _socket = match odd {
            "pipe" => {
                for at in [&text, &picture] {
                    let name = std::ffi::CString::new(at.as_os_str().as_encoded_bytes()).unwrap();
                    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
                }
                None
            }
            "socket" => Some([std::os::unix::net::UnixListener::bind(&text).unwrap(), std::os::unix::net::UnixListener::bind(&picture).unwrap()]),
            _ => {
                std::fs::create_dir(&text).unwrap();
                std::fs::create_dir(&picture).unwrap();
                None
            }
        };
        let (listed, skipped) = recovery.untitled();
        assert_eq!((listed.len(), skipped), (0, 1), "{odd}");
        assert_eq!(recovery.take_untitled(id).unwrap_err(), NotTaken::Unreadable, "{odd}");
        // With its text back, the picture that isn't a file is left out.
        std::fs::remove_file(&text).or_else(|_| std::fs::remove_dir(&text)).unwrap();
        std::fs::write(&text, "# Hello\n").unwrap();
        assert!(recovery.take_untitled(id).unwrap().files.is_empty(), "{odd}");
    }

    // A folder that can't be read is skipped, and the others listed.
    let (_dir, root) = scratch_root();
    let recovery = Recovery::new(root.clone());
    let (locked, open) = (UntitledId::random(), UntitledId::random());
    recovery.keep_untitled(locked, "Untitled presentation", "# Locked\n", &[], &[]).unwrap();
    recovery.keep_untitled(open, "Untitled presentation", "# Open\n", &[], &[]).unwrap();
    let slot = untitled_slot(&root, locked);
    std::fs::set_permissions(&slot, std::fs::Permissions::from_mode(0o000)).unwrap();
    let still_readable = std::fs::read_dir(&slot).is_ok();
    let (listed, skipped) = recovery.untitled();
    std::fs::set_permissions(&slot, std::fs::Permissions::from_mode(0o700)).unwrap();
    if !still_readable {
        assert_eq!(listed.iter().map(|copy| copy.id).collect::<Vec<_>>(), vec![open]);
        assert_eq!(skipped, 1);
    }
    let key = slot.parent().unwrap().to_path_buf();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o000)).unwrap();
    let still_readable = std::fs::read_dir(&key).is_ok();
    let (listed, skipped) = recovery.untitled();
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o700)).unwrap();
    if !still_readable {
        assert_eq!(listed.iter().map(|copy| copy.id).collect::<Vec<_>>(), vec![open]);
        assert_eq!(skipped, 1);
    }
}

#[test]
fn a_copy_held_by_a_live_window_is_not_listed() {
    let (_dir, root) = scratch_root();
    let app = Recovery::new(root.clone());
    let window = app.for_another_window();
    let id = UntitledId::random();
    window.keep_untitled(id, "Untitled presentation", "# Mine\n", &[], &[]).unwrap();

    assert_eq!(app.untitled(), (Vec::new(), 0), "held, so not left over from a crash");
    assert_eq!(app.take_untitled(id).unwrap_err(), NotTaken::Held);
    assert_eq!(window.untitled(), (Vec::new(), 0), "not even by the window holding it");
    let another = app.for_another_window();
    assert_eq!(another.untitled(), (Vec::new(), 0));
    assert_eq!(another.take_untitled(id).unwrap_err(), NotTaken::Held);
    app.discard_untitled(id);
    another.discard_untitled(id);
    assert!(window.find(&TalkPlace::Untitled(id)).is_some(), "nobody else lets it go");

    // Once the window has gone, as it has after a crash, it's listed.
    drop(window);
    let (listed, skipped) = app.untitled();
    assert_eq!((listed.len(), skipped), (1, 0));
    assert_eq!(another.take_untitled(id).unwrap().text, "# Mine\n");
}

#[test]
fn an_old_untitled_copy_is_swept() {
    let (_dir, root) = scratch_root();
    let recovery = Recovery::new(root.clone());
    let (old, recent) = (UntitledId::random(), UntitledId::random());
    recovery.keep_untitled(old, "Untitled presentation", "# Old\n", &[], &[]).unwrap();
    recovery.keep_untitled(recent, "Untitled presentation", "# Recent\n", &[], &[]).unwrap();
    let month_ago = SystemTime::now() - KEEP_FOR - Duration::from_secs(60 * 60);
    let old_slot = untitled_slot(&root, old);
    set_kept_at(&old_slot, month_ago.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs());

    recovery.sweep(KEEP_FOR);

    assert!(!old_slot.parent().unwrap().exists(), "its folder goes too");
    let (listed, skipped) = recovery.untitled();
    assert_eq!(listed.iter().map(|copy| copy.id).collect::<Vec<_>>(), vec![recent]);
    assert_eq!(skipped, 0);
}

#[test]
fn older_about_files_still_read() {
    // As plan D4 wrote them, for a talk in a folder.
    let setup = Setup::new(ONE);
    keep(&setup, EDITED);
    let json = serde_json::json!({
        "talk": setup.folder.canonicalize().unwrap(),
        "base_sha256": hash_text(ONE),
        "kept_at": now_secs(),
    });
    std::fs::write(setup.kept_folder().join("about.json"), json.to_string()).unwrap();
    let found = setup.recovery().find(&setup.place()).expect("still found");
    assert_eq!(found.text, EDITED);
    assert!(found.base_matches);
    assert_eq!(setup.recovery().untitled(), (Vec::new(), 0), "not a new presentation");
    // And what's written now has nothing about being untitled.
    keep(&setup, EDITED);
    let about: serde_json::Value =
        serde_json::from_slice(&std::fs::read(setup.kept_folder().join("about.json")).unwrap()).unwrap();
    assert!(about.get("untitled").is_none(), "{about}");
}

#[gpui_kit::test]
fn a_crash_before_the_first_save_brings_it_back(cx: &mut TestAppContext) {
    let setup = Setup::new(ONE);
    let id = UntitledId::random();
    let template = stagepack_studio::templates::template("light").unwrap();
    let talk = Document::new_untitled(template, id, Fonts::bundled_only()).unwrap();
    let (handle, view) = open_view_on(cx, &setup, move |window, cx| WorkspaceView::open_with(talk, window, cx));
    let picture = add_red(cx, &view, "red");
    let talk = document(cx, &view);
    cx.update(|cx| {
        talk.update(cx, |document, cx| {
            document.insert_tag("Insert picture", "Picture", &[("src", &picture)], None).unwrap();
            assert!(document.begin_text_in_region(0, "title"));
            document.apply_text(|edit| edit.insert("Plans for spring")).unwrap();
            document.end_typing_run();
            cx.notify();
        })
    });
    cx.run_until_parked();
    let edited = source_of(cx, &view);
    assert!(edited.contains("Plans for spring") && edited.contains(&picture), "{edited}");
    wait(cx, 1_000);
    assert!(setup.recovery().find(&TalkPlace::Untitled(id)).is_some(), "kept");
    assert_eq!(Recovery::new(setup.root.clone()).untitled(), (Vec::new(), 0), "held while the window's open");
    drop(talk);
    drop(view);
    crash(cx, handle);

    let recovery = Recovery::new(setup.root.clone());
    let (listed, skipped) = recovery.untitled();
    assert_eq!(skipped, 0);
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0].id, id);
    assert_eq!(listed[0].name, "Untitled presentation");
    assert_eq!(listed[0].slides, 1);
    assert_eq!(listed[0].first_words, "Plans for spring");

    let found = recovery.take_untitled(id).expect("its copy");
    let mut back = Document::untitled_with(id, found.text, found.files, found.builds, Fonts::bundled_only()).unwrap();
    assert_eq!(back.source().as_bytes(), edited.as_bytes());
    assert!(back.is_untitled());
    assert!(back.has_unsaved_changes());
    assert!(back.has_file(&picture));
    assert!(draws_red(&mut back));

    let out = tempfile::tempdir().unwrap();
    let packed = out.path().join("Back.sptx");
    let saved = back.save_as_job(SaveTarget::Packed(packed.clone())).run().unwrap();
    back.saved(saved);
    assert!(!back.is_untitled());
    let pack = slides_engine::open_packed(&packed).unwrap();
    assert_eq!(pack.slides().unwrap().as_bytes(), edited.as_bytes());
    assert_eq!(slides_engine::TalkFiles::read(&pack, &picture).as_deref(), Some(red_png().as_slice()));
}

/// Holds `slot` as a window in another running copy of the app does: its `held` file locked,
/// through a file opened apart from any this app has, which conflicts with them as another
/// app's would. Closing it is that app stopping.
fn hold_from_another_app(slot: &Path) -> std::fs::File {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(slot.join("held"))
        .unwrap();
    file.lock().unwrap();
    file
}

#[test]
fn a_copy_another_running_app_holds_is_left_alone() {
    let (_dir, root) = scratch_root();
    let app = Recovery::new(root.clone());
    let window = app.for_another_window();
    assert!(!root.exists(), "opening a window writes nothing");

    // A window holds its slot with a lock others see.
    let id = UntitledId::random();
    window.keep_untitled(id, "Untitled presentation", "# Theirs\n", &[], &[]).unwrap();
    let slot = untitled_slot(&root, id);
    let looking = std::fs::File::open(slot.join("held")).unwrap();
    assert!(matches!(looking.try_lock_shared(), Err(std::fs::TryLockError::WouldBlock)), "locked while it's open");
    drop(looking);
    // Cleared and kept again, it's held again.
    window.clear(&TalkPlace::Untitled(id));
    window.keep_untitled(id, "Untitled presentation", "# Theirs\n", &[], &[]).unwrap();
    let looking = std::fs::File::open(untitled_slot(&root, id).join("held")).unwrap();
    assert!(matches!(looking.try_lock_shared(), Err(std::fs::TryLockError::WouldBlock)));
    drop(looking);
    drop(window);
    let looking = std::fs::File::open(slot.join("held")).unwrap();
    looking.try_lock_shared().expect("let go with the window, as a crash lets it go");
    drop(looking);

    // Held by another app: not listed, taken, discarded or swept.
    let other_app = hold_from_another_app(&slot);
    assert_eq!(app.untitled(), (Vec::new(), 0));
    assert_eq!(app.take_untitled(id).unwrap_err(), NotTaken::Held);
    assert_eq!(app.for_another_window().take_untitled(id).unwrap_err(), NotTaken::Held);
    app.discard_untitled(id);
    app.sweep(Duration::ZERO);
    let found = app.find(&TalkPlace::Untitled(id)).expect("there to be seen");
    app.discard(&found);
    assert!(slot.join("slides.md").is_file(), "all of it left alone");

    // Once that app has stopped, it's listed and can be brought back.
    drop(other_app);
    let (listed, skipped) = app.untitled();
    assert_eq!((listed.len(), skipped), (1, 0));
    assert_eq!(app.take_untitled(id).unwrap().text, "# Theirs\n");

    // The same goes for a talk with a place.
    let setup = Setup::new(ONE);
    keep(&setup, EDITED);
    let other_app = hold_from_another_app(&setup.kept_folder());
    let window = setup.recovery().for_another_window();
    assert!(window.find(&setup.place()).is_none(), "not offered to a window here");
    setup.recovery().sweep(Duration::ZERO);
    assert!(setup.kept_folder().join("slides.md").is_file());
    drop(other_app);
    assert_eq!(window.find(&setup.place()).unwrap().text, EDITED);
}

#[test]
fn a_slot_holding_only_files_is_not_counted_and_is_swept() {
    let (_dir, root) = scratch_root();
    let talk = tempfile::tempdir().unwrap();
    let picture = red_png();
    let (untitled, folder) = (TalkPlace::Untitled(UntitledId::random()), TalkPlace::Folder(talk.path().to_path_buf()));
    // A picture kept as it was added, and no text after it, for a new presentation and for a
    // talk with a place: each in a slot of its own that no window holds.
    for place in [&untitled, &folder] {
        Recovery::new(root.clone()).keep_file(place, "assets/photo.png", &picture).unwrap();
    }
    let left: Vec<PathBuf> = kept_folders(&root);
    assert_eq!(left.len(), 2);
    assert!(left.iter().all(|slot| !slot.join("about.json").exists() && !slot.join("slides.md").exists()));
    // The same, held by a window here and by one in another running copy of the app.
    let app = Recovery::new(root.clone());
    let window = app.for_another_window();
    let held_here = TalkPlace::Untitled(UntitledId::random());
    window.keep_file(&held_here, "assets/photo.png", &picture).unwrap();
    let held_elsewhere = TalkPlace::Untitled(UntitledId::random());
    Recovery::new(root.clone()).keep_file(&held_elsewhere, "assets/photo.png", &picture).unwrap();
    let elsewhere_slot = kept_folders(&root).into_iter().find(|slot| !left.contains(slot) && !slot.join("held").exists()).unwrap();
    let other_app = hold_from_another_app(&elsewhere_slot);
    // A copy whose about file is gone but whose text is there still counts, as it holds text.
    let spoilt = UntitledId::random();
    Recovery::new(root.clone()).keep_untitled(spoilt, "Untitled presentation", "# Text\n", &[], &[]).unwrap();
    let spoilt_slot = untitled_slot(&root, spoilt);
    std::fs::remove_file(spoilt_slot.join("about.json")).unwrap();

    // Nothing was ever kept in those holding only files, so they're not counted as unreadable.
    assert_eq!(app.untitled(), (Vec::new(), 1), "only the copy with text is counted");

    // At launch they go, whatever their age, but those held stay, as does the copy with text.
    app.sweep(KEEP_FOR);
    for slot in &left {
        assert!(!slot.exists(), "{slot:?}");
        assert!(!slot.parent().unwrap().exists(), "its talk's folder goes too");
    }
    assert!(elsewhere_slot.join("files/assets/photo.png").is_file(), "held by another app");
    let window_slots: Vec<PathBuf> =
        kept_folders(&root).into_iter().filter(|slot| *slot != elsewhere_slot && *slot != spoilt_slot).collect();
    assert_eq!(window_slots.len(), 1, "held by a window here");
    assert!(window_slots[0].join("files/assets/photo.png").is_file());
    assert!(spoilt_slot.join("slides.md").is_file(), "its text is kept for its time");
    assert!(talk.path().read_dir().unwrap().next().is_none(), "nothing in the talk's own folder is touched");

    // Once they're let go, they go too.
    drop(other_app);
    drop(window);
    app.sweep(KEEP_FOR);
    assert_eq!(kept_folders(&root), vec![spoilt_slot]);
}

#[test]
fn a_copy_with_a_time_ahead_of_the_clock_is_still_swept() {
    let (_dir, root) = scratch_root();
    let recovery = Recovery::new(root.clone());
    let id = UntitledId::random();
    recovery.keep_untitled(id, "Untitled presentation", "# Ahead\n", &[], &[]).unwrap();
    let slot = untitled_slot(&root, id);
    set_kept_at(&slot, now_secs() + 10 * 365 * 24 * 60 * 60);
    recovery.sweep(KEEP_FOR);
    assert!(slot.is_dir(), "it goes by when its folder changed, which is now");
    let long_ago = SystemTime::now() - KEEP_FOR - Duration::from_secs(60 * 60);
    std::fs::File::open(&slot).unwrap().set_modified(long_ago).unwrap();
    recovery.sweep(KEEP_FOR);
    assert!(!slot.exists());
}

#[test]
fn a_copy_is_kept_when_its_lock_is_refused() {
    let (_dir, root) = scratch_root();
    let window = Recovery::new(root.clone()).for_another_window();
    window.fail_locks_for_test();
    let id = UntitledId::random();
    window.keep_untitled(id, "Untitled presentation", "# Kept anyway\n", &[], &[]).expect("kept without a lock");
    window.keep_untitled(id, "Untitled presentation", "# And again\n", &[], &[]).expect("and every time after");
    let slot = untitled_slot(&root, id);
    assert_eq!(std::fs::read_to_string(slot.join("slides.md")).unwrap(), "# And again\n");
}
