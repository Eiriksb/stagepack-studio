use slides_engine::*;
use std::io::Write;
use std::path::{Path, PathBuf};

const MANIFEST: &[u8] = br#"{"format":1,"app":"0.1.0","entry":"slides.md"}"#;

fn talk_folder() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    std::fs::write(p.join("slides.md"), "# One\n\n---\n\n# Two\n").unwrap();
    for (name, bytes) in [
        ("assets/a.png", &b"\x89PNG not really"[..]),
        ("components/c.rs", b"fn main() {}"),
        ("build/c.wasm", b"\0asm\x01\0\0\0"),
        ("notes.txt", b"private"),
        (".git/x", b"git"),
        ("target/y", b"target"),
    ] {
        let file = p.join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, bytes).unwrap();
    }
    dir
}

struct Zipped(#[allow(dead_code)] tempfile::TempDir, PathBuf);
impl std::ops::Deref for Zipped {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.1
    }
}

fn zip_with(entries: &[(&str, &[u8])]) -> Zipped {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.sptx");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    for (name, bytes) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    Zipped(dir, path)
}

fn pack_and_list(dir: &tempfile::TempDir) -> Vec<String> {
    let out = dir.path().join("talk.sptx");
    pack_folder(dir.path(), &out, "0.1.0").unwrap();
    open_packed(&out).unwrap().names()
}

#[test]
fn pack_then_open_gives_the_same_files() {
    let dir = talk_folder();
    let out = dir.path().join("talk.sptx");
    pack_folder(dir.path(), &out, "0.1.0").unwrap();
    let packed = open_packed(&out).unwrap();
    assert_eq!(
        packed.slides().unwrap(),
        std::fs::read_to_string(dir.path().join("slides.md")).unwrap()
    );
    assert_eq!(
        packed.read("assets/a.png"),
        Some(std::fs::read(dir.path().join("assets/a.png")).unwrap())
    );
    assert_eq!(packed.read("talk.json").unwrap(), MANIFEST);
}

#[test]
fn packing_leaves_out_everything_else() {
    let names = pack_and_list(&talk_folder());
    assert_eq!(
        names,
        [
            "assets/a.png",
            "build/c.wasm",
            "components/c.rs",
            "slides.md",
            "talk.json"
        ]
    );
}

#[test]
fn packing_twice_gives_identical_bytes() {
    let dir = talk_folder();
    let (a, b) = (dir.path().join("a.sptx"), dir.path().join("b.sptx"));
    pack_folder(dir.path(), &a, "0.1.0").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    pack_folder(dir.path(), &b, "0.1.0").unwrap();
    assert_eq!(std::fs::read(a).unwrap(), std::fs::read(b).unwrap());
}

#[test]
fn a_stagepack_file_is_recognised() {
    let path = zip_with(&[("stagepack.json", b"{}"), ("slides.md", b"# A")]);
    assert!(matches!(open_packed(&path), Err(PackError::SlidevTalk)));
}

#[test]
fn a_zip_without_a_manifest_is_refused() {
    assert!(matches!(
        open_packed(&zip_with(&[("slides.md", b"# A")])),
        Err(PackError::NotATalk)
    ));
}

#[test]
fn something_that_is_not_a_zip_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.sptx");
    std::fs::write(&path, "just some text, not a zip at all").unwrap();
    assert!(matches!(
        open_packed(&path),
        Err(PackError::Corrupt(_)) | Err(PackError::NotATalk)
    ));
    assert!(open_packed(&dir.path().join("missing.sptx")).is_err());
}

#[test]
fn a_newer_format_is_refused() {
    let path = zip_with(&[
        (
            "talk.json",
            br#"{"format":2,"app":"9","entry":"slides.md"}"#,
        ),
        ("slides.md", b"# A"),
    ]);
    assert!(matches!(
        open_packed(&path),
        Err(PackError::TooNew { format: 2 })
    ));
}

#[test]
fn a_bad_manifest_is_corrupt() {
    for manifest in [
        &b"not json"[..],
        br#"{"format":1,"app":"1","entry":"other.md"}"#,
        br#"{"format":1,"app":"1"}"#,
        br#"{"app":"1","entry":"slides.md"}"#,
    ] {
        let path = zip_with(&[("talk.json", manifest), ("slides.md", b"# A")]);
        assert!(
            matches!(open_packed(&path), Err(PackError::Corrupt(_))),
            "{}",
            String::from_utf8_lossy(manifest)
        );
    }
    let big = vec![b' '; 1024 * 1024 + 10];
    let path = zip_with(&[("talk.json", &big), ("slides.md", b"# A")]);
    assert!(matches!(open_packed(&path), Err(PackError::Corrupt(_))));
}

#[test]
fn unsafe_names_are_refused() {
    for name in ["../x", "/abs", "a\\b", "C:/x", "a/../b", "a//b", "./a"] {
        let path = zip_with(&[("talk.json", MANIFEST), ("slides.md", b"# A"), (name, b"x")]);
        assert!(
            matches!(open_packed(&path), Err(PackError::UnsafeName(_))),
            "{name}"
        );
    }
}

#[test]
fn directory_entries_are_allowed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.sptx");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    zip.add_directory("assets/", options).unwrap();
    zip.start_file("talk.json", options).unwrap();
    zip.write_all(MANIFEST).unwrap();
    zip.start_file("slides.md", options).unwrap();
    zip.write_all(b"# A").unwrap();
    zip.finish().unwrap();
    assert_eq!(
        open_packed(&path).unwrap().names(),
        ["slides.md", "talk.json"]
    );
}

#[test]
fn a_symlink_entry_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.sptx");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("talk.json", options).unwrap();
    zip.write_all(MANIFEST).unwrap();
    zip.start_file("slides.md", options).unwrap();
    zip.write_all(b"# A").unwrap();
    zip.add_symlink("assets/link", "/etc/passwd", options)
        .unwrap();
    zip.finish().unwrap();
    assert!(matches!(open_packed(&path), Err(PackError::UnsafeName(_))));
}

#[test]
fn oversized_entries_are_refused() {
    let path = zip_with(&[
        ("talk.json", MANIFEST),
        ("slides.md", b"# A"),
        ("assets/big", &vec![0u8; 4096]),
    ]);
    let limits = PackLimits {
        entry_bytes: 1024,
        ..Default::default()
    };
    assert!(matches!(
        open_packed_limited(&path, limits),
        Err(PackError::TooLarge(_))
    ));
    let limits = PackLimits {
        total_bytes: 2048,
        ..Default::default()
    };
    assert!(matches!(
        open_packed_limited(&path, limits),
        Err(PackError::TooLarge(_))
    ));
}

