//! The Slidev add-on in the window: the Add-ons dialog, Export ▸ Slidev…, and a Slidev talk made
//! for Stagepack, opened with the add-on and without it. Every app folder is a scratch folder.

#[path = "support/typing.rs"]
mod typing;
#[path = "support/addons.rs"]
mod support;
#[path = "support/server.rs"]
mod server;

use std::path::{Path, PathBuf};

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{App, AppContext as _, Bounds, Point, TestAppContext, WindowBounds, WindowOptions, px, size};
use sha2::{Digest as _, Sha256};
use stagepack_studio::addons::dialog::{
    ADDON_CANCEL_ID, ADDON_CHECK_ID, ADDON_GET_ID, ADDON_INSTALL_ID, ADDON_NEWS_ID, ADDON_PROBLEM_ID, ADDON_PROGRESS_ID, ADDON_REMOVE_ID, ADDON_STATE_ID,
    ADDON_UPDATE_ID, ADDONS_DIALOG_ID, NEWEST,
};
use stagepack_studio::permissions::BuildPermissions;
use stagepack_studio::recovery::Recovery;
use stagepack_studio::start_screen::{START_OPEN_FILE_ID, START_PROBLEM_ID};
use stagepack_studio::workspace_view::{
    self, EXPORT_REPORT_ID, EXPORT_SLIDEV_ITEM_ID, ExportSlidev, IMPORT_REPORT_ID, OFFER_ADD_ONS_ID, SLIDEV_NOT_INSTALLED, ShowAddOns, WorkspaceView,
};
use support::*;
use typing::*;

/// A start window whose working copies, Documents folder, build permissions and app folder are
/// all under `root`.
fn open_start(cx: &mut TestAppContext, root: &Path) -> Typing {
    let places = UntitledPlaces::under(root);
    let app = root.join("app");
    let scratch = tempfile::tempdir().unwrap();
    let file = scratch.path().join("no file yet");
    let (handle, view) = cx.update(|cx: &mut App| {
        gpui_kit::init(cx);
        workspace_view::init(cx);
        let bounds = Bounds { origin: Point::default(), size: size(px(1280.), px(800.)) };
        gpui_kit::open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() }, cx, |window, cx| {
            cx.new(|cx| {
                let mut view = WorkspaceView::for_start(Some(Recovery::new(places.recovery)), window, cx);
                view.set_permissions_for_test(BuildPermissions::load(places.permissions));
                view.set_documents_folder_for_test(places.documents);
                view.set_no_watching_for_test();
                view.set_stagepack_for_test(None);
                view.set_app_folder_for_test(app, cx);
                view
            })
        })
        .expect("open test window")
    });
    let typing = Typing { _folder: scratch, file, handle, view };
    typing.settle(cx);
    typing
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

fn labels_within(typing: &Typing, cx: &mut TestAppContext, id: impl Into<gpui_kit::ElementId>) -> Vec<String> {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.within(id).elements().iter().filter_map(|element| element.label().map(str::to_string)).collect()
    })
}

/// Answers the Open dialog showing with `chosen`, a file.
fn answer_open(typing: &Typing, cx: &mut TestAppContext, chosen: PathBuf) {
    cx.run_until_parked();
    assert!(cx.did_prompt_for_paths(), "an Open dialog is showing");
    cx.simulate_path_prompt_response(move |options| {
        assert!(options.files && !options.directories && !options.multiple);
        Some(vec![chosen])
    });
    settle_work(typing, cx);
}

