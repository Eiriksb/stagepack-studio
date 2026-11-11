use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use stagepack_studio::watcher::{self, Change};

fn expect_change(changes: &Receiver<Change>) -> Change {
    changes
        .recv_timeout(Duration::from_secs(5))
        .expect("a change event")
}

/// Gives the events of the last change time to arrive, and forgets them.
fn drain(changes: &Receiver<Change>) {
    while changes.recv_timeout(Duration::from_millis(300)).is_ok() {}
}

#[test]
fn a_change_to_the_slides_file_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("slides.md");
    std::fs::write(&file, "a").unwrap();
    let (tx, rx) = channel();
    let _watcher = watcher::watch(dir.path(), move |change| {
        let _ = tx.send(change);
    })
    .unwrap();
    std::fs::write(&file, "b").unwrap();
    assert_eq!(expect_change(&rx), Change::Slides);
}

#[test]
fn other_files_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = channel();
    let _watcher = watcher::watch(dir.path(), move |change| {
        let _ = tx.send(change);
    })
    .unwrap();
    std::fs::write(dir.path().join("notes.txt"), "b").unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(800)).is_err());
}

#[test]
fn saving_by_renaming_over_the_file_is_reported_every_time() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("slides.md");
    std::fs::write(&file, "a").unwrap();
    let (tx, rx) = channel();
    let _watcher = watcher::watch(dir.path(), move |change| {
        let _ = tx.send(change);
    })
    .unwrap();
    for contents in ["b", "c"] {
        let temporary = dir.path().join(".slides.md.tmp");
        std::fs::write(&temporary, contents).unwrap();
        std::fs::rename(&temporary, &file).unwrap();
        expect_change(&rx);
        drain(&rx);
    }
}

#[test]
fn reading_the_file_is_not_a_change() {
    // The app reads the file each time it changes. If that counted, it would never stop.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("slides.md");
    std::fs::write(&file, "a").unwrap();
    let (tx, rx) = channel();
    let _watcher = watcher::watch(dir.path(), move |change| {
        let _ = tx.send(change);
    })
    .unwrap();
    std::fs::read_to_string(&file).unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(800)).is_err());
}

/// A talk folder with a `components` folder, watched, and the changes it reports.
fn watched_talk() -> (tempfile::TempDir, watcher::TalkWatcher, Receiver<Change>) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), "a").unwrap();
    std::fs::create_dir(dir.path().join("components")).unwrap();
    std::fs::create_dir(dir.path().join("build")).unwrap();
    let (tx, rx) = channel();
    let watcher = watcher::watch(dir.path(), move |change| {
        let _ = tx.send(change);
    })
    .unwrap();
    (dir, watcher, rx)
}

#[test]
fn a_save_to_a_component_counts() {
    let (dir, _watcher, rx) = watched_talk();
    std::fs::write(dir.path().join("components/score.rs"), "fn a() {}").unwrap();
    assert_eq!(expect_change(&rx), Change::Component);
    drain(&rx);

    let temporary = dir.path().join("components/.score.rs.tmp");
    std::fs::write(&temporary, "fn b() {}").unwrap();
    std::fs::rename(&temporary, dir.path().join("components/score.rs")).unwrap();
    assert_eq!(expect_change(&rx), Change::Component);
}

#[test]
fn a_write_to_the_build_folder_does_not() {
    // The app writes there itself when it builds, and that mustn't start another build.
    let (dir, _watcher, rx) = watched_talk();
    std::fs::write(dir.path().join("build/score.wasm"), "wasm").unwrap();
    std::fs::write(dir.path().join("build/index.json"), "{}").unwrap();
    std::fs::write(dir.path().join("build/score.rs"), "fn a() {}").unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(800)).is_err());
}

#[test]
fn a_rust_file_elsewhere_does_not() {
    let (dir, _watcher, rx) = watched_talk();
    std::fs::write(dir.path().join("score.rs"), "fn a() {}").unwrap();
    std::fs::create_dir(dir.path().join("components/deeper")).unwrap();
    std::fs::write(dir.path().join("components/deeper/score.rs"), "fn a() {}").unwrap();
    std::fs::write(dir.path().join("components/notes.txt"), "a").unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("assets/slides.md"), "a").unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(800)).is_err());
}

#[test]
fn reading_a_component_is_not_a_change() {
    // A build reads each source, and that mustn't start another build.
    let (dir, _watcher, rx) = watched_talk();
    let file = dir.path().join("components/score.rs");
    std::fs::write(&file, "fn a() {}").unwrap();
    expect_change(&rx);
    drain(&rx);
    std::fs::read_to_string(&file).unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(800)).is_err());
}

#[test]
fn a_components_folder_made_later_is_watched() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), "a").unwrap();
    let (tx, rx) = channel();
    let _watcher = watcher::watch(dir.path(), move |change| {
        let _ = tx.send(change);
    })
    .unwrap();

    std::fs::create_dir(dir.path().join("components")).unwrap();
    assert_eq!(expect_change(&rx), Change::Component);
    drain(&rx);
    std::fs::write(dir.path().join("components/score.rs"), "fn a() {}").unwrap();
    assert_eq!(expect_change(&rx), Change::Component);
}

#[test]
fn a_components_folder_put_back_is_watched_again() {
    let (dir, _watcher, rx) = watched_talk();
    std::fs::remove_dir(dir.path().join("components")).unwrap();
    expect_change(&rx);
    drain(&rx);
    std::fs::create_dir(dir.path().join("components")).unwrap();
    expect_change(&rx);
    drain(&rx);
    std::fs::write(dir.path().join("components/score.rs"), "fn a() {}").unwrap();
    assert_eq!(expect_change(&rx), Change::Component);
}

#[cfg(unix)]
#[test]
fn a_linked_components_folder_is_not_watched() {
    let elsewhere = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), "a").unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), dir.path().join("components")).unwrap();
    let (tx, rx) = channel();
    let _watcher = watcher::watch(dir.path(), move |change| {
        let _ = tx.send(change);
    })
    .unwrap();

    std::fs::write(elsewhere.path().join("score.rs"), "fn a() {}").unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(800)).is_err());
}

#[test]
fn the_folder_below_is_not_watched() {
    // A talk with a large tree mustn't use up the system's watches.
    let (dir, _watcher, rx) = watched_talk();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    drain(&rx);
    std::fs::write(dir.path().join("assets/slides.md"), "a").unwrap();
    std::fs::create_dir(dir.path().join("assets/components")).unwrap();
    std::fs::write(dir.path().join("assets/components/score.rs"), "a").unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(800)).is_err());
}
