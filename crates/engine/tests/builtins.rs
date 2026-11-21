mod common;

use std::cell::Cell;
use std::collections::HashMap;

use slides_engine::*;
use slides_format::parse_talk;
use tiny_skia::PathSegment;
use toolkit::Registry;

struct MapFiles(HashMap<String, Vec<u8>>);

impl TalkFiles for MapFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.0.get(path).cloned()
    }
}

fn lay_out_with(source: &str, files: &[(&str, Vec<u8>)], registry: &Registry) -> (LaidOutSlide, Fonts) {
    let mut fonts = Fonts::bundled_only();
    let talk = parse_talk(source);
    let files = MapFiles(files.iter().map(|(path, bytes)| (path.to_string(), bytes.clone())).collect());
    let slide = layout_slide(
        &talk,
        0,
        &mut LayoutEnv { fonts: &mut fonts, registry, files: &files, pictures: &mut PictureCache::default(), components: &mut TalkComponents::none(), moment: Moment::START, asks: ComponentAsks::default() },
    );
    (slide, fonts)
}

fn lay_out(source: &str, files: &[(&str, Vec<u8>)]) -> (LaidOutSlide, Fonts) {
    lay_out_with(source, files, &builtin_registry())
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::new(width, height)
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

fn picture_of(layer: &LayerSpec) -> (&tiny_skia::Pixmap, Rect) {
    layer
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Picture { pixmap, rect, .. } => Some((&**pixmap, *rect)),
            _ => None,
        })
        .expect("the layer has a picture")
}

fn has_picture(layer: &LayerSpec) -> bool {
    layer.items.iter().any(|item| matches!(item, DisplayItem::Picture { .. }))
}

fn fills(layer: &LayerSpec) -> Vec<&tiny_skia::Path> {
    layer
        .items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Fill { path, .. } => Some(path),
            _ => None,
        })
        .collect()
}

/// `(colour, width, path)` of each stroke in a layer.
fn strokes(layer: &LayerSpec) -> Vec<(toolkit::Color, f32, &tiny_skia::Path)> {
    layer
        .items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Stroke { path, color, width } => Some((*color, *width, path)),
            _ => None,
        })
        .collect()
}

/// `(x, y, width, height)` of each piece of text in a layer, in drawing order.
fn text_boxes(layer: &LayerSpec) -> Vec<(f32, f32, f32, f32)> {
    layer
        .items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Text { text, x, y } => Some((*x, *y, text.width, text.height)),
            _ => None,
        })
        .collect()
}

fn bounds(path: &tiny_skia::Path) -> (f32, f32, f32, f32) {
    let b = path.bounds();
    (b.left(), b.top(), b.right(), b.bottom())
}

fn painted(pixmap: &tiny_skia::Pixmap) -> bool {
    pixmap.pixels().iter().any(|pixel| pixel.alpha() > 0)
}

fn notice_about(slide: &LaidOutSlide, text: &str) -> bool {
    slide.notices.iter().any(|notice| notice.0.contains(text))
}

#[test]
fn a_box_is_its_own_layer_at_its_position() {
    let src = "<Box x=\"100\" y=\"200\" w=\"300\" h=\"150\" rotate=\"10\">\n\nHello\n\n</Box>\n";
    let (slide, _) = lay_out(src, &[]);
    let layer = slide.layers.last().unwrap();
    assert_eq!(layer.rect, Rect { x: 100.0, y: 200.0, w: 300.0, h: 150.0 });
    assert_eq!(layer.rotate, 10.0);
    assert_eq!(layer.source.clone().unwrap(), 0..src.trim_end().len());
}

#[test]
fn an_unknown_tag_is_a_labelled_placeholder_and_a_notice() {
    let (slide, _) = lay_out("<Mystery x=\"10\" y=\"10\" w=\"400\" h=\"200\" />\n", &[]);
    assert!(slide.notices.iter().any(|n| n.0.contains("Mystery")));
    assert!(slide.layers.last().unwrap().items.iter().any(|i| matches!(i, DisplayItem::Text { .. })));
}

#[test]
fn missing_picture_shows_a_placeholder() {
    let (slide, _) = lay_out("<Picture src=\"assets/none.png\" x=\"0\" y=\"0\" w=\"100\" h=\"100\" />\n", &[]);
    assert!(slide.notices.iter().any(|n| n.0.contains("assets/none.png")));
    assert!(!slide.layers.last().unwrap().items.iter().any(|i| matches!(i, DisplayItem::Picture { .. })));
}

#[test]
fn corrupt_picture_shows_a_placeholder() {
    let (slide, _) = lay_out("![x](assets/bad.png)\n", &[("assets/bad.png", b"not a png".to_vec())]);
    assert!(slide.notices.iter().any(|n| n.0.contains("assets/bad.png")));
}

#[test]
fn a_real_picture_is_drawn() {
    let mut png = Vec::new();
    image::RgbaImage::new(4, 4).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let (slide, _) = lay_out("<Picture src=\"a.png\" x=\"0\" y=\"0\" w=\"100\" h=\"100\" />\n", &[("a.png", png)]);
    assert!(slide.layers.last().unwrap().items.iter().any(|i| matches!(i, DisplayItem::Picture { .. })));
}

#[test]
fn an_unknown_icon_is_a_notice_not_a_crash() {
    let (slide, _) = lay_out("<Icon name=\"not-an-icon\" x=\"0\" y=\"0\" w=\"64\" h=\"64\" />\n", &[]);
    assert!(slide.notices.iter().any(|n| n.0.contains("not-an-icon")));
}

#[test]
fn slide_error_shows_a_labelled_placeholder() {
    let (slide, _) = lay_out("<Box>\n\nnever closed\n", &[]);
    assert!(slide.layers.iter().any(|l| l.items.iter().any(|i| matches!(i, DisplayItem::Text { .. }))));
    assert!(slide.notices.iter().any(|n| n.0.contains("Box")));
}

#[test]
fn golden_shapes_and_icons() {
    let mut src = String::from("<Box x=\"60\" y=\"60\" w=\"500\" h=\"200\" fill=\"#dbeafe\" radius=\"24\">\n\nIn a box\n\n</Box>\n\n");
    for (i, kind) in ["rect", "rounded", "ellipse", "line", "arrow", "callout"].iter().enumerate() {
        src += &format!("<Shape kind=\"{kind}\" x=\"{}\" y=\"400\" w=\"260\" h=\"180\" fill=\"orange\" outline=\"black\" outline-width=\"4\" />\n\n", 60 + i * 300);
    }
    src += "<Icon name=\"arrow-right\" x=\"60\" y=\"700\" w=\"128\" h=\"128\" size=\"128\" />\n";
    let (slide, mut fonts) = lay_out(&src, &[]);
    let layers: Vec<_> = slide.layers.iter().map(|l| render_layer(l, 1.0, &mut fonts)).collect();
    let mut target = tiny_skia::Pixmap::new(1920, 1080).unwrap();
    compose(&mut target, &layers);
    common::assert_golden("shapes", &target);
}

// Where a tag goes.

#[test]
fn a_tag_with_no_place_flows_in_its_region() {
    let src = "<Box>\n\nHello\n\n</Box>\n";
    let (slide, _) = lay_out(src, &[]);
    assert_eq!(slide.layers.len(), 2);
    assert_eq!(slide.layers[1].rect, Rect { x: 100.0, y: 220.0, w: 1720.0, h: 780.0 });
    assert_eq!(slide.layers[1].source.clone().unwrap(), 0..src.trim_end().len());
    assert_eq!(text_boxes(&slide.layers[1]).len(), 1);
}

#[test]
fn any_one_of_the_placement_numbers_makes_a_layer() {
    for attr in ["x", "y", "w", "h"] {
        let (slide, _) = lay_out(&format!("<Box {attr}=\"10\" />\n"), &[]);
        assert_eq!(slide.layers.len(), 2);
        assert_ne!(slide.layers[1].rect.y, 220.0, "{attr} should not leave the box in the body region");
    }
}

#[test]
fn positioned_tags_are_above_the_regions_in_the_order_written() {
    let src = "# T\n\n<Box x=\"1\" y=\"1\" w=\"10\" h=\"10\" />\n\nBody\n\n<Icon name=\"arrow-right\" x=\"2\" y=\"2\" />\n";
    let (slide, _) = lay_out(src, &[]);
    let xs: Vec<f32> = slide.layers.iter().map(|layer| layer.rect.x).collect();
    assert_eq!(xs, [0.0, 100.0, 100.0, 1.0, 2.0]);
}

#[test]
fn placement_numbers_that_cannot_be_read_count_as_absent() {
    let (slide, _) = lay_out("<Box x=\"oops\" y=\"NaN\" w=\"inf\" h=\"-inf\" />\n\nAfter\n", &[]);
    assert_eq!(slide.layers.len(), 2, "with none of the numbers readable, the box flows");

    let (slide, _) = lay_out("<Box x=\"10\" y=\"NaN\" w=\"100\" h=\"oops\" rotate=\"inf\" />\n", &[]);
    assert_eq!(slide.layers[1].rect, Rect { x: 10.0, y: 0.0, w: 100.0, h: 200.0 });
    assert_eq!(slide.layers[1].rotate, 0.0);
}

#[test]
fn placement_numbers_are_held_to_the_limits() {
    let (slide, _) = lay_out("<Box x=\"99999\" y=\"-99999\" w=\"99999999\" h=\"-5\" rotate=\"9000\" />\n", &[]);
    assert_eq!(slide.layers[1].rect, Rect { x: 7680.0, y: -3840.0, w: 3840.0, h: 0.0 });
    assert_eq!(slide.layers[1].rotate, 360.0);
}

#[test]
fn a_place_with_no_size_and_no_natural_size_is_400_by_200() {
    let (slide, _) = lay_out("<Box x=\"10\" y=\"20\" />\n", &[]);
    assert_eq!(slide.layers[1].rect, Rect { x: 10.0, y: 20.0, w: 400.0, h: 200.0 });
    let (slide, _) = lay_out("<Mystery y=\"20\" />\n", &[]);
    assert_eq!(slide.layers[1].rect, Rect { x: 0.0, y: 20.0, w: 400.0, h: 200.0 });
}

#[test]
fn an_icon_placed_with_no_size_is_its_own_size() {
    let (slide, _) = lay_out("<Icon name=\"arrow-right\" x=\"10\" y=\"20\" />\n", &[]);
    assert_eq!(slide.layers[1].rect, Rect { x: 10.0, y: 20.0, w: 64.0, h: 64.0 });
    let (slide, _) = lay_out("<Icon name=\"arrow-right\" x=\"10\" y=\"20\" size=\"96\" />\n", &[]);
    assert_eq!(slide.layers[1].rect, Rect { x: 10.0, y: 20.0, w: 96.0, h: 96.0 });
}

#[test]
fn a_picture_placed_with_no_size_is_its_own_size() {
    let (slide, _) = lay_out("<Picture src=\"a.png\" x=\"5\" y=\"6\" />\n", &[("a.png", png(30, 20))]);
    assert_eq!(slide.layers[1].rect, Rect { x: 5.0, y: 6.0, w: 30.0, h: 20.0 });
    assert_eq!(picture_of(&slide.layers[1]).1, Rect { x: 0.0, y: 0.0, w: 30.0, h: 20.0 });
}

#[test]
fn a_picture_given_one_side_keeps_its_shape() {
    let (slide, _) = lay_out("<Picture src=\"a.png\" w=\"300\" />\n", &[("a.png", png(30, 20))]);
    assert_eq!((slide.layers[1].rect.w, slide.layers[1].rect.h), (300.0, 200.0));
    let (slide, _) = lay_out("<Picture src=\"a.png\" h=\"40\" />\n", &[("a.png", png(30, 20))]);
    assert_eq!((slide.layers[1].rect.w, slide.layers[1].rect.h), (60.0, 40.0));
}

