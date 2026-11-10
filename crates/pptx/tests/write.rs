//! A whole `.pptx` from a laid-out talk: what parts there are, how they point at one another,
//! and that PowerPoint's reader (here LibreOffice) opens it.

use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use quick_xml::Reader;
use quick_xml::events::Event;
use slides_engine::{Deck, FolderFiles, Fonts};
use slides_pptx::{PptxOutput, write_pptx};
use zip::ZipArchive;

fn showcase_folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/showcase")
}

fn export_from(folder: &Path, source: &str) -> PptxOutput {
    let mut deck = Deck::open(Box::new(FolderFiles::new(folder.to_path_buf())), Fonts::bundled_only(), source.to_string());
    write_pptx(&mut deck, &FolderFiles::new(folder.to_path_buf())).expect("the export succeeds")
}

fn showcase() -> PptxOutput {
    let source = std::fs::read_to_string(showcase_folder().join("slides.md")).unwrap();
    export_from(&showcase_folder(), &source)
}

fn export(source: &str) -> PptxOutput {
    let dir = tempfile::tempdir().unwrap();
    export_from(dir.path(), source)
}

fn archive(bytes: &[u8]) -> ZipArchive<Cursor<&[u8]>> {
    ZipArchive::new(Cursor::new(bytes)).expect("a zip")
}

fn names(bytes: &[u8]) -> Vec<String> {
    archive(bytes).file_names().filter_map(Result::ok).map(|n| n.into_owned()).collect()
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut text = String::new();
    archive(bytes).by_name(name).unwrap_or_else(|_| panic!("no part {name}")).read_to_string(&mut text).unwrap();
    text
}

fn has_part(bytes: &[u8], name: &str) -> bool {
    archive(bytes).by_name(name).is_ok()
}

#[test]
fn every_slide_has_a_part() {
    let out = showcase();
    for n in 1..=7 {
        assert!(has_part(&out.bytes, &format!("ppt/slides/slide{n}.xml")), "slide{n}");
    }
    assert!(!has_part(&out.bytes, "ppt/slides/slide8.xml"));
}

#[test]
fn content_types_cover_every_part() {
    let out = showcase();
    let types = part(&out.bytes, "[Content_Types].xml");
    for name in names(&out.bytes) {
        if name == "[Content_Types].xml" {
            continue;
        }
        let ext = name.rsplit('.').next().unwrap();
        let covered = types.contains(&format!("PartName=\"/{name}\"")) || types.contains(&format!("Extension=\"{ext}\""));
        assert!(covered, "{name} has no content type");
    }
}

/// `target` as a part name, resolved from the folder of the part whose `.rels` this is.
fn resolve(rels_name: &str, target: &str) -> String {
    let source_dir = match rels_name.rsplit_once("_rels/") {
        Some((dir, _)) => dir.trim_end_matches('/').to_string(),
        None => String::new(),
    };
    let mut parts: Vec<&str> = if target.starts_with('/') || source_dir.is_empty() {
        Vec::new()
    } else {
        source_dir.split('/').collect()
    };
    for piece in target.trim_start_matches('/').split('/') {
        if piece == ".." {
            parts.pop();
        } else {
            parts.push(piece);
        }
    }
    parts.join("/")
}

#[test]
fn relationships_point_at_parts_that_exist() {
    let out = showcase();
    let existing: HashSet<String> = names(&out.bytes).into_iter().collect();
    let mut checked = 0;
    for rels in existing.iter().filter(|n| n.ends_with(".rels")) {
        let xml = part(&out.bytes, rels);
        let mut reader = Reader::from_str(&xml);
        loop {
            match reader.read_event().unwrap() {
                Event::Empty(e) | Event::Start(e) if e.name().as_ref() == b"Relationship" => {
                    let attr = |key: &str| {
                        e.attributes().flatten().find(|a| a.key.as_ref() == key.as_bytes()).map(|a| String::from_utf8_lossy(&a.value).into_owned())
                    };
                    if attr("TargetMode").as_deref() == Some("External") {
                        continue;
                    }
                    let target = resolve(rels, &attr("Target").unwrap());
                    assert!(existing.contains(&target), "{rels} points at {target}");
                    checked += 1;
                }
                Event::Eof => break,
                _ => {}
            }
        }
    }
    assert!(checked > 20, "only {checked} relationships were checked");
}

