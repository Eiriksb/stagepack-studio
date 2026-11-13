use std::sync::Arc;

use slides_engine::*;
use slides_format::{FlowPlace, Pos, text_flows};
use unicode_segmentation::UnicodeSegmentation;

struct NoFiles;
impl TalkFiles for NoFiles { fn read(&self, _: &str) -> Option<Vec<u8>> { None } }

fn deck(source: &str) -> Deck { Deck::open(Box::new(NoFiles), Fonts::bundled_only(), source.to_string()) }

fn layout(source: &str) -> Arc<TextLayout> { deck(source).text_layout(0) }

fn at(para: usize, offset: usize) -> Pos { Pos { para, offset } }

fn caret(layout: &TextLayout, flow: usize, pos: Pos) -> CaretRect {
    layout.caret(flow, pos).unwrap_or_else(|| panic!("no caret at {pos:?} of flow {flow}"))
}

/// Where the middle of the caret is on the slide, turned with its layer.
fn middle(caret: &CaretRect) -> (f32, f32) { caret.on_slide(caret.x, caret.top + caret.height / 2.0) }

fn hit_at(layout: &TextLayout, point: (f32, f32)) -> Hit { layout.hit(point.0, point.1).expect("a point on the slide") }

fn near(a: f32, b: f32, within: f32) -> bool { (a - b).abs() <= within }

fn run(text: &str) -> TextRun {
    let theme = toolkit::Theme::default();
    TextRun { text: text.into(), size: 40.0, bold: false, italic: false, underline: false, color: theme.text, family: theme.font }
}

/// A body paragraph long enough to wrap in the content layout's body.
fn long_text() -> String { "Words that wrap ".repeat(16).trim_end().to_string() }

/// The offsets of `flow`'s paragraph `para` where a caret can be: its grapheme edges.
fn edges(layout_source: &str, flow: usize, para: usize) -> Vec<usize> {
    let talk = slides_format::parse_talk(layout_source);
    let text: String = text_flows(&talk, 0)[flow].paras[para].spans.iter().map(|span| span.text.as_str()).collect();
    text.grapheme_indices(true).map(|(at, _)| at).chain([text.len()]).collect()
}

#[test]
fn the_caret_at_the_start_is_at_the_paragraphs_left() {
    let layout = layout("# Title\n\nHello world\n");
    let start = caret(&layout, 1, at(0, 0));
    let placed = layout.paras().iter().find(|para| para.flow == 1 && para.para == 0).expect("the body is placed");
    assert!(near(start.x, 100.0, 0.5), "x is {}", start.x);
    assert!(near(start.x, placed.rect.x, 0.01));
    assert!(near(start.top, placed.rect.y, 0.01));
    assert!(near(start.top, 220.0, 0.5), "top is {}", start.top);
    assert!(near(start.height, 50.0, 0.5), "height is {}", start.height);
    assert_eq!(start.rotate, 0.0);
}

#[test]
fn the_caret_moves_right_by_each_characters_width() {
    let layout = layout("# Title\n\nHello\n");
    let mut fonts = Fonts::bundled_only();
    let start = caret(&layout, 1, at(0, 0)).x;
    for end in 1..="Hello".len() {
        let width = shape_text(&mut fonts, &[run(&"Hello"[..end])], None, Align::Left).width;
        let x = caret(&layout, 1, at(0, end)).x;
        assert!(near(x - start, width, 0.5), "after {end} letters the caret moved {}, the letters are {width} wide", x - start);
    }
}

#[test]
fn the_caret_after_a_wrap_is_on_the_next_line() {
    let text = long_text();
    let layout = layout(&format!("# Title\n\n{text}\n"));
    let first = caret(&layout, 1, at(0, 0));
    let on_next = (0..=text.len()).find(|&offset| caret(&layout, 1, at(0, offset)).top > first.top).expect("the text wraps");
    let wrapped = caret(&layout, 1, at(0, on_next));
    assert!(near(wrapped.x, 100.0, 0.5), "the next line starts at the left, not at {}", wrapped.x);
    assert!(near(wrapped.top, first.top + first.height, 0.5));
    let before = caret(&layout, 1, at(0, on_next - 1));
    assert_eq!(before.top, first.top);
    assert!(before.x > 1000.0, "the caret before the wrap is at the line's end, not at {}", before.x);
}