#[test]
fn a_picture_fills_the_box_it_is_given() {
    let (slide, _) = lay_out("<Picture src=\"a.png\" x=\"0\" y=\"0\" w=\"300\" h=\"150\" />\n", &[("a.png", png(4, 4))]);
    assert_eq!(picture_of(&slide.layers[1]).1, Rect { x: 0.0, y: 0.0, w: 300.0, h: 150.0 });
}

mod probe {
    use toolkit::prelude::*;

    /// Says which attributes reach it. Written by hand, as `#[component]` can't take
    /// an argument named for the slide's placement.
    #[allow(non_snake_case)]
    pub fn Probe_component() -> NativeComponent {
        fn schema() -> ComponentSchema {
            ComponentSchema::new("Probe", Vec::new(), false)
        }
        fn render(props: &Props, _: &Ctx) -> Element {
            let get = |name: &str| props.0.get(name).cloned().unwrap_or_default();
            text(format!("[{}][{}][{}][{}]", get("x"), get("rotate"), get("click"), get("note")))
        }
        NativeComponent { name: "Probe", schema, render }
    }

    #[component]
    pub fn Stacked() -> Element {
        row![icon("arrow-right", 40.0), icon("arrow-right", 40.0)].gap(10.0)
    }

    #[component]
    pub fn Drawn(data: String) -> Element {
        path(data, 10.0, 10.0).fill(Color::parse("red").unwrap())
    }

    #[component]
    pub fn Written() -> Element {
        text("Hi").size(100.0).bold().fill(Color::parse("red").unwrap())
    }

    #[component]
    pub fn Troubled() -> Element {
        problem("No data")
    }
}

fn probe_registry() -> Registry {
    let mut registry = builtin_registry();
    registry.add(probe::Probe_component());
    registry.add(probe::Stacked_component());
    registry.add(probe::Drawn_component());
    registry.add(probe::Written_component());
    registry.add(probe::Troubled_component());
    registry
}

fn text_of(layer: &LayerSpec) -> String {
    layer
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Text { text, .. } => Some(text.buffer.layout_runs().next().unwrap().text.to_string()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn placement_attributes_do_not_reach_the_component() {
    assert_eq!(PLACEMENT_ATTRS, ["x", "y", "w", "h", "rotate", "click", "enter", "align"]);
    let src = "<Probe x=\"1\" rotate=\"2\" click=\"3\" note=\"kept\" />\n";
    let (slide, _) = lay_out_with(src, &[], &probe_registry());
    assert_eq!(text_of(slide.layers.last().unwrap()), "[][][][kept]");
}

#[test]
fn the_first_of_a_repeated_attribute_is_the_one_used() {
    let src = "<Probe note=\"first\" note=\"second\" />\n";
    let (slide, _) = lay_out_with(src, &[], &probe_registry());
    assert_eq!(text_of(slide.layers.last().unwrap()), "[][][][first]");
}

// What elements draw.

#[test]
fn a_box_starts_its_children_at_its_corner() {
    let (slide, _) = lay_out("<Box x=\"0\" y=\"0\" w=\"600\" h=\"400\">\n\nHello\n\n</Box>\n", &[]);
    let text = text_boxes(&slide.layers[1])[0];
    assert_eq!((text.0, text.1), (0.0, 0.0));
}

#[test]
fn a_box_wraps_its_children_at_its_width() {
    let src = format!("<Box x=\"0\" y=\"0\" w=\"300\" h=\"400\">\n\n{}\n\n</Box>\n", "word ".repeat(40));
    let (slide, _) = lay_out(&src, &[]);
    let text = text_boxes(&slide.layers[1])[0];
    assert!(text.2 <= 300.0);
    assert!(text.3 > 100.0);
}

#[test]
fn a_shape_centres_its_children() {
    let (slide, _) = lay_out("<Shape kind=\"rect\" x=\"0\" y=\"0\" w=\"600\" h=\"400\">\n\nHello\n\n</Shape>\n", &[]);
    let (x, y, width, height) = text_boxes(&slide.layers[1])[0];
    assert!((x + width / 2.0 - 300.0).abs() < 1.0, "x {x}, width {width}");
    assert!((y + height / 2.0 - 200.0).abs() < 1.0, "y {y}, height {height}");
}

#[test]
fn a_shape_fills_its_box_with_fill_then_outline() {
    let (slide, _) = lay_out("<Shape kind=\"rect\" x=\"5\" y=\"6\" w=\"260\" h=\"180\" fill=\"orange\" outline=\"black\" outline-width=\"4\" />\n", &[]);
    let layer = &slide.layers[1];
    assert_eq!(bounds(fills(layer)[0]), (0.0, 0.0, 260.0, 180.0));
    let (color, width, path) = strokes(layer)[0];
    assert_eq!((color, width), (toolkit::Color::parse("black").unwrap(), 4.0));
    assert_eq!(bounds(path), (0.0, 0.0, 260.0, 180.0));
    assert!(matches!(layer.items[0], DisplayItem::Fill { .. }));
    assert!(matches!(layer.items[1], DisplayItem::Stroke { .. }));
}

#[test]
fn an_outline_with_no_width_is_one_wide() {
    let (slide, _) = lay_out("<Box x=\"0\" outline=\"black\" />\n", &[]);
    assert_eq!(strokes(&slide.layers[1])[0].1, 1.0);
}

#[test]
fn a_line_is_drawn_corner_to_corner_in_its_outline_colour() {
    let (slide, _) = lay_out("<Shape kind=\"line\" x=\"0\" w=\"200\" h=\"100\" fill=\"orange\" outline=\"black\" outline-width=\"4\" />\n", &[]);
    let layer = &slide.layers[1];
    assert!(fills(layer).is_empty());
    let (color, width, path) = strokes(layer)[0];
    assert_eq!((color, width), (toolkit::Color::parse("black").unwrap(), 4.0));
    assert_eq!(bounds(path), (0.0, 0.0, 200.0, 100.0));
}

#[test]
fn a_line_with_no_colours_uses_the_text_colour() {
    let (slide, _) = lay_out("<Shape kind=\"line\" x=\"0\" w=\"200\" h=\"0\" />\n", &[]);
    let (color, width, _) = strokes(&slide.layers[1])[0];
    assert_eq!(color, toolkit::Theme::default().text);
    assert!(width > 0.0);
}

#[test]
fn an_arrow_has_a_head_at_its_end() {
    let (slide, _) = lay_out("<Shape kind=\"arrow\" x=\"0\" w=\"200\" h=\"0\" outline=\"black\" outline-width=\"4\" />\n", &[]);
    let layer = &slide.layers[1];
    let head = bounds(fills(layer)[0]);
    assert_eq!(head.2, 200.0, "the head's tip is at the end of the arrow");
    assert!(head.0 > 100.0 && head.0 < 200.0);
    assert!(head.1 < 0.0 && head.3 > 0.0, "the head is wider than the line");
    assert_eq!(strokes(layer).len(), 1);
}

#[test]
fn a_callout_has_a_tail_and_centres_its_children_above_it() {
    let (slide, _) = lay_out("<Shape kind=\"callout\" x=\"0\" w=\"600\" h=\"400\" fill=\"orange\">\n\nHi\n\n</Shape>\n", &[]);
    let layer = &slide.layers[1];
    assert_eq!(bounds(fills(layer)[0]), (0.0, 0.0, 600.0, 400.0));
    let (_, y, _, height) = text_boxes(layer)[0];
    assert!(y + height / 2.0 < 195.0, "the middle of the text is at {}", y + height / 2.0);
}

#[test]
fn an_unknown_kind_of_shape_is_a_rectangle() {
    let (slide, _) = lay_out("<Shape kind=\"blob\" x=\"0\" w=\"100\" h=\"100\" fill=\"red\" />\n", &[]);
    assert_eq!(fills(&slide.layers[1]).len(), 1);
    assert_eq!(bounds(fills(&slide.layers[1])[0]), (0.0, 0.0, 100.0, 100.0));
}

#[test]
fn a_colour_that_cannot_be_read_is_no_colour() {
    let (slide, _) = lay_out("<Box x=\"0\" fill=\"nonsense\" outline=\"nonsense\" />\n", &[]);
    assert!(slide.layers[1].items.is_empty());
}

#[test]
fn radius_rounds_a_boxs_corners() {
    let (square, mut fonts) = lay_out("<Box x=\"0\" y=\"0\" w=\"200\" h=\"200\" fill=\"red\" />\n", &[]);
    let (round, _) = lay_out("<Box x=\"0\" y=\"0\" w=\"200\" h=\"200\" fill=\"red\" radius=\"50\" />\n", &[]);
    let corner = |slide: &LaidOutSlide, fonts: &mut Fonts, at: u32| {
        render_layer(&slide.layers[1], 1.0, fonts).pixmap.pixel(at, at).unwrap().alpha()
    };
    assert_eq!(corner(&square, &mut fonts, 2), 255);
    assert_eq!(corner(&round, &mut fonts, 2), 0);
    // Still inside the corner of a radius of 24, the default for a rounded shape.
    assert_eq!(corner(&round, &mut fonts, 8), 0);
}

#[test]
fn a_tag_inside_a_box_is_drawn_inside_it() {
    let src = "<Box x=\"100\" y=\"100\" w=\"500\" h=\"300\">\n\n<Icon name=\"arrow-right\" />\n\n</Box>\n";
    let (slide, _) = lay_out(src, &[]);
    assert_eq!(slide.layers.len(), 2);
    assert_eq!(picture_of(&slide.layers[1]).1, Rect { x: 0.0, y: 0.0, w: 64.0, h: 64.0 });
}

#[test]
fn a_tag_inside_a_box_cannot_be_placed_and_says_so() {
    let src = "<Box x=\"100\" y=\"100\" w=\"500\" h=\"300\">\n\n<Icon name=\"arrow-right\" x=\"50\" y=\"50\" />\n\n</Box>\n";
    let (slide, _) = lay_out(src, &[]);
    assert_eq!(slide.layers.len(), 2);
    assert_eq!(picture_of(&slide.layers[1]).1, Rect { x: 0.0, y: 0.0, w: 64.0, h: 64.0 });
    assert!(notice_about(&slide, "Icon"));
}

#[test]
fn stacks_and_text_from_a_component_are_laid_out() {
    let (slide, _) = lay_out_with("<Stacked x=\"0\" y=\"0\" w=\"300\" h=\"100\" />\n", &[], &probe_registry());
    let rects: Vec<Rect> = slide.layers[1]
        .items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Picture { rect, .. } => Some(*rect),
            _ => None,
        })
        .collect();
    assert_eq!(rects, [Rect { x: 0.0, y: 0.0, w: 40.0, h: 40.0 }, Rect { x: 50.0, y: 0.0, w: 40.0, h: 40.0 }]);

    let (slide, _) = lay_out_with("<Written />\n", &[], &probe_registry());
    let (_, _, _, height) = text_boxes(&slide.layers[1])[0];
    assert_eq!(height, 125.0, "size 100 at the line height of 1.25");
}

#[test]
fn a_path_from_a_component_is_drawn_and_a_bad_one_is_a_placeholder() {
    let good = "<Drawn data=\"M0 0H10V10H0Z\" />\n";
    let (slide, _) = lay_out_with(good, &[], &probe_registry());
    assert!(!has_picture(&slide.layers[1]), "a path is a vector, not a picture");
    assert_eq!(bounds(fills(&slide.layers[1])[0]), (0.0, 0.0, 10.0, 10.0));

    let injected = "<Drawn data='M0 0\"/&gt;&lt;image href=\"x.png' />\n";
    let (slide, _) = lay_out_with(injected, &[], &probe_registry());
    assert!(!has_picture(&slide.layers[1]));
    assert!(notice_about(&slide, "path"));
}