/// Lets background work end and the window draw what came of it.
fn settle_work(typing: &Typing, cx: &mut TestAppContext) {
    for _ in 0..20 {
        cx.run_until_parked();
        typing.settle(cx);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn show_add_ons(typing: &Typing, cx: &mut TestAppContext) {
    typing.with_window(cx, |window, cx| window.dispatch_action(Box::new(ShowAddOns), cx));
    typing.settle(cx);
}

#[gpui_kit::test]
fn the_addons_dialog_shows_not_installed_then_installed(cx: &mut TestAppContext) {
    test_key();
    let root = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let typing = open_start(cx, root.path());

    show_add_ons(&typing, cx);
    assert!(shows(&typing, cx, ADDONS_DIALOG_ID));
    let row = labels_within(&typing, cx, ADDONS_DIALOG_ID);
    assert!(row.iter().any(|label| label == "Slidev — export to and import from Slidev (for Stagepack)"), "{row:?}");
    assert_eq!(label_of(&typing, cx, ADDON_STATE_ID).as_deref(), Some("Not installed"));
    assert!(shows(&typing, cx, ADDON_INSTALL_ID));
    assert!(!shows(&typing, cx, ADDON_REMOVE_ID));

    let file = package_file(scratch.path(), &answering_guest(&export_answer()), "0.1.0");
    click_on(&typing, cx, ADDON_INSTALL_ID);
    answer_open(&typing, cx, file);
    assert_eq!(label_of(&typing, cx, ADDON_STATE_ID).as_deref(), Some("Installed, version 0.1.0"));
    assert!(shows(&typing, cx, ADDON_REMOVE_ID) && shows(&typing, cx, ADDON_INSTALL_ID));
    assert!(root.path().join("app/addons/slidev/addon.wasm").is_file());

    // A file signed with another key says why in one line, and the earlier install stays.
    let forged = scratch.path().join("forged.spaddon");
    std::fs::write(&forged, package_signed(&answering_guest(&export_answer()), "0.2.0", &other_key())).unwrap();
    click_on(&typing, cx, ADDON_INSTALL_ID);
    answer_open(&typing, cx, forged);
    let problem = label_of(&typing, cx, ADDON_PROBLEM_ID).expect("it says why");
    assert!(problem.contains("signature") && !problem.contains('\n'), "{problem}");
    assert_eq!(label_of(&typing, cx, ADDON_STATE_ID).as_deref(), Some("Installed, version 0.1.0"));

    // A damaged one shows as damaged, with Remove alone, which takes it away.
    let wasm = root.path().join("app/addons/slidev/addon.wasm");
    let mut bytes = std::fs::read(&wasm).unwrap();
    bytes[0] ^= 1;
    std::fs::write(&wasm, bytes).unwrap();
    typing.with_window(cx, gpui_kit::component::WindowExt::close_dialog);
    show_add_ons(&typing, cx);
    let state = label_of(&typing, cx, ADDON_STATE_ID).unwrap();
    assert!(state.starts_with("Damaged: ") && state.contains("signature"), "{state}");
    assert!(shows(&typing, cx, ADDON_REMOVE_ID) && !shows(&typing, cx, ADDON_INSTALL_ID));
    click_on(&typing, cx, ADDON_REMOVE_ID);
    settle_work(&typing, cx);
    assert_eq!(label_of(&typing, cx, ADDON_STATE_ID).as_deref(), Some("Not installed"));
    assert!(!root.path().join("app/addons/slidev").exists());
}

#[gpui_kit::test]
fn slidev_export_is_disabled_until_installed(cx: &mut TestAppContext) {
    test_key();
    let root = tempfile::tempdir().unwrap();
    let app = root.path().join("app");
    let typing = open(cx, "# Rock pools\n\nA made-up talk.\n");
    let set_app = app.clone();
    cx.update(|cx| typing.view.update(cx, |view, cx| view.set_app_folder_for_test(set_app, cx)));
    typing.settle(cx);
    assert!(!cx.read(|cx| typing.view.read(cx).slidev_export_ready()));

    // The menu item says why it's off.
    click_on(&typing, cx, workspace_view::EXPORT_MENU_ID);
    let item = label_of(&typing, cx, EXPORT_SLIDEV_ITEM_ID).expect("the menu has Slidev…");
    assert!(item.starts_with("Slidev…") && item.contains(SLIDEV_NOT_INSTALLED), "{item}");
    typing.press(cx, "escape");
    // And asked for anyway, it says so, with no Save dialog.
    typing.with_window(cx, |window, cx| window.dispatch_action(Box::new(ExportSlidev), cx));
    typing.settle(cx);
    assert!(!cx.did_prompt_for_new_path());
    assert_eq!(typing.status(cx).as_deref(), Some(SLIDEV_NOT_INSTALLED));

    let scratch = tempfile::tempdir().unwrap();
    stagepack_studio::addons::install_from_file(&app, &package_file(scratch.path(), &answering_guest(&export_answer()), "0.1.0")).unwrap();
    cx.update(workspace_view::refresh_addon_status);
    typing.settle(cx);
    assert!(cx.read(|cx| typing.view.read(cx).slidev_export_ready()));
    assert_eq!(stagepack_studio::export::slidev_file_name("Rock pools"), "Rock pools (Slidev).sptx");

    let out = scratch.path().join("Rock pools (Slidev).sptx");
    typing.with_window(cx, |window, cx| window.dispatch_action(Box::new(ExportSlidev), cx));
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path(), "a Save dialog is showing");
    let chosen = out.clone();
    cx.simulate_new_path_selection(move |_| Some(chosen));
    settle_work(&typing, cx);
    let status = typing.status(cx).unwrap_or_default();
    assert_eq!(status, "Exported Rock pools (Slidev).sptx, with 1 note.");
    // The add-on's notice is in the report, by its slide.
    assert!(shows(&typing, cx, EXPORT_REPORT_ID));
    let report = labels_within(&typing, cx, EXPORT_REPORT_ID);
    for line in ["Slide 2", "The gauge may not sit where it does on the slide."] {
        assert!(report.iter().any(|label| label == line), "{line:?} isn't in {report:?}");
    }
    assert!(out.is_file());
}

/// A window on `source`, packed, with an add-on that gives back `answer` installed for it.
fn packed_with_addon(cx: &mut TestAppContext, source: &str, answer: &addon_abi::Bundle) -> (Typing, tempfile::TempDir) {
    test_key();
    let root = tempfile::tempdir().unwrap();
    let app = root.path().join("app");
    stagepack_studio::addons::install_from_file(&app, &package_file(root.path(), &answering_guest(answer), "0.1.0")).unwrap();
    let typing = open_packed(cx, source);
    cx.update(|cx| typing.view.update(cx, |view, cx| view.set_app_folder_for_test(app, cx)));
    typing.settle(cx);
    (typing, root)
}

/// Asks for Export ▸ Slidev… and answers the Save dialog with `out`.
fn export_slidev_to(typing: &Typing, cx: &mut TestAppContext, out: PathBuf) {
    typing.with_window(cx, |window, cx| window.dispatch_action(Box::new(ExportSlidev), cx));
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path(), "a Save dialog is showing");
    cx.simulate_new_path_selection(move |_| Some(out));
    settle_work(typing, cx);
}