#[test]
fn too_many_entries_are_refused() {
    let path = zip_with(&[("talk.json", MANIFEST), ("slides.md", b"# A")]);
    let limits = PackLimits {
        entries: 1,
        ..Default::default()
    };
    assert!(matches!(
        open_packed_limited(&path, limits),
        Err(PackError::TooLarge(_))
    ));
}

#[test]
fn reading_outside_the_pack_gives_nothing() {
    let dir = talk_folder();
    let out = dir.path().join("talk.sptx");
    pack_folder(dir.path(), &out, "0.1.0").unwrap();
    let packed = open_packed(&out).unwrap();
    assert_eq!(packed.read("../slides.md"), None);
    assert_eq!(packed.read("missing"), None);
}

#[cfg(unix)]
#[test]
fn a_symlink_is_left_out_and_reported() {
    let dir = talk_folder();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret"), "s").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("secret"),
        dir.path().join("assets/link"),
    )
    .unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("components/dir")).unwrap();
    let out = dir.path().join("talk.sptx");
    let report = pack_folder(dir.path(), &out, "0.1.0").unwrap();
    assert_eq!(report.skipped, ["assets/link", "components/dir"]);
    assert_eq!(
        open_packed(&out).unwrap().names(),
        [
            "assets/a.png",
            "build/c.wasm",
            "components/c.rs",
            "slides.md",
            "talk.json"
        ]
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_assets_folder_is_not_followed() {
    let dir = talk_folder();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret"), "s").unwrap();
    std::fs::remove_dir_all(dir.path().join("assets")).unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("assets")).unwrap();
    let out = dir.path().join("talk.sptx");
    let report = pack_folder(dir.path(), &out, "0.1.0").unwrap();
    assert_eq!(report.skipped, ["assets"]);
    assert_eq!(
        open_packed(&out).unwrap().names(),
        ["build/c.wasm", "components/c.rs", "slides.md", "talk.json"]
    );
}

#[cfg(unix)]
#[test]
fn a_failed_pack_leaves_the_old_file_and_no_temp() {
    use std::os::unix::fs::PermissionsExt;
    let dir = talk_folder();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("talk.sptx");
    std::fs::write(&out, "old contents").unwrap();
    let png = dir.path().join("assets/a.png");
    std::fs::set_permissions(&png, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&png).is_ok() {
        return; // running as root: the file can't be made unreadable
    }
    let result = pack_folder(dir.path(), &out, "0.1.0");
    std::fs::set_permissions(&png, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(result.is_err());
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "old contents");
    let left: Vec<_> = std::fs::read_dir(out_dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, ["talk.sptx"]);
}

#[cfg(unix)]
#[test]
fn a_replaced_file_keeps_its_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("x.bin");
    std::fs::write(&out, "old").unwrap();
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o640)).unwrap();
    write_atomically(&out, |f| f.write_all(b"new")).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), b"new");
    assert_eq!(std::fs::metadata(&out).unwrap().permissions().mode() & 0o777, 0o640);
}

#[test]
fn write_atomically_cleans_up_after_a_failure() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("x.bin");
    let result = write_atomically(&out, |f| {
        f.write_all(b"half")?;
        Err(std::io::Error::other("nope"))
    });
    assert!(result.is_err());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    write_atomically(&out, |f| f.write_all(b"whole")).unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), b"whole");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                let name = p
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((name, std::fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

#[test]
fn unpack_then_pack_round_trips() {
    let dir = talk_folder();
    let first = dir.path().join("talk.sptx");
    pack_folder(dir.path(), &first, "0.1.0").unwrap();
    let work = tempfile::tempdir().unwrap();
    let folder = work.path().join("talk");
    unpack_to_folder(&first, &folder).unwrap();
    let second = work.path().join("again.sptx");
    pack_folder(&folder, &second, "0.1.0").unwrap();
    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap()
    );
    // Into an existing empty folder too.
    let empty = work.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    unpack_to_folder(&first, &empty).unwrap();
    assert_eq!(snapshot(&empty), snapshot(&folder));
}

#[test]
fn unpacking_refuses_a_folder_with_things_in_it() {
    let dir = talk_folder();
    let packed = dir.path().join("talk.sptx");
    pack_folder(dir.path(), &packed, "0.1.0").unwrap();
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("mine.txt"), "keep").unwrap();
    assert!(unpack_to_folder(&packed, work.path()).is_err());
    assert_eq!(
        snapshot(work.path()),
        [("mine.txt".to_string(), b"keep".to_vec())]
    );
}

// ---- Fix round 1 ----

/// Finds the local and central headers of `name` in a zip, as offsets of their signatures.
fn headers(bytes: &[u8], name: &str) -> (usize, usize) {
    let find = |sig: &[u8], name_at: usize, len_at: usize| {
        (0..bytes.len().saturating_sub(46))
            .find(|&i| {
                bytes[i..].starts_with(sig) && {
                    let n = u16::from_le_bytes([bytes[i + len_at], bytes[i + len_at + 1]]) as usize;
                    &bytes[i + name_at..i + name_at + n] == name.as_bytes()
                }
            })
            .unwrap()
    };
    (find(b"PK\x03\x04", 30, 26), find(b"PK\x01\x02", 46, 28))
}

fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// Rewrites the zip at `zip` after `change` has altered its bytes.
fn patched(zip: &Zipped, change: impl FnOnce(&mut Vec<u8>)) -> Zipped {
    let mut bytes = std::fs::read(&**zip).unwrap();
    change(&mut bytes);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.sptx");
    std::fs::write(&path, bytes).unwrap();
    Zipped(dir, path)
}

fn is_corrupt<T>(r: Result<T, PackError>) -> bool {
    matches!(r, Err(PackError::Corrupt(_)))
}

#[test]
fn damage_inside_the_file_is_corrupt_not_a_disk_error() {
    let padded = format!(
        r#"{{"format":1,"app":"{}","entry":"slides.md"}}"#,
        "x".repeat(200)
    );
    let base = zip_with(&[("talk.json", padded.as_bytes()), ("slides.md", b"# A")]);
    // The manifest says it is smaller than it is.
    let understated = patched(&base, |b| {
        let (local, central) = headers(b, "talk.json");
        put_u32(b, local + 22, 10);
        put_u32(b, central + 24, 10);
    });
    assert!(is_corrupt(open_packed(&understated)));
    // The manifest says it is 300 MiB.
    let huge = patched(&base, |b| {
        let (local, central) = headers(b, "talk.json");
        put_u32(b, local + 22, 300 << 20);
        put_u32(b, central + 24, 300 << 20);
    });
    assert!(is_corrupt(open_packed(&huge)));
    // The checksum is wrong.
    let bad_crc = patched(&base, |b| {
        let (local, central) = headers(b, "talk.json");
        put_u32(b, local + 14, 0x1234_5678);
        put_u32(b, central + 16, 0x1234_5678);
    });
    assert!(is_corrupt(open_packed(&bad_crc)));
    // The compressed data can't be decoded.
    let bad_deflate = patched(&base, |b| {
        let (local, _) = headers(b, "talk.json");
        let start = local + 30 + "talk.json".len();
        b[start..start + 4].fill(0xFF);
    });
    assert!(is_corrupt(open_packed(&bad_deflate)));
}

