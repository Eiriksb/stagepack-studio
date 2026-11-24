//! Import from Slidev: a Slidev talk as a talk of our own, compared with an expected file in
//! `tests/import/`. `UPDATE_EXPECTED=1` writes the expected files afresh, to be read through
//! before they're kept.

use std::path::{Path, PathBuf};

use addon_abi::{decode, encode, Bundle, Notice, NoticeKind, Report, ARGS, OP, REPORT};
use slides_format::{parse_talk, Block, Slide, Talk};
use slidev_addon::{export, import, run, ExportArgs, ImportArgs};

fn file(name: &str, data: &[u8]) -> (String, Vec<u8>) {
    (name.to_string(), data.to_vec())
}

fn args() -> ImportArgs {
    ImportArgs { entry: "slides.md".into() }
}

fn import_with(source: &str, mut files: Bundle) -> (Bundle, Report) {
    files.insert(0, file("slides.md", source.as_bytes()));
    import(&args(), &files).expect("the import finishes")
}

fn slides_md(bundle: &Bundle) -> String {
    let (_, data) = bundle.iter().find(|(name, _)| name == "slides.md").expect("the import has slides.md");
    String::from_utf8(data.clone()).unwrap()
}

fn expected_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/import").join(format!("{name}.md"))
}

/// Holds `actual` to `tests/import/<name>.md`, and holds it to parsing with no slide errors.
fn assert_expected(name: &str, actual: &str) {
    assert_parses(actual);
    let path = expected_path(name);
    if std::env::var_os("UPDATE_EXPECTED").is_some() {
        std::fs::write(&path, actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{} is missing", path.display()));
    assert_eq!(actual, expected, "the import differs from {}", path.display());
}

fn assert_parses(markdown: &str) -> Talk {
    let talk = parse_talk(markdown);
    for (index, slide) in talk.slides.iter().enumerate() {
        assert!(slide.error.is_none(), "slide {} can't be read: {:?}\n{markdown}", index + 1, slide.error);
    }
    talk
}

fn the_talk(source: &str, files: Bundle, name: &str) -> Report {
    let (bundle, report) = import_with(source, files);
    assert_expected(name, &slides_md(&bundle));
    report
}

fn notice(slide: usize, kind: NoticeKind, text: &str) -> Notice {
    Notice { slide: Some(slide), kind, text: text.to_string() }
}

fn texts(report: &Report) -> Vec<(Option<usize>, NoticeKind, String)> {
    report.notices.iter().map(|notice| (notice.slide, notice.kind, notice.text.clone())).collect()
}

const PNG: &[u8] = b"\x89PNG a picture";

#[test]
fn each_layout_maps() {
    let source = "---\nlayout: cover\n---\n\n# Cover\n\nA subtitle\n\n---\nlayout: intro\n---\n\n# Intro\n\n---\nlayout: default\n---\n\n# Default\n\n---\nlayout: center\n---\n\n# Center\n\n---\nlayout: two-cols\n---\n\n# Two\n\nLeft\n\n::right::\n\nRight\n\n---\nlayout: two-cols-header\n---\n\n# Header\n\n::left::\n\nLeft\n\n::right::\n\nRight\n\n---\nlayout: section\n---\n\n# Section\n\n---\nlayout: none\n---\n\nNothing\n\n---\nlayout: full\n---\n\nFull\n\n---\nlayout: quote\n---\n\n# A quote\n\n---\n\n# No layout\n";
    let report = the_talk(source, Vec::new(), "each_layout_maps");
    assert_eq!(report.notices, vec![notice(10, NoticeKind::Approximation, "Slidev's layout quote has no match here, so this slide uses content.")]);
}

#[test]
fn slots_become_regions() {
    let source = "---\nlayout: two-cols\n---\n\n# Two columns\n\nOn the left.\n\n::right::\n\nOn the right.\n\n::default::\n\nBack on the left.\n\n---\nlayout: default\n---\n\n# Slots on one column\n\nFirst.\n\n::right::\n\nSecond.\n\n::bottom::\n\nThird.\n";
    let report = the_talk(source, Vec::new(), "slots_become_regions");
    assert_eq!(
        report.notices,
        vec![
            notice(2, NoticeKind::Approximation, "The slot right has no place in the content layout, so what's in it is in the body."),
            notice(2, NoticeKind::Approximation, "The slot bottom has no place in the content layout, so what's in it is in the body."),
        ]
    );
}

#[test]
fn clicks_in_order() {
    // Each comes in as Slidev counts it: one after another, `v-after` with the one before, an
    // absolute `at` where it says, and a relative one that far after the last.
    let source = concat!(
        "# Clicks\n\n",
        "<div v-click>One</div>\n\n",
        "<div v-after>Also one</div>\n\n",
        "<v-click>\n\nTwo\n\n</v-click>\n\n",
        "<v-click at=\"+2\">\n\nFour\n\n</v-click>\n\n",
        "<v-click at=\"7\">\n\nSeven\n\n</v-click>\n\n",
        "<div v-click=\"'+1'\">Five</div>\n\n",
        "<img v-click src=\"/dot.png\" style=\"transition:none\">\n\n",
        "<v-click at=\"soon\">\n\nNever counted\n\n</v-click>\n\n",
        "<div v-click.hide>Hidden on a click</div>\n\n",
        "<div v-motion :initial=\"{y:40,opacity:0}\" :click-3=\"{y:0,opacity:1}\">Flies up</div>\n\n",
        "```ts {1|2|3}\na\nb\nc\n```\n\n",
        "<div v-click>Ten</div>\n",
    );
    let report = the_talk(source, vec![file("public/dot.png", PNG)], "clicks_in_order");
    assert_eq!(
        texts(&report),
        vec![
            (Some(1), NoticeKind::Approximation, "at=\"soon\" isn't a click Slidev can read, so this is shown from the start.".to_string()),
            (Some(1), NoticeKind::Approximation, "Something here is hidden on a click in Slidev, and here it is always shown.".to_string()),
            (Some(1), NoticeKind::Approximation, "A code block's lines are lit up one click at a time in Slidev, and here it is shown all at once.".to_string()),
        ]
    );
}

#[test]
fn v_clicks_on_a_list() {
    let source = concat!(
        "# From the first click\n\n<v-clicks>\n\n- One\n- Two\n- Three\n\n</v-clicks>\n\n",
        "---\n\n# After a click\n\n<div v-click>First</div>\n\n<v-clicks>\n\n1. Two\n2. Three\n\n</v-clicks>\n\n",
        "---\n\n# At five, two at a time\n\n<v-clicks at=\"5\" every=\"2\">\n\n- A\n- B\n- C\n\n</v-clicks>\n\n",
        "---\n\n# Paragraphs, from the first click\n\n<v-clicks>\n\nOne\n\nTwo\n\n</v-clicks>\n\n",
        "---\n\n# A list with words around it\n\nBefore the list.\n\n<v-clicks>\n\n- One\n- Two\n\n</v-clicks>\n",
    );
    let report = the_talk(source, Vec::new(), "v_clicks_on_a_list");
    assert!(report.notices.is_empty(), "{report:?}");
}

#[test]
fn notes_comments() {
    let source = concat!(
        "# Notes\n\nWords.\n\n<!-- a comment in the middle -->\n\nMore words.\n\n<!--\nThe last comment is the notes.\n\nWith </Notes> and &lt;b&gt;, and --- alone:\n---\n-->\n",
        "---\n\n# A comment that isn't last\n\n<!-- not the notes -->\n\nThe end.\n",
        "---\n\n# Two comments at the end\n\n<!-- first -->\n<!-- second -->\n",
    );
    let report = the_talk(source, Vec::new(), "notes_comments");
    assert_eq!(
        report.notices,
        vec![
            notice(1, NoticeKind::LeftOut, "A comment in the slide, which Slidev never shows, was left out."),
            notice(2, NoticeKind::LeftOut, "A comment in the slide, which Slidev never shows, was left out."),
            notice(3, NoticeKind::LeftOut, "A comment in the slide, which Slidev never shows, was left out."),
        ]
    );
    let talk = parse_talk(&slides_md(&import_with(source, Vec::new()).0));
    let notes = talk.slides[0].notes.as_ref().expect("the first slide has notes");
    assert_eq!(notes.text, "The last comment is the notes.\n\nWith </Notes> and <b>, and --- alone:\n---");
    assert_eq!(talk.slides[2].notes.as_ref().map(|notes| notes.text.as_str()), Some("second"));
}

#[test]
fn transitions_and_hide() {
    // Slidev plays a slide's transition as it leaves it, and the headmatter's where a slide
    // has none; here a slide's transition is how it's arrived at.
    let source = concat!(
        "---\ntransition: fade\n---\n\n# One\n\n",
        "---\ntransition: slide-left\n---\n\n# Two\n\n",
        "---\ntransition: slide-up | slide-down\n---\n\n# Three\n\n",
        "---\ntransition: view-transition\n---\n\n# Four\n\n",
        "---\nhide: true\ntransition: none\n---\n\n# Hidden\n\n",
        "---\ndisabled: true\n---\n\n# Disabled\n\n",
        "---\n---\n\n# Six\n\n",
        "---\n\n# Seven\n",
    );
    let report = the_talk(source, Vec::new(), "transitions_and_hide");
    assert_eq!(report.notices, vec![notice(5, NoticeKind::Approximation, "The transition view-transition has no match here, so this slide has none.")]);
}

#[test]
fn src_imports_are_inlined() {
    let source = "# Start\n\n---\nsrc: ./pages/more.md\nlayout: section\n---\n\nThis is never shown, as Slidev has it.\n\n---\nsrc: /pages/more.md#2\n---\n\n---\n\n# End\n";
    let more = "---\nlayout: default\n---\n\n# More one\n\n![Pool](../public/pool.png)\n\n---\n\n# More two\n\n---\nsrc: ./deeper.md\n---\n";
    let deeper = "# Deeper\n\n<!--\nNotes from deep down.\n-->\n";
    let files = vec![file("pages/more.md", more.as_bytes()), file("pages/deeper.md", deeper.as_bytes()), file("public/pool.png", PNG)];
    let report = the_talk(source, files, "src_imports_are_inlined");
    assert!(report.notices.is_empty(), "{report:?}");
}

#[test]
fn public_files_move_to_assets() {
    let source = concat!(
        "---\nlayout: image-right\nimage: /photos/moth.jpg\n---\n\n# Pictures\n\n![A pool](/pool.png)\n\n",
        "<img src=\"/photos/moth.jpg\" alt=\"A moth\" style=\"position:absolute;left:98px;top:49px;width:196px\">\n\n",
        "<img src=\"/missing.png\">\n\n![On the web](https://example.com/a.png)\n\n![Beside](./beside.png)\n",
    );
    let files = vec![
        file("public/pool.png", PNG),
        file("public/photos/moth.jpg", b"jpeg"),
        file("public/fonts/Sea.ttf", b"font"),
        file("beside.png", PNG),
        file("components/Counter.vue", b"<template/>"),
        file("vendor/a/index.js", b"x"),
    ];
    let (bundle, report) = import_with(source, files);
    assert_expected("public_files_move_to_assets", &slides_md(&bundle));
    let names: Vec<&str> = bundle.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["slides.md", "assets/beside.png", "assets/fonts/Sea.ttf", "assets/photos/moth.jpg", "assets/pool.png"]);
    assert_eq!(bundle[4].1, PNG);
    assert_eq!(
        texts(&report),
        vec![
            (Some(1), NoticeKind::Approximation, "Slidev's layout image-right has no match here, so this slide uses content, with its picture at the end.".to_string()),
            (Some(1), NoticeKind::Placeholder, "The picture /missing.png isn't in the talk, so a labelled box holds its original text.".to_string()),
            (Some(1), NoticeKind::Placeholder, "The picture https://example.com/a.png is on the web, which isn't read, so a labelled box holds its original text.".to_string()),
            (None, NoticeKind::LeftOut, "The talk's vendor folder was left out.".to_string()),
            (None, NoticeKind::LeftOut, "1 file of the Slidev talk isn't slides or a picture, so it was left out: components/Counter.vue.".to_string()),
        ]
    );
}

#[test]
fn vue_html_and_script_become_placeholders_and_are_reported() {
    let source = concat!(
        "---\ntheme: seriph\nthemeConfig:\n  primary: '#5d8392'\n---\n\n# Things Slidev runs\n\n<Counter :count=\"10\" />\n\n",
        "---\n\n# Raw HTML\n\n<div class=\"grid\">\n<div>Left</div>\n<div>Right</div>\n</div>\n\nPage {{ $page }} of the talk, with a <carbon-moon /> in it.\n\n",
        "---\n\n# Code\n\n<script setup>\nconst count = 2\n</script>\n\n<style>\nh1 { color: red; }\n</style>\n",
    );
    let (bundle, report) = import_with(source, Vec::new());
    let markdown = slides_md(&bundle);
    assert_expected("vue_html_and_script_become_placeholders_and_are_reported", &markdown);
    assert_eq!(
        texts(&report),
        vec![
            (Some(1), NoticeKind::Approximation, "The theme seriph isn't carried over, so the talk has Stagepack's own look.".to_string()),
            (Some(1), NoticeKind::Approximation, "The headmatter's colours (themeConfig) aren't carried over.".to_string()),
            (Some(1), NoticeKind::Placeholder, "The component Counter isn't converted, so a labelled box holds its original text.".to_string()),
            (Some(2), NoticeKind::Placeholder, "The HTML element div isn't converted, so a labelled box holds its original text.".to_string()),
            (Some(2), NoticeKind::Placeholder, "The expression {{ $page }} isn't converted, so a labelled box holds its original text.".to_string()),
            (Some(2), NoticeKind::Placeholder, "The component carbon-moon isn't converted, so a labelled box holds its original text.".to_string()),
            (Some(3), NoticeKind::Placeholder, "The script isn't converted, so a labelled box holds its original text.".to_string()),
            (Some(3), NoticeKind::Placeholder, "The style block isn't converted, so a labelled box holds its original text.".to_string()),
        ]
    );
    for original in ["<Counter :count=\"10\" />", "<div class=\"grid\">", "<div>Right</div>", "{{ $page }}", "<carbon-moon />", "const count = 2", "h1 { color: red; }"] {
        assert!(markdown.contains(original), "{original} is kept");
    }
    assert_eq!(markdown.matches("**From Slidev, not converted:").count(), 6);
}

#[test]
fn an_import_outside_the_talk_is_a_placeholder() {
    let source = "# Start\n\n---\nsrc: ../outside.md\n---\n\n---\nsrc: /missing.md\n---\n\n---\nsrc: ./loop.md\n---\n";
    let looping = "# Loop\n\n---\nsrc: ./loop.md\n---\n";
    let (bundle, report) = import_with(source, vec![file("loop.md", looping.as_bytes())]);
    assert_expected("an_import_outside_the_talk_is_a_placeholder", &slides_md(&bundle));
    assert_eq!(
        report.notices,
        vec![
            notice(2, NoticeKind::Placeholder, "The slides of ../outside.md are outside the talk, so a labelled box holds its original text."),
            notice(3, NoticeKind::Placeholder, "The slides of /missing.md aren't in the talk, so a labelled box holds its original text."),
            notice(5, NoticeKind::Placeholder, "The slides of ./loop.md import themselves, so a labelled box holds its original text."),
        ]
    );
}

#[test]
fn front_matter_that_isnt_yaml_is_ignored_with_a_notice() {
    let source = "# One\n\n---\nlayout: [section\n---\n\n# Two\n";
    let (bundle, report) = import_with(source, Vec::new());
    let talk = assert_parses(&slides_md(&bundle));
    assert_eq!(talk.slides.len(), 2);
    assert_eq!(talk.slides[1].settings.as_ref().and_then(|settings| settings.get("layout")), Some("content"));
    assert_eq!(report.notices.len(), 1, "{report:?}");
    assert_eq!(report.notices[0].slide, Some(2));
    assert!(report.notices[0].text.starts_with("This slide's settings can't be read as YAML"), "{report:?}");
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(name)
}

/// Every file of a folder, as the app hands them over: all but `stagepack.json` and `vendor/`.
fn folder_files(folder: &Path) -> Bundle {
    let mut files = Vec::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(at) = folders.pop() {
        for entry in std::fs::read_dir(&at).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
                continue;
            }
            let name = path.strip_prefix(folder).unwrap().to_string_lossy().replace('\\', "/");
            if name != "stagepack.json" && !name.starts_with("vendor/") {
                files.push(file(&name, &std::fs::read(&path).unwrap()));
            }
        }
    }
    files.sort();
    files
}

