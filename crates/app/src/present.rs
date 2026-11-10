//! Where a show is, and how its layers and slides look at each moment.

use slides_engine::{Enter, LayerTiming, Transition};

/// How long a layer takes to come in.
pub const ENTER_SECONDS: f32 = 0.4;

/// How long a slide takes to arrive.
pub const TRANSITION_SECONDS: f32 = 0.5;

/// How far below its place a layer flying up starts, in slide units.
pub const FLY_UNITS: f32 = 80.0;

/// How far along an animation is, `t` of the way through its time: quick to start, gentle
/// to land.
pub fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// A moment of a talk: a slide, and how many of its clicks have come.
#[derive(Clone, Copy, Debug, PartialEq)]
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

/// Where a show is, in a talk with so many clicks on each slide.
pub struct Session {
    steps: Vec<u32>,
    transitions: Vec<Transition>,
    place: Place,
}

impl Session {
    /// A show at the start of a talk. `steps` holds each slide's clicks, and `transitions`
    /// how each slide is arrived at.
    pub fn new(steps: Vec<u32>, transitions: Vec<Transition>) -> Session {
        Session {
            steps,
            transitions,
            place: Place { slide: 0, step: 0 },
        }
    }

    /// Goes forward by a click, or to the next slide once this one's clicks have all come.
    // Not an iterator: at the end of a show a press is still answered, with `Moved::Stayed`.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Moved {
        let Some(upcoming) = self.upcoming() else {
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
                transition: self
                    .transitions
                    .get(upcoming.slide)
                    .copied()
                    .unwrap_or_default(),
            }
        }
    }

    /// Goes back by a click, or to the end of the slide before.
    pub fn previous(&mut self) -> Moved {
        let Place { slide, step } = self.place;
        if step > 0 {
            self.place.step = step - 1;
        } else if slide > 0 {
            self.place = Place {
                slide: slide - 1,
                step: self.steps_of(slide - 1),
            };
        } else {
            return Moved::Stayed;
        }
        Moved::BackedUp
    }

    pub fn place(&self) -> Place {
        self.place
    }

    /// Where [`Self::next`] would go. `None` at the end of the talk.
    pub fn upcoming(&self) -> Option<Place> {
        let Place { slide, step } = self.place;
        if step < self.steps_of(slide) {
            Some(Place {
                slide,
                step: step + 1,
            })
        } else if slide + 1 < self.steps.len() {
            Some(Place {
                slide: slide + 1,
                step: 0,
            })
        } else {
            None
        }
    }

    /// Takes in a talk that has changed, staying on a place that still exists.
    pub fn refit(&mut self, steps: Vec<u32>, transitions: Vec<Transition>) {
        self.steps = steps;
        self.transitions = transitions;
        let last = self.steps.len().saturating_sub(1);
        if self.place.slide > last {
            self.place = Place {
                slide: last,
                step: self.steps_of(last),
            };
        }
        self.place.step = self.place.step.min(self.steps_of(self.place.slide));
    }

    fn steps_of(&self, slide: usize) -> u32 {
        self.steps.get(slide).copied().unwrap_or(0)
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