#[gpui_kit::test]
fn slidev_export_wont_write_over_the_open_talk(cx: &mut TestAppContext) {
    let (typing, _root) = packed_with_addon(cx, "# Rock pools\n", &export_answer());
    let hash = Sha256::digest(std::fs::read(&typing.file).unwrap());
    export_slidev_to(&typing, cx, typing.file.clone());
    assert_eq!(typing.status(cx).as_deref(), Some("talk.sptx is a talk open in a window, so it wasn't written over."));
    assert_eq!(Sha256::digest(std::fs::read(&typing.file).unwrap()), hash, "the open talk is as it was");

    // Nor over the talk open in another window.
    let (other, _other_root) = packed_with_addon(cx, "# Another\n", &export_answer());
    let hash = Sha256::digest(std::fs::read(&other.file).unwrap());
    export_slidev_to(&typing, cx, other.file.clone());
    assert!(typing.status(cx).unwrap_or_default().ends_with("is a talk open in a window, so it wasn't written over."));
    assert_eq!(Sha256::digest(std::fs::read(&other.file).unwrap()), hash);
}

#[gpui_kit::test]
fn an_export_with_many_notes_lists_200_and_how_many_more(cx: &mut TestAppContext) {
    let mut answer = export_answer();
    answer.retain(|(name, _)| name != addon_abi::REPORT);
    answer.push(report((1..=250).map(|n| notice(Some(n), addon_abi::NoticeKind::Approximation, &format!("Note {n}."))).collect()));
    let (typing, root) = packed_with_addon(cx, "# Rock pools\n", &answer);
    export_slidev_to(&typing, cx, root.path().join("out.sptx"));
    assert_eq!(typing.status(cx).as_deref(), Some("Exported out.sptx, with 250 notes."));
    let report = labels_within(&typing, cx, EXPORT_REPORT_ID);
    assert!(report.iter().any(|label| label == "Note 200."), "{report:?}");
    assert!(!report.iter().any(|label| label == "Note 201."), "{report:?}");
    assert!(report.iter().any(|label| label == "and 50 more"), "{report:?}");
}