#[test]
fn the_fixture_imports_and_parses_cleanly() {
    let files = folder_files(&fixture("slidev-import"));
    let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture("slidev-import/stagepack.json")).unwrap()).unwrap();
    let args = ImportArgs { entry: manifest["entry"].as_str().unwrap().to_string() };
    let (bundle, report) = import(&args, &files).expect("the fixture imports");
    let markdown = slides_md(&bundle);
    assert_expected("the_fixture", &markdown);
    let talk = assert_parses(&markdown);
    assert_eq!(talk.slides.len(), 10);
    let names: Vec<&str> = bundle.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["slides.md", "assets/photo.png"]);
    let placeholders: Vec<(Option<usize>, String)> = report.notices.iter().filter(|notice| notice.kind == NoticeKind::Placeholder).map(|notice| (notice.slide, notice.text.clone())).collect();
    assert_eq!(
        placeholders,
        vec![
            (Some(5), "The component Counter isn't converted, so a labelled box holds its original text.".to_string()),
            (Some(5), "The HTML element div isn't converted, so a labelled box holds its original text.".to_string()),
            (Some(7), "The script isn't converted, so a labelled box holds its original text.".to_string()),
        ]
    );
    assert!(report.notices.iter().any(|notice| notice.slide == Some(1) && notice.text.contains("seriph")), "{report:?}");
}