#[test]
fn a_caret_in_centred_text_follows_the_alignment() {
    let layout = layout("---\nlayout: title\n---\n\n# Hi there\n");
    let (start, end) = (caret(&layout, 0, at(0, 0)), caret(&layout, 0, at(0, "Hi there".len())));
    assert!(start.x > 600.0, "the start is at {}", start.x);
    assert!(near((start.x + end.x) / 2.0, 960.0, 1.0), "the text is centred on {}", (start.x + end.x) / 2.0);
}

#[test]
fn a_click_on_a_letter_gives_the_gap_nearest_it() {
    let layout = layout("# Title\n\nHello\n");
    let (one, two) = (caret(&layout, 1, at(0, 1)), caret(&layout, 1, at(0, 2)));
    let y = one.top + one.height / 2.0;
    assert_eq!(hit_at(&layout, (one.x + (two.x - one.x) * 0.3, y)), Hit::Text { flow: 1, pos: at(0, 1) });
    assert_eq!(hit_at(&layout, (one.x + (two.x - one.x) * 0.7, y)), Hit::Text { flow: 1, pos: at(0, 2) });
}

#[test]
fn a_click_past_the_end_of_a_line_gives_the_lines_end() {
    let layout = layout("# Title\n\nHello\n");
    let start = caret(&layout, 1, at(0, 0));
    assert_eq!(hit_at(&layout, (1500.0, start.top + 10.0)), Hit::Text { flow: 1, pos: at(0, 5) });

    let text = long_text();
    let layout = self::layout(&format!("# Title\n\n{text}\n"));
    let first = caret(&layout, 1, at(0, 0));
    let Hit::Text { flow: 1, pos } = hit_at(&layout, (1815.0, first.top + 10.0)) else { panic!("a click on the line's right is on the text") };
    let end = caret(&layout, 1, pos);
    assert_eq!(end.top, first.top, "the end of the first line, not the start of the next");
    assert_eq!(pos, layout.line_move(1, at(0, 0), LineMove::LineEnd, None));
    let next = caret(&layout, 1, at(0, pos.offset + 1));
    assert!(next.top > first.top || next.x > end.x);
}

#[test]
fn a_click_in_a_turned_box_is_turned_back() {
    let layout = layout("<Text x=\"600\" y=\"300\" w=\"600\" h=\"200\" rotate=\"90\">\n\nHello world\n\n</Text>\n");
    let third = caret(&layout, 0, at(0, 3));
    assert_eq!(third.rotate, 90.0);
    assert_eq!(third.centre, (900.0, 400.0));
    let turned = middle(&third);
    assert!(!near(turned.0, third.x, 1.0), "the point is turned");
    assert_eq!(hit_at(&layout, turned), Hit::Text { flow: 0, pos: at(0, 3) });
    assert!(!matches!(hit_at(&layout, (third.x, third.top + third.height / 2.0)), Hit::Text { .. }), "the box isn't there unturned");
}

#[test]
fn a_click_on_a_table_says_why_it_cannot_be_edited() {
    let layout = layout("# T\n\n| a | b |\n|---|---|\n| 1 | 2 |\n");
    assert_eq!(hit_at(&layout, (300.0, 240.0)), Hit::Uneditable("Tables can't be edited on the slide yet."));
}

#[test]
fn a_click_in_an_empty_region_names_it() {
    let layout = layout("# Only a title\n");
    assert_eq!(hit_at(&layout, (500.0, 600.0)), Hit::Placeholder("body".to_string()));
    let layout = self::layout("Body only\n");
    assert_eq!(hit_at(&layout, (500.0, 100.0)), Hit::Placeholder("title".to_string()));
    let layout = self::layout("---\nlayout: two-columns\n---\n\n# T\n\nLeft\n");
    assert_eq!(hit_at(&layout, (1400.0, 600.0)), Hit::Placeholder("right".to_string()));
}

#[test]
fn a_click_on_the_background_is_nothing() {
    let layout = layout("# T\n\nBody\n");
    assert_eq!(hit_at(&layout, (10.0, 1070.0)), Hit::Nothing);
    assert_eq!(layout.hit(f32::NAN, 10.0), None);
}

