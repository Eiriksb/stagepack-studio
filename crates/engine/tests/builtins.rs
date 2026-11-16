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
        for item in &slide.layers[1].items {
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
    for item in &layer.items {
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