#[gpui_kit::test]
fn a_stagepack_talk_without_the_addon_offers_add_ons(cx: &mut TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let talk = stagepack_talk(dir.path());
    let typing = open_start(cx, root.path());

    click_on(&typing, cx, START_OPEN_FILE_ID);
    answer_open(&typing, cx, talk);
    let says = label_of(&typing, cx, START_PROBLEM_ID).expect("it says what the file is");
    assert!(says.contains("Slidev talk made for Stagepack"), "{says}");
    assert!(shows(&typing, cx, OFFER_ADD_ONS_ID));
    click_on(&typing, cx, OFFER_ADD_ONS_ID);
    assert!(shows(&typing, cx, ADDONS_DIALOG_ID));
    assert_eq!(label_of(&typing, cx, ADDON_STATE_ID).as_deref(), Some("Not installed"));
}

#[gpui_kit::test]
fn a_stagepack_talk_with_the_addon_opens_unsaved_with_a_report(cx: &mut TestAppContext) {
    test_key();
    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    stagepack_studio::addons::install_from_file(&root.path().join("app"), &package_file(scratch.path(), &answering_guest(&import_answer()), "0.1.0"))
        .unwrap();
    let talk = stagepack_talk(dir.path());
    let hash = Sha256::digest(std::fs::read(&talk).unwrap());
    let typing = open_start(cx, root.path());

    click_on(&typing, cx, START_OPEN_FILE_ID);
    answer_open(&typing, cx, talk.clone());
    let document = cx.read(|cx| typing.view.read(cx).document().cloned()).expect("the talk is open");
    cx.read(|cx| {
        let document = document.read(cx);
        assert!(document.is_untitled(), "it has no file of its own");
        assert!(document.has_unsaved_changes());
        assert_eq!(document.slide_count(), 2);
    });
    assert!(shows(&typing, cx, IMPORT_REPORT_ID));
    let report = labels_within(&typing, cx, IMPORT_REPORT_ID);
    for line in ["Whole talk", "Stagepack's bundled packages (vendor/) aren't used.", "Slide 2", "A Vue component became a labelled box."] {
        assert!(report.iter().any(|label| label == line), "{line:?} isn't in {report:?}");
    }
    assert_eq!(Sha256::digest(std::fs::read(&talk).unwrap()), hash, "the original is never written to");
}

/// A window opened on `file`, as the app opens one it's given, with add-ons in `app`.
fn open_file(cx: &mut TestAppContext, file: PathBuf, app: PathBuf) -> Typing {
    let scratch = tempfile::tempdir().unwrap();
    let (handle, view) = cx.update(|cx: &mut App| {
        gpui_kit::init(cx);
        workspace_view::init(cx);
        let bounds = Bounds { origin: Point::default(), size: size(px(1280.), px(800.)) };
        gpui_kit::open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() }, cx, |window, cx| {
            cx.new(|cx| WorkspaceView::open_with_addons(file, Some(app), window, cx))
        })
        .expect("open test window")
    });
    let typing = Typing { file: scratch.path().join("no file yet"), _folder: scratch, handle, view };
    settle_work(&typing, cx);
    typing
}

#[gpui_kit::test]
fn a_window_opened_on_a_stagepack_talk_imports_it_or_offers_add_ons(cx: &mut TestAppContext) {
    test_key();
    let dir = tempfile::tempdir().unwrap();
    let talk = stagepack_talk(dir.path());
    let root = tempfile::tempdir().unwrap();
    let app = root.path().join("app");

    let without = open_file(cx, talk.clone(), app.clone());
    assert_eq!(label_of(&without, cx, workspace_view::PROBLEM_ID).as_deref(), Some(workspace_view::SLIDEV_TALK));
    assert!(shows(&without, cx, OFFER_ADD_ONS_ID));

    let scratch = tempfile::tempdir().unwrap();
    stagepack_studio::addons::install_from_file(&app, &package_file(scratch.path(), &answering_guest(&import_answer()), "0.1.0")).unwrap();
    let with = open_file(cx, talk, app);
    let document = cx.read(|cx| with.view.read(cx).document().cloned()).expect("the talk is open");
    assert!(cx.read(|cx| document.read(cx).is_untitled()));
    assert!(shows(&with, cx, IMPORT_REPORT_ID));
    assert!(!shows(&with, cx, OFFER_ADD_ONS_ID));
}

// Getting the add-on: every server is the test's own, on 127.0.0.1.

/// The tests that get the add-on, one at a time, as where it's got from is the whole process's.
fn one_at_a_time() -> std::sync::MutexGuard<'static, ()> {
    static NETWORK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    NETWORK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What the test's server serves at a path: a file, or the first `.1` bytes of a file `.0`
