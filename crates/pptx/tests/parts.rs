//! The Rust parts must be byte-for-byte what the JavaScript exporter wrote, for the same inputs.
//! `tests/reference/README.md` says which call made each reference file.

use std::collections::HashSet;
use std::io::{Cursor, Read};

use slides_pptx::master::{self, Geometry, LayoutId};
use slides_pptx::package::{CT, Package, REL};
use slides_pptx::presentation::{self, EmbeddedFont, SlideRef};
use slides_pptx::theme;
use slides_pptx::xml::{self, ColorRef, Slot};
use toolkit::{Color, Theme};

fn zip_names(bytes: &[u8]) -> Vec<String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().unwrap().into_owned()).collect()
}

fn zip_text(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut text).unwrap();
    text
}

// ---- xml.rs ----

#[test]
fn units_become_emu() {
    assert_eq!(xml::emu_from_units(1920.0), 12192000);
    assert_eq!(xml::emu_from_units(1080.0), 6858000);
}

#[test]
fn escaping_covers_xml_and_drops_control_characters() {
    assert_eq!(xml::esc("a & <b> \"c\""), "a &amp; &lt;b&gt; &quot;c&quot;");
    assert_eq!(xml::esc("a\u{0B}b\u{FFFE}c\td"), "abc\td");
}

#[test]
fn escaping_keeps_astral_characters_and_escapes_apostrophes() {
    assert_eq!(xml::esc("it's \u{1F600}\r\n"), "it&apos;s \u{1F600}\r\n");
}

#[test]
fn colors_match_the_reference() {
    let blue = ColorRef::Rgb(Color::rgb(0x25, 0x63, 0xeb));
    assert_eq!(xml::color_xml(blue, None), include_str!("reference/color-rgb.xml"));
    assert_eq!(xml::color_xml(blue, Some(0.5)), include_str!("reference/color-rgb-alpha.xml"));
    assert_eq!(xml::color_xml(ColorRef::Slot(Slot::Dk1), None), include_str!("reference/color-slot-dk1.xml"));
    assert_eq!(
        xml::color_xml(ColorRef::Slot(Slot::Lt2), Some(0.16)),
        include_str!("reference/color-slot-lt2-alpha.xml")
    );
    assert_eq!(xml::color_xml(ColorRef::Slot(Slot::Accent3), None), include_str!("reference/color-slot-accent3.xml"));
    assert_eq!(
        xml::color_xml(ColorRef::Rgb(Color::rgb(0, 0, 0)), Some(1.0)),
        include_str!("reference/color-opaque-alpha.xml")
    );
}

#[test]
fn solid_fills_match_the_reference() {
    assert_eq!(
        xml::solid_fill(ColorRef::Rgb(Color::rgb(0x1e, 0x29, 0x3b)), Some(0.25)),
        include_str!("reference/solid-fill.xml")
    );
    assert_eq!(xml::solid_fill(ColorRef::Slot(Slot::Accent1), None), include_str!("reference/solid-fill-slot.xml"));
}

// ---- theme.rs ----

#[test]
fn theme_matches_the_reference() {
    assert_eq!(theme::theme_xml("Deck", &Theme::default()), include_str!("reference/theme.xml"));
    assert_eq!(theme::theme_xml("A & B <\"c\">", &Theme::default()), include_str!("reference/theme-escaped-name.xml"));
}

// ---- master.rs ----

fn geometry() -> Geometry {
    Geometry::new(13.333, 7.5, 0.6)
}

fn layout(key: &str) -> String {
    let defs = master::layout_defs(&geometry());
    master::layout_xml(defs.iter().find(|d| d.key == key).unwrap(), "en-US")
}

#[test]
fn only_the_title_only_and_blank_layouts_exist() {
    let keys: Vec<&str> = master::layout_defs(&geometry()).iter().map(|d| d.key).collect();
    assert_eq!(keys, ["titleOnly", "blank"]);
}

#[test]
fn layouts_match_the_reference() {
    assert_eq!(layout("titleOnly"), include_str!("reference/layout-title-only.xml"));
    assert_eq!(layout("blank"), include_str!("reference/layout-blank.xml"));
}

fn layout_ids() -> Vec<LayoutId> {
    vec![LayoutId { id: 2147483649, r_id: "rId2".into() }, LayoutId { id: 2147483650, r_id: "rId3".into() }]
}

#[test]
fn master_matches_the_reference() {
    assert_eq!(master::master_xml(&geometry(), "en-US", &layout_ids()), include_str!("reference/master.xml"));
    let exact = Geometry::new(1920.0 / 144.0, 1080.0 / 144.0, 0.6);
    assert_eq!(master::master_xml(&exact, "en-US", &layout_ids()), include_str!("reference/master-exact.xml"));
}

