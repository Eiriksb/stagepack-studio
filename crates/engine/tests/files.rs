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
