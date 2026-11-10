use std::collections::HashMap;

use slides_engine::*;
use slides_format::parse_talk;
use toolkit::{Color, Registry, ShapeKind, Theme};

struct MapFiles(HashMap<String, Vec<u8>>);

impl TalkFiles for MapFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.0.get(path).cloned()
    }
}

fn lay_out_using(source: &str, files: &[(&str, Vec<u8>)], registry: &Registry) -> LaidOutSlide {
    let mut fonts = Fonts::bundled_only();
    let talk = parse_talk(source);
    let files = MapFiles(files.iter().map(|(path, bytes)| (path.to_string(), bytes.clone())).collect());
    layout_slide(&talk, 0, &mut LayoutEnv { fonts: &mut fonts, registry, files: &files, pictures: &mut PictureCache::default() })
}

fn lay_out_with(source: &str, files: &[(&str, Vec<u8>)]) -> LaidOutSlide {
    lay_out_using(source, files, &builtin_registry())
}

fn lay_out(source: &str) -> LaidOutSlide {
    lay_out_with(source, &[])
}

fn text_of(p: &NativeParagraph) -> String { p.runs.iter().map(|r| r.text.as_str()).collect() }

#[test]
fn a_region_of_text_is_one_text_box() {
    let slide = lay_out("# T\n\nFirst\n\n## Sub\n\n- a\n- b\n");
    let body = slide.layers.last().unwrap();
    assert_eq!(body.native.len(), 1);
    let NativeContent::Text { paragraphs, fill: None, .. } = &body.native[0].content else { panic!() };
    assert_eq!(paragraphs.iter().map(text_of).collect::<Vec<_>>(), ["First", "Sub", "a", "b"]);
    assert_eq!(paragraphs[2].bullet, Bullet::Dot);
    assert!(paragraphs[1].runs[0].bold);
}

#[test]
fn paragraphs_carry_the_line_height_and_gaps_they_are_laid_out_with() {
    let slide = lay_out("# T\n\nFirst\n\nSecond\n\n- a\n  - b\n");
    let NativeContent::Text { paragraphs, .. } = &slide.layers.last().unwrap().native[0].content else { panic!() };
    for paragraph in paragraphs {
        assert_eq!(paragraph.line_height, Some(paragraph.runs[0].size * 1.25), "{}", text_of(paragraph));
    }
    assert_eq!(paragraphs.iter().map(|p| p.space_before).collect::<Vec<_>>(), [None, Some(24.0), Some(24.0), Some(24.0)]);
}

#[test]
fn a_line_is_as_tall_as_the_biggest_run_on_it_makes_it() {
    let slide = lay_out("# T\n\nsmall <Text size=\"80\">big</Text>\n");
    let NativeContent::Text { paragraphs, .. } = &slide.layers.last().unwrap().native[0].content else { panic!() };
    assert_eq!(paragraphs[0].line_height, Some(100.0));
}

#[test]
fn code_lines_are_as_far_apart_as_they_are_drawn() {
    let slide = lay_out("# T\n\n```\none\ntwo\n```\n");
    let NativeContent::Text { paragraphs, .. } = &slide.layers.last().unwrap().native[0].content else { panic!() };
    assert_eq!(paragraphs.iter().map(|p| (p.line_height, p.space_before)).collect::<Vec<_>>(), [(Some(40.0), None), (Some(40.0), None)]);
}

#[test]
fn a_marker_has_the_colour_it_is_drawn_in() {
    let slide = lay_out("---\ncolor.text: #112233\n---\n\n# T\n\n- [a](https://example.com)\n\n1. b\n\nplain\n");
    let NativeContent::Text { paragraphs, .. } = &slide.layers.last().unwrap().native[0].content else { panic!() };
    let ink = Color::parse("#112233");
    assert_eq!(paragraphs.iter().map(|p| p.bullet_color).collect::<Vec<_>>(), [ink, ink, None]);
}

#[test]
fn only_the_title_region_is_marked_as_the_title() {
    let slide = lay_out("# T\n\nBody\n");
    assert_eq!(slide.layers.iter().map(|l| l.is_title).collect::<Vec<_>>(), [false, true, false]);
}

