//! The ready-made chart: bar, line and pie, from numbers in the tag or a CSV file.

mod common;

use std::collections::{BTreeMap, HashMap};

use slides_engine::chart_parts::{axis_for, parse_csv, parse_inline, plot_area, series_color, ChartData};
use slides_engine::*;
use slides_format::parse_talk;
use toolkit::{Color, Ctx, CtxData, Element, Placed, Props, ShapeKind, Theme};

struct MapFiles(HashMap<String, Vec<u8>>);

impl TalkFiles for MapFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.0.get(path).cloned()
    }
}

fn files(list: &[(&str, &str)]) -> MapFiles {
    MapFiles(list.iter().map(|(path, text)| (path.to_string(), text.as_bytes().to_vec())).collect())
}

const SALES: &str = "Month,Sales,Costs\nJan,10,4\nFeb,24,9\nMar,17,12\n";

/// What the chart draws for `attrs` in a box `width` by `height`, at `step`, and the clicks it asked for.
fn chart_at(attrs: &[(&str, &str)], width: f32, height: f32, step: u32, assets: &[(&str, &str)]) -> (Element, u32) {
    let registry = builtin_registry();
    let chart = registry.get("Chart").expect("Chart is a built-in");
    let props = Props(attrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>());
    let assets: HashMap<String, Vec<u8>> = assets.iter().map(|(p, t)| (p.to_string(), t.as_bytes().to_vec())).collect();
    let read = |path: &str| assets.get(path).cloned();
    let data = CtxData::new(step, 0.0, 1920.0, 1080.0, width, height, Theme::default());
    let ctx = Ctx::new(data, &read);
    let element = (chart.render)(&props, &ctx);
    let output = ctx.finish(element);
    (output.element, output.steps)
}

fn chart(attrs: &[(&str, &str)]) -> Element {
    chart_at(attrs, 1200.0, 600.0, 0, &[]).0
}

fn items(element: &Element) -> &[Placed] {
    match element {
        Element::Canvas { items, .. } => items,
        other => panic!("not a canvas: {other:?}"),
    }
}

/// `(x, y, width, height)` of each shape of `kind` on the chart.
fn shapes(element: &Element, wanted: ShapeKind) -> Vec<(f32, f32, f32, f32)> {
    items(element)
        .iter()
        .filter_map(|placed| match &placed.element {
            Element::Shape { kind, width, height, .. } if *kind == wanted => Some((placed.x, placed.y, *width, *height)),
            _ => None,
        })
        .collect()
}

fn fill_of(placed: &Placed) -> Option<Color> {
    match &placed.element {
        Element::Shape { fill, .. } | Element::Path { fill, .. } => *fill,
        _ => None,
    }
}

/// `(data, fill, stroke, stroke width)` of each path on the chart.
fn paths(element: &Element) -> Vec<(String, Option<Color>, Option<Color>, f32)> {
    items(element)
        .iter()
        .filter_map(|placed| match &placed.element {
            Element::Path { data, fill, stroke, stroke_width, .. } => Some((data.clone(), *fill, *stroke, *stroke_width)),
            _ => None,
        })
        .collect()
}

