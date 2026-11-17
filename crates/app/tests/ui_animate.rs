//! The Animate tab: the step each thing and area comes in on, how it comes in, a list shown one
//! at a time, and how a slide is arrived at; and the badges that show the steps on the slide.

use std::ops::Range;

#[path = "support/typing.rs"]
mod typing;

use typing::*;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{ElementId, Modifiers, TestAppContext, px, size};
use slides_engine::{Enter, Transition};
use stagepack_studio::animate::Choice;
use stagepack_studio::animate_tools::{
    ChooseAnimation, ChooseEntrance, ENTRANCE_ID, EARLIER_ID, LATER_ID, ONE_AT_A_TIME_ID, STEP_LABEL_ID, TRANSITION_ALL_ID, TRANSITION_ID,
};
use stagepack_studio::ribbon::{ON_THE_WAY_ID, TAB_ANIMATE_ID, TAB_HOME_ID};
use stagepack_studio::selection::Thing;
use stagepack_studio::slide_editor::step_badge_id;

const TALK: &str = "---\ntitle: T\n---\n\n# Shapes\n\n- one\n- two\n- three\n\n\
<Shape kind=\"rect\" fill=\"#2563eb\" x=\"200\" y=\"500\" w=\"200\" h=\"200\" />\n\n\
<Shape kind=\"ellipse\" fill=\"#dc2626\" x=\"700\" y=\"550\" w=\"100\" h=\"100\" />\n\n\
<Icon name=\"arrow-right\" x=\"1400\" y=\"800\" size=\"96\" />\n\n---\n\n# Two\n\nJust one paragraph\n\n---\n\n# Three\n";
const RECT: &str = "<Shape kind=\"rect\"";
const ELLIPSE: &str = "<Shape kind=\"ellipse\"";
const ICON: &str = "<Icon";
const NOTHING_SELECTED: &str = "Select something on the slide first.";
const IN_THE_TEXT: &str = "Drag it out of the text first.";
const TOO_MANY: &str = "This slide has as many clicks as it can have.";
const TYPE_FIRST: &str = "Type something in it first.";

/// The Appear menu's items, and the Transition menu's.
const ENTRANCES: [&str; 4] = ["No animation", "Appear", "Fade in", "Fly up"];
const TRANSITIONS: [&str; 3] = ["None", "Fade", "Push"];

/// Opens `source` in a window wide enough that the Animate tab's tools all show, on that tab.
fn open_animate(cx: &mut TestAppContext, source: &str) -> Typing {
    let typing = open(cx, source);
    cx.simulate_window_resize(typing.handle, size(px(1800.), px(900.)));
    typing.settle(cx);
    click_on(&typing, cx, TAB_ANIMATE_ID);
    typing
}

/// `TALK` with `with` written into the tag that starts with `start`, before its end.
fn talk_with(start: &str, with: &str) -> String {
    let at = TALK.find(start).unwrap();
    let end = at + TALK[at..].find(" />").unwrap();
    format!("{}{with}{}", &TALK[..end], &TALK[end..])
}

fn of(typing: &Typing, cx: &mut TestAppContext, start: &str) -> Range<usize> {
    thing(typing, cx, start).source
}

fn click_thing(typing: &Typing, cx: &mut TestAppContext, start: &str, modifiers: Modifiers) {
    let element = thing(typing, cx, start);
    let at = middle(typing, cx, &element);
    typing.clicks(cx, at, 1, modifiers);
}

/// The tag that starts with `start`, as written.
fn tag_of(typing: &Typing, cx: &mut TestAppContext, start: &str) -> String {
    let source = typing.source(cx);
    source[of(typing, cx, start)].to_string()
}

fn undo_label(typing: &Typing, cx: &TestAppContext) -> Option<&'static str> {
    typing.document(cx).read_with(cx, |document, _| document.undo_label())
}

fn label_of(typing: &Typing, cx: &mut TestAppContext, id: impl Into<ElementId>) -> Option<String> {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(id).and_then(|found| found.label().map(str::to_string))
    })
}

fn disabled(typing: &Typing, cx: &mut TestAppContext, id: impl Into<ElementId>) -> bool {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.find(id).disabled() == Some(true)
    })
}

fn shown(typing: &Typing, cx: &mut TestAppContext, id: &str) -> bool {
    bounds_of(typing, cx, id.to_string()).is_some()
}

