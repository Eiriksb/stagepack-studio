use slides_engine::{Enter, LayerTiming, Transition};
use stagepack_studio::present::{
    FLY_UNITS, LayerLook, Moved, Place, ScreenInfo, Session, SlideLook, TalkShape, WindowPlan,
    choose_windows, clock_text, ease, layer_look, slide_looks,
};

/// A talk with so many clicks on each slide, which remembers the slides it was asked about.
struct Shape {
    steps: Vec<u32>,
    transitions: Vec<Transition>,
    asked: Vec<usize>,
}

impl Shape {
    fn new(steps: Vec<u32>, transitions: Vec<Transition>) -> Shape {
        Shape {
            steps,
            transitions,
            asked: Vec::new(),
        }
    }
}

impl TalkShape for Shape {
    fn slide_count(&self) -> usize {
        self.steps.len()
    }

    // By index, as there's no answer for a slide that isn't there.
    fn steps(&mut self, slide: usize) -> u32 {
        self.asked.push(slide);
        self.steps[slide]
    }

    fn transition(&mut self, slide: usize) -> Transition {
        self.asked.push(slide);
        self.transitions.get(slide).copied().unwrap_or_default()
    }
}

fn talk() -> Shape {
    Shape::new(
        vec![2, 0, 1],
        vec![Transition::None, Transition::Fade, Transition::Push],
    )
}

#[test]
fn next_reveals_each_step_then_moves_on() {
    let (mut s, mut t) = (Session::new(), talk());
    assert_eq!(s.next(&mut t), Moved::Revealed { step: 1 });
    assert_eq!(s.next(&mut t), Moved::Revealed { step: 2 });
    assert_eq!(
        s.next(&mut t),
        Moved::Arrived {
            from: 0,
            transition: Transition::Fade
        }
    );
    assert_eq!(s.place(), Place { slide: 1, step: 0 });
}

#[test]
fn next_stops_at_the_end() {
    let (mut s, mut t) = (Session::new(), Shape::new(vec![0], vec![Transition::None]));
    assert_eq!(s.next(&mut t), Moved::Stayed);
}

#[test]
fn previous_steps_back_with_no_animation() {
    let (mut s, mut t) = (Session::new(), talk());
    s.next(&mut t);
    assert_eq!(s.previous(&mut t), Moved::BackedUp);
    assert_eq!(s.place(), Place { slide: 0, step: 0 });
    assert_eq!(s.previous(&mut t), Moved::Stayed);
}

#[test]
fn going_back_a_slide_lands_on_its_last_step() {
    let (mut s, mut t) = (Session::new(), talk());
    s.next(&mut t);
    s.next(&mut t);
    s.next(&mut t);
    assert_eq!(s.previous(&mut t), Moved::BackedUp);
    assert_eq!(s.place(), Place { slide: 0, step: 2 });
}

#[test]
fn upcoming_is_the_next_step_or_slide() {
    let (mut s, mut t) = (Session::new(), talk());
    assert_eq!(s.upcoming(&mut t), Some(Place { slide: 0, step: 1 }));
    s.next(&mut t);
    s.next(&mut t);
    assert_eq!(s.upcoming(&mut t), Some(Place { slide: 1, step: 0 }));
    s.next(&mut t);
    s.next(&mut t);
    s.next(&mut t);
    assert_eq!(s.upcoming(&mut t), None);
}

#[test]
fn refit_keeps_the_show_on_a_real_place() {
    let (mut s, mut t) = (Session::new(), talk());
    s.next(&mut t);
    s.next(&mut t);
    s.next(&mut t);
    s.next(&mut t);
    s.refit(&mut Shape::new(vec![1], vec![Transition::None]));
    assert_eq!(s.place(), Place { slide: 0, step: 1 });
}

#[test]
fn refit_keeps_a_place_that_still_exists() {
    let (mut s, mut t) = (Session::new(), talk());
    s.next(&mut t);
    s.refit(&mut Shape::new(vec![5, 5], vec![]));
    assert_eq!(s.place(), Place { slide: 0, step: 1 });
}

#[test]
fn refit_takes_back_a_step_the_slide_no_longer_has() {
    let (mut s, mut t) = (Session::new(), talk());
    s.next(&mut t);
    s.next(&mut t);
    s.refit(&mut Shape::new(vec![1, 0, 1], vec![]));
    assert_eq!(s.place(), Place { slide: 0, step: 1 });
}

#[test]
fn a_slide_with_no_transition_listed_has_none() {
    let (mut s, mut t) = (Session::new(), Shape::new(vec![0, 0], vec![]));
    assert_eq!(
        s.next(&mut t),
        Moved::Arrived {
            from: 0,
            transition: Transition::None
        }
    );
}