#[test]
fn damage_in_a_later_read_is_corrupt() {
    let text = "hello hello hello ".repeat(100);
    let base = zip_with(&[
        ("talk.json", MANIFEST),
        ("slides.md", b"# A"),
        ("assets/t.txt", text.as_bytes()),
    ]);
    let bad = patched(&base, |b| {
        let (local, _) = headers(b, "assets/t.txt");
        let start = local + 30 + "assets/t.txt".len();
        b[start..start + 4].fill(0xFF);
    });
    let work = tempfile::tempdir().unwrap();
    let result = unpack_to_folder(&bad, &work.path().join("out"));
    assert!(is_corrupt(result));
    assert!(!work.path().join("out").exists());
}

#[test]
fn a_stated_size_that_lies_is_corrupt() {
    let zeros = vec![0u8; 4 * 1024 * 1024];
    let base = zip_with(&[
        ("talk.json", MANIFEST),
        ("slides.md", b"# A"),
        ("assets/bomb", &zeros),
    ]);
    let lying = patched(&base, |b| {
        let (local, central) = headers(b, "assets/bomb");
        put_u32(b, local + 22, 10);
        put_u32(b, central + 24, 10);
    });
    let packed = open_packed(&lying).unwrap();
    assert_eq!(packed.read("assets/bomb"), None);
    let work = tempfile::tempdir().unwrap();
    assert!(is_corrupt(unpack_to_folder(
        &lying,
        &work.path().join("out")
    )));
}

#[test]
fn paths_are_read_the_way_a_folder_reads_them() {
    let dir = talk_folder();
    let out = dir.path().join("talk.sptx");
    pack_folder(dir.path(), &out, "0.1.0").unwrap();
    let packed = open_packed(&out).unwrap();
    let folder = FolderFiles::new(dir.path().to_path_buf());
    let want = std::fs::read(dir.path().join("assets/a.png")).unwrap();
    for form in [
        "./assets/a.png",
        "assets/./a.png",
        "assets//a.png",
        "assets/../assets/a.png",
    ] {
        assert_eq!(folder.read(form), Some(want.clone()), "folder {form}");
        assert_eq!(packed.read(form), Some(want.clone()), "packed {form}");
    }
    for form in ["../slides.md", "assets/../../slides.md", "", "."] {
        assert_eq!(packed.read(form), None, "{form}");
    }
}

#[test]
fn a_leading_slash_is_refused_in_both_forms() {
    let dir = talk_folder();
    let out = dir.path().join("talk.sptx");
    pack_folder(dir.path(), &out, "0.1.0").unwrap();
    let packed = open_packed(&out).unwrap();
    let folder = FolderFiles::new(dir.path().to_path_buf());
    for form in ["/assets/a.png", "//assets/a.png", "/slides.md"] {
        assert_eq!(folder.read(form), None, "folder {form}");
        assert_eq!(packed.read(form), None, "packed {form}");
    }
}

#[test]
fn unpacking_refuses_files_a_talk_has_no_business_with() {
    for name in [
        ".git/config",
        ".cargo/config.toml",
        "notes.txt",
        "assets",
        "other/a.png",
    ] {
        let path = zip_with(&[("talk.json", MANIFEST), ("slides.md", b"# A"), (name, b"x")]);
        let work = tempfile::tempdir().unwrap();
        let target = work.path().join("out");
        assert!(
            matches!(
                unpack_to_folder(&path, &target),
                Err(PackError::UnsafeName(_))
            ),
            "{name}"
        );
        assert!(!target.exists(), "{name}");
        // Into an existing empty folder it leaves the folder empty.
        std::fs::create_dir(&target).unwrap();
        assert!(unpack_to_folder(&path, &target).is_err());
        assert_eq!(std::fs::read_dir(&target).unwrap().count(), 0, "{name}");
    }
    // A Stagepack file is recognised whatever else it holds.
    let path = zip_with(&[("stagepack.json", b"{}"), (".git/config", b"x")]);
    assert!(matches!(open_packed(&path), Err(PackError::SlidevTalk)));
}

#[test]
fn duplicate_names_are_corrupt() {
    let base = zip_with(&[
        ("talk.json", MANIFEST),
        ("slides.md", b"# A"),
        ("slides.mx", b"# B"),
    ]);
    let twice = patched(&base, |b| {
        let mut i = 0;
        while i + 9 <= b.len() {
            if &b[i..i + 9] == b"slides.mx" {
                b[i..i + 9].copy_from_slice(b"slides.md");
            }
            i += 1;
        }
    });
    assert!(is_corrupt(open_packed(&twice)));
}

#[test]
fn names_that_windows_treats_specially_are_refused() {
    for name in [
        "assets/a.",
        "assets/a ",
        "assets/...",
        "assets/ ",
        "assets/CON",
        "assets/con.png",
        "assets/Nul.txt",
        "assets/COM1",
        "assets/lpt9.x",
        "assets/aux",
        "assets/PRN.",
        "assets/a<b",
        "assets/a>b",
        "assets/a\"b",
        "assets/a|b",
        "assets/a?b",
        "assets/a*b",
        "assets/a\tb",
        "assets/a\u{7f}b",
        "assets/.hidden",
    ] {
        let path = zip_with(&[("talk.json", MANIFEST), ("slides.md", b"# A"), (name, b"x")]);
        assert!(
            matches!(open_packed(&path), Err(PackError::UnsafeName(_))),
            "{name:?}"
        );
    }
    // Names that only look similar are fine.
    let path = zip_with(&[
        ("talk.json", MANIFEST),
        ("slides.md", b"# A"),
        ("assets/console.png", b"x"),
    ]);
    assert!(open_packed(&path).is_ok());
}

/// A folder holding the pictures `names` and a talk of `source`, and what packing it reports.
fn pack_talk_with_pictures(source: &str, names: &[&str]) -> PackReport {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), source).unwrap();
    for name in names {
        let file = dir.path().join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "a picture").unwrap();
    }
    let elsewhere = tempfile::tempdir().unwrap();
    pack_folder(dir.path(), &elsewhere.path().join("talk.sptx"), "0.1.0").unwrap()
}