/// Picks item `label` of the menu the tool `tool` opens, whose items are `items`.
fn pick(typing: &Typing, cx: &mut TestAppContext, tool: &str, items: &[&str], label: &str) {
    click_on(typing, cx, tool.to_string());
    let at = items.iter().position(|item| *item == label).expect("an item of the menu");
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.within("popup-menu").click(at, cx);
    });
    typing.settle(cx);
}

/// The items of the menu the tool `tool` opens, `count` of them; the menu is closed again.
fn items_of(typing: &Typing, cx: &mut TestAppContext, tool: &str, count: usize) -> Vec<String> {
    click_on(typing, cx, tool.to_string());
    let items = typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let menu = window.within("popup-menu");
        (0..count).map(|item| menu.find(item).label().unwrap_or_default().to_string()).collect()
    });
    typing.press(cx, "escape");
    items
}

/// The transition of each slide, as the show plays it.
fn transitions(typing: &Typing, cx: &mut TestAppContext) -> Vec<Transition> {
    let count = typing.slide_count(cx);
    typing.document(cx).update(cx, |document, _| (0..count).map(|slide| document.timeline_of(slide).transition).collect())
}

/// The flow of slide `slide`'s main text area.
fn body_flow(typing: &Typing, cx: &mut TestAppContext, slide: usize) -> usize {
    let layout = typing.layout(cx);
    (0..typing.flows(cx, slide).len()).find(|&flow| layout.region_of_flow(flow) == Some("body")).expect("the main text")
}

/// Selects the main text's area of the slide shown with a press on its frame, once typing in
/// it shows the frame.
fn select_body(typing: &Typing, cx: &mut TestAppContext) {
    let slide = typing.current(cx);
    let body = body_flow(typing, cx, slide);
    typing.click_text(cx, body, pos(0, 3));
    let rect = typing.layout(cx).region("body").expect("the area").rect;
    let at = typing.at(cx, rect.x, rect.y + rect.h / 2.);
    typing.click(cx, at);
    let selection = typing.document(cx).read_with(cx, |document, _| document.selection().cloned());
    assert_eq!(selection.map(|selection| selection.things), Some(vec![Thing::Region("body".to_string())]));
}

/// The words on each badge shown on the slide, in order.
fn badges(typing: &Typing, cx: &mut TestAppContext) -> Vec<String> {
    (0..).map_while(|n| label_of(typing, cx, step_badge_id(n))).collect()
}

#[gpui_kit::test]
fn the_tab_has_its_tools_and_says_nothing_is_on_the_way(cx: &mut TestAppContext) {
    let typing = open_animate(cx, TALK);
    assert!(!shown(&typing, cx, ON_THE_WAY_ID));
    for group in ["Transition", "Appear", "Order", "List"] {
        assert!(shown(&typing, cx, &format!("group-{group}")), "{group}");
    }
    for tool in [TRANSITION_ID, TRANSITION_ALL_ID, ENTRANCE_ID, EARLIER_ID, LATER_ID, STEP_LABEL_ID, ONE_AT_A_TIME_ID] {
        assert!(shown(&typing, cx, tool), "{tool}");
    }
    assert_eq!(items_of(&typing, cx, TRANSITION_ID, 3), TRANSITIONS);
    click_thing(&typing, cx, RECT, Modifiers::default());
    assert_eq!(items_of(&typing, cx, ENTRANCE_ID, 4), ENTRANCES);
    assert_eq!(label_of(&typing, cx, STEP_LABEL_ID).as_deref(), Some("Not animated"));
    assert_eq!(typing.source(cx), TALK);
}

#[gpui_kit::test]
fn choosing_fade_in_writes_click_and_enter_in_one_step(cx: &mut TestAppContext) {
    let typing = open_animate(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    pick(&typing, cx, ENTRANCE_ID, &ENTRANCES, "Fade in");
    assert_eq!(typing.source(cx), talk_with(RECT, " click=\"1\" enter=\"fade\""));
    assert_eq!(undo_label(&typing, cx), Some("Change animation"));
    assert_eq!(label_of(&typing, cx, STEP_LABEL_ID).as_deref(), Some("Click 1 of 1"));
    assert_eq!(selected(&typing, cx).map(|(_, source)| source), Some(of(&typing, cx, RECT)), "it stays selected");
    // Another entrance changes only that.
    pick(&typing, cx, ENTRANCE_ID, &ENTRANCES, "Fly up");
    assert_eq!(typing.source(cx), talk_with(RECT, " click=\"1\" enter=\"fly-up\""));
    typing.press(cx, "ctrl-z");
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK);
}

