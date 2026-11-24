//! Export to Slidev: each part of a talk as Slidev's Markdown, compared with an expected file
//! in `tests/export/`. `UPDATE_EXPECTED=1` writes the expected files afresh, to be read
//! through before they're kept.

use std::path::{Path, PathBuf};

use addon_abi::{decode, encode, Bundle, Notice, NoticeKind, Report, ARGS, ERROR, MANIFEST, OP, REPORT};
use slidev_addon::{export, info_json, is_code_name, run, ExportArgs};

fn file(name: &str, data: &[u8]) -> (String, Vec<u8>) {
    (name.to_string(), data.to_vec())
}

fn args() -> ExportArgs {
    ExportArgs { title: "A made-up talk".into() }
}

fn export_with(source: &str, mut files: Bundle) -> (Bundle, Report) {
    files.insert(0, file("slides.md", source.as_bytes()));
    export(&args(), &files).expect("the export finishes")
}

fn slides_md(bundle: &Bundle) -> String {
    let (_, data) = bundle.iter().find(|(name, _)| name == "slides.md").expect("the export has slides.md");
    String::from_utf8(data.clone()).unwrap()
}

fn expected_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/export").join(format!("{name}.md"))
}

/// Holds `actual` to `tests/export/<name>.md`.
fn assert_expected(name: &str, actual: &str) {
    let path = expected_path(name);
    if std::env::var_os("UPDATE_EXPECTED").is_some() {
        std::fs::write(&path, actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{} is missing", path.display()));
    assert_eq!(actual, expected, "the export differs from {}", path.display());
}

fn the_slide(source: &str, files: Bundle, name: &str) -> Report {
    let (bundle, report) = export_with(source, files);
    assert_expected(name, &slides_md(&bundle));
    report
}

fn notice(slide: usize, kind: NoticeKind, text: &str) -> Notice {
    Notice { slide: Some(slide), kind, text: text.to_string() }
}

/// A baked index entry, as the app writes it.
fn baked(slide: usize, n: usize, tag: &str, (x, y, w, h): (f32, f32, f32, f32), steps: &[u32], animated: bool) -> serde_json::Value {
    serde_json::json!({ "slide": slide, "n": n, "tag": tag, "x": x, "y": y, "w": w, "h": h, "rotate": 0.0, "steps": steps, "animated": animated })
}

fn index(entries: &[serde_json::Value]) -> (String, Vec<u8>) {
    file("baked/index.json", serde_json::to_vec(entries).unwrap().as_slice())
}

const PNG: &[u8] = b"\x89PNG a picture";

#[test]
fn each_layout_maps() {
    let source = "---\nlayout: title\n---\n\n# A title\n\nA subtitle\n\n---\nlayout: content\n---\n\n# Content\n\nA body.\n\n---\nlayout: two-columns\n---\n\n# Two\n\nLeft words.\n\n<Region name=\"right\" />\n\nRight words.\n\n---\nlayout: section\n---\n\n# A section\n\n---\nlayout: blank\n---\n\nNothing but this.\n\n---\nlayout: nowhere\n---\n\n# Unknown\n";
    let report = the_slide(source, Vec::new(), "each_layout_maps");
    assert_eq!(report.notices, vec![notice(6, NoticeKind::Approximation, "There is no layout called nowhere, so this slide uses content.")]);
}

#[test]
fn a_moved_region_is_a_positioned_div() {
    let source = "---\nlayout: content\nregion.title: y=20\nregion.body: x=200 y=300 w=800 h=400 rotate=5\n---\n\n# Moved\n\nText in the moved body.\n";
    let report = the_slide(source, Vec::new(), "a_moved_region_is_a_positioned_div");
    assert!(report.notices.is_empty(), "{report:?}");
}

#[test]
fn boxes_text_and_shapes() {
    let mut source = String::from("---\nlayout: blank\n---\n\n");
    source += "<Box x=\"100\" y=\"100\" w=\"600\" h=\"300\" fill=\"#dbeafe\" outline=\"#1d4ed8\" outline-width=\"3\" radius=\"24\">\n\nIn a box, <Text color=\"red\" size=\"48\" bold>loud</Text> and <Text underline>marked</Text>, **bold**, *italic* and `code`.\n\n</Box>\n\n";
    source += "<Box padding=\"8\">\n\nA box in the text.\n\n</Box>\n\n";
    source += "<Text x=\"800\" y=\"100\" color=\"#16a34a\" italic>\n\nGreen words.\n\n</Text>\n\n";
    for (i, kind) in ["rect", "rounded", "ellipse", "line", "arrow", "callout"].iter().enumerate() {
        source += &format!("<Shape kind=\"{kind}\" x=\"{}\" y=\"600\" w=\"260\" h=\"180\" fill=\"orange\" outline=\"black\" outline-width=\"4\" />\n\n", 60 + i * 300);
    }
    source += "<Shape kind=\"arrow\" x=\"100\" y=\"900\" w=\"400\" h=\"0\" />\n\n<Shape kind=\"callout\" x=\"600\" y=\"850\" w=\"400\" h=\"200\" fill=\"white\">\n\nHello\n\n</Shape>\n";
    let report = the_slide(&source, Vec::new(), "boxes_text_and_shapes");
    assert!(report.notices.is_empty(), "{report:?}");
}

#[test]
fn pictures_are_copied_to_public() {
    let source = "# Pictures\n\n![A pool](assets/pool.png)\n\n<Picture src=\"assets/photos/crab.jpg\" x=\"1000\" y=\"300\" w=\"480\" />\n\n<Picture src=\"assets/missing.png\" />\n\n<Video src=\"assets/waves.mp4\" x=\"100\" y=\"600\" w=\"640\" h=\"360\" loop />\n";
    let files = vec![file("assets/pool.png", PNG), file("assets/photos/crab.jpg", b"jpeg"), file("assets/waves.mp4", b"mp4"), file("assets/unused.png", PNG)];
    let (bundle, report) = export_with(source, files);
    assert_expected("pictures_are_copied_to_public", &slides_md(&bundle));
    let names: Vec<&str> = bundle.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["slides.md", MANIFEST, "public/pool.png", "public/photos/crab.jpg", "public/waves.mp4"]);
    assert_eq!(bundle[2].1, PNG);
    // A video the app drew no still for plays from its own first picture.
    assert_eq!(report.notices, vec![notice(1, NoticeKind::LeftOut, "The picture assets/missing.png isn't in the talk, so it was left out.")]);
}

#[test]
fn baked_parts_are_images_at_their_box() {
    let source = "---\nlayout: content\n---\n\n# Baked\n\n<Icon name=\"star\" />\n\n<Chart src=\"assets/counts.csv\" x=\"900\" y=\"300\" w=\"800\" h=\"500\" click=\"1\" />\n\n<Box x=\"100\" y=\"600\" w=\"600\" h=\"400\" outline=\"black\" outline-width=\"2\">\n\n<TideGauge level=\"3\" />\n\n</Box>\n\n<Video src=\"assets/waves.mp4\" x=\"100\" y=\"100\" />\n\n<Model src=\"assets/shell.glb\" x=\"1200\" y=\"800\" w=\"300\" h=\"200\" />\n";
    let files = vec![
        file("assets/waves.mp4", b"mp4"),
        index(&[
            baked(1, 0, "Icon", (100.0, 220.0, 64.0, 64.0), &[], false),
            baked(1, 1, "Chart", (900.0, 300.0, 800.0, 500.0), &[], false),
            baked(1, 2, "TideGauge", (126.0, 626.0, 300.0, 200.0), &[], false),
            baked(1, 3, "Video", (100.0, 100.0, 960.0, 540.0), &[], false),
        ]),
        file("baked/1/0.png", PNG),
        file("baked/1/1.png", PNG),
        file("baked/1/2.png", PNG),
        file("baked/1/3.png", PNG),
    ];
    let (bundle, report) = export_with(source, files);
    assert_expected("baked_parts_are_images_at_their_box", &slides_md(&bundle));
    let names: Vec<&str> = bundle.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["slides.md", MANIFEST, "public/baked-1-0.png", "public/baked-1-1.png", "public/baked-1-2.png", "public/waves.mp4", "public/baked-1-3.png"]);
    assert_eq!(report.notices, vec![notice(1, NoticeKind::LeftOut, "The Model wasn't drawn for the export, so it was left out.")]);
}

