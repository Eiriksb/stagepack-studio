use toolkit::prelude::*;
use std::collections::BTreeMap;

#[derive(Prop, Debug, PartialEq)]
enum Kind {
    Bar,
    LineChart,
}

#[component]
fn Score(team: String, points: u32, ready: bool, colour: Color, note: Option<String>, kind: Kind, logo: PictureFile, children: Children, ctx: &Ctx) -> Element {
    let summary = format!("{team} {points} {ready} {} {kind:?} {} {}", note.unwrap_or_default(), logo.0, ctx.click_step());
    col![text(summary).fill(colour), children.into()]
}

fn data(click_step: u32) -> CtxData {
    CtxData { click_step, time: 0.5, slide_width: 1920.0, slide_height: 1080.0, width: 800.0, height: 0.0, theme: Theme::default() }
}

fn no_assets(_: &str) -> Option<Vec<u8>> {
    None
}

fn ctx_at(click_step: u32) -> Ctx<'static> {
    Ctx::new(data(click_step), &no_assets)
}

fn ctx() -> Ctx<'static> {
    ctx_at(2)
}

fn props(pairs: &[(&str, &str)]) -> Props {
    Props(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>())
}

#[test]
fn the_schema_says_what_each_argument_is() {
    let schema = (Score_component().schema)();
    assert_eq!(schema.name, "Score");
    let fields: Vec<_> = schema.props.iter().map(|p| (p.name.as_str(), p.kind.clone(), p.optional)).collect();
    let pictures = ["png", "jpg", "jpeg", "svg", "webp"].map(String::from).to_vec();
    assert_eq!(
        fields,
        [
            ("team", PropKind::Text, false),
            ("points", PropKind::Whole, false),
            ("ready", PropKind::Flag, false),
            ("colour", PropKind::Color, false),
            ("note", PropKind::Text, true),
            ("kind", PropKind::Choice(vec!["bar".into(), "line-chart".into()]), false),
            ("logo", PropKind::File(pictures), false),
        ]
    );
    assert!(schema.children);
}

#[test]
fn the_other_kinds_of_argument() {
    assert_eq!(i32::kind(), PropKind::Whole);
    assert_eq!(f32::kind(), PropKind::Number);
    assert_eq!(DataFile::kind(), PropKind::File(vec!["csv".into()]));
    assert_eq!(i32::from_attr(Some("-4")), -4);
    assert_eq!(i32::from_attr(Some("many")), 0);
    assert_eq!(DataFile::from_attr(Some("assets/a.csv")), DataFile("assets/a.csv".into()));
    const { assert!(<Option<Kind>>::OPTIONAL && !Kind::OPTIONAL) };
}

#[test]
fn a_choice_reads_kebab_case_and_falls_back_to_the_first() {
    assert_eq!(Kind::from_attr(Some("line-chart")), Kind::LineChart);
    assert_eq!(Kind::from_attr(Some("bar")), Kind::Bar);
    assert_eq!(Kind::from_attr(Some("LineChart")), Kind::Bar);
    assert_eq!(Kind::from_attr(Some("nonsense")), Kind::Bar);
    assert_eq!(Kind::from_attr(None), Kind::Bar);
}

#[test]
fn attributes_become_arguments() {
    let attrs = [("team", "Oslo"), ("points", "3"), ("ready", "true"), ("colour", "red"), ("note", "away"), ("kind", "line-chart"), ("logo", "assets/a.png")];
    let element = (Score_component().render)(&props(&attrs), &ctx());
    let Element::Stack { children, .. } = element else { panic!() };
    assert_eq!(children[0], text("Oslo 3 true away LineChart assets/a.png 2").fill(Color::parse("red").unwrap()));
    assert_eq!(children[1], Element::Children);
}

#[test]
fn registry_finds_components_by_tag_name() {
    let mut registry = Registry::default();
    registry.add(Score_component());
    assert!(registry.get("Score").is_some());
    assert!(registry.get("Nope").is_none());
}

#[component]
fn Outlined(outline_width: u32) -> Element {
    text(outline_width.to_string())
}

#[test]
fn underscored_arguments_read_hyphenated_attributes() {
    assert_eq!((Outlined_component().render)(&props(&[("outline-width", "4")]), &ctx()), text("4"));
    assert_eq!((Outlined_component().schema)().props[0].name, "outline-width");
}

#[test]
fn a_missing_or_bad_number_is_zero_not_a_crash() {
    assert_eq!((Outlined_component().render)(&props(&[("outline-width", "many")]), &ctx()), text("0"));
    assert_eq!((Outlined_component().render)(&props(&[]), &ctx()), text("0"));
}

#[test]
fn a_component_with_no_children_argument_says_so() {
    assert!(!(Outlined_component().schema)().children);
}

#[component]
fn Measured(ctx: &Ctx<'_>) -> Element {
    text(ctx.width().to_string())
}