#[gpui_kit::test]
fn no_animation_removes_both(cx: &mut TestAppContext) {
    let talk = talk_with(RECT, " click=\"1\" enter=\"fade\"");
    let typing = open_animate(cx, &talk);
    click_thing(&typing, cx, RECT, Modifiers::default());
    pick(&typing, cx, ENTRANCE_ID, &ENTRANCES, "No animation");
    assert_eq!(typing.source(cx), TALK);
    assert_eq!(label_of(&typing, cx, STEP_LABEL_ID).as_deref(), Some("Not animated"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), talk);
}

#[gpui_kit::test]
fn later_swaps_with_the_next_step(cx: &mut TestAppContext) {
    let talk = talk_with(ICON, " click=\"3\"");
    let talk = talk.replacen("h=\"200\" />", "h=\"200\" click=\"1\" />", 1).replacen("h=\"100\" />", "h=\"100\" click=\"2\" />", 1);
    let typing = open_animate(cx, &talk);
    click_thing(&typing, cx, RECT, Modifiers::default());
    assert_eq!(label_of(&typing, cx, STEP_LABEL_ID).as_deref(), Some("Click 1 of 3"));
    click_on(&typing, cx, LATER_ID);
    assert!(tag_of(&typing, cx, RECT).contains("click=\"2\""));
    assert!(tag_of(&typing, cx, ELLIPSE).contains("click=\"1\""));
    assert!(tag_of(&typing, cx, ICON).contains("click=\"3\""));
    assert_eq!(label_of(&typing, cx, STEP_LABEL_ID).as_deref(), Some("Click 2 of 3"));
    click_on(&typing, cx, EARLIER_ID);
    assert_eq!(typing.source(cx), talk);
    typing.press(cx, "ctrl-z");
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), talk);
    // At the first step, Earlier does nothing.
    click_on(&typing, cx, EARLIER_ID);
    assert_eq!(typing.source(cx), talk);
    assert!(typing.document(cx).read_with(cx, |document, _| !document.can_undo()));
    // Neither goes for a thing that isn't animated.
    let typing = open_animate(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    assert!(disabled(&typing, cx, EARLIER_ID) && disabled(&typing, cx, LATER_ID));
}

#[gpui_kit::test]
fn an_area_being_typed_in_is_the_target(cx: &mut TestAppContext) {
    let typing = open_animate(cx, TALK);
    let body = body_flow(&typing, cx, 0);
    typing.click_text(cx, body, pos(0, 2));
    assert!(typing.editing(cx).is_some());
    assert_eq!(label_of(&typing, cx, STEP_LABEL_ID).as_deref(), Some("Not animated"));
    pick(&typing, cx, ENTRANCE_ID, &ENTRANCES, "Fade in");
    assert!(typing.source(cx).contains("---\ntitle: T\nregion.body: click=1 enter=fade\n---\n"), "{}", typing.source(cx));
    // The area stays the one the tools are about.
    assert_eq!(label_of(&typing, cx, STEP_LABEL_ID).as_deref(), Some("Click 1 of 1"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK);
}

#[gpui_kit::test]
fn one_at_a_time_writes_click_each_on_the_areas_line_and_is_off_for_one_paragraph(cx: &mut TestAppContext) {
    let typing = open_animate(cx, TALK);
    select_body(&typing, cx);
    assert!(!disabled(&typing, cx, ONE_AT_A_TIME_ID));
    click_on(&typing, cx, ONE_AT_A_TIME_ID);
    assert!(typing.source(cx).contains("region.body: click=each\n"), "{}", typing.source(cx));
    let checked = |typing: &Typing, cx: &mut TestAppContext| {
        typing.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window.find(ONE_AT_A_TIME_ID).checked()
        })
    };
    assert_eq!(checked(&typing, cx), Some(true));
    assert_eq!(badges(&typing, cx), ["1\u{2013}3"]);
    assert_eq!(typing.document(cx).update(cx, |document, _| document.timeline_of(0).steps), 3);
    click_on(&typing, cx, ONE_AT_A_TIME_ID);
    assert_eq!(typing.source(cx), TALK);
    assert_eq!(checked(&typing, cx), Some(false));
    // One paragraph can't be shown a piece at a time.
    typing.press(cx, "escape");
    go_to(&typing, cx, 1);
    select_body(&typing, cx);
    assert!(disabled(&typing, cx, ONE_AT_A_TIME_ID));
}

