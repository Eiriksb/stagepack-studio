//! Canvases, where each thing sits where it is placed, and paths drawn as vectors.

use slides_engine::*;
use slides_format::parse_talk;
use toolkit::Registry;

struct NoFiles;

impl TalkFiles for NoFiles {
    fn read(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
}

mod probe {
    use toolkit::prelude::*;

    #[component]
    pub fn Board() -> Element {
        canvas(800.0, 400.0, vec![at(100.0, 200.0, shape(ShapeKind::Rect, 40.0, 20.0).fill(Color::parse("red").unwrap()))])
    }

    #[component]
    pub fn Centred() -> Element {
        canvas(800.0, 400.0, vec![at(100.0, 0.0, text("Hi").size(40.0)).centered_in(400.0)])
    }

    #[component]
    pub fn Pair() -> Element {
        canvas(800.0, 400.0, vec![at(0.0, 0.0, text("one")), at(0.0, 100.0, text("two"))])
    }

    #[component]
    pub fn Absurd() -> Element {
        canvas(1e9, 1e9, vec![at(1e30, -1e30, text("far")), at(0.0, 0.0, text("near")).centered_in(1e9)])
    }

    #[component]
    pub fn Solid(data: String) -> Element {
        path(data, 10.0, 10.0)
    }

    #[component]
    pub fn Outlined(data: String) -> Element {
        path(data, 10.0, 10.0).stroke(Color::parse("blue").unwrap(), 3.0)
    }

    #[component]
    pub fn Framed() -> Element {
        match shape(ShapeKind::Rect, 100.0, 100.0) {
            Element::Shape { kind, width, height, .. } => Element::Shape {
                kind,
                width,
                height,
                fill: None,
                outline: None,
                outline_width: 0.0,
                radius: 0.0,
                padding: 0.0,
                child: Some(Box::new(path("M0 0 L10 0 L10 10 Z", 10.0, 10.0))),
            },
            other => other,
        }
    }