#[test]
fn a_selection_over_two_lines_is_two_rects() {
    let layout = layout("# T\n\nOne\nTwo\n");
    let rects = layout.selection_rects(1, at(0, 1), at(0, 6)).rects;
    assert_eq!(rects.len(), 2, "{rects:?}");
    assert!(rects[1].y > rects[0].y);
    let (one, three) = (caret(&layout, 1, at(0, 1)), caret(&layout, 1, at(0, 3)));
    assert!(near(rects[0].x, one.x, 0.01));
    assert!(rects[0].x + rects[0].w >= three.x, "the first runs to the line's end, and over its break");
    let (two_start, six) = (caret(&layout, 1, at(0, 4)), caret(&layout, 1, at(0, 6)));
    assert!(near(rects[1].x, two_start.x, 0.01));
    assert!(near(rects[1].x + rects[1].w, six.x, 0.01));
    assert_eq!(layout.selection_rects(1, at(0, 6), at(0, 1)).rects, rects, "either way round");
    assert!(layout.selection_rects(1, at(0, 2), at(0, 2)).rects.is_empty());
}

#[test]
fn up_and_down_keep_the_column() {
    let layout = layout("# T\n\nLong line here\nab\nLong line here\n");
    let from = at(0, 10);
    let x = caret(&layout, 1, from).x;
    let down = layout.line_move(1, from, LineMove::Down, None);
    assert_eq!(down, at(0, 17), "the short line's end");
    let down_again = layout.line_move(1, down, LineMove::Down, Some(x));
    assert_eq!(down_again, at(0, 28), "back in the column it started in");
    let up = layout.line_move(1, down_again, LineMove::Up, Some(x));
    assert_eq!(layout.line_move(1, up, LineMove::Up, Some(x)), from);
    assert_eq!(layout.line_move(1, from, LineMove::LineStart, None), at(0, 0));
    assert_eq!(layout.line_move(1, from, LineMove::LineEnd, None), at(0, 14));
    assert_eq!(layout.line_move(1, at(0, 2), LineMove::Up, None), at(0, 0), "up from the first line goes to its start");
    assert_eq!(layout.line_move(1, at(0, 20), LineMove::Down, None), at(0, 32), "down from the last goes to its end");
}

#[test]
fn up_and_down_cross_paragraphs() {
    let layout = layout("# T\n\nFirst\n\nSecond\n");
    assert_eq!(layout.line_move(1, at(0, 2), LineMove::Down, None).para, 1);
    assert_eq!(layout.line_move(1, at(1, 2), LineMove::Up, None).para, 0);
}

/// Slides covering every layout, an unknown one, a region marker, boxes of each kind,
/// lists, breaks, wrapping, styles, and characters made of more than one code point.
fn mixed_slides() -> Vec<String> {
    let text = long_text();
    let mut slides = vec![
        "---\nlayout: title\n---\n\n# A talk\n\nBy someone\n".to_string(),
        format!("---\nlayout: content\n---\n\n# Content\n\n{text}\n\n- One\n- Two **bold**\n  - Nested *it*\n- \n\n1. First\n2. Second\n"),
        "---\nlayout: two-columns\n---\n\n# Two\n\nOn the left\n\n<Region name=\"right\" />\n\nOn the right\\\nwith a break\n".to_string(),
        "---\nlayout: section\n---\n\n# Part two\n\nWhat comes next\n".to_string(),
        "---\nlayout: blank\n---\n\nAll over the slide\n\n## A heading\n".to_string(),
        "---\nlayout: nonsense\n---\n\n# Unknown\n\nIn the body\n".to_string(),
        "# Characters\n\nCafé e\u{301}t\u{e9} 👍🏽 office `code` [a link](https://example.com)\n\nSplit\u{2029}apart and \u{85}again\n".to_string(),
        "<Text x=\"100\" y=\"100\" w=\"700\" h=\"300\" rotate=\"12\">\n\nA turned box\nof text\n\n</Text>\n\n<Box x=\"900\" y=\"100\" w=\"700\" h=\"300\" fill=\"#dbeafe\">\n\nIn a box\n\n- and a list\n\n</Box>\n\n<Shape x=\"300\" y=\"600\" w=\"600\" h=\"300\" kind=\"ellipse\" fill=\"#fde68a\">\n\nIn a shape\n\n</Shape>\n".to_string(),
        "---\nregion.body: align=right\n---\n\n# Right\n\nTo the right\n\n<Text x=\"100\" y=\"700\" w=\"800\" h=\"200\" align=\"center\">\n\nCentred box\n\n</Text>\n".to_string(),
    ];
    slides.push("# Each\n\n- one\n- two\n\nAnd more\n".replace("# Each", "---\nregion.body: click=each\n---\n\n# Each"));
    // Empty paragraphs and breaks at a paragraph's edges, as the format crate writes them.
    slides.push("# Edges\n\n**<Text></Text>**\n\nMiddle\n\n**<Text></Text>**\n\nEnds in a break\\\n**<Text></Text>**\n\n\\\nStarts with one\n\n**<Text></Text>**\n".to_string());
    slides.push("<Text x=\"1000\" y=\"500\" w=\"700\" h=\"300\">\n\n**<Text></Text>**\n\nA *break\\\n<Text></Text>*\n\n</Text>\n\n- \n- item\\\n**<Text></Text>**\n".to_string());
    slides
}