#[test]
fn a_show_with_no_slides_goes_nowhere() {
    let (mut s, mut t) = (Session::new(), Shape::new(vec![], vec![]));
    assert_eq!(s.next(&mut t), Moved::Stayed);
    assert_eq!(s.previous(&mut t), Moved::Stayed);
    assert_eq!(s.upcoming(&mut t), None);
    assert_eq!(s.place(), Place { slide: 0, step: 0 });

    let (mut s, mut t) = (Session::new(), talk());
    s.next(&mut t);
    let mut t = Shape::new(vec![], vec![]);
    s.refit(&mut t);
    assert_eq!(s.place(), Place { slide: 0, step: 0 });
    assert_eq!(s.next(&mut t), Moved::Stayed);
}

#[test]
fn ease_starts_fast_and_stays_between_nought_and_one() {
    assert_eq!(ease(0.0), 0.0);
    assert_eq!(ease(1.0), 1.0);
    assert_eq!(ease(0.5), 0.875);
    assert_eq!(ease(-3.0), 0.0);
    assert_eq!(ease(3.0), 1.0);
}

#[test]
fn a_long_show_only_asks_about_the_slides_around_it() {
    let (mut s, mut t) = (Session::new(), Shape::new(vec![1; 1000], vec![]));
    assert!(t.asked.is_empty(), "starting a show asks about nothing");

    s.next(&mut t);
    assert_eq!(s.place(), Place { slide: 0, step: 1 });
    s.next(&mut t);
    assert_eq!(s.place(), Place { slide: 1, step: 0 });
    s.upcoming(&mut t);
    assert!(t.asked.iter().all(|slide| *slide <= 2), "{:?}", t.asked);

    t.asked.clear();
    for _ in 0..40 {
        s.next(&mut t);
    }
    let here = s.place().slide;
    assert_eq!(here, 21);
    assert!(
        t.asked.iter().all(|slide| *slide >= 1 && *slide <= here + 1),
        "{:?}",
        t.asked
    );

    t.asked.clear();
    s.previous(&mut t);
    s.previous(&mut t);
    assert!(
        t.asked.iter().all(|slide| slide.abs_diff(here) <= 1),
        "{:?}",
        t.asked
    );
}

#[test]
fn a_save_during_a_long_show_leaves_it_on_a_real_slide() {
    let (mut s, mut t) = (Session::new(), Shape::new(vec![0; 1000], vec![]));
    for _ in 0..500 {
        s.next(&mut t);
    }
    assert_eq!(s.place(), Place { slide: 500, step: 0 });

    let mut shorter = Shape::new(vec![0, 0, 3], vec![]);
    s.refit(&mut shorter);
    assert_eq!(s.place(), Place { slide: 2, step: 3 });
    assert_eq!(s.next(&mut shorter), Moved::Stayed);

    let mut same_length = Shape::new(vec![1; 1000], vec![]);
    let mut there = Session::new();
    for _ in 0..3 {
        there.next(&mut same_length);
    }
    same_length.asked.clear();
    there.refit(&mut same_length);
    assert!(same_length.asked.iter().all(|slide| *slide == 1), "{:?}", same_length.asked);
}

const LATER: fn(Enter) -> LayerTiming = |enter| LayerTiming {
    click: Some(2),
    enter,
};

#[test]
fn a_layer_waits_for_its_click() {
    assert!(!layer_look(LATER(Enter::Fade), 1, None).visible);
    assert_eq!(
        layer_look(LATER(Enter::Fade), 2, None),
        LayerLook {
            visible: true,
            opacity: 1.0,
            offset_y: 0.0
        }
    );
}

#[test]
fn a_layer_with_no_click_is_there_from_the_start() {
    let timing = LayerTiming {
        click: None,
        enter: Enter::Fade,
    };
    assert_eq!(
        layer_look(timing, 0, Some((0, 0.0))),
        LayerLook {
            visible: true,
            opacity: 1.0,
            offset_y: 0.0
        }
    );
}

#[test]
fn a_fade_rises_with_time() {
    let look = layer_look(LATER(Enter::Fade), 2, Some((2, 0.2)));
    assert!((look.opacity - ease(0.5)).abs() < 1e-4);
}

#[test]
fn fly_up_starts_below_and_lands() {
    assert_eq!(
        layer_look(LATER(Enter::FlyUp), 2, Some((2, 0.0))).offset_y,
        FLY_UNITS
    );
    assert_eq!(
        layer_look(LATER(Enter::FlyUp), 2, Some((2, 0.4))).offset_y,
        0.0
    );
}