// ---- presentation.rs ----

fn presentation(
    notes: Option<&str>,
    slides: &[SlideRef],
    size: (f64, f64),
    fonts: &[EmbeddedFont],
) -> String {
    presentation::presentation_xml(&presentation::Presentation {
        master_id: 2147483648,
        master_r_id: "rId1",
        notes_master_r_id: notes,
        slides,
        width: size.0,
        height: size.1,
        embedded_fonts: fonts,
    })
}

fn slide(id: u32, r_id: &str) -> SlideRef {
    SlideRef { id, r_id: r_id.into() }
}

#[test]
fn presentations_match_the_reference() {
    assert_eq!(presentation(None, &[], (13.333, 7.5), &[]), include_str!("reference/presentation.xml"));
    assert_eq!(
        presentation(Some("rId2"), &[slide(256, "rId3"), slide(257, "rId4")], (13.333, 7.5), &[]),
        include_str!("reference/presentation-slides-notes.xml")
    );
    assert_eq!(
        presentation(None, &[slide(256, "rId2")], (1920.0 / 144.0, 1080.0 / 144.0), &[]),
        include_str!("reference/presentation-exact.xml")
    );
}

#[test]
fn slide_sizes_are_kept_inside_what_powerpoint_accepts() {
    assert_eq!(presentation(None, &[], (0.2, 100.0), &[]), include_str!("reference/presentation-clamped.xml"));
}

#[test]
fn embedded_fonts_match_the_reference() {
    let fonts = [
        EmbeddedFont {
            typeface: "Inter".into(),
            regular: Some("rId5".into()),
            bold: Some("rId6".into()),
            italic: Some("rId7".into()),
            bold_italic: Some("rId8".into()),
        },
        EmbeddedFont { typeface: "Odd & \"Font\"".into(), regular: Some("rId9".into()), ..Default::default() },
    ];
    assert_eq!(
        presentation(None, &[slide(256, "rId2")], (13.333, 7.5), &fonts),
        include_str!("reference/presentation-fonts.xml")
    );
}

#[test]
fn small_parts_match_the_reference() {
    assert_eq!(presentation::pres_props_xml(), include_str!("reference/pres-props.xml"));
    assert_eq!(presentation::view_props_xml(), include_str!("reference/view-props.xml"));
    assert_eq!(presentation::app_xml(12, 3), include_str!("reference/app.xml"));
    assert_eq!(presentation::core_xml("My talk", "Ada", None), include_str!("reference/core.xml"));
    assert_eq!(
        presentation::core_xml("My talk", "Ada", Some("2026-11-10T09:08:07Z")),
        include_str!("reference/core-dated.xml")
    );
    assert_eq!(presentation::core_xml("A & B <c>", "\"Q\" 'x'", None), include_str!("reference/core-escaped.xml"));
}

// ---- package.rs ----

const PRES: &str = "/ppt/presentation.xml";
const MASTER: &str = "/ppt/slideMasters/slideMaster1.xml";
const THEME: &str = "/ppt/theme/theme1.xml";
const SLIDE: &str = "/ppt/slides/slide1.xml";

fn reference_package() -> Package {
    let mut pkg = Package::default();
    pkg.rel("", PRES, REL::OFFICE_DOCUMENT, false);
    pkg.rel("", "/docProps/core.xml", REL::CORE_PROPS, false);
    pkg.add_xml(THEME, CT::THEME, "<t/>");
    pkg.add_xml(MASTER, CT::SLIDE_MASTER, "<m/>");
    pkg.rel(MASTER, THEME, REL::THEME, false);
    pkg.add_xml(SLIDE, CT::SLIDE, "<s/>");
    pkg.add_media("/ppt/media/image1.PNG", vec![1, 2, 3]);
    pkg.add_media("/ppt/media/image2.png", vec![4]);
    pkg.add_media("/ppt/fonts/font1.fntdata", vec![5]);
    let first = pkg.rel(SLIDE, "/ppt/media/image1.PNG", REL::IMAGE, false);
    let again = pkg.rel(SLIDE, "/ppt/media/image1.PNG", REL::IMAGE, false);
    assert_eq!(first, again, "an identical relationship reuses its id");
    pkg.rel(SLIDE, MASTER, REL::SLIDE_MASTER, false);
    pkg.rel(SLIDE, "https://example.com/?a=1&b=2", REL::HYPERLINK, true);
    pkg.rel(PRES, MASTER, REL::SLIDE_MASTER, false);
    pkg.add_xml(PRES, CT::PRESENTATION, "<p/>");
    pkg.add_xml("/docProps/core.xml", CT::CORE_PROPS, "<c/>");
    pkg
}

