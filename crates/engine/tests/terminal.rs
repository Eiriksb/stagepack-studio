//! A terminal recording on a slide: laid out as its last screen, in display items.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use slides_engine::*;
use proptest::prelude::*;
use slides_terminal::{CantPlay, Screen, make_test_cast};

struct MapFiles(HashMap<String, Vec<u8>>);

impl TalkFiles for MapFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.0.get(path).cloned()
    }
}

/// How wide a cell is for each unit of its height: JetBrains Mono's advance over its line.
const CELL_SHAPE: f32 = 0.6 / 1.25;

/// The screen's own background, near black, and foreground, light grey.
const BACKGROUND: (u8, u8, u8) = (0x18, 0x18, 0x18);
const FOREGROUND: (u8, u8, u8) = (0xcc, 0xcc, 0xcc);

fn deck_of(source: &str, files: &[(&str, Vec<u8>)]) -> Deck {
    let files = MapFiles(files.iter().map(|(path, bytes)| (path.to_string(), bytes.clone())).collect());
    Deck::open(Box::new(files), Fonts::bundled_only(), format!("# One\n\n{source}"))
}

fn terminal_layer(deck: &mut Deck, index: usize) -> LayerSpec {
    deck.specs(index).iter().find(|spec| spec.terminal.is_some()).expect("the recording has a layer").clone()
}

/// A text item: where it is, its text, its colour and the weight of its font.
type Written = (f32, f32, String, (u8, u8, u8), u16);

/// Each text item of `layer`.
fn texts(layer: &LayerSpec, fonts: &mut Fonts) -> Vec<Written> {
    let mut found = Vec::new();
    for item in &layer.items {
        let DisplayItem::Text { text, x, y } = item else { continue };
        let Some(run) = text.buffer.layout_runs().next() else { continue };
        let glyph = run.glyphs.first().expect("a text item has a glyph");
        let colour = glyph.color_opt.expect("a terminal's text has its colour");
        let weight = fonts.system.db().face(glyph.font_id).map_or(0, |face| face.weight.0);
        found.push((*x, *y, run.text.to_string(), (colour.r(), colour.g(), colour.b()), weight));
    }
    found
}

/// The screen's rows as the text items write them, each at its column, for cells `cell` units
/// wide and `row` high starting at the layer's corner.
fn rows(layer: &LayerSpec, fonts: &mut Fonts, cell: f32, row: f32, count: usize) -> Vec<String> {
    let mut rows = vec![String::new(); count];
    let mut items = texts(layer, fonts);
    items.sort_by(|a, b| (a.1, a.0).partial_cmp(&(b.1, b.0)).unwrap());
    for (x, y, text, _, _) in items {
        let (col, at) = ((x / cell).round() as usize, (y / row).round() as usize);
        let line = &mut rows[at];
        while line.chars().count() < col {
            line.push(' ');
        }
        line.push_str(&text);
    }
    rows.iter().map(|row| row.trim_end().to_string()).collect()
}

/// Each fill of `layer`: its bounds as x, y, width and height, and its colour.
fn fills(layer: &LayerSpec) -> Vec<([f32; 4], (u8, u8, u8))> {
    layer
        .items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Fill { path, color } => {
                let b = path.bounds();
                Some(([b.left(), b.top(), b.width(), b.height()], (color.r, color.g, color.b)))
            }
            _ => None,
        })
        .collect()
}

fn near(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.01)
}