#[test]
fn a_picture_outside_the_packed_folders_is_reported_by_name() {
    let source = "![](pics/a.jpg)\n\n<Picture src=\"pics/b.png\" x=\"0\" y=\"0\" w=\"99\" h=\"99\" />\n\n---\n\n![](assets/in.png)\n";
    let report = pack_talk_with_pictures(source, &["pics/a.jpg", "pics/b.png", "assets/in.png"]);
    assert_eq!(report.outside_pictures, ["pics/a.jpg", "pics/b.png"]);
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
}

#[test]
fn a_picture_is_found_wherever_the_talk_shows_it() {
    let source = "- item\n\n  ![](pics/list.jpg)\n\n<Box>\n\n<Picture src=\"pics/nested.jpg\" />\n\n![](pics/boxed.jpg)\n\n</Box>\n";
    let report = pack_talk_with_pictures(source, &["pics/list.jpg", "pics/nested.jpg", "pics/boxed.jpg"]);
    assert_eq!(report.outside_pictures, ["pics/boxed.jpg", "pics/list.jpg", "pics/nested.jpg"]);
}

#[test]
fn a_picture_shown_twice_is_reported_once() {
    let report = pack_talk_with_pictures("![](pics/a.jpg)\n\n---\n\n![](./pics/a.jpg)\n", &["pics/a.jpg"]);
    assert_eq!(report.outside_pictures, ["pics/a.jpg"]);
}

#[test]
fn only_a_picture_that_could_have_been_packed_is_reported() {
    let report = pack_talk_with_pictures("![](pics/missing.jpg)\n\n![](../beside.jpg)\n\n![](https://example.com/a.jpg)\n", &[]);
    assert!(report.outside_pictures.is_empty(), "{:?}", report.outside_pictures);
}

#[test]
fn a_hidden_folder_below_the_packed_folders_is_never_unpacked() {
    for name in ["assets/.git/config", "components/.cargo/config.toml", "assets/deep/.hidden", "build/.env"] {
        let path = zip_with(&[("talk.json", MANIFEST), ("slides.md", b"# A"), (name, b"x")]);
        let work = tempfile::tempdir().unwrap();
        let target = work.path().join("out");
        assert!(matches!(unpack_to_folder(&path, &target), Err(PackError::UnsafeName(_))), "{name}");
        assert!(!target.exists(), "{name}");
    }
}

#[test]
fn a_hidden_file_in_a_packed_folder_is_left_out_of_a_pack() {
    let dir = talk_folder();
    std::fs::write(dir.path().join("assets/.DS_Store"), "x").unwrap();
    std::fs::create_dir(dir.path().join("assets/.git")).unwrap();
    std::fs::write(dir.path().join("assets/.git/config"), "x").unwrap();
    let out = dir.path().join("talk.sptx");
    let report = pack_folder(dir.path(), &out, "0.1.0").unwrap();
    assert_eq!(report.skipped, ["assets/.DS_Store", "assets/.git"]);
    let names = open_packed(&out).unwrap().names();
    assert!(!names.iter().any(|name| name.contains("/.")), "{names:?}");
}

#[test]
fn an_invisible_character_cannot_hide_a_name() {
    for name in [
        "assets/\u{200b}.git/config",
        "assets/\u{feff}.git/config",
        "assets/a\u{ad}b.png",
        "assets/a\u{200c}b.png",
        "assets/a\u{200f}b.png",
        "assets/a\u{202a}b.png",
        "assets/a\u{202e}b.png",
        "assets/a\u{2060}b.png",
        "assets/a\u{206f}b.png",
    ] {
        let path = zip_with(&[("talk.json", MANIFEST), ("slides.md", b"# A"), (name, b"x")]);
        assert!(matches!(open_packed(&path), Err(PackError::UnsafeName(_))), "{name:?}");
        let work = tempfile::tempdir().unwrap();
        let target = work.path().join("out");
        assert!(matches!(unpack_to_folder(&path, &target), Err(PackError::UnsafeName(_))), "{name:?}");
        assert!(!target.exists(), "{name:?}");
    }
}

#[test]
fn a_character_next_to_the_invisible_ones_is_fine() {
    for name in ["assets/a\u{200a}b.png", "assets/a\u{2010}b.png", "assets/a\u{2029}b.png", "assets/a\u{202f}b.png", "assets/a\u{2070}b.png"] {
        let path = zip_with(&[("talk.json", MANIFEST), ("slides.md", b"# A"), (name, b"x")]);
        assert!(open_packed(&path).is_ok(), "{name:?}");
    }
}

#[test]
fn an_invisible_character_keeps_a_file_out_of_a_pack() {
    let dir = talk_folder();
    let hiding = dir.path().join("assets/\u{200b}.git");
    std::fs::create_dir(&hiding).unwrap();
    std::fs::write(hiding.join("config"), "x").unwrap();
    let out = dir.path().join("talk.sptx");
    let report = pack_folder(dir.path(), &out, "0.1.0").unwrap();
    assert_eq!(report.skipped, ["assets/\u{200b}.git"]);
    let names = open_packed(&out).unwrap().names();
    assert!(!names.iter().any(|name| name.contains(".git")), "{names:?}");
}

#[test]
fn a_stagepack_file_is_recognised_before_anything_in_it_is_judged() {
    let limits = PackLimits::default();
    let unsafe_name = zip_with(&[("stagepack.json", b"{}"), ("a:b", b"x")]);
    assert!(matches!(open_packed(&unsafe_name), Err(PackError::SlidevTalk)));
    let big = zip_with(&[("stagepack.json", b"{}"), ("big", &[0; 64])]);
    assert!(matches!(open_packed_limited(&big, PackLimits { entry_bytes: 8, ..limits }), Err(PackError::SlidevTalk)));
    assert!(matches!(open_packed_limited(&big, PackLimits { total_bytes: 8, ..limits }), Err(PackError::SlidevTalk)));
    assert!(matches!(open_packed_limited(&big, PackLimits { entries: 1, ..limits }), Err(PackError::SlidevTalk)));
}

#[test]
fn a_talk_of_ours_with_a_stagepack_file_in_it_is_ours() {
    let path = zip_with(&[("talk.json", MANIFEST), ("slides.md", b"# A"), ("stagepack.json", b"{}")]);
    assert!(open_packed(&path).is_ok());
}

#[cfg(unix)]
#[test]
fn packing_skips_names_a_pack_could_not_open() {
    let dir = talk_folder();
    std::fs::write(dir.path().join("assets/CON.png"), "x").unwrap();
    std::fs::write(dir.path().join("assets/what?.png"), "x").unwrap();
    let out = dir.path().join("talk.sptx");
    let report = pack_folder(dir.path(), &out, "0.1.0").unwrap();
    assert_eq!(report.skipped, ["assets/CON.png", "assets/what?.png"]);
    assert!(open_packed(&out).is_ok());
}

