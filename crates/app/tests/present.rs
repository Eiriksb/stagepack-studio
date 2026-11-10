use slides_engine::{Enter, LayerTiming, Transition};
use stagepack_studio::present::{
    FLY_UNITS, LayerLook, Moved, Place, Session, SlideLook, ease, layer_look, slide_looks,
};

fn session() -> Session {
    Session::new(
        vec![2, 0, 1],
        vec![Transition::None, Transition::Fade, Transition::Push],
    )
}

#[test]
fn next_reveals_each_step_then_moves_on() {
    let mut s = session();
    assert_eq!(s.next(), Moved::Revealed { step: 1 });
    assert_eq!(s.next(), Moved::Revealed { step: 2 });
    assert_eq!(
        s.next(),
        Moved::Arrived {
            from: 0,
            transition: Transition::Fade
        }
    );
    assert_eq!(s.place(), Place { slide: 1, step: 0 });
}

#[test]
fn next_stops_at_the_end() {
    let mut s = Session::new(vec![0], vec![Transition::None]);
    assert_eq!(s.next(), Moved::Stayed);
}

#[test]
fn previous_steps_back_with_no_animation() {
    let mut s = session();
    s.next();
    assert_eq!(s.previous(), Moved::BackedUp);
    assert_eq!(s.place(), Place { slide: 0, step: 0 });
    assert_eq!(s.previous(), Moved::Stayed);
}

#[test]
fn going_back_a_slide_lands_on_its_last_step() {
    let mut s = session();
    s.next();
    s.next();
    s.next();
    assert_eq!(s.previous(), Moved::BackedUp);
    assert_eq!(s.place(), Place { slide: 0, step: 2 });
}

#[test]
fn upcoming_is_the_next_step_or_slide() {
    let mut s = session();
    assert_eq!(s.upcoming(), Some(Place { slide: 0, step: 1 }));
    s.next();
    s.next();
    assert_eq!(s.upcoming(), Some(Place { slide: 1, step: 0 }));
    s.next();
    s.next();
    s.next();
    assert_eq!(s.upcoming(), None);
}

#[test]
fn refit_keeps_the_show_on_a_real_place() {
    let mut s = session();
    s.next();
    s.next();
    s.next();
    s.next();
    s.refit(vec![1], vec![Transition::None]);
    assert_eq!(s.place(), Place { slide: 0, step: 1 });
}

#[test]
fn refit_keeps_a_place_that_still_exists() {
    let mut s = session();
    s.next();
    s.refit(vec![5, 5], vec![]);
    assert_eq!(s.place(), Place { slide: 0, step: 1 });
}

#[test]
fn refit_takes_back_a_step_the_slide_no_longer_has() {
    let mut s = session();
    s.next();
    s.next();
    s.refit(vec![1, 0, 1], vec![]);
    assert_eq!(s.place(), Place { slide: 0, step: 1 });
}

#[test]
fn a_slide_with_no_transition_listed_has_none() {
    let mut s = Session::new(vec![0, 0], vec![]);
    assert_eq!(
        s.next(),
        Moved::Arrived {
            from: 0,
            transition: Transition::None
        }
    );
}

#[test]
fn a_show_with_no_slides_goes_nowhere() {
    let mut s = Session::new(vec![], vec![]);
    assert_eq!(s.next(), Moved::Stayed);
    assert_eq!(s.previous(), Moved::Stayed);
    assert_eq!(s.upcoming(), None);
    assert_eq!(s.place(), Place { slide: 0, step: 0 });

    let mut s = session();
    s.next();
    s.refit(vec![], vec![]);
    assert_eq!(s.place(), Place { slide: 0, step: 0 });
    assert_eq!(s.next(), Moved::Stayed);
}

#[test]
fn ease_starts_fast_and_stays_between_nought_and_one() {
    assert_eq!(ease(0.0), 0.0);
    assert_eq!(ease(1.0), 1.0);
    assert_eq!(ease(0.5), 0.875);
    assert_eq!(ease(-3.0), 0.0);
    assert_eq!(ease(3.0), 1.0);
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