/// The clicks a slide of ours takes: the most any tag asks for, or a region shown a piece at
/// a time takes, as the engine counts them.
fn clicks_of(slide: &Slide) -> u32 {
    fn most(blocks: &[Block]) -> u32 {
        blocks
            .iter()
            .map(|block| match block {
                Block::Tag(tag) => tag.attr("click").and_then(|click| click.parse().ok()).unwrap_or(0).max(most(&tag.children)),
                Block::List { items, .. } => items.iter().map(|item| most(item)).max().unwrap_or(0),
                _ => 0,
            })
            .max()
            .unwrap_or(0)
    }
    let mut clicks = most(&slide.blocks);
    for entry in slide.settings.iter().flat_map(|settings| &settings.entries) {
        if entry.key.starts_with("region.") && entry.value.split_whitespace().any(|word| word == "click=each") {
            // Each block of the region, or each item of a list alone in it.
            let region = &entry.key["region.".len()..];
            let blocks: Vec<&Block> = slide.blocks.iter().filter(|block| !matches!(block, Block::Heading { .. } | Block::Region { .. })).collect();
            let pieces = match blocks.as_slice() {
                [Block::List { items, .. }] => items.len(),
                all => all.len(),
            };
            assert!(!region.is_empty());
            clicks = clicks.max(pieces as u32);
        }
    }
    clicks
}