#[test]
fn geometry_agrees_with_the_model() {
    for source in mixed_slides() {
        let mut deck = deck(&source);
        let layout = deck.text_layout(0);
        let flows = text_flows(deck.talk(), 0);
        assert!(!flows.is_empty(), "{source}");
        for (f, flow) in flows.iter().enumerate() {
            for p in 0..flow.paras.len() {
                assert!(layout.paras().iter().any(|placed| placed.flow == f && placed.para == p), "flow {f} paragraph {p} isn't placed in {source}");
                for offset in edges(&source, f, p) {
                    let caret = caret(&layout, f, at(p, offset));
                    assert_eq!(hit_at(&layout, middle(&caret)), Hit::Text { flow: f, pos: at(p, offset) }, "{offset} of flow {f} paragraph {p} in {source}");
                }
            }
            if let FlowPlace::Tag(range) = &flow.place {
                let layer = layout.paras().iter().find(|placed| placed.flow == f).unwrap().layer;
                assert_eq!(deck.specs(0)[layer].source.as_ref(), Some(range));
            }
        }
    }
}

#[test]
fn an_empty_paragraph_has_a_caret_as_tall_as_its_text() {
    let layout = layout("# T\n\n- \n");
    let caret = caret(&layout, 1, at(0, 0));
    assert!(near(caret.height, 50.0, 0.5), "{caret:?}");
    assert!(caret.x >= 148.0 - 0.5);
    assert_eq!(hit_at(&layout, middle(&caret)), Hit::Text { flow: 1, pos: at(0, 0) });
}

#[test]
fn a_caret_past_what_the_talk_holds_goes_on_a_new_line() {
    let layout = layout("# T\n\nOne\n");
    let end = caret(&layout, 1, at(0, 3));
    let after_a_break = caret(&layout, 1, at(0, 4));
    assert!(near(after_a_break.x, 100.0, 0.5));
    assert!(near(after_a_break.top, end.top + end.height, 0.5));
    let new_paragraph = caret(&layout, 1, at(1, 0));
    assert!(new_paragraph.top > end.top + end.height, "{new_paragraph:?}");
}

/// The text each of `specs` draws, line by line.
fn texts_in(specs: &[LayerSpec]) -> Vec<String> {
    specs.iter().flat_map(|spec| &spec.items).filter_map(|item| match item {
        DisplayItem::Text { text, .. } => Some(text.buffer.layout_runs().map(|run| run.text.to_string()).collect::<Vec<_>>().join("\n")),
        _ => None,
    }).collect()
}

