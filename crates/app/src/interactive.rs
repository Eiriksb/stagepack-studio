//! The things on a slide in the show that a press is for, rather than the show: a press, a drag or
//! the wheel on one goes to it, and the show doesn't move. Both of a show's windows find them the
//! same way.

use std::ops::Range;

use gpui_kit::Point;
use slides_engine::{BUTTON_ON_NOTHING, BUTTON_STEP_PAST, ButtonAction, ButtonOn, Moment, Rect};

use crate::document::Document;
use crate::live_terminal::{LiveId, untypable};
use crate::model_view::inside_turned;

/// What an interactive thing is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum InteractiveKind {
    /// A button, which acts when it's pressed and let go of over it.
    Button,
    /// A terminal that runs a command: it takes the keys while it runs.
    Terminal(LiveId),
    /// A model, by the place of its layer among the slide's: a drag turns it, the wheel zooms it.
    Model(usize),
    /// A plain box the tests put on a slide, which says what reached it.
    #[cfg(feature = "test-hooks")]
    StandIn,
}

/// An interactive thing on a slide, in slide units: where its tag is written, where it is, turned by
/// `rotate` degrees, what it is, and the `id` its tag gives, by which a button names it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InteractiveThing {
    pub source: Range<usize>,
    pub rect: Rect,
    pub rotate: f32,
    pub kind: InteractiveKind,
    pub id: Option<String>,
}

/// The interactive thing at `at`, in slide units, among `things`, which are in the order they're
/// drawn: the one in front.
pub(crate) fn interactive_at(things: &[InteractiveThing], at: Point<f32>) -> Option<&InteractiveThing> {
    things.iter().rev().find(|thing| inside_turned(thing.rect, thing.rotate, (at.x, at.y)))
}

/// The interactive things of the talk's slide `slide` at `moment`, in the order they're drawn: only
/// those shown by then. A terminal is among them whether or not its command runs yet.
pub(crate) fn interactive_on(document: &mut Document, slide: usize, moment: Moment) -> Vec<InteractiveThing> {
    let shown = |click: Option<u32>| click.is_none_or(|click| click <= moment.step);
    let terminals = document.terminals_at(slide, moment).into_iter().filter(|terminal| shown(terminal.click)).filter_map(|terminal| {
        let source = terminal.source?;
        let live = LiveId::new(source.clone(), terminal.on.run?);
        Some((terminal.layer, InteractiveThing { source, rect: terminal.on.rect, rotate: terminal.on.rotate, kind: InteractiveKind::Terminal(live), id: terminal.id }))
    });
    let models = document.models_at(slide, moment).into_iter().filter(|model| shown(model.click)).filter_map(|model| {
        let source = model.source?;
        Some((model.layer, InteractiveThing { source, rect: model.on.rect, rotate: model.on.rotate, kind: InteractiveKind::Model(model.layer), id: model.id }))
    });
    let buttons = document.buttons_at(slide, moment).into_iter().filter(|button| shown(button.click)).map(|button| {
        let on = button.on;
        (button.layer, InteractiveThing { source: button.source, rect: on.rect, rotate: on.rotate, kind: InteractiveKind::Button, id: on.id })
    });
    let mut things: Vec<(usize, InteractiveThing)> = terminals.chain(models).chain(buttons).collect();
    things.sort_by_key(|(layer, _)| *layer);
    things.into_iter().map(|(_, thing)| thing).collect()
}

/// What's said of a button whose `run` text is empty.
pub const BUTTON_TEXT_EMPTY: &str = "This button's text is empty.";

/// What pressing a button is to do, from what it says and the clicks its slide takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Press {
    /// Nothing: it has no action, or more than one, and its notice says so.
    Nothing,
    /// Go to this click step of the slide.
    Step(u32),
    /// Type `text`, then Enter, into the live terminal on the slide whose `id` is `terminal`.
    Run { text: String, terminal: String },
    /// Nothing, for this, which the status line says.
    Refused(String),
}

/// What came of acting on a button's press.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Acted {
    /// It did what it does, or there was nothing for it to do.
    Done,
    /// It waits for a yes, which is being asked.
    Asked,
    /// It did nothing, for this, the one line the status line shows.
    Refused(String),
}

/// What pressing `button` is to do, on a slide that takes `steps` clicks. A text that can't be typed
/// is refused here, before anything is asked.
pub(crate) fn press_button(button: &ButtonOn, steps: u32) -> Press {
    let Ok(action) = &button.action else { return Press::Nothing };
    match action {
        ButtonAction::Step(step) if *step > steps => Press::Refused(BUTTON_STEP_PAST.to_string()),
        ButtonAction::Step(step) => Press::Step(*step),
        // What a button acts on comes with plan H1 task 5.
        ButtonAction::Do { .. } => Press::Refused(BUTTON_ON_NOTHING.to_string()),
        ButtonAction::Run { text, .. } if text.is_empty() => Press::Refused(BUTTON_TEXT_EMPTY.to_string()),
        ButtonAction::Run { text, terminal } => match untypable(text) {
            Some(why) => Press::Refused(why.to_string()),
            None => Press::Run { text: text.clone(), terminal: terminal.clone() },
        },
    }
}

/// What reached a test's box, and in which window.
#[cfg(feature = "test-hooks")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StandInPress {
    /// A press on it.
    Down(PressedIn),
    /// The press let go of over it.
    Up(PressedIn),
}

/// Which of a show's windows a press began in: that window has it until it's let go of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressedIn {
    Audience,
    Presenter,
}
