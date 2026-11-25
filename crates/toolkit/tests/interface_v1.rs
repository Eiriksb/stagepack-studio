//! Version 1 of the interface between the app and a talk's components, as the JSON that
//! crosses between them. Components already built depend on exactly this text, so what
//! these tests assert must never change.

use std::collections::BTreeMap;
use std::fmt::Debug;

use serde::Serialize;
use serde::de::DeserializeOwned;
use toolkit::prelude::*;

const STACK: &str = r#"{"Stack":{"horizontal":true,"gap":8.0,"children":[{"Text":{"text":"Hi","size":48.0,"bold":true,"color":{"r":1,"g":2,"b":3,"a":255}}},"Children","Empty"]}}"#;
const SHAPE: &str = r#"{"Shape":{"kind":"Rounded","width":10.0,"height":20.0,"fill":null,"outline":null,"outline_width":0.0,"radius":4.0,"padding":0.0,"child":null}}"#;
const PATH: &str = r#"{"Path":{"data":"M0 0","width":5.0,"height":6.0,"fill":null,"stroke":{"r":0,"g":0,"b":0,"a":255},"stroke_width":2.0}}"#;
const PICTURE: &str = r#"{"Picture":{"src":"assets/a.png","width":10.0,"height":20.0}}"#;
const ICON: &str = r#"{"Icon":{"name":"star","size":64.0,"color":{"r":1,"g":2,"b":3,"a":255}}}"#;
const CANVAS: &str = r#"{"Canvas":{"width":100.0,"height":50.0,"items":[{"x":1.0,"y":2.0,"width":0.0,"align":"Center","element":"Empty"}]}}"#;
const PROBLEM: &str = r#"{"Problem":{"message":"No data"}}"#;
const OUTPUT: &str = r#"{"element":"Empty","steps":3,"stepped":true,"animated":false}"#;
const SCHEMA: &str = r#"{"name":"Score","props":[{"name":"team","kind":"Text","optional":false},{"name":"kind","kind":{"Choice":["bar","line"]},"optional":true},{"name":"logo","kind":{"File":["png"]},"optional":false}],"children":true}"#;
const INPUT: &str = r#"{"props":{"team":"A"},"ctx":{"click_step":1,"time":0.5,"slide_width":1920.0,"slide_height":1080.0,"width":800.0,"height":0.0,"theme":{"background":{"r":255,"g":255,"b":255,"a":255},"text":{"r":30,"g":41,"b":59,"a":255},"accent":{"r":37,"g":99,"b":235,"a":255},"font":"Inter","mono_font":"JetBrains Mono"}}}"#;

/// `literal` reads as `value`, and writes back as the same text.
fn holds<T: Serialize + DeserializeOwned + PartialEq + Debug>(literal: &str, value: T) {
    let parsed: T = serde_json::from_str(literal).unwrap();
    assert_eq!(parsed, value);
    assert_eq!(serde_json::to_string(&parsed).unwrap(), literal);
}

/// The app speaks version 2 now, and what it sends a version-1 module, and reads back from one,
/// is the same text as before: no state, no events, and no `interactive` in its schema.
#[test]
fn a_v1_module_still_runs() {
    assert_eq!(ABI_VERSION, 2);
    let input: RenderInput = serde_json::from_str(INPUT).unwrap();
    assert_eq!(input.state, None);
    assert!(input.events.is_empty());
    assert_eq!(serde_json::to_string(&input).unwrap(), INPUT);
    let output: RenderOutput = serde_json::from_str(OUTPUT).unwrap();
    assert_eq!(output.state, None);
    assert_eq!(serde_json::to_string(&output).unwrap(), OUTPUT);
    let schema: ComponentSchema = serde_json::from_str(SCHEMA).unwrap();
    assert!(!schema.interactive);
    assert_eq!(serde_json::to_string(&schema).unwrap(), SCHEMA);
}

#[test]
fn a_stack_of_text_holds() {
    holds(
        STACK,
        Element::Stack {
            horizontal: true,
            gap: 8.0,
            children: vec![
                Element::Text { text: "Hi".into(), size: Some(48.0), bold: true, color: Some(Color::rgb(1, 2, 3)) },
                Element::Children,
                Element::Empty,
            ],
        },
    );
}

#[test]
fn a_shape_holds() {
    holds(
        SHAPE,
        Element::Shape {
            kind: ShapeKind::Rounded,
            width: 10.0,
            height: 20.0,
            fill: None,
            outline: None,
            outline_width: 0.0,
            radius: 4.0,
            padding: 0.0,
            child: None,
        },
    );
}

#[test]
fn a_path_holds() {
    holds(
        PATH,
        Element::Path {
            data: "M0 0".into(),
            width: 5.0,
            height: 6.0,
            fill: None,
            stroke: Some(Color::rgb(0, 0, 0)),
            stroke_width: 2.0,
        },
    );
}

#[test]
fn a_picture_holds() {
    holds(PICTURE, Element::Picture { src: "assets/a.png".into(), width: 10.0, height: 20.0 });
}

#[test]
fn an_icon_holds() {
    holds(ICON, Element::Icon { name: "star".into(), size: 64.0, color: Some(Color::rgb(1, 2, 3)) });
}