fn headings(blocks: &[Block], out: &mut Vec<String>) {
    for block in blocks {
        match block {
            Block::Heading { spans, .. } => out.push(spans.iter().map(|span| span.text.as_str()).collect()),
            Block::Tag(tag) => headings(&tag.children, out),
            Block::List { items, .. } => items.iter().for_each(|item| headings(item, out)),
            _ => {}
        }
    }
}

fn layout(slide: &Slide) -> &str {
    slide.settings.as_ref().and_then(|settings| settings.get("layout")).unwrap_or("content")
}

#[test]
fn export_then_import_keeps_text_layouts_clicks_and_notes() {
    let folder = fixture("slidev-export");
    let source = std::fs::read_to_string(folder.join("slides.md")).unwrap();
    let baked = |slide: usize, n: usize, tag: &str, (x, y, w, h): (f32, f32, f32, f32), steps: &[u32]| {
        serde_json::json!({ "slide": slide, "n": n, "tag": tag, "x": x, "y": y, "w": w, "h": h, "rotate": 0.0, "steps": steps, "animated": false })
    };
    let index = serde_json::to_vec(&[baked(7, 0, "Icon", (100.0, 840.0, 96.0, 96.0), &[]), baked(7, 1, "TideGauge", (700.0, 560.0, 400.0, 300.0), &[0, 1])]).unwrap();
    let files = vec![
        file("slides.md", source.as_bytes()),
        file("assets/photo.png", &std::fs::read(folder.join("assets/photo.png")).unwrap()),
        file("baked/index.json", &index),
        file("baked/7/0.png", PNG),
        file("baked/7/1@0.png", PNG),
        file("baked/7/1@1.png", PNG),
    ];
    let (exported, _) = export(&ExportArgs { title: "A made-up talk".into() }, &files).expect("the fixture exports");
    let slidev: Bundle = exported.into_iter().filter(|(name, _)| !name.starts_with('.')).collect();
    let (imported, _) = import(&args(), &slidev).expect("the export imports");
    let markdown = slides_md(&imported);
    assert_expected("export_then_import", &markdown);

    let before = parse_talk(&source);
    let after = assert_parses(&markdown);
    assert_eq!(after.slides.len(), before.slides.len());
    for (index, (was, is)) in before.slides.iter().zip(&after.slides).enumerate() {
        let number = index + 1;
        let (mut was_headings, mut is_headings) = (Vec::new(), Vec::new());
        headings(&was.blocks, &mut was_headings);
        headings(&is.blocks, &mut is_headings);
        assert_eq!(is_headings, was_headings, "the headings of slide {number}");
        assert_eq!(layout(is), layout(was), "the layout of slide {number}");
        assert_eq!(clicks_of(is), clicks_of(was), "the clicks of slide {number}");
        assert_eq!(is.notes.as_ref().map(|notes| &notes.text), was.notes.as_ref().map(|notes| &notes.text), "the notes of slide {number}");
        assert_eq!(slides_format::is_hidden(is), slides_format::is_hidden(was), "slide {number} hidden");
    }
    // The talk's look comes back from the style block the export wrote.
    let first = after.slides[0].settings.as_ref().unwrap();
    assert_eq!(first.get("title"), Some("A made-up talk"));
    assert_eq!(first.get("color.accent"), Some("#0d9488"));
    assert_eq!(first.get("color.background"), Some("#f8fafc"));
    assert_eq!(first.get("font"), Some("Inter"));
    assert!(imported.iter().any(|(name, data)| name == "assets/photo.png" && !data.is_empty()));
}

