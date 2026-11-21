//! The starter templates bundled in the app: each reads with nothing to say, carries no code,
//! has the files it names and no others, and can be read in its own colours, the editor's
//! marks on it too.

#[path = "support/typing.rs"]
mod typing;

use std::io::Read as _;
use std::path::PathBuf;
use std::sync::Arc;

use typing::*;

use gpui_kit::{Modifiers, Pixels, Point, TestAppContext, point, px};
use slides_engine::{Deck, DisplayItem, Fonts, LayerSpec, MARKS_STAND_OUT, Pixmap, TalkFiles};
use slides_format::{Block, DEFAULT_LAYOUT, LAYOUT_NAMES, layout_names, parse_talk};
use stagepack_studio::export::{ExportInput, ExportKind, run_export};
use stagepack_studio::ribbon::TAB_ANIMATE_ID;
use stagepack_studio::slide_editor::{MarkColors, step_badge_id};
use stagepack_studio::templates::{Template, contrast, memory_files, template, templates};
use toolkit::Color;

fn deck_of(template: &Template) -> Deck {
    Deck::open(Box::new(memory_files(template)), Fonts::bundled_only(), template.slides.to_string())
}

fn background_of(template: &Template) -> Color {
    deck_of(template).theme().background
}

/// Every block of `blocks`, those inside lists and tags too.
fn every_block(blocks: &[Block]) -> Vec<&Block> {
    let mut all = Vec::new();
    for block in blocks {
        all.push(block);
        match block {
            Block::List { items, .. } => items.iter().for_each(|item| all.extend(every_block(item))),
            Block::Tag(tag) => all.extend(every_block(&tag.children)),
            _ => {}
        }
    }
    all
}

/// The files `template`'s slides name: each picture's and each chart's.
fn named_files(template: &Template) -> Vec<String> {
    let talk = parse_talk(template.slides);
    let mut names = Vec::new();
    for slide in &talk.slides {
        for block in every_block(&slide.blocks) {
            match block {
                Block::Picture { src, .. } => names.push(src.clone()),
                Block::Tag(tag) => names.extend(["src", "data"].iter().filter_map(|name| tag.attr(name)).map(str::to_string)),
                _ => {}
            }
        }
    }
    names
}

#[test]
fn there_are_five_templates_in_order() {
    let ids: Vec<&str> = templates().iter().map(|template| template.id).collect();
    assert_eq!(ids, ["light", "dark", "warm", "ocean", "tour"]);
    let names: Vec<&str> = templates().iter().map(|template| template.name).collect();
    assert_eq!(names, ["Light", "Dark", "Warm", "Ocean", "Tour"]);
    assert_eq!(template("tour").map(|template| template.says), Some("A short talk that shows what a slide can hold"));
    assert_eq!(template("light").map(|template| template.says), Some("White, with dark text"));
    assert!(template("nope").is_none());
    for template in templates() {
        assert!(!template.says.is_empty() && !template.says.contains('\n'), "{}", template.id);
    }
}

#[test]
fn every_template_reads_with_nothing_to_say() {
    for template in templates() {
        let talk = parse_talk(template.slides);
        assert!(!talk.slides.is_empty(), "{}", template.id);
        let mut deck = deck_of(template);
        for index in 0..deck.slide_count() {
            assert_eq!(deck.notices_whole(index), Vec::<String>::new(), "{} slide {}", template.id, index + 1);
        }
        for slide in &talk.slides {
            assert!(slide.error.is_none(), "{}: {:?}", template.id, slide.error);
            let Some(settings) = &slide.settings else { continue };
            assert!(settings.get("title").is_none(), "{} has a title of its own", template.id);
            if let Some(layout) = settings.get("layout") {
                assert!(LAYOUT_NAMES.iter().any(|names| names.name == layout), "{}: {layout}", template.id);
            }
        }
    }
}