#[test]
fn a_stepped_component_switches_on_clicks() {
    let source = "# Steps\n\n<Reveal x=\"100\" y=\"300\" w=\"800\" h=\"400\" />\n\n<Clock x=\"1000\" y=\"300\" w=\"400\" h=\"400\" />\n\n<Bars x=\"100\" y=\"750\" w=\"400\" h=\"300\" click=\"2\" />\n";
    let files = vec![
        index(&[
            baked(1, 0, "Reveal", (100.0, 300.0, 800.0, 400.0), &[0, 1, 2, 3], false),
            baked(1, 1, "Clock", (1000.0, 300.0, 400.0, 400.0), &[], true),
            baked(1, 2, "Bars", (100.0, 750.0, 400.0, 300.0), &[0, 2, 5], false),
        ]),
        file("baked/1/0@0.png", PNG),
        file("baked/1/0@1.png", PNG),
        file("baked/1/0@2.png", PNG),
        file("baked/1/0@3.png", PNG),
        file("baked/1/1.png", PNG),
        file("baked/1/2@0.png", PNG),
        file("baked/1/2@2.png", PNG),
        file("baked/1/2@5.png", PNG),
    ];
    let report = the_slide(source, files, "a_stepped_component_switches_on_clicks");
    assert_eq!(report.notices, vec![notice(1, NoticeKind::Frozen, "The Clock moves on its own, and in Slidev it is a picture of how it starts.")]);
}

