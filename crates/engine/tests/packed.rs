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
    // Opening it is still fine, so a Stagepack file is still recognised.
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
        ("assets/.hidden", b"x"),
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