#[cfg(unix)]
#[test]
fn the_temp_file_never_follows_a_planted_link() {
    let dir = tempfile::tempdir().unwrap();
    let victim = dir.path().join("victim");
    std::fs::write(&victim, "precious").unwrap();
    let out = dir.path().join("x.bin");
    // The names the temp file used to have.
    std::os::unix::fs::symlink(
        &victim,
        dir.path()
            .join(format!(".x.bin.{}.tmp", std::process::id())),
    )
    .unwrap();
    write_atomically(&out, |f| f.write_all(b"new")).unwrap();
    assert_eq!(std::fs::read(&victim).unwrap(), b"precious");
    assert_eq!(std::fs::read(&out).unwrap(), b"new");
}

// ---- Saving a talk ----

/// Every file in the pack at `path` with what it holds, uncompressed, by name.
fn entries_of(path: &Path) -> Vec<(String, Vec<u8>)> {
    let packed = open_packed(path).unwrap();
    packed
        .names()
        .into_iter()
        .map(|name| {
            let bytes = packed.read(&name).unwrap();
            (name, bytes)
        })
        .collect()
}

#[test]
fn a_repacked_talk_has_the_new_slides_and_everything_else_as_it_was() {
    let dir = talk_folder();
    let packed = dir.path().join("talk.sptx");
    pack_folder(dir.path(), &packed, "0.1.0").unwrap();
    let out = dir.path().join("again.sptx");

    repack_with_slides(&packed, &out, "# New\r\n\r\n---\r\n\r\n# Slides\r\n", "0.2.0", &[]).unwrap();

    let reopened = open_packed(&out).unwrap();
    assert_eq!(reopened.slides().unwrap(), "# New\r\n\r\n---\r\n\r\n# Slides\r\n");
    let before = entries_of(&packed);
    let after = entries_of(&out);
    let others = |entries: &[(String, Vec<u8>)]| -> Vec<(String, Vec<u8>)> {
        entries
            .iter()
            .filter(|(name, _)| name != "slides.md" && name != "talk.json")
            .cloned()
            .collect()
    };
    assert_eq!(others(&after), others(&before));
    assert_eq!(after.len(), before.len());
    let manifest = reopened.read("talk.json").unwrap();
    assert_eq!(manifest, br#"{"format":1,"app":"0.2.0","entry":"slides.md"}"#);
}

#[test]
fn repacking_over_itself_is_atomic() {
    let dir = talk_folder();
    let packed = dir.path().join("talk.sptx");
    pack_folder(dir.path(), &packed, "0.1.0").unwrap();
    let before = entries_of(&packed);

    // A failure part way leaves the pack as it was, and nothing beside it.
    let too_big = "x".repeat(MAX_FILE_BYTES as usize + 1);
    assert!(repack_with_slides(&packed, &packed, &too_big, "0.1.0", &[]).is_err());
    assert_eq!(entries_of(&packed), before);

    repack_with_slides(&packed, &packed, "# Changed\n", "0.1.0", &[]).unwrap();
    assert_eq!(open_packed(&packed).unwrap().slides().unwrap(), "# Changed\n");
    let mut left: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tmp"))
        .collect();
    left.sort();
    assert!(left.is_empty(), "{left:?}");
}

#[test]
fn a_folder_is_copied_with_the_new_slides() {
    let dir = talk_folder();
    let work = tempfile::tempdir().unwrap();
    let to = work.path().join("copy");

    let left_out = copy_talk_folder(dir.path(), &to, "# Copied\r\n", &[]).unwrap().skipped;

    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(
        snapshot(&to),
        [
            ("assets/a.png".to_string(), b"\x89PNG not really".to_vec()),
            ("build/c.wasm".to_string(), b"\0asm\x01\0\0\0".to_vec()),
            ("components/c.rs".to_string(), b"fn main() {}".to_vec()),
            ("slides.md".to_string(), b"# Copied\r\n".to_vec()),
        ]
    );
    // The talk it came from is as it was.
    assert_eq!(
        std::fs::read_to_string(dir.path().join("slides.md")).unwrap(),
        "# One\n\n---\n\n# Two\n"
    );
    // Into an empty folder too, but never one with things in it.
    let empty = work.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    copy_talk_folder(dir.path(), &empty, "# Copied\r\n", &[]).unwrap();
    assert_eq!(snapshot(&empty), snapshot(&to));
    assert!(copy_talk_folder(dir.path(), &to, "# Again\n", &[]).is_err());
    assert_eq!(std::fs::read(to.join("slides.md")).unwrap(), b"# Copied\r\n");
}

#[cfg(unix)]
#[test]
fn a_copied_folder_leaves_out_what_a_pack_would() {
    let dir = talk_folder();
    std::os::unix::fs::symlink("/etc/passwd", dir.path().join("assets/link.png")).unwrap();
    let work = tempfile::tempdir().unwrap();
    let to = work.path().join("copy");
    let left_out = copy_talk_folder(dir.path(), &to, "# Copied\n", &[]).unwrap().skipped;
    assert_eq!(left_out, ["assets/link.png"]);
    assert!(std::fs::symlink_metadata(to.join("assets/link.png")).is_err());
}

#[test]
fn a_packed_talk_is_unpacked_with_the_new_slides() {
    let dir = talk_folder();
    let packed = dir.path().join("talk.sptx");
    pack_folder(dir.path(), &packed, "0.1.0").unwrap();
    let work = tempfile::tempdir().unwrap();
    let folder = work.path().join("talk");
    unpack_with_slides(&packed, &folder, "# Unpacked\n", &[]).unwrap();
    assert_eq!(std::fs::read_to_string(folder.join("slides.md")).unwrap(), "# Unpacked\n");
    assert_eq!(std::fs::read(folder.join("assets/a.png")).unwrap(), b"\x89PNG not really");
}

#[test]
fn a_folder_is_packed_with_the_new_slides() {
    let dir = talk_folder();
    let work = tempfile::tempdir().unwrap();
    let out = work.path().join("talk.sptx");
    pack_folder_with_slides(dir.path(), &out, "# Packed\n", "0.1.0", &[]).unwrap();
    assert_eq!(open_packed(&out).unwrap().slides().unwrap(), "# Packed\n");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("slides.md")).unwrap(),
        "# One\n\n---\n\n# Two\n"
    );
}

#[test]
fn a_folder_is_not_copied_into_itself() {
    let dir = talk_folder();
    let before = snapshot(dir.path());
    for inside in ["assets/copy", "copy", "assets/../copy"] {
        let to = dir.path().join(inside);
        assert!(matches!(copy_talk_folder(dir.path(), &to, "# Copy\n", &[]), Err(PackError::InsideTheTalk)), "{inside}");
        assert_eq!(snapshot(dir.path()), before, "{inside}");
    }
    assert!(lies_within(&dir.path().join("not/there/yet"), dir.path()));
    assert!(!lies_within(&dir.path().with_extension("copy"), dir.path()));
}

