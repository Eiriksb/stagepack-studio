use std::path::{Path, PathBuf};

use proptest::prelude::*;
use slides_engine::{FolderFiles, Fonts, TalkFiles as _};
use slides_format::{Change, SourceEdit, parse_talk};
use stagepack_studio::added_files::{FileKind, MAX_ADDED_BYTES, NotAdded, picture_bytes, safe_name, talk_uses};
use stagepack_studio::document::Document;

const ONE: &str = "---\ntitle: T\n---\n\n# One\n";

/// A talk folder holding `source`, in a folder of its own, so that anything written beside it shows.
fn talk_with(source: &str) -> (tempfile::TempDir, PathBuf, Document) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("talk");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("slides.md"), source).unwrap();
    let doc = Document::open(&folder, Fonts::bundled_only()).unwrap();
    (dir, folder, doc)
}

fn talk() -> (tempfile::TempDir, PathBuf, Document) {
    talk_with(ONE)
}

fn encoded(width: u32, height: u32, rgba: [u8; 4], format: image::ImageFormat) -> Vec<u8> {
    let mut bytes = Vec::new();
    let picture = image::DynamicImage::from(image::RgbaImage::from_pixel(width, height, image::Rgba(rgba)));
    // A JPEG has no alpha.
    let picture = if format == image::ImageFormat::Jpeg { image::DynamicImage::from(picture.to_rgb8()) } else { picture };
    picture.write_to(&mut std::io::Cursor::new(&mut bytes), format).unwrap();
    bytes
}

fn png(rgba: [u8; 4]) -> Vec<u8> {
    encoded(64, 64, rgba, image::ImageFormat::Png)
}

fn red_png() -> Vec<u8> {
    png([255, 0, 0, 255])
}

/// A PNG that says it's `width` by `height` and holds no pixels.
fn png_header(width: u32, height: u32) -> Vec<u8> {
    fn crc(bytes: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }
    let mut chunk = b"IHDR".to_vec();
    chunk.extend(width.to_be_bytes());
    chunk.extend(height.to_be_bytes());
    chunk.extend([8, 6, 0, 0, 0]);
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend(13u32.to_be_bytes());
    bytes.extend(&chunk);
    bytes.extend(crc(&chunk).to_be_bytes());
    bytes
}

/// Puts a picture from `path` on the end of the last slide.
fn show(doc: &mut Document, path: &str) {
    let end = doc.source().len();
    let change = Change::single(SourceEdit { range: end..end, text: format!("\n![]({path})\n") });
    doc.edit("Insert picture", change, 0).unwrap();
}

/// Whether the current slide draws with nothing to say and something red on it.
fn draws_red(doc: &mut Document) -> bool {
    let index = doc.current();
    let notices = doc.notices();
    assert!(notices.is_empty(), "{notices:?}");
    let thumbnail = doc.thumbnail(index, 480).expect("the slide draws");
    thumbnail.data().chunks(4).any(|pixel| pixel[0] > 200 && pixel[1] < 40 && pixel[2] < 40)
}

fn save(doc: &mut Document) {
    let saved = doc.save_job().run().unwrap();
    doc.saved(saved);
}