#[gpui_kit::test]
fn turning_one_at_a_time_off_closes_the_steps_it_took(cx: &mut TestAppContext) {
    let talk = TALK.replacen("title: T\n", "title: T\nregion.body: click=each\n", 1).replacen("h=\"200\" />", "h=\"200\" click=\"4\" />", 1);
    let typing = open_animate(cx, &talk);
    select_body(&typing, cx);
    click_on(&typing, cx, ONE_AT_A_TIME_ID);
    assert!(!typing.source(cx).contains("region.body"));
    assert!(tag_of(&typing, cx, RECT).contains("click=\"1\""), "{}", typing.source(cx));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), talk, "one step");
}

#[gpui_kit::test]
fn a_transition_is_written_for_this_slide(cx: &mut TestAppContext) {
    let typing = open_animate(cx, TALK);
    go_to(&typing, cx, 1);
    pick(&typing, cx, TRANSITION_ID, &TRANSITIONS, "Fade");
    assert_eq!(transitions(&typing, cx), [Transition::None, Transition::Fade, Transition::None]);
    assert!(typing.source(cx).contains("---\ntransition: fade\n---\n"), "{}", typing.source(cx));
    assert_eq!(undo_label(&typing, cx), Some("Change transition"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK);
}

#[gpui_kit::test]
fn apply_to_all_writes_every_slide_in_one_step(cx: &mut TestAppContext) {
    let typing = open_animate(cx, TALK);
    pick(&typing, cx, TRANSITION_ID, &TRANSITIONS, "Push");
    assert_eq!(transitions(&typing, cx), [Transition::Push, Transition::None, Transition::None]);
    click_on(&typing, cx, TRANSITION_ALL_ID);
    assert_eq!(transitions(&typing, cx), [Transition::Push; 3]);
    assert_eq!(undo_label(&typing, cx), Some("Change transition"));
    typing.press(cx, "ctrl-z");
    assert_eq!(transitions(&typing, cx), [Transition::Push, Transition::None, Transition::None]);
}

#[gpui_kit::test]
fn none_removes_the_setting(cx: &mut TestAppContext) {
    let typing = open_animate(cx, TALK);
    go_to(&typing, cx, 1);
    pick(&typing, cx, TRANSITION_ID, &TRANSITIONS, "Push");
    pick(&typing, cx, TRANSITION_ID, &TRANSITIONS, "None");
    assert_eq!(typing.source(cx), TALK);
    // One the talk was written with too.
    let talk = TALK.replacen("---\n\n# Two", "---\ntransition: fade\n---\n\n# Two", 1);
    let typing = open_animate(cx, &talk);
    go_to(&typing, cx, 1);
    pick(&typing, cx, TRANSITION_ID, &TRANSITIONS, "None");
    assert!(!typing.source(cx).contains("transition"), "{}", typing.source(cx));
    assert_eq!(transitions(&typing, cx), [Transition::None; 3]);
}

#[gpui_kit::test]
fn nothing_selected_disables_the_tools_and_says_why(cx: &mut TestAppContext) {
    let typing = open_animate(cx, TALK);
    for tool in [ENTRANCE_ID, EARLIER_ID, LATER_ID, ONE_AT_A_TIME_ID] {
        assert!(disabled(&typing, cx, tool), "{tool}");
    }
    assert!(!disabled(&typing, cx, TRANSITION_ID) && !disabled(&typing, cx, TRANSITION_ALL_ID));
    assert_eq!(label_of(&typing, cx, STEP_LABEL_ID).as_deref(), Some(NOTHING_SELECTED));
    typing.with_window(cx, |window, cx| window.dispatch_action(Box::new(ChooseAnimation { choice: Choice::Later }), cx));
    typing.settle(cx);
    assert_eq!(typing.status(cx).as_deref(), Some(NOTHING_SELECTED));
    assert_eq!(typing.source(cx), TALK);
}

#[gpui_kit::test]
fn a_table_in_the_text_says_drag_it_out_first(cx: &mut TestAppContext) {
    let talk = "# T\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n<Shape kind=\"rect\" x=\"100\" y=\"500\" w=\"200\" h=\"200\" />\n";
    let typing = open_animate(cx, talk);
    let table = of(&typing, cx, "| A");
    typing.document(cx).update(cx, |document, cx| {
        document.select(0, table);
        cx.notify();
    });
    typing.settle(cx);
    assert_eq!(label_of(&typing, cx, STEP_LABEL_ID).as_deref(), Some(IN_THE_TEXT));
    assert!(disabled(&typing, cx, ENTRANCE_ID));
    typing.with_window(cx, |window, cx| window.dispatch_action(Box::new(ChooseAnimation { choice: Choice::Animate }), cx));
    typing.settle(cx);
    assert_eq!(typing.status(cx).as_deref(), Some(IN_THE_TEXT));
    assert_eq!(typing.source(cx), talk);
}

#[gpui_kit::test]
fn badges_show_only_on_the_animate_tab(cx: &mut TestAppContext) {
    let talk = TALK
        .replacen("title: T\n", "title: T\nregion.body: click=each\n", 1)
        .replacen("h=\"200\" />", "h=\"200\" click=\"2\" />", 1)
        .replacen("h=\"100\" />", "h=\"100\" click=\"5\" />", 1);
    let typing = open_animate(cx, &talk);
    assert_eq!(badges(&typing, cx), ["1\u{2013}3", "2", "5"]);
    // At the thing's top-left corner.
    let rect = thing(&typing, cx, RECT).rect;
    let corner = typing.at(cx, rect.x, rect.y);
    let badge = bounds_of(&typing, cx, step_badge_id(1)).expect("the badge");
    let editor = bounds_of(&typing, cx, stagepack_studio::slide_editor::SLIDE_EDITOR_ID).expect("the editor");
    let at = badge.origin;
    assert!((at.x - corner.x).abs() <= px(1.) && (at.y - corner.y).abs() <= px(1.), "{at:?} isn't at {corner:?} in {editor:?}");
    click_on(&typing, cx, TAB_HOME_ID);
    assert!(badges(&typing, cx).is_empty());
    click_on(&typing, cx, TAB_ANIMATE_ID);
    assert_eq!(badges(&typing, cx).len(), 3);
    // Nothing is drawn again while the window sits idle.
    let editor = typing.view.read_with(cx, |view, _| view.slide_editor().clone());
    let (count, _watching) = redraws(cx, &editor);
    wait(cx, 2000);
    assert_eq!(count.get(), 0);
}

#[gpui_kit::test]
fn the_show_plays_what_was_chosen(cx: &mut TestAppContext) {
    let typing = open_animate(cx, TALK);
    click_thing(&typing, cx, RECT, Modifiers::default());
    pick(&typing, cx, ENTRANCE_ID, &ENTRANCES, "Fade in");
    click_thing(&typing, cx, ELLIPSE, Modifiers::default());
    pick(&typing, cx, ENTRANCE_ID, &ENTRANCES, "Fly up");
    // The ellipse a step earlier: before the rectangle.
    click_on(&typing, cx, EARLIER_ID);
    click_thing(&typing, cx, ICON, Modifiers::default());
    pick(&typing, cx, ENTRANCE_ID, &ENTRANCES, "Appear");
    let timeline = typing.document(cx).update(cx, |document, _| document.timeline_of(0));
    assert_eq!(timeline.steps, 3);
    let clicked: Vec<(u32, Enter)> = timeline.layers.iter().filter_map(|layer| Some((layer.click?, layer.enter))).collect();
    assert_eq!(clicked, [(2, Enter::Fade), (1, Enter::FlyUp), (3, Enter::Appear)]);
    assert_eq!(tag_of(&typing, cx, ICON), "<Icon name=\"arrow-right\" x=\"1400\" y=\"800\" size=\"96\" click=\"3\" enter=\"appear\" />");
}

#[gpui_kit::test]
fn hand_written_click_zero_reads_as_not_animated(cx: &mut TestAppContext) {
    let talk = talk_with(RECT, " click=\"0\"");
    let typing = open_animate(cx, &talk);
    click_thing(&typing, cx, RECT, Modifiers::default());
    assert_eq!(label_of(&typing, cx, STEP_LABEL_ID).as_deref(), Some("Not animated"));
    assert!(disabled(&typing, cx, LATER_ID));
    pick(&typing, cx, ENTRANCE_ID, &ENTRANCES, "Fade in");
    assert_eq!(typing.source(cx), talk_with(RECT, " click=\"1\" enter=\"fade\""));
}

#[gpui_kit::test]
fn a_click_written_at_the_last_step_makes_room_for_another(cx: &mut TestAppContext) {
    let talk = talk_with(RECT, " click=\"999\"");
    let typing = open_animate(cx, &talk);
    click_thing(&typing, cx, ELLIPSE, Modifiers::default());
    pick(&typing, cx, ENTRANCE_ID, &ENTRANCES, "Fade in");
    assert!(tag_of(&typing, cx, RECT).contains("click=\"1\""), "{}", typing.source(cx));
    assert!(tag_of(&typing, cx, ELLIPSE).contains("click=\"2\" enter=\"fade\""), "{}", typing.source(cx));
}

#[gpui_kit::test]
fn a_slide_with_every_step_taken_refuses_another(cx: &mut TestAppContext) {
    // A list shown an item at a time, of as many items as a slide has steps.
    let mut talk = String::from("---\nregion.body: click=each\n---\n\n# Full\n\n");
    for _ in 0..999 {
        talk.push_str("- a\n");
    }
    talk.push_str("\n<Shape kind=\"ellipse\" x=\"700\" y=\"550\" w=\"100\" h=\"100\" />\n");
    let typing = open_animate(cx, &talk);
    click_thing(&typing, cx, ELLIPSE, Modifiers::default());
    pick(&typing, cx, ENTRANCE_ID, &ENTRANCES, "Fade in");
    assert_eq!(typing.status(cx).as_deref(), Some(TOO_MANY));
    assert_eq!(typing.source(cx), talk);
}

#[gpui_kit::test]
fn no_animation_takes_away_a_click_that_cannot_be_read(cx: &mut TestAppContext) {
    for written in [" click=\"0\" enter=\"fade\"", " click=\"x\"", " enter=\"sideways\""] {
        let talk = talk_with(RECT, written);
        let typing = open_animate(cx, &talk);
        click_thing(&typing, cx, RECT, Modifiers::default());
        pick(&typing, cx, ENTRANCE_ID, &ENTRANCES, "No animation");
        assert_eq!(typing.source(cx), TALK, "{written}");
    }
}

#[gpui_kit::test]
fn the_box_being_typed_in_is_the_target(cx: &mut TestAppContext) {
    let talk = "# T\n\n<Text x=\"200\" y=\"300\" w=\"800\" h=\"400\">\n\nHi there\n\n</Text>\n";
    let typing = open_animate(cx, talk);
    let flow = (0..typing.flows(cx, 0).len()).find(|&flow| typing.layout(cx).region_of_flow(flow).is_none()).expect("the box's text");
    typing.double_click_text(cx, flow, pos(0, 2));
    assert!(typing.editing(cx).is_some());
    assert_eq!(label_of(&typing, cx, STEP_LABEL_ID).as_deref(), Some("Not animated"));
    pick(&typing, cx, ENTRANCE_ID, &ENTRANCES, "Fade in");
    assert_eq!(typing.source(cx), talk.replace("h=\"400\">", "h=\"400\" click=\"1\" enter=\"fade\">"));
    let source = typing.source(cx);
    assert_eq!(selected(&typing, cx).map(|(_, range)| source[range].starts_with("<Text")), Some(true), "the box is selected");
    // Typed in, the box was selected all along: Undo leaves it so.
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), talk);
    assert_eq!(selected(&typing, cx).map(|(_, range)| talk[range].starts_with("<Text")), Some(true));
    typing.press(cx, "ctrl-shift-z");
    assert_eq!(typing.source(cx), talk.replace("h=\"400\">", "h=\"400\" click=\"1\" enter=\"fade\">"));
    assert!(selected(&typing, cx).is_some(), "selected after Redo");
}