#[test]
fn clicks_and_entries() {
    let source = "---\nlayout: two-columns\nregion.left: click=each\nregion.right: click=2 enter=fly-up\n---\n\n# Clicks\n\n- One\n- Two\n- Three\n\n<Region name=\"right\" />\n\nOn the second click.\n\n<Box x=\"100\" y=\"800\" w=\"300\" h=\"100\" click=\"1\" enter=\"fade\">\n\nFades in\n\n</Box>\n\n<Box x=\"500\" y=\"800\" w=\"300\" h=\"100\" click=\"3\">\n\nAppears\n\n</Box>\n\n<Box x=\"900\" y=\"800\" w=\"300\" h=\"100\" click=\"4\" enter=\"fly-up\">\n\nFlies up\n\n</Box>\n\n<Box x=\"1300\" y=\"800\" w=\"300\" h=\"100\" click=\"0\" enter=\"zoom\">\n\nAlways there\n\n</Box>\n";
    let report = the_slide(source, Vec::new(), "clicks_and_entries");
    assert!(report.notices.is_empty(), "{report:?}");
}

#[test]
fn transitions_hidden_and_notes() {
    let source = "---\ntransition: fade\n---\n\n# Fade\n\n<Notes>\nSay hello --> then wait.\nA second line.\n</Notes>\n\n---\ntransition: push\nhidden: true\n---\n\n# Pushed and hidden\n\n---\ntransition: none\n---\n\n# None\n\n---\ntransition: spin\n---\n\n# Spin\n";
    let report = the_slide(source, Vec::new(), "transitions_hidden_and_notes");
    assert_eq!(report.notices, vec![notice(4, NoticeKind::Approximation, "transition: spin isn't none, fade or push, so this slide has none.")]);
}

#[test]
fn talk_colours_become_one_style_block() {
    let source = "---\ncolor.background: #0f172a\ncolor.text: #f8fafc\ncolor.accent: #f59e0b\nfont: Inter\nfont.mono: Fira Code\n---\n\n# Dark\n\n---\n\n# Second\n";
    let font = include_bytes!("../../engine/assets/fonts/Inter-Bold.ttf");
    let files = vec![file("assets/fonts/Inter-Bold.ttf", font), file("assets/fonts/odd.woff2", b"woff2"), file("assets/fonts/deeper/x.ttf", b"x")];
    let (bundle, report) = export_with(source, files);
    let markdown = slides_md(&bundle);
    assert_expected("talk_colours_become_one_style_block", &markdown);
    assert_eq!(markdown.matches("<style>").count(), 1);
    let names: Vec<&str> = bundle.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["slides.md", MANIFEST, "public/fonts/Inter-Bold.ttf", "public/fonts/odd.woff2"]);
    assert!(report.notices.is_empty(), "{report:?}");
}

/// The first frontmatter block of `markdown`, as `key: value` pairs.
fn headmatter(markdown: &str) -> Vec<(String, String)> {
    let rest = markdown.strip_prefix("---\n").expect("slides.md starts with its headmatter");
    let end = rest.find("\n---\n").expect("the headmatter is closed");
    rest[..end].lines().map(|line| line.split_once(": ").map(|(key, value)| (key.to_string(), value.to_string())).expect("a key and a value")).collect()
}