#[test]
fn numbered_and_nested_lists_keep_their_shape() {
    let slide = lay_out("# T\n\n1. one\n2. two\n   - inner\n");
    let NativeContent::Text { paragraphs, .. } = &slide.layers.last().unwrap().native[0].content else { panic!() };
    assert_eq!(paragraphs[1].bullet, Bullet::Number(2));
    assert_eq!((paragraphs[2].bullet, paragraphs[2].level), (Bullet::Dot, 1));
}

#[test]
fn a_table_splits_the_text_around_it() {
    let slide = lay_out("# T\n\nBefore\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nAfter\n");
    let native = &slide.layers.last().unwrap().native;
    assert_eq!(native.len(), 3);
    let NativeContent::Table { rows, column_widths, row_heights, .. } = &native[1].content else { panic!() };
    assert_eq!((rows.len(), column_widths.len(), row_heights.len()), (2, 2, 2));
    assert!(native[0].rect.y < native[1].rect.y && native[1].rect.y < native[2].rect.y);
}

#[test]
fn code_is_a_text_box_with_a_panel() {
    let slide = lay_out("# T\n\n```rust\nfn main() {}\n```\n");
    let NativeContent::Text { fill, inset, paragraphs } = &slide.layers.last().unwrap().native[0].content else { panic!() };
    assert_eq!((*fill, *inset), (Color::parse("#f1f5f9"), 24.0));
    assert_eq!(paragraphs[0].runs[0].family, "JetBrains Mono");
}

#[test]
fn a_box_with_text_is_a_shape_holding_it() {
    let slide = lay_out("<Box x=\"10\" y=\"10\" w=\"400\" h=\"200\" fill=\"red\">\n\nHello\n\n</Box>\n");
    let NativeContent::Shape { paragraphs, fill, inset, centered, .. } = &slide.layers.last().unwrap().native[0].content else { panic!() };
    assert_eq!(text_of(&paragraphs[0]), "Hello");
    assert_eq!(*fill, Color::parse("red"));
    assert_eq!((*inset, *centered), (24.0, false));
}

#[test]
fn shapes_keep_their_kind() {
    for (name, kind) in [("rect", ShapeKind::Rect), ("rounded", ShapeKind::Rounded), ("ellipse", ShapeKind::Ellipse),
                         ("line", ShapeKind::Line), ("arrow", ShapeKind::Arrow), ("callout", ShapeKind::Callout)] {
        let slide = lay_out(&format!("<Shape kind=\"{name}\" x=\"0\" y=\"0\" w=\"99\" h=\"99\" />\n"));
        assert!(matches!(&slide.layers.last().unwrap().native[0].content, NativeContent::Shape { kind: k, .. } if *k == kind), "{name}");
    }
}

#[test]
fn pictures_remember_their_file_and_icons_have_none() {
    let mut png = Vec::new();
    image::RgbaImage::new(4, 4).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let slide = lay_out_with("![A cat](a.png)\n\n<Icon name=\"arrow-right\" x=\"0\" y=\"0\" w=\"64\" h=\"64\" />\n", &[("a.png", png)]);
    let picture = slide.layers.iter().flat_map(|l| &l.native).find(|n| matches!(&n.content, NativeContent::Picture { src: Some(_), .. })).unwrap();
    assert!(matches!(&picture.content, NativeContent::Picture { src: Some(s), alt, .. } if s == "a.png" && alt == "A cat"));
    assert!(matches!(&slide.layers.last().unwrap().native[0].content, NativeContent::Picture { src: None, .. }));
}

#[test]
fn links_are_kept() {
    let slide = lay_out("# T\n\n[home](https://example.com)\n");
    let NativeContent::Text { paragraphs, .. } = &slide.layers.last().unwrap().native[0].content else { panic!() };
    assert_eq!(paragraphs[0].runs[0].link.as_deref(), Some("https://example.com"));
}

#[test]
fn an_unknown_tag_is_labelled_text() {
    let slide = lay_out("<Mystery x=\"0\" y=\"0\" w=\"99\" h=\"99\" />\n");
    assert!(matches!(&slide.layers.last().unwrap().native[0].content, NativeContent::Text { .. }));
}

// Beyond the cases the plan lists.

mod probe {
    use toolkit::prelude::*;

    #[component]
    pub fn Apart() -> Element {
        col![text("one"), text("two"), icon("arrow-right", 40.0), shape(ShapeKind::Rect, 9.0, 9.0)]
    }

    #[component]
    pub fn Traced() -> Element {
        path("M0 0 L10 10 L0 10 Z", 10.0, 10.0)
    }