/// Every file under `root`, by its path from there.
fn names_under(root: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            if std::fs::symlink_metadata(&path).unwrap().is_dir() {
                walk(root, &path, out);
            } else {
                out.push(name);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

#[test]
fn the_reasons_are_sentences() {
    let said: Vec<String> = [NotAdded::TooLarge, NotAdded::NotAPicture, NotAdded::NotThatKind, NotAdded::Unreadable, NotAdded::NameTaken]
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        said,
        [
            "That file is larger than 32 MB, so it wasn't added.",
            "That file isn't a picture this app can show.",
            "That kind of file can't be used here.",
            "That file couldn't be read.",
            "This talk already has a different file with that name.",
        ]
    );
    assert_eq!(MAX_ADDED_BYTES, 32 * 1024 * 1024);
}

#[test]
fn each_kind_has_its_folder_and_extensions() {
    assert_eq!(FileKind::Picture.folder(), "assets");
    assert_eq!(FileKind::Data.folder(), "assets");
    assert_eq!(FileKind::ComponentSource.folder(), "components");
    assert_eq!(FileKind::ComponentBuild.folder(), "build");
    assert_eq!(FileKind::Picture.extensions(), ["png", "jpg", "jpeg", "webp", "svg"]);
    assert_eq!(FileKind::Data.extensions(), ["csv"]);
    assert_eq!(FileKind::ComponentSource.extensions(), ["rs"]);
    assert_eq!(FileKind::ComponentBuild.extensions(), ["wasm"]);
}

#[test]
fn a_file_that_cannot_be_used_adds_nothing() {
    let (dir, folder, mut doc) = talk();
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let text_png = outside.join("text.png");
    std::fs::write(&text_png, "just some words").unwrap();
    let huge = outside.join("huge.png");
    std::fs::File::create(&huge).unwrap().set_len(33 * 1024 * 1024).unwrap();
    let a_folder = outside.join("a folder");
    std::fs::create_dir(&a_folder).unwrap();
    let missing = outside.join("not there.png");
    let mut paths: Vec<(&str, PathBuf, NotAdded)> = vec![
        ("a folder", a_folder.clone(), NotAdded::Unreadable),
        ("a path that doesn't exist", missing.clone(), NotAdded::Unreadable),
        ("33 MiB", huge.clone(), NotAdded::TooLarge),
    ];
    #[cfg(unix)]
    {
        let dangling = outside.join("dangling.png");
        std::os::unix::fs::symlink(&missing, &dangling).unwrap();
        let to_folder = outside.join("to folder.png");
        std::os::unix::fs::symlink(&a_folder, &to_folder).unwrap();
        paths.push(("a dangling symlink", dangling, NotAdded::Unreadable));
        paths.push(("a symlink to a folder", to_folder, NotAdded::Unreadable));
    }
    for (what, path, refused) in &paths {
        assert_eq!(Document::read_outside_file(path).err(), Some(*refused), "{what}");
    }

    let text = Document::read_outside_file(&text_png).unwrap();
    let bytes: Vec<(&str, FileKind, &str, Vec<u8>, NotAdded)> = vec![
        ("text in a .png", FileKind::Picture, "text.png", text, NotAdded::NotAPicture),
        ("zero bytes", FileKind::Picture, "empty.png", Vec::new(), NotAdded::NotAPicture),
        ("a 20,000 × 20,000 PNG header", FileKind::Picture, "big.png", png_header(20_000, 20_000), NotAdded::NotAPicture),
        ("33 MiB", FileKind::Picture, "huge.png", vec![0; 33 * 1024 * 1024], NotAdded::TooLarge),
        ("a program as data", FileKind::Data, "run.exe", b"MZ".to_vec(), NotAdded::NotThatKind),
        ("data that isn't text", FileKind::Data, "sales.csv", vec![0xFF, 0xFE, 0], NotAdded::NotThatKind),
        ("a build that isn't one", FileKind::ComponentBuild, "score", b"not wasm".to_vec(), NotAdded::NotThatKind),
    ];
    for (what, kind, name, bytes, refused) in bytes {
        assert_eq!(doc.add_file(kind, name, bytes), Err(refused), "{what}");
    }

    assert_eq!(doc.source(), ONE);
    assert!(!doc.has_unsaved_changes());
    assert!(doc.added_files().is_empty());
    assert_eq!(names_under(&folder), ["slides.md"]);
    for name in ["assets/text.png", "assets/empty.png", "assets/big.png", "assets/huge.png", "assets/run.csv", "assets/sales.csv", "build/score.wasm"] {
        assert!(!doc.has_file(name), "{name}");
    }
}

#[test]
fn a_file_from_outside_is_read_whole() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.png");
    std::fs::write(&path, red_png()).unwrap();
    assert_eq!(Document::read_outside_file(&path).unwrap(), red_png());
    #[cfg(unix)]
    {
        let link = dir.path().join("link.png");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert_eq!(Document::read_outside_file(&link).unwrap(), red_png());
    }
}

