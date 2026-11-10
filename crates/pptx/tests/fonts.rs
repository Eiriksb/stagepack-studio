//! Fonts carried in the file: the Embedded OpenType form PowerPoint reads, and which families
//! of a talk are embedded.

use std::io::{Cursor, Read};

use sha2::{Digest, Sha256};
use slides_engine::{Deck, FolderFiles, Fonts};
use slides_pptx::eot::{embeddable, eot_from_ttf, font_info};
use slides_pptx::{PptxOutput, write_pptx};
use zip::ZipArchive;

const INTER: &[u8] = include_bytes!("../../engine/assets/fonts/Inter-Regular.ttf");

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn export(source: &str) -> PptxOutput {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().to_path_buf();
    let mut deck = Deck::open(Box::new(FolderFiles::new(folder.clone())), Fonts::bundled_only(), source.to_string());
    write_pptx(&mut deck, &FolderFiles::new(folder)).expect("the export succeeds")
}

fn zip_names(bytes: &[u8]) -> Vec<String> {
    let archive = ZipArchive::new(Cursor::new(bytes)).expect("a zip");
    archive.file_names().filter_map(Result::ok).map(|n| n.into_owned()).collect()
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut text = String::new();
    let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("a zip");
    archive.by_name(name).unwrap_or_else(|_| panic!("no part {name}")).read_to_string(&mut text).unwrap();
    text
}

#[test]
fn eot_matches_the_original_writer() {
    let eot = eot_from_ttf(INTER).unwrap();
    assert_eq!(hex(&Sha256::digest(&eot)), "c3f45ed38ca197bf068dd75830de64a2036adf3f2512be8bd0eb38658efb0a85");
}

#[test]
fn font_info_reads_the_family() {
    let info = font_info(include_bytes!("../../engine/assets/fonts/Inter-Bold.ttf")).unwrap();
    assert_eq!(info.family, "Inter");
    assert!(info.weight >= 600);
}

#[test]
fn embeddable_follows_fs_type() {
    assert!(embeddable(0));
    assert!(!embeddable(0x0002));
}

#[test]
fn short_and_corrupt_fonts_are_errors_not_panics() {
    // Every truncation of the start of a real font, which cuts the directory and the tables.
    for len in (0..4096).chain([20_000, INTER.len() / 2, INTER.len() - 1]) {
        let cut = &INTER[..len];
        assert!(font_info(cut).is_err() || len > 4096, "font_info at {len}");
        let _ = eot_from_ttf(cut);
    }
    // The directory alone: every table offset points past the end.
    let tables = u16::from_be_bytes([INTER[4], INTER[5]]) as usize;
    let directory = &INTER[..12 + 16 * tables];
    assert!(font_info(directory).is_err());
    assert!(eot_from_ttf(directory).is_err());
    // Tables whose offsets are as large as a u32 goes.
    let mut far = INTER.to_vec();
    for i in 0..tables {
        far[12 + 16 * i + 8..12 + 16 * i + 12].copy_from_slice(&u32::MAX.to_be_bytes());
    }
    assert!(font_info(&far).is_err());
    assert!(eot_from_ttf(&far).is_err());
    // A name table that claims more records and string room than it has.
    let mut name_at = None;
    for i in 0..tables {
        let entry = 12 + 16 * i;
        if &INTER[entry..entry + 4] == b"name" {
            name_at = Some(u32::from_be_bytes(INTER[entry + 8..entry + 12].try_into().unwrap()) as usize);
        }
    }
    let name_at = name_at.unwrap();
    let mut wild = INTER.to_vec();
    wild[name_at + 2..name_at + 4].copy_from_slice(&u16::MAX.to_be_bytes());
    wild[name_at + 4..name_at + 6].copy_from_slice(&u16::MAX.to_be_bytes());
    let _ = font_info(&wild);
    let _ = eot_from_ttf(&wild);
    // Garbage, with and without the TrueType signature.
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    for round in 0..200 {
        let mut junk: Vec<u8> = (0..(round * 7 % 900)).map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 32) as u8
        }).collect();
        if round % 2 == 0 && junk.len() >= 4 {
            junk[..4].copy_from_slice(&[0, 1, 0, 0]);
        }
        let _ = font_info(&junk);
        let _ = eot_from_ttf(&junk);
    }
    // A font file with CFF outlines is refused, not read.
    assert!(font_info(b"OTTO\0\0\0\0\0\0\0\0").is_err());
}

#[test]
fn used_families_are_embedded_and_listed() {
    let out = export("# A\n\n`code`\n");
    let names = zip_names(&out.bytes);
    assert!(names.iter().any(|n| n.starts_with("ppt/fonts/") && n.ends_with(".fntdata")));
    let xml = part(&out.bytes, "ppt/presentation.xml");
    assert!(xml.contains("<p:embeddedFont><p:font typeface=\"Inter\"") && xml.contains("typeface=\"JetBrains Mono\""));
}

#[test]
fn each_face_is_embedded_once() {
    let out = export("# A\n\n`code`\n");
    let fonts: Vec<_> = zip_names(&out.bytes).into_iter().filter(|n| n.starts_with("ppt/fonts/") && n.ends_with(".fntdata")).collect();
    // Inter in four styles and JetBrains Mono in three (it has no bold italic).
    assert_eq!(fonts.len(), 7, "{fonts:?}");
}

#[test]
fn unused_families_are_not_embedded() {
    let xml = part(&export("# A\n").bytes, "ppt/presentation.xml");
    assert!(!xml.contains("JetBrains Mono"));
}

#[test]
fn a_family_that_cannot_be_embedded_says_so() {
    let out = export("---\nfont: Not A Real Font\n---\n\n# A\n");
    assert!(
        out.notices.iter().any(|n| n == "Not A Real Font can't be embedded, so PowerPoint may show another font"),
        "{:?}",
        out.notices
    );
}