#[test]
fn the_operation_runs_from_a_bundle() {
    let input = encode(&vec![file(OP, b"import"), file(ARGS, br#"{"entry":"talk.md"}"#), file("talk.md", b"# Hello\n")]);
    let output = decode(&run(input)).unwrap();
    let names: Vec<&str> = output.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["slides.md", REPORT]);
    assert!(String::from_utf8_lossy(&output[0].1).contains("# Hello"));
    // No arguments: the entry is slides.md.
    let output = decode(&run(encode(&vec![file(OP, b"import"), file("slides.md", b"# Hi\n")]))).unwrap();
    assert_eq!(output[0].0, "slides.md");
    let output = decode(&run(encode(&vec![file(OP, b"import")]))).unwrap();
    assert_eq!(output[0].0, ".error");
    assert_eq!(String::from_utf8_lossy(&output[0].1), "The Slidev talk has no slides.md.");
}

/// Small pieces of Slidev's Markdown, put together at random below.
const PIECES: [&str; 48] = [
    "---", "---\n", "----", "--- x", "\n", "\r\n", "\r", "\u{feff}", "\0", "```", "```ts {1|2}", "~~~", "    ", "<!--", "-->", "<!-- x -->",
    "<div v-click>", "</div>", "<div", "v-click", "<v-clicks>", "</v-clicks>", "<v-click at=\"+2\">", "</v-click>", "<v-after>", "::right::", "{{", "}}",
    "<", ">", "</", "/>", "- item", "1. one", "# Heading", "src: ./a.md", "src: ../b.md", "layout: two-cols", "hide: true", "a: [", "`", "**", "|a|b|\n|-|-|",
    "<Counter />", "<script setup>", "</script>", "<style>", "é漢😀",
];

#[test]
fn odd_inputs_never_panic_and_always_parse() {
    let long_line = "a <b c=\"d".repeat(40_000);
    let mut sources: Vec<String> = [
        "", "---", "---\n", "---\n---", "---\n---\n---\n", "```", "```\n---\nnot a break", "~~~\n---\n", "<!--", "<!-- open\n---\n", "-->", "a\r\nb\r\n---\r\n# c\r\n",
        "\u{feff}---\ntitle: BOM\n---\n# x", "\0\0\0", "# a\0b\n<div v-click>\0</div>", "<div v-click>", "</div>", "<v-clicks>\n\n- a\n", "src: ./self.md", "---\nsrc: slides.md\n---\n",
        "---\nsrc: ./a.md#1-999999999\n---\n", "---\nlayout: [\n---\n", "---\na: &a [*a]\n---\n", "::right::", "{{ x", "<Text bold>x</Text>", "</Box>", "<Region name=\"x\" />",
    ]
    .iter()
    .map(|source| source.to_string())
    .collect();
    sources.push(long_line.clone());
    sources.push(format!("# Long\n\n{}\n", "x".repeat(200_000)));
    sources.push(format!("{}x{}", "<div v-click>\n\n".repeat(200), "</div>\n\n".repeat(200)));
    sources.push(format!("---\na: {}\n---\n", "[".repeat(5000)));
    sources.push(format!("---\na:\n{}\n---\n", (0..3000).map(|level| format!("{}- x", "  ".repeat(level))).collect::<Vec<_>>().join("\n")));
    // And many put together at random, the same every run.
    let mut seed: u64 = 0x5eed_1234_abcd_0001;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..1500 {
        let count = (next() % 40) as usize;
        let source: String = (0..count).map(|_| PIECES[(next() % PIECES.len() as u64) as usize]).collect::<Vec<_>>().join(if next() % 2 == 0 { "\n" } else { "" });
        sources.push(source);
    }
    for source in &sources {
        for extra in [Vec::new(), vec![file("a.md", source.as_bytes()), file("self.md", b"---\nsrc: ./self.md\n---\n")]] {
            let mut files = vec![file("slides.md", source.as_bytes())];
            files.extend(extra);
            let (bundle, report) = import(&args(), &files).unwrap_or_else(|error| panic!("{error}: {source:?}"));
            let markdown = slides_md(&bundle);
            let talk = parse_talk(&markdown);
            for slide in &talk.slides {
                assert!(slide.error.is_none(), "{:?} from {source:?}", slide.error);
            }
            assert!(!talk.slides.is_empty());
            for notice in &report.notices {
                assert!(notice.text.chars().count() <= 500 && !notice.text.contains('\n'), "{notice:?}");
                assert!(notice.slide.is_none_or(|slide| slide >= 1 && slide <= talk.slides.len()), "{notice:?} of {}", talk.slides.len());
            }
        }
    }
    // A file that isn't UTF-8 is refused with a sentence.
    assert!(import(&args(), &vec![file("slides.md", b"\xff\xfe")]).is_err());
}

/// About `mb` MB of `unit`, over and over.
fn of_size(unit: &str, mb: usize) -> String {
    unit.repeat(mb * 1024 * 1024 / unit.len().max(1))
}

fn four_mb(unit: &str) -> String {
    of_size(unit, 4)
}

/// How long the import of `source` takes.
fn import_time(source: &str) -> std::time::Duration {
    let started = std::time::Instant::now();
    let (bundle, _) = import_with(source, Vec::new());
    let took = started.elapsed();
    assert_parses(&slides_md(&bundle));
    took
}

#[test]
fn unclosed_constructs_take_time_in_line_with_their_length() {
    let filler = "a\n".repeat(1024 * 1024);
    let leads: String = (0..1500).map(|k| format!("{}```\n", [" ", "\t"][k % 2].repeat(k / 2 + 1))).collect();
    let runs: String = (1..2000).map(|k| format!("{}a", "`".repeat(k))).collect();
    let cases = [
        ("a < with no >", four_mb("a<")),
        ("{{ with no }}", four_mb("{{ a ")),
        ("<code> with no end", four_mb("<code>x ")),
        ("![ with no ](", four_mb("![a")),
        ("![a]( with no )", four_mb("![a](b ")),
        ("unknown inline tags", four_mb("<x>")),
        ("closing tags with nothing open", four_mb("<b></i>")),
        ("formatting opened and never closed", four_mb("<b>")),
        ("<!-- with no -->", four_mb("a <!-- ")),
        ("a quoted attribute that never ends", four_mb("<a b=\"")),
        ("small scripts", four_mb("<script>a</script>\n\n")),
        ("fences that never close", format!("{leads}{filler}")),
        ("backtick runs with no partner", format!("{runs}\n{filler}")),
        ("backtick runs in an element", format!("<div>\n\n{runs}\n{filler}</div>\n")),
        ("blank lines in a list", format!("- a\n{}b\n", "\n".repeat(4 * 1024 * 1024))),
        ("indented code", four_mb("    code\n")),
        ("v-clicks in v-clicks", four_mb("<v-clicks>\n\n")),
    ];
    // Under 2 s for 4 MB in a release build; a debug build is several times slower.
    let most = std::time::Duration::from_secs(if cfg!(debug_assertions) { 10 } else { 2 });
    for (what, source) in cases {
        let took = import_time(&source);
        assert!(took < most, "{what} took {took:?}");
    }
    // And in line with the length: four times as much takes about four times as long.
    for unit in ["a<", "{{ a ", "![a", "<x>", "a <!-- ", "<a b=\"", "<script>a</script>\n\n", "<b>"] {
        let (one, four) = (import_time(&of_size(unit, 1)), import_time(&of_size(unit, 4)));
        assert!(four < one * 8 + std::time::Duration::from_millis(50), "{unit:?}: 1 MB took {one:?}, 4 MB {four:?}");
    }
}

#[test]
fn imports_stop_at_a_budget() {
    // A chain of files, each importing the next ten times: ten to the sixteenth calls unchecked.
    let mut files = Vec::new();
    for level in 0..12 {
        let next = if level == 11 { "end.md#none".to_string() } else { format!("level{}.md", level + 1) };
        let slides: String = (0..10).map(|_| format!("---\nsrc: ./{next}\n---\n\n")).collect();
        files.push(file(&format!("level{level}.md"), slides.as_bytes()));
    }
    files.push(file("end.md", b"# End\n"));
    let started = std::time::Instant::now();
    let (bundle, report) = import_with("# Start\n\n---\nsrc: ./level0.md\n---\n", files);
    assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
    assert_parses(&slides_md(&bundle));
    assert!(report.notices.iter().any(|notice| notice.slide.is_none() && notice.text.starts_with("The talk's imports go past the most")), "{report:?}");
    assert!(report.notices.iter().any(|notice| notice.kind == NoticeKind::Placeholder && notice.text.contains("are past the most the import follows")));

    // Many slides importing one big file, which would be read in again for each.
    let big = format!("# Big\n\n{}\n", "word ".repeat(200 * 1024));
    let source: String = std::iter::once("# Start\n\n".to_string()).chain((0..2000).map(|_| "---\nsrc: ./big.md\n---\n\n".to_string())).collect();
    let started = std::time::Instant::now();
    let (bundle, report) = import_with(&source, vec![file("big.md", big.as_bytes())]);
    assert!(started.elapsed() < std::time::Duration::from_secs(20), "{:?}", started.elapsed());
    let markdown = slides_md(&bundle);
    assert_parses(&markdown);
    assert!(markdown.len() < 80 * 1024 * 1024, "{}", markdown.len());
    assert!(report.notices.iter().any(|notice| notice.text.starts_with("The talk's imports go past the most")));
}

#[test]
fn a_pictures_title_cant_write_our_tags() {
    let source = "![a](/dot.png <Box click=\"3\">x</Box>)\n\n![b](/dot.png \"a <Box click=\\\"3\\\"> title\")\n";
    let (bundle, report) = import_with(source, vec![file("public/dot.png", PNG)]);
    let markdown = slides_md(&bundle);
    let talk = assert_parses(&markdown);
    let boxes = |blocks: &[Block]| blocks.iter().filter(|block| matches!(block, Block::Tag(tag) if tag.name == "Box")).count();
    assert_eq!(boxes(&talk.slides[0].blocks), 0, "{markdown}");
    assert!(markdown.contains("![b](assets/dot.png \"a \\<Box click=\\\"3\\\"> title\")"), "{markdown}");
    assert!(report.notices.iter().any(|notice| notice.text == "A picture's title can't be read as one, so it was left out."), "{report:?}");
}

#[test]
fn indented_code_cant_write_our_tags() {
    let source = "# Code\n\n    let x = 1;\nThen <Video src=\"assets/x.mp4\" /> {{ y }} and <b>\n\n    more code\n\n        deeper\n";
    let (bundle, _) = import_with(source, vec![file("public/x.mp4", b"mp4")]);
    let markdown = slides_md(&bundle);
    let talk = assert_parses(&markdown);
    fn tags(blocks: &[Block], out: &mut Vec<String>) {
        for block in blocks {
            if let Block::Tag(tag) = block {
                out.push(tag.name.clone());
                tags(&tag.children, out);
            }
        }
    }
    let mut names = Vec::new();
    tags(&talk.slides[0].blocks, &mut names);
    // Only the labelled boxes that hold the tag and the expression.
    assert_eq!(names, ["Box", "Box"], "{markdown}");
    assert!(markdown.contains("```\nlet x = 1;\n```\n\nThen"), "{markdown}");
    assert!(markdown.contains("```\nmore code\n\n    deeper\n```"), "{markdown}");
}

#[test]
fn v_clicks_counts_a_lists_levels_as_slidev_does() {
    // As `VClicks.ts` counts `depth="2"`: a, a1, a2 and b each take a click, so the click
    // after the list is the fifth.
    let source = "# Depth\n\n<v-clicks depth=\"2\">\n\n- a\n  - a1\n  - a2\n- b\n\n</v-clicks>\n\n<div v-click>After</div>\n\n---\n\n# Bound\n\n<v-clicks :every=\"2\" :at=\"'+1'\">\n\n- a\n- b\n- c\n\n</v-clicks>\n\n<div v-click>After</div>\n";
    let (bundle, report) = import_with(source, Vec::new());
    let markdown = slides_md(&bundle);
    assert_expected("v_clicks_counts_a_lists_levels_as_slidev_does", &markdown);
    let talk = assert_parses(&markdown);
    let clicks = |slide: &Slide| -> Vec<String> { slide.blocks.iter().filter_map(|block| if let Block::Tag(tag) = block { tag.attr("click").map(str::to_string) } else { None }).collect() };
    // a at 1, b at 4, After at 5. Slidev numbers a after a1 and a2 by delta, but shows it at 1.
    assert_eq!(clicks(&talk.slides[0]), ["1", "4", "5"]);
    // Two at a time from the first click: a and b at 1, c at 2, After at 3.
    assert_eq!(clicks(&talk.slides[1]), ["1", "1", "2", "3"]);
    assert!(report.notices.iter().any(|notice| notice.slide == Some(1) && notice.text.starts_with("The items under a list's items")), "{report:?}");
}

#[test]
fn small_things_slidev_reads_its_own_way() {
    // ` +1` is the number 1 to Slidev; an empty layout is the default one; a path with `\`
    // is outside the talk; a file name a link can't hold is renamed.
    let source = "# One\n\n<v-click at=\" +1\">\n\nA\n\n</v-click>\n\n---\nlayout: ''\n---\n\n# Two\n\n---\nsrc: ..\\\\x.md\n---\n\n---\n\n![x](/a%20%28b%29.png)\n";
    let (bundle, report) = import_with(source, vec![file("public/a (b).png", PNG)]);
    let markdown = slides_md(&bundle);
    assert_parses(&markdown);
    assert!(markdown.contains("<Box click=\"1\""), "{markdown}");
    assert!(markdown.contains("![x](assets/a--b-.png)"), "{markdown}");
    assert!(bundle.iter().any(|(name, data)| name == "assets/a--b-.png" && data == PNG));
    assert!(!report.notices.iter().any(|notice| notice.text.contains("layout")), "{report:?}");
    assert!(report.notices.iter().any(|notice| notice.text.contains("are outside the talk")), "{report:?}");
}

/// A generous bound on time for something linear: a debug build is several times slower.
fn generous(release_secs: u64) -> std::time::Duration {
    std::time::Duration::from_secs(if cfg!(debug_assertions) { release_secs * 8 } else { release_secs })
}

#[test]
fn a_range_of_many_parts_costs_only_the_slides_it_names() {
    // A million slides, and a range that names them all six thousand times over.
    let big = "a\n---\n".repeat(1_000_000);
    let range = vec!["1-9999999"; 6000].join(",");
    let source = format!("# Start\n\n---\nsrc: ./big.md#{range}\n---\n\n---\nsrc: ./big.md\n---\n\n---\nsrc: ./big.md\n---\n");
    let started = std::time::Instant::now();
    let (bundle, report) = import_with(&source, vec![file("big.md", big.as_bytes())]);
    let took = started.elapsed();
    assert!(took < generous(5), "{took:?}");
    let markdown = slides_md(&bundle);
    let talk = assert_parses(&markdown);
    assert_eq!(talk.slides.len(), 5000);
    assert!(markdown.len() < 2 * 1024 * 1024, "{}", markdown.len());
    let texts: Vec<&str> = report.notices.iter().filter(|notice| notice.slide.is_none()).map(|notice| notice.text.as_str()).collect();
    assert!(texts.contains(&"A src: range has more than 256 parts, so only its first 256 were read."), "{texts:?}");
    assert!(texts.contains(&"The talk imports more than 5000 slides, so the rest were left out."), "{texts:?}");
}

#[test]
fn front_matter_is_shared_and_capped() {
    // An importer's front matter of thousands of keys is refused, with a notice.
    let keys: String = (0..7000).map(|key| format!("k{key}: 1\n")).collect();
    let page = "# P\n\n---\n\n".repeat(5000);
    let source = format!("# Start\n\n---\n{keys}src: ./page.md\n---\n");
    let started = std::time::Instant::now();
    let (bundle, report) = import_with(&source, vec![file("page.md", page.as_bytes())]);
    assert!(started.elapsed() < generous(5), "{:?}", started.elapsed());
    assert_parses(&slides_md(&bundle));
    assert!(report.notices.iter().any(|notice| notice.slide == Some(2) && notice.text.contains("it names more than 256 settings")), "{report:?}");

    // Just under the cap, its block is given to each of the 5000 slides it brings in, as one.
    let keys: String = (0..250).map(|key| format!("k{key}: {}\n", "v".repeat(200))).collect();
    let source = format!("# Start\n\n---\n{keys}layout: section\nsrc: ./page.md\n---\n");
    let started = std::time::Instant::now();
    let (bundle, _) = import_with(&source, vec![file("page.md", page.as_bytes())]);
    assert!(started.elapsed() < generous(5), "{:?}", started.elapsed());
    let markdown = slides_md(&bundle);
    let talk = assert_parses(&markdown);
    assert_eq!(talk.slides.len(), 5000);
    assert!(talk.slides[1..].iter().all(|slide| slide.settings.as_ref().and_then(|settings| settings.get("layout")) == Some("section")));
    assert!(markdown.len() < 4 * 1024 * 1024, "{}", markdown.len());
}

#[test]
fn fences_that_cant_close_are_known_at_once() {
    // Fences of 3000 backticks down to three, none of which a later line can close, and a
    // million lines of one backtick after them.
    let fences: String = (3..3000).rev().map(|ticks| format!("{}\n", "`".repeat(ticks))).collect();
    let source = format!("{fences}{}", "`\n".repeat(1_000_000));
    let started = std::time::Instant::now();
    let (bundle, _) = import_with(&source, Vec::new());
    assert!(started.elapsed() < generous(3), "{:?}", started.elapsed());
    assert_parses(&slides_md(&bundle));
}

#[test]
fn a_yaml_block_after_javascripts_white_space_is_front_matter() {
    let source = "# One\n\n---\n\n\u{feff}```yaml\nlayout: section\n```\n\n# Two\n";
    let (bundle, _) = import_with(source, Vec::new());
    let talk = assert_parses(&slides_md(&bundle));
    assert_eq!(talk.slides[1].settings.as_ref().and_then(|settings| settings.get("layout")), Some("section"));
}

#[test]
fn a_picture_the_app_drew_for_an_export_comes_back_said_to_be_one() {
    let png = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/slidev-export/assets/photo.png")).unwrap();
    let source = "# Drawn\n\n<img src=\"/stagepack-baked/1-0.png\" alt=\"\" />\n\n![A photo](/photo.png)\n";
    let (bundle, report) = import_with(source, vec![file("public/stagepack-baked/1-0.png", &png), file("public/photo.png", &png)]);
    let frozen: Vec<&Notice> = report.notices.iter().filter(|notice| notice.kind == NoticeKind::Frozen).collect();
    assert_eq!(frozen.len(), 1, "{:?}", report.notices);
    assert_eq!(frozen[0].slide, Some(1));
    assert!(frozen[0].text.contains("1-0.png"), "{}", frozen[0].text);
    assert!(bundle.iter().any(|(name, _)| name.ends_with("1-0.png")));
}