    #[component]
    pub fn Divided() -> Element {
        Element::Shape {
            kind: ShapeKind::Rect,
            width: 0.0,
            height: 0.0,
            fill: None,
            outline: None,
            outline_width: 0.0,
            radius: 0.0,
            padding: 0.0,
            child: Some(Box::new(col![text("one"), text("two")])),
        }
    }
}

fn probe_registry() -> Registry {
    let mut registry = builtin_registry();
    registry.add(probe::Apart_component());
    registry.add(probe::Traced_component());
    registry.add(probe::Divided_component());
    registry
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::new(width, height).write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
    bytes
}

fn paragraphs_of(element: &NativeElement) -> &[NativeParagraph] {
    match &element.content {
        NativeContent::Text { paragraphs, .. } | NativeContent::Shape { paragraphs, .. } => paragraphs,
        _ => panic!("this element has no text"),
    }
}

fn texts_of(element: &NativeElement) -> Vec<String> {
    paragraphs_of(element).iter().map(text_of).collect()
}

/// `(y, height)` of the last piece of text a layer draws.
fn last_text(layer: &LayerSpec) -> (f32, f32) {
    layer
        .items
        .iter()
        .rev()
        .find_map(|item| match item {
            DisplayItem::Text { text, y, .. } => Some((*y, text.height)),
            _ => None,
        })
        .expect("the layer has text")
}

#[test]
fn the_background_has_no_native_content() {
    assert!(lay_out("# T\n").layers[0].native.is_empty());
}

#[test]
fn a_text_box_covers_the_text_it_holds() {
    let slide = lay_out("# T\n\nFirst\n\nSecond\n");
    let body = slide.layers.last().unwrap();
    let (y, height) = last_text(body);
    assert_eq!(body.native[0].rect, Rect { x: 0.0, y: 0.0, w: body.rect.w, h: y + height });
}

#[test]
fn a_list_text_box_starts_at_the_marker() {
    let slide = lay_out("# T\n\n- a\n");
    assert_eq!(slide.layers.last().unwrap().native[0].rect.x, 0.0);
}

#[test]
fn each_piece_of_a_split_region_holds_only_itself() {
    let source = "---\nregion.body: click=each\n---\n\n# T\n\nIntro\n\n1. one\n2. two\n   - inner\n\n| a |\n|---|\n| 1 |\n";
    let slide = lay_out(source);
    let pieces = &slide.layers[2..];
    assert_eq!(pieces.len(), 4);
    for piece in pieces {
        assert_eq!(piece.native.len(), 1);
        let rect = piece.native[0].rect;
        assert_eq!((rect.x, rect.y, rect.w, rect.h), (0.0, 0.0, piece.rect.w, piece.rect.h));
    }
    assert_eq!(texts_of(&pieces[0].native[0]), ["Intro"]);
    assert_eq!(paragraphs_of(&pieces[1].native[0])[0].bullet, Bullet::Number(1));
    assert_eq!(texts_of(&pieces[2].native[0]), ["two", "inner"]);
    let two = paragraphs_of(&pieces[2].native[0]);
    assert_eq!((two[0].bullet, two[0].level), (Bullet::Number(2), 0));
    assert_eq!((two[1].bullet, two[1].level), (Bullet::Dot, 1));
    assert!(matches!(pieces[3].native[0].content, NativeContent::Table { .. }));
}

#[test]
fn only_the_first_piece_of_a_split_title_is_the_title() {
    let slide = lay_out("---\nregion.title: click=each\n---\n\n<Region name=\"title\" />\n\nOne\n\nTwo\n");
    assert_eq!(slide.layers.iter().map(|l| l.is_title).collect::<Vec<_>>(), [false, true, false]);
}

#[test]
fn a_positioned_tag_is_never_the_title() {
    let slide = lay_out("<Box x=\"0\" y=\"0\" w=\"99\" h=\"99\" />\n");
    assert!(!slide.layers.last().unwrap().is_title);
}

#[test]
fn text_in_a_text_tag_is_styled_and_joins_the_text_around_it() {
    let slide = lay_out("# T\n\nBefore\n\n<Text color=\"#ff0000\" size=\"50\" italic=\"true\">\n\nStyled\n\n</Text>\n\nAfter\n");
    let body = slide.layers.last().unwrap();
    assert_eq!(body.native.len(), 1);
    assert_eq!(texts_of(&body.native[0]), ["Before", "Styled", "After"]);
    let paragraphs = paragraphs_of(&body.native[0]);
    let run = &paragraphs[1].runs[0];
    assert_eq!((Some(run.color), run.size, run.italic), (Color::parse("#ff0000"), 50.0, true));
    assert!(!paragraphs[0].runs[0].italic);
}