#[test]
fn the_colour_templates_are_one_empty_title_slide() {
    let colours = [
        ("light", None),
        ("dark", Some(("#0f172a", "#f1f5f9", "#38bdf8"))),
        ("warm", Some(("#fdf6e3", "#3b2f2f", "#c2410c"))),
        ("ocean", Some(("#0c4a6e", "#f0f9ff", "#fbbf24"))),
    ];
    for (id, colours) in colours {
        let template = template(id).unwrap();
        let talk = parse_talk(template.slides);
        assert_eq!(talk.slides.len(), 1, "{id}");
        let slide = &talk.slides[0];
        assert!(slide.blocks.is_empty() && slide.notes.is_none(), "{id}");
        let settings = slide.settings.as_ref().expect("a settings block");
        assert_eq!(settings.get("layout"), Some("title"), "{id}");
        let written = ["color.background", "color.text", "color.accent"].map(|key| settings.get(key));
        match colours {
            None => assert_eq!(written, [None, None, None], "{id} writes the app's own colours"),
            Some((background, text, accent)) => assert_eq!(written, [Some(background), Some(text), Some(accent)], "{id}"),
        }
        assert!(template.files.is_empty(), "{id}");
    }
}

#[test]
fn the_tour_has_eight_slides_of_what_a_slide_can_hold() {
    let tour = template("tour").unwrap();
    let talk = parse_talk(tour.slides);
    assert_eq!(talk.slides.len(), 8);
    let layout = |index: usize| talk.slides[index].settings.as_ref().and_then(|settings| settings.get("layout")).unwrap_or("content");
    assert_eq!(layout(0), "title");
    assert_eq!(layout(2), "two-columns");
    assert_eq!(layout(6), "section");
    let one_at_a_time = talk.slides[1].settings.as_ref().and_then(|settings| settings.get("region.body"));
    assert_eq!(one_at_a_time, Some("click=each"));
    let blocks = |index: usize| every_block(&talk.slides[index].blocks).into_iter().cloned().collect::<Vec<_>>();
    let tags = |index: usize| blocks(index).into_iter().filter_map(|block| if let Block::Tag(tag) = block { Some(tag.name) } else { None }).collect::<Vec<_>>();
    assert!(blocks(1).iter().any(|block| matches!(block, Block::List { .. })));
    assert_eq!(tags(3), ["Picture", "Shape"]);
    assert!(blocks(4).iter().any(|block| matches!(block, Block::Table { .. })));
    assert_eq!(tags(5), ["Chart"]);
    // Everything placed is where the app would put it: in whole numbers.
    for slide in &talk.slides {
        for block in every_block(&slide.blocks) {
            let Block::Tag(tag) = block else { continue };
            for name in ["x", "y", "w", "h"] {
                let value = tag.attr(name).unwrap_or_else(|| panic!("{} has no {name}", tag.name));
                assert!(value.parse::<u32>().is_ok(), "{} {name}={value}", tag.name);
            }
        }
    }
    // What the slides show is plain words: no tag, no file and no Markdown in them.
    let mut deck = deck_of(tour);
    for index in 0..deck.slide_count() {
        for spec in deck.specs(index) {
            for words in words_of(spec) {
                assert!(!words.contains(['<', '>', '*', '`', '#', '|', '[']) && !words.contains("assets/"), "slide {}: {words:?}", index + 1);
            }
        }
    }
}