#[test]
fn every_xml_part_is_well_formed() {
    let out = showcase();
    for name in names(&out.bytes).into_iter().filter(|n| n.ends_with(".xml") || n.ends_with(".rels")) {
        let xml = part(&out.bytes, &name);
        let mut reader = Reader::from_str(&xml);
        loop {
            match reader.read_event() {
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(e) => panic!("{name} is not well formed: {e}"),
            }
        }
    }
}

#[test]
fn shapes_are_native() {
    let xml = part(&showcase().bytes, "ppt/slides/slide6.xml");
    for expected in ["prst=\"ellipse\"", "<a:tbl>", "<p:pic>", "tailEnd type=\"triangle\""] {
        assert!(xml.contains(expected), "{expected}");
    }
}

#[test]
fn a_title_is_a_title_placeholder() {
    let out = showcase();
    let xml = part(&out.bytes, "ppt/slides/slide2.xml");
    assert!(xml.contains("type=\"title\""));
    assert!(xml.contains("The content layout"));
    assert!(part(&out.bytes, "ppt/slides/_rels/slide2.xml.rels").contains("slideLayout1.xml"), "titleOnly");
    // A slide with no title uses the blank layout.
    assert!(part(&out.bytes, "ppt/slides/_rels/slide5.xml.rels").contains("slideLayout2.xml"), "blank");
}

#[test]
fn click_steps_become_animations() {
    let out = export("<Box x=\"0\" y=\"0\" w=\"99\" h=\"99\" fill=\"red\" click=\"1\" enter=\"fade\">\n\nHi\n\n</Box>\n");
    let xml = part(&out.bytes, "ppt/slides/slide1.xml");
    assert!(xml.contains("presetID=\"10\""));
    assert!(xml.contains("animBg=\"1\""), "a filled box animates with its fill");
}

#[test]
fn a_transition_is_written() {
    let out = export("---\ntransition: fade\n---\n\n# A\n");
    assert!(part(&out.bytes, "ppt/slides/slide1.xml").contains("<p:fade/>"));
}

#[test]
fn notes_become_notes_slides() {
    let out = export("# A\n\n<Notes>\nSay hello.\n</Notes>\n");
    assert!(part(&out.bytes, "ppt/notesSlides/notesSlide1.xml").contains("Say hello."));
    assert!(has_part(&out.bytes, "ppt/notesMasters/notesMaster1.xml"));
}

#[test]
fn a_talk_without_notes_has_no_notes_master() {
    let out = export("# A\n");
    assert!(!has_part(&out.bytes, "ppt/notesMasters/notesMaster1.xml"));
}

fn photo_folder() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(showcase_folder().join("assets/photo.png"), dir.path().join("a.png")).unwrap();
    dir
}

#[test]
fn identical_pictures_are_stored_once() {
    let dir = photo_folder();
    let out = export_from(dir.path(), "# One\n\n![](a.png)\n\n---\n\n# Two\n\n![](a.png)\n");
    let media: Vec<String> = names(&out.bytes).into_iter().filter(|n| n.starts_with("ppt/media/")).collect();
    assert_eq!(media.len(), 1, "{media:?}");
    for n in 1..=2 {
        assert!(part(&out.bytes, &format!("ppt/slides/_rels/slide{n}.xml.rels")).contains(&media[0][4..]));
    }
}

#[test]
fn a_picture_file_is_stored_as_it_was_read() {
    let dir = photo_folder();
    let out = export_from(dir.path(), "# One\n\n![](a.png)\n");
    let mut stored = Vec::new();
    archive(&out.bytes).by_name("ppt/media/image1.png").unwrap().read_to_end(&mut stored).unwrap();
    assert_eq!(stored, std::fs::read(dir.path().join("a.png")).unwrap());
}

#[test]
fn links_become_hyperlinks() {
    let out = export("# A\n\n[one](https://example.com/x) and [two](https://example.com/x)\n");
    assert!(part(&out.bytes, "ppt/slides/slide1.xml").contains("hlinkClick"));
    let rels = part(&out.bytes, "ppt/slides/_rels/slide1.xml.rels");
    assert!(rels.contains("TargetMode=\"External\""));
    assert_eq!(rels.matches("https://example.com/x").count(), 1, "one relationship for one address");
}

