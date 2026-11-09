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
    assert_eq!(files.read(dir.path().join("secret.txt").to_str().unwrap()), None);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.path().join("secret.txt"), talk.join("link.txt")).unwrap();
        assert_eq!(files.read("link.txt"), None);
    }
}