/// Whether `path` is `assets/` and a name of 1 to 40 of `[a-z0-9-]`, a dot and an extension of letters.
fn is_plain_asset_path(path: &str) -> bool {
    let Some(name) = path.strip_prefix("assets/") else { return false };
    let Some((stem, ext)) = name.rsplit_once('.') else { return false };
    (1..=40).contains(&stem.len())
        && stem.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !ext.is_empty()
        && ext.bytes().all(|b| b.is_ascii_lowercase())
}

const DEVICE_NAMES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3",
    "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

proptest! {
    #[test]
    fn a_safe_name_is_always_plain(suggested in any::<String>()) {
        let name = safe_name(&suggested, "png");
        let path = format!("assets/{name}");
        prop_assert!(is_plain_asset_path(&path), "{suggested:?} gave {name:?}");
        let stem = name.strip_suffix(".png").unwrap();
        prop_assert!(!stem.starts_with('-') && !stem.ends_with('-') && !stem.contains("--"), "{name:?}");
        prop_assert!(!DEVICE_NAMES.contains(&stem), "{name:?}");
    }
}

#[test]
fn a_safe_name_follows_the_rules() {
    let cases = [
        ("Photo", "photo.png"),
        ("My Holiday Photo.JPG", "my-holiday-photo.png"),
        ("../../x", "x.png"),
        ("C:\\x", "c-x.png"),
        ("con", "con-file.png"),
        ("CON.png", "con-file.png"),
        ("lpt9", "lpt9-file.png"),
        ("com10", "com10.png"),
        ("", "file.png"),
        ("...", "file.png"),
        ("Ünïcödé", "n-c-d.png"),
        ("日本語", "file.png"),
        ("--a--b--", "a-b.png"),
    ];
    for (suggested, expected) in cases {
        assert_eq!(safe_name(suggested, "png"), expected, "{suggested:?}");
    }
    let long = safe_name(&"a".repeat(300), "png");
    assert_eq!(long, format!("{}.png", "a".repeat(40)));
    let cut_at_a_dash = safe_name(&format!("{}-b", "a".repeat(39)), "png");
    assert_eq!(cut_at_a_dash, format!("{}.png", "a".repeat(39)));
}

#[test]
fn no_name_leaves_the_talk() {
    let (dir, folder, mut doc) = talk();
    let names = ["../../x", "C:\\x", "con", &"n".repeat(300), "", "...", "日本語", "/etc/passwd", "a/b", "\\\\server\\share", ".hidden", "x.png.exe"];
    let mut placed = Vec::new();
    for (shade, name) in names.iter().enumerate() {
        let bytes = png([255, shade as u8, 0, 255]);
        let path = doc.add_file(FileKind::Picture, name, bytes.clone()).unwrap();
        assert!(is_plain_asset_path(&path), "{name:?} gave {path:?}");
        show(&mut doc, &path);
        placed.push((path, bytes));
    }
    save(&mut doc);

    let files = FolderFiles::new(folder.clone());
    for (path, bytes) in &placed {
        assert_eq!(files.read(path).as_ref(), Some(bytes), "{path}");
    }
    // Nothing was written anywhere but in the talk.
    let beside: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(beside, ["talk"]);
    let mut expected: Vec<String> = placed.iter().map(|(path, _)| path.clone()).collect();
    expected.push("slides.md".to_string());
    expected.sort();
    assert_eq!(names_under(&folder), expected);
}

#[test]
fn the_same_bytes_twice_are_one_file() {
    let (_dir, folder, mut doc) = talk();
    let first = doc.add_file(FileKind::Picture, "first", red_png()).unwrap();
    assert_eq!(first, "assets/first.png");
    assert_eq!(doc.add_file(FileKind::Picture, "second", red_png()).unwrap(), first);

    // A file the talk had already, saved.
    std::fs::create_dir(folder.join("assets")).unwrap();
    let blue = png([0, 0, 255, 255]);
    std::fs::write(folder.join("assets/Sky.PNG"), &blue).unwrap();
    assert_eq!(doc.add_file(FileKind::Picture, "sky again", blue).unwrap(), "assets/Sky.PNG");

    // A component's source, under its own name.
    let source = b"// a component".to_vec();
    let path = doc.add_file(FileKind::ComponentSource, "Score", source.clone()).unwrap();
    assert_eq!(path, "components/score.rs");
    assert_eq!(doc.add_file(FileKind::ComponentSource, "score.rs", source).unwrap(), path);
    let build = b"\0asm\x01\0\0\0".to_vec();
    assert_eq!(doc.add_file(FileKind::ComponentBuild, "Score", build.clone()).unwrap(), "build/score.wasm");
    assert_eq!(doc.add_file(FileKind::ComponentBuild, "score", build).unwrap(), "build/score.wasm");
    let data = b"a,b\n1,2\n".to_vec();
    assert_eq!(doc.add_file(FileKind::Data, "Sales.csv", data.clone()).unwrap(), "assets/sales.csv");
    assert_eq!(doc.add_file(FileKind::Data, "other", data).unwrap(), "assets/sales.csv");
}