/// Holds an export to Global Constraints' list, so that Stagepack asks no code question of it.
fn assert_no_code_reasons(bundle: &Bundle) {
    for (name, _) in bundle {
        assert!(name == "slides.md" || name == MANIFEST || name.starts_with("public/"), "{name} isn't one of the files an export holds");
        assert!(!is_code_name(name), "{name} would be taken for code");
        for code in [".vue", ".js", ".mjs", ".cjs", ".ts", ".mts", ".cts", ".jsx", ".tsx", ".node", ".wasm"] {
            assert!(!name.to_ascii_lowercase().ends_with(code), "{name}");
        }
        let base = name.rsplit('/').next().unwrap();
        for prefix in ["vite.config", "uno.config", "unocss.config", "postcss.config", ".postcssrc"] {
            assert!(!base.starts_with(prefix), "{name}");
        }
        assert!(!["index.html", "package.json"].contains(&base), "{name}");
        assert!(!name.split('/').any(|part| part == "setup"), "{name}");
    }
    let markdown = slides_md(bundle);
    let keys = headmatter(&markdown);
    let value = |key: &str| keys.iter().find(|(name, _)| name == key).map(|(_, value)| value.as_str());
    assert_eq!(value("theme"), Some("default"));
    assert_eq!(value("canvasWidth"), Some("1920"));
    assert_eq!(value("aspectRatio"), Some("16/9"));
    assert_eq!(value("addons"), None);
    assert_eq!(value("pwa"), None);
    assert!(!markdown.contains("<script"), "slides.md holds a script");
    let (_, manifest) = bundle.iter().find(|(name, _)| name == MANIFEST).expect("the export says its title");
    assert_eq!(serde_json::from_slice::<serde_json::Value>(manifest).unwrap(), serde_json::json!({ "title": "A made-up talk" }));
}

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/slidev-export")
}

fn fixture_files() -> Bundle {
    let folder = fixture();
    let mut files = vec![file("slides.md", &std::fs::read(folder.join("slides.md")).unwrap()), file("assets/photo.png", &std::fs::read(folder.join("assets/photo.png")).unwrap())];
    files.push(index(&[
        baked(7, 0, "Icon", (100.0, 840.0, 96.0, 96.0), &[], false),
        baked(7, 1, "TideGauge", (700.0, 560.0, 400.0, 300.0), &[0, 1], false),
    ]));
    files.extend([file("baked/7/0.png", PNG), file("baked/7/1@0.png", PNG), file("baked/7/1@1.png", PNG)]);
    files
}

#[test]
fn the_fixture_exports_with_no_code_reasons() {
    let (bundle, report) = export(&args(), &fixture_files()).unwrap();
    assert_no_code_reasons(&bundle);
    assert_expected("the_fixture", &slides_md(&bundle));
    let names: Vec<&str> = bundle.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["slides.md", MANIFEST, "public/baked-7-0.png", "public/photo.png", "public/baked-7-1-0.png", "public/baked-7-1-1.png"]);
    assert!(report.notices.is_empty(), "{report:?}");
}

#[test]
fn every_export_here_has_no_code_reasons() {
    let sources = ["# One\n", "", "---\nlayout: blank\n---\n\n<Box>\n\nnever closed\n"];
    for source in sources {
        let (bundle, _) = export_with(source, vec![file("assets/run.js", b"alert(1)"), file("assets/package.json", b"{}")]);
        assert_no_code_reasons(&bundle);
    }
    // A picture whose name is on the code list is left out rather than written.
    let (bundle, report) = export_with("<Picture src=\"assets/setup/a.png\" />\n\n<Picture src=\"assets/index.html\" />\n", vec![file("assets/setup/a.png", PNG), file("assets/index.html", b"x")]);
    assert_no_code_reasons(&bundle);
    assert_eq!(report.notices.len(), 2);
    assert!(report.notices.iter().all(|notice| notice.kind == NoticeKind::LeftOut));
}

/// The lines of the export's last slide that come from its body, past its frontmatter.
fn body_of(markdown: &str) -> String {
    markdown.rsplit_once("\n---\n\n").map(|(_, body)| body.to_string()).unwrap()
}