#[test]
fn a_package_is_a_zip_with_content_types_first() {
    let mut package = Package::default();
    package.add_xml("/ppt/presentation.xml", CT::PRESENTATION, "<a/>");
    let names = zip_names(&package.to_bytes().unwrap());
    assert_eq!(names[0], "[Content_Types].xml");
}

#[test]
fn a_package_matches_the_reference_entries_and_text() {
    let bytes = reference_package().to_bytes().unwrap();
    let expected: Vec<&str> = include_str!("reference/package-entries.txt").lines().collect();
    assert_eq!(zip_names(&bytes), expected);
    for (name, reference) in [
        ("[Content_Types].xml", include_str!("reference/package-content-types.xml")),
        ("_rels/.rels", include_str!("reference/package-root-rels.xml")),
        ("ppt/_rels/presentation.xml.rels", include_str!("reference/package-presentation-rels.xml")),
        ("ppt/slideMasters/_rels/slideMaster1.xml.rels", include_str!("reference/package-master-rels.xml")),
        ("ppt/slides/_rels/slide1.xml.rels", include_str!("reference/package-slide-rels.xml")),
    ] {
        assert_eq!(zip_text(&bytes, name), reference, "{name}");
    }
}

#[test]
fn the_same_calls_give_the_same_bytes() {
    assert_eq!(reference_package().to_bytes().unwrap(), reference_package().to_bytes().unwrap());
}

#[test]
fn every_entry_carries_the_fixed_dos_epoch_time() {
    let bytes = reference_package().to_bytes().unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    for i in 0..zip.len() {
        let time = zip.by_index(i).unwrap().last_modified().unwrap();
        assert_eq!((time.year(), time.month(), time.day(), time.hour()), (1980, 1, 1, 0));
    }
}

#[test]
fn media_is_stored_and_xml_is_deflated() {
    let bytes = reference_package().to_bytes().unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    assert_eq!(zip.by_name("ppt/media/image1.PNG").unwrap().compression(), zip::CompressionMethod::Stored);
    assert_eq!(zip.by_name("ppt/theme/theme1.xml").unwrap().compression(), zip::CompressionMethod::Deflated);
}

#[test]
fn bad_packages_are_errors_and_not_panics() {
    let broken = |build: &dyn Fn(&mut Package)| {
        let mut pkg = Package::default();
        build(&mut pkg);
        pkg.to_bytes().is_err()
    };
    assert!(broken(&|p| p.add_xml("no-slash.xml", CT::SLIDE, "<a/>")));
    assert!(broken(&|p| p.add_xml("/trailing/", CT::SLIDE, "<a/>")));
    assert!(broken(&|p| p.add_xml("/has space.xml", CT::SLIDE, "<a/>")));
    assert!(broken(&|p| {
        p.add_xml("/a.xml", CT::SLIDE, "<a/>");
        p.add_xml("/a.xml", CT::SLIDE, "<a/>");
    }));
    assert!(broken(&|p| p.add_media("/a.exe", vec![1])), "unsupported media type");
    assert!(broken(&|p| {
        p.rel("/missing.xml", "/also-missing.xml", REL::IMAGE, false);
    }), "relationships from a part that was never added");
    assert!(broken(&|p| {
        p.add_xml("/a.xml", CT::SLIDE, "<a/>");
        p.rel("/a.xml", "/nope.png", REL::IMAGE, false);
    }), "a relationship to a missing part");
}

// ---- every part is well-formed XML ----

fn assert_well_formed(name: &str, xml: &str) {
    let mut reader = quick_xml::Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(_) => {}
            Err(e) => panic!("{name} is not well-formed: {e}"),
        }
    }
}

#[test]
fn every_part_is_well_formed() {
    let mut seen = HashSet::new();
    for (name, xml) in [
        ("theme", theme::theme_xml("A & B", &Theme::default())),
        ("master", master::master_xml(&geometry(), "en-US", &layout_ids())),
        ("title only", layout("titleOnly")),
        ("blank", layout("blank")),
        ("presentation", presentation(Some("rId2"), &[slide(256, "rId3")], (13.333, 7.5), &[])),
        ("pres props", presentation::pres_props_xml()),
        ("view props", presentation::view_props_xml()),
        ("app", presentation::app_xml(1, 0)),
        ("core", presentation::core_xml("T & T", "A", Some("2026-01-01T00:00:00Z"))),
    ] {
        assert!(seen.insert(name));
        assert_well_formed(name, &xml);
    }
}