fn text_of(layer: &LayerSpec) -> String {
    layer
        .items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Text { text, .. } => Some(text.buffer.lines.iter().map(|line| line.text().to_string()).collect::<Vec<_>>().join(" ")),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn a_recording_lays_out_as_its_last_screen() {
    let cast = make_test_cast(
        20,
        4,
        &[
            (0.1, "o", "old words\r\n"),
            (0.2, "o", "\u{1b}[2J\u{1b}[H"),
            (0.3, "o", "\u{1b}[31mred\u{1b}[0m plain\r\n"),
            (0.4, "o", "\u{1b}[1;34mbold\u{1b}[0m \u{1b}[44m  \u{1b}[0m\r\n"),
            (0.5, "o", "$ "),
        ],
    );
    let mut deck = deck_of("<Terminal src=\"assets/run.cast\" x=\"100\" y=\"50\" />\n", &[("assets/run.cast", cast)]);
    let layer = terminal_layer(&mut deck, 0);
    // Its own size: 20 cells by 4 rows, 18 units a row.
    let (cell, row) = (18.0 * CELL_SHAPE, 18.0);
    assert!(near([layer.rect.x, layer.rect.y, layer.rect.w, layer.rect.h], [100.0, 50.0, 20.0 * cell, 4.0 * row]), "{:?}", layer.rect);
    let on = layer.terminal.clone().unwrap();
    assert_eq!(on.src, "assets/run.cast");
    assert_eq!((on.rect, on.rotate, on.stops), (layer.rect, layer.rotate, 0));
    assert_eq!(deck.terminals(0).len(), 1);
    assert_eq!(deck.terminals(0)[0].on, on);
    assert_eq!(deck.terminal_problem(0, layer.source.as_ref().unwrap()), None);
    assert!(deck.notices(0).is_empty(), "{:?}", deck.notices(0));

    let shown = rows(&layer, deck.fonts_mut(), cell, row, 4);
    assert_eq!(shown, ["red plain", "bold", "$", ""], "the last screen, row by row");
    let items = texts(&layer, deck.fonts_mut());
    assert!(items.iter().all(|(_, _, text, _, _)| !text.contains("words")), "what was cleared isn't shown");
    let colour_of = |word: &str| items.iter().find(|(_, _, text, _, _)| text.trim() == word).map(|(_, _, _, colour, weight)| (*colour, *weight)).unwrap();
    assert_eq!(colour_of("red"), ((0xcd, 0x31, 0x31), 400));
    assert_eq!(colour_of("plain"), (FOREGROUND, 400));
    assert_eq!(colour_of("bold"), ((0x24, 0x72, 0xc8), 700), "bold, in its own colour");

    let fills = fills(&layer);
    assert!(near(fills[0].0, [0.0, 0.0, 20.0 * cell, 4.0 * row]) && fills[0].1 == BACKGROUND, "the screen's background fills the box: {fills:?}");
    assert!(fills.iter().any(|(at, colour)| near(*at, [5.0 * cell, row, 2.0 * cell, row]) && *colour == (0x24, 0x72, 0xc8)), "cells of a colour of their own: {fills:?}");
    assert!(fills.iter().any(|(at, colour)| near(*at, [2.0 * cell, 2.0 * row, cell, row]) && *colour == FOREGROUND), "the cursor, a block: {fills:?}");
    // Each character on its cell.
    for item in &layer.items {
        let DisplayItem::Text { text, .. } = item else { continue };
        for (at, glyph) in text.buffer.layout_runs().next().unwrap().glyphs.iter().enumerate() {
            assert!((glyph.x - at as f32 * cell).abs() < 0.01 && (glyph.w - cell).abs() < 0.01, "{at}: {} {}", glyph.x, glyph.w);
        }
    }
    // Real text, in the bundled monospaced font.
    let DisplayItem::Text { text, .. } = layer.items.iter().find(|item| matches!(item, DisplayItem::Text { .. })).unwrap() else { unreachable!() };
    let glyph = &text.buffer.layout_runs().next().unwrap().glyphs[0];
    let fonts = deck.fonts_mut();
    let face = fonts.system.db().face(glyph.font_id).unwrap();
    assert_eq!(face.families[0].0, "JetBrains Mono");
}

#[test]
fn the_screen_fills_its_box_keeping_its_shape() {
    let cast = make_test_cast(80, 24, &[(0.1, "o", "hello")]);
    let files = [("assets/run.cast", cast)];
    // A box of another shape: the cells as large as fit, on whole units, in the middle.
    let mut square = deck_of("<Terminal src=\"assets/run.cast\" x=\"0\" y=\"0\" w=\"1000\" h=\"1000\" />\n", &files);
    let layer = terminal_layer(&mut square, 0);
    assert_eq!((layer.rect.w, layer.rect.h), (1000.0, 1000.0), "the box is as written");
    let fills = fills(&layer);
    assert!(near(fills[0].0, [0.0, 0.0, 1000.0, 1000.0]) && fills[0].1 == BACKGROUND, "{fills:?}");
    let row = (1000.0_f32 / (80.0 * CELL_SHAPE)).floor();
    assert_eq!(row, 26.0);
    let items = texts(&layer, square.fonts_mut());
    let (x, y, text, _, _) = &items[0];
    assert_eq!(text, "hello");
    assert_eq!((*x, *y), (((1000.0 - 80.0 * row * CELL_SHAPE) / 2.0).round(), ((1000.0 - 24.0 * row) / 2.0).round()), "centred");
    let DisplayItem::Text { text, .. } = layer.items.iter().find(|item| matches!(item, DisplayItem::Text { .. })).unwrap() else { unreachable!() };
    assert_eq!(text.buffer.metrics().line_height, row, "a row a line");
    // The cursor sits after the text, a cell on.
    assert!(fills.iter().any(|(at, colour)| near(*at, [x + 5.0 * row * CELL_SHAPE, *y, row * CELL_SHAPE, row]) && *colour == FOREGROUND), "{fills:?}");

    // With no size, its own; with one side, the other keeps its shape.
    let natural = (80.0 * 18.0 * CELL_SHAPE, 24.0 * 18.0);
    for (written, (w, h)) in [("", natural), (" w=\"1382.4\"", (1382.4, 864.0)), (" h=\"216\"", (345.6, 216.0))] {
        let mut deck = deck_of(&format!("<Terminal src=\"assets/run.cast\" x=\"0\"{written} />\n"), &files);
        let layer = terminal_layer(&mut deck, 0);
        assert!(near([layer.rect.w, layer.rect.h, 0.0, 0.0], [w, h, 0.0, 0.0]), "{written}: {:?}", layer.rect);
    }
}

#[test]
fn a_row_of_many_colours_keeps_its_characters_on_their_cells() {
    // Forty colours in a row, and a word after a gap.
    let mut row: String = (0..40).map(|at| format!("\u{1b}[38;5;{}m{}", 16 + at, (b'a' + at % 26) as char)).collect();
    row.push_str("\u{1b}[0m   end");
    let mut deck = deck_of("<Terminal src=\"assets/run.cast\" x=\"0\" />\n", &[("assets/run.cast", make_test_cast(60, 2, &[(0.1, "o", &row)]))]);
    let layer = terminal_layer(&mut deck, 0);
    let cell = 18.0 * CELL_SHAPE;
    let expected: String = (0..40u8).map(|at| (b'a' + at % 26) as char).collect::<String>() + "   end";
    assert_eq!(rows(&layer, deck.fonts_mut(), cell, 18.0, 2), [expected.as_str(), ""]);
    let items = texts(&layer, deck.fonts_mut());
    assert_eq!(items.len(), 1, "one text item for the row");
    let DisplayItem::Text { text, .. } = layer.items.iter().find(|item| matches!(item, DisplayItem::Text { .. })).unwrap() else { unreachable!() };
    let glyphs = &text.buffer.layout_runs().next().unwrap().glyphs;
    let e = glyphs.iter().find(|glyph| glyph.start == 43).unwrap();
    assert!((e.x - 43.0 * cell).abs() < 0.01, "the word after the gap is on its cells: {}", e.x);
    assert_eq!(glyphs[1].color_opt.map(|c| (c.r(), c.g(), c.b())), Some((0, 0, 95)), "each in its colour");
}

#[test]
fn a_recording_that_cant_be_read_is_a_panel_saying_why() {
    let source = "<Terminal src=\"assets/run.cast\" x=\"0\" w=\"640\" h=\"360\" />\n";
    let damaged = [make_test_cast(20, 4, &[(0.1, "o", "hi")]), b"[0.2, \"o\"\n".to_vec()].concat();
    for (bytes, why) in [
        (Some(b"just some words".to_vec()), CantPlay::NotARecording),
        (None, CantPlay::CouldNotRead),
        (Some(damaged), CantPlay::Damaged),
        (Some(b"{\"version\": 2, \"width\": 100000, \"height\": 24}\n".to_vec()), CantPlay::TooLarge),
        (Some(b"{\"version\": 3, \"term\": {\"cols\": 80, \"rows\": 24}}\n".to_vec()), CantPlay::KindNotPlayed),
    ] {
        let why = why.to_string();
        let files: Vec<(&str, Vec<u8>)> = bytes.into_iter().map(|bytes| ("assets/run.cast", bytes)).collect();
        let mut deck = deck_of(source, &files);
        let layer = terminal_layer(&mut deck, 0);
        assert_eq!(text_of(&layer), why, "the panel says why");
        assert_eq!((layer.rect.w, layer.rect.h), (640.0, 360.0));
        assert!(deck.notices(0).contains(&why), "{why}: {:?}", deck.notices(0));
        assert_eq!(deck.terminal_problem(0, layer.source.as_ref().unwrap()).as_deref(), Some(why.as_str()));
    }
    // With no file, it says so.
    let mut deck = deck_of("<Terminal x=\"0\" w=\"640\" h=\"360\" />\n", &[]);
    let layer = terminal_layer(&mut deck, 0);
    assert_eq!(text_of(&layer), TERMINAL_NO_FILE);
    assert!(deck.notices(0).iter().any(|notice| notice == TERMINAL_NO_FILE));
    assert_eq!(deck.terminal_problem(0, layer.source.as_ref().unwrap()).as_deref(), Some(TERMINAL_NO_FILE));
    // Anything else has no problem to tell of.
    assert_eq!(deck.terminal_problem(0, &(0..5)), None);
    // One in the text isn't played, and says so.
    let mut deck = deck_of("<Terminal src=\"assets/run.cast\" />\n", &[("assets/run.cast", make_test_cast(20, 4, &[]))]);
    assert!(deck.notices(0).iter().any(|notice| notice == TERMINAL_IN_TEXT), "{:?}", deck.notices(0));
    assert!(deck.specs(0).iter().all(|spec| spec.terminal.is_none()));
}

#[test]
fn marks_are_steps() {
    // Two marks at the same time are one stop.
    let cast = make_test_cast(20, 4, &[(0.1, "o", "one"), (0.5, "m", "first"), (0.6, "o", " two"), (1.0, "m", "second"), (1.0, "m", "again"), (1.2, "o", " three")]);
    let files = [("assets/run.cast", cast)];
    let mut on_its_own = deck_of("<Terminal src=\"assets/run.cast\" x=\"0\" w=\"640\" h=\"360\" />\n", &files);
    assert_eq!(terminal_layer(&mut on_its_own, 0).terminal.unwrap().stops, 2);
    assert_eq!(on_its_own.timeline(0).steps, 2, "a step for each stop");
    let mut clicked = deck_of("<Terminal src=\"assets/run.cast\" x=\"0\" w=\"640\" h=\"360\" click=\"1\" />\n", &files);
    assert_eq!(clicked.timeline(0).steps, 3, "after the click it appears on");
    // Its last screen is after every mark.
    assert!(text_of(&terminal_layer(&mut clicked, 0)).contains("one two three"));
    let mut none = deck_of("<Terminal src=\"assets/run.cast\" x=\"0\" w=\"640\" h=\"360\" />\n", &[("assets/run.cast", make_test_cast(20, 4, &[(0.1, "o", "x")]))]);
    assert_eq!(none.timeline(0).steps, 0);
}

#[test]
fn ten_recordings_are_read_and_played_aside_not_where_slides_are_laid_out() {
    let paths: Vec<String> = (0..10).map(|at| format!("assets/run{at}.cast")).collect();
    let casts: HashMap<String, Vec<u8>> = paths
        .iter()
        .enumerate()
        .map(|(at, path)| (path.clone(), make_test_cast(40, 10, &[(0.1, "o", &format!("slide {at}")), (0.2, "m", "half"), (0.3, "o", " done")])))
        .collect();
    let talk: Vec<String> = paths.iter().map(|path| format!("# Slide\n\n<Terminal src=\"{path}\" x=\"100\" y=\"100\" w=\"640\" h=\"360\" />\n")).collect();
    let mut deck = Deck::open(Box::new(MapFiles(casts.clone())), Fonts::bundled_only(), talk.join("\n---\n\n"));
    let wanted = Arc::new(AtomicUsize::new(0));
    let told = wanted.clone();
    deck.make_stills_aside(
        Arc::new(move || {
            told.fetch_add(1, Ordering::SeqCst);
        }),
        Arc::new(|_: &str| None),
    );
    let made_here = stills_decoded_on_this_thread();
    for slide in 0..10 {
        assert!(deck.layers(slide, 0.25).len() >= 2);
        deck.thumbnail(slide, 160);
        assert!(deck.notices(slide).is_empty(), "nothing said while it's read: {:?}", deck.notices(slide));
        // A plain dark panel of the box's size meanwhile, and no steps yet.
        let layer = terminal_layer(&mut deck, slide);
        assert_eq!(fills(&layer), [([0.0, 0.0, 640.0, 360.0], BACKGROUND)]);
        assert_eq!(layer.items.len(), 1, "nothing but the panel");
        assert_eq!(deck.terminal_problem(slide, layer.source.as_ref().unwrap()), None);
    }
    assert_eq!(stills_decoded_on_this_thread(), made_here, "no recording read on the thread laying out");
    assert!(deck.stills_coming());
    assert_eq!(wanted.load(Ordering::SeqCst), 10);
    let jobs = deck.still_jobs();
    assert_eq!(jobs.len(), 10, "one still each, thumbnails included");
    assert!(jobs.iter().all(|job| job.is_terminal() && !job.is_model()));
    // Made on another thread, it's taken in and its slide laid out again.
    let version = deck.slide_version(3);
    let job = jobs.into_iter().find(|job| job.path() == "assets/run3.cast").unwrap();
    let bytes = casts["assets/run3.cast"].clone();
    let made = std::thread::spawn(move || job.make(Some(Box::new(std::io::Cursor::new(bytes))))).join().unwrap();
    assert!(!made.failed());
    assert_eq!(deck.take_still(made), vec![3]);
    assert_ne!(deck.slide_version(3), version);
    let layer = terminal_layer(&mut deck, 3);
    assert!(text_of(&layer).contains("slide 3 done"), "{}", text_of(&layer));
    assert_eq!(layer.terminal.unwrap().stops, 1);
    assert_eq!(deck.timeline(3).steps, 1, "its mark is a step once it's read");
    assert!(deck.still_jobs().is_empty(), "it's made once");
    assert_eq!(stills_decoded_on_this_thread(), made_here);
}

/// Every text item of `layer` on one line, and everything it draws within its box.
fn all_on_one_line_and_within(items: &[DisplayItem], rect: Rect) -> Result<(), String> {
    let within = |l: f32, t: f32, r: f32, b: f32| l >= rect.x - 0.01 && t >= rect.y - 0.01 && r <= rect.x + rect.w + 0.01 && b <= rect.y + rect.h + 0.01;
    for item in items {
        match item {
            DisplayItem::Text { text, x, y } => {
                let runs: Vec<_> = text.buffer.layout_runs().collect();
                if runs.len() > 1 || runs.iter().any(|run| run.line_i != 0) {
                    return Err(format!("a text item of {} lines: {:?}", runs.len(), runs.iter().map(|run| run.text).collect::<Vec<_>>()));
                }
                if !within(*x, *y, *x, *y + text.height) {
                    return Err(format!("text at {x}, {y}, {} high, outside {rect:?}", text.height));
                }
            }
            DisplayItem::Fill { path, .. } | DisplayItem::Stroke { path, .. } => {
                let b = path.bounds();
                if !within(b.left(), b.top(), b.right(), b.bottom()) {
                    return Err(format!("{b:?} outside {rect:?}"));
                }
            }
            DisplayItem::Picture { .. } => return Err("a picture".into()),
        }
    }
    Ok(())
}

/// What a recording's output is made of: anything at all, and what's hard on a screen's text.
fn piece() -> impl Strategy<Value = String> {
    prop_oneof![
        any::<char>().prop_map(String::from),
        "[a-z ]{1,6}",
        prop::sample::select(vec![
            "\u{2029}", "\u{2028}", "\u{85}", "\u{a0}", "\u{3000}", "\u{202e}", "\u{200f}", "\u{1c}", "\u{301}", "\u{5d0}\u{5d1}",
            "\u{4e2d}", "\u{1f600}", "\t", "\r\n", "\u{1b}[31m", "\u{1b}[7m", "\u{1b}[4m", "\u{1b}[44m", "\u{1b}[1;3m", "\u{1b}[2m",
            "\u{1b}[0m", "\u{1b}[?25l", "\u{1b}[?25h", "\u{1b}[H", "\u{1b}[2J", "\u{1b}[38;2;1;2;3m", "\u{1b}[48;5;200m", "  ",
        ])
        .prop_map(String::from),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

    #[test]
    fn any_screen_lays_out_on_its_cells(width in 1u16..48, height in 1u16..12, output in prop::collection::vec(piece(), 0..200), w in 1.0f32..900.0, h in 1.0f32..600.0) {
        let mut screen = Screen::new(width, height);
        screen.feed(&output.concat());
        let mut fonts = Fonts::bundled_only();
        let rect = Rect { x: 10.0, y: 20.0, w, h };
        let items = screen_items(&screen, rect, &mut fonts);
        prop_assert!(all_on_one_line_and_within(&items, rect).is_ok(), "{:?}", all_on_one_line_and_within(&items, rect));
    }
}

#[test]
fn what_would_end_a_line_is_a_blank() {
    // A next line written as U+0085 is one to the screen, as to a terminal; these aren't.
    for written in ["ab\u{2029}cd", "ab\u{2028}cd", "ab\u{a0}\u{a0}", "ab\u{3000}", "\u{202e}abc", "ab\u{1c}cd"] {
        let cast = make_test_cast(20, 4, &[(0.1, "o", written)]);
        let mut deck = deck_of("<Terminal src=\"assets/run.cast\" x=\"0\" />\n", &[("assets/run.cast", cast)]);
        let layer = terminal_layer(&mut deck, 0);
        let rect = Rect { x: 0.0, y: 0.0, w: layer.rect.w, h: layer.rect.h };
        assert_eq!(all_on_one_line_and_within(&layer.items, rect), Ok(()), "{written:?}");
        let cell = 18.0 * CELL_SHAPE;
        let row = rows(&layer, deck.fonts_mut(), cell, 18.0, 4);
        assert!(row[0].starts_with("ab") || row[0].contains("abc"), "{written:?}: {row:?}");
        assert!(row[1..].iter().all(String::is_empty), "{written:?}: {row:?}");
    }
}

#[test]
fn a_wide_character_in_a_busy_row_keeps_the_rest_on_their_cells() {
    let mut row = String::from("\u{1b}[31m\u{4e2d}");
    row.extend((0..40).map(|at| format!("\u{1b}[38;5;{}m{}", 16 + at, (b'a' + at % 26) as char)));
    let cast = make_test_cast(60, 2, &[(0.1, "o", &row)]);
    let mut deck = deck_of("<Terminal src=\"assets/run.cast\" x=\"0\" />\n", &[("assets/run.cast", cast)]);
    let layer = terminal_layer(&mut deck, 0);
    let cell = 18.0 * CELL_SHAPE;
    let mut placed = Vec::new();
    for item in &layer.items {
        let DisplayItem::Text { text, x, .. } = item else { continue };
        let run = text.buffer.layout_runs().next().unwrap();
        for glyph in run.glyphs {
            placed.push((run.text[glyph.start..glyph.end].to_string(), x + glyph.x));
        }
    }
    assert_eq!(placed[0], ("\u{4e2d}".to_string(), 0.0));
    for (at, (text, x)) in placed[1..].iter().enumerate() {
        assert!((x - (at as f32 + 2.0) * cell).abs() < 0.01, "{text} at {x}, not on cell {}", at + 2);
    }
    assert_eq!(placed.len(), 41);
}

#[test]
fn the_cursor_on_a_wide_character_covers_both_its_cells() {
    let cast = make_test_cast(10, 2, &[(0.1, "o", "a\u{4e2d}b\u{1b}[1;2H")]);
    let mut deck = deck_of("<Terminal src=\"assets/run.cast\" x=\"0\" />\n", &[("assets/run.cast", cast)]);
    let layer = terminal_layer(&mut deck, 0);
    let cell = 18.0 * CELL_SHAPE;
    assert!(fills(&layer).iter().any(|(at, colour)| near(*at, [cell, 0.0, 2.0 * cell, 18.0]) && *colour == FOREGROUND), "{:?}", fills(&layer));
}

#[test]
fn a_recording_in_a_pack_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("talk.sptx");
    let cast = make_test_cast(30, 4, &[(0.1, "o", "$ echo packed\r\npacked\r\n")]);
    let source = "# One\n\n<Terminal src=\"assets/run.cast\" x=\"0\" w=\"640\" h=\"360\" />\n";
    pack_new(&pack, source, "0.1.0", &[("assets/run.cast".to_string(), cast.clone())]).unwrap();
    // Laid out where it's read.
    let mut deck = Deck::open(Box::new(open_packed(&pack).unwrap()), Fonts::bundled_only(), source.to_string());
    let layer = terminal_layer(&mut deck, 0);
    assert!(text_of(&layer).contains("echo packed"), "{}", text_of(&layer));
    assert_eq!(deck.terminal_problem(0, layer.source.as_ref().unwrap()), None);
    // And aside, its file read as the talk's files are.
    let packed = open_packed(&pack).unwrap();
    assert_eq!(recording_bytes(&packed, "assets/run.cast").as_deref(), Some(cast.as_slice()));
    let mut deck = Deck::open(Box::new(open_packed(&pack).unwrap()), Fonts::bundled_only(), source.to_string());
    deck.make_stills_aside(Arc::new(|| {}), Arc::new(|_: &str| None));
    assert!(deck.terminal_pending(0));
    let job = deck.still_jobs().pop().unwrap();
    let made = std::thread::spawn(move || {
        let bytes = recording_bytes(&packed, job.path()).map(|bytes| Box::new(std::io::Cursor::new(bytes)) as Box<dyn ReadSeek>);
        job.make(bytes)
    })
    .join()
    .unwrap();
    assert!(!made.failed());
    deck.take_still(made);
    assert!(!deck.terminal_pending(0));
    assert!(text_of(&terminal_layer(&mut deck, 0)).contains("echo packed"));
}

#[test]
fn the_box_has_its_shape_before_the_recording_is_read() {
    let cast = make_test_cast(132, 43, &[(0.1, "o", "wide"), (0.2, "m", "here")]);
    let mut deck = Deck::open(Box::new(MapFiles([("assets/run.cast".to_string(), cast.clone())].into())), Fonts::bundled_only(), "# One\n\n<Terminal src=\"assets/run.cast\" x=\"0\" />\n".to_string());
    deck.make_stills_aside(Arc::new(|| {}), Arc::new(|_: &str| None));
    let made_here = stills_decoded_on_this_thread();
    let before = terminal_layer(&mut deck, 0);
    let own = (132.0 * 18.0 * CELL_SHAPE, 43.0 * 18.0);
    assert!(near([before.rect.w, before.rect.h, 0.0, 0.0], [own.0, own.1, 0.0, 0.0]), "its header's shape at once: {:?}", before.rect);
    assert!(deck.terminal_pending(0), "it's being read");
    assert_eq!(deck.timeline(0).steps, 0, "its stops come with it");
    assert_eq!(stills_decoded_on_this_thread(), made_here, "only its header was read here");
    let job = deck.still_jobs().pop().unwrap();
    let made = std::thread::spawn(move || job.make(Some(Box::new(std::io::Cursor::new(cast))))).join().unwrap();
    deck.take_still(made);
    assert!(!deck.terminal_pending(0));
    let after = terminal_layer(&mut deck, 0);
    assert_eq!(after.rect, before.rect, "nothing moved when it came");
    assert_eq!(deck.timeline(0).steps, 1);
}

#[test]
fn recordings_are_let_go_of_past_the_budget() {
    let files: HashMap<String, Vec<u8>> = (0..3).map(|at| (format!("assets/run{at}.cast"), make_test_cast(80, 24, &[(0.1, "o", &format!("number {at}"))]))).collect();
    let talk = (0..3).map(|at| format!("# Slide {at}\n\n<Terminal src=\"assets/run{at}.cast\" x=\"0\" w=\"640\" h=\"360\" />\n")).collect::<Vec<_>>().join("\n---\n\n");
    // Each read and laid out once, and kept, within a budget.
    let read_again = |budget: usize| {
        let mut deck = Deck::open(Box::new(MapFiles(files.clone())), Fonts::bundled_only(), talk.clone());
        deck.set_picture_budget(budget);
        let made = stills_decoded_on_this_thread();
        for slide in 0..3 {
            assert!(text_of(&terminal_layer(&mut deck, slide)).contains(&format!("number {slide}")));
        }
        assert_eq!(stills_decoded_on_this_thread() - made, 3);
        // Slide 0 changed, it's laid out again: its recording is read again only if it was let go of.
        deck.update(talk.replacen("# Slide 0", "# Slide zero", 1));
        assert!(text_of(&terminal_layer(&mut deck, 0)).contains("number 0"));
        stills_decoded_on_this_thread() - made - 3
    };
    assert_eq!(read_again(512 * 1024 * 1024), 0, "kept within the budget");
    assert_eq!(read_again(1), 1, "let go of past it, and read again");
}