#[test]
fn text_that_slidev_would_read_is_escaped() {
    let source = "# First\n\n---\n\n\\---\n\nA \\< sign, and \\<!-- not a note.\n\n{{ x }} is not code.\n\n::right::\n\n- \\---\n- a `{{ y }}` in code\n";
    let (bundle, _) = export_with(source, Vec::new());
    let body = body_of(&slides_md(&bundle));
    assert_expected("text_that_slidev_would_read_is_escaped", &body);
    let lines: Vec<&str> = body.lines().collect();
    // None of Slidev's own lines is left: no slide break, no slot, no comment.
    assert!(!lines.iter().any(|line| line.trim() == "---"), "{body}");
    assert!(!lines.iter().any(|line| line.trim().starts_with("::")), "{body}");
    assert!(!body.contains("<!--"), "{body}");
    assert!(body.contains("\\---"));
    assert!(body.contains("A &lt; sign, and &lt;\\!-- not a note."));
    assert!(body.contains("\\:\\:right\\:\\:"));
    // Vue's `{{` only stays text inside `v-pre`.
    for (at, _) in body.match_indices("{{") {
        let before = &body[..at];
        let opened = before.matches("<div v-pre>").count();
        let closed = before.matches("</div>").count();
        assert_eq!(opened, closed + 1, "the {{{{ at {at} isn't inside v-pre: {body}");
    }
}

#[test]
fn a_slide_with_an_error_is_left_out_with_a_notice() {
    let source = "# One\n\n---\n\n<Box>\n\nnever closed\n\n---\n\n# Three\n";
    let (bundle, report) = export_with(source, Vec::new());
    let markdown = slides_md(&bundle);
    assert_expected("a_slide_with_an_error_is_left_out_with_a_notice", &markdown);
    assert!(!markdown.contains("never closed"));
    assert_eq!(report.notices.len(), 1, "{report:?}");
    assert_eq!(report.notices[0].slide, Some(2));
    assert_eq!(report.notices[0].kind, NoticeKind::LeftOut);
    assert!(report.notices[0].text.starts_with("This slide can't be read ("), "{}", report.notices[0].text);
    assert!(report.notices[0].text.ends_with("), so it was left out."));
}

#[test]
fn a_talk_with_only_errors_still_has_its_headmatter() {
    let (bundle, report) = export_with("<Box>\n\nnever closed\n", Vec::new());
    assert_no_code_reasons(&bundle);
    assert_eq!(report.notices.len(), 1);
}

#[test]
fn a_talk_without_slides_md_is_refused() {
    assert_eq!(export(&args(), &Vec::new()).unwrap_err(), "The talk has no slides.md to export.");
    let not_text = vec![file("slides.md", &[0xff, 0xfe])];
    assert_eq!(export(&args(), &not_text).unwrap_err(), "The talk's slides.md isn't UTF-8 text.");
    let bad_index = vec![file("slides.md", b"# A\n"), file("baked/index.json", b"[{")];
    assert!(export(&args(), &bad_index).unwrap_err().starts_with("The list of parts drawn for the export, baked/index.json, can't be read: "));
}

#[test]
fn the_info_names_the_add_on_and_its_operations() {
    let info: serde_json::Value = serde_json::from_str(&info_json()).unwrap();
    assert_eq!(info, serde_json::json!({ "name": "slidev", "version": "0.1.0", "operations": ["export", "import"] }));
}

#[test]
fn run_answers_an_export_with_its_files_and_a_report() {
    let input = encode(&vec![file(OP, b"export"), file(ARGS, br#"{"title":"A made-up talk"}"#), file("slides.md", b"# A\n\n<Picture src=\"assets/none.png\" />\n")]);
    let output = decode(&run(input)).unwrap();
    let names: Vec<&str> = output.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["slides.md", MANIFEST, REPORT]);
    let report: Report = serde_json::from_slice(&output[2].1).unwrap();
    assert_eq!(report.notices.len(), 1);
}

#[test]
fn run_answers_what_it_cant_do_with_an_error() {
    let cases: [(Vec<u8>, &str); 4] = [
        (b"not a bundle".to_vec(), "The bundle doesn't start with the bundle magic `SPB1`."),
        (encode(&vec![file("slides.md", b"# A\n")]), "The add-on was given no operation to do."),
        (encode(&vec![file(OP, b"export"), file(ARGS, b"{}"), file("slides.md", b"# A\n")]), "The export's arguments can't be read: missing field `title` at line 1 column 2."),
        (encode(&vec![file(OP, b"dance"), file(ARGS, b"{}")]), "The add-on has no operation called \"dance\"."),
    ];
    for (input, sentence) in cases {
        let output = decode(&run(input)).unwrap();
        assert_eq!(output, vec![file(ERROR, sentence.as_bytes())]);
    }
}