/// bytes long, and then nothing for a long while.
#[derive(Clone)]
enum Served {
    File(Vec<u8>),
    Stalled(u64, usize),
}

type Files = std::sync::Arc<std::sync::Mutex<Vec<(String, Served)>>>;

/// A server of the test's own, serving what's put in its files, made where the app gets the
/// add-on from.
fn add_on_server() -> (server::Server, Files) {
    let files: Files = Default::default();
    let held = files.clone();
    let server = server::Server::start(move |asked| {
        let found = held.lock().unwrap().iter().find(|(path, _)| path == asked).map(|(_, served)| served.clone());
        match found {
            Some(Served::File(body)) => server::ok(body),
            Some(Served::Stalled(length, sent)) => server::Reply::Stall(length, sent),
            None => server::Reply::Fixed(404, Vec::new(), b"not here".to_vec()),
        }
    });
    server.allowed("/index.json");
    (server, files)
}

/// The signed package of a made-up add-on at `version`.
fn package_at(version: &str) -> Vec<u8> {
    package_signed(&answering_guest_at(&export_answer(), version), version, test_key())
}

/// Has `server` offer the add-on at `version`, its package stalled part way if `stalled`.
fn offer(server: &server::Server, files: &Files, version: &str, stalled: bool) {
    let package = package_at(version);
    let index = server::index_of(server, &package, version);
    let path = index.url.strip_prefix(&server.base).unwrap().to_string();
    let served = if stalled { Served::Stalled(package.len() as u64, package.len() / 2) } else { Served::File(package) };
    let mut files = files.lock().unwrap();
    files.clear();
    files.push(("/index.json".into(), Served::File(server::json(&index))));
    files.push((path, served));
}

fn hits(server: &server::Server) -> usize {
    server.hits.load(std::sync::atomic::Ordering::SeqCst)
}

