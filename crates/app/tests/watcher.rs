use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use stagepack_studio::watcher;

fn expect_change(changes: &Receiver<()>) {
    changes
        .recv_timeout(Duration::from_secs(5))
        .expect("a change event");
}

/// Gives the events of the last change time to arrive, and forgets them.
fn drain(changes: &Receiver<()>) {
    while changes.recv_timeout(Duration::from_millis(300)).is_ok() {}
}

#[test]
fn a_change_to_the_slides_file_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("slides.md");
    std::fs::write(&file, "a").unwrap();
    let (tx, rx) = channel();
    let _watcher = watcher::watch(dir.path(), move || {
        let _ = tx.send(());
    })
    .unwrap();
    std::fs::write(&file, "b").unwrap();
    expect_change(&rx);
}

#[test]
fn other_files_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = channel();
    let _watcher = watcher::watch(dir.path(), move || {
        let _ = tx.send(());
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
    let _watcher = watcher::watch(dir.path(), move || {
        let _ = tx.send(());
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
    let _watcher = watcher::watch(dir.path(), move || {
        let _ = tx.send(());
    })
    .unwrap();
    std::fs::read_to_string(&file).unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(800)).is_err());
}