#[test]
fn empty_regions_say_click_to_add_text_only_in_the_editor() {
    let mut d = deck("Body only\n\n---\n\n# Only a title\n");
    for (index, label) in [(0, "Click to add title"), (1, "Click to add text")] {
        let layers = d.layers(index, 0.5);
        let editor = d.editor_layers(index, 0.5);
        assert_eq!(editor.len(), layers.len() + 1);
        for (shown, drawn) in layers.iter().zip(&editor) {
            assert!(Arc::ptr_eq(shown, drawn), "the editor shows the slide's own layers first");
        }
        assert_eq!(texts_in(&d.placeholder_specs(index)), vec![label.to_string()]);
        let everything = d.specs(index);
        assert!(texts_in(everything).iter().all(|text| !text.contains("Click to add")));
        assert_eq!(d.layers_at(index, Moment::START, 0.5).len(), layers.len());
        let thumbnail = d.thumbnail(index, 960).unwrap();
        let mut composed = Pixmap::new(960, 540).unwrap();
        compose(&mut composed, &layers.iter().map(|layer| Layer { pixmap: layer.pixmap.clone(), x: layer.x, y: layer.y }).collect::<Vec<_>>());
        assert_eq!(thumbnail.data(), composed.data(), "the thumbnail has no placeholder");
        let mut with_placeholder = composed.clone();
        let last = editor.last().unwrap();
        compose(&mut with_placeholder, &[Layer { pixmap: last.pixmap.clone(), x: last.x, y: last.y }]);
        assert_ne!(with_placeholder.data(), composed.data(), "the placeholder draws something");
    }
    assert_eq!(d.editor_layers(0, 0.5).len(), d.editor_layers(0, 0.5).len());
    assert!(Arc::ptr_eq(d.editor_layers(0, 0.5).last().unwrap(), d.editor_layers(0, 0.5).last().unwrap()), "drawn once");
    assert!(d.placeholder_specs(0).iter().all(|spec| spec.source.is_none()));
    let mut full = deck("# Title\n\nBody\n");
    assert_eq!(full.editor_layers(0, 0.5).len(), full.layers(0, 0.5).len(), "nothing to add on a full slide");
}

#[test]
fn a_region_can_be_centred_from_its_settings_line() {
    let layout = layout("---\nregion.body: align=center\n---\n\n# T\n\nHi\n");
    let (start, end) = (caret(&layout, 1, at(0, 0)), caret(&layout, 1, at(0, 2)));
    assert!(near((start.x + end.x) / 2.0, 960.0, 1.0), "centred on {}", (start.x + end.x) / 2.0);
    assert!(near(caret(&layout, 0, at(0, 0)).x, 100.0, 0.5), "the title is as it was");
    let right = self::layout("---\nregion.body: x=100 align=right\n---\n\n# T\n\nHi\n");
    assert!(near(caret(&right, 1, at(0, 2)).x, 1820.0, 0.5));
}

#[test]
fn a_box_can_be_right_aligned() {
    let layout = layout("<Text x=\"100\" y=\"100\" w=\"800\" h=\"200\" align=\"right\">\n\nHi\n\n</Text>\n");
    assert!(near(caret(&layout, 0, at(0, 2)).x, 900.0, 0.5));
    assert!(caret(&layout, 0, at(0, 0)).x > 800.0);
    let left = self::layout("<Text x=\"100\" y=\"100\" w=\"800\" h=\"200\">\n\nHi\n\n</Text>\n");
    assert!(near(caret(&left, 0, at(0, 0)).x, 100.0, 0.5));
}

#[test]
fn each_paragraph_knows_the_size_and_alignment_it_is_drawn_with() {
    let layout = layout("---\nregion.body: align=center\n---\n\n# T\n\nHi <Text size=\"80\">big</Text>\n\n- item\n");
    assert_eq!(layout.text_size(0, 0), Some(64.0), "a title");
    assert_eq!(layout.text_size(1, 0), Some(40.0), "the body, whatever its spans ask for");
    assert_eq!(layout.text_size(1, 1), Some(40.0), "a list item");
    assert_eq!(layout.text_size(1, 2), None, "no such paragraph");
    assert_eq!(layout.text_size(7, 0), None, "no such flow");
    assert_eq!(layout.align(0), Some(Align::Left));
    assert_eq!(layout.align(1), Some(Align::Center));
    assert_eq!(layout.align(2), None);
    let boxed = self::layout("<Text x=\"100\" y=\"100\" w=\"800\" h=\"200\" align=\"right\">\n\nHi\n\n</Text>\n");
    assert_eq!(boxed.align(0), Some(Align::Right));
    let title = self::layout("---\nlayout: title\n---\n\n# Big\n");
    assert_eq!((title.text_size(0, 0), title.align(0)), (Some(96.0), Some(Align::Center)));
}

