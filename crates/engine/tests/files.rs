use slides_engine::*;

#[test]
fn reads_a_file_inside_the_talk() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("assets/a.txt"), b"hi").unwrap();
    assert_eq!(FolderFiles::new(dir.path().into()).read("assets/a.txt"), Some(b"hi".to_vec()));
}

#[test]
fn reading_outside_the_talk_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("secret.txt"), b"no").unwrap();
    let talk = dir.path().join("talk");
    std::fs::create_dir(&talk).unwrap();
    let files = FolderFiles::new(talk.clone());
    assert_eq!(files.read("../secret.txt"), None);
    assert_eq!(files.read("\\\\server\\share\\x"), None);
    assert_eq!(files.read("C:\\x"), None);
    assert_eq!(files.read("a\\b"), None);
    assert_eq!(files.read(dir.path().join("secret.txt").to_str().unwrap()), None);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.path().join("secret.txt"), talk.join("link.txt")).unwrap();
        assert_eq!(files.read("link.txt"), None);
    }
}

#[test]
fn a_stamp_is_a_files_length_and_modified_time() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"hi").unwrap();
    let modified = std::fs::metadata(dir.path().join("a.txt")).unwrap().modified().unwrap();
    assert_eq!(FolderFiles::new(dir.path().into()).stamp("a.txt"), Some((2, modified)));
}

#[test]
fn a_stamp_cannot_reach_outside_the_talk() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("secret.txt"), b"no").unwrap();
    let talk = dir.path().join("talk");
    std::fs::create_dir(&talk).unwrap();
    let files = FolderFiles::new(talk.clone());
    assert_eq!(files.stamp("../secret.txt"), None);
    assert_eq!(files.stamp(dir.path().join("secret.txt").to_str().unwrap()), None);
    assert_eq!(files.stamp("missing.txt"), None);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.path().join("secret.txt"), talk.join("link.txt")).unwrap();
        assert_eq!(files.stamp("link.txt"), None);
    }
}

#[test]
fn a_file_over_the_limit_reads_as_absent() {
    let dir = tempfile::tempdir().unwrap();
    let huge = std::fs::File::create(dir.path().join("huge.ttf")).unwrap();
    // Sparse, so the test costs no disk.
    huge.set_len(MAX_FILE_BYTES + 1).unwrap();
    assert_eq!(FolderFiles::new(dir.path().into()).read("huge.ttf"), None);
}

#[cfg(unix)]
#[test]
fn a_named_pipe_reads_as_absent_without_waiting_for_a_writer() {
    let dir = tempfile::tempdir().unwrap();
    let made = std::process::Command::new("mkfifo").arg(dir.path().join("pipe.png")).status().unwrap();
    assert!(made.success());
    let files = FolderFiles::new(dir.path().into());
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || done.send(files.read("pipe.png")));
    let read = finished.recv_timeout(std::time::Duration::from_secs(5)).expect("the read came back");
    assert_eq!(read, None);
}

#[test]
fn a_talks_text_is_read_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("slides.md");
    std::fs::write(&path, "# Hei på deg\n").unwrap();
    assert_eq!(read_talk_text(&path).unwrap(), "# Hei på deg\n");
}

#[test]
fn a_talks_text_over_the_limit_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("slides.md");
    // Sparse, so the test costs no disk.
    std::fs::File::create(&path).unwrap().set_len(MAX_FILE_BYTES + 1).unwrap();
    assert!(read_talk_text(&path).unwrap_err().to_string().contains("limit"));
}

#[test]
fn a_talks_text_that_is_not_utf8_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("slides.md");
    std::fs::write(&path, [0xFF, 0xFE, 0x00]).unwrap();
    assert!(read_talk_text(&path).is_err());
}

#[cfg(unix)]
#[test]
fn a_named_pipe_is_refused_as_a_talks_text_without_waiting_for_a_writer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("slides.md");
    let made = std::process::Command::new("mkfifo").arg(&path).status().unwrap();
    assert!(made.success());
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || done.send(read_talk_text(&path).is_err()));
    let refused = finished.recv_timeout(std::time::Duration::from_secs(5)).expect("the read waited for a writer");
    assert!(refused);
}

#[test]
fn a_folder_reads_as_absent() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    assert_eq!(FolderFiles::new(dir.path().into()).read("assets"), None);
}

#[test]
fn a_pack_takes_entries_as_big_as_a_folder_reads() {
    assert_eq!(PackLimits::default().entry_bytes, MAX_FILE_BYTES);
}