/// The words `spec` draws, a line at a time.
fn words_of(spec: &LayerSpec) -> Vec<String> {
    spec.items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Text { text, .. } => Some(text.buffer.lines.iter().map(|line| line.text().to_string()).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect()
}

#[test]
fn no_template_carries_code() {
    for template in templates() {
        for (path, _) in template.files {
            assert!(!path.starts_with("components/") && !path.starts_with("build/"), "{}: {path}", template.id);
        }
        assert!(!template.slides.contains("```") && !template.slides.contains("~~~"), "{} has a code fence", template.id);
        let talk = parse_talk(template.slides);
        let deck = deck_of(template);
        for slide in &talk.slides {
            for block in every_block(&slide.blocks) {
                assert!(!matches!(block, Block::Code { .. }), "{} has code", template.id);
                if let Block::Tag(tag) = block {
                    assert!(deck.is_built_in(&tag.name), "{}: {}", template.id, tag.name);
                }
            }
            assert!(slide.inline_tags.iter().all(|tag| deck.is_built_in(tag)), "{}: {:?}", template.id, slide.inline_tags);
        }
        assert!(deck.components().is_empty(), "{}", template.id);
    }
}

#[test]
fn every_bundled_file_can_be_kept_in_a_working_copy() {
    // A new presentation's working copy holds the template's files it uses, so it comes back
    // whole after a crash: one whose name a copy can't hold would be lost then.
    let kept = tempfile::tempdir().unwrap();
    let recovery = stagepack_studio::recovery::Recovery::new(kept.path().to_path_buf());
    for template in templates() {
        let place = stagepack_studio::document::TalkPlace::Untitled(stagepack_studio::document::UntitledId::random());
        let files: Vec<(String, Arc<[u8]>)> = template.files.iter().map(|(path, bytes)| (path.to_string(), Arc::from(*bytes))).collect();
        recovery
            .keep_with_files(&place, "base", template.slides, &files)
            .unwrap_or_else(|error| panic!("{}: {error}", template.id));
        let found = recovery.find(&place).expect("the copy");
        assert_eq!(found.files.len(), template.files.len(), "{}", template.id);
    }
}

#[test]
fn every_file_a_template_names_is_bundled() {
    for template in templates() {
        let files = memory_files(template);
        for name in named_files(template) {
            assert!(files.read(&name).is_some_and(|bytes| !bytes.is_empty()), "{}: {name}", template.id);
        }
    }
    let tour = template("tour").unwrap();
    let photo = std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/showcase/assets/photo.png")).unwrap();
    assert_eq!(memory_files(tour).read("assets/photo.png"), Some(photo), "the showcase's photo");
}

#[test]
fn no_bundled_file_is_unused() {
    for template in templates() {
        let named = named_files(template);
        for (path, _) in template.files {
            assert!(named.iter().any(|name| name == path), "{}: {path} isn't used", template.id);
        }
    }
}

#[test]
fn contrast_is_what_the_standard_says() {
    let (black, white) = (Color::rgb(0, 0, 0), Color::rgb(255, 255, 255));
    assert!((contrast(black, white) - 21.0).abs() < 0.01);
    assert!((contrast(white, black) - 21.0).abs() < 0.01, "either way round");
    assert!((contrast(white, white) - 1.0).abs() < 0.001);
    let grey = Color::parse("#777").unwrap();
    assert!((contrast(grey, white) - 4.48).abs() < 0.01, "{}", contrast(grey, white));
}

#[test]
fn text_stands_out_on_every_template() {
    for template in templates() {
        let theme = deck_of(template).theme();
        let text = contrast(theme.text, theme.background);
        let accent = contrast(theme.accent, theme.background);
        assert!(text >= 7.0, "{}: text {text}", template.id);
        assert!(accent >= 3.0, "{}: accent {accent}", template.id);
    }
}

/// The colours the offers in the first slide's empty areas are drawn in: each words' and
/// each dashed line's, with the words they say.
fn placeholder_colours(source: &str, template: &Template) -> Vec<(String, Color)> {
    let mut deck = Deck::open(Box::new(memory_files(template)), Fonts::bundled_only(), source.to_string());
    let mut colours = Vec::new();
    for spec in deck.placeholder_specs(0) {
        let words = words_of(&spec).join(" ");
        for item in spec.items.iter() {
            match item {
                DisplayItem::Stroke { color, .. } => colours.push((format!("the line around {words}"), *color)),
                DisplayItem::Text { text, .. } => {
                    for run in text.buffer.layout_runs() {
                        for glyph in run.glyphs {
                            let color = glyph.color_opt.expect("the words have a colour of their own");
                            colours.push((words.clone(), Color { r: color.r(), g: color.g(), b: color.b(), a: color.a() }));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    colours
}

fn mark_colours(typing: &Typing, cx: &TestAppContext) -> MarkColors {
    cx.read(|cx| typing.view.read(cx).slide_editor().read(cx).mark_colors())
}

/// Asserts that the mark `what` was drawn, in a colour that stands out from `background`.
fn stands_out(id: &str, what: &str, colour: Option<Color>, background: Color) {
    let colour = colour.unwrap_or_else(|| panic!("{id}: no {what} was drawn"));
    let ratio = contrast(colour, background);
    assert!(ratio >= MARKS_STAND_OUT, "{id}: {what} {colour:?} on {background:?} is {ratio:.2}");
}

/// A text box, shown on the first click, put at the end of a template's first slide.
const TEXT_BOX: &str = "\n<Box x=\"200\" y=\"600\" w=\"400\" h=\"300\" click=\"1\" enter=\"fade\">\n\nWords\n\n</Box>\n";

/// Where the box's middle, at 400 across, is 2 short of the slide's, at 960.
const NEAR_THE_CENTRE: (f32, f32) = (558., 0.);

fn pixels(typing: &Typing, cx: &TestAppContext, (x, y): (f32, f32)) -> Point<Pixels> {
    let unit = f32::from(typing.slide_bounds(cx).size.width) / 1920.;
    point(px(x * unit), px(y * unit))
}

#[gpui_kit::test]
fn every_mark_stands_out_on_every_template(cx: &mut TestAppContext) {
    for template in templates() {
        let id = template.id;
        let background = background_of(template);
        let first = parse_talk(template.slides).slides[0].range.end;
        let source = format!("{}{TEXT_BOX}{}", &template.slides[..first], &template.slides[first..]);
        let typing = open(cx, &source);
        assert_eq!(typing.document(cx).read_with(cx, |document, _| document.theme().background), background, "{id}");

        // The words in the empty areas, and the dashed line around each: a colour template's
        // first slide is all empty areas, and the tour's has none.
        let empty = typing.layout(cx).placeholder("title").is_some();
        let offers = placeholder_colours(&typing.source(cx), template);
        if empty {
            assert!(offers.iter().any(|(words, _)| words == "Click to add text"), "{id}: {offers:?}");
            assert!(offers.iter().any(|(words, _)| words == "Click to add title"), "{id}: {offers:?}");
        }
        for (words, colour) in offers {
            stands_out(id, &words, Some(colour), background);
        }

        // The caret, typing in the title, and the frame around the area typed in.
        if empty {
            let title = typing.placeholder_point(cx, "title");
            typing.click(cx, title);
        } else {
            typing.click_text(cx, 0, pos(0, 1));
        }
        assert!(typing.caret(cx).is_some(), "{id}: a caret shows");
        let marks = mark_colours(&typing, cx);
        stands_out(id, "the caret", marks.caret, background);
        stands_out(id, "the frame of the area typed in", marks.frame, background);
        typing.press(cx, "escape");

        // A snap guide, while the box is dragged to the slide's middle.
        let element = thing(&typing, cx, "<Box");
        let from = middle(&typing, cx, &element);
        typing.mouse_down(cx, from, 1, Modifiers::default());
        typing.move_held(cx, from + pixels(&typing, cx, (200., 0.)), Modifiers::default());
        typing.move_held(cx, from + pixels(&typing, cx, NEAR_THE_CENTRE), Modifiers::default());
        let marks = mark_colours(&typing, cx);
        stands_out(id, "the snap guide", marks.guides, background);
        typing.mouse_up(cx, from + pixels(&typing, cx, NEAR_THE_CENTRE), 1, Modifiers::default());

        // The text box dragged is selected: its outline and its handles.
        assert!(outline(&typing, cx).is_some(), "{id}: the box is selected");
        let marks = mark_colours(&typing, cx);
        stands_out(id, "the selection's outline", marks.outline, background);
        stands_out(id, "the handles", marks.handles, background);

        // The step the box comes in on, on the Animate tab: the badge, and its words on it.
        click_on(&typing, cx, TAB_ANIMATE_ID);
        assert!(bounds_of(&typing, cx, step_badge_id(0)).is_some(), "{id}: a step badge shows");
        let marks = mark_colours(&typing, cx);
        stands_out(id, "the step badge", marks.badge, background);
        let badge = marks.badge.unwrap();
        let words = marks.badge_words.expect("the badge has words");
        assert!(contrast(words, badge) >= MARKS_STAND_OUT, "{id}: the badge's words {words:?} on {badge:?}");
    }
}

#[gpui_kit::test]
fn a_selection_on_a_white_slide_is_only_as_dark_as_it_must_be(cx: &mut TestAppContext) {
    let typing = open(cx, &format!("# One\n{TEXT_BOX}"));
    let background = typing.document(cx).read_with(cx, |document, _| document.theme().background);
    assert_eq!(background, Color::rgb(255, 255, 255), "a slide with no colours of its own is white");
    let element = thing(&typing, cx, "<Box");
    let at = middle(&typing, cx, &element);
    typing.click(cx, at);
    assert!(outline(&typing, cx).is_some(), "the box is selected");
    // The window theme's colour for it, as it shows on white, which doesn't stand out there.
    let grey = typing.with_window(cx, |_, cx| {
        let ring = gpui_kit::Rgba::from(gpui_kit::component::ActiveTheme::theme(cx).ring);
        let over = |value: f32| (value * ring.a * 255. + 255. * (1. - ring.a)).round() as u8;
        Color::rgb(over(ring.r), over(ring.g), over(ring.b))
    });
    assert!(contrast(grey, background) < MARKS_STAND_OUT, "{grey:?}");
    let shown = mark_colours(&typing, cx).outline.expect("the outline was drawn");
    let ratio = contrast(shown, background);
    assert!((MARKS_STAND_OUT..4.0).contains(&ratio), "{shown:?} is {ratio:.2}");
    for (channel, old) in [(shown.r, grey.r), (shown.g, grey.g), (shown.b, grey.b)] {
        assert!(channel <= old && channel > 0, "{shown:?}: between {grey:?} and black");
    }
    assert!(shown != Color::rgb(0x0a, 0x0a, 0x0a), "not the near black");
}

/// Every colour a PDF sets for filling or drawing, from its content streams: three numbers
/// before `rg`, `RG`, `sc` or `scn`, each from 0 to 1.
fn pdf_colours(pdf: &[u8]) -> Vec<Color> {
    let mut colours = Vec::new();
    let mut rest = pdf;
    while let Some(start) = find(rest, b"stream") {
        let body = &rest[start + 6..];
        let body = body.strip_prefix(b"\r\n").or_else(|| body.strip_prefix(b"\n")).unwrap_or(body);
        let Some(end) = find(body, b"endstream") else { break };
        let raw = &body[..end];
        let content = miniz_oxide::inflate::decompress_to_vec_zlib(raw).unwrap_or_else(|_| raw.to_vec());
        let text = String::from_utf8_lossy(&content);
        let words: Vec<&str> = text.split_ascii_whitespace().collect();
        for (at, word) in words.iter().enumerate() {
            if !matches!(*word, "rg" | "RG" | "sc" | "scn" | "SC" | "SCN") || at < 3 {
                continue;
            }
            let numbers: Option<Vec<f32>> = words[at - 3..at].iter().map(|number| number.parse().ok()).collect();
            if let Some([r, g, b]) = numbers.as_deref() {
                let channel = |value: f32| (value * 255.0).round() as u8;
                colours.push(Color::rgb(channel(*r), channel(*g), channel(*b)));
            }
        }
        rest = &body[end + b"endstream".len()..];
    }
    colours
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn near(a: Color, b: Color) -> bool {
    [(a.r, b.r), (a.g, b.g), (a.b, b.b)].iter().all(|(x, y)| x.abs_diff(*y) <= 1)
}

#[test]
fn a_templates_colours_reach_powerpoint_and_pdf() {
    let dark = template("dark").unwrap();
    let theme = deck_of(dark).theme();
    // With a title typed, so that the slide has text to draw in the talk's text colour.
    let source = format!("{}\n# A title\n", dark.slides);
    let input = || ExportInput { source: source.clone(), files: Arc::new(memory_files(dark)), talk_fonts: Vec::new() };
    let folder = tempfile::tempdir().unwrap();

    let pptx = folder.path().join("dark.pptx");
    run_export(input(), ExportKind::PowerPoint, &pptx).unwrap();
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&pptx).unwrap()).unwrap();
    let mut xml = String::new();
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).unwrap();
        if file.name().is_ok_and(|name| name.ends_with(".xml")) {
            file.read_to_string(&mut xml).unwrap();
        }
    }
    for (what, colour) in [("background", "0F172A"), ("text", "F1F5F9")] {
        assert!(xml.contains(&format!("val=\"{colour}\"")) || xml.contains(&format!("val=\"{}\"", colour.to_lowercase())), "the PowerPoint has the {what} colour");
    }

    let pdf = folder.path().join("dark.pdf");
    run_export(input(), ExportKind::Pdf, &pdf).unwrap();
    let colours = pdf_colours(&std::fs::read(&pdf).unwrap());
    for (what, colour) in [("background", theme.background), ("text", theme.text)] {
        assert!(colours.iter().any(|seen| near(*seen, colour)), "the PDF has the {what} colour {colour:?}: {colours:?}");
    }
}

/// Compares `pixmap` with `tests/golden/tour/<name>.png`, as the engine's golden tests do:
/// `UPDATE_GOLDEN=1` writes the file; a mismatch writes `target/golden-actual/tour/<name>.png`
/// and panics with its path.
fn assert_golden(name: &str, pixmap: &Pixmap) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let golden = root.join("tests/golden/tour").join(format!("{name}.png"));
    let actual = pixmap.encode_png().expect("encode png");
    if std::env::var("UPDATE_GOLDEN").is_ok_and(|value| value == "1") {
        std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
        std::fs::write(&golden, &actual).unwrap();
        return;
    }
    let expected = std::fs::read(&golden)
        .unwrap_or_else(|_| panic!("missing golden {}; run with UPDATE_GOLDEN=1", golden.display()));
    if expected != actual {
        let out = root.join("../../target/golden-actual/tour");
        std::fs::create_dir_all(&out).unwrap();
        let out = out.join(format!("{name}.png"));
        std::fs::write(&out, &actual).unwrap();
        panic!("golden {name} differs; actual written to {}", out.display());
    }
}

#[test]
fn no_offer_to_add_text_lies_under_a_thing_on_the_tour() {
    let tour = template("tour").unwrap();
    let talk = parse_talk(tour.slides);
    let mut deck = deck_of(tour);
    for index in 0..deck.slide_count() {
        let layout = deck.text_layout(index);
        let name = talk.slides[index].settings.as_ref().and_then(|settings| settings.get("layout")).unwrap_or(DEFAULT_LAYOUT);
        for region in layout_names(name).expect("a layout the app has").regions {
            let Some((offer, _)) = layout.placeholder(region) else { continue };
            for element in layout.elements() {
                let thing = element.rect;
                let apart = offer.x + offer.w <= thing.x || thing.x + thing.w <= offer.x || offer.y + offer.h <= thing.y || thing.y + thing.h <= offer.y;
                assert!(apart, "slide {}: the words offering to add text in {region} lie under a thing at {thing:?}", index + 1);
            }
        }
    }
}

#[test]
fn the_tour_draws_as_its_golden_pictures() {
    let mut deck = deck_of(template("tour").unwrap());
    assert_eq!(deck.slide_count(), 8);
    for index in 0..deck.slide_count() {
        let picture = deck.thumbnail(index, 480).expect("the slide draws");
        assert_eq!(picture.width(), 480);
        assert_golden(&format!("slide-{}", index + 1), &picture);
    }
}