#[gpui_kit::test]
fn a_choice_that_changes_nothing_leaves_the_typing_be(cx: &mut TestAppContext) {
    let typing = open_animate(cx, TALK);
    let body = body_flow(&typing, cx, 0);
    typing.click_text(cx, body, pos(0, 2));
    pick(&typing, cx, ENTRANCE_ID, &ENTRANCES, "No animation");
    typing.with_window(cx, |window, cx| window.dispatch_action(Box::new(ChooseAnimation { choice: Choice::Earlier }), cx));
    typing.settle(cx);
    assert!(typing.editing(cx).is_some(), "still typing");
    assert_eq!(typing.source(cx), TALK);
    assert!(typing.document(cx).read_with(cx, |document, _| !document.can_undo()));
    // The area chosen while typed in is selected; Undo lets go of it.
    pick(&typing, cx, ENTRANCE_ID, &ENTRANCES, "Appear");
    let selection = typing.document(cx).read_with(cx, |document, _| document.selection().cloned());
    assert_eq!(selection.map(|selection| selection.things), Some(vec![Thing::Region("body".to_string())]));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK);
    assert!(typing.document(cx).read_with(cx, |document, _| document.selection().is_none()));
}

#[gpui_kit::test]
fn an_area_emptied_by_typing_is_not_animated_and_says_why(cx: &mut TestAppContext) {
    let typing = open_animate(cx, TALK);
    go_to(&typing, cx, 1);
    let body = body_flow(&typing, cx, 1);
    typing.click_text(cx, body, pos(0, 2));
    typing.press(cx, "ctrl-a");
    typing.press(cx, "backspace");
    typing.with_window(cx, |window, cx| window.dispatch_action(Box::new(ChooseEntrance { enter: Some(Enter::Fade) }), cx));
    typing.settle(cx);
    assert_eq!(typing.status(cx).as_deref(), Some(TYPE_FIRST));
    assert!(!typing.source(cx).contains("region.body"), "{}", typing.source(cx));
}