#[test]
fn a_caret_in_an_empty_region_is_as_tall_as_its_text_where_its_text_starts() {
    let layout = layout("# Only a title\n");
    let (rect, _) = layout.placeholder("body").expect("the body is empty");
    assert_eq!(layout.placeholder_text("body"), Some((40.0, Align::Left)));
    let caret = layout.placeholder_caret("body").expect("a caret in the body");
    // As a caret in an empty paragraph typed there would be.
    let typed = self::layout("# Only a title\n\n- \n");
    let empty_line = self::caret(&typed, 1, at(0, 0));
    assert!(near(caret.height, empty_line.height, 0.5), "{caret:?} against {empty_line:?}");
    assert!(near(caret.x, rect.x, 0.01) && near(caret.top, rect.y, 0.01), "{caret:?}");
    assert_eq!(caret.rotate, 0.0);

    let centred = self::layout("---\nregion.body: align=center\n---\n\n# T\n");
    let (rect, _) = centred.placeholder("body").unwrap();
    assert_eq!(centred.placeholder_text("body"), Some((40.0, Align::Center)));
    assert!(near(centred.placeholder_caret("body").unwrap().x, rect.x + rect.w / 2.0, 0.01));
    let title = self::layout("---\nlayout: title\n---\n\nSubtitle\n");
    assert_eq!(title.placeholder_text("title"), Some((96.0, Align::Center)));
    let caret = title.placeholder_caret("title").unwrap();
    assert!(near(caret.height, 120.0, 0.5), "{caret:?}");
    assert_eq!(layout.placeholder_caret("title"), None, "the title has text");
}

#[test]
fn list_markers_are_not_part_of_the_text() {
    let layout = layout("# T\n\n- One\n- Two\n\n1. Three\n");
    for para in 0..3 {
        let start = caret(&layout, 1, at(para, 0));
        assert!(start.x >= 148.0 - 0.5, "paragraph {para} starts at {}", start.x);
    }
    let first = caret(&layout, 1, at(0, 0));
    assert_eq!(hit_at(&layout, (first.x + 1.0, first.top + 5.0)), Hit::Text { flow: 1, pos: at(0, 0) });
}

#[test]
fn a_moved_slide_finds_its_text_where_a_fresh_deck_does() {
    let talk = "# One\n\nFirst\n\n---\n\n# Two\n\n- a\n- b\n\n<Text x=\"100\" y=\"700\" w=\"500\">\n\nBoxed\n\n</Text>\n";
    let mut d = deck(talk);
    d.layers(1, 0.5);
    let edited = talk.replace("First", "First, made longer");
    assert_eq!(d.update_text(slides_format::parse_talk(&edited)), vec![0]);
    let mut fresh = deck(&edited);
    assert_eq!(d.drawn_text_sources(1), fresh.drawn_text_sources(1));
    let sources = |deck: &mut Deck| deck.specs(1).iter().map(|spec| spec.source.clone()).collect::<Vec<_>>();
    assert_eq!(sources(&mut d), sources(&mut fresh));
    assert_eq!(*d.text_layout(1), *fresh.text_layout(1));
    assert_eq!(d.text_layout(1).paras().len(), 4);
}

#[test]
fn the_layout_is_kept_until_the_slide_changes() {
    let mut d = deck("# One\n\n---\n\n# Two\n");
    let before = d.text_layout(1);
    assert!(Arc::ptr_eq(&before, &d.text_layout(1)));
    d.update_text(slides_format::parse_talk("# One changed\n\n---\n\n# Two\n"));
    assert!(Arc::ptr_eq(&before, &d.text_layout(1)), "another slide's edit keeps it");
    d.update_text(slides_format::parse_talk("# One changed\n\n---\n\n# Two changed\n"));
    assert!(!Arc::ptr_eq(&before, &d.text_layout(1)));
    assert_eq!(d.text_layout(5).paras().len(), 0, "a slide that isn't there has no text");
}

#[test]
fn a_click_in_a_box_away_from_its_text_goes_to_the_nearest_paragraph() {
    let layout = layout("<Box x=\"100\" y=\"100\" w=\"800\" h=\"400\" fill=\"#dbeafe\">\n\nFirst\n\nSecond\n\n</Box>\n");
    let second = layout.paras().iter().find(|placed| placed.para == 1).unwrap().rect;
    assert_eq!(hit_at(&layout, (850.0, 480.0)), Hit::Text { flow: 0, pos: at(1, "Second".len()) });
    assert!(second.y + second.h < 480.0);
    assert_eq!(hit_at(&layout, (950.0, 480.0)), Hit::Placeholder("body".to_string()), "outside the box");
}

