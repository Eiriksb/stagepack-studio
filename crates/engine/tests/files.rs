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

fn with_added(root: &std::path::Path) -> (WithAdded, std::sync::Arc<AddedSet>) {
    let under: std::sync::Arc<dyn TalkFiles + Send + Sync> = std::sync::Arc::new(FolderFiles::new(root.into()));
    let added = std::sync::Arc::new(AddedSet::new());
    (WithAdded::new(under, added.clone()), added)
}

#[test]
fn an_added_file_is_read_with_the_talks_own() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("assets/a.txt"), b"on disk").unwrap();
    let (files, added) = with_added(dir.path());
    assert_eq!(files.read("assets/c.txt"), None);
    assert_eq!(files.stamp("assets/c.txt"), None);

    let before = added.stamp();
    assert!(added.add("assets/c.txt", b"added".to_vec().into()));
    assert_ne!(added.stamp(), before);

    assert_eq!(files.read("assets/c.txt"), Some(b"added".to_vec()));
    assert_eq!(files.read("./assets/c.txt"), Some(b"added".to_vec()));
    assert_eq!(files.len("assets/c.txt"), Some(5));
    assert!(files.stamp("assets/c.txt").is_some());
    assert_eq!(files.read("assets/a.txt"), Some(b"on disk".to_vec()));
    assert_eq!(files.names_in("assets"), ["assets/a.txt", "assets/c.txt"]);
    assert_eq!(added.names_in("assets"), ["assets/c.txt"]);
    assert_eq!(added.get("assets/c.txt").as_deref(), Some(&b"added"[..]));
    // What can be added to changes.
    assert!(!files.never_changes());
}

#[test]
fn an_added_file_is_never_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let (files, added) = with_added(dir.path());
    assert!(added.add("assets/c.txt", b"first".to_vec().into()));
    let stamp = added.stamp();
    let file_stamp = files.stamp("assets/c.txt");
    assert!(!added.add("assets/c.txt", b"second".to_vec().into()));
    assert_eq!(added.stamp(), stamp);
    assert_eq!(files.read("assets/c.txt"), Some(b"first".to_vec()));
    assert_eq!(files.stamp("assets/c.txt"), file_stamp);
}

#[test]
fn a_released_file_is_read_from_the_talks_own() {
    let dir = tempfile::tempdir().unwrap();
    let (files, added) = with_added(dir.path());
    assert!(added.add("assets/c.txt", b"added".to_vec().into()));
    // A save writes it, and another program changes it after.
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("assets/c.txt"), b"changed on disk").unwrap();
    assert_eq!(files.read("assets/c.txt"), Some(b"added".to_vec()), "held, it's read from memory");
    let stamp = added.stamp();
    assert!(added.release(&["./assets/c.txt".to_string(), "assets/none.txt".to_string()]));
    assert_ne!(added.stamp(), stamp, "what reads it knows to read it again");
    assert_eq!(files.read("assets/c.txt"), Some(b"changed on disk".to_vec()));
    assert_eq!(files.len("assets/c.txt"), Some(15));
    assert_eq!(added.get("assets/c.txt"), None);
    assert!(added.names_in("assets").is_empty());
    let stamp = added.stamp();
    assert!(!added.release(&["assets/c.txt".to_string()]), "nothing more to let go of");
    assert_eq!(added.stamp(), stamp);
    // Its name can be given again.
    assert!(added.add("assets/c.txt", b"again".to_vec().into()));
}

#[test]
fn a_packs_files_keep_one_stamp_under_what_is_added() {
    let dir = tempfile::tempdir().unwrap();
    let talk = dir.path().join("talk");
    std::fs::create_dir_all(talk.join("assets")).unwrap();
    std::fs::write(talk.join("slides.md"), "# One\n").unwrap();
    std::fs::write(talk.join("assets/a.png"), b"a").unwrap();
    let out = dir.path().join("talk.sptx");
    pack_folder(&talk, &out, "0.1.0").unwrap();
    let under: std::sync::Arc<dyn TalkFiles + Send + Sync> = std::sync::Arc::new(open_packed(&out).unwrap());
    let files = WithAdded::new(under, std::sync::Arc::new(AddedSet::new()));
    let stamp = files.stamp("assets/a.png");
    assert!(stamp.is_some());
    assert_eq!(files.stamp("assets/a.png"), stamp);
    assert_eq!(files.stamp("assets/none.png"), None);
}

#[test]
fn the_engine_tells_a_picture_from_its_bytes() {
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(3, 2, image::Rgba([255, 0, 0, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    assert_eq!(picture_format(&png), Some(PictureFormat::Png));
    let mut bmp = Vec::new();
    image::RgbaImage::from_pixel(3, 2, image::Rgba([0, 0, 255, 255]))
        .write_to(&mut std::io::Cursor::new(&mut bmp), image::ImageFormat::Bmp)
        .unwrap();
    assert_eq!(picture_format(&bmp), Some(PictureFormat::Bmp));
    let again = picture_as_png(&bmp).unwrap();
    assert_eq!(picture_format(&again), Some(PictureFormat::Png));
    let decoded = image::load_from_memory(&again).unwrap().to_rgba8();
    assert_eq!((decoded.width(), decoded.height()), (3, 2));
    assert_eq!(decoded.get_pixel(1, 1).0, [0, 0, 255, 255]);
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4"/></svg>"#;
    assert_eq!(picture_format(svg), Some(PictureFormat::Svg));
    assert_eq!(picture_format(b"hello"), None);
    assert_eq!(picture_format(b""), None);
    assert_eq!(picture_format(&png[..png.len() / 2]), None);
}