fn new_file(name: &str, bytes: &[u8]) -> (String, Vec<u8>) {
    (name.to_string(), bytes.to_vec())
}

#[test]
fn a_repack_writes_new_files_as_new_entries() {
    let dir = talk_folder();
    let packed = dir.path().join("talk.sptx");
    pack_folder(dir.path(), &packed, "0.1.0").unwrap();
    let before = entries_of(&packed);

    let extra = [new_file("assets/new.png", b"new picture"), new_file("assets/data.csv", b"a,b\n1,2\n")];
    repack_with_slides(&packed, &packed, "# Changed\n", "0.1.0", &extra).unwrap();

    let after = entries_of(&packed);
    assert_eq!(after.len(), before.len() + 2);
    let reopened = open_packed(&packed).unwrap();
    assert_eq!(reopened.read("assets/new.png").unwrap(), b"new picture");
    assert_eq!(reopened.read("assets/data.csv").unwrap(), b"a,b\n1,2\n");
    assert_eq!(reopened.read("assets/a.png").unwrap(), b"\x89PNG not really");
    assert_eq!(reopened.slides().unwrap(), "# Changed\n");
}

#[test]
fn a_repack_never_replaces_a_different_entry() {
    let dir = talk_folder();
    let packed = dir.path().join("talk.sptx");
    pack_folder(dir.path(), &packed, "0.1.0").unwrap();
    let before = entries_of(&packed);

    for clash in ["assets/a.png", "assets/A.PNG"] {
        let outcome = repack_with_slides(&packed, &packed, "# Changed\n", "0.1.0", &[new_file(clash, b"other bytes")]);
        assert!(matches!(outcome, Err(PackError::NameTaken(_))), "{clash}: {outcome:?}");
        assert_eq!(entries_of(&packed), before, "{clash}");
    }
    // Two new files under one name, with different bytes.
    let twice = [new_file("assets/b.png", b"one"), new_file("assets/b.png", b"two")];
    assert!(matches!(repack_with_slides(&packed, &packed, "# C\n", "0.1.0", &twice), Err(PackError::NameTaken(_))));
    assert_eq!(entries_of(&packed), before);

    // The same bytes under the same name are the entry that's there.
    repack_with_slides(&packed, &packed, "# Changed\n", "0.1.0", &[new_file("assets/a.png", b"\x89PNG not really")]).unwrap();
    assert_eq!(entries_of(&packed).len(), before.len());
}

#[test]
fn a_new_file_with_a_name_that_leaves_the_talk_is_refused() {
    let dir = talk_folder();
    let packed = dir.path().join("talk.sptx");
    pack_folder(dir.path(), &packed, "0.1.0").unwrap();
    let work = tempfile::tempdir().unwrap();
    for name in ["../x.png", "/x.png", "assets/../../x.png", "notes.txt", "slides.md", "talk.json", "assets/.hidden", "assets\\x.png", "assets/con.png", ""] {
        let extra = [new_file(name, b"x")];
        let outcome = repack_with_slides(&packed, &packed, "# C\n", "0.1.0", &extra);
        assert!(matches!(outcome, Err(PackError::UnsafeName(_))), "{name:?}: {outcome:?}");
        let to = work.path().join("copy");
        assert!(matches!(copy_talk_folder(dir.path(), &to, "# C\n", &extra), Err(PackError::UnsafeName(_))), "{name:?}");
        assert!(!to.exists(), "{name:?}");
    }
}

#[test]
fn every_other_way_of_saving_a_talk_carries_new_files() {
    let dir = talk_folder();
    let work = tempfile::tempdir().unwrap();
    let extra = [new_file("assets/new.png", b"new picture"), new_file("components/d.rs", b"// d")];

    let copy = work.path().join("copy");
    copy_talk_folder(dir.path(), &copy, "# Copied\n", &extra).unwrap();
    assert_eq!(std::fs::read(copy.join("assets/new.png")).unwrap(), b"new picture");
    assert_eq!(std::fs::read(copy.join("components/d.rs")).unwrap(), b"// d");
    assert_eq!(std::fs::read(copy.join("assets/a.png")).unwrap(), b"\x89PNG not really");

    let out = work.path().join("talk.sptx");
    pack_folder_with_slides(dir.path(), &out, "# Packed\n", "0.1.0", &extra).unwrap();
    let packed = open_packed(&out).unwrap();
    assert_eq!(packed.read("assets/new.png").unwrap(), b"new picture");
    assert_eq!(packed.read("components/d.rs").unwrap(), b"// d");

    let plain = work.path().join("plain.sptx");
    pack_folder(dir.path(), &plain, "0.1.0").unwrap();
    let unpacked = work.path().join("unpacked");
    unpack_with_slides(&plain, &unpacked, "# Unpacked\n", &extra).unwrap();
    assert_eq!(std::fs::read(unpacked.join("assets/new.png")).unwrap(), b"new picture");
    assert_eq!(std::fs::read_to_string(unpacked.join("slides.md")).unwrap(), "# Unpacked\n");

    // A file the talk has with other bytes is never written over, and nothing is left behind.
    let clash = [new_file("assets/a.png", b"other")];
    let to = work.path().join("clash");
    assert!(matches!(copy_talk_folder(dir.path(), &to, "# C\n", &clash), Err(PackError::NameTaken(_))));
    assert!(!to.exists());
    let to = work.path().join("clash.sptx");
    assert!(matches!(pack_folder_with_slides(dir.path(), &to, "# C\n", "0.1.0", &clash), Err(PackError::NameTaken(_))));
    assert!(!to.exists());
    let to = work.path().join("clash-unpacked");
    assert!(matches!(unpack_with_slides(&plain, &to, "# C\n", &clash), Err(PackError::NameTaken(_))));
    assert!(!to.exists());
}

#[test]
fn a_new_file_is_written_beside_and_never_over_another() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("a.png");
    write_new_atomically(&out, b"first").unwrap();
    assert_eq!(std::fs::read(&out).unwrap(), b"first");
    let again = write_new_atomically(&out, b"second").unwrap_err();
    assert_eq!(again.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read(&out).unwrap(), b"first");
    let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, ["a.png"], "no temporary file is left");
}

