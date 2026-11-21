use std::path::{Path, PathBuf};
use std::process::Command;

use slides_engine::{compose, Deck, FolderFiles, Fonts};
use tiny_skia::Pixmap;

/// Runs a poppler tool, or returns `None` (and says so) when it isn't installed.
fn tool(name: &str) -> Option<&str> {
    if Command::new(name).arg("-v").output().is_ok() {
        Some(name)
    } else {
        eprintln!("skipped: {name} is not installed");
        None
    }
}

/// Runs `name` with `args`, returning stdout and stderr together, or `None` if skipped.
fn run(name: &str, args: &[&str]) -> Option<String> {
    let name = tool(name)?;
    let out = Command::new(name).args(args).output().expect("tool runs");
    assert!(out.status.success(), "{name} failed: {}", String::from_utf8_lossy(&out.stderr));
    Some(format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
}

fn showcase_folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/showcase")
}

fn open_showcase() -> Deck {
    let source = std::fs::read_to_string(showcase_folder().join("slides.md")).unwrap();
    Deck::open(Box::new(FolderFiles::new(showcase_folder())), Fonts::bundled_only(), source)
}

fn open_source(source: &str) -> Deck {
    let dir = tempfile::tempdir().unwrap();
    Deck::open(Box::new(FolderFiles::new(dir.path().to_path_buf())), Fonts::bundled_only(), source.to_string())
}

/// Writes the deck's PDF into `dir` and returns its path.
fn write_to(deck: &mut Deck, dir: &Path, name: &str) -> String {
    let bytes = slides_pdf::write_pdf(deck).expect("the export succeeds");
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path.to_string_lossy().into_owned()
}

fn showcase_pdf() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_to(&mut open_showcase(), dir.path(), "showcase.pdf");
    (dir, path)
}

/// The mean absolute difference per channel between the PDF's page and the engine's own
/// drawing of the slide, both at 1920 by 1080 over white.
fn page_difference(deck: &mut Deck, pdf: &str, page: usize, dir: &Path) -> Option<f64> {
    let stem = dir.join(format!("page{page}"));
    let number = page.to_string();
    run("pdftoppm", &["-png", "-r", "144", "-scale-to-x", "1920", "-scale-to-y", "1080", "-f", &number, "-l", &number, "-singlefile", pdf, &stem.to_string_lossy()])?;
    let rendered = Pixmap::load_png(dir.join(format!("page{page}.png"))).unwrap();
    let mut ours = Pixmap::new(1920, 1080).unwrap();
    ours.fill(tiny_skia::Color::WHITE);
    for layer in deck.layers(page - 1, 1.0) {
        compose(&mut ours, std::slice::from_ref(&*layer));
    }
    assert_eq!((rendered.width(), rendered.height()), (1920, 1080));
    let total: u64 = rendered
        .data()
        .iter()
        .zip(ours.data())
        .map(|(a, b)| u64::from(a.abs_diff(*b)))
        .sum();
    Some(total as f64 / rendered.data().len() as f64)
}

#[test]
fn one_page_per_slide_at_sixteen_by_nine() {
    let (_dir, pdf) = showcase_pdf();
    let Some(info) = run("pdfinfo", &[&pdf]) else { return };
    assert!(info.contains("Pages:           7"), "{info}");
    assert!(info.contains("960 x 540 pts"), "{info}");
}

#[test]
fn text_is_selectable() {
    let (_dir, pdf) = showcase_pdf();
    let Some(text) = run("pdftotext", &[&pdf, "-"]) else { return };
    assert!(text.contains("Stagepack Showcase") && text.contains("Two columns"), "{text}");
}

#[test]
fn an_empty_line_exports_the_same_in_either_spelling() {
    let pdf = |line: &str| slides_pdf::write_pdf(&mut open_source(&format!("# T\n\nFirst\n\n{line}\n\nThird\n"))).expect("the export succeeds");
    assert_eq!(pdf("&nbsp;"), pdf("**<Text></Text>**"));
}

#[test]
fn fonts_are_embedded_and_subset() {
    let (_dir, pdf) = showcase_pdf();
    let Some(fonts) = run("pdffonts", &[&pdf]) else { return };
    let inter = fonts.lines().find(|l| l.contains("Inter")).unwrap_or_else(|| panic!("{fonts}"));
    assert!(inter.contains("yes yes"), "embedded and subset: {inter}");
    let copies = fonts.lines().filter(|l| l.contains("Inter-Regular")).count();
    assert_eq!(copies, 1, "one copy of each face: {fonts}");
}