#[test]
fn a_picture_asks_for_a_picture_file() {
    let schema = (builtin_registry().get("Picture").unwrap().schema)();
    assert!(matches!(&schema.props[0].kind, toolkit::PropKind::File(extensions) if extensions.iter().any(|e| e == "png")));
}

#[test]
fn a_problem_is_a_labelled_placeholder_and_a_notice() {
    let (slide, _) = lay_out_with("<Troubled />\n", &[], &probe_registry());
    assert!(notice_about(&slide, "No data"));
    let layer = &slide.layers[1];
    assert_eq!(text_of(layer).trim_end(), "No data");
    assert_eq!(strokes(layer).len(), 1, "the dashed outline");
}

#[test]
fn a_problem_in_a_box_fills_the_box() {
    let (slide, _) = lay_out_with("<Troubled x=\"0\" y=\"0\" w=\"800\" h=\"600\" />\n", &[], &probe_registry());
    let (_, y, _, height) = text_boxes(&slide.layers[1])[0];
    assert_eq!(y + height / 2.0, 300.0);
}

// Tags in the flow.

#[test]
fn a_region_marker_sends_a_flowing_tag_to_that_region() {
    let (slide, _) = lay_out("---\nlayout: two-columns\n---\n\n<Region name=\"right\" />\n\n<Icon name=\"arrow-right\" />\n", &[]);
    assert_eq!(slide.layers[1].rect.x, 980.0);
    assert!(has_picture(&slide.layers[1]));
}

#[test]
fn a_flowing_box_is_as_wide_as_its_region_and_as_tall_as_its_content() {
    let (slide, _) = lay_out("<Box fill=\"red\">\n\nHello\n\n</Box>\n", &[]);
    let layer = &slide.layers[1];
    let (_, _, _, height) = text_boxes(layer)[0];
    assert_eq!(bounds(fills(layer)[0]), (0.0, 0.0, 1720.0, height + 48.0), "the text plus 24 of padding above and below");
}

#[test]
fn an_empty_flowing_shape_is_400_by_200() {
    let (slide, _) = lay_out("<Shape kind=\"ellipse\" fill=\"red\" />\n\nAfter\n", &[]);
    let layer = &slide.layers[1];
    assert_eq!(bounds(fills(layer)[0]), (0.0, 0.0, 400.0, 200.0));
    assert_eq!(text_boxes(layer)[0].1, 224.0, "the next block starts below, after the gap");
}

#[test]
fn an_unknown_flowing_tag_takes_up_room() {
    let (slide, _) = lay_out("<Mystery />\n\nAfter\n", &[]);
    let boxes = text_boxes(&slide.layers[1]);
    assert_eq!(boxes.last().unwrap().1, 224.0);
    assert!(notice_about(&slide, "Mystery"));
}

#[test]
fn a_flowing_icon_is_its_own_size_and_follows_the_region_alignment() {
    let (slide, _) = lay_out("<Icon name=\"arrow-right\" />\n", &[]);
    assert_eq!(picture_of(&slide.layers[1]).1, Rect { x: 0.0, y: 0.0, w: 64.0, h: 64.0 });

    let (slide, _) = lay_out("---\nlayout: title\n---\n\n<Icon name=\"arrow-right\" />\n", &[]);
    assert_eq!(picture_of(&slide.layers[1]).1.x, 768.0);
}

#[test]
fn a_flowing_picture_is_its_own_size() {
    let (slide, _) = lay_out("![x](a.png)\n", &[("a.png", png(30, 20))]);
    assert_eq!(picture_of(&slide.layers[1]).1, Rect { x: 0.0, y: 0.0, w: 30.0, h: 20.0 });
}

#[test]
fn a_flowing_picture_too_wide_for_its_column_is_scaled_to_fit() {
    let (slide, _) = lay_out("![x](a.png)\n", &[("a.png", png(3440, 1720))]);
    assert_eq!(picture_of(&slide.layers[1]).1, Rect { x: 0.0, y: 0.0, w: 1720.0, h: 860.0 });
}

#[test]
fn a_picture_tag_in_the_flow_is_its_own_size_too() {
    let (slide, _) = lay_out("<Picture src=\"a.png\" />\n", &[("a.png", png(30, 20))]);
    assert_eq!(picture_of(&slide.layers[1]).1, Rect { x: 0.0, y: 0.0, w: 30.0, h: 20.0 });
}

#[test]
fn a_flowing_picture_in_a_centred_region_is_centred() {
    let (slide, _) = lay_out("---\nlayout: title\n---\n\n![x](a.png)\n", &[("a.png", png(30, 20))]);
    assert_eq!(picture_of(&slide.layers[1]).1.x, 785.0);
}

#[test]
fn a_picture_in_a_list_is_drawn() {
    let (slide, _) = lay_out("- ![x](a.png)\n", &[("a.png", png(30, 20))]);
    assert!(has_picture(&slide.layers[1]));
}

// Placeholders.

#[test]
fn a_placeholder_has_a_dashed_outline_and_a_centred_label() {
    let (slide, _) = lay_out("<Mystery x=\"0\" y=\"0\" w=\"400\" h=\"200\" />\n", &[]);
    let layer = &slide.layers[1];
    let (_, _, path) = strokes(layer)[0];
    let dashes = path.segments().filter(|segment| matches!(segment, PathSegment::MoveTo(_))).count();
    assert!(dashes > 8, "an outline drawn in {dashes} pieces is not dashed");
    let (_, y, _, height) = text_boxes(layer)[0];
    assert_eq!(y + height / 2.0, 100.0);
}

#[test]
fn a_slide_error_fills_the_slide_and_shows_the_message() {
    let source = "<Box>\n\nnever closed\n";
    let talk = parse_talk(source);
    let message = talk.slides[0].error.clone().unwrap().message;
    let (slide, _) = lay_out(source, &[]);
    assert_eq!(slide.layers.len(), 2);
    let layer = &slide.layers[1];
    assert_eq!(layer.rect, Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 });
    assert_eq!(layer.source, Some(talk.slides[0].range.clone()));
    assert_eq!(text_of(layer).trim_end(), message);
    assert!(notice_about(&slide, &message));
}

#[test]
fn the_same_missing_picture_is_only_a_notice_once() {
    let (slide, _) = lay_out("![a](x.png)\n\n![b](x.png)\n", &[]);
    assert_eq!(slide.notices.len(), 1);
}

// Pictures are untrusted input.

/// A complete QOI file of one colour, which takes a byte per 62 pixels. These are valid
/// files, so the only thing that can refuse them is a limit.
fn flat_qoi(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"qoif".to_vec();
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend([4, 0]);
    let mut left = u64::from(width) * u64::from(height);
    while left > 0 {
        let run = left.min(62);
        bytes.push(0xc0 | (run - 1) as u8);
        left -= run;
    }
    bytes.extend([0, 0, 0, 0, 0, 0, 0, 1]);
    bytes
}

#[test]
fn a_picture_within_the_limits_is_drawn() {
    let (slide, _) = lay_out("<Picture src=\"a.qoi\" x=\"0\" w=\"100\" h=\"100\" />\n", &[("a.qoi", flat_qoi(16384, 1))]);
    assert!(has_picture(&slide.layers[1]));
}

#[test]
fn a_picture_too_many_pixels_across_or_down_shows_a_placeholder() {
    for (width, height) in [(16385, 1), (1, 16385)] {
        let (slide, _) = lay_out("<Picture src=\"a.qoi\" x=\"0\" w=\"100\" h=\"100\" />\n", &[("a.qoi", flat_qoi(width, height))]);
        assert!(notice_about(&slide, "a.qoi"));
        assert!(!has_picture(&slide.layers[1]));
    }
}

#[test]
fn a_picture_needing_too_much_memory_shows_a_placeholder() {
    // Each side is within the limit, but four bytes a pixel is just over 256 MiB.
    let (slide, _) = lay_out("<Picture src=\"a.qoi\" x=\"0\" w=\"100\" h=\"100\" />\n", &[("a.qoi", flat_qoi(16384, 4097))]);
    assert!(notice_about(&slide, "a.qoi"));
    assert!(!has_picture(&slide.layers[1]));
}

#[test]
fn a_long_picture_is_limited_to_4096_on_its_longest_edge() {
    let (slide, _) = lay_out("<Picture src=\"a.png\" x=\"0\" w=\"100\" h=\"100\" />\n", &[("a.png", png(5000, 100))]);
    let (pixmap, _) = picture_of(&slide.layers[1]);
    assert_eq!((pixmap.width(), pixmap.height()), (4096, 82));
}

fn red_svg(attributes: &str) -> Vec<u8> {
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" {attributes}><rect width=\"100%\" height=\"100%\" fill=\"#ff0000\"/></svg>").into_bytes()
}

#[test]
fn an_svg_is_drawn() {
    let (slide, _) = lay_out("<Picture src=\"a.svg\" x=\"0\" />\n", &[("a.svg", red_svg("width=\"20\" height=\"10\""))]);
    let layer = &slide.layers[1];
    assert_eq!((layer.rect.w, layer.rect.h), (20.0, 10.0));
    let (pixmap, _) = picture_of(layer);
    assert_eq!((pixmap.width(), pixmap.height()), (20, 10));
    assert_eq!(pixmap.pixel(10, 5).unwrap().red(), 255);
}

#[test]
fn an_svg_is_recognised_whatever_the_case_of_its_extension() {
    let (slide, _) = lay_out("<Picture src=\"a.SVG\" x=\"0\" />\n", &[("a.SVG", red_svg("width=\"20\" height=\"10\""))]);
    assert!(has_picture(&slide.layers[1]));
}

#[test]
fn an_enormous_svg_is_limited_to_4096_on_its_longest_edge() {
    let svg = red_svg("width=\"100000\" height=\"50000\"");
    let (slide, _) = lay_out("<Picture src=\"a.svg\" x=\"0\" w=\"100\" h=\"100\" />\n", &[("a.svg", svg)]);
    let (pixmap, _) = picture_of(&slide.layers[1]);
    assert_eq!((pixmap.width(), pixmap.height()), (4096, 2048));
    assert_eq!(pixmap.pixel(4000, 2000).unwrap().red(), 255);
}

#[test]
fn a_corrupt_svg_shows_a_placeholder() {
    let (slide, _) = lay_out("<Picture src=\"a.svg\" x=\"0\" />\n", &[("a.svg", b"<svg".to_vec())]);
    assert!(notice_about(&slide, "a.svg"));
    assert!(!has_picture(&slide.layers[1]));
}

/// A PNG of one red pixel.
const RED_PIXEL: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/q842iQAAAABJRU5ErkJggg==";

#[test]
fn an_svg_cannot_draw_files_from_the_computer_or_embedded_pictures() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside.png");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 255])).save(&outside).unwrap();
    for href in [outside.display().to_string(), format!("data:image/png;base64,{RED_PIXEL}")] {
        let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"8\" height=\"8\"><image href=\"{href}\" width=\"8\" height=\"8\"/></svg>");
        let (slide, _) = lay_out("<Picture src=\"a.svg\" x=\"0\" />\n", &[("a.svg", svg.into_bytes())]);
        for item in slide.layers[1].items.iter() {
            if let DisplayItem::Picture { pixmap, .. } = item {
                assert!(!painted(pixmap), "{href} was drawn");
            }
        }
    }
}

struct CountedFiles {
    bytes: Vec<u8>,
    reads: Cell<u32>,
}

