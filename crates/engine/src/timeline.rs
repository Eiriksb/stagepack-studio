//! When a slide's layers appear, how they come in, and how the slide itself is arrived at.
//! A talk writes these as `click`, `enter` and `transition`; a value that can't be read counts
//! as absent.

use crate::Notice;

/// How a layer comes in when its click arrives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Enter {
    #[default]
    Appear,
    Fade,
    FlyUp,
}

/// How a slide is played on arriving at it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Transition {
    #[default]
    None,
    Fade,
    Push,
}

/// When one layer comes in. `None` means it is there from the start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerTiming {
    pub click: Option<u32>,
    pub enter: Enter,
}

/// A slide's timeline: how many clicks it takes, and when each of its layers, in the order
/// of `Deck::layers`, comes in.
#[derive(Clone, Debug, PartialEq)]
pub struct SlideTimeline {
    pub steps: u32,
    pub layers: Vec<LayerTiming>,
    pub transition: Transition,
}

/// The most clicks a slide can ask for.
pub(crate) const MAX_CLICK: u32 = 999;

/// What a `region.<name>` line says about when its content appears.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum RegionClick {
    #[default]
    Absent,
    Step(u32),
    Each,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RegionTiming {
    pub click: RegionClick,
    pub enter: Enter,
}

/// A whole number from 1 to 999.
fn read_click(value: &str) -> Option<u32> {
    let value = value.trim();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse::<u32>().ok().filter(|click| (1..=MAX_CLICK).contains(click))
}

fn read_enter(value: &str) -> Option<Enter> {
    match value.trim() {
        "appear" => Some(Enter::Appear),
        "fade" => Some(Enter::Fade),
        "fly-up" => Some(Enter::FlyUp),
        _ => None,
    }
}

/// The click a tag asks for, and how it comes in.
pub(crate) fn read_tag_timing(click: Option<&str>, enter: Option<&str>, notices: &mut Vec<Notice>) -> (Option<u32>, Enter) {
    let click = click.and_then(|value| {
        let click = read_click(value);
        if click.is_none() {
            say(notices, format!("click=\"{value}\" isn't a whole number from 1 to {MAX_CLICK}, so it is ignored"));
        }
        click
    });
    let enter = enter.and_then(|value| {
        let enter = read_enter(value);
        if enter.is_none() {
            say(notices, format!("enter=\"{value}\" isn't appear, fade or fly-up, so it is ignored"));
        }
        enter
    });
    (click, enter.unwrap_or_default())
}

/// What the `click` and `enter` of a `region.<name>` line say.
pub(crate) fn read_region_timing(line: &str, notices: &mut Vec<Notice>) -> RegionTiming {
    let mut timing = RegionTiming::default();
    for pair in line.split_whitespace() {
        match pair.split_once('=') {
            Some(("click", "each")) => timing.click = RegionClick::Each,
            Some(("click", value)) => match read_click(value) {
                Some(click) => timing.click = RegionClick::Step(click),
                None => say(notices, format!("click={value} isn't each or a whole number from 1 to {MAX_CLICK}, so it is ignored")),
            },
            Some(("enter", value)) => match read_enter(value) {
                Some(enter) => timing.enter = enter,
                None => say(notices, format!("enter={value} isn't appear, fade or fly-up, so it is ignored")),
            },
            _ => {}
        }
    }
    timing
}

pub(crate) fn read_transition(value: Option<&str>, notices: &mut Vec<Notice>) -> Transition {
    match value.map(str::trim) {
        None | Some("") | Some("none") => Transition::None,
        Some("fade") => Transition::Fade,
        Some("push") => Transition::Push,
        Some(other) => {
            say(notices, format!("transition: {other} isn't none, fade or push, so this slide has none"));
            Transition::None
        }
    }
}

fn say(notices: &mut Vec<Notice>, text: String) {
    let notice = Notice(text);
    if !notices.contains(&notice) {
        notices.push(notice);
    }
}
