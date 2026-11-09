use toolkit::column; // std has its own `column!`, so a glob import alone is ambiguous
use toolkit::prelude::*;
use std::collections::BTreeMap;

#[component]
fn Score(team: String, points: u32, colour: Color, ctx: &Ctx, children: Children) -> Element {
    column![text(format!("{team} {points} {}", ctx.click_step)).fill(colour), children.into()]
}

fn ctx() -> Ctx {
    Ctx { click_step: 2, time: 0.0, slide_width: 1920.0, slide_height: 1080.0, theme: Theme::default() }
}

fn props(pairs: &[(&str, &str)]) -> Props {
    Props(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>())
}

#[test]
fn schema_lists_only_real_arguments() {
    let names: Vec<_> = Score_component().schema.iter().map(|p| (p.name, p.type_name)).collect();
    assert_eq!(names, [("team", "String"), ("points", "u32"), ("colour", "Color")]);
}

#[test]
fn attributes_become_arguments() {
    let element = (Score_component().render)(&props(&[("team", "Oslo"), ("points", "3"), ("colour", "red")]), &ctx());
    let Element::Stack { children, .. } = element else { panic!() };
    assert_eq!(children[0], text("Oslo 3 2").fill(Color::parse("red").unwrap()));
    assert_eq!(children[1], Element::Children);
}

#[test]
fn a_missing_or_bad_number_is_zero_not_a_crash() {
    let element = (Score_component().render)(&props(&[("points", "many")]), &ctx());
    let Element::Stack { children, .. } = element else { panic!() };
    assert!(matches!(&children[0], Element::Text { text, .. } if text == " 0 2"));
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
    assert_eq!(Outlined_component().schema[0].name, "outline-width");
}

#[test]
fn colours_parse() {
    assert_eq!(Color::parse("red"), Some(Color { r: 255, g: 0, b: 0, a: 255 }));
    assert_eq!(Color::parse("#ff0000"), Color::parse("red"));
    assert_eq!(Color::parse("#0f0"), Some(Color { r: 0, g: 255, b: 0, a: 255 }));
    assert_eq!(Color::parse("nonsense"), None);
}
