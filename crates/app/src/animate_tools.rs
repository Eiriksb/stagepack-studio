//! The Animate tab: how the slide is arrived at, how the things selected come in and on which
//! click, and a list shown one item at a time. Each tool asks the window to do its work, and is
//! off while there's nothing it can act on, the tab saying why.
//!
//! A press on a tool leaves the keyboard where it is, so the area whose text is being typed stays
//! the one the tools act on.

use gpui_kit::assets::IconName as Icons;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _};
use gpui_kit::*;
use slides_engine::{Enter, Transition};

use crate::animate::Choice;
use crate::document::AnimateHere;
use crate::ribbon::group;

/// The tool that chooses how the slide is arrived at, and the one that gives every slide that.
pub const TRANSITION_ID: &str = "transition";
pub const TRANSITION_ALL_ID: &str = "transition-all";
/// The tool that chooses how what's selected comes in.
pub const ENTRANCE_ID: &str = "entrance";
/// The tools that move what's selected a click earlier or later, and the words saying which
/// click it comes in on.
pub const EARLIER_ID: &str = "earlier";
pub const LATER_ID: &str = "later";
pub const STEP_LABEL_ID: &str = "step-label";
/// The toggle that shows the area selected one item at a time.
pub const ONE_AT_A_TIME_ID: &str = "one-at-a-time";

/// What the Order group says of what isn't animated.
pub const NOT_ANIMATED: &str = "Not animated";

/// Makes a choice for what the Animate tab acts on.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = animate_tools, no_json)]
pub struct ChooseAnimation {
    pub choice: Choice,
}

/// Has what the Animate tab acts on come in as `enter` says, or with `None`, not animated.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = animate_tools, no_json)]
pub struct ChooseEntrance {
    pub enter: Option<Enter>,
}

/// Has the current slide, or with `all` every slide, arrived at as `transition` says.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = animate_tools, no_json)]
pub struct ChooseTransition {
    pub transition: Transition,
    pub all: bool,
}

/// The ways a slide is arrived at, by what the menu calls them.
pub const TRANSITIONS: [(Transition, &str); 3] = [(Transition::None, "None"), (Transition::Fade, "Fade"), (Transition::Push, "Push")];

/// The ways a thing comes in, by what the menu calls them, not animated first.
pub const ENTRANCES: [(Option<Enter>, &str); 4] =
    [(None, "No animation"), (Some(Enter::Appear), "Appear"), (Some(Enter::Fade), "Fade in"), (Some(Enter::FlyUp), "Fly up")];

/// What the Order group says of what `here` is about.
pub fn step_words(here: &AnimateHere) -> String {
    if let Some(why) = here.why_not {
        return why.to_string();
    }
    match (here.click, here.enter, here.each) {
        (_, _, Some(true)) => "One at a time".to_string(),
        (Some(click), ..) => format!("Click {click} of {}", here.steps),
        (None, Some(_), _) => "Clicks differ".to_string(),
        (None, None, _) => NOT_ANIMATED.to_string(),
    }
}

/// The Animate tab's groups, for what `here` says the tools are about, in a window with a talk
/// open if `has_talk`.
pub fn animate_tab(here: &AnimateHere, has_talk: bool, cx: &App) -> Vec<AnyElement> {
    let off = here.why_not.is_some() || !has_talk;
    let current = here.transition;
    let transition_name = TRANSITIONS.iter().find(|(transition, _)| *transition == current).map_or("None", |(_, name)| name);
    let transition = Button::new(TRANSITION_ID)
        .ghost()
        .small()
        .icon(Icons::Film)
        .label(transition_name)
        .dropdown_caret(true)
        .tooltip("How this slide comes on in the show")
        .disabled(!has_talk)
        .dropdown_menu(move |menu, _, _| {
            TRANSITIONS.iter().fold(menu, |menu, &(transition, label)| {
                menu.menu_with_check(label, transition == current, Box::new(ChooseTransition { transition, all: false }))
            })
        });
    let all = Button::new(TRANSITION_ALL_ID)
        .ghost()
        .small()
        .icon(Icons::CopyCheck)
        .label("Apply to all slides")
        .tooltip("Give every slide this slide's transition")
        .disabled(!has_talk)
        .on_click(move |_, window, cx| window.dispatch_action(Box::new(ChooseTransition { transition: current, all: true }), cx));
    let enter = here.enter;
    let entrance_name = ENTRANCES.iter().find(|(each, _)| *each == enter).map_or("No animation", |(_, name)| name);
    let entrance = Button::new(ENTRANCE_ID)
        .ghost()
        .small()
        .icon(Icons::Sparkles)
        .label(entrance_name)
        .dropdown_caret(true)
        .tooltip(here.why_not.map_or_else(|| SharedString::from("How it comes in, on a click of its own"), |why| why.to_string().into()))
        .disabled(off)
        .dropdown_menu(move |menu, _, _| {
            ENTRANCES.iter().fold(menu, |menu, &(each, label)| menu.menu_with_check(label, each == enter, Box::new(ChooseEntrance { enter: each })))
        });
    // Only what comes in on a numbered click goes earlier or later.
    let steps_off = off || here.enter.is_none() || here.each == Some(true);
    let step_tip = |tip: &'static str| -> SharedString {
        match here.why_not {
            Some(why) => why.to_string().into(),
            None if steps_off => NOT_ANIMATED.into(),
            None => tip.into(),
        }
    };
    let earlier = Button::new(EARLIER_ID)
        .ghost()
        .small()
        .icon(Icons::StepBack)
        .label("Earlier")
        .tooltip(step_tip("Come in a click earlier"))
        .disabled(steps_off)
        .on_click(|_, window, cx| window.dispatch_action(Box::new(ChooseAnimation { choice: Choice::Earlier }), cx));
    let later = Button::new(LATER_ID)
        .ghost()
        .small()
        .icon(Icons::StepForward)
        .label("Later")
        .tooltip(step_tip("Come in a click later"))
        .disabled(steps_off)
        .on_click(|_, window, cx| window.dispatch_action(Box::new(ChooseAnimation { choice: Choice::Later }), cx));
    let words = step_words(here);
    let label = div()
        .id(STEP_LABEL_ID)
        .test_support()
        .aria_label(words.clone())
        .px_1()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(words);
    let each = here.each == Some(true);
    let one_at_a_time = Button::new(ONE_AT_A_TIME_ID)
        .ghost()
        .small()
        .icon(Icons::ListStart)
        .label("One at a time")
        .tooltip(match here.why_not {
            Some(why) => SharedString::from(why.to_string()),
            None if !here.can_each => "Select an area that holds a list".into(),
            None => "Show the list a click at a time".into(),
        })
        .selected(each)
        .toggled(each)
        .disabled(off || !here.can_each)
        .on_click(move |_, window, cx| window.dispatch_action(Box::new(ChooseAnimation { choice: Choice::Each(!each) }), cx));
    vec![
        group("Transition", vec![transition.into_any_element(), all.into_any_element()], cx),
        group("Appear", vec![entrance.into_any_element()], cx),
        group("Order", vec![earlier.into_any_element(), later.into_any_element(), label.into_any_element()], cx),
        group("List", vec![one_at_a_time.into_any_element()], cx),
    ]
}
