//! The add-on itself, built for WebAssembly and run the way the app runs it. These build it
//! first, which takes a while, so they're run by name:
//!
//! `scripts/cargo-safe.sh test -p slidev-addon --release --test wasm -- --ignored --nocapture`

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use addon_abi::{NoticeKind, MANIFEST};
use slides_components::addon::{Addon, ADDON_TIME_LIMIT};
use slides_format::MAX_SLIDE_BYTES;

/// How many levels a slide can nest and still be read: the format's own limit.
const MAX_NESTING: usize = 64;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The add-on, built as it's shipped, into a target folder of its own.
fn built_addon() -> Vec<u8> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(workspace())
        .args(["build", "-p", "slidev-addon", "--release", "--target", "wasm32-unknown-unknown", "--target-dir", "target/addon-check"])
        .status()
        .expect("cargo runs");
    assert!(status.success(), "the add-on builds for WebAssembly");
    let wasm = std::fs::read(workspace().join("target/addon-check/wasm32-unknown-unknown/release/slidev_addon.wasm")).expect("the add-on is built");
    println!("the add-on is {} bytes", wasm.len());
    wasm
}

fn file(name: &str, data: &[u8]) -> (String, Vec<u8>) {
    (name.to_string(), data.to_vec())
}

const ARGS: &str = r#"{"title":"A made-up talk"}"#;

/// A slide `body` as long as a slide can be and still be read, made of `unit` over and over.
fn as_long_as_allowed(unit: &str, around: impl Fn(usize) -> String) -> String {
    let mut levels = 1;
    while around(levels + 1).len() <= MAX_SLIDE_BYTES - unit.len() {
        levels += 1;
    }
    around(levels)
}

#[test]
#[ignore = "builds the add-on for WebAssembly"]
fn the_deepest_slides_the_format_reads_or_refuses_export() {
    let addon = Addon::load(&built_addon()).expect("the add-on loads");
    assert_eq!(addon.info().name, "slidev");
    assert_eq!(addon.info().version, "0.1.0");

    // As deep as the format reads: 64 boxes, one in another, and a list as deep.
    let boxes = format!("{}Deep\n\n{}", "<Box>\n\n".repeat(MAX_NESTING), "</Box>\n\n".repeat(MAX_NESTING));
    let list: String = (0..MAX_NESTING).map(|level| format!("{}- item {level}\n", "  ".repeat(level))).collect();
    // As deep as the size limit lets the Markdown reader go before the format refuses it.
    let emphasis = as_long_as_allowed("**", |n| format!("{}b{}\n", "*".repeat(n), "*".repeat(n)));
    let words = as_long_as_allowed("*a  a*", |n| format!("{}b{}\n", "*a ".repeat(n), " a*".repeat(n)));
    let tags = as_long_as_allowed("<Box>\n\n</Box>\n\n", |n| format!("{}Deep\n\n{}", "<Box>\n\n".repeat(n), "</Box>\n\n".repeat(n)));
    let strong = as_long_as_allowed("**a  a**", |n| format!("{}b{}\n", "**a ".repeat(n), " a**".repeat(n)));
    let underscores = as_long_as_allowed("_a  a_", |n| format!("{}b{}\n", "_a ".repeat(n), " a_".repeat(n)));
    // These are read, however long: a guest stack of 4 KiB isn't enough for them.
    let links = as_long_as_allowed("[](u)", |n| format!("{}a{}\n", "[".repeat(n), "](u)".repeat(n)));
    let pictures = as_long_as_allowed("![](u)", |n| format!("{}a{}\n", "![".repeat(n), "](u)".repeat(n)));
    let quotes = as_long_as_allowed("> > x\n", |n| (0..n).map(|line| format!("{}x\n", "> ".repeat(line % MAX_NESTING))).collect());
    for body in [&emphasis, &words, &tags, &strong, &underscores, &links, &pictures, &quotes] {
        assert!(body.len() <= MAX_SLIDE_BYTES && body.len() > MAX_SLIDE_BYTES - 200, "{}", body.len());
    }
    let source = [boxes, list, emphasis, words, tags, strong, underscores, links, pictures, quotes].join("\n---\n\n");

    let started = Instant::now();
    let (files, report) = addon.run("export", ARGS, vec![file("slides.md", source.as_bytes())]).expect("the export finishes");
    println!("the deepest slides took {:?}", started.elapsed());
    let (_, markdown) = files.iter().find(|(name, _)| name == "slides.md").expect("slides.md");
    let markdown = String::from_utf8(markdown.clone()).unwrap();
    assert!(markdown.contains("Deep"), "the 64 boxes are exported");
    assert!(markdown.contains("item 63"), "the 64-level list is exported");
    // Those past the format's nesting are left out, each with a notice.
    let unread: Vec<usize> = report.notices.iter().filter(|notice| notice.text.starts_with("This slide can't be read")).filter_map(|notice| notice.slide).collect();
    assert_eq!(unread, [3, 4, 5, 6, 7], "{report:?}");
    assert!(report.notices.iter().all(|notice| notice.kind == NoticeKind::LeftOut), "{report:?}");
}

