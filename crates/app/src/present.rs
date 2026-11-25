//! Where a show is, and how its layers and slides look at each moment.

use slides_engine::{Enter, LayerTiming, Transition};

/// How long a layer takes to come in.
pub const ENTER_SECONDS: f32 = 0.4;

/// How long a slide takes to arrive.
pub const TRANSITION_SECONDS: f32 = 0.5;

/// How far below its place a layer flying up starts, in slide units.
pub const FLY_UNITS: f32 = 80.0;

/// How many times a second a slide that moves is drawn anew.
pub const MOMENTS_PER_SECOND: f32 = 30.0;

/// `seconds` into a slide, held to the moment before it, so that everything asking about the
/// same time asks about the same moment.
pub fn moment_time(seconds: f32) -> f32 {
    (seconds.max(0.0) * MOMENTS_PER_SECOND).floor() / MOMENTS_PER_SECOND
}

/// How far along an animation is, `t` of the way through its time: quick to start, gentle
/// to land.
pub fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// What a show needs to know about a talk, asked for only as it needs it, as finding out about a
/// slide can mean laying it out.
pub trait TalkShape {
    fn slide_count(&self) -> usize;
    /// How many clicks `slide` takes.
    fn steps(&mut self, slide: usize) -> u32;
    /// How `slide` is arrived at.
    fn transition(&mut self, slide: usize) -> Transition;
}

/// A moment of a talk: a slide, and how many of its clicks have come.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Place {
    pub slide: usize,
    pub step: u32,
}

/// What a move through the show did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Moved {
    Revealed { step: u32 },
    Arrived { from: usize, transition: Transition },
    BackedUp,
    Stayed,
}

/// Where a show is. What the talk is like is asked of it each time, and only about the slides
/// around the show.
#[derive(Clone, Copy, Debug, Default)]
pub struct Session {
    place: Place,
}

impl Session {
    /// A show at the start of a talk.
    pub fn new() -> Session {
        Session::default()
    }

    /// Goes forward by a click, or to the next slide once this one's clicks have all come.
    // Not an iterator: at the end of a show a press is still answered, with `Moved::Stayed`.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self, talk: &mut impl TalkShape) -> Moved {
        let Some(upcoming) = self.upcoming(talk) else {
            return Moved::Stayed;
        };
        let from = std::mem::replace(&mut self.place, upcoming).slide;
        if upcoming.slide == from {
            Moved::Revealed {
                step: upcoming.step,
            }
        } else {
            Moved::Arrived {
                from,
                transition: talk.transition(upcoming.slide),
            }
        }
    }

    /// Goes back by a click, or to the end of the slide before.
    pub fn previous(&mut self, talk: &mut impl TalkShape) -> Moved {
        let Place { slide, step } = self.place;
        if step > 0 {
            self.place.step = step - 1;
        } else if slide > 0 {
            self.place = Place {
                slide: slide - 1,
                step: talk.steps(slide - 1),
            };
        } else {
            return Moved::Stayed;
        }
        Moved::BackedUp
    }

    /// Goes straight to click step `step` of the slide the show is on, or to its last if it has
    /// fewer. Going to an earlier one is going back.
    pub fn go_to_step(&mut self, talk: &mut impl TalkShape, step: u32) -> Moved {
        if self.place.slide >= talk.slide_count() {
            return Moved::Stayed;
        }
        let step = step.min(talk.steps(self.place.slide));
        let was = std::mem::replace(&mut self.place.step, step);
        match step.cmp(&was) {
            std::cmp::Ordering::Greater => Moved::Revealed { step },
            std::cmp::Ordering::Less => Moved::BackedUp,
            std::cmp::Ordering::Equal => Moved::Stayed,
        }
    }

    pub fn place(&self) -> Place {
        self.place
    }

    /// Where [`Self::next`] would go. `None` at the end of the talk.
    pub fn upcoming(&self, talk: &mut impl TalkShape) -> Option<Place> {
        let Place { slide, step } = self.place;
        let count = talk.slide_count();
        if slide >= count {
            return None;
        }
        if step < talk.steps(slide) {
            Some(Place {
                slide,
                step: step + 1,
            })
        } else if slide + 1 < count {
            Some(Place {
                slide: slide + 1,
                step: 0,
            })
        } else {
            None
        }
    }

    /// Takes in a talk that has changed, staying on a place that still exists.
    pub fn refit(&mut self, talk: &mut impl TalkShape) {
        let Some(last) = talk.slide_count().checked_sub(1) else {
            self.place = Place::default();
            return;
        };
        if self.place.slide > last {
            self.place = Place {
                slide: last,
                step: talk.steps(last),
            };
        }
        self.place.step = self.place.step.min(talk.steps(self.place.slide));
    }
}