/// A build index entering `tag`, as the app merges one in.
fn an_index(tag: &str) -> Vec<u8> {
    format!(r#"{{"format":1,"components":[{{"tag":"{tag}","file":"c","source_hash":"{}"}}]}}"#, "a".repeat(64)).into_bytes()
}

#[test]
fn every_way_of_saving_takes_the_merged_build_index_in_place_of_the_talks_own() {
    let dir = talk_folder();
    std::fs::write(dir.path().join("build/index.json"), an_index("Old")).unwrap();
    let work = tempfile::tempdir().unwrap();
    let extra = [new_file("build/index.json", &an_index("Merged")), new_file("assets/new.png", b"new picture")];
    let one_index = |names: Vec<String>| names.iter().filter(|name| *name == "build/index.json").count() == 1;

    let packed = work.path().join("talk.sptx");
    pack_folder(dir.path(), &packed, "0.1.0").unwrap();
    repack_with_slides(&packed, &packed, "# C\n", "0.1.0", &extra).unwrap();
    let reopened = open_packed(&packed).unwrap();
    assert_eq!(reopened.read("build/index.json").unwrap(), an_index("Merged"), "pack in place");
    assert!(one_index(reopened.names()));

    let out = work.path().join("out.sptx");
    pack_folder_with_slides(dir.path(), &out, "# C\n", "0.1.0", &extra).unwrap();
    let reopened = open_packed(&out).unwrap();
    assert_eq!(reopened.read("build/index.json").unwrap(), an_index("Merged"), "folder to pack");
    assert!(one_index(reopened.names()));

    let copy = work.path().join("copy");
    copy_talk_folder(dir.path(), &copy, "# C\n", &extra).unwrap();
    assert_eq!(std::fs::read(copy.join("build/index.json")).unwrap(), an_index("Merged"), "folder to folder");

    let plain = work.path().join("plain.sptx");
    pack_folder(dir.path(), &plain, "0.1.0").unwrap();
    let unpacked = work.path().join("unpacked");
    unpack_with_slides(&plain, &unpacked, "# C\n", &extra).unwrap();
    assert_eq!(std::fs::read(unpacked.join("build/index.json")).unwrap(), an_index("Merged"), "pack to folder");
    assert_eq!(std::fs::read(dir.path().join("build/index.json")).unwrap(), an_index("Old"), "the talk itself isn't touched");
}

#[test]
fn only_the_build_index_is_taken_in_place_and_only_as_an_index() {
    let dir = talk_folder();
    let work = tempfile::tempdir().unwrap();
    let packed = work.path().join("talk.sptx");
    pack_folder(dir.path(), &packed, "0.1.0").unwrap();
    let before = entries_of(&packed);
    for (what, extra) in [
        ("not an index", vec![new_file("build/index.json", b"not json")]),
        ("too large", vec![new_file("build/index.json", &vec![b' '; 1024 * 1024 + 1])]),
        ("twice", vec![new_file("build/index.json", &an_index("A")), new_file("build/index.json", &an_index("B"))]),
    ] {
        let outcome = repack_with_slides(&packed, &packed, "# C\n", "0.1.0", &extra);
        assert!(matches!(outcome, Err(PackError::UnsafeName(_))), "{what}: {outcome:?}");
        assert_eq!(entries_of(&packed), before, "{what}");
    }
    // A file named like it but for case is any other file, and never written over.
    std::fs::write(dir.path().join("build/index.json"), an_index("Old")).unwrap();
    let out = work.path().join("cased.sptx");
    let cased = [new_file("build/Index.json", &an_index("New"))];
    assert!(matches!(pack_folder_with_slides(dir.path(), &out, "# C\n", "0.1.0", &cased), Err(PackError::NameTaken(_))));
    // And every other file keeps the rule.
    let other = [new_file("build/index.json", &an_index("New")), new_file("assets/a.png", b"other")];
    assert!(matches!(pack_folder_with_slides(dir.path(), &out, "# C\n", "0.1.0", &other), Err(PackError::NameTaken(_))));
    assert!(!out.exists());
}

#[test]
fn a_file_named_as_the_index_but_for_case_is_another_and_never_gets_a_second_beside_it() {
    let dir = talk_folder();
    std::fs::write(dir.path().join("build/Index.json"), an_index("Theirs")).unwrap();
    let work = tempfile::tempdir().unwrap();
    let extra = [new_file("build/index.json", &an_index("Merged"))];
    let out = work.path().join("out.sptx");
    assert!(matches!(pack_folder_with_slides(dir.path(), &out, "# C\n", "0.1.0", &extra), Err(PackError::NameTaken(_))));
    assert!(!out.exists(), "nothing is written");
    let copy = work.path().join("copy");
    assert!(matches!(copy_talk_folder(dir.path(), &copy, "# C\n", &extra), Err(PackError::NameTaken(_))));
    assert!(!copy.exists());
    let packed = work.path().join("talk.sptx");
    pack_folder(dir.path(), &packed, "0.1.0").unwrap();
    let before = entries_of(&packed);
    assert!(matches!(repack_with_slides(&packed, &packed, "# C\n", "0.1.0", &extra), Err(PackError::NameTaken(_))));
    assert_eq!(entries_of(&packed), before);
    let unpacked = work.path().join("unpacked");
    assert!(matches!(unpack_with_slides(&packed, &unpacked, "# C\n", &extra), Err(PackError::NameTaken(_))));
    assert!(!unpacked.exists());
}

#[test]
fn the_index_replaced_is_not_counted_against_a_packs_limits() {
    // As many files as a pack can hold, counting the talk's index and its manifest.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), "# A\n").unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::create_dir(dir.path().join("build")).unwrap();
    std::fs::write(dir.path().join("build/index.json"), an_index("Old")).unwrap();
    let limit = PackLimits::default().entries;
    for at in 0..limit - 3 {
        std::fs::write(dir.path().join(format!("assets/f{at}.csv")), "a").unwrap();
    }
    let work = tempfile::tempdir().unwrap();
    let out = work.path().join("full.sptx");
    let extra = [new_file("build/index.json", &an_index("Merged"))];
    pack_folder_with_slides(dir.path(), &out, "# A\n", "0.1.0", &extra).unwrap();
    assert_eq!(open_packed(&out).unwrap().read("build/index.json").unwrap(), an_index("Merged"));
    repack_with_slides(&out, &out, "# B\n", "0.1.0", &[new_file("build/index.json", &an_index("Again"))]).unwrap();
    assert_eq!(open_packed(&out).unwrap().read("build/index.json").unwrap(), an_index("Again"));
}

fn new_files() -> Vec<NewFile> {
    vec![
        ("assets/a.png".to_string(), b"\x89PNG not really".to_vec()),
        ("components/c.rs".to_string(), b"fn main() {}".to_vec()),
        ("build/c.wasm".to_string(), b"\0asm\x01\0\0\0".to_vec()),
    ]
}

fn names_in_folder(folder: &Path) -> Vec<String> {
    let mut names = Vec::new();
    fn walk(dir: &Path, prefix: &str, names: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            if entry.file_type().unwrap().is_dir() {
                walk(&entry.path(), &format!("{name}/"), names);
            } else {
                names.push(name);
            }
        }
    }
    walk(folder, "", &mut names);
    names.sort();
    names
}