#[test]
fn every_kind_of_shape_holds() {
    for (literal, kind) in [
        (r#""Rect""#, ShapeKind::Rect),
        (r#""Rounded""#, ShapeKind::Rounded),
        (r#""Ellipse""#, ShapeKind::Ellipse),
        (r#""Line""#, ShapeKind::Line),
        (r#""Arrow""#, ShapeKind::Arrow),
        (r#""Callout""#, ShapeKind::Callout),
    ] {
        holds(literal, kind);
    }
}

#[test]
fn every_text_alignment_holds() {
    for (literal, align) in [
        (r#""Left""#, TextAlign::Left),
        (r#""Center""#, TextAlign::Center),
        (r#""Right""#, TextAlign::Right),
    ] {
        holds(literal, align);
    }
}

#[test]
fn every_plain_kind_of_prop_holds() {
    for (literal, kind) in [
        (r#""Text""#, PropKind::Text),
        (r#""Whole""#, PropKind::Whole),
        (r#""Number""#, PropKind::Number),
        (r#""Flag""#, PropKind::Flag),
        (r#""Color""#, PropKind::Color),
    ] {
        holds(literal, kind);
    }
}

#[test]
fn a_canvas_holds() {
    holds(
        CANVAS,
        Element::Canvas {
            width: 100.0,
            height: 50.0,
            items: vec![at(1.0, 2.0, Element::Empty).centered_in(0.0)],
        },
    );
}

#[test]
fn a_problem_holds() {
    holds(PROBLEM, Element::Problem { message: "No data".into() });
}

#[test]
fn an_output_holds() {
    holds(OUTPUT, RenderOutput::new(Element::Empty, 3, true, false));
}

#[test]
fn a_schema_holds() {
    holds(
        SCHEMA,
        ComponentSchema::new(
            "Score",
            vec![
                PropField::new("team", PropKind::Text, false),
                PropField::new("kind", PropKind::Choice(vec!["bar".into(), "line".into()]), true),
                PropField::new("logo", PropKind::File(vec!["png".into()]), false),
            ],
            true,
        ),
    );
}

#[test]
fn an_input_holds() {
    holds(
        INPUT,
        RenderInput::new(
            BTreeMap::from([("team".to_string(), "A".to_string())]),
            CtxData::new(
                1,
                0.5,
                1920.0,
                1080.0,
                800.0,
                0.0,
                Theme {
                    background: Color::rgb(255, 255, 255),
                    text: Color::rgb(30, 41, 59),
                    accent: Color::rgb(37, 99, 235),
                    font: "Inter".into(),
                    mono_font: "JetBrains Mono".into(),
                },
            ),
        ),
    );
}

#[test]
fn an_output_with_fields_left_out_takes_their_defaults() {
    let output: RenderOutput = serde_json::from_str(r#"{"element":"Empty"}"#).unwrap();
    assert_eq!(output, RenderOutput::new(Element::Empty, 0, false, false));
}

#[test]
fn a_path_with_no_stroke_written_takes_the_defaults() {
    let path: Element = serde_json::from_str(r#"{"Path":{"data":"M0 0","width":5.0,"height":6.0,"fill":null}}"#).unwrap();
    assert_eq!(path, Element::Path { data: "M0 0".into(), width: 5.0, height: 6.0, fill: None, stroke: None, stroke_width: 0.0 });
}

#[test]
fn something_placed_with_fields_left_out_takes_their_defaults() {
    let placed: Placed = serde_json::from_str(r#"{"x":1.0,"y":2.0,"element":"Empty"}"#).unwrap();
    assert_eq!(placed, at(1.0, 2.0, Element::Empty));
}

#[test]
fn an_input_with_a_field_this_version_does_not_know_still_parses() {
    let input = INPUT.replacen(r#"{"props""#, r#"{"later":[1,2],"props""#, 1).replacen(r#""click_step""#, r#""later":{"a":1},"click_step""#, 1);
    let parsed: RenderInput = serde_json::from_str(&input).unwrap();
    assert_eq!(parsed, serde_json::from_str::<RenderInput>(INPUT).unwrap());
    let ctx = r#"{"click_step":1,"time":0.5,"slide_width":1920.0,"slide_height":1080.0,"width":800.0,"height":0.0,"later":true,"theme":{"background":{"r":255,"g":255,"b":255,"a":255},"text":{"r":30,"g":41,"b":59,"a":255},"accent":{"r":37,"g":99,"b":235,"a":255},"font":"Inter","mono_font":"JetBrains Mono"}}"#;
    assert_eq!(serde_json::from_str::<CtxData>(ctx).unwrap(), parsed.ctx);
}

#[test]
fn an_output_with_a_field_this_version_does_not_know_still_parses() {
    let output = OUTPUT.replacen(r#"{"element""#, r#"{"later":"x","element""#, 1);
    assert_eq!(serde_json::from_str::<RenderOutput>(&output).unwrap(), serde_json::from_str::<RenderOutput>(OUTPUT).unwrap());
}

#[test]
fn a_schema_with_a_field_this_version_does_not_know_still_parses() {
    let schema = SCHEMA.replacen(r#"{"name":"Score""#, r#"{"later":1,"name":"Score""#, 1).replacen(r#""optional":false}"#, r#""optional":false,"later":null}"#, 1);
    assert_eq!(serde_json::from_str::<ComponentSchema>(&schema).unwrap(), serde_json::from_str::<ComponentSchema>(SCHEMA).unwrap());
    let field = r#"{"name":"team","kind":"Text","optional":false,"default":"A"}"#;
    assert_eq!(serde_json::from_str::<PropField>(field).unwrap().name, "team");
}