#[test]
fn a_click_on_a_component_says_why() {
    let layout = layout("# T\n\n<Box fill=\"#dbeafe\">\n\nBoxed but not placed\n\n</Box>\n\n<Text x=\"1000\" y=\"600\" w=\"400\" h=\"200\">\n\n| a |\n|---|\n| b |\n\n</Text>\n");
    assert_eq!(hit_at(&layout, (300.0, 250.0)), Hit::Uneditable("Text inside a component can't be edited on the slide yet."));
    assert_eq!(hit_at(&layout, (1100.0, 620.0)), Hit::Uneditable("Tables can't be edited on the slide yet."));
    assert_eq!(hit_at(&layout, (1350.0, 780.0)), Hit::Uneditable("Text in a box that holds more than text can't be edited on the slide yet."));
}

#[test]
fn an_empty_paragraph_is_a_line_with_a_caret() {
    let source = "# T\n\nFirst\n\n**<Text></Text>**\n\nThird\n";
    let mut d = deck(source);
    let layout = d.text_layout(0);
    let body: Vec<_> = layout.paras().iter().filter(|placed| placed.flow == 1).collect();
    assert_eq!(body.iter().map(|placed| placed.para).collect::<Vec<_>>(), vec![0, 1, 2], "every paragraph is placed, in order");
    let (first, empty, third) = (caret(&layout, 1, at(0, 0)), caret(&layout, 1, at(1, 0)), caret(&layout, 1, at(2, 0)));
    assert!(near(empty.height, 50.0, 0.5), "{empty:?}");
    assert!(near(empty.x, 100.0, 0.5));
    assert!(near(empty.top, first.top + first.height + 24.0, 0.5), "{empty:?}");
    assert!(near(third.top, empty.top + empty.height + 24.0, 0.5), "{third:?}");
    assert!(near(body[1].rect.h, 50.0, 0.5), "the empty line is drawn as tall as the region's text");
    assert_eq!(hit_at(&layout, middle(&empty)), Hit::Text { flow: 1, pos: at(1, 0) });
    assert_eq!(hit_at(&layout, (900.0, empty.top + 10.0)), Hit::Text { flow: 1, pos: at(1, 0) });
    let texts = texts_in(d.specs(0));
    assert!(texts.iter().all(|text| !text.contains("Text")), "nothing of its spelling is drawn: {texts:?}");
    let without = self::layout("# T\n\nFirst\n\nThird\n");
    assert!(near(caret(&without, 1, at(1, 0)).top + 74.0, third.top, 0.5));
}

#[test]
fn a_break_at_the_end_has_a_caret_on_its_own_line() {
    let layout = layout("# T\n\nab\\\n**<Text></Text>**\n\nNext\n");
    let (end, after) = (caret(&layout, 1, at(0, 2)), caret(&layout, 1, at(0, 3)));
    assert!(near(after.top, end.top + end.height, 0.5), "{after:?}");
    assert!(near(after.x, 100.0, 0.5));
    assert_eq!(hit_at(&layout, middle(&after)), Hit::Text { flow: 1, pos: at(0, 3) });
    let placed = layout.paras().iter().find(|placed| placed.flow == 1 && placed.para == 0).unwrap();
    assert!(near(placed.rect.h, 100.0, 0.5), "two lines are drawn: {placed:?}");
    let next = caret(&layout, 1, at(1, 0));
    assert!(near(next.top, after.top + after.height + 24.0, 0.5), "the next paragraph is below the empty line");
}

#[test]
fn a_click_between_paragraphs_goes_to_the_nearest() {
    let layout = layout("# T\n\nFirst\n\nSecond\n");
    let (first, second) = (caret(&layout, 1, at(0, 0)), caret(&layout, 1, at(1, 0)));
    let gap = first.top + first.height + 4.0;
    assert!(gap < second.top);
    assert_eq!(hit_at(&layout, (100.0, gap)), Hit::Text { flow: 1, pos: at(0, 0) });
    assert_eq!(hit_at(&layout, (100.0, second.top - 4.0)), Hit::Text { flow: 1, pos: at(1, 0) });
}

#[test]
fn a_click_on_a_marker_goes_to_its_items_start() {
    let layout = layout("# T\n\n- One\n- Two\n\n1. Three\n");
    let two = caret(&layout, 1, at(1, 0));
    assert_eq!(hit_at(&layout, (110.0, two.top + 20.0)), Hit::Text { flow: 1, pos: at(1, 0) });
    let three = caret(&layout, 1, at(2, 0));
    assert_eq!(hit_at(&layout, (105.0, three.top + 20.0)), Hit::Text { flow: 1, pos: at(2, 0) });
}