fn attr_of(xml: &str, tag: &str, key: &str) -> i64 {
    let start = xml.find(&format!("<{tag}")).unwrap_or_else(|| panic!("no {tag}"));
    let rest = &xml[start..];
    let at = rest.find(&format!("{key}=\"")).unwrap() + key.len() + 2;
    rest[at..].split('"').next().unwrap().parse().unwrap()
}

/// The centre of the first shape that isn't the slide's own title, in EMU.
fn centre_of_box(xml: &str) -> (i64, i64) {
    let at = xml.find("<a:xfrm").unwrap();
    let rest = &xml[at..];
    let (x, y) = (attr_of(rest, "a:off", "x"), attr_of(rest, "a:off", "y"));
    let (cx, cy) = (attr_of(rest, "a:ext", "cx"), attr_of(rest, "a:ext", "cy"));
    (x + cx / 2, y + cy / 2)
}

#[test]
fn a_rotated_layer_turns_its_elements_around_its_centre() {
    let flat = export("<Box x=\"100\" y=\"100\" w=\"400\" h=\"200\" fill=\"red\">\n\nHi\n\n</Box>\n");
    let turned = export("<Box x=\"100\" y=\"100\" w=\"400\" h=\"200\" fill=\"red\" rotate=\"90\">\n\nHi\n\n</Box>\n");
    let flat = part(&flat.bytes, "ppt/slides/slide1.xml");
    let turned = part(&turned.bytes, "ppt/slides/slide1.xml");
    assert!(turned.contains("rot=\"5400000\""));
    assert!(!flat.contains("rot=\""));
    let (a, b) = (centre_of_box(&flat), centre_of_box(&turned));
    assert!((a.0 - b.0).abs() <= 1 && (a.1 - b.1).abs() <= 1, "{a:?} vs {b:?}");
}

#[test]
fn exporting_twice_gives_identical_bytes() {
    assert!(showcase().bytes == showcase().bytes);
}

#[test]
fn a_slide_with_an_error_still_exports() {
    let out = export("# One\n\n---\n\n<Box>\n\nnever closed\n\n---\n\n# Three\n");
    for n in 1..=3 {
        assert!(has_part(&out.bytes, &format!("ppt/slides/slide{n}.xml")), "slide{n}");
    }
    assert!(!has_part(&out.bytes, "ppt/slides/slide4.xml"));
}

#[test]
fn an_empty_talk_exports_one_slide() {
    let out = export("");
    assert!(has_part(&out.bytes, "ppt/slides/slide1.xml"));
    assert!(!has_part(&out.bytes, "ppt/slides/slide2.xml"));
}

#[test]
fn the_slide_is_sixteen_nine_in_exact_units() {
    let xml = part(&showcase().bytes, "ppt/presentation.xml");
    assert!(xml.contains("<p:sldSz cx=\"12192000\" cy=\"6858000\"/>"));
}

#[test]
fn the_title_of_the_talk_is_the_document_title() {
    let xml = part(&showcase().bytes, "docProps/core.xml");
    assert!(xml.contains("<dc:title>Showcase</dc:title>"));
}

fn tool_installed(name: &str, probe: &str) -> bool {
    let found = Command::new(name).arg(probe).output().is_ok();
    if !found {
        eprintln!("skipped: {name} is not installed");
    }
    found
}

#[test]
fn it_opens_in_libreoffice() {
    if !tool_installed("soffice", "--version") || !tool_installed("pdfinfo", "-v") || !tool_installed("pdftotext", "-v") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let pptx = dir.path().join("showcase.pptx");
    std::fs::write(&pptx, showcase().bytes).unwrap();
    let profile = dir.path().join("profile");
    let converted = Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "pdf", "--outdir"])
        .arg(dir.path())
        .arg(&pptx)
        .output()
        .expect("soffice runs");
    assert!(converted.status.success(), "{}", String::from_utf8_lossy(&converted.stderr));
    let pdf = dir.path().join("showcase.pdf");
    let info = Command::new("pdfinfo").arg(&pdf).output().unwrap();
    let info = String::from_utf8_lossy(&info.stdout);
    assert!(info.lines().any(|l| l.starts_with("Pages:") && l.trim_end().ends_with(" 7")), "{info}");
    let text = Command::new("pdftotext").arg(&pdf).arg("-").output().unwrap();
    assert!(String::from_utf8_lossy(&text.stdout).contains("Stagepack Showcase"));
}