/// How one layer looks at a moment of the show.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerLook {
    pub visible: bool,
    pub opacity: f32,
    /// How far below its place it is, in slide units.
    pub offset_y: f32,
}

impl Default for LayerLook {
    /// Fully visible and in its place.
    fn default() -> Self {
        LayerLook {
            visible: true,
            opacity: 1.0,
            offset_y: 0.0,
        }
    }
}

/// How a layer looks with `step` clicks come. `revealing` is the step being revealed, and
/// the seconds since it began.
pub fn layer_look(timing: LayerTiming, step: u32, revealing: Option<(u32, f32)>) -> LayerLook {
    let at_rest = LayerLook::default();
    if timing.click.is_some_and(|click| click > step) {
        return LayerLook {
            visible: false,
            opacity: 0.0,
            ..at_rest
        };
    }
    let entering = revealing.filter(|(revealed, _)| timing.click == Some(*revealed));
    let Some((_, seconds)) = entering else {
        return at_rest;
    };
    let progress = ease(seconds / ENTER_SECONDS);
    match timing.enter {
        Enter::Appear => at_rest,
        Enter::Fade => LayerLook {
            opacity: progress,
            ..at_rest
        },
        Enter::FlyUp => LayerLook {
            offset_y: FLY_UNITS * (1.0 - progress),
            ..at_rest
        },
    }
}

/// How a whole slide looks at a moment of the show.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlideLook {
    pub opacity: f32,
    /// How far to the right of its place it is, as a fraction of the slide's width.
    pub offset_x: f32,
}

impl Default for SlideLook {
    /// Fully visible and in its place.
    fn default() -> Self {
        SlideLook {
            opacity: 1.0,
            offset_x: 0.0,
        }
    }
}

/// How the slide being left and the slide arrived at look, in that order, `seconds` into a
/// transition.
pub fn slide_looks(transition: Transition, seconds: f32) -> (SlideLook, SlideLook) {
    let at_rest = SlideLook::default();
    let progress = ease(seconds / TRANSITION_SECONDS);
    match transition {
        Transition::None => (
            SlideLook {
                opacity: 0.0,
                ..at_rest
            },
            at_rest,
        ),
        Transition::Fade => (
            at_rest,
            SlideLook {
                opacity: progress,
                ..at_rest
            },
        ),
        Transition::Push => (
            SlideLook {
                offset_x: -progress,
                ..at_rest
            },
            SlideLook {
                offset_x: 1.0 - progress,
                ..at_rest
            },
        ),
    }
}

/// A screen a show can use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenInfo {
    pub id: u32,
    pub primary: bool,
}

/// Which windows a show opens, and on which screens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WindowPlan {
    Both { audience: u32, presenter: u32 },
    AudienceOnly { screen: Option<u32> },
    Rehearsal,
}

/// Where a show's windows go. The presenter stays on the primary screen, and the audience
/// gets another. With only one screen the audience has it to themselves.
pub fn choose_windows(screens: &[ScreenInfo], rehearsing: bool) -> WindowPlan {
    if rehearsing {
        return WindowPlan::Rehearsal;
    }
    let primary = screens.iter().find(|screen| screen.primary);
    let Some(presenter) = primary.or(screens.first()) else {
        return WindowPlan::AudienceOnly { screen: None };
    };
    match screens.iter().find(|screen| screen.id != presenter.id) {
        Some(audience) => WindowPlan::Both {
            audience: audience.id,
            presenter: presenter.id,
        },
        None => WindowPlan::AudienceOnly {
            screen: Some(presenter.id),
        },
    }
}

/// How long a show has been running, as `mm:ss`. The minutes go on past 59.
pub fn clock_text(elapsed_seconds: u64) -> String {
    format!("{:02}:{:02}", elapsed_seconds / 60, elapsed_seconds % 60)
}