#[gpui_kit::test]
fn an_empty_area_is_not_animated_and_says_why(cx: &mut TestAppContext) {
    let typing = open_animate(cx, TALK);
    go_to(&typing, cx, 2);
    let at = typing.placeholder_point(cx, "body");
    typing.click(cx, at);
    assert!(typing.editing(cx).is_some(), "typing in the empty area");
    assert_eq!(label_of(&typing, cx, STEP_LABEL_ID).as_deref(), Some(TYPE_FIRST));
    assert!(disabled(&typing, cx, ENTRANCE_ID));
    for action in [
        Box::new(ChooseEntrance { enter: Some(Enter::Fade) }) as Box<dyn gpui_kit::Action>,
        Box::new(ChooseAnimation { choice: Choice::Animate }),
    ] {
        typing.with_window(cx, |window, cx| window.dispatch_action(action, cx));
        typing.settle(cx);
        assert_eq!(typing.status(cx).as_deref(), Some(TYPE_FIRST));
        assert_eq!(typing.source(cx), TALK);
    }
    assert!(typing.document(cx).read_with(cx, |document, _| !document.can_undo()));
}

/// A talk whose second slide's main area is written as animated, with nothing in it, beside a
/// shape.
const STRAY: &str = "---\ntitle: T\n---\n\n# Shapes\n\n---\nregion.body: click=1 enter=fade\n---\n\n# Empty\n\n\
<Shape kind=\"rect\" fill=\"#2563eb\" x=\"200\" y=\"500\" w=\"200\" h=\"200\" />\n";