#[test]
fn fly_up_moves_without_fading() {
    assert_eq!(
        layer_look(LATER(Enter::FlyUp), 2, Some((2, 0.0))).opacity,
        1.0
    );
}

#[test]
fn appear_is_instant() {
    assert_eq!(
        layer_look(LATER(Enter::Appear), 2, Some((2, 0.0))).opacity,
        1.0
    );
}

#[test]
fn only_the_layers_of_the_step_being_revealed_animate() {
    let earlier = LayerTiming {
        click: Some(1),
        enter: Enter::Fade,
    };
    assert_eq!(layer_look(earlier, 2, Some((2, 0.0))).opacity, 1.0);
}

#[test]
fn a_layer_rests_once_its_entrance_is_over() {
    assert_eq!(
        layer_look(LATER(Enter::Fade), 2, Some((2, 100.0))),
        LayerLook {
            visible: true,
            opacity: 1.0,
            offset_y: 0.0
        }
    );
}

#[test]
fn a_fade_transition_brings_the_new_slide_over_the_old() {
    let (out, inc) = slide_looks(Transition::Fade, 0.25);
    assert_eq!(
        out,
        SlideLook {
            opacity: 1.0,
            offset_x: 0.0
        }
    );
    assert!((inc.opacity - ease(0.5)).abs() < 1e-4);
}

#[test]
fn a_push_slides_both_to_the_left() {
    let (out, inc) = slide_looks(Transition::Push, 0.5);
    assert_eq!((out.offset_x, inc.offset_x), (-1.0, 0.0));
    let (out, inc) = slide_looks(Transition::Push, 0.0);
    assert_eq!((out.offset_x, inc.offset_x), (0.0, 1.0));
}

#[test]
fn a_push_keeps_both_slides_opaque() {
    let (out, inc) = slide_looks(Transition::Push, 0.1);
    assert_eq!((out.opacity, inc.opacity), (1.0, 1.0));
}

#[test]
fn no_transition_is_immediate() {
    assert_eq!(
        slide_looks(Transition::None, 0.0).1,
        SlideLook {
            opacity: 1.0,
            offset_x: 0.0
        }
    );
}

#[test]
fn with_no_transition_nothing_of_the_old_slide_shows() {
    assert_eq!(slide_looks(Transition::None, 0.0).0.opacity, 0.0);
}

#[test]
fn one_screen_gives_one_full_screen_window() {
    assert_eq!(
        choose_windows(
            &[ScreenInfo {
                id: 1,
                primary: true
            }],
            false
        ),
        WindowPlan::AudienceOnly { screen: Some(1) }
    );
}

#[test]
fn two_screens_put_the_audience_on_the_other_one() {
    let screens = [
        ScreenInfo {
            id: 1,
            primary: true,
        },
        ScreenInfo {
            id: 2,
            primary: false,
        },
    ];
    assert_eq!(
        choose_windows(&screens, false),
        WindowPlan::Both {
            audience: 2,
            presenter: 1
        }
    );
}

#[test]
fn the_presenter_stays_on_the_primary_screen_wherever_it_is_listed() {
    let screens = [
        ScreenInfo {
            id: 7,
            primary: false,
        },
        ScreenInfo {
            id: 3,
            primary: true,
        },
    ];
    assert_eq!(
        choose_windows(&screens, false),
        WindowPlan::Both {
            audience: 7,
            presenter: 3
        }
    );
}

#[test]
fn with_no_screen_marked_primary_the_first_one_counts_as_it() {
    let screens = [
        ScreenInfo {
            id: 4,
            primary: false,
        },
        ScreenInfo {
            id: 5,
            primary: false,
        },
    ];
    assert_eq!(
        choose_windows(&screens, false),
        WindowPlan::Both {
            audience: 5,
            presenter: 4
        }
    );
    assert_eq!(
        choose_windows(&screens[..1], false),
        WindowPlan::AudienceOnly { screen: Some(4) }
    );
}

#[test]
fn no_display_information_still_opens_a_window() {
    assert_eq!(
        choose_windows(&[], false),
        WindowPlan::AudienceOnly { screen: None }
    );
}

#[test]
fn rehearsing_opens_ordinary_windows() {
    assert_eq!(
        choose_windows(
            &[ScreenInfo {
                id: 1,
                primary: true
            }],
            true
        ),
        WindowPlan::Rehearsal
    );
    assert_eq!(choose_windows(&[], true), WindowPlan::Rehearsal);
}

#[test]
fn the_clock_reads_minutes_and_seconds() {
    assert_eq!(clock_text(0), "00:00");
    assert_eq!(clock_text(65), "01:05");
    assert_eq!(clock_text(3725), "62:05");
}