#[test]
fn pages_match_the_engines_own_drawing() {
    let dir = tempfile::tempdir().unwrap();
    let mut deck = open_showcase();
    let pdf = write_to(&mut deck, dir.path(), "showcase.pdf");
    for page in 1..=7 {
        let Some(difference) = page_difference(&mut deck, &pdf, page, dir.path()) else { return };
        assert!(difference < 6.0, "page {page} differs by {difference}");
    }
}

#[test]
fn layers_waiting_for_a_click_are_on_the_page() {
    let mut deck = open_source("# A\n\n<Box x=\"100\" y=\"100\" w=\"600\" h=\"200\" click=\"1\">\n\nLater\n\n</Box>\n");
    let dir = tempfile::tempdir().unwrap();
    let pdf = write_to(&mut deck, dir.path(), "click.pdf");
    let Some(text) = run("pdftotext", &[&pdf, "-"]) else { return };
    assert!(text.contains("Later"), "{text}");
}

#[test]
fn a_rotated_box_is_rotated() {
    let source = "# A\n\n<Box x=\"600\" y=\"300\" w=\"700\" h=\"300\" rotate=\"15\" fill=\"#cc2222\">\n\nTilted\n\n</Box>\n";
    let mut deck = open_source(source);
    let dir = tempfile::tempdir().unwrap();
    let pdf = write_to(&mut deck, dir.path(), "rotated.pdf");
    let Some(difference) = page_difference(&mut deck, &pdf, 1, dir.path()) else { return };
    assert!(difference < 6.0, "differs by {difference}");
    // The rotation really happened: the same box unrotated is a different picture.
    let mut flat = open_source(&source.replace("rotate=\"15\"", "rotate=\"0\""));
    let flat_pdf = write_to(&mut flat, dir.path(), "flat.pdf");
    let apart = page_difference(&mut deck, &flat_pdf, 1, dir.path()).unwrap();
    assert!(apart > difference, "rotated {difference}, against unrotated {apart}");
}

#[test]
fn emoji_and_accents_survive() {
    let mut deck = open_source("# blåbær 🎉\n");
    let dir = tempfile::tempdir().unwrap();
    let pdf = write_to(&mut deck, dir.path(), "emoji.pdf");
    let Some(text) = run("pdftotext", &[&pdf, "-"]) else { return };
    assert!(text.contains("blåbær"), "{text}");
    run("pdftoppm", &["-png", "-r", "72", "-singlefile", &pdf, &dir.path().join("emoji").to_string_lossy()]).unwrap();
    let page = Pixmap::load_png(dir.path().join("emoji.png")).unwrap();
    let coloured = page.pixels().iter().any(|p| {
        let (max, min) = (p.red().max(p.green()).max(p.blue()), p.red().min(p.green()).min(p.blue()));
        max - min > 80
    });
    assert!(coloured, "the emoji should have colour");
}

#[test]
fn a_slide_with_an_error_still_exports() {
    let mut deck = open_source("# A\n\n---\n\n<Box>\n\nnever closed\n\n---\n\n# C\n");
    let dir = tempfile::tempdir().unwrap();
    let pdf = write_to(&mut deck, dir.path(), "error.pdf");
    let Some(info) = run("pdfinfo", &[&pdf]) else { return };
    assert!(info.contains("Pages:           3"), "{info}");
}

/// What a picture can add to a file besides its own bytes.
const PICTURE_OVERHEAD: usize = 32 * 1024;

/// Noise, which compresses as badly as a photo, as a file of `format`.
fn noisy_picture(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
    let mut state = 0x2545_f491_u32;
    let mut byte = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as u8
    };
    let picture = image::RgbImage::from_fn(width, height, |_, _| image::Rgb([byte(), byte(), byte()]));
    let mut bytes = Vec::new();
    picture.write_to(&mut std::io::Cursor::new(&mut bytes), format).unwrap();
    bytes
}

/// A deck on a folder holding `files`, which has to outlive it.
fn deck_with_files(source: &str, files: &[(&str, &[u8])]) -> (tempfile::TempDir, Deck) {
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in files {
        std::fs::write(dir.path().join(name), bytes).unwrap();
    }
    let deck = Deck::open(Box::new(FolderFiles::new(dir.path().to_path_buf())), Fonts::bundled_only(), source.to_string());
    (dir, deck)
}