#[test]
fn a_new_pack_opens_and_reads_back_every_file() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("new.sptx");
    pack_new(&out, "# One\n", "0.1.0", &new_files()).unwrap();
    let packed = open_packed(&out).unwrap();
    assert_eq!(packed.slides().unwrap(), "# One\n");
    for (name, bytes) in new_files() {
        assert_eq!(packed.read(&name), Some(bytes), "{name}");
    }
    assert_eq!(packed.read("talk.json"), Some(MANIFEST.to_vec()));
    assert_eq!(packed.names(), ["assets/a.png", "build/c.wasm", "components/c.rs", "slides.md", "talk.json"]);
    let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(left, ["new.sptx"]);
}

#[test]
fn a_new_folder_has_slides_and_every_file() {
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("talk");
    write_new_folder(&to, "# One\n", &new_files()).unwrap();
    assert_eq!(std::fs::read_to_string(to.join("slides.md")).unwrap(), "# One\n");
    for (name, bytes) in new_files() {
        assert_eq!(std::fs::read(to.join(&name)).unwrap(), bytes, "{name}");
    }
    assert_eq!(names_in_folder(&to), ["assets/a.png", "build/c.wasm", "components/c.rs", "slides.md"]);
    // An empty folder that's there already is used.
    let empty = dir.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    write_new_folder(&empty, "# One\n", &[]).unwrap();
    assert_eq!(names_in_folder(&empty), ["slides.md"]);
}

#[test]
fn a_new_folder_in_a_folder_that_is_not_empty_is_refused_and_nothing_is_written() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mine.txt"), "mine").unwrap();
    assert!(write_new_folder(dir.path(), "# One\n", &new_files()).is_err());
    assert_eq!(names_in_folder(dir.path()), ["mine.txt"]);
    assert_eq!(std::fs::read_to_string(dir.path().join("mine.txt")).unwrap(), "mine");
}

#[test]
fn a_new_folder_that_fails_half_way_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    // A file whose parent is a file can't be written, but is a name a talk could hold.
    let files = vec![("assets/a".to_string(), b"one".to_vec()), ("assets/a/b".to_string(), b"two".to_vec())];
    let to = dir.path().join("talk");
    assert!(write_new_folder(&to, "# One\n", &files).is_err());
    assert!(!to.exists());
    // And in a folder that was there, empty, it is left empty.
    let empty = dir.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    assert!(write_new_folder(&empty, "# One\n", &files).is_err());
    assert!(empty.is_dir());
    assert!(names_in_folder(&empty).is_empty());
}

#[test]
fn a_file_path_that_leaves_the_folder_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    for bad in ["../x", "assets/../../x", "/etc/x", "assets/../x", "C:\\x", "assets/a\0b", "slides.md", "notes.txt", "assets/.hidden"] {
        let files = vec![(bad.to_string(), b"x".to_vec())];
        let to = dir.path().join("talk");
        assert!(matches!(write_new_folder(&to, "# One\n", &files), Err(PackError::UnsafeName(_))), "{bad:?}");
        assert!(!to.exists(), "{bad:?}");
        let out = dir.path().join("t.sptx");
        assert!(matches!(pack_new(&out, "# One\n", "0.1.0", &files), Err(PackError::UnsafeName(_))), "{bad:?}");
        assert!(!out.exists(), "{bad:?}");
    }
    assert!(!dir.path().join("x").exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn a_new_pack_over_an_existing_file_replaces_it_whole() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("t.sptx");
    std::fs::write(&out, "old contents").unwrap();
    pack_new(&out, "# New\n", "0.1.0", &new_files()).unwrap();
    assert_eq!(open_packed(&out).unwrap().slides().unwrap(), "# New\n");
    // A write that fails leaves what was there, and no temporary file.
    let bad = vec![("../x".to_string(), b"x".to_vec())];
    assert!(pack_new(&out, "# Newer\n", "0.1.0", &bad).is_err());
    assert_eq!(open_packed(&out).unwrap().slides().unwrap(), "# New\n");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn a_new_pack_is_the_pack_of_a_folder_that_holds_the_same() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("talk");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("slides.md"), "ignored").unwrap();
    let mut files = new_files();
    files.push(("build/index.json".to_string(), an_index("New")));
    let (a, b) = (dir.path().join("a.sptx"), dir.path().join("b.sptx"));
    pack_folder_with_slides(&folder, &a, "# One\n", "0.1.0", &files).unwrap();
    pack_new(&b, "# One\n", "0.1.0", &files).unwrap();
    let (a, b) = (open_packed(&a).unwrap(), open_packed(&b).unwrap());
    assert_eq!(a.names(), b.names());
    for name in a.names() {
        assert_eq!(a.read(&name), b.read(&name), "{name}");
    }
}

mod alike {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]
        #[test]
        fn a_pack_and_a_folder_of_the_same_talk_read_alike(
            text in ".{0,200}",
            files in proptest::collection::vec(
                (prop_oneof![Just("assets"), Just("components"), Just("build")], "[a-z]{1,8}", proptest::collection::vec(any::<u8>(), 0..64)),
                0..=8,
            ),
        ) {
            // Names that only differ by case, or repeat with other bytes, aren't one talk's.
            let mut given: Vec<NewFile> = Vec::new();
            for (folder, name, bytes) in files {
                let path = format!("{folder}/{name}.bin");
                if !given.iter().any(|(other, _)| other.eq_ignore_ascii_case(&path)) {
                    given.push((path, bytes));
                }
            }
            let dir = tempfile::tempdir().unwrap();
            let (pack, folder) = (dir.path().join("t.sptx"), dir.path().join("talk"));
            pack_new(&pack, &text, "0.1.0", &given).unwrap();
            write_new_folder(&folder, &text, &given).unwrap();
            let packed = open_packed(&pack).unwrap();
            let on_disk = FolderFiles::new(folder.clone());
            prop_assert_eq!(packed.slides().unwrap(), text.clone());
            prop_assert_eq!(on_disk.read("slides.md"), Some(text.into_bytes()));
            for (name, bytes) in &given {
                prop_assert_eq!(packed.read(name), Some(bytes.clone()));
                prop_assert_eq!(on_disk.read(name), Some(bytes.clone()));
            }
            prop_assert_eq!(names_in_folder(&folder).len(), given.len() + 1);
            prop_assert_eq!(packed.names().len(), given.len() + 2);
        }
    }
}