fn texts(element: &Element) -> Vec<String> {
    items(element)
        .iter()
        .filter_map(|placed| match &placed.element {
            Element::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn is_problem(element: &Element) -> bool {
    matches!(element, Element::Problem { .. })
}

fn message(element: &Element) -> String {
    match element {
        Element::Problem { message } => message.clone(),
        other => panic!("not a problem: {other:?}"),
    }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

fn deck(source: &str, assets: &[(&str, &str)]) -> Deck {
    Deck::open(Box::new(files(assets)), Fonts::bundled_only(), source.to_string())
}

/// Slide 0 of `source`, laid out at click `step`.
fn lay_out_at(source: &str, step: u32) -> LaidOutSlide {
    let mut fonts = Fonts::bundled_only();
    let talk = parse_talk(source);
    let registry = builtin_registry();
    let mut env = LayoutEnv {
        fonts: &mut fonts,
        registry: &registry,
        files: &files(&[]),
        pictures: &mut PictureCache::default(),
        components: &mut TalkComponents::none(),
        moment: Moment::at(step, 0.0),
        asks: ComponentAsks::default(),
    };
    layout_slide(&talk, 0, &mut env)
}

fn native_rects(specs: &[LayerSpec]) -> usize {
    specs
        .iter()
        .flat_map(|spec| &spec.native)
        .filter(|element| matches!(element.content, NativeContent::Shape { kind: ShapeKind::Rect, .. }))
        .count()
}

#[test]
fn ticks_are_round_numbers_and_include_zero() {
    let axis = axis_for(10.0, 24.0);
    assert_eq!((axis.low, axis.high, axis.step), (0.0, 25.0, 5.0));
    assert_eq!(axis.labels(), ["0", "5", "10", "15", "20", "25"]);
    let axis = axis_for(-5.0, 31.0);
    assert_eq!((axis.low, axis.high, axis.step), (-10.0, 40.0, 10.0));
    assert_eq!(axis.labels(), ["-10", "0", "10", "20", "30", "40"]);
    let axis = axis_for(0.001, 0.004);
    assert_eq!(axis.labels(), ["0", "0.001", "0.002", "0.003", "0.004"]);
    let axis = axis_for(0.0, 1e12);
    assert_eq!(axis.labels(), ["0", "200000000000", "400000000000", "600000000000", "800000000000", "1000000000000"]);
    let axis = axis_for(0.0, 12.0);
    assert_eq!(axis.labels(), ["0", "2", "4", "6", "8", "10", "12"]);
    let axis = axis_for(0.0, 14.0);
    assert_eq!(axis.labels(), ["0", "2.5", "5", "7.5", "10", "12.5", "15"]);
}

#[test]
fn all_zero_gives_an_axis_to_one() {
    let axis = axis_for(0.0, 0.0);
    assert_eq!((axis.low, axis.high), (0.0, 1.0));
    assert!(axis.ticks().contains(&0.0));
}

#[test]
fn axes_of_the_extremes_are_finite_and_hold_zero() {
    for (low, high) in [(-1e15, 1e15), (1e-300, 1e14), (0.0, 5e-324), (-3.0, -1.0), (f64::NAN, f64::INFINITY)] {
        let axis = axis_for(low, high);
        assert!(axis.low.is_finite() && axis.high.is_finite() && axis.high > axis.low, "{low} {high}: {axis:?}");
        assert!(axis.low <= low.min(0.0) || low.is_nan());
        assert!(axis.ticks().len() <= 7);
        assert!(axis.labels().iter().any(|label| label == "0"), "{:?}", axis.labels());
        assert!(axis.labels().iter().all(|label| !label.contains('e')), "{:?}", axis.labels());
    }
}

#[test]
fn labels_and_values_are_read() {
    let data = parse_inline("Q1, Q2", "10, 24").unwrap();
    assert_eq!(data, ChartData { labels: vec!["Q1".into(), "Q2".into()], series: vec![(String::new(), vec![10.0, 24.0])] });
}

#[test]
fn a_mismatch_says_what_it_found() {
    assert_eq!(parse_inline("A, B, C", "1, 2"), Err("The chart has 3 labels and 2 values".to_string()));
}

#[test]
fn a_value_that_is_not_a_number_says_so() {
    assert_eq!(parse_inline("A, B", "1, x"), Err("\"x\" isn't a number".to_string()));
}

#[test]
fn values_that_are_not_finite_are_refused() {
    for bad in ["NaN", "inf", "-infinity"] {
        assert_eq!(parse_inline("A", bad), Err(format!("\"{bad}\" isn't a number")));
        assert_eq!(parse_csv(&format!("L,S\nA,{bad}\n")), Err(format!("\"{bad}\" isn't a number")));
    }
}

#[test]
fn values_too_large_to_draw_are_refused() {
    assert!(parse_inline("A", "1e300").unwrap_err().contains("too large"));
    assert!(parse_inline("A", "-1e300").unwrap_err().contains("too large"));
}

#[test]
fn csv_gives_series_by_column() {
    let data = parse_csv("Month,Sales,Costs\nJan,10,4\nFeb,24,9\n").unwrap();
    assert_eq!(data.labels, vec!["Jan", "Feb"]);
    assert_eq!(data.series, vec![("Sales".to_string(), vec![10.0, 24.0]), ("Costs".to_string(), vec![4.0, 9.0])]);
}

#[test]
fn csv_handles_quotes() {
    let data = parse_csv("Name,Value\n\"A, B\",1\n\"Say \"\"hi\"\"\",2\r\n").unwrap();
    assert_eq!(data.labels, vec!["A, B", "Say \"hi\""]);
    assert_eq!(data.series[0].1, vec![1.0, 2.0]);
}

#[test]
fn a_short_csv_row_says_which() {
    assert_eq!(parse_csv("L,A,B\nx,1,2\ny,3\n"), Err("Row 3 of the data has 1 value, and the header names 2 series".to_string()));
}

#[test]
fn an_empty_chart_says_so() {
    assert_eq!(parse_inline("", ""), Err("The chart has no data".to_string()));
    assert_eq!(parse_csv(""), Err("The chart has no data".to_string()));
    assert_eq!(parse_csv("Month,Sales\n"), Err("The chart has no data".to_string()));
    assert_eq!(message(&chart(&[("kind", "bar")])), "The chart has no data");
}

#[test]
fn the_limits_are_held() {
    let labels = vec!["a"; 1001].join(",");
    let values = vec!["1"; 1001].join(",");
    assert!(parse_inline(&labels, &values).is_err());
    let rows: String = (0..1001).map(|i| format!("{i},1\n")).collect();
    assert!(parse_csv(&format!("L,S\n{rows}")).is_err());
    let header = (0..17).map(|i| format!("S{i}")).collect::<Vec<_>>().join(",");
    let row = vec!["1"; 17].join(",");
    assert!(parse_csv(&format!("L,{header}\nx,{row}\n")).is_err());
    let header = (0..16).map(|i| format!("S{i}")).collect::<Vec<_>>().join(",");
    let row = vec!["1"; 16].join(",");
    assert!(parse_csv(&format!("L,{header}\nx,{row}\n")).is_ok());
    let rows: String = (0..1000).map(|i| format!("{i},1\n")).collect();
    assert!(parse_csv(&format!("L,S\n{rows}")).is_ok());
}

#[test]
fn colours_start_at_the_accent_and_turn() {
    let theme = Theme::default();
    assert_eq!(series_color(&theme, 0), theme.accent);
    assert_ne!(series_color(&theme, 1), theme.accent);
    assert_ne!(series_color(&theme, 1), series_color(&theme, 2));
    let red = Theme { accent: Color::rgb(255, 0, 0), ..Theme::default() };
    // 47° from red, at the same saturation and lightness.
    let turned = series_color(&red, 1);
    assert_eq!((turned.r, turned.b), (255, 0));
    assert!((198..=202).contains(&turned.g), "{turned:?}");
}

#[test]
fn bars_are_as_tall_as_their_values() {
    let element = chart(&[("kind", "bar"), ("labels", "A, B, C"), ("values", "10, 24, 17")]);
    let (_, top, _, height) = plot_area(1200.0, 600.0, false, false);
    let bars = shapes(&element, ShapeKind::Rect);
    assert_eq!(bars.len(), 3);
    for ((_, y, _, h), share) in bars.iter().zip([0.4, 0.96, 0.68]) {
        assert!(close(*h, height * share), "{h} is not {share} of {height}");
        assert!(close(y + h, top + height), "the bar stands on the axis");
    }
}

#[test]
fn bars_leave_room_for_the_title() {
    let element = chart(&[("kind", "bar"), ("labels", "A"), ("values", "10"), ("title", "Sales")]);
    let (_, top, _, _) = plot_area(1200.0, 600.0, true, false);
    assert!(top >= 64.0);
    assert!(texts(&element).contains(&"Sales".to_string()));
    let bar = shapes(&element, ShapeKind::Rect)[0];
    assert!(bar.1 >= top);
}

#[test]
fn a_negative_value_goes_below_the_axis() {
    let element = chart(&[("kind", "bar"), ("labels", "A, B"), ("values", "10, -5")]);
    let bars = shapes(&element, ShapeKind::Rect);
    let (up, down) = (bars[0], bars[1]);
    assert!(close(up.1 + up.3, down.1), "both meet at zero");
    // The bar below is half the one above, and both are measured from 0.
    assert!(close(down.3 * 2.0, up.3));
    assert!(texts(&element).iter().any(|label| label.starts_with('-')), "{:?}", texts(&element));
}

#[test]
fn a_line_chart_draws_a_stroked_path_and_a_point_each() {
    let element = chart(&[("kind", "line"), ("labels", "A, B, C"), ("values", "10, 24, 17")]);
    let accent = Theme::default().accent;
    let lines: Vec<_> = paths(&element).into_iter().filter(|(_, _, stroke, _)| *stroke == Some(accent)).collect();
    assert_eq!(lines.len(), 1);
    let (data, fill, _, width) = &lines[0];
    assert_eq!(*fill, None);
    assert_eq!(*width, 6.0);
    assert_eq!(data.matches('L').count(), 2, "{data}");
    let points = shapes(&element, ShapeKind::Ellipse);
    assert_eq!(points.len(), 3);
    assert!(points.iter().all(|(_, _, w, h)| *w == 16.0 && *h == 16.0));
}

#[test]
fn a_pie_chart_draws_a_slice_each() {
    let element = chart(&[("kind", "pie"), ("labels", "A, B, C"), ("values", "1, 2, 1")]);
    let theme = Theme::default();
    let slices: Vec<_> = paths(&element).into_iter().filter(|(data, ..)| data.contains('A')).collect();
    assert_eq!(slices.len(), 3);
    for (index, (_, fill, ..)) in slices.iter().enumerate() {
        assert_eq!(*fill, Some(series_color(&theme, index)));
    }
    let labels = texts(&element);
    assert!(["A", "B", "C"].iter().all(|label| labels.iter().any(|text| text.contains(label))), "{labels:?}");
}

#[test]
fn one_value_is_a_whole_circle() {
    let element = chart(&[("kind", "pie"), ("labels", "All"), ("values", "5")]);
    let circles = shapes(&element, ShapeKind::Ellipse);
    assert_eq!(circles.len(), 1);
    assert!(circles[0].2 == circles[0].3 && circles[0].2 > 100.0);
    assert!(paths(&element).iter().all(|(data, ..)| !data.contains('A')));
}

#[test]
fn a_pie_cannot_show_a_negative_value() {
    assert!(is_problem(&chart(&[("kind", "pie"), ("labels", "A, B"), ("values", "3, -1")])));
    assert!(is_problem(&chart(&[("kind", "pie"), ("labels", "A, B"), ("values", "0, 0")])));
}

#[test]
fn a_bad_list_is_a_labelled_box_on_the_slide() {
    let mut d = deck("<Chart kind=\"bar\" labels=\"A, B\" values=\"1\" />\n", &[]);
    assert!(d.notices(0).iter().any(|notice| notice.contains("The chart has 2 labels and 1 value")), "{:?}", d.notices(0));
}

#[test]
fn a_csv_file_is_read_from_assets() {
    let mut d = deck("<Chart kind=\"bar\" data=\"assets/sales.csv\" />\n", &[("assets/sales.csv", SALES)]);
    assert!(d.notices(0).is_empty(), "{:?}", d.notices(0));
    assert_eq!(native_rects(d.specs(0)), 6);
}

#[test]
fn a_csv_outside_assets_is_refused() {
    let element = chart_at(&[("kind", "bar"), ("data", "../sales.csv")], 1200.0, 600.0, 0, &[("../sales.csv", SALES)]).0;
    assert!(message(&element).contains("assets"), "{element:?}");
}

#[test]
fn a_missing_csv_shows_a_labelled_box() {
    let mut d = deck("<Chart kind=\"bar\" data=\"assets/x.csv\" />\n", &[]);
    assert!(d.notices(0).iter().any(|notice| notice.contains("Can't read assets/x.csv")), "{:?}", d.notices(0));
}

#[test]
fn a_bad_csv_shows_a_labelled_box() {
    let mut d = deck("<Chart kind=\"line\" data=\"assets/bad.csv\" />\n", &[("assets/bad.csv", "L,S\nA,lots\n")]);
    assert!(d.notices(0).iter().any(|notice| notice.contains("\"lots\" isn't a number")), "{:?}", d.notices(0));
}

const BUILDING: &str = "<Chart kind=\"bar\" labels=\"A, B, C\" values=\"10, 24, 17\" build=\"true\" />\n";

#[test]
fn building_adds_a_click_for_each_bar() {
    assert_eq!(deck(BUILDING, &[]).timeline(0).steps, 3);
    assert_eq!(native_rects(&lay_out_at(BUILDING, 0).layers), 0);
    assert_eq!(native_rects(&lay_out_at(BUILDING, 1).layers), 1);
    assert_eq!(native_rects(deck(BUILDING, &[]).specs(0)), 3);
}

#[test]
fn several_series_build_one_series_per_click() {
    let (element, steps) = chart_at(&[("kind", "bar"), ("data", "assets/s.csv"), ("build", "true")], 1200.0, 600.0, 1, &[("assets/s.csv", SALES)]);
    assert_eq!(steps, 2);
    let theme = Theme::default();
    let bars: Vec<_> = items(&element).iter().filter(|placed| matches!(placed.element, Element::Shape { kind: ShapeKind::Rect, .. })).collect();
    assert_eq!(bars.len(), 3);
    assert!(bars.iter().all(|bar| fill_of(bar) == Some(series_color(&theme, 0))));
    let (element, _) = chart_at(&[("kind", "line"), ("data", "assets/s.csv"), ("build", "true")], 1200.0, 600.0, 2, &[("assets/s.csv", SALES)]);
    assert_eq!(paths(&element).iter().filter(|(_, _, _, width)| *width == 6.0).count(), 2);
}

#[test]
fn a_building_line_and_pie_add_a_point_or_slice_per_click() {
    let attrs = |kind| [("kind", kind), ("labels", "A, B, C"), ("values", "1, 2, 3"), ("build", "true")];
    let (line, steps) = chart_at(&attrs("line"), 1200.0, 600.0, 2, &[]);
    assert_eq!(steps, 3);
    assert_eq!(shapes(&line, ShapeKind::Ellipse).len(), 2);
    let (pie, steps) = chart_at(&attrs("pie"), 1200.0, 600.0, 1, &[]);
    assert_eq!(steps, 3);
    assert_eq!(paths(&pie).iter().filter(|(data, ..)| data.contains('A')).count(), 1);
}

/// The example talk's chart, which the check by hand presses through one bar at a time.
#[test]
fn the_example_talk_builds_its_chart_one_bar_per_click() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/components");
    let source = std::fs::read_to_string(root.join("slides.md")).unwrap();
    let mut d = Deck::open(Box::new(FolderFiles::new(root)), Fonts::bundled_only(), source);
    let last = d.slide_count() - 1;
    assert_eq!(d.timeline(last).steps, 4, "a click for each of the 4 rows of assets/sales.csv");
}

#[test]
fn not_building_adds_no_clicks() {
    let mut d = deck("<Chart kind=\"bar\" labels=\"A, B, C\" values=\"10, 24, 17\" />\n", &[]);
    assert_eq!(d.timeline(0).steps, 0);
    assert_eq!(native_rects(&lay_out_at("<Chart kind=\"bar\" labels=\"A, B, C\" values=\"10, 24, 17\" />\n", 0).layers), 3);
}

#[test]
fn a_chart_exports_as_real_shapes() {
    let mut d = deck(
        "<Chart kind=\"bar\" labels=\"Q1, Q2\" values=\"10, 24\" title=\"Sales\" x=\"0\" y=\"0\" w=\"1200\" h=\"600\" />\n\n<Chart kind=\"line\" labels=\"Q1, Q2\" values=\"10, 24\" x=\"0\" y=\"600\" w=\"1200\" h=\"400\" />\n",
        &[],
    );
    let natives: Vec<&NativeElement> = d.specs(0).iter().flat_map(|spec| &spec.native).collect();
    assert_eq!(natives.iter().filter(|e| matches!(e.content, NativeContent::Shape { kind: ShapeKind::Rect, .. })).count(), 2);
    let texts: Vec<String> = natives
        .iter()
        .filter_map(|e| match &e.content {
            NativeContent::Text { paragraphs, .. } => Some(paragraphs.iter().flat_map(|p| &p.runs).map(|r| r.text.as_str()).collect()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|t| t == "Sales") && texts.iter().any(|t| t == "Q2"), "{texts:?}");
    assert!(natives.iter().any(|e| matches!(e.content, NativeContent::Path { fill: None, stroke: Some(_), stroke_width: 6.0, .. })));
}

#[test]
fn its_kind_is_a_choice_in_the_form() {
    let schema = (builtin_registry().get("Chart").unwrap().schema)();
    let kind = schema.props.iter().find(|field| field.name == "kind").unwrap();
    assert_eq!(kind.kind, toolkit::PropKind::Choice(vec!["bar".into(), "line".into(), "pie".into()]));
    let data = schema.props.iter().find(|field| field.name == "data").unwrap();
    assert!(data.optional);
}

#[test]
fn a_thousand_points_keep_their_labels_apart() {
    let labels = (0..1000).map(|i| format!("Day {i}")).collect::<Vec<_>>().join(",");
    let values = (0..1000).map(|i| ((i * 37) % 100).to_string()).collect::<Vec<_>>().join(",");
    for kind in ["bar", "line"] {
        let element = chart(&[("kind", kind), ("labels", &labels), ("values", &values)]);
        let (_, top, _, height) = plot_area(1200.0, 600.0, false, false);
        let mut below: Vec<(f32, f32)> = items(&element)
            .iter()
            .filter(|placed| matches!(placed.element, Element::Text { .. }) && placed.y > top + height)
            .map(|placed| (placed.x, placed.x + placed.width))
            .collect();
        below.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert!(below.len() > 2 && below.len() < 100, "{kind}: {} labels", below.len());
        assert!(below.windows(2).all(|pair| pair[0].1 <= pair[1].0), "{kind}: labels overlap");
        assert!(below.iter().all(|(left, right)| *left >= 0.0 && *right <= 1200.0));
    }
}

#[test]
fn any_size_gives_finite_numbers() {
    for (width, height) in [(0.0, 0.0), (f32::NAN, f32::INFINITY), (1e9, 1e9), (-5.0, 3.0), (10.0, 10.0)] {
        for kind in ["bar", "line", "pie"] {
            let element = chart_at(&[("kind", kind), ("labels", "A, B"), ("values", "1e-300, 1e14"), ("title", "T")], width, height, 0, &[]).0;
            let Element::Canvas { width: w, height: h, items } = &element else { panic!("{element:?}") };
            assert!(w.is_finite() && h.is_finite() && *w <= 3840.0 && *h <= 3840.0);
            assert!(items.iter().all(|placed| placed.x.is_finite() && placed.y.is_finite() && placed.width.is_finite()));
            for (data, ..) in paths(&element) {
                assert!(!data.contains("NaN") && !data.contains("inf"), "{data}");
            }
        }
    }
}

#[test]
fn every_chart_draws_without_a_problem() {
    let source = "<Chart kind=\"pie\" labels=\"A, B, C\" values=\"1, 2, 1e-12\" x=\"0\" y=\"0\" w=\"900\" h=\"500\" />\n\n<Chart kind=\"line\" labels=\"A, B\" values=\"-1e14, 5\" x=\"900\" y=\"0\" w=\"900\" h=\"500\" />\n";
    let mut d = deck(source, &[]);
    assert!(d.notices(0).is_empty(), "{:?}", d.notices(0));
}

/// The chart as it is on a slide, in the bundled fonts.
fn slide_png(source: &str, assets: &[(&str, &str)]) -> tiny_skia::Pixmap {
    let mut d = deck(source, assets);
    assert!(d.notices(0).is_empty(), "{:?}", d.notices(0));
    let mut target = tiny_skia::Pixmap::new(1920, 1080).unwrap();
    for layer in &d.layers(0, 1.0) {
        target.draw_pixmap(layer.x, layer.y, tiny_skia::Pixmap::as_ref(&layer.pixmap), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
    }
    target
}

const GOLDEN: &str = "# Sales and costs\n\n<Chart kind=\"bar\" data=\"assets/sales.csv\" title=\"First quarter\" />\n";

#[test]
fn a_chart_looks_the_same() {
    common::assert_golden("chart", &slide_png(GOLDEN, &[("assets/sales.csv", SALES)]));
}

/// Writes a bar, a line and a pie chart, and one of a thousand points, to `target/check/`,
/// for a person to look at.
#[test]
#[ignore = "writes pictures for a person to look at"]
fn charts_for_a_look() {
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/check");
    std::fs::create_dir_all(&out).unwrap();
    let year = "Month,North,South,West\nJan,12,8,3\nFeb,18,11,-4\nMar,9,15,6\nApr,22,13,9\nMay,27,19,12\nJun,31,24,15\n";
    let many: String = (0..1000).map(|i| format!("Day {i},{}\n", ((i as f64) / 40.0).sin() * 50.0 + 60.0)).collect();
    let many = format!("Day,Visitors\n{many}");
    let charts = [
        ("chart-bar", "<Chart kind=\"bar\" data=\"assets/year.csv\" title=\"Sales by region\" x=\"120\" y=\"120\" w=\"1680\" h=\"840\" />\n"),
        ("chart-line", "<Chart kind=\"line\" data=\"assets/year.csv\" title=\"Sales by region\" x=\"120\" y=\"120\" w=\"1680\" h=\"840\" />\n"),
        ("chart-pie", "<Chart kind=\"pie\" labels=\"Rent, Staff, Stock, Travel, Other\" values=\"30, 45, 15, 6, 4\" title=\"Where it goes\" x=\"120\" y=\"120\" w=\"1680\" h=\"840\" />\n"),
        ("chart-many", "<Chart kind=\"line\" data=\"assets/many.csv\" title=\"A thousand days\" x=\"120\" y=\"120\" w=\"1680\" h=\"840\" />\n"),
    ];
    for (name, source) in charts {
        let pixmap = slide_png(source, &[("assets/year.csv", year), ("assets/many.csv", &many)]);
        pixmap.save_png(out.join(format!("{name}.png"))).unwrap();
    }
}

#[test]
fn a_csv_over_one_mebibyte_is_refused_before_it_is_read() {
    let big = format!("L,S\n{}", "x".repeat(1024 * 1024));
    let element = chart_at(&[("kind", "bar"), ("data", "assets/big.csv")], 1200.0, 600.0, 0, &[("assets/big.csv", &big)]).0;
    assert_eq!(message(&element), "The chart's data is over 1 MiB, more than a chart can show");
}

#[test]
fn what_a_chart_quotes_is_shortened() {
    let long = "y".repeat(5000);
    let refused = parse_csv(&format!("L,S\nA,{long}\n")).unwrap_err();
    assert!(refused.chars().count() < 100, "{refused}");
    let element = chart_at(&[("kind", "bar"), ("data", &format!("assets/{long}.csv"))], 1200.0, 600.0, 0, &[]).0;
    assert!(message(&element).chars().count() < 100, "{}", message(&element));
}