#[test]
fn a_ctx_can_be_written_with_its_lifetime() {
    assert_eq!((Measured_component().render)(&props(&[]), &ctx()), text("800"));
    assert!((Measured_component().schema)().props.is_empty());
}

#[test]
fn a_ctx_gives_back_what_it_was_made_with() {
    let ctx = ctx();
    assert_eq!((ctx.slide_width(), ctx.slide_height(), ctx.width(), ctx.height()), (1920.0, 1080.0, 800.0, 0.0));
    assert_eq!(*ctx.theme(), Theme::default());
}

#[test]
fn a_component_that_asks_for_nothing_varies_by_nothing() {
    let output = ctx().finish(Element::Empty);
    assert_eq!(output, RenderOutput { element: Element::Empty, steps: 0, stepped: false, animated: false });
}

#[test]
fn asking_for_steps_is_recorded() {
    let ctx = ctx();
    ctx.steps(3);
    ctx.steps(2);
    let output = ctx.finish(Element::Empty);
    assert_eq!((output.steps, output.stepped, output.animated), (3, true, false));
}

#[test]
fn steps_holds_the_step_to_the_count() {
    assert_eq!(ctx_at(9).steps(3), 3);
    assert_eq!(ctx_at(1).steps(3), 1);
}

#[test]
fn reading_the_click_step_marks_it_stepped_and_adds_no_steps() {
    let ctx = ctx();
    assert_eq!(ctx.click_step(), 2);
    let output = ctx.finish(Element::Empty);
    assert_eq!((output.steps, output.stepped, output.animated), (0, true, false));
}

#[test]
fn asking_for_the_time_marks_it_animated() {
    let ctx = ctx();
    assert_eq!(ctx.time(), 0.5);
    let output = ctx.finish(Element::Empty);
    assert_eq!((output.steps, output.stepped, output.animated), (0, false, true));
}

#[test]
fn an_asset_comes_from_what_the_app_gave() {
    let assets = |path: &str| match path {
        "assets/a.csv" => Some(b"1,2".to_vec()),
        "assets/a.png" => Some(vec![0xff, 0xfe]),
        _ => None,
    };
    let ctx = Ctx::new(data(0), &assets);
    assert_eq!(ctx.asset("assets/a.csv"), Some(b"1,2".to_vec()));
    assert_eq!(ctx.asset_text("assets/a.csv"), Some("1,2".to_string()));
    assert_eq!(ctx.asset("assets/none.csv"), None);
    assert_eq!(ctx.asset_text("assets/none.csv"), None);
    assert_eq!(ctx.asset_text("assets/a.png"), None, "these bytes aren't text");
}

#[test]
fn an_asset_path_is_held_to_the_assets_folder() {
    for path in ["assets/a.csv", "assets/x/a.png"] {
        assert!(asset_path_allowed(path), "{path}");
    }
    for path in ["slides.md", "assets/../slides.md", "assets//a", "assets/./a", "/assets/a", "assets\\a", "build/a.wasm", ""] {
        assert!(!asset_path_allowed(path), "{path}");
    }
}

#[test]
fn something_placed_has_its_own_width_until_it_is_given_one() {
    assert_eq!(at(1.0, 2.0, text("a")), Placed { x: 1.0, y: 2.0, width: 0.0, align: TextAlign::Left, element: text("a") });
    let centered = at(1.0, 2.0, text("a")).centered_in(40.0);
    assert_eq!((centered.width, centered.align), (40.0, TextAlign::Center));
    let right = at(1.0, 2.0, text("a")).right_aligned_in(40.0);
    assert_eq!((right.width, right.align), (40.0, TextAlign::Right));
}

#[test]
fn a_canvas_and_a_problem_are_made_by_their_helpers() {
    assert_eq!(canvas(10.0, 20.0, vec![at(0.0, 0.0, Element::Empty)]), Element::Canvas { width: 10.0, height: 20.0, items: vec![at(0.0, 0.0, Element::Empty)] });
    assert_eq!(problem("No data"), Element::Problem { message: "No data".into() });
}

#[test]
fn only_a_path_takes_a_stroke() {
    let red = Color::parse("red").unwrap();
    let stroked = path("M0 0", 5.0, 6.0).stroke(red, 2.0);
    assert_eq!(stroked, Element::Path { data: "M0 0".into(), width: 5.0, height: 6.0, fill: None, stroke: Some(red), stroke_width: 2.0 });
    assert_eq!(text("a").stroke(red, 2.0), text("a"));
}

#[test]
fn colours_parse() {
    assert_eq!(Color::parse("red"), Some(Color { r: 255, g: 0, b: 0, a: 255 }));
    assert_eq!(Color::parse("#ff0000"), Color::parse("red"));
    assert_eq!(Color::parse("#0f0"), Some(Color { r: 0, g: 255, b: 0, a: 255 }));
    assert_eq!(Color::parse("nonsense"), None);
}
