mod common;
use slides_engine::*;
use slides_format::parse_talk;

struct NoFiles;
impl TalkFiles for NoFiles { fn read(&self, _: &str) -> Option<Vec<u8>> { None } }

fn lay_out(source: &str) -> (LaidOutSlide, Fonts) {
    let mut fonts = Fonts::bundled_only();
    let talk = parse_talk(source);
    let registry = builtin_registry();
    let slide = layout_slide(&talk, 0, &mut LayoutEnv { fonts: &mut fonts, registry: &registry, files: &NoFiles, pictures: &mut PictureCache::default(), components: &mut TalkComponents::none(), moment: Moment::START, asks: ComponentAsks::default() });
    (slide, fonts)
}

fn source_of<'a>(source: &'a str, layer: &LayerSpec) -> &'a str { &source[layer.source.clone().unwrap()] }

/// `(x, y, height)` of each piece of text in a layer, in drawing order.
fn text_boxes(layer: &LayerSpec) -> Vec<(f32, f32, f32)> {
    layer.items.iter().filter_map(|item| match item {
        DisplayItem::Text { text, x, y } => Some((*x, *y, text.height)),
        _ => None,
    }).collect()
}

fn first_text(layer: &LayerSpec) -> &ShapedText {
    layer.items.iter().find_map(|item| match item {
        DisplayItem::Text { text, .. } => Some(text),
        _ => None,
    }).expect("the layer has text")
}

#[test]
fn first_heading_goes_to_the_title_region() {
    let src = "# Title here\n\nBody text\n";
    let (slide, _) = lay_out(src);
    assert_eq!(slide.layers[0].source, None);          // the background
    assert_eq!(slide.layers[1].rect, Rect { x: 100.0, y: 60.0, w: 1720.0, h: 140.0 });
    assert!(source_of(src, &slide.layers[1]).contains("Title here"));
    assert!(source_of(src, &slide.layers[2]).contains("Body text"));
}

#[test]
fn region_marker_sends_what_follows_to_that_region() {
    let src = "---\nlayout: two-columns\n---\n\n# T\n\nLeft\n\n<Region name=\"right\" />\n\nRight\n";
    let (slide, _) = lay_out(src);
    let right = slide.layers.iter().find(|l| l.rect.x == 980.0).unwrap();
    assert!(source_of(src, right).contains("Right"));
}

#[test]
fn a_moved_region_uses_its_settings_line() {
    let src = "---\nregion.title: x=10 y=20 w=500 h=100 rotate=3\n---\n\n# T\n";
    let (slide, _) = lay_out(src);
    assert_eq!(slide.layers[1].rect, Rect { x: 10.0, y: 20.0, w: 500.0, h: 100.0 });
    assert_eq!(slide.layers[1].rotate, 3.0);
}

#[test]
fn an_unknown_layout_uses_content_and_says_so() {
    let (slide, _) = lay_out("---\nlayout: fancy\n---\n\n# T\n");
    assert_eq!(slide.layers[1].rect.y, 60.0);
    assert!(slide.notices[0].0.contains("fancy"));
}

#[test]
fn background_uses_the_talks_colour() {
    let src = "---\ncolor.background: #123456\n---\n\n# T\n";
    let talk = parse_talk(src);
    assert_eq!(talk_theme(&talk).background, toolkit::Color::parse("#123456").unwrap());
}

#[test]
fn rust_keywords_get_their_own_colour() {
    let lines = highlight("fn main() {}\n", Some("rust"));
    let colours: std::collections::HashSet<_> = lines[0].iter().map(|(c, _)| (c.r, c.g, c.b)).collect();
    assert!(colours.len() >= 2);
}

#[test]
fn an_unknown_language_still_comes_back_as_text() {
    let lines = highlight("hello\n", Some("not-a-language"));
    assert_eq!(lines[0].iter().map(|(_, t)| t.as_str()).collect::<String>().trim_end(), "hello");
}

#[test]
fn golden_markdown_slide() {
    let src = "# A heading\n\nA paragraph with **bold**, *italic* and <Text color=\"red\">red</Text> words.\n\n- First\n- Second\n\n| Name | Score |\n|------|-------|\n| Oslo | 3 |\n\n```rust\nfn main() {}\n```\n";
    let (slide, mut fonts) = lay_out(src);
    let layers: Vec<_> = slide.layers.iter().map(|l| render_layer(l, 1.0, &mut fonts)).collect();
    let mut target = tiny_skia::Pixmap::new(1920, 1080).unwrap();
    compose(&mut target, &layers);
    common::assert_golden("markdown", &target);
}

