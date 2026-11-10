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

#[test]
fn the_version_is_one() {
    assert_eq!(ABI_VERSION, 1);
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
            items: vec![Placed { x: 1.0, y: 2.0, width: 0.0, align: TextAlign::Center, element: Element::Empty }],
        },
    );
}

#[test]
fn a_problem_holds() {
    holds(PROBLEM, Element::Problem { message: "No data".into() });
}

#[test]
fn an_output_holds() {
    holds(OUTPUT, RenderOutput { element: Element::Empty, steps: 3, stepped: true, animated: false });
}

#[test]
fn a_schema_holds() {
    holds(
        SCHEMA,
        ComponentSchema {
            name: "Score".into(),
            props: vec![
                PropField { name: "team".into(), kind: PropKind::Text, optional: false },
                PropField { name: "kind".into(), kind: PropKind::Choice(vec!["bar".into(), "line".into()]), optional: true },
                PropField { name: "logo".into(), kind: PropKind::File(vec!["png".into()]), optional: false },
            ],
            children: true,
        },
    );
}

#[test]
fn an_input_holds() {
    holds(
        INPUT,
        RenderInput {
            props: BTreeMap::from([("team".to_string(), "A".to_string())]),
            ctx: CtxData {
                click_step: 1,
                time: 0.5,
                slide_width: 1920.0,
                slide_height: 1080.0,
                width: 800.0,
                height: 0.0,
                theme: Theme {
                    background: Color::rgb(255, 255, 255),
                    text: Color::rgb(30, 41, 59),
                    accent: Color::rgb(37, 99, 235),
                    font: "Inter".into(),
                    mono_font: "JetBrains Mono".into(),
                },
            },
        },
    );
}

#[test]
fn an_output_with_fields_left_out_takes_their_defaults() {
    let output: RenderOutput = serde_json::from_str(r#"{"element":"Empty"}"#).unwrap();
    assert_eq!(output, RenderOutput { element: Element::Empty, steps: 0, stepped: false, animated: false });
}

#[test]
fn a_path_with_no_stroke_written_takes_the_defaults() {
    let path: Element = serde_json::from_str(r#"{"Path":{"data":"M0 0","width":5.0,"height":6.0,"fill":null}}"#).unwrap();
    assert_eq!(path, Element::Path { data: "M0 0".into(), width: 5.0, height: 6.0, fill: None, stroke: None, stroke_width: 0.0 });
}

#[test]
fn something_placed_with_fields_left_out_takes_their_defaults() {
    let placed: Placed = serde_json::from_str(r#"{"x":1.0,"y":2.0,"element":"Empty"}"#).unwrap();
    assert_eq!(placed, Placed { x: 1.0, y: 2.0, width: 0.0, align: TextAlign::Left, element: Element::Empty });
}
