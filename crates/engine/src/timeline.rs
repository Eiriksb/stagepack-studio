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
    /// The most clicks any of its layers or components asks for.
    pub steps: u32,
    pub layers: Vec<LayerTiming>,
    pub transition: Transition,
    /// Whether a component on it changes over time, and not only from click to click.
    pub animated: bool,
}

/// A point in the playing of a slide: how many clicks it has had, and the seconds since it appeared.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moment {
    pub step: u32,
    pub time: f32,
}

impl Moment {
    /// The slide as it is on arriving at it.
    pub const START: Moment = Moment { step: 0, time: 0.0 };
}

/// What the components of a slide, or of one layer, looked at to draw it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComponentAsks {
    /// The most clicks a component asked for.
    pub steps: u32,
    /// Whether one can differ from one click to the next.
    pub stepped: bool,
    /// Whether one can differ from one moment to the next.
    pub animated: bool,
    /// The components that can differ, once each, in the order they came.
    pub tags: Vec<String>,
}

impl ComponentAsks {
    /// Adds what component `tag` looked at.
    pub(crate) fn note(&mut self, tag: &str, steps: u32, stepped: bool, animated: bool) {
        self.steps = self.steps.max(steps);
        self.stepped |= stepped;
        self.animated |= animated;
        if (stepped || animated) && !self.tags.iter().any(|known| known == tag) {
            self.tags.push(tag.to_string());
        }
    }

    /// Adds what the components of another part of the slide looked at.
    pub(crate) fn add(&mut self, other: ComponentAsks) {
        self.steps = self.steps.max(other.steps);
        self.stepped |= other.stepped;
        self.animated |= other.animated;
        for tag in other.tags {
            if !self.tags.contains(&tag) {
                self.tags.push(tag);
            }
        }
    }

    /// Whether what they drew can change with the moment.
    pub(crate) fn varies(&self) -> bool {
        self.stepped || self.animated
    }
}

/// The most clicks a slide can ask for.
pub const MAX_CLICK: u32 = 999;

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

/// The click `value`, as a talk writes it, reads as: a whole number from 1 to 999, or none.
pub fn read_click(value: &str) -> Option<u32> {
    let value = value.trim();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse::<u32>().ok().filter(|click| (1..=MAX_CLICK).contains(click))
}

/// How `value`, as a talk writes it, says a layer comes in, if it's one of the ways.
pub fn read_enter(value: &str) -> Option<Enter> {
    match value.trim() {
        "appear" => Some(Enter::Appear),
        "fade" => Some(Enter::Fade),
        "fly-up" => Some(Enter::FlyUp),
        _ => None,
    }
}

/// How `enter` is written, as [`read_enter`] reads it.
pub fn enter_word(enter: Enter) -> &'static str {
    match enter {
        Enter::Appear => "appear",
        Enter::Fade => "fade",
        Enter::FlyUp => "fly-up",
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