#[test]
fn a_taken_name_gets_a_number() {
    let (_dir, folder, mut doc) = talk();
    std::fs::create_dir(folder.join("assets")).unwrap();
    std::fs::write(folder.join("assets/Pic.png"), png([0, 255, 0, 255])).unwrap();
    assert_eq!(doc.add_file(FileKind::Picture, "pic", png([0, 0, 255, 255])).unwrap(), "assets/pic-2.png");

    assert_eq!(doc.add_file(FileKind::Picture, "photo", png([1, 0, 0, 255])).unwrap(), "assets/photo.png");
    for n in 2..=99 {
        let path = doc.add_file(FileKind::Picture, "photo", png([n as u8, 0, 0, 255])).unwrap();
        assert_eq!(path, format!("assets/photo-{n}.png"));
    }
    assert_eq!(doc.add_file(FileKind::Picture, "photo", png([200, 0, 0, 255])), Err(NotAdded::NameTaken));

    // A long name still fits with its number.
    let long = "l".repeat(60);
    assert_eq!(doc.add_file(FileKind::Picture, &long, png([0, 1, 0, 255])).unwrap(), format!("assets/{}.png", "l".repeat(40)));
    assert_eq!(doc.add_file(FileKind::Picture, &long, png([0, 2, 0, 255])).unwrap(), format!("assets/{}-2.png", "l".repeat(38)));

    // A component is never renamed.
    doc.add_file(FileKind::ComponentSource, "score", b"// one".to_vec()).unwrap();
    assert_eq!(doc.add_file(FileKind::ComponentSource, "score", b"// two".to_vec()), Err(NotAdded::NameTaken));
    doc.add_file(FileKind::ComponentBuild, "score", b"\0asm\x01\0\0\0".to_vec()).unwrap();
    assert_eq!(doc.add_file(FileKind::ComponentBuild, "score", b"\0asm\x01\0\0\0 other".to_vec()), Err(NotAdded::NameTaken));
}

#[test]
fn a_bmp_becomes_a_png() {
    for format in [image::ImageFormat::Bmp, image::ImageFormat::Gif, image::ImageFormat::Tiff] {
        let bytes = encoded(5, 3, [0, 0, 255, 255], format);
        let (written, ext) = picture_bytes(bytes, Some("bmp")).unwrap();
        assert_eq!(ext, "png", "{format:?}");
        let decoded = image::load_from_memory_with_format(&written, image::ImageFormat::Png).unwrap().to_rgba8();
        assert_eq!((decoded.width(), decoded.height()), (5, 3), "{format:?}");
        assert_eq!(decoded.get_pixel(2, 1).0, [0, 0, 255, 255], "{format:?}");
    }
    // The content says what it is, not the name.
    let png = red_png();
    assert_eq!(picture_bytes(png.clone(), Some("jpg")).unwrap(), (png.clone(), "png"));
    let jpeg = encoded(8, 8, [255, 0, 0, 255], image::ImageFormat::Jpeg);
    assert_eq!(picture_bytes(jpeg.clone(), Some("png")).unwrap(), (jpeg.clone(), "jpg"));
    assert_eq!(picture_bytes(jpeg.clone(), Some("JPEG")).unwrap(), (jpeg, "jpeg"));
    let webp = encoded(8, 8, [255, 0, 0, 255], image::ImageFormat::WebP);
    assert_eq!(picture_bytes(webp.clone(), None).unwrap(), (webp, "webp"));

    let (_dir, _folder, mut doc) = talk();
    let bmp = encoded(64, 64, [255, 0, 0, 255], image::ImageFormat::Bmp);
    let path = doc.add_file(FileKind::Picture, "scan.bmp", bmp).unwrap();
    assert_eq!(path, "assets/scan.png");
    show(&mut doc, &path);
    assert!(draws_red(&mut doc));
}