#[test]
fn a_click_below_the_last_paragraph_goes_to_it() {
    let layout = layout("# T\n\nFirst\n\nLast\n");
    assert_eq!(hit_at(&layout, (1500.0, 900.0)), Hit::Text { flow: 1, pos: at(1, 4) });
    assert_eq!(hit_at(&layout, (50.0, 900.0)), Hit::Nothing, "outside the region");
    let table = self::layout("# T\n\nText\n\n| a | b |\n|---|---|\n| 1 | 2 |\n");
    assert_eq!(hit_at(&table, (300.0, 320.0)), Hit::Uneditable("Tables can't be edited on the slide yet."), "the table still says why");
}

#[test]
fn a_turned_boxs_selection_turns_with_it() {
    let layout = layout("<Text x=\"600\" y=\"300\" w=\"600\" h=\"200\" rotate=\"30\">\n\nHello world\n\n</Text>\n");
    let selection = layout.selection_rects(0, at(0, 0), at(0, 5));
    assert_eq!((selection.rotate, selection.centre), (30.0, (900.0, 400.0)));
    assert_eq!(selection.rects.len(), 1);
    assert!(layout.selection_rects(7, at(0, 0), at(0, 5)).rects.is_empty());
}

#[test]
fn a_caret_far_past_the_text_is_none() {
    let layout = layout("# T\n\nOne\n");
    assert!(layout.caret(1, at(0, 5)).is_none());
    assert!(layout.caret(1, at(2, 0)).is_none());
    assert!(layout.caret(1, at(1, 3)).is_none());
    assert!(layout.caret(9, at(0, 0)).is_none());
    let mut d = deck("# T\n");
    assert!(d.editor_layers(4, 0.5).is_empty());
    assert!(d.placeholder_specs(4).is_empty());
    assert!(d.drawn_text_sources(4).is_empty());
}

#[test]
fn align_where_it_does_nothing_says_so() {
    let said = "Only a box placed on the slide can be aligned, so align is ignored here".to_string();
    for source in ["<Box align=\"right\">\n\nHi\n\n</Box>\n", "Hi <Text align=\"right\">there</Text>\n", "- Hi <Text bold align=\"center\">there</Text>\n"] {
        assert!(deck(source).notices_whole(0).contains(&said), "{source}");
    }
    assert!(!deck("<Text x=\"10\" y=\"10\" w=\"500\" align=\"right\">\n\nHi\n\n</Text>\n").notices_whole(0).contains(&said));
    let layout = layout("---\nregion.body: align=center align=right\n---\n\n# T\n\nHi\n");
    let (start, end) = (caret(&layout, 1, at(0, 0)), caret(&layout, 1, at(0, 2)));
    assert!(near((start.x + end.x) / 2.0, 960.0, 1.0), "the first align wins");
}

#[test]
fn the_alignment_asked_for_is_told_whatever_a_list_is_drawn_at() {
    let region = |name: &str| FlowPlace::Region(name.to_string());
    let talk = |source: &str| slides_format::parse_talk(source);
    let plain = talk("# T\n\n- One\n");
    assert_eq!(text_align(&plain, 0, &region("body")), Some(Align::Left));
    assert_eq!(text_align(&plain, 0, &region("left")), None, "no such region in this layout");
    assert_eq!(text_align(&plain, 3, &region("body")), None, "no such slide");
    let centred = talk("---\nregion.body: x=10 align=center\n---\n\n# T\n\n- One\n");
    assert_eq!(text_align(&centred, 0, &region("body")), Some(Align::Center));
    assert_eq!(text_align(&centred, 0, &region("title")), Some(Align::Left));
    let title = talk("---\nlayout: title\n---\n\n# Big\n");
    assert_eq!(text_align(&title, 0, &region("title")), Some(Align::Center), "as the layout has it");
    let boxes = talk("<Text x=\"1\" align=\"right\">\n\nHi\n\n</Text>\n\n<Text x=\"2\">\n\nHo\n\n</Text>\n");
    let places: Vec<FlowPlace> = text_flows(&boxes, 0).into_iter().map(|flow| flow.place).collect();
    assert_eq!(text_align(&boxes, 0, &places[0]), Some(Align::Right));
    assert_eq!(text_align(&boxes, 0, &places[1]), Some(Align::Left));
    assert_eq!(text_align(&boxes, 0, &FlowPlace::Tag(0..1)), None);
}