#[test]
fn a_size_is_the_one_the_text_is_drawn_at() {
    let slide = lay_out("# T\n\n<Text size=\"9999\">\n\nBig\n\n</Text>\n");
    assert_eq!(paragraphs_of(&slide.layers.last().unwrap().native[0])[0].runs[0].size, 512.0);
}

#[test]
fn each_thing_in_a_stack_is_its_own_element() {
    let slide = lay_out_using("<Apart x=\"0\" y=\"0\" w=\"400\" h=\"400\" />\n", &[], &probe_registry());
    let native = &slide.layers.last().unwrap().native;
    assert_eq!(native.len(), 4);
    assert_eq!(texts_of(&native[0]), ["one"]);
    assert_eq!(texts_of(&native[1]), ["two"]);
    assert!(matches!(native[2].content, NativeContent::Picture { src: None, .. }));
    assert!(matches!(native[3].content, NativeContent::Shape { kind: ShapeKind::Rect, .. }));
    assert!(native.windows(2).all(|pair| pair[0].rect.y < pair[1].rect.y));
}

#[test]
fn a_path_is_a_path() {
    let slide = lay_out_using("<Traced x=\"0\" y=\"0\" w=\"10\" h=\"10\" />\n", &[], &probe_registry());
    assert!(matches!(&slide.layers.last().unwrap().native[0].content, NativeContent::Path { .. }));
}

#[test]
fn a_picture_tag_has_a_file_and_no_alt() {
    let slide = lay_out_with("<Picture src=\"a.png\" x=\"5\" y=\"5\" w=\"300\" h=\"150\" />\n", &[("a.png", png(4, 4))]);
    let element = &slide.layers.last().unwrap().native[0];
    assert!(matches!(&element.content, NativeContent::Picture { src: Some(s), alt, .. } if s == "a.png" && alt.is_empty()));
    assert_eq!(element.rect, Rect { x: 0.0, y: 0.0, w: 300.0, h: 150.0 });
}

#[test]
fn a_picture_ends_the_text_before_it() {
    let slide = lay_out_with("# T\n\nBefore\n\n![A cat](a.png)\n\nAfter\n", &[("a.png", png(4, 4))]);
    let native = &slide.layers.last().unwrap().native;
    assert_eq!(native.len(), 3);
    assert!(matches!(native[1].content, NativeContent::Picture { .. }));
}

#[test]
fn a_missing_picture_is_its_label_in_its_own_box() {
    let slide = lay_out("# T\n\nBefore\n\n![A cat](missing.png)\n\nAfter\n");
    let native = &slide.layers.last().unwrap().native;
    assert_eq!(native.len(), 3);
    assert_eq!(texts_of(&native[1]), ["missing.png"]);
}

#[test]
fn a_flowing_unknown_tag_is_its_name() {
    let slide = lay_out("# T\n\n<Mystery />\n");
    let element = &slide.layers.last().unwrap().native[0];
    assert_eq!(texts_of(element), ["Mystery"]);
    assert_eq!(paragraphs_of(element)[0].align, Align::Center);
}

#[test]
fn a_broken_slide_is_its_message() {
    let source = "<Box>\n\nnever closed\n";
    let message = parse_talk(source).slides[0].error.clone().unwrap().message;
    let slide = lay_out(source);
    let element = &slide.layers.last().unwrap().native[0];
    assert_eq!(texts_of(element), [message]);
    assert_eq!(element.rect, Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 });
}

#[test]
fn a_list_in_a_box_is_text_the_shape_holds() {
    let slide = lay_out("<Box x=\"0\" y=\"0\" w=\"400\" h=\"200\" fill=\"red\">\n\n- a\n- b\n\n</Box>\n");
    let native = &slide.layers.last().unwrap().native;
    assert_eq!(native.len(), 1);
    assert_eq!(texts_of(&native[0]), ["a", "b"]);
    assert_eq!(paragraphs_of(&native[0])[1].bullet, Bullet::Dot);
}