#[test]
fn every_layout_has_the_regions_of_the_table() {
    let places = |name: &str| -> Vec<(&str, f32, f32, f32, f32)> {
        layout_by_name(name).unwrap().regions.iter().map(|r| (r.name, r.rect.x, r.rect.y, r.rect.w, r.rect.h)).collect()
    };
    assert_eq!(places("title"), [("title", 160.0, 340.0, 1600.0, 220.0), ("body", 160.0, 580.0, 1600.0, 180.0)]);
    assert_eq!(places("content"), [("title", 100.0, 60.0, 1720.0, 140.0), ("body", 100.0, 220.0, 1720.0, 780.0)]);
    assert_eq!(places("two-columns"), [("title", 100.0, 60.0, 1720.0, 140.0), ("left", 100.0, 220.0, 840.0, 780.0), ("right", 980.0, 220.0, 840.0, 780.0)]);
    assert_eq!(places("section"), [("title", 160.0, 420.0, 1600.0, 240.0), ("body", 160.0, 680.0, 1600.0, 200.0)]);
    assert_eq!(places("blank"), [("body", 0.0, 0.0, 1920.0, 1080.0)]);
    assert_eq!(layout_by_name("two-columns").unwrap().main, "left");
    assert!(layout_by_name("fancy").is_none());
}

#[test]
fn a_region_with_nothing_in_it_has_no_layer() {
    let (slide, _) = lay_out("# Only a title\n");
    assert_eq!(slide.layers.len(), 2);
}

#[test]
fn a_layer_spans_its_first_to_its_last_block() {
    let src = "# T\n\nOne\n\nTwo\n";
    let (slide, _) = lay_out(src);
    assert_eq!(source_of(src, &slide.layers[2]), "One\n\nTwo");
}

#[test]
fn only_the_first_heading_is_the_title() {
    let src = "# First\n\n## Second\n";
    let (slide, _) = lay_out(src);
    assert_eq!(source_of(src, &slide.layers[1]), "# First");
    assert_eq!(source_of(src, &slide.layers[2]), "## Second");
}

#[test]
fn a_heading_after_a_region_marker_stays_in_that_region() {
    let src = "---\nlayout: two-columns\n---\n\n<Region name=\"right\" />\n\n# Over here\n";
    let (slide, _) = lay_out(src);
    assert_eq!(slide.layers.len(), 2);
    assert_eq!(slide.layers[1].rect.x, 980.0);
}

#[test]
fn an_unknown_region_falls_back_to_the_main_one_and_says_so() {
    let (slide, _) = lay_out("<Region name=\"nope\" />\n\nText\n");
    assert_eq!(slide.layers[1].rect.y, 220.0);
    assert!(slide.notices[0].0.contains("nope"));
}

#[test]
fn a_region_line_keeps_what_it_omits_and_what_it_cannot_read() {
    let (slide, _) = lay_out("---\nregion.title: w=500 x=oops y=NaN\n---\n\n# T\n");
    assert_eq!(slide.layers[1].rect, Rect { x: 100.0, y: 60.0, w: 500.0, h: 140.0 });
}

#[test]
fn region_values_are_clamped_to_sensible_limits() {
    let (slide, _) = lay_out("---\nregion.title: x=99999 w=99999999\n---\n\n# T\n");
    assert_eq!(slide.layers[1].rect.x, 7680.0);
    assert_eq!(slide.layers[1].rect.w, 3840.0);

    let (slide, _) = lay_out("---\nregion.title: x=-99999 y=-99999 h=-5 rotate=9000\n---\n\n# T\n");
    assert_eq!(slide.layers[1].rect, Rect { x: -3840.0, y: -3840.0, w: 1720.0, h: 0.0 });
    assert_eq!(slide.layers[1].rotate, 360.0);
}

#[test]
fn long_text_is_not_cut_off_at_the_region_edge() {
    let (slide, mut fonts) = lay_out(&format!("# {}\n", "Overflowing title words ".repeat(12)));
    let text_height = first_text(&slide.layers[1]).height;
    assert!(text_height > 140.0, "the heading has to overflow its region");
    let layer = render_layer(&slide.layers[1], 1.0, &mut fonts);
    assert_eq!(layer.y, 60);
    // The pixmap always had 1 spare row, so being taller than 140 alone would prove nothing.
    assert!(layer.pixmap.height() as f32 >= text_height);
}

