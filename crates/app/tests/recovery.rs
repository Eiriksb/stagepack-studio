use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, Point, TestAppContext, VisualTestContext,
    WindowBounds, WindowOptions, px, size,
};
use slides_engine::Fonts;
use stagepack_studio::document::{Document, TalkPlace};
use stagepack_studio::notes_panel::NOTES_ID;
use stagepack_studio::recovery::{KEEP_FOR, Recovery, hash_text};
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