#[test]
fn a_shape_holding_more_than_text_is_followed_by_what_it_holds() {
    let slide = lay_out("<Box x=\"50\" y=\"50\" w=\"800\" h=\"600\" fill=\"red\">\n\nHello\n\n| a |\n|---|\n| 1 |\n\n</Box>\n");
    let native = &slide.layers.last().unwrap().native;
    assert_eq!(native.len(), 3);
    assert!(paragraphs_of(&native[0]).is_empty());
    assert_eq!(native[0].rect, Rect { x: 0.0, y: 0.0, w: 800.0, h: 600.0 });
    assert_eq!(texts_of(&native[1]), ["Hello"]);
    assert_eq!((native[1].rect.x, native[1].rect.y), (24.0, 24.0));
    assert!(matches!(native[2].content, NativeContent::Table { .. }));
}

#[test]
fn a_shape_around_a_placeholder_does_not_hold_it_as_text() {
    let slide = lay_out("<Box x=\"0\" y=\"0\" w=\"400\" h=\"200\">\n\n<Mystery />\n\n</Box>\n");
    let native = &slide.layers.last().unwrap().native;
    assert_eq!(native.len(), 2);
    assert!(paragraphs_of(&native[0]).is_empty());
    assert_eq!(texts_of(&native[1]), ["Mystery"]);
}

#[test]
fn a_shape_around_a_stack_of_texts_keeps_them_apart() {
    let slide = lay_out_using("<Divided x=\"0\" y=\"0\" w=\"400\" h=\"200\" />\n", &[], &probe_registry());
    let native = &slide.layers.last().unwrap().native;
    assert_eq!(native.len(), 3);
    assert!(paragraphs_of(&native[0]).is_empty());
    assert_eq!((texts_of(&native[1]), texts_of(&native[2])), (vec!["one".to_string()], vec!["two".to_string()]));
}

#[test]
fn table_sizes_are_those_of_the_layout() {
    let slide = lay_out("# T\n\n| a | b |\n|---|---|\n| 1 | 2 |\n");
    let element = &slide.layers.last().unwrap().native[0];
    let NativeContent::Table { rows, column_widths, row_heights, line } = &element.content else { panic!() };
    // The table's box has room for its last outlines, which are one unit wide.
    assert_eq!(column_widths.iter().sum::<f32>() + 1.0, element.rect.w);
    assert_eq!(row_heights.iter().sum::<f32>() + 1.0, element.rect.h);
    assert_eq!(*line, Theme::default().text);
    assert_eq!(text_of(&rows[1][1][0]), "2");
    assert!(rows[0][0][0].runs[0].bold);
    assert!(!rows[1][0][0].runs[0].bold);
}

#[test]
fn a_tall_cell_makes_only_its_own_row_taller() {
    let long = "words that wrap ".repeat(40);
    let slide = lay_out(&format!("# T\n\n| a | b |\n|---|---|\n| {long} | 2 |\n| 3 | 4 |\n"));
    let NativeContent::Table { row_heights, .. } = &slide.layers.last().unwrap().native[0].content else { panic!() };
    assert!(row_heights[1] > row_heights[0]);
    assert_eq!(row_heights[2], row_heights[0]);
}

#[test]
fn every_row_of_a_table_has_every_column() {
    let slide = lay_out("# T\n\n| a | b |\n|---|---|\n| 1 |\n");
    let NativeContent::Table { rows, .. } = &slide.layers.last().unwrap().native[0].content else { panic!() };
    assert!(rows.iter().all(|row| row.len() == 2 && row.iter().all(|cell| cell.len() == 1)));
}

/// A talk with text and then a table of `rows` rows, counting the header. Only the header
/// has all its `columns`.
fn talk_with_short_rows(columns: usize, rows: usize) -> String {
    let (header, line, short_rows) = ("| a ".repeat(columns), "|---".repeat(columns), "| 1 |\n".repeat(rows - 1));
    format!("# T\n\nBefore\n\n{header}|\n{line}|\n{short_rows}")
}

#[test]
fn a_table_too_big_to_fill_out_is_labelled_text() {
    let slide = lay_out(&talk_with_short_rows(200, 201));
    let native = &slide.layers.last().unwrap().native;
    assert_eq!(native.len(), 2);
    assert_eq!(texts_of(&native[1]), ["This table is too big to export"]);
}