#[gpui_kit::test]
fn a_click_written_on_an_empty_area_is_not_counted(cx: &mut TestAppContext) {
    let typing = open_animate(cx, STRAY);
    go_to(&typing, cx, 1);
    assert_eq!(typing.document(cx).update(cx, |document, _| document.timeline_of(1).steps), 0, "the show gives it no step");
    assert!(badges(&typing, cx).is_empty(), "no badge says it comes in");
    click_thing(&typing, cx, RECT, Modifiers::default());
    pick(&typing, cx, ENTRANCE_ID, &ENTRANCES, "Fade in");
    assert!(tag_of(&typing, cx, RECT).contains("click=\"1\" enter=\"fade\""), "{}", typing.source(cx));
    assert_eq!(label_of(&typing, cx, STEP_LABEL_ID).as_deref(), Some("Click 1 of 1"));
}

#[gpui_kit::test]
fn no_animation_takes_the_words_away_from_an_empty_area(cx: &mut TestAppContext) {
    let typing = open_animate(cx, STRAY);
    go_to(&typing, cx, 1);
    let at = typing.placeholder_point(cx, "body");
    typing.click(cx, at);
    assert!(!disabled(&typing, cx, ENTRANCE_ID), "what's written on it can be taken away");
    // Anything else is still refused.
    typing.with_window(cx, |window, cx| window.dispatch_action(Box::new(ChooseEntrance { enter: Some(Enter::FlyUp) }), cx));
    typing.settle(cx);
    assert_eq!(typing.status(cx).as_deref(), Some(TYPE_FIRST));
    assert_eq!(typing.source(cx), STRAY);
    pick(&typing, cx, ENTRANCE_ID, &ENTRANCES, "No animation");
    assert!(!typing.source(cx).contains("click=1") && !typing.source(cx).contains("enter=fade"), "{}", typing.source(cx));
    assert!(typing.source(cx).contains("<Shape kind=\"rect\" fill=\"#2563eb\" x=\"200\" y=\"500\" w=\"200\" h=\"200\" />"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), STRAY);
}