fn holds(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

#[test]
fn a_jpeg_goes_in_as_its_own_bytes() {
    let jpeg = noisy_picture(800, 450, image::ImageFormat::Jpeg);
    let (_dir, mut deck) = deck_with_files("![](photo.jpg)\n", &[("photo.jpg", &jpeg)]);
    let pdf = slides_pdf::write_pdf(&mut deck).unwrap();
    assert!(holds(&pdf, b"DCTDecode"));
    assert!(holds(&pdf, &jpeg), "the file's own bytes");
    assert!(pdf.len() < jpeg.len() + PICTURE_OVERHEAD, "{} against a JPEG of {}", pdf.len(), jpeg.len());
}

#[test]
fn a_jpeg_used_twice_is_stored_once() {
    let jpeg = noisy_picture(800, 450, image::ImageFormat::Jpeg);
    let (_dir, mut deck) = deck_with_files(
        "![](photo.jpg)\n\n![](photo.jpg)\n\n---\n\n![](photo.jpg)\n",
        &[("photo.jpg", &jpeg)],
    );
    let pdf = slides_pdf::write_pdf(&mut deck).unwrap();
    assert!(pdf.len() < jpeg.len() + PICTURE_OVERHEAD, "{} against a JPEG of {}", pdf.len(), jpeg.len());
}

#[test]
fn a_png_stays_lossless_and_is_stored_once() {
    let png = noisy_picture(400, 225, image::ImageFormat::Png);
    let (_dir, mut once) = deck_with_files("![](a.png)\n", &[("a.png", &png)]);
    let (_dir_too, mut twice) = deck_with_files("![](a.png)\n\n---\n\n![](a.png)\n", &[("a.png", &png)]);
    let one = slides_pdf::write_pdf(&mut once).unwrap();
    let two = slides_pdf::write_pdf(&mut twice).unwrap();
    assert!(!holds(&one, b"DCTDecode"));
    assert!(two.len() < one.len() + PICTURE_OVERHEAD, "once {}, twice {}", one.len(), two.len());
}

#[test]
fn a_picture_that_only_calls_itself_a_jpeg_is_still_drawn() {
    let png = noisy_picture(64, 36, image::ImageFormat::Png);
    let (dir, mut deck) = deck_with_files("![](a.jpg)\n", &[("a.jpg", &png)]);
    let pdf = write_to(&mut deck, dir.path(), "out.pdf");
    assert!(!holds(&std::fs::read(&pdf).unwrap(), b"DCTDecode"));
    let Some(difference) = page_difference(&mut deck, &pdf, 1, dir.path()) else { return };
    assert!(difference < 6.0, "differs by {difference}");
}

/// A photo with bands in it, which shows if it's turned or stretched wrongly.
fn banded_jpeg(width: u32, height: u32) -> Vec<u8> {
    let picture = image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([if x < width / 2 { 220 } else { 30 }, if y < height / 2 { 200 } else { 40 }, 90])
    });
    let mut bytes = Vec::new();
    picture.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Jpeg).unwrap();
    bytes
}

#[test]
fn a_jpeg_is_stretched_to_the_box_it_is_placed_in() {
    let jpeg = banded_jpeg(640, 160);
    let (dir, mut deck) = deck_with_files("<Picture src=\"a.jpg\" x=\"200\" y=\"100\" w=\"300\" h=\"800\" />\n", &[("a.jpg", &jpeg)]);
    let pdf = write_to(&mut deck, dir.path(), "out.pdf");
    assert!(holds(&std::fs::read(&pdf).unwrap(), b"DCTDecode"));
    let Some(difference) = page_difference(&mut deck, &pdf, 1, dir.path()) else { return };
    assert!(difference < 6.0, "differs by {difference}");
}

/// `jpeg` with an EXIF block saying it is to be turned a quarter of a circle.
fn with_exif_orientation(jpeg: &[u8]) -> Vec<u8> {
    let exif: &[u8] = &[
        0xFF, 0xE1, 0, 34, b'E', b'x', b'i', b'f', 0, 0, b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0,
        0, 0, 0, 0, 0, 0,
    ];
    [&jpeg[..2], exif, &jpeg[2..]].concat()
}

#[test]
fn a_jpeg_marked_as_turned_looks_as_it_does_on_the_slide() {
    let jpeg = with_exif_orientation(&banded_jpeg(640, 160));
    let (dir, mut deck) = deck_with_files("![](a.jpg)\n", &[("a.jpg", &jpeg)]);
    let pdf = write_to(&mut deck, dir.path(), "out.pdf");
    let Some(difference) = page_difference(&mut deck, &pdf, 1, dir.path()) else { return };
    assert!(difference < 6.0, "differs by {difference}");
}