#[test]
fn a_table_too_big_to_export_says_why_and_one_at_the_limit_does_not() {
    let notice = Notice("This table is too big to export, so it's shown as text".to_string());
    assert!(lay_out(&talk_with_short_rows(101, 100)).notices.contains(&notice));

    let slide = lay_out(&talk_with_short_rows(100, 100));
    assert!(!slide.notices.contains(&notice));
    assert!(matches!(slide.layers.last().unwrap().native[1].content, NativeContent::Table { .. }));
}

#[test]
fn links_in_table_cells_are_kept() {
    let slide = lay_out("# T\n\n| [home](https://example.com) |\n|---|\n| x |\n");
    let NativeContent::Table { rows, .. } = &slide.layers.last().unwrap().native[0].content else { panic!() };
    assert_eq!(rows[0][0][0].runs[0].link.as_deref(), Some("https://example.com"));
}

#[test]
fn code_has_a_paragraph_for_each_line() {
    let slide = lay_out("# T\n\n```\nfirst\n\nthird\n```\n");
    let element = &slide.layers.last().unwrap().native[0];
    assert_eq!(texts_of(element), ["first", "", "third"]);
    assert_eq!(paragraphs_of(element)[1].runs.len(), 1, "an empty line keeps its height");
}

#[test]
fn a_list_item_that_starts_with_code_keeps_its_marker() {
    let slide = lay_out("# T\n\n- ```\n  x\n  ```\n");
    let native = &slide.layers.last().unwrap().native;
    assert_eq!(native.len(), 2);
    assert_eq!(texts_of(&native[0]), [""]);
    assert_eq!(paragraphs_of(&native[0])[0].bullet, Bullet::Dot);
    assert!(matches!(native[1].content, NativeContent::Text { fill: Some(_), .. }));
}

#[test]
fn shapes_carry_what_they_are_drawn_with() {
    let shape_of = |attrs: &str| {
        let slide = lay_out(&format!("<Shape x=\"0\" y=\"0\" w=\"99\" h=\"99\" {attrs} />\n"));
        slide.layers.last().unwrap().native[0].clone()
    };
    let NativeContent::Shape { outline, outline_width, fill, .. } = shape_of("kind=\"line\"").content else { panic!() };
    assert_eq!((outline, outline_width, fill), (Some(Theme::default().text), 4.0, None));
    let NativeContent::Shape { outline, .. } = shape_of("kind=\"arrow\" fill=\"red\"").content else { panic!() };
    assert_eq!(outline, Color::parse("red"));
    let NativeContent::Shape { radius, .. } = shape_of("kind=\"rounded\"").content else { panic!() };
    assert_eq!(radius, 24.0);
    let NativeContent::Shape { radius, .. } = shape_of("kind=\"rect\"").content else { panic!() };
    assert_eq!(radius, 0.0);
}

#[test]
fn a_shape_with_text_inside_centres_it() {
    let slide = lay_out("<Shape kind=\"ellipse\" x=\"0\" y=\"0\" w=\"400\" h=\"200\">\n\nHello\n\n</Shape>\n");
    let NativeContent::Shape { paragraphs, inset, centered, .. } = &slide.layers.last().unwrap().native[0].content else { panic!() };
    assert_eq!(text_of(&paragraphs[0]), "Hello");
    assert_eq!((*inset, *centered), (0.0, true));
}

#[test]
fn a_shapes_inset_is_the_padding_of_its_layout() {
    let inset_of = |tag: &str| {
        let slide = lay_out(&format!("{tag}\n"));
        let NativeContent::Shape { inset, .. } = slide.layers.last().unwrap().native[0].content else { panic!() };
        inset
    };
    assert_eq!(inset_of("<Box x=\"0\" y=\"0\" w=\"400\" h=\"200\" fill=\"red\" padding=\"40\" />"), 40.0);
    assert_eq!(inset_of("<Box x=\"0\" y=\"0\" w=\"400\" h=\"200\" />"), 0.0);
    assert_eq!(inset_of("<Shape kind=\"callout\" x=\"0\" y=\"0\" w=\"400\" h=\"200\" />"), 0.0, "a callout's tail isn't padding");
}

#[test]
fn a_box_keeps_its_own_radius() {
    let slide = lay_out("<Box x=\"0\" y=\"0\" w=\"99\" h=\"99\" radius=\"30\" />\n");
    let NativeContent::Shape { kind, radius, .. } = &slide.layers.last().unwrap().native[0].content else { panic!() };
    assert_eq!((*kind, *radius), (ShapeKind::Rounded, 30.0));
}
