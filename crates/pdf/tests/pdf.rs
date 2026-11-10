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