    #[component]
    pub fn Both(data: String) -> Element {
        path(data, 10.0, 10.0).fill(Color::parse("red").unwrap()).stroke(Color::parse("blue").unwrap(), 3.0)
    }
}

fn registry() -> Registry {
    let mut registry = builtin_registry();
    registry.add(probe::Board_component());
    registry.add(probe::Centred_component());
    registry.add(probe::Pair_component());
    registry.add(probe::Absurd_component());
    registry.add(probe::Solid_component());
    registry.add(probe::Outlined_component());
    registry.add(probe::Both_component());
    registry.add(probe::Framed_component());
    registry
}

fn lay_out(source: &str) -> LaidOutSlide {
    let mut fonts = Fonts::bundled_only();
    let talk = parse_talk(source);
    let registry = registry();
    layout_slide(&talk, 0, &mut LayoutEnv { fonts: &mut fonts, registry: &registry, files: &NoFiles, pictures: &mut PictureCache::default(), components: &mut TalkComponents::none(), moment: Moment::START, asks: ComponentAsks::default() })
}

fn bounds(path: &tiny_skia::Path) -> (f32, f32, f32, f32) {
    let b = path.bounds();
    (b.left(), b.top(), b.right(), b.bottom())
}

fn has_picture(layer: &LayerSpec) -> bool {
    layer.items.iter().any(|item| matches!(item, DisplayItem::Picture { .. }))
}

fn fills(layer: &LayerSpec) -> Vec<&tiny_skia::Path> {
    layer.items.iter().filter_map(|item| if let DisplayItem::Fill { path, .. } = item { Some(path) } else { None }).collect()
}

fn strokes(layer: &LayerSpec) -> Vec<(f32, &tiny_skia::Path)> {
    layer.items.iter().filter_map(|item| if let DisplayItem::Stroke { path, width, .. } = item { Some((*width, path)) } else { None }).collect()
}

fn steps_of(layer: &LayerSpec) -> Vec<PathStep> {
    match &layer.native[0].content {
        NativeContent::Path { steps, .. } => steps.clone(),
        _ => panic!("the layer's first element is not a path"),
    }
}

#[test]
fn things_on_a_canvas_sit_where_they_are_placed() {
    let slide = lay_out("<Board x=\"0\" y=\"0\" w=\"800\" h=\"400\" />\n");
    let layer = slide.layers.last().unwrap();
    assert_eq!(bounds(fills(layer)[0]), (100.0, 200.0, 140.0, 220.0));
}

#[test]
fn text_centred_in_a_width_sits_in_the_middle_of_it() {
    let slide = lay_out("<Centred x=\"0\" y=\"0\" w=\"800\" h=\"400\" />\n");
    let layer = slide.layers.last().unwrap();
    let (text, x) = layer.items.iter().find_map(|item| if let DisplayItem::Text { text, x, .. } = item { Some((text, *x)) } else { None }).unwrap();
    let glyphs: Vec<(f32, f32)> = text.buffer.layout_runs().flat_map(|run| run.glyphs.iter().map(|g| (g.x, g.x + g.w)).collect::<Vec<_>>()).collect();
    let left = glyphs.iter().map(|g| g.0).fold(f32::MAX, f32::min);
    let right = glyphs.iter().map(|g| g.1).fold(f32::MIN, f32::max);
    let centre = x + (left + right) / 2.0;
    assert!((centre - 300.0).abs() < 2.0, "centre was {centre}");
}

#[test]
fn a_canvas_holds_its_size_in_the_flow() {
    let slide = lay_out("<Board />\n\nAfter\n");
    let layer = slide.layers.last().unwrap();
    // The red box sits 200 down the canvas, and the text is the block after it.
    let canvas_top = fills(layer)[0].bounds().top() - 200.0;
    let text_top = layer.items.iter().find_map(|item| if let DisplayItem::Text { y, .. } = item { Some(*y) } else { None }).unwrap();
    assert!((text_top - canvas_top - 424.0).abs() < 0.01, "{canvas_top} {text_top}");
}

#[test]
fn each_thing_on_a_canvas_exports_on_its_own() {
    let slide = lay_out("<Pair x=\"0\" y=\"0\" w=\"800\" h=\"400\" />\n");
    let native = &slide.layers.last().unwrap().native;
    assert_eq!(native.len(), 2);
    assert!(native.iter().all(|element| matches!(element.content, NativeContent::Text { .. })));
}

#[test]
fn a_path_is_drawn_as_a_vector() {
    let slide = lay_out("<Solid data=\"M0 0 L10 0 L10 10 Z\" />\n");
    let layer = slide.layers.last().unwrap();
    assert_eq!(fills(layer).len(), 1);
    assert!(!has_picture(layer));
}

#[test]
fn a_path_with_a_stroke_and_no_fill_has_only_a_stroke() {
    let slide = lay_out("<Outlined data=\"M0 0 L10 0 L10 10 Z\" />\n");
    let layer = slide.layers.last().unwrap();
    assert!(fills(layer).is_empty());
    assert_eq!(strokes(layer)[0].0, 3.0);

    let slide = lay_out("<Both data=\"M0 0 L10 0 L10 10 Z\" />\n");
    let layer = slide.layers.last().unwrap();
    assert_eq!((fills(layer).len(), strokes(layer).len()), (1, 1));
}

#[test]
fn a_path_exports_its_steps() {
    let slide = lay_out("<Solid data=\"M0 0 L10 0 L10 10 Z\" x=\"0\" y=\"0\" w=\"10\" h=\"10\" />\n");
    let layer = slide.layers.last().unwrap();
    assert_eq!(steps_of(layer), [PathStep::Move(0.0, 0.0), PathStep::Line(10.0, 0.0), PathStep::Line(10.0, 10.0), PathStep::Close]);
    let NativeContent::Path { fill, stroke, stroke_width, .. } = &layer.native[0].content else { panic!() };
    assert!(fill.is_some() && stroke.is_none());
    assert_eq!(*stroke_width, 0.0);
    assert_eq!(layer.native[0].rect, Rect { x: 0.0, y: 0.0, w: 10.0, h: 10.0 });
}

#[test]
fn a_curve_in_a_path_exports_as_a_curve() {
    let slide = lay_out("<Solid data=\"M0 5 A5 5 0 0 1 10 5\" />\n");
    assert!(steps_of(slide.layers.last().unwrap()).iter().any(|step| matches!(step, PathStep::Cubic(..))));
}

#[test]
fn a_path_that_cannot_be_read_is_a_placeholder_with_a_notice() {
    for data in ["not a path", "M0 0 L", "M0 0 L1e300 1e300", ""] {
        let slide = lay_out(&format!("<Solid data=\"{data}\" />\n"));
        let layer = slide.layers.last().unwrap();
        assert!(fills(layer).is_empty() && !has_picture(layer), "{data}");
        assert!(slide.notices.iter().any(|notice| notice.0.contains("path")), "{data}");
    }
}

#[test]
fn an_absurd_canvas_is_held_to_the_limits() {
    let slide = lay_out("<Absurd />\n");
    let layer = slide.layers.last().unwrap();
    assert!(layer.rect.w <= 3840.0 && layer.rect.h <= 3840.0, "{:?}", layer.rect);
}

#[test]
fn a_path_inside_a_shape_is_kept_in_the_export() {
    let slide = lay_out("<Framed x=\"0\" y=\"0\" w=\"100\" h=\"100\" />\n");
    let native = &slide.layers.last().unwrap().native;
    assert!(native.iter().any(|e| matches!(e.content, NativeContent::Shape { .. })));
    assert!(native.iter().any(|e| matches!(e.content, NativeContent::Path { .. })));
}

#[test]
fn a_mark_is_taken_toward_black_or_white_only_as_far_as_it_must_be() {
    use toolkit::Color;
    let (white, black, mid) = (Color::rgb(255, 255, 255), Color::rgb(0, 0, 0), Color::rgb(0x77, 0x77, 0x77));
    // A light grey on white, a dark grey on black, a blue on white, and a grey on a mid grey.
    let cases = [
        (Color::rgb(0xd4, 0xd4, 0xd8), white),
        (Color::rgb(0x27, 0x27, 0x2a), black),
        (Color::rgb(0x93, 0xc5, 0xfd), white),
        (Color::rgb(0x80, 0x80, 0x80), mid),
    ];
    for (mark, background) in cases {
        let shown = standing_out(mark, background);
        let ratio = contrast(shown, background);
        assert!(ratio >= MARKS_STAND_OUT, "{mark:?} on {background:?}: {shown:?} is {ratio:.2}");
        assert!(ratio < MARKS_STAND_OUT + 0.1, "{mark:?} on {background:?}: {shown:?} is {ratio:.2}, further than it had to go");
        assert_eq!(shown.a, mark.a);
    }
    // Toward black on white: darker than it was in every channel, and not black.
    let shown = standing_out(cases[0].0, white);
    assert!(shown.r < 0xd4 && shown.g < 0xd4 && shown.b < 0xd8 && shown.r > 0x0a, "{shown:?}");
    // Toward white on black.
    let shown = standing_out(cases[1].0, black);
    assert!(shown.r > 0x27 && shown.r < 0xfa, "{shown:?}");
    // One that stands out already is as it was.
    for (mark, background) in [(Color::rgb(0x25, 0x63, 0xeb), white), (Color::rgb(0xfa, 0xcc, 0x15), black)] {
        assert_eq!(standing_out(mark, background), mark);
    }
}