impl TalkFiles for CountedFiles {
    fn read(&self, _: &str) -> Option<Vec<u8>> {
        self.reads.set(self.reads.get() + 1);
        Some(self.bytes.clone())
    }
}

#[test]
fn a_picture_is_only_read_once() {
    let talk = parse_talk("![a](a.png)\n\n![b](a.png)\n");
    let files = CountedFiles { bytes: png(4, 4), reads: Cell::new(0) };
    let registry = builtin_registry();
    let mut fonts = Fonts::bundled_only();
    let mut pictures = PictureCache::default();
    for _ in 0..2 {
        layout_slide(&talk, 0, &mut LayoutEnv { fonts: &mut fonts, registry: &registry, files: &files, pictures: &mut pictures, components: &mut TalkComponents::none(), moment: Moment::START, asks: ComponentAsks::default() });
    }
    assert_eq!(files.reads.get(), 1);
}

#[test]
fn a_picture_that_could_not_be_read_is_tried_again() {
    let talk = parse_talk("![a](a.png)\n");
    let files = CountedFiles { bytes: b"not yet a png".to_vec(), reads: Cell::new(0) };
    let registry = builtin_registry();
    let mut fonts = Fonts::bundled_only();
    let mut pictures = PictureCache::default();
    for _ in 0..2 {
        layout_slide(&talk, 0, &mut LayoutEnv { fonts: &mut fonts, registry: &registry, files: &files, pictures: &mut pictures, components: &mut TalkComponents::none(), moment: Moment::START, asks: ComponentAsks::default() });
    }
    assert_eq!(files.reads.get(), 2);
}

// Icons.

#[test]
fn an_icon_is_drawn_in_its_colour_and_at_its_size() {
    let (slide, _) = lay_out("<Icon name=\"arrow-right\" x=\"0\" size=\"32\" color=\"#ff0000\" />\n", &[]);
    let (pixmap, rect) = picture_of(&slide.layers[1]);
    assert_eq!((pixmap.width(), pixmap.height()), (32, 32));
    assert_eq!((rect.w, rect.h), (32.0, 32.0));
    let reddest = pixmap.pixels().iter().map(|pixel| (pixel.red(), pixel.green(), pixel.blue())).max().unwrap();
    assert_eq!(reddest, (255, 0, 0));
}

#[test]
fn an_icon_with_no_colour_uses_the_text_colour() {
    let (slide, _) = lay_out("<Icon name=\"arrow-right\" x=\"0\" size=\"32\" />\n", &[]);
    let (pixmap, _) = picture_of(&slide.layers[1]);
    let text = toolkit::Theme::default().text;
    assert!(pixmap.pixels().iter().any(|p| (p.red(), p.green(), p.blue(), p.alpha()) == (text.r, text.g, text.b, 255)));
}

#[test]
fn an_icon_size_that_cannot_be_read_is_the_default() {
    for size in ["many", "NaN", "-3", "0"] {
        let (slide, _) = lay_out(&format!("<Icon name=\"arrow-right\" x=\"0\" size=\"{size}\" />\n"), &[]);
        assert_eq!(picture_of(&slide.layers[1]).1, Rect { x: 0.0, y: 0.0, w: 64.0, h: 64.0 });
    }
}

// Nothing a talk contains may crash the app.

#[test]
fn numbers_a_tag_can_hold_cannot_crash_it() {
    let sources = [
        "<Box x=\"0\" y=\"0\" w=\"0\" h=\"0\" fill=\"red\" outline=\"black\" outline-width=\"4\" radius=\"9\" />\n",
        "<Box x=\"0\" y=\"0\" w=\"100\" h=\"100\" fill=\"red\" outline=\"black\" outline-width=\"NaN\" radius=\"NaN\" />\n",
        "<Box x=\"0\" y=\"0\" w=\"100\" h=\"100\" fill=\"red\" outline=\"black\" outline-width=\"-4\" radius=\"-5\" />\n",
        "<Box x=\"0\" y=\"0\" w=\"100\" h=\"100\" fill=\"red\" radius=\"inf\" />\n",
        "<Shape kind=\"arrow\" x=\"0\" w=\"0\" h=\"0\" outline=\"black\" />\n",
        "<Shape kind=\"arrow\" x=\"0\" w=\"2\" h=\"1\" outline=\"black\" outline-width=\"40\" />\n",
        "<Shape kind=\"line\" x=\"0\" w=\"0\" h=\"0\" />\n",
        "<Shape kind=\"callout\" x=\"0\" w=\"1\" h=\"1\" fill=\"red\">\n\nHi\n\n</Shape>\n",
        "<Shape kind=\"callout\" x=\"0\" w=\"0\" h=\"0\" fill=\"red\" />\n",
        "<Shape kind=\"ellipse\" x=\"0\" w=\"0\" h=\"50\" fill=\"red\" />\n",
        "<Shape kind=\"rounded\" x=\"0\" w=\"50\" h=\"0\" fill=\"red\" />\n",
        "<Icon name=\"\" x=\"0\" />\n",
        "<Picture src=\"\" x=\"0\" />\n",
        "<Picture src=\"a.png\" x=\"0\" w=\"0\" h=\"0\" />\n",
        "<Picture src=\"a.svg\" x=\"0\" />\n",
        "<Mystery x=\"0\" w=\"0\" h=\"0\" />\n",
        "<Mystery x=\"0\" w=\"1\" h=\"1\" rotate=\"45\" />\n",
    ];
    let files = [("a.png", png(4, 4)), ("a.svg", red_svg("width=\"1e30\" height=\"1e-30\""))];
    for source in sources {
        let (slide, mut fonts) = lay_out(source, &files);
        for layer in &slide.layers {
            render_layer(layer, 1.0, &mut fonts);
        }
    }
}

#[test]
fn an_outline_and_an_icon_can_only_be_so_big() {
    let (slide, _) = lay_out("<Box x=\"0\" outline=\"black\" outline-width=\"1e30\" />\n", &[]);
    assert!(strokes(&slide.layers[1])[0].1 <= 256.0);

    let (slide, _) = lay_out("<Icon name=\"arrow-right\" x=\"0\" y=\"0\" w=\"10\" h=\"10\" size=\"99999999\" />\n", &[]);
    let (pixmap, rect) = picture_of(&slide.layers[1]);
    assert_eq!((pixmap.width(), rect.w), (3840, 3840.0));
}

#[test]
fn tags_nested_absurdly_deep_stop_with_a_notice() {
    let depth = 40;
    let source = format!("{}Deep\n{}", "<Box>\n\n".repeat(depth), "\n</Box>\n".repeat(depth));
    let (slide, mut fonts) = lay_out(&source, &[]);
    assert!(notice_about(&slide, "nested"));
    render_layer(&slide.layers[1], 1.0, &mut fonts);
}

#[test]
fn lists_nested_absurdly_deep_stop_with_a_notice() {
    // As deep as the format reads, which with the paragraph in the last item is past the
    // 64 levels the layout goes to.
    let source: String = (0..64).map(|depth| format!("{}- item\n", "  ".repeat(depth))).collect();
    let (slide, mut fonts) = lay_out(&source, &[]);
    assert!(slide.notices.iter().any(|notice| notice.0 == "Some content is nested too deeply to show"), "{:?}", slide.notices);
    render_layer(&slide.layers[1], 1.0, &mut fonts);
}

#[test]
fn ordinary_nesting_is_left_alone() {
    let depth = 8;
    let source = format!("{}Deep\n{}", "<Box>\n\n".repeat(depth), "\n</Box>\n".repeat(depth));
    let (slide, _) = lay_out(&source, &[]);
    assert_eq!(slide.notices, []);
    assert_eq!(text_of(&slide.layers[1]).trim_end(), "Deep");
}

fn assert_svg_shows_a_placeholder(svg: String) {
    let (slide, _) = lay_out("<Picture src=\"a.svg\" x=\"0\" w=\"100\" h=\"100\" />\n", &[("a.svg", svg.into_bytes())]);
    assert!(notice_about(&slide, "a.svg"));
    assert!(!has_picture(slide.layers.last().unwrap()));
}

fn svg(side: u32, body: &str) -> String {
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{side}\" height=\"{side}\">{body}</svg>")
}

fn pattern(id: &str, side: u32, inside: &str) -> String {
    format!("<pattern id=\"{id}\" patternUnits=\"userSpaceOnUse\" width=\"{side}\" height=\"{side}\">{inside}</pattern>")
}

#[test]
fn svg_with_a_huge_pattern_shows_a_placeholder() {
    let tile = pattern("p", 12000, "<rect width=\"12000\" height=\"12000\" fill=\"red\"/>");
    assert_svg_shows_a_placeholder(svg(10, &format!("<defs>{tile}</defs><rect width=\"10\" height=\"10\" fill=\"url(#p)\"/>")));
}

#[test]
fn svg_with_a_huge_layer_shows_a_placeholder() {
    assert_svg_shows_a_placeholder(svg(
        4096,
        "<g opacity=\"0.5\"><rect x=\"-8192\" y=\"-8192\" width=\"20480\" height=\"20480\" fill=\"red\"/></g>",
    ));
}

#[test]
fn an_ordinary_svg_still_draws() {
    let body = "<defs>\
        <pattern id=\"p\" patternUnits=\"userSpaceOnUse\" width=\"32\" height=\"32\"><circle cx=\"16\" cy=\"16\" r=\"8\" fill=\"red\"/></pattern>\
        <clipPath id=\"c\"><circle cx=\"256\" cy=\"256\" r=\"200\"/></clipPath>\
        <mask id=\"m\"><rect width=\"512\" height=\"512\" fill=\"white\"/></mask>\
        <filter id=\"f\"><feGaussianBlur stdDeviation=\"4\"/></filter>\
        </defs>\
        <rect width=\"512\" height=\"512\" fill=\"url(#p)\"/>\
        <g opacity=\"0.5\" clip-path=\"url(#c)\" mask=\"url(#m)\"><rect width=\"512\" height=\"512\" fill=\"blue\"/></g>\
        <g style=\"mix-blend-mode:multiply\"><rect x=\"100\" y=\"100\" width=\"100\" height=\"100\" fill=\"green\" filter=\"url(#f)\"/></g>";
    let (slide, _) = lay_out("<Picture src=\"a.svg\" x=\"0\" />\n", &[("a.svg", svg(512, body).into_bytes())]);
    assert_eq!(slide.notices, []);
    let (picture, _) = picture_of(slide.layers.last().unwrap());
    assert_eq!((picture.width(), picture.height()), (512, 512));
    assert!(painted(picture));
}

#[test]
fn svg_with_a_huge_pattern_inside_a_small_one_shows_a_placeholder() {
    let inner = pattern("big", 12000, "<rect width=\"12000\" height=\"12000\" fill=\"red\"/>");
    let outer = pattern("small", 8, "<rect width=\"8\" height=\"8\" fill=\"url(#big)\"/>");
    assert_svg_shows_a_placeholder(svg(10, &format!("<defs>{inner}{outer}</defs><rect width=\"10\" height=\"10\" fill=\"url(#small)\"/>")));
}

#[test]
fn svg_with_a_huge_filter_region_shows_a_placeholder() {
    let filter = "<filter id=\"f\" filterUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" width=\"12000\" height=\"12000\"><feFlood flood-color=\"red\"/></filter>";
    assert_svg_shows_a_placeholder(svg(10, &format!("<defs>{filter}</defs><rect width=\"10\" height=\"10\" fill=\"blue\" filter=\"url(#f)\"/>")));
}