#[test]
fn a_hidden_slide_is_left_out_of_a_pdf() {
    let mut deck = open_source("# One\n\n---\nhidden: true\n---\n\n# Two\n\n---\n\n# Three\n");
    let dir = tempfile::tempdir().unwrap();
    let pdf = write_to(&mut deck, dir.path(), "hidden.pdf");
    let Some(info) = run("pdfinfo", &[&pdf]) else { return };
    assert!(info.contains("Pages:           2"), "{info}");
    let Some(text) = run("pdftotext", &[&pdf, "-"]) else { return };
    assert!(text.contains("One") && text.contains("Three") && !text.contains("Two"), "{text}");
}

#[test]
fn a_pdf_of_a_talk_with_every_slide_hidden_still_has_its_pages() {
    let mut deck = open_source("---\nhidden: true\n---\n\n# One\n\n---\nhidden: true\n---\n\n# Two\n");
    let dir = tempfile::tempdir().unwrap();
    let pdf = write_to(&mut deck, dir.path(), "all-hidden.pdf");
    let Some(info) = run("pdfinfo", &[&pdf]) else { return };
    assert!(info.contains("Pages:           2"), "{info}");
}

#[test]
fn a_pdf_has_the_screens_text() {
    let cast = slides_terminal::make_test_cast(40, 6, &[(0.1, "o", "$ echo \u{1b}[32mstagepacked\u{1b}[0m\r\n"), (0.4, "o", "stagepacked\r\n$ ")]);
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("assets")).unwrap();
    std::fs::write(dir.path().join("assets/run.cast"), cast).unwrap();
    let source = "# One\n\n<Terminal src=\"assets/run.cast\" x=\"100\" y=\"300\" w=\"960\" h=\"540\" />\n";
    let mut deck = Deck::open(Box::new(FolderFiles::new(dir.path().to_path_buf())), Fonts::bundled_only(), source.to_string());
    let pdf = write_to(&mut deck, dir.path(), "terminal.pdf");
    let Some(text) = run("pdftotext", &[&pdf, "-"]) else { return };
    assert!(text.contains("echo stagepacked") && text.matches("stagepacked").count() == 2, "the screen's text is text: {text}");
    // And looks as the slide does.
    let Some(difference) = page_difference(&mut deck, &pdf, 1, dir.path()) else { return };
    assert!(difference < 6.0, "{difference}");
}

/// Each stream of `pdf` that's packed, unpacked.
fn streams(pdf: &[u8]) -> Vec<Vec<u8>> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(start) = pdf[at..].windows(7).position(|window| window == b"stream\n").map(|start| at + start + 7) {
        let Some(end) = pdf[start..].windows(9).position(|window| window == b"endstream").map(|end| start + end) else { break };
        let bytes = pdf[start..end].strip_suffix(b"\n").unwrap_or(&pdf[start..end]);
        found.extend(miniz_oxide::inflate::decompress_to_vec_zlib(bytes).ok());
        at = end + 9;
    }
    found
}

#[test]
fn a_recording_is_drawn_within_its_box_on_the_page() {
    let cast = slides_terminal::make_test_cast(40, 6, &[(0.1, "o", "$ echo stagepacked\r\n")]);
    let (_dir, mut deck) = deck_with_files("# One\n\n<Terminal src=\"run.cast\" x=\"100\" y=\"300\" w=\"960\" h=\"540\" />\n", &[("run.cast", &cast)]);
    let pdf = slides_pdf::write_pdf(&mut deck).unwrap();
    let page = streams(&pdf).into_iter().map(|stream| String::from_utf8_lossy(&stream).into_owned()).find(|stream| stream.contains("BT")).expect("a page draws text");
    // The box, in points from the page's foot, is the clip, and the screen's text is drawn within it.
    let clip = page.find("50 390 m 530 390 l 530 120 l 50 120 l\nh\nW\nn").unwrap_or_else(|| panic!("no clip of the box on the page: {page}"));
    assert!(page[clip..].contains("BT"), "{page}");
    // A slide without a recording has none.
    let (_dir, mut plain) = deck_with_files("# One\n\nWords\n", &[]);
    let pdf = slides_pdf::write_pdf(&mut plain).unwrap();
    assert!(streams(&pdf).iter().any(|stream| String::from_utf8_lossy(stream).contains("BT")));
    assert!(streams(&pdf).iter().all(|stream| !String::from_utf8_lossy(stream).contains("W\nn")));
}