#[gpui_kit::test]
fn one_paragraph_is_not_shown_a_piece_at_a_time_however_it_is_asked(cx: &mut TestAppContext) {
    let typing = open_animate(cx, TALK);
    go_to(&typing, cx, 1);
    select_body(&typing, cx);
    typing.with_window(cx, |window, cx| window.dispatch_action(Box::new(ChooseAnimation { choice: Choice::Each(true) }), cx));
    typing.settle(cx);
    assert_eq!(typing.status(cx).as_deref(), Some("Select an area that holds a list to show it one item at a time."));
    assert_eq!(typing.source(cx), TALK);
}

#[gpui_kit::test]
fn some_animated_and_some_not_says_so(cx: &mut TestAppContext) {
    let talk = talk_with(RECT, " click=\"1\"");
    let typing = open_animate(cx, &talk);
    click_thing(&typing, cx, RECT, Modifiers::default());
    click_thing(&typing, cx, ELLIPSE, Modifiers::shift());
    assert_eq!(label_of(&typing, cx, STEP_LABEL_ID).as_deref(), Some("Some aren't animated"));
    let talk = talk.replacen("h=\"100\" />", "h=\"100\" click=\"2\" />", 1);
    let typing = open_animate(cx, &talk);
    click_thing(&typing, cx, RECT, Modifiers::default());
    click_thing(&typing, cx, ELLIPSE, Modifiers::shift());
    assert_eq!(label_of(&typing, cx, STEP_LABEL_ID).as_deref(), Some("Clicks differ"));
}

#[gpui_kit::test]
fn a_badge_follows_a_thing_dragged(cx: &mut TestAppContext) {
    let talk = talk_with(RECT, " click=\"1\"");
    let typing = open_animate(cx, &talk);
    let rect = thing(&typing, cx, RECT).rect;
    let from = typing.at(cx, rect.x + rect.w / 2., rect.y + rect.h / 2.);
    let to = typing.at(cx, rect.x + rect.w / 2. + 300., rect.y + rect.h / 2. + 100.);
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, typing.at(cx, rect.x + rect.w / 2. + 150., rect.y + rect.h / 2. + 50.), Modifiers::default());
    typing.move_held(cx, to, Modifiers::default());
    let badge = bounds_of(&typing, cx, step_badge_id(0)).expect("the badge").origin;
    let corner = typing.at(cx, rect.x + 300., rect.y + 100.);
    assert!((badge.x - corner.x).abs() <= px(2.) && (badge.y - corner.y).abs() <= px(2.), "{badge:?} isn't at {corner:?}");
    typing.mouse_up(cx, to, 1, Modifiers::default());
    let rect = thing(&typing, cx, RECT).rect;
    let corner = typing.at(cx, rect.x, rect.y);
    let badge = bounds_of(&typing, cx, step_badge_id(0)).expect("the badge").origin;
    assert!((badge.x - corner.x).abs() <= px(1.) && (badge.y - corner.y).abs() <= px(1.), "{badge:?} isn't at {corner:?}");
}

#[gpui_kit::test]
fn a_turned_list_shown_one_at_a_time_comes_in_whole_on_click_one(cx: &mut TestAppContext) {
    let talk = TALK
        .replacen("title: T\n", "title: T\nregion.body: click=each rotate=10\n", 1)
        .replacen("h=\"200\" />", "h=\"200\" click=\"2\" />", 1);
    let typing = open_animate(cx, &talk);
    assert_eq!(badges(&typing, cx), ["1", "2"]);
    assert_eq!(typing.document(cx).update(cx, |document, _| document.timeline_of(0).steps), 2);
}