#[test]
fn svg_with_a_very_long_filter_shows_a_placeholder() {
    let steps = "<feFlood flood-color=\"red\"/>".repeat(100);
    let filter = format!("<filter id=\"f\" filterUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" width=\"1000\" height=\"1000\">{steps}</filter>");
    assert_svg_shows_a_placeholder(svg(1000, &format!("<defs>{filter}</defs><rect width=\"1000\" height=\"1000\" fill=\"blue\" filter=\"url(#f)\"/>")));
}

#[test]
fn svg_with_a_long_chain_of_clips_or_masks_shows_a_placeholder() {
    let chain = |tag: &str, link: &str, inside: &str| -> String {
        (0..40)
            .map(|n| {
                let next = if n + 1 < 40 { format!(" {link}=\"url(#c{})\"", n + 1) } else { String::new() };
                format!("<{tag} id=\"c{n}\"{next}>{inside}</{tag}>")
            })
            .collect()
    };
    let rect = "<rect width=\"2000\" height=\"2000\"/>";
    let drawn = |link: &str| format!("<g {link}=\"url(#c0)\"><rect width=\"2000\" height=\"2000\" fill=\"red\"/></g>");
    assert_svg_shows_a_placeholder(svg(2000, &format!("<defs>{}</defs>{}", chain("clipPath", "clip-path", rect), drawn("clip-path"))));
    assert_svg_shows_a_placeholder(svg(2000, &format!("<defs>{}</defs>{}", chain("mask", "mask", rect), drawn("mask"))));
}

fn text_items(layer: &LayerSpec) -> Vec<&ShapedText> {
    layer
        .items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::Text { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

const RED: (u8, u8, u8) = (255, 0, 0);

/// The colour of the first glyph of each text in a layer.
fn glyph_colours(layer: &LayerSpec) -> Vec<(u8, u8, u8)> {
    text_items(layer)
        .iter()
        .map(|text| {
            let colour = text.buffer.layout_runs().next().unwrap().glyphs[0].color_opt.unwrap();
            (colour.r(), colour.g(), colour.b())
        })
        .collect()
}

#[test]
fn block_level_text_styles_its_children() {
    let (plain, _) = lay_out("Hello\n", &[]);
    let (slide, _) = lay_out("<Text color=\"#ff0000\" size=\"100\" bold italic underline>\n\nHello\n\n- World\n\n</Text>\n", &[]);
    assert_eq!(slide.notices, []);
    let layer = slide.layers.last().unwrap();
    assert_eq!(text_of(layer).trim_end(), "Hello");
    assert_eq!(glyph_colours(layer)[0], RED);
    let texts = text_items(layer);
    assert!(texts[0].height > text_items(plain.layers.last().unwrap())[0].height);
    assert!(!texts[0].underlines.is_empty());
}

#[test]
fn block_level_text_stops_at_its_closing_tag_and_reaches_inside_other_tags() {
    let (slide, _) = lay_out("<Text color=\"#ff0000\">\n\n<Box>\n\nIn a box\n\n</Box>\n\n</Text>\n\nOutside\n", &[]);
    assert_eq!(slide.notices, []);
    let colours = glyph_colours(slide.layers.last().unwrap());
    assert_eq!(colours.len(), 2);
    assert_eq!(colours[0], RED);
    assert_ne!(colours[1], RED);
}

#[test]
fn a_spans_own_colour_wins_over_a_block_level_text() {
    let (slide, _) = lay_out("<Text color=\"#ff0000\">\n\n<Text color=\"#00ff00\">green</Text> then red\n\n</Text>\n", &[]);
    assert_eq!(slide.notices, []);
    assert_eq!(glyph_colours(slide.layers.last().unwrap()), [(0, 255, 0)]);
}

#[test]
fn block_level_text_styles_code_too() {
    let (slide, _) = lay_out("<Text color=\"#ff0000\">\n\n```\nlet a = 1;\n```\n\n</Text>\n", &[]);
    assert_eq!(slide.notices, []);
    assert_eq!(glyph_colours(slide.layers.last().unwrap()), [RED]);
}

#[test]
fn a_placed_block_level_text_styles_its_children() {
    let (slide, _) = lay_out("<Text x=\"100\" y=\"100\" color=\"#ff0000\">\n\nHello\n\n</Text>\n", &[]);
    assert_eq!(slide.notices, []);
    let layer = slide.layers.last().unwrap();
    assert_eq!(text_of(layer).trim_end(), "Hello");
    assert_eq!(glyph_colours(layer), [RED]);
}

fn first_text_position(layer: &LayerSpec) -> (f32, f32) {
    layer
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Text { x, y, .. } => Some((*x, *y)),
            _ => None,
        })
        .expect("the layer draws text")
}

#[test]
fn a_box_with_a_fill_pads_its_text() {
    let filled = "<Box x=\"0\" y=\"0\" w=\"400\" h=\"200\" fill=\"#dbeafe\">\n\nHi\n\n</Box>\n";
    let (slide, _) = lay_out(filled, &[]);
    let (x, y) = first_text_position(slide.layers.last().unwrap());
    assert!(x >= 24.0 && y >= 24.0, "({x}, {y})");
    let outlined = filled.replace("fill=\"#dbeafe\"", "outline=\"black\"");
    let (slide, _) = lay_out(&outlined, &[]);
    let (x, y) = first_text_position(slide.layers.last().unwrap());
    assert!(x >= 24.0 && y >= 24.0, "({x}, {y})");
    let bare = filled.replace(" fill=\"#dbeafe\"", "");
    let (slide, _) = lay_out(&bare, &[]);
    assert_eq!(first_text_position(slide.layers.last().unwrap()), (0.0, 0.0));
    let tight = filled.replace("fill=", "padding=\"0\" fill=");
    let (slide, _) = lay_out(&tight, &[]);
    assert_eq!(first_text_position(slide.layers.last().unwrap()), (0.0, 0.0));
}

#[test]
fn the_names_kept_for_the_slide_hold_every_placement_attribute() {
    for name in PLACEMENT_ATTRS {
        assert!(toolkit::RESERVED_NAMES.contains(&name), "{name}");
    }
}

#[test]
fn a_shapes_kind_is_a_choice() {
    let schema = (builtin_registry().get("Shape").unwrap().schema)();
    let kind = schema.props.iter().find(|prop| prop.name == "kind").expect("a shape has a kind");
    let choices = ["rect", "rounded", "ellipse", "line", "arrow", "callout"].map(String::from).to_vec();
    assert_eq!(kind.kind, toolkit::PropKind::Choice(choices));
}

/// Everything a layer draws and is, written out, to compare with what it was.
fn described(layer: &LayerSpec) -> String {
    let mut out = format!("{:?} turned {}\n", layer.rect, layer.rotate);
    for item in layer.items.iter() {
        out += &match item {
            DisplayItem::Fill { path, color } => format!("fill {color:?} {:?}\n", path.segments().collect::<Vec<_>>()),
            DisplayItem::Stroke { path, color, width } => format!("stroke {color:?} {width} {:?}\n", path.segments().collect::<Vec<_>>()),
            DisplayItem::Text { x, y, .. } => format!("text at {x} {y}\n"),
            DisplayItem::Picture { rect, .. } => format!("picture {rect:?}\n"),
        };
    }
    for native in &layer.native {
        out += &match &native.content {
            NativeContent::Shape { kind, fill, outline, outline_width, radius, paragraphs, inset, centered } => format!(
                "shape {:?} {kind:?} {fill:?} {outline:?} {outline_width} {radius} {} {inset} {centered}\n",
                native.rect,
                paragraphs.len()
            ),
            _ => format!("other {:?}\n", native.rect),
        };
    }
    out
}

/// What `<Shape kind="circle" …>` drew before a shape's kind was a choice.
const UNKNOWN_KIND_DRAWN: &str = "Rect { x: 10.0, y: 20.0, w: 300.0, h: 200.0 } turned 0
fill Color { r: 255, g: 0, b: 0, a: 255 } [MoveTo(Point { x: 0.0, y: 0.0 }), LineTo(Point { x: 300.0, y: 0.0 }), LineTo(Point { x: 300.0, y: 200.0 }), LineTo(Point { x: 0.0, y: 200.0 }), Close]
stroke Color { r: 0, g: 0, b: 0, a: 255 } 1 [MoveTo(Point { x: 0.0, y: 0.0 }), LineTo(Point { x: 300.0, y: 0.0 }), LineTo(Point { x: 300.0, y: 200.0 }), LineTo(Point { x: 0.0, y: 200.0 }), Close]
shape Rect { x: 0.0, y: 0.0, w: 300.0, h: 200.0 } Rect Some(Color { r: 255, g: 0, b: 0, a: 255 }) Some(Color { r: 0, g: 0, b: 0, a: 255 }) 1 0 0 0 true
";

#[test]
fn an_unknown_kind_still_draws_a_rectangle() {
    let shape = |kind: &str| format!("<Shape {kind}x=\"10\" y=\"20\" w=\"300\" h=\"200\" fill=\"#ff0000\" outline=\"#000000\" />\n");
    let (slide, _) = lay_out(&shape("kind=\"circle\" "), &[]);
    assert!(slide.notices.is_empty(), "{:?}", slide.notices);
    let drawn = described(slide.layers.last().unwrap());
    assert_eq!(drawn, UNKNOWN_KIND_DRAWN, "as it was drawn before");
    for other in ["kind=\"rect\" ", "kind=\"Ellipse\" ", "kind=\"\" ", ""] {
        let (slide, _) = lay_out(&shape(other), &[]);
        assert!(slide.notices.is_empty(), "{other}: {:?}", slide.notices);
        assert_eq!(described(slide.layers.last().unwrap()), drawn, "{other}");
    }
}

#[test]
fn icon_names_are_sorted_and_2776() {
    let names = icon_names();
    assert_eq!(names.len(), 2776);
    assert!(names.windows(2).all(|pair| pair[0] < pair[1]), "in order, each once");
    assert!(names.contains(&"arrow-right"));
}

#[test]
fn every_listed_icon_draws() {
    for name in icon_names() {
        let icon = icon_preview(name, 24, [0x11, 0x22, 0x33, 0xff]).unwrap_or_else(|| panic!("{name} draws"));
        assert_eq!((icon.width(), icon.height()), (24, 24), "{name}");
        assert!(painted(&icon), "{name} has some ink");
    }
    assert!(icon_preview("no-such-icon", 24, [0, 0, 0, 255]).is_none());
    assert!(icon_preview("arrow-right", 0, [0, 0, 0, 255]).is_none(), "nothing to draw at no size");
    let big = icon_preview("arrow-right", u32::MAX, [0, 0, 0, 255]).expect("held to a size that can be drawn");
    assert!(big.width() <= 4096);
}

// Videos.

/// A picture `width` by `height` of one colour, as straight BGRA.
fn solid(width: u32, height: u32, bgra: [u8; 4]) -> Vec<u8> {
    bgra.repeat((width * height) as usize)
}

/// A video `width` by `height` of `frames` pictures at 10 a second, each of the colour `colour`
/// gives it, as BGRA.
fn video_of(width: u32, height: u32, frames: u32, colour: impl Fn(u32) -> [u8; 4]) -> Vec<u8> {
    slides_video::make_test_video(width, height, frames, 10, |at| solid(width, height, colour(at)))
}

const RED_BGRA: [u8; 4] = [0, 0, 255, 255];
const BLUE_BGRA: [u8; 4] = [255, 0, 0, 255];

/// The colour in the middle of `pixmap`, as red, green and blue.
fn middle_of(pixmap: &tiny_skia::Pixmap) -> (u8, u8, u8) {
    let pixel = pixmap.pixel(pixmap.width() / 2, pixmap.height() / 2).expect("a pixel");
    (pixel.red(), pixel.green(), pixel.blue())
}

