//! Version 2 of the interface: what version 1 has, and a component's state and the events that
//! reached it since its last render. Components built for it depend on exactly this text.

use std::collections::BTreeMap;

use toolkit::prelude::*;

const THEME: &str = r#"{"background":{"r":255,"g":255,"b":255,"a":255},"text":{"r":30,"g":41,"b":59,"a":255},"accent":{"r":37,"g":99,"b":235,"a":255},"font":"Inter","mono_font":"JetBrains Mono"}"#;

fn ctx_json() -> String {
    format!(r#"{{"click_step":0,"time":0.0,"slide_width":1920.0,"slide_height":1080.0,"width":400.0,"height":200.0,"theme":{THEME}}}"#)
}

fn data() -> CtxData {
    CtxData::new(
        0,
        0.0,
        1920.0,
        1080.0,
        400.0,
        200.0,
        Theme {
            background: Color::rgb(255, 255, 255),
            text: Color::rgb(30, 41, 59),
            accent: Color::rgb(37, 99, 235),
            font: "Inter".into(),
            mono_font: "JetBrains Mono".into(),
        },
    )
}

#[test]
fn v2_round_trips() {
    let input = format!(
        r#"{{"props":{{}},"ctx":{},"state":{{"count":2}},"events":[{{"Pointer":{{"kind":"Down","x":1.5,"y":2.0,"hit":"plus"}}}},{{"Pointer":{{"kind":{{"Wheel":{{"dy":-3.0}}}},"x":0.0,"y":0.0,"hit":null}}}},{{"Action":{{"name":"add","value":"5"}}}}]}}"#,
        ctx_json()
    );
    let events = vec![
        Event::Pointer { kind: PointerKind::Down, x: 1.5, y: 2.0, hit: Some("plus".into()) },
        Event::Pointer { kind: PointerKind::Wheel { dy: -3.0 }, x: 0.0, y: 0.0, hit: None },
        Event::Action { name: "add".into(), value: "5".into() },
    ];
    let expected = RenderInput::new(BTreeMap::new(), data()).with_state(Some(serde_json::json!({"count": 2})), events);
    let parsed: RenderInput = serde_json::from_str(&input).unwrap();
    assert_eq!(parsed, expected);
    assert_eq!(serde_json::to_string(&parsed).unwrap(), input);

    let output = r#"{"element":{"Hit":{"name":"plus","child":"Empty"}},"steps":0,"stepped":false,"animated":false,"state":{"count":3}}"#;
    let parsed: RenderOutput = serde_json::from_str(output).unwrap();
    assert_eq!(parsed, RenderOutput::new(hit("plus", Element::Empty), 0, false, false).with_state(Some(serde_json::json!({"count": 3}))));
    assert_eq!(serde_json::to_string(&parsed).unwrap(), output);

    let without = format!(r#"{{"props":{{}},"ctx":{}}}"#, ctx_json());
    let parsed: RenderInput = serde_json::from_str(&without).unwrap();
    assert_eq!(parsed.state, None);
    assert!(parsed.events.is_empty());
    assert_eq!(serde_json::to_string(&parsed).unwrap(), without);
}

#[test]
fn a_schema_says_whether_it_is_interactive() {
    let schema = r#"{"name":"Counter","props":[],"children":false,"interactive":true}"#;
    let parsed: ComponentSchema = serde_json::from_str(schema).unwrap();
    assert!(parsed.interactive);
    assert_eq!(serde_json::to_string(&parsed).unwrap(), schema);
    let plain: ComponentSchema = serde_json::from_str(r#"{"name":"Score","props":[],"children":false}"#).unwrap();
    assert!(!plain.interactive);
}

fn no_assets(_: &str) -> Option<Vec<u8>> {
    None
}

#[test]
fn a_component_reads_its_events_and_keeps_state() {
    let events = vec![Event::Action { name: "add".into(), value: "5".into() }];
    let ctx = Ctx::with_input(data(), Some(serde_json::json!(2)), events.clone(), &no_assets);
    assert_eq!(ctx.events(), events.as_slice());
    let count: u32 = ctx.state();
    assert_eq!(count, 2);
    ctx.set_state(&(count + 5));
    assert_eq!(ctx.state::<u32>(), 7);
    assert_eq!(ctx.finish(Element::Empty).state, Some(serde_json::json!(7)));
}

#[test]
fn state_that_is_absent_or_does_not_parse_is_the_default() {
    let ctx = Ctx::new(data(), &no_assets);
    assert!(ctx.events().is_empty());
    assert_eq!(ctx.state::<u32>(), 0);
    assert_eq!(ctx.finish(Element::Empty).state, None);
    let ctx = Ctx::with_input(data(), Some(serde_json::json!("many")), Vec::new(), &no_assets);
    assert_eq!(ctx.state::<u32>(), 0);
}

#[component(interactive)]
fn Counter(ctx: &Ctx) -> Element {
    let count = ctx.state::<u32>() + ctx.events().len() as u32;
    ctx.set_state(&count);
    hit("plus", text(count.to_string()))
}

#[component]
fn Plain() -> Element {
    text("plain")
}

#[test]
fn an_interactive_component_says_so_in_its_schema() {
    assert!((Counter_component().schema)().interactive);
    assert!(!(Plain_component().schema)().interactive);
    let schema = serde_json::to_string(&(Plain_component().schema)()).unwrap();
    assert_eq!(schema, r#"{"name":"Plain","props":[],"children":false}"#);
}