/// Lets the window and the work off its thread go on until `done`, for no more than 20 s.
fn wait_until(typing: &Typing, cx: &mut TestAppContext, what: &str, mut done: impl FnMut(&Typing, &mut TestAppContext) -> bool) {
    let start = std::time::Instant::now();
    while !done(typing, cx) {
        assert!(start.elapsed() < std::time::Duration::from_secs(20), "waited too long for {what}");
        cx.executor().advance_clock(stagepack_studio::addons::dialog::FETCH_TICK);
        cx.run_until_parked();
        typing.settle(cx);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn state_is(typing: &Typing, cx: &mut TestAppContext, state: &str) -> bool {
    label_of(typing, cx, ADDON_STATE_ID).as_deref() == Some(state)
}

/// What's in the add-ons folder under `app`, but its lock.
fn addons_folder(app: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(app.join("addons"))
        .map(|entries| entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).filter(|name| name != ".lock").collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// The hash of each of the installed add-on's files under `app`.
fn installed_hashes(app: &Path) -> Vec<Vec<u8>> {
    ["addon.json", "addon.wasm", "addon.sig"].iter().map(|name| Sha256::digest(std::fs::read(app.join("addons/slidev").join(name)).unwrap()).to_vec()).collect()
}

#[test]
fn downloading_says_megabytes() {
    use stagepack_studio::addons::dialog::downloading_words;
    assert_eq!(downloading_words(1_500_000, 14_000_000), "Downloading… 1.5 of 14.0 MB");
    assert_eq!(downloading_words(0, 14_000_000), "Downloading… 0.0 of 14.0 MB");
    // A little is never nothing.
    assert_eq!(downloading_words(2_000, 9_000), "Downloading… 0.1 of 0.1 MB");
}

#[gpui_kit::test]
fn get_installs_from_the_local_server(cx: &mut TestAppContext) {
    let _one = one_at_a_time();
    test_key();
    let (server, files) = add_on_server();
    offer(&server, &files, "0.2.0", false);
    let root = tempfile::tempdir().unwrap();
    let app = root.path().join("app");
    let typing = open_start(cx, root.path());

    show_add_ons(&typing, cx);
    assert_eq!(label_of(&typing, cx, ADDON_STATE_ID).as_deref(), Some("Not installed"));
    assert!(shows(&typing, cx, ADDON_GET_ID) && shows(&typing, cx, ADDON_INSTALL_ID));
    assert!(!shows(&typing, cx, ADDON_CHECK_ID) && !shows(&typing, cx, ADDON_REMOVE_ID) && !shows(&typing, cx, ADDON_UPDATE_ID));
    assert_eq!(hits(&server), 0);

    click_on(&typing, cx, ADDON_GET_ID);
    wait_until(&typing, cx, "the install", |typing, cx| state_is(typing, cx, "Installed, version 0.2.0"));
    assert!(app.join("addons/slidev/addon.wasm").is_file());
    assert_eq!(addons_folder(&app), ["slidev"], "nothing else is left");
    assert_eq!(hits(&server), 2, "the index, then the package");
    assert!(shows(&typing, cx, ADDON_CHECK_ID) && shows(&typing, cx, ADDON_REMOVE_ID) && shows(&typing, cx, ADDON_INSTALL_ID));
    assert!(!shows(&typing, cx, ADDON_GET_ID) && !shows(&typing, cx, ADDON_CANCEL_ID) && !shows(&typing, cx, ADDON_PROGRESS_ID));
    assert!(!shows(&typing, cx, ADDON_PROBLEM_ID));
    // The window's menus know it is there.
    assert!(cx.read(|cx| typing.view.read(cx).slidev_export_ready()));

    // A server that can't give it says why, in one line.
    files.lock().unwrap().clear();
    let other = tempfile::tempdir().unwrap();
    let fresh = open_start(cx, other.path());
    show_add_ons(&fresh, cx);
    click_on(&fresh, cx, ADDON_GET_ID);
    wait_until(&fresh, cx, "the problem", |typing, cx| shows(typing, cx, ADDON_PROBLEM_ID));
    let problem = label_of(&fresh, cx, ADDON_PROBLEM_ID).unwrap();
    assert!(problem.contains("404") && !problem.contains('\n'), "{problem}");
    assert_eq!(label_of(&fresh, cx, ADDON_STATE_ID).as_deref(), Some("Not installed"));
    assert!(shows(&fresh, cx, ADDON_GET_ID));
}

#[gpui_kit::test]
fn check_for_updates_offers_a_newer_version_only(cx: &mut TestAppContext) {
    let _one = one_at_a_time();
    test_key();
    let (server, files) = add_on_server();
    let root = tempfile::tempdir().unwrap();
    let app = root.path().join("app");
    stagepack_studio::addons::install_from_bytes(&app, &package_at("0.2.0")).unwrap();
    let typing = open_start(cx, root.path());
    show_add_ons(&typing, cx);
    assert!(shows(&typing, cx, ADDON_CHECK_ID) && shows(&typing, cx, ADDON_REMOVE_ID) && shows(&typing, cx, ADDON_INSTALL_ID));
    assert!(!shows(&typing, cx, ADDON_GET_ID) && !shows(&typing, cx, ADDON_UPDATE_ID));

    // The same version, and an older one, are the newest.
    for offered in ["0.2.0", "0.1.0"] {
        offer(&server, &files, offered, false);
        let before = hits(&server);
        click_on(&typing, cx, ADDON_CHECK_ID);
        wait_until(&typing, cx, "the check", |typing, cx| label_of(typing, cx, ADDON_NEWS_ID).is_some());
        assert_eq!(label_of(&typing, cx, ADDON_NEWS_ID).as_deref(), Some(NEWEST), "offered {offered}");
        assert!(!shows(&typing, cx, ADDON_UPDATE_ID), "offered {offered}");
        assert_eq!(hits(&server), before + 1, "only the index was asked for");
    }

    offer(&server, &files, "0.3.0", false);
    click_on(&typing, cx, ADDON_CHECK_ID);
    wait_until(&typing, cx, "the check", |typing, cx| label_of(typing, cx, ADDON_NEWS_ID).as_deref() == Some("Version 0.3.0 is available"));
    assert!(shows(&typing, cx, ADDON_UPDATE_ID));
    assert_eq!(label_of(&typing, cx, ADDON_STATE_ID).as_deref(), Some("Installed, version 0.2.0"));

    click_on(&typing, cx, ADDON_UPDATE_ID);
    wait_until(&typing, cx, "the update", |typing, cx| state_is(typing, cx, "Installed, version 0.3.0"));
    assert!(!shows(&typing, cx, ADDON_UPDATE_ID) && !shows(&typing, cx, ADDON_NEWS_ID) && !shows(&typing, cx, ADDON_PROBLEM_ID));
    assert_eq!(addons_folder(&app), ["slidev"]);
}

#[gpui_kit::test]
fn cancel_leaves_nothing_behind(cx: &mut TestAppContext) {
    let _one = one_at_a_time();
    test_key();
    let (server, files) = add_on_server();
    let root = tempfile::tempdir().unwrap();
    let app = root.path().join("app");
    let typing = open_start(cx, root.path());
    show_add_ons(&typing, cx);

    // Get, cancelled part way: back to Get, and nothing is installed.
    offer(&server, &files, "0.2.0", true);
    click_on(&typing, cx, ADDON_GET_ID);
    wait_until(&typing, cx, "the download", |typing, cx| label_of(typing, cx, ADDON_PROGRESS_ID).is_some_and(|said| said.starts_with("Downloading… ")));
    let progress = label_of(&typing, cx, ADDON_PROGRESS_ID).unwrap();
    assert!(progress.ends_with(" MB") && progress.contains(" of "), "{progress}");
    assert!(shows(&typing, cx, ADDON_CANCEL_ID) && !shows(&typing, cx, ADDON_GET_ID));
    let cancelled = std::time::Instant::now();
    click_on(&typing, cx, ADDON_CANCEL_ID);
    wait_until(&typing, cx, "the row as it was", |typing, cx| shows(typing, cx, ADDON_GET_ID));
    assert!(cancelled.elapsed() < std::time::Duration::from_secs(1), "{:?}", cancelled.elapsed());
    assert_eq!(label_of(&typing, cx, ADDON_STATE_ID).as_deref(), Some("Not installed"));
    assert!(!shows(&typing, cx, ADDON_CANCEL_ID) && !shows(&typing, cx, ADDON_PROGRESS_ID) && !shows(&typing, cx, ADDON_PROBLEM_ID));
    settle_work(&typing, cx);
    assert!(addons_folder(&app).is_empty(), "{:?}", addons_folder(&app));

    // Update, cancelled part way: the version on offer still is, and the old install is untouched.
    stagepack_studio::addons::install_from_bytes(&app, &package_at("0.1.0")).unwrap();
    let hashes = installed_hashes(&app);
    let typing = open_start(cx, root.path());
    show_add_ons(&typing, cx);
    offer(&server, &files, "0.2.0", true);
    click_on(&typing, cx, ADDON_CHECK_ID);
    wait_until(&typing, cx, "the check", |typing, cx| shows(typing, cx, ADDON_UPDATE_ID));
    click_on(&typing, cx, ADDON_UPDATE_ID);
    wait_until(&typing, cx, "the download", |typing, cx| label_of(typing, cx, ADDON_PROGRESS_ID).is_some_and(|said| said.starts_with("Downloading… ")));
    let cancelled = std::time::Instant::now();
    click_on(&typing, cx, ADDON_CANCEL_ID);
    wait_until(&typing, cx, "the row as it was", |typing, cx| shows(typing, cx, ADDON_UPDATE_ID));
    assert!(cancelled.elapsed() < std::time::Duration::from_secs(1), "{:?}", cancelled.elapsed());
    assert_eq!(label_of(&typing, cx, ADDON_NEWS_ID).as_deref(), Some("Version 0.2.0 is available"));
    assert_eq!(label_of(&typing, cx, ADDON_STATE_ID).as_deref(), Some("Installed, version 0.1.0"));
    settle_work(&typing, cx);
    assert_eq!(installed_hashes(&app), hashes);
    assert_eq!(addons_folder(&app), ["slidev"]);
}

#[gpui_kit::test]
fn an_update_while_the_addon_is_busy_says_so(cx: &mut TestAppContext) {
    let _one = one_at_a_time();
    test_key();
    let (server, files) = add_on_server();
    let root = tempfile::tempdir().unwrap();
    let app = root.path().join("app");
    stagepack_studio::addons::install_from_bytes(&app, &package_at("0.1.0")).unwrap();
    let hashes = installed_hashes(&app);
    let typing = open_start(cx, root.path());
    show_add_ons(&typing, cx);
    offer(&server, &files, "0.2.0", false);
    click_on(&typing, cx, ADDON_CHECK_ID);
    wait_until(&typing, cx, "the check", |typing, cx| shows(typing, cx, ADDON_UPDATE_ID));

    // As an export or an import holds it while it runs.
    let in_use = stagepack_studio::addons::in_use(&app);
    let before = hits(&server);
    click_on(&typing, cx, ADDON_UPDATE_ID);
    wait_until(&typing, cx, "the problem", |typing, cx| shows(typing, cx, ADDON_PROBLEM_ID));
    assert_eq!(label_of(&typing, cx, ADDON_PROBLEM_ID).as_deref(), Some(stagepack_studio::addons::BUSY));
    assert_eq!(hits(&server), before, "nothing was downloaded");
    assert_eq!(label_of(&typing, cx, ADDON_STATE_ID).as_deref(), Some("Installed, version 0.1.0"));
    assert_eq!(installed_hashes(&app), hashes);
    assert!(shows(&typing, cx, ADDON_UPDATE_ID), "it can be asked for again");

    drop(in_use);
    click_on(&typing, cx, ADDON_UPDATE_ID);
    wait_until(&typing, cx, "the update", |typing, cx| state_is(typing, cx, "Installed, version 0.2.0"));
    assert!(!shows(&typing, cx, ADDON_PROBLEM_ID));
}

#[gpui_kit::test]
fn opening_the_dialog_makes_no_request(cx: &mut TestAppContext) {
    let _one = one_at_a_time();
    test_key();
    let (server, files) = add_on_server();
    offer(&server, &files, "0.2.0", false);
    let asked = stagepack_studio::addons::fetch::requests_for_test();
    let root = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let typing = open_start(cx, root.path());

    // From the menu, not installed.
    show_add_ons(&typing, cx);
    assert!(shows(&typing, cx, ADDON_GET_ID));
    settle_work(&typing, cx);
    typing.with_window(cx, gpui_kit::component::WindowExt::close_dialog);

    // From a Slidev talk opened without the add-on.
    click_on(&typing, cx, START_OPEN_FILE_ID);
    answer_open(&typing, cx, stagepack_talk(dir.path()));
    click_on(&typing, cx, OFFER_ADD_ONS_ID);
    assert!(shows(&typing, cx, ADDONS_DIALOG_ID) && shows(&typing, cx, ADDON_GET_ID));
    settle_work(&typing, cx);
    typing.with_window(cx, gpui_kit::component::WindowExt::close_dialog);

    // And installed, with Check for updates.
    stagepack_studio::addons::install_from_bytes(&root.path().join("app"), &package_at("0.1.0")).unwrap();
    show_add_ons(&typing, cx);
    assert!(shows(&typing, cx, ADDON_CHECK_ID));
    settle_work(&typing, cx);
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(hits(&server), 0);
    assert_eq!(stagepack_studio::addons::fetch::requests_for_test(), asked);

    // Only a click asks, which the server counts.
    click_on(&typing, cx, ADDON_CHECK_ID);
    wait_until(&typing, cx, "the check", |typing, cx| shows(typing, cx, ADDON_UPDATE_ID));
    assert_eq!(hits(&server), 1);
    assert_eq!(stagepack_studio::addons::fetch::requests_for_test(), asked + 1);
}

#[gpui_kit::test]
fn start_up_makes_no_request(cx: &mut TestAppContext) {
    let _one = one_at_a_time();
    test_key();
    let (server, files) = add_on_server();
    offer(&server, &files, "0.2.0", false);
    let asked = stagepack_studio::addons::fetch::requests_for_test();
    let dir = tempfile::tempdir().unwrap();
    let talk = stagepack_talk(dir.path());

    // The start screen, a window opened on a Slidev talk without the add-on, and one with it.
    let root = tempfile::tempdir().unwrap();
    let start = open_start(cx, root.path());
    let without = open_file(cx, talk.clone(), root.path().join("app"));
    assert!(shows(&without, cx, OFFER_ADD_ONS_ID));
    let with_root = tempfile::tempdir().unwrap();
    let with_app = with_root.path().join("app");
    stagepack_studio::addons::install_from_file(&with_app, &package_file(with_root.path(), &answering_guest(&import_answer()), "0.1.0")).unwrap();
    let with = open_file(cx, talk, with_app);
    assert!(shows(&with, cx, IMPORT_REPORT_ID));
    // And a talk of the app's own.
    let own = open(cx, "# Rock pools\n");
    for typing in [&start, &without, &with, &own] {
        settle_work(typing, cx);
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    assert_eq!(hits(&server), 0);
    assert_eq!(stagepack_studio::addons::fetch::requests_for_test(), asked);
}