fn close_to((r, g, b): (u8, u8, u8), (er, eg, eb): (u8, u8, u8)) -> bool {
    r.abs_diff(er) <= 12 && g.abs_diff(eg) <= 12 && b.abs_diff(eb) <= 12
}

#[test]
fn a_videos_still_is_its_first_frame() {
    let video = video_of(64, 36, 3, |at| if at == 0 { RED_BGRA } else { BLUE_BGRA });
    let (slide, _) = lay_out("<Video src=\"assets/clip.mp4\" x=\"10\" y=\"20\" w=\"640\" h=\"360\" loop />\n", &[("assets/clip.mp4", video)]);
    let layer = &slide.layers[1];
    let (pixmap, rect) = picture_of(layer);
    assert!(close_to(middle_of(pixmap), (255, 0, 0)), "{:?}", middle_of(pixmap));
    assert_eq!(rect, Rect { x: 0.0, y: 0.0, w: 640.0, h: 360.0 });
    let on = layer.video.as_ref().expect("the layer plays a video");
    assert_eq!(on.src, "assets/clip.mp4");
    assert!(on.looped);
    assert_eq!((on.rect, on.rotate), (layer.rect, layer.rotate));
    assert!(slide.notices.is_empty(), "{:?}", slide.notices);
    // Anything else plays nothing.
    assert!(slide.layers[0].video.is_none());
}

#[test]
fn a_poster_is_used_when_named() {
    let video = video_of(64, 36, 2, |_| RED_BGRA);
    let mut poster = Vec::new();
    image::RgbaImage::from_pixel(16, 9, image::Rgba([0, 255, 0, 255]))
        .write_to(&mut std::io::Cursor::new(&mut poster), image::ImageFormat::Png)
        .unwrap();
    let files = [("assets/clip.mp4", video), ("assets/still.png", poster)];
    let (slide, _) = lay_out("<Video src=\"assets/clip.mp4\" poster=\"assets/still.png\" x=\"0\" w=\"640\" h=\"360\" />\n", &files);
    let (pixmap, _) = picture_of(&slide.layers[1]);
    assert!(close_to(middle_of(pixmap), (0, 255, 0)), "{:?}", middle_of(pixmap));
    assert!(!slide.layers[1].video.as_ref().unwrap().looped);
    // A poster that isn't a picture leaves the video's own still.
    let files = [("assets/clip.mp4", files[0].1.clone()), ("assets/still.png", b"not a picture".to_vec())];
    let (slide, _) = lay_out("<Video src=\"assets/clip.mp4\" poster=\"assets/still.png\" x=\"0\" w=\"640\" h=\"360\" />\n", &files);
    assert!(close_to(middle_of(picture_of(&slide.layers[1]).0), (255, 0, 0)));
}

#[test]
fn a_video_with_no_size_takes_its_own_shape() {
    let small = video_of(64, 48, 1, |_| RED_BGRA);
    let (slide, _) = lay_out("<Video src=\"assets/small.mp4\" x=\"5\" y=\"6\" />\n", &[("assets/small.mp4", small.clone())]);
    assert_eq!(slide.layers[1].rect, Rect { x: 5.0, y: 6.0, w: 64.0, h: 48.0 });
    // One side given, the other keeps the video's shape.
    let (slide, _) = lay_out("<Video src=\"assets/small.mp4\" x=\"0\" w=\"320\" />\n", &[("assets/small.mp4", small)]);
    assert_eq!((slide.layers[1].rect.w, slide.layers[1].rect.h), (320.0, 240.0));
    // A large one is fitted within 960 by 540.
    let large = video_of(1280, 720, 1, |_| RED_BGRA);
    let (slide, _) = lay_out("<Video src=\"assets/large.mp4\" x=\"0\" />\n", &[("assets/large.mp4", large)]);
    assert_eq!((slide.layers[1].rect.w, slide.layers[1].rect.h), (960.0, 540.0));
}

#[test]
fn a_video_that_cannot_be_read_draws_a_panel_and_says_why() {
    let source = "<Video src=\"assets/clip.mp4\" x=\"0\" w=\"640\" h=\"360\" />\n";
    for (bytes, why) in [
        (Some(b"just some words".to_vec()), "This file isn't a video."),
        (None, "This video couldn't be read."),
    ] {
        let files: Vec<(&str, Vec<u8>)> = bytes.into_iter().map(|bytes| ("assets/clip.mp4", bytes)).collect();
        let (slide, _) = lay_out(source, &files);
        let layer = &slide.layers[1];
        assert!(!has_picture(layer), "{why}");
        assert!(fills(layer).len() >= 2, "a panel and its play mark: {why}");
        assert!(layer.video.is_some(), "it's still a video: {why}");
        assert!(slide.notices.iter().any(|notice| notice.0 == why), "{:?}", slide.notices);
    }

    let talk = format!("# One\n\n{source}");
    let files = MapFiles([("assets/clip.mp4".to_string(), b"just some words".to_vec())].into());
    let mut deck = Deck::open(Box::new(files), Fonts::bundled_only(), talk.clone());
    let start = talk.find("<Video").unwrap();
    let range = start..start + source.trim_end().len();
    assert_eq!(deck.video_problem(0, &range).as_deref(), Some("This file isn't a video."));
    assert!(deck.notices(0).iter().any(|notice| notice == "This file isn't a video."));
    // Anything else has no problem to tell of.
    assert_eq!(deck.video_problem(0, &(0..5)), None);
}

#[test]
fn a_video_with_no_file_draws_a_panel_and_says_so() {
    for source in ["<Video x=\"0\" w=\"640\" h=\"360\" />\n", "<Video src=\"  \" x=\"0\" w=\"640\" h=\"360\" />\n"] {
        let (slide, _) = lay_out(source, &[]);
        let layer = &slide.layers[1];
        assert!(!has_picture(layer), "{source}");
        assert!(fills(layer).len() >= 2, "a panel and its play mark: {source}");
        let notices: Vec<&str> = slide.notices.iter().map(|notice| notice.0.as_str()).collect();
        assert_eq!(notices, [slides_engine::VIDEO_NO_FILE], "{source}");
    }
    assert_eq!(slides_engine::VIDEO_NO_FILE, "This video has no file. Insert a video to choose one.");
}

#[test]
fn a_deck_says_which_videos_show_as_pictures() {
    let mut poster = Vec::new();
    image::RgbaImage::from_pixel(16, 9, image::Rgba([0, 255, 0, 255]))
        .write_to(&mut std::io::Cursor::new(&mut poster), image::ImageFormat::Png)
        .unwrap();
    let files = MapFiles(
        [
            ("assets/clip.mp4".to_string(), video_of(64, 36, 2, |_| RED_BGRA)),
            ("assets/words.mp4".to_string(), b"just some words".to_vec()),
            ("assets/still.png".to_string(), poster),
        ]
        .into(),
    );
    let talk = "# One\n\n<Video src=\"assets/clip.mp4\" x=\"0\" />\n\n<Video src=\"assets/words.mp4\" x=\"700\" />\n";
    let mut deck = Deck::open(Box::new(files), Fonts::bundled_only(), talk.to_string());
    deck.notices(0);
    assert!(deck.video_shows_a_picture("assets/clip.mp4", None));
    assert!(!deck.video_shows_a_picture("assets/words.mp4", None), "its panel");
    assert!(deck.video_shows_a_picture("assets/words.mp4", Some("assets/still.png")), "its still");
    assert!(!deck.video_shows_a_picture("assets/words.mp4", Some("assets/clip.mp4")), "a still that isn't a picture");
    assert!(!deck.video_shows_a_picture("", None), "no file");
}

/// Files that count the bytes read from each in pieces, and those of each read whole apart.
struct CountingFiles {
    files: HashMap<String, Vec<u8>>,
    read: std::sync::Arc<std::sync::Mutex<HashMap<String, u64>>>,
}

struct CountingReader {
    path: String,
    inner: std::io::Cursor<Vec<u8>>,
    read: std::sync::Arc<std::sync::Mutex<HashMap<String, u64>>>,
}

impl std::io::Read for CountingReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        *self.read.lock().unwrap().entry(self.path.clone()).or_default() += read as u64;
        Ok(read)
    }
}

impl std::io::Seek for CountingReader {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(to)
    }
}

impl TalkFiles for CountingFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let bytes = self.files.get(path).cloned()?;
        *self.read.lock().unwrap().entry(format!("{path}, whole")).or_default() += bytes.len() as u64;
        Some(bytes)
    }

    fn open(&self, path: &str) -> Option<Box<dyn ReadSeek>> {
        let bytes = self.files.get(path)?.clone();
        Some(Box::new(CountingReader { path: path.to_string(), inner: std::io::Cursor::new(bytes), read: self.read.clone() }))
    }

    fn stamp(&self, path: &str) -> Option<(u64, std::time::SystemTime)> {
        Some((self.files.get(path)?.len() as u64, std::time::SystemTime::UNIX_EPOCH))
    }

    fn len(&self, path: &str) -> Option<u64> {
        Some(self.files.get(path)?.len() as u64)
    }
}

#[test]
fn a_talk_of_twenty_videos_opens_reading_only_their_starts() {
    // Pictures of noise, which hardly compress, until the file is 5 MB.
    let noise = |at: u32| -> Vec<u8> {
        let mut state = 0x9e37_79b9_u32.wrapping_mul(at + 1);
        (0..320 * 240)
            .flat_map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let [b, g, r, _] = state.to_le_bytes();
                [b, g, r, 255]
            })
            .collect()
    };
    let video = slides_video::make_test_video(320, 240, 90, 30, noise);
    assert!(video.len() >= 5 * 1024 * 1024, "{} bytes", video.len());
    let paths: Vec<String> = (0..20).map(|at| format!("assets/clip{at}.mp4")).collect();
    let talk: Vec<String> = paths.iter().map(|path| format!("# Slide\n\n<Video src=\"{path}\" x=\"0\" w=\"640\" h=\"360\" />\n")).collect();
    let read = std::sync::Arc::new(std::sync::Mutex::new(HashMap::new()));
    let files = CountingFiles { files: paths.iter().map(|path| (path.clone(), video.clone())).collect(), read: read.clone() };
    let mut deck = Deck::open(Box::new(files), Fonts::bundled_only(), talk.join("\n---\n\n"));
    for slide in 0..20 {
        assert!(deck.layers(slide, 0.25).len() >= 2);
        deck.thumbnail(slide, 160);
    }
    let read = read.lock().unwrap();
    assert_eq!(read.len(), 20, "every video's still was read, and none whole: {:?}", read.keys());
    for (path, bytes) in read.iter() {
        assert!(*bytes < 1024 * 1024, "{path}: {bytes} bytes read");
    }
}