#[test]
fn blocks_stack_from_the_layers_corner_with_a_gap() {
    let (slide, _) = lay_out("# T\n\nOne\n\nTwo\n\nThree\n");
    let boxes = text_boxes(&slide.layers[2]);
    assert_eq!((boxes[0].0, boxes[0].1), (0.0, 0.0));
    for pair in boxes.windows(2) {
        assert_eq!(pair[1].1, pair[0].1 + pair[0].2 + 24.0);
    }
}

#[test]
fn long_text_wraps_inside_its_column() {
    let (slide, _) = lay_out(&format!("---\nlayout: two-columns\n---\n\n{}\n", "word ".repeat(200)));
    let text = first_text(&slide.layers[1]);
    assert!(text.width <= 840.0);
    assert!(text.height > 50.0);
}

#[test]
fn list_text_sits_past_its_marker() {
    let (slide, _) = lay_out("- One\n- Two\n");
    let boxes = text_boxes(&slide.layers[1]);
    assert_eq!(boxes.len(), 4);
    assert_eq!((boxes[0].0, boxes[1].0), (0.0, 48.0));
    assert_eq!(boxes[0].1, boxes[1].1);
}

#[test]
fn two_digit_markers_do_not_touch_their_text() {
    let list: String = (1..=10).map(|n| format!("{n}. Item\n")).collect();
    let (slide, _) = lay_out(&list);
    let pieces: Vec<(f32, f32)> = slide.layers[1].items.iter().filter_map(|item| match item {
        DisplayItem::Text { text, x, .. } => Some((*x, text.width)),
        _ => None,
    }).collect();
    assert_eq!(pieces.len(), 20);
    let (marker, body) = (pieces[18], pieces[19]);
    assert!(body.0 >= marker.0 + marker.1 + 12.0 - 0.01, "the marker ends at {} and its text starts at {}", marker.0 + marker.1, body.0);
    assert_eq!(pieces[1].0, body.0, "every item in a list has the same indent");

    let (short, _) = lay_out("1. One\n2. Two\n");
    assert_eq!(text_boxes(&short.layers[1])[1].0, 48.0, "a single digit keeps the usual indent");
}

#[test]
fn a_nested_list_indents_again() {
    let (slide, _) = lay_out("- One\n  - Two\n");
    let boxes = text_boxes(&slide.layers[1]);
    assert_eq!(boxes.last().unwrap().0, 96.0);
}

#[test]
fn a_table_outlines_each_cell() {
    let (slide, _) = lay_out("| A | B |\n|---|---|\n| 1 | 2 |\n");
    let outlines = slide.layers[1].items.iter().filter(|i| matches!(i, DisplayItem::Stroke { width: 1.0, .. })).count();
    assert_eq!(outlines, 4);
}

#[test]
fn code_sits_on_its_panel_inside_the_padding() {
    let (slide, _) = lay_out("```\nlet a = 1;\n```\n");
    let layer = &slide.layers[1];
    assert!(matches!(layer.items[0], DisplayItem::Fill { .. }));
    let boxes = text_boxes(layer);
    assert_eq!((boxes[0].0, boxes[0].1), (24.0, 24.0));
}

#[test]
fn code_measured_inside_its_padding_wraps_the_way_it_is_drawn() {
    let long_line = "word ".repeat(100);
    let (slide, _) = lay_out(&format!("```\n{long_line}\n```\n\nAfter\n"));
    let boxes = text_boxes(&slide.layers[1]);
    assert_eq!(boxes[1].1, 24.0 + boxes[0].2 + 24.0 + 24.0);
}

#[test]
fn each_line_of_code_comes_back_on_its_own() {
    assert_eq!(highlight("a\n\nb", None).len(), 3);
}

#[test]
fn a_link_is_underlined_in_the_accent_colour() {
    let (slide, _) = lay_out("[site](https://example.test)\n");
    assert_eq!(first_text(&slide.layers[1]).underlines[0].3, toolkit::Theme::default().accent);
}

#[test]
fn a_colour_that_cannot_be_read_falls_back_to_the_text_colour() {
    let (slide, _) = lay_out("<Text color=\"nonsense\">words</Text> here\n");
    let text = first_text(&slide.layers[1]);
    let run = text.buffer.layout_runs().next().unwrap();
    let colour = run.glyphs[0].color_opt.unwrap();
    let expected = toolkit::Theme::default().text;
    assert_eq!((colour.r(), colour.g(), colour.b()), (expected.r, expected.g, expected.b));
}

#[test]
fn a_notice_is_only_given_once_per_slide() {
    let (slide, _) = lay_out("\u{4e2d}\n\n\u{4e2d}\n");
    assert_eq!(slide.notices.len(), 1);
}