#[test]
#[ignore = "builds the add-on for WebAssembly"]
fn the_fixture_exports_well_within_the_time_limit() {
    let addon = Addon::load(&built_addon()).expect("the add-on loads");
    let folder = workspace().join("fixtures/slidev-export");
    let files = vec![
        file("slides.md", &std::fs::read(folder.join("slides.md")).unwrap()),
        file("assets/photo.png", &std::fs::read(folder.join("assets/photo.png")).unwrap()),
    ];
    let started = Instant::now();
    let (files, _) = addon.run("export", ARGS, files).expect("the export finishes");
    let took = started.elapsed();
    println!("the fixture took {took:?}");
    assert!(files.iter().any(|(name, _)| name == "public/photo.png"));
    // The host lets the title for the manifest through, as one of the bundle's own names.
    let (_, manifest) = files.iter().find(|(name, _)| name == MANIFEST).expect("the export says its title");
    assert_eq!(manifest.as_slice(), br#"{"title":"A made-up talk"}"#);
    assert!(took < ADDON_TIME_LIMIT / 10, "{took:?} is too close to the limit of {ADDON_TIME_LIMIT:?}");

    // A talk as long as the fixture many times over is still well within it.
    let source = std::fs::read_to_string(folder.join("slides.md")).unwrap();
    let long = std::iter::repeat_n(source.as_str(), 20).collect::<Vec<_>>().join("\n---\n\n");
    let started = Instant::now();
    addon.run("export", ARGS, vec![file("slides.md", long.as_bytes())]).expect("the export finishes");
    let took = started.elapsed();
    println!("the fixture 20 times over, {} slides, took {took:?}", 20 * 10);
    assert!(took < Duration::from_secs(20), "{took:?}");
}

/// Every file of the import fixture, as the app hands them over: all but `stagepack.json`.
fn import_fixture() -> Vec<(String, Vec<u8>)> {
    let folder = workspace().join("fixtures/slidev-import");
    ["slides.md", "pages/extra.md", "public/photo.png"].iter().map(|name| file(name, &std::fs::read(folder.join(name)).unwrap())).collect()
}

#[test]
#[ignore = "builds the add-on for WebAssembly"]
fn the_fixture_imports_through_the_addon() {
    let addon = Addon::load(&built_addon()).expect("the add-on loads");
    let started = Instant::now();
    let (files, report) = addon.run("import", r#"{"entry":"slides.md"}"#, import_fixture()).expect("the import finishes");
    let took = started.elapsed();
    println!("the import of the fixture took {took:?}");
    let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["slides.md", "assets/photo.png"]);
    // As the native build imports it, to the byte.
    let expected = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/import/the_fixture.md")).unwrap();
    assert_eq!(String::from_utf8(files[0].1.clone()).unwrap(), expected);
    let talk = slides_format::parse_talk(&expected);
    assert!(talk.slides.iter().all(|slide| slide.error.is_none()));
    assert!(report.notices.iter().any(|notice| notice.kind == NoticeKind::Placeholder && notice.slide == Some(5)), "{report:?}");
    assert!(took < ADDON_TIME_LIMIT / 10, "{took:?} is too close to the limit of {ADDON_TIME_LIMIT:?}");

    // Odd input reaches the add-on whole and comes back as a talk, never as a stopped run.
    let odd = format!("\u{feff}---\r\nsrc: ./slides.md\r\n---\r\n```\r\n{}<!--\0", "<div v-click>".repeat(3000));
    let (files, _) = addon.run("import", "{}", vec![file("slides.md", odd.as_bytes())]).expect("odd input imports");
    let markdown = String::from_utf8(files[0].1.clone()).unwrap();
    assert!(slides_format::parse_talk(&markdown).slides.iter().all(|slide| slide.error.is_none()), "{markdown}");
}