#[test]
fn a_still_made_aside_is_taken_in_and_its_slide_shown_again() {
    let video = video_of(64, 36, 2, |_| RED_BGRA);
    let files: HashMap<String, Vec<u8>> = [("assets/clip.mp4".to_string(), video.clone())].into();
    // Unsized: it takes its own shape from what its file says, before its still is made.
    let talk = "# One\n\n<Video src=\"assets/clip.mp4\" x=\"0\" />\n\n---\n\n# Two\n";
    // Files that can say how long each is, as a talk's on disk can.
    struct Sized(MapFiles);
    impl TalkFiles for Sized {
        fn read(&self, path: &str) -> Option<Vec<u8>> {
            self.0.read(path)
        }
        fn len(&self, path: &str) -> Option<u64> {
            self.0.0.get(path).map(|bytes| bytes.len() as u64)
        }
    }
    let mut deck = Deck::open(Box::new(Sized(MapFiles(files))), Fonts::bundled_only(), talk.to_string());
    let wanted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let told = wanted.clone();
    deck.make_stills_aside(
        std::sync::Arc::new(move || {
            told.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }),
        std::sync::Arc::new(|path: &str| (path == "assets/clip.mp4").then_some((64, 36))),
    );
    let version = deck.slide_version(0);
    let video_layer = |deck: &mut Deck| deck.specs(0).iter().find(|spec| spec.video.is_some()).expect("the video has a layer").clone();
    // A panel while it's made, and nothing said of it.
    let panel = video_layer(&mut deck);
    assert!(!has_picture(&panel));
    assert_eq!((panel.rect.w, panel.rect.h), (64.0, 36.0), "its own shape while its still is made");
    assert!(deck.stills_coming());
    assert!(deck.notices(0).is_empty());
    assert_eq!(wanted.load(std::sync::atomic::Ordering::SeqCst), 1, "told once");
    let mut jobs = deck.still_jobs();
    assert_eq!(jobs.len(), 1);
    assert!(deck.still_jobs().is_empty(), "given once");
    let job = jobs.pop().unwrap();
    assert_eq!(job.path(), "assets/clip.mp4");
    // Made on another thread.
    let made = std::thread::spawn(move || job.make(Some(Box::new(std::io::Cursor::new(video))))).join().unwrap();
    assert_eq!(deck.take_still(made), vec![0]);
    assert_ne!(deck.slide_version(0), version, "the slide is drawn again");
    assert!(!deck.stills_coming());
    let layer = video_layer(&mut deck);
    assert!(close_to(middle_of(picture_of(&layer).0), (255, 0, 0)));
    assert_eq!(layer.rect, panel.rect, "nothing moved when it came");
    assert!(deck.still_jobs().is_empty(), "it's made once");
    // The talk's files taken anew, as after a save, the same file keeps its still.
    let files: HashMap<String, Vec<u8>> = [("assets/clip.mp4".to_string(), video_of(64, 36, 2, |_| RED_BGRA))].into();
    deck.set_files(Box::new(Sized(MapFiles(files))));
    let layer = video_layer(&mut deck);
    assert!(has_picture(&layer), "the still is kept");
    assert!(deck.still_jobs().is_empty(), "and isn't made again");
    assert_eq!(wanted.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn a_video_keeps_its_shape_as_a_picture_does() {
    let talk = parse_talk("# One\n\n<Video src=\"assets/clip.mp4\" x=\"0\" w=\"640\" h=\"360\" />\n");
    let mut deck = Deck::open(Box::new(MapFiles(HashMap::new())), Fonts::bundled_only(), talk.source.clone());
    let layout = deck.text_layout(0);
    let video = layout.elements().iter().find(|element| talk.source[element.source.clone()].starts_with("<Video")).expect("the video is a thing on the slide");
    assert!(video.keeps_shape);
    assert!(!video.holds_text);
}

#[test]
fn loop_is_read_as_yes_or_no() {
    let video = video_of(64, 36, 1, |_| RED_BGRA);
    for (written, looped, said) in [
        (" loop", true, false),
        (" loop=\"true\"", true, false),
        (" loop=\"yes\"", true, false),
        (" loop=\"1\"", true, false),
        (" loop=\"no\"", false, false),
        (" loop=\"0\"", false, false),
        ("", false, false),
        (" loop=\"often\"", false, true),
    ] {
        let source = format!("<Video src=\"assets/clip.mp4\" x=\"0\" w=\"64\" h=\"36\"{written} />\n");
        let (slide, _) = lay_out(&source, &[("assets/clip.mp4", video.clone())]);
        assert_eq!(slide.layers[1].video.as_ref().unwrap().looped, looped, "{written}");
        let noticed = slide.notices.iter().any(|notice| notice.0.contains("loop can be yes or no"));
        assert_eq!(noticed, said, "{written}: {:?}", slide.notices);
    }
}

#[test]
fn a_poster_that_cannot_be_shown_is_said() {
    let files = [("assets/clip.mp4", video_of(64, 36, 1, |_| RED_BGRA)), ("assets/still.png", b"not a picture".to_vec())];
    let (slide, _) = lay_out("<Video src=\"assets/clip.mp4\" poster=\"assets/still.png\" x=\"0\" w=\"64\" h=\"36\" />\n", &files);
    assert!(slide.notices.iter().any(|notice| notice.0.contains("can't be shown, so its first picture shows instead")), "{:?}", slide.notices);
}

#[test]
fn a_video_in_the_text_says_it_plays_only_once_placed() {
    let (slide, _) = lay_out("# One\n\n<Video src=\"assets/clip.mp4\" />\n", &[("assets/clip.mp4", video_of(64, 36, 1, |_| RED_BGRA))]);
    assert!(slide.notices.iter().any(|notice| notice.0 == VIDEO_IN_TEXT), "{:?}", slide.notices);
    assert!(slide.layers.iter().all(|layer| layer.video.is_none()), "it doesn't play");
}

/// Files that can be changed under a deck, each stamped with how many times it has been.
type ChangingFiles = std::rc::Rc<std::cell::RefCell<HashMap<String, (Vec<u8>, u64)>>>;

struct Changing(ChangingFiles);

impl TalkFiles for Changing {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.0.borrow().get(path).map(|(bytes, _)| bytes.clone())
    }
    fn stamp(&self, path: &str) -> Option<(u64, std::time::SystemTime)> {
        let files = self.0.borrow();
        let (bytes, changes) = files.get(path)?;
        Some((bytes.len() as u64, std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(*changes)))
    }
    fn len(&self, path: &str) -> Option<u64> {
        self.0.borrow().get(path).map(|(bytes, _)| bytes.len() as u64)
    }
}

#[test]
fn a_video_replaced_under_its_name_gets_a_new_still() {
    let files = std::rc::Rc::new(std::cell::RefCell::new(HashMap::new()));
    files.borrow_mut().insert("assets/clip.mp4".to_string(), (video_of(64, 36, 1, |_| RED_BGRA), 0));
    let talk = "# One\n\n<Video src=\"assets/clip.mp4\" x=\"0\" w=\"640\" h=\"360\" />\n";
    let mut deck = Deck::open(Box::new(Changing(files.clone())), Fonts::bundled_only(), talk.to_string());
    let still = |deck: &mut Deck| {
        let layer = deck.specs(0).iter().find(|spec| spec.video.is_some()).unwrap().clone();
        middle_of(picture_of(&layer).0)
    };
    assert!(close_to(still(&mut deck), (255, 0, 0)));
    files.borrow_mut().insert("assets/clip.mp4".to_string(), (video_of(64, 36, 1, |_| BLUE_BGRA), 1));
    assert_eq!(deck.update(talk.to_string()), vec![0], "the slide shows anew");
    assert!(close_to(still(&mut deck), (0, 0, 255)), "{:?}", still(&mut deck));
}

#[test]
fn stills_count_in_the_picture_budget() {
    let video = video_of(320, 180, 1, |_| RED_BGRA);
    let paths: Vec<String> = (0..6).map(|at| format!("assets/clip{at}.mp4")).collect();
    let talk: Vec<String> = paths.iter().map(|path| format!("# Slide\n\n<Video src=\"{path}\" x=\"0\" w=\"640\" h=\"360\" />\n")).collect();
    let files = MapFiles(paths.iter().map(|path| (path.clone(), video.clone())).collect());
    let mut deck = Deck::open(Box::new(files), Fonts::bundled_only(), talk.join("\n---\n\n"));
    let one = 320 * 180 * 4;
    deck.set_picture_budget(2 * one);
    for slide in 0..6 {
        deck.specs(slide);
        assert!(deck.picture_bytes() >= one, "slide {slide}'s still is counted");
        assert!(deck.picture_bytes() <= 3 * one, "slide {slide}: {} bytes held", deck.picture_bytes());
    }
}

// 3D models.

/// A drawer that waits, once a test is done with it, for its thread to let go of its device: a
/// program ending while a driver lets go of one can crash in the driver.
struct Closing(Option<slides_model::Drawer>);

impl Drop for Closing {
    fn drop(&mut self) {
        if let Some(drawer) = self.0.take() {
            assert!(drawer.close(std::time::Duration::from_secs(30)), "the drawer's thread never ended");
        }
    }
}

impl Closing {
    fn drawer(&self) -> slides_model::Drawer {
        self.0.clone().expect("not yet closed")
    }
}

/// A drawer on the software adapter where there is one, else on any, else `None` after saying
/// why `test` is skipped.
fn model_drawer(test: &str) -> Option<Closing> {
    use slides_model::{AdapterKind, Drawer, DrawerOptions};
    let ready = |drawer: &Drawer| {
        let began = std::time::Instant::now();
        loop {
            if let Some(ready) = drawer.ready() {
                return ready;
            }
            assert!(began.elapsed() < std::time::Duration::from_secs(60), "the drawer never said whether it can draw");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    let software = Closing(Some(Drawer::start_with(DrawerOptions { only: Some(AdapterKind::Software), ..DrawerOptions::default() })));
    if ready(&software.drawer()).is_ok() {
        return Some(software);
    }
    drop(software);
    let any = Closing(Some(Drawer::start()));
    match ready(&any.drawer()) {
        Ok(_) => Some(any),
        Err(why) => {
            eprintln!("{test} skipped: no graphics adapter, not even a software one, can be had here. {why}");
            None
        }
    }
}

/// What draws a model's still with `drawer`, as the app's does.
fn drawing_with(drawer: slides_model::Drawer) -> DrawModel {
    std::sync::Arc::new(move |reader, view, width, height| {
        let model = slides_model::read(Box::new(reader))?;
        let view = view.unwrap_or_else(|| slides_model::View::framing(&model.facts));
        let scene = drawer.upload(model);
        drawer.draw(&scene, view, width, height).recv().map_err(|why| match why {
            slides_model::NotDrawn::Cant(why) => why,
            _ => slides_model::CantDraw::NoAdapter,
        })
    })
}

/// A box a unit a side, its faces along x red, along y green and along z blue.
fn box_model() -> Vec<u8> {
    slides_model::make_test_glb(slides_model::MadeGlb::Boxes)
}

/// A deck of a talk of `source`, its files `files`, drawing its models' stills with `draw`.
fn model_deck(source: &str, files: &[(&str, Vec<u8>)], draw: Option<DrawModel>) -> Deck {
    let files = MapFiles(files.iter().map(|(path, bytes)| (path.to_string(), bytes.clone())).collect());
    let mut deck = Deck::open(Box::new(files), Fonts::bundled_only(), format!("# One\n\n{source}"));
    if let Some(draw) = draw {
        deck.draw_models_with(draw);
    }
    deck
}

fn model_layer(deck: &mut Deck) -> LayerSpec {
    deck.specs(0).iter().find(|spec| spec.model.is_some()).expect("the model has a layer").clone()
}

/// Which of red, green and blue is the most of the colour in the middle of `pixmap`.
fn most_in_the_middle(pixmap: &tiny_skia::Pixmap) -> usize {
    let (r, g, b) = middle_of(pixmap);
    [r, g, b].iter().enumerate().max_by_key(|(_, value)| **value).map(|(at, _)| at).unwrap()
}

#[test]
fn a_models_still_is_drawn_at_its_view() {
    let Some(closing) = model_drawer("a_models_still_is_drawn_at_its_view") else { return };
    let files = [("assets/box.glb", box_model())];
    let front = "0 0 4 0 0.5 0";
    let source = format!("<Model src=\"assets/box.glb\" x=\"100\" y=\"50\" w=\"640\" h=\"360\" view=\"{front}\" />\n");
    let mut deck = model_deck(&source, &files, Some(drawing_with(closing.drawer())));
    let layer = model_layer(&mut deck);
    let (pixmap, rect) = picture_of(&layer);
    assert_eq!(rect, Rect { x: 0.0, y: 0.0, w: 640.0, h: 360.0 });
    assert_eq!((pixmap.width(), pixmap.height()), (640, 360), "drawn a pixel a unit");
    assert_eq!(most_in_the_middle(pixmap), 2, "the front, blue: {:?}", middle_of(pixmap));
    assert_eq!(pixmap.pixel(0, 0).unwrap().alpha(), 0, "clear where the model isn't");
    let on = layer.model.as_ref().unwrap();
    assert_eq!(on.src, "assets/box.glb");
    assert_eq!(on.view, slides_model::View::parse(front));
    assert!(!on.turn);
    assert!(on.views.is_empty());
    assert_eq!((on.rect, on.rotate), (layer.rect, layer.rotate));
    assert_eq!(layer.rect, Rect { x: 100.0, y: 50.0, w: 640.0, h: 360.0 });
    assert!(deck.notices(0).is_empty(), "{:?}", deck.notices(0));
    assert_eq!(deck.model_problem(0, layer.source.as_ref().unwrap()), None);

    // Seen from the right, its red side shows; it turns, and has two views to fly to.
    let source = "<Model src=\"assets/box.glb\" x=\"0\" w=\"320\" h=\"180\" view=\"90 0 4 0 0.5 0\" turn=\"auto\" views=\"0 0 4 0 0.5 0; 0 80 4 0 0.5 0\" />\n";
    let mut deck = model_deck(source, &files, Some(drawing_with(closing.drawer())));
    let layer = model_layer(&mut deck);
    assert_eq!(most_in_the_middle(picture_of(&layer).0), 0, "the side, red: {:?}", middle_of(picture_of(&layer).0));
    let on = layer.model.as_ref().unwrap();
    assert!(on.turn);
    assert_eq!(on.views.len(), 2);
    assert_eq!(on.views[1].pitch, 80.0);
    assert_eq!(deck.models(0).len(), 1);
    assert_eq!(deck.models(0)[0].on, *on);
    assert_eq!(deck.timeline(0).steps, 2, "a click for each view");

    // With no size it's 960 by 540; with no view the whole of it is seen, framed.
    let mut deck = model_deck("<Model src=\"assets/box.glb\" x=\"0\" />\n", &files, Some(drawing_with(closing.drawer())));
    let layer = model_layer(&mut deck);
    assert_eq!((layer.rect.w, layer.rect.h), (960.0, 540.0));
    let (pixmap, _) = picture_of(&layer);
    assert_eq!(pixmap.pixel(pixmap.width() / 2, pixmap.height() / 2).unwrap().alpha(), 255, "the model is in the middle");
    assert_eq!(layer.model.as_ref().unwrap().view, None);
}

#[test]
fn a_model_that_cannot_be_read_draws_a_panel_and_says_why() {
    // Something that draws nothing: what it's asked to draw is read, and what can't be read says so.
    let reads_only: DrawModel = std::sync::Arc::new(|reader, _, _, _| slides_model::read(Box::new(reader)).and(Err(slides_model::CantDraw::NoAdapter)));
    let source = "<Model src=\"assets/box.glb\" x=\"0\" w=\"640\" h=\"360\" />\n";
    let no_adapter = "This computer can't draw 3D models here. The slide shows a still when it's presented somewhere that can.";
    for (bytes, draw, why) in [
        (Some(b"just some words".to_vec()), Some(reads_only.clone()), "This file isn't a 3D model."),
        (None, Some(reads_only.clone()), "This model couldn't be read."),
        (Some(box_model()[..100].to_vec()), Some(reads_only.clone()), "This model is damaged, so it can't be drawn."),
        // Nothing can draw it, here or with nothing given to draw with.
        (Some(box_model()), Some(reads_only.clone()), no_adapter),
        (Some(box_model()), None, no_adapter),
    ] {
        let files: Vec<(&str, Vec<u8>)> = bytes.into_iter().map(|bytes| ("assets/box.glb", bytes)).collect();
        let mut deck = model_deck(source, &files, draw);
        let layer = model_layer(&mut deck);
        assert!(!has_picture(&layer), "{why}");
        assert_eq!(text_of(&layer), why, "the panel says why");
        assert!(layer.model.is_some(), "it's still a model: {why}");
        assert!(deck.notices(0).iter().any(|notice| notice == why), "{why}: {:?}", deck.notices(0));
        assert_eq!(deck.model_problem(0, layer.source.as_ref().unwrap()).as_deref(), Some(why));
    }
    // Anything else has no problem to tell of.
    let mut deck = model_deck(source, &[], None);
    assert_eq!(deck.model_problem(0, &(0..5)), None);

    // No file, a view that can't be read, a turn that isn't auto, views that can't be read, and
    // one in the text: each is said plainly.
    for (source, said) in [
        ("<Model x=\"0\" w=\"640\" h=\"360\" />\n", MODEL_NO_FILE.to_string()),
        ("<Model src=\"assets/box.glb\" x=\"0\" w=\"64\" h=\"36\" view=\"1 2 three\" />\n", MODEL_VIEW_UNREAD.to_string()),
        ("<Model src=\"assets/box.glb\" x=\"0\" w=\"64\" h=\"36\" view=\"0 0 NaN 0 0 0\" />\n", MODEL_VIEW_UNREAD.to_string()),
        ("<Model src=\"assets/box.glb\" x=\"0\" w=\"64\" h=\"36\" turn=\"fast\" />\n", "A 3D model's turn can be auto, or left out to stay still, not fast, so it stays still".to_string()),
        ("<Model src=\"assets/box.glb\" x=\"0\" w=\"64\" h=\"36\" views=\"0 0 4 0 0 0; nonsense\" />\n", "Some of this 3D model's views can't be read, or there are more than 256, so they're left out".to_string()),
        ("<Model src=\"assets/box.glb\" />\n", MODEL_IN_TEXT.to_string()),
    ] {
        let mut deck = model_deck(source, &[("assets/box.glb", box_model())], Some(reads_only.clone()));
        assert!(deck.notices(0).contains(&said), "{source}: {:?}", deck.notices(0));
    }
    let mut deck = model_deck("<Model src=\"assets/box.glb\" x=\"0\" w=\"64\" h=\"36\" views=\"0 0 4 0 0 0; nonsense, 1 2 3 4 5 6\" />\n", &[], None);
    assert_eq!(model_layer(&mut deck).model.unwrap().views.len(), 2, "those that read are kept");
    let mut deck = model_deck("<Model src=\"assets/box.glb\" />\n", &[], None);
    assert!(deck.specs(0).iter().all(|spec| spec.model.is_none()), "one in the text isn't drawn");
}

#[test]
fn a_talk_of_ten_models_opens_reading_only_their_headers() {
    // Large enough that reading one whole would show.
    let model = slides_model::make_test_glb(slides_model::MadeGlb::Grid { cells: 200, texture: 512, jpeg: false });
    assert!(model.len() >= 1024 * 1024, "{} bytes", model.len());
    let paths: Vec<String> = (0..10).map(|at| format!("assets/model{at}.glb")).collect();
    let talk: Vec<String> = paths.iter().map(|path| format!("# Slide\n\n<Model src=\"{path}\" x=\"0\" w=\"640\" h=\"360\" />\n")).collect();
    let read = std::sync::Arc::new(std::sync::Mutex::new(HashMap::new()));
    let files = CountingFiles { files: paths.iter().map(|path| (path.clone(), model.clone())).collect(), read: read.clone() };
    let mut deck = Deck::open(Box::new(files), Fonts::bundled_only(), talk.join("\n---\n\n"));
    let drawn = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = drawn.clone();
    // Draws a picture of one colour, as no adapter is needed to make one aside.
    deck.draw_models_with(std::sync::Arc::new(move |_, _, width, height| {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(slides_model::Picture { width, height, bgra: [0, 0, 255, 255].repeat((width * height) as usize), drawing: Default::default(), reading_back: Default::default() })
    }));
    let wanted = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let told = wanted.clone();
    deck.make_stills_aside(
        std::sync::Arc::new(move || {
            told.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }),
        std::sync::Arc::new(|_: &str| None),
    );
    for slide in 0..10 {
        assert!(deck.layers(slide, 0.25).len() >= 2);
        deck.thumbnail(slide, 160);
        assert!(deck.notices(slide).is_empty(), "nothing said while it's drawn: {:?}", deck.notices(slide));
    }
    for (path, bytes) in read.lock().unwrap().iter() {
        assert!(!path.ends_with("whole") && *bytes <= 64 * 1024, "{path}: {bytes} bytes read");
    }
    assert_eq!(drawn.load(std::sync::atomic::Ordering::SeqCst), 0, "nothing drawn on the thread laying out");
    assert!(deck.stills_coming());
    assert_eq!(wanted.load(std::sync::atomic::Ordering::SeqCst), 10);
    let jobs = deck.still_jobs();
    assert_eq!(jobs.len(), 10, "one still each, thumbnails included");
    assert!(jobs.iter().all(StillJob::is_model));
    // Made on another thread, a still is taken in and its slide shown again.
    let version = deck.slide_version(3);
    let job = jobs.into_iter().find(|job| job.path() == "assets/model3.glb").unwrap();
    let bytes = model.clone();
    let made = std::thread::spawn(move || job.make(Some(Box::new(std::io::Cursor::new(bytes))))).join().unwrap();
    assert!(!made.failed());
    assert_eq!(deck.take_still(made), vec![3]);
    assert_ne!(deck.slide_version(3), version);
    let layer = deck.specs(3).iter().find(|spec| spec.model.is_some()).unwrap().clone();
    assert!(close_to(middle_of(picture_of(&layer).0), (255, 0, 0)), "{:?}", middle_of(picture_of(&layer).0));
    assert_eq!(drawn.load(std::sync::atomic::Ordering::SeqCst), 1);
}

#[test]
fn a_model_resized_shows_its_still_until_the_new_one_is_drawn() {
    // Files that can say how long each is, as a talk's on disk can.
    let files = CountingFiles { files: [("assets/box.glb".to_string(), box_model())].into(), read: Default::default() };
    let talk = "# One\n\n<Model src=\"assets/box.glb\" x=\"0\" w=\"640\" h=\"360\" />\n";
    let mut deck = Deck::open(Box::new(files), Fonts::bundled_only(), talk.to_string());
    deck.draw_models_with(std::sync::Arc::new(|_, _, width, height| {
        Ok(slides_model::Picture { width, height, bgra: [0, 0, 255, 255].repeat((width * height) as usize), drawing: Default::default(), reading_back: Default::default() })
    }));
    deck.make_stills_aside(std::sync::Arc::new(|| {}), std::sync::Arc::new(|_: &str| None));
    deck.specs(0);
    let job = deck.still_jobs().pop().unwrap();
    let made = job.make(Some(Box::new(std::io::Cursor::new(box_model()))));
    deck.take_still(made);
    assert_eq!(picture_of(&model_layer(&mut deck)).0.width(), 640);
    // Resized, the still drawn before shows, stretched, while the new one is drawn; one wanted
    // at a size passed through isn't drawn.
    for w in [700, 800] {
        let source = talk.replace("w=\"640\" h=\"360\"", &format!("w=\"{w}\" h=\"360\""));
        deck.update(source);
        assert_eq!(picture_of(&model_layer(&mut deck)).0.width(), 640, "{w}");
    }
    let jobs = deck.still_jobs();
    assert_eq!(jobs.len(), 1, "only the newest size is drawn");
    let made = jobs.into_iter().next().unwrap().make(Some(Box::new(std::io::Cursor::new(box_model()))));
    deck.take_still(made);
    assert_eq!(picture_of(&model_layer(&mut deck)).0.width(), 800);
}