const SVG_WITH_A_SCRIPT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="64" height="64">
<script>alert("hello")</script>
<image href="file:///etc/passwd" width="64" height="64"/>
<image xlink:href="https://example.invalid/x.png" width="64" height="64"/>
<rect width="64" height="64" fill="#ff0000" onclick="alert(1)"/>
</svg>
"##;

#[test]
fn an_svg_with_a_script_is_kept_and_draws_without_it() {
    let bytes = SVG_WITH_A_SCRIPT.as_bytes().to_vec();
    assert_eq!(picture_bytes(bytes.clone(), Some("svg")).unwrap(), (bytes.clone(), "svg"));
    let (_dir, folder, mut doc) = talk();
    let path = doc.add_file(FileKind::Picture, "drawing", bytes.clone()).unwrap();
    assert_eq!(path, "assets/drawing.svg");
    show(&mut doc, &path);
    assert!(draws_red(&mut doc));
    save(&mut doc);
    assert_eq!(std::fs::read(folder.join(&path)).unwrap(), bytes);
}

#[test]
fn the_engine_sees_an_added_file_at_once() {
    // Added, then shown.
    let (_dir, _folder, mut doc) = talk();
    let path = doc.add_file(FileKind::Picture, "photo", red_png()).unwrap();
    assert!(doc.has_file(&path));
    show(&mut doc, &path);
    assert!(draws_red(&mut doc));

    // Named on a slide before it was there.
    let (_dir, _folder, mut doc) = talk_with("---\ntitle: T\n---\n\n# One\n\n![](assets/late.png)\n");
    assert_eq!(doc.notices(), ["Can't show the picture assets/late.png"]);
    assert_eq!(doc.add_file(FileKind::Picture, "late", red_png()).unwrap(), "assets/late.png");
    assert!(draws_red(&mut doc));
    assert!(!doc.has_unsaved_changes(), "adding a file changes no text");
    assert_eq!(doc.added_files().len(), 1, "but it's used, so a save writes it");
}

#[test]
fn a_file_is_used_where_the_talk_names_it() {
    let talk = parse_talk(concat!(
        "# One\n\n![](assets/a.png)\n\n- <Picture src=\"assets/in-a-list.png\" />\n\n",
        "<Box x=\"10\">\n\n<Chart data=\"assets/sales.csv\" />\n\n</Box>\n\n---\n\n<Score value=\"3\" />\n\n",
        "Says assets/words.png in words, and `assets/code.png`.\n"
    ));
    for used in ["assets/a.png", "assets/in-a-list.png", "assets/sales.csv", "components/score.rs", "build/score.wasm"] {
        assert!(talk_uses(&talk, used), "{used}");
    }
    for unused in ["assets/words.png", "assets/code.png", "assets/a", "assets/A.png", "components/chart2.rs", "build/box2.wasm", "assets/score.rs"] {
        assert!(!talk_uses(&talk, unused), "{unused}");
    }
}

#[test]
fn only_the_files_the_text_uses_are_saved() {
    let (_dir, folder, mut doc) = talk();
    let used = doc.add_file(FileKind::Picture, "used", red_png()).unwrap();
    doc.add_file(FileKind::Picture, "unused", png([0, 0, 255, 255])).unwrap();
    show(&mut doc, &used);
    let pending: Vec<String> = doc.added_files().into_iter().map(|(path, _)| path).collect();
    assert_eq!(pending, std::slice::from_ref(&used));
    save(&mut doc);
    assert_eq!(names_under(&folder), ["assets/used.png", "slides.md"]);
    assert!(doc.added_files().is_empty(), "saved, so no longer pending");
}
