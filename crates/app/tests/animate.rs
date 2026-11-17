//! Click steps as the Animate tab chooses them: which step each thing and area comes in on,
//! after a thing is animated, made still, or moved earlier or later.

use proptest::prelude::*;
use slides_format::Target;
use stagepack_studio::animate::{Choice, MAX_STEP, Stepped, chosen, steps_shown};

fn tag(n: usize) -> Target {
    Target::Tag(n * 10..n * 10 + 5)
}

fn area(name: &str) -> Target {
    Target::Region(name.to_string())
}

fn stepped(target: Target, click: Option<u32>) -> Stepped {
    Stepped { target, click, each: false }
}

/// `all` with the new clicks of `result` written in.
fn applied(all: &[Stepped], result: &[(Target, Option<u32>)]) -> Vec<Stepped> {
    all.iter()
        .map(|was| {
            let click = result.iter().find(|(target, _)| *target == was.target).map_or(was.click, |(_, click)| *click);
            Stepped { click, ..was.clone() }
        })
        .collect()
}

/// `all` with any step above `fixed` that nothing has closed, as a slide without gaps.
fn without_gaps(all: &[Stepped], fixed: u32) -> Vec<Stepped> {
    let mut used: Vec<u32> = all.iter().filter_map(|was| was.click).filter(|click| *click > fixed).collect();
    used.sort_unstable();
    used.dedup();
    all.iter()
        .map(|was| {
            let click = was.click.map(|click| if click > fixed { fixed + 1 + used.iter().position(|used| *used == click).unwrap() as u32 } else { click });
            Stepped { click, ..was.clone() }
        })
        .collect()
}

/// The step `click` of `all` is at once the steps above `fixed` that nothing has are closed.
fn closed(all: &[Stepped], fixed: u32, click: Option<u32>) -> Option<u32> {
    let mut used: Vec<u32> = all.iter().filter_map(|was| was.click).filter(|click| *click > fixed).collect();
    used.sort_unstable();
    used.dedup();
    click.map(|click| if click > fixed { fixed + 1 + used.iter().position(|used| *used == click).unwrap() as u32 } else { click })
}

/// Whether every step from `fixed + 1` to the slide's last has something on it.
fn no_empty_step(all: &[Stepped], fixed: u32) -> bool {
    (fixed + 1..=steps_shown(all, fixed)).all(|step| all.iter().any(|was| was.click == Some(step)))
}

fn choice() -> impl Strategy<Value = Choice> {
    prop_oneof![
        Just(Choice::Animate),
        Just(Choice::Remove),
        Just(Choice::Earlier),
        Just(Choice::Later),
        any::<bool>().prop_map(Choice::Each),
    ]
}

fn click() -> impl Strategy<Value = Option<u32>> {
    prop_oneof![
        3 => Just(None),
        6 => (1u32..=8).prop_map(Some),
        1 => (MAX_STEP - 2..=MAX_STEP).prop_map(Some),
    ]
}

/// A slide of up to twelve things and areas, any of them animated at any step, some areas shown
/// one at a time, the steps no click can renumber, and which of them are chosen.
fn slide() -> impl Strategy<Value = (Vec<Stepped>, u32, Vec<Target>)> {
    (prop::collection::vec((click(), any::<bool>(), any::<bool>(), any::<bool>()), 0..=12), 0u32..=6).prop_map(|(things, fixed)| {
        let mut all = Vec::new();
        let mut picked = Vec::new();
        for (at, (click, is_area, each, pick)) in things.into_iter().enumerate() {
            let target = if is_area { area(&format!("area{at}")) } else { tag(at) };
            // An area shown one at a time has no numbered click, and its steps are fixed ones.
            let each = is_area && each && click.is_none();
            if pick {
                picked.push(target.clone());
            }
            all.push(Stepped { target, click, each });
        }
        let fixed = if all.iter().any(|was| was.each) { fixed.max(1) } else { fixed };
        (all, fixed, picked)
    })
}

proptest! {
    #[test]
    fn no_click_shows_nothing_after_any_choice((all, fixed, picked) in slide(), choice in choice()) {
        if let Some(result) = chosen(&all, fixed, &picked, choice) {
            let after = applied(&all, &result);
            prop_assert!(no_empty_step(&after, fixed), "{choice:?} on {all:?} at {fixed} gave {after:?}");
            prop_assert!(after.iter().filter_map(|was| was.click).all(|click| (1..=MAX_STEP).contains(&click)));
            prop_assert!(!result.is_empty());
            for (target, click) in &result {
                let was = all.iter().find(|was| was.target == *target).expect("a target of the slide");
                prop_assert_ne!(was.click, *click, "only what changes is in the result");
            }
        }
    }

    #[test]
    fn what_was_not_chosen_keeps_its_order((all, fixed, picked) in slide(), choice in choice()) {
        let Some(result) = chosen(&all, fixed, &picked, choice) else { return Ok(()) };
        let after = applied(&all, &result);
        let not_chosen: Vec<usize> = (0..all.len()).filter(|at| !picked.contains(&all[*at].target)).collect();
        for &a in &not_chosen {
            prop_assert_eq!(all[a].click.is_some(), after[a].click.is_some(), "it's neither animated nor made still");
            for &b in &not_chosen {
                if let (Some(was_a), Some(was_b)) = (all[a].click, all[b].click) {
                    let (now_a, now_b) = (after[a].click.unwrap(), after[b].click.unwrap());
                    prop_assert_eq!(was_a.cmp(&was_b), now_a.cmp(&now_b), "{:?} and {:?}", all[a], all[b]);
                }
            }
        }
        // Animating renumbers the others only to close the gaps the slide has.
        if choice == Choice::Animate {
            for at in &not_chosen {
                prop_assert_eq!(after[*at].click, closed(&all, fixed, all[*at].click), "{:?}", all[*at]);
            }
        }
        // On a slide without gaps, another moves only as the chosen leave room for it.
        let all = without_gaps(&all, fixed);
        let Some(result) = chosen(&all, fixed, &picked, choice) else { return Ok(()) };
        let after = applied(&all, &result);
        // Whether only chosen things came in on `step`, and some did.
        let only_chosen = |step: u32| {
            all.iter().any(|was| was.click == Some(step)) && all.iter().all(|was| was.click != Some(step) || picked.contains(&was.target))
        };
        for &at in &not_chosen {
            let (Some(was), Some(now)) = (all[at].click, after[at].click) else { continue };
            match choice {
                // Up only into a run of steps just above it that only the chosen came in on, as
                // they move, and down no further than the steps below it they came in on alone.
                Choice::Later | Choice::Earlier => {
                    let alone_below = (fixed + 1..was).filter(|step| only_chosen(*step)).count() as u32;
                    prop_assert!((was + 1..=now).all(only_chosen) && was - now.min(was) <= alone_below, "{:?} went from {was} to {now}", all[at]);
                }
                Choice::Remove | Choice::Each(true) => {
                    // The steps below it that only the chosen came in on, which are left empty.
                    let emptied = (fixed + 1..was).filter(|step| only_chosen(*step)).count() as u32;
                    prop_assert_eq!(now, was - emptied, "{:?}", all[at]);
                }
                Choice::Animate | Choice::Each(false) => prop_assert_eq!(now, was),
            }
        }
    }

    #[test]
    fn animate_then_remove_is_where_it_began((all, fixed, picked) in slide()) {
        let all = without_gaps(&all, fixed);
        let still: Vec<Target> = picked.into_iter().filter(|target| all.iter().any(|was| was.target == *target && was.click.is_none() && !was.each)).collect();
        let Some(animated) = chosen(&all, fixed, &still, Choice::Animate) else { return Ok(()) };
        let after = applied(&all, &animated);
        prop_assert!(still.iter().all(|target| after.iter().any(|was| was.target == *target && was.click.is_some())));
        let removed = chosen(&after, fixed, &still, Choice::Remove).expect("each was animated");
        prop_assert_eq!(applied(&after, &removed), all);
    }

    #[test]
    fn earlier_then_later_is_where_it_began((all, fixed, picked) in slide(), which in any::<prop::sample::Index>(), whole in any::<bool>()) {
        // Some of what's at one step, or all of it, above the first after the fixed steps.
        let all = without_gaps(&all, fixed);
        let steps: Vec<u32> = all.iter().filter_map(|was| was.click).filter(|click| *click >= fixed + 2).collect();
        if steps.is_empty() {
            return Ok(());
        }
        let step = steps[which.index(steps.len())];
        let at_step: Vec<Target> = all.iter().filter(|was| was.click == Some(step)).map(|was| was.target.clone()).collect();
        let picked: Vec<Target> = if whole { at_step } else { at_step.into_iter().filter(|target| picked.contains(target)).collect() };
        prop_assume!(!picked.is_empty());
        let earlier = chosen(&all, fixed, &picked, Choice::Earlier).expect("it can go earlier");
        let after = applied(&all, &earlier);
        let later = chosen(&after, fixed, &picked, Choice::Later).expect("it can go later");
        prop_assert_eq!(applied(&after, &later), all);
    }

    #[test]
    fn fixed_steps_are_never_renumbered((all, fixed, picked) in slide(), choice in choice()) {
        let Some(result) = chosen(&all, fixed, &picked, choice) else { return Ok(()) };
        for (target, click) in &result {
            let was = all.iter().find(|was| was.target == *target).unwrap().click;
            if !picked.contains(target) {
                prop_assert!(was.is_some_and(|was| was > fixed), "{target:?} at {was:?} is at a fixed step");
                prop_assert!(click.is_some_and(|click| click > fixed), "{target:?} went to {click:?}, a fixed step");
            }
        }
    }
}

#[test]
fn the_first_animation_is_click_one() {
    let all = [stepped(tag(0), None), stepped(tag(1), None)];
    assert_eq!(chosen(&all, 0, &[tag(1)], Choice::Animate), Some(vec![(tag(1), Some(1))]));
    // After the steps a list shown one at a time takes.
    assert_eq!(chosen(&all, 3, &[tag(1)], Choice::Animate), Some(vec![(tag(1), Some(4))]));
}

#[test]
fn two_chosen_share_a_step() {
    let all = [stepped(tag(0), Some(1)), stepped(tag(1), None), stepped(tag(2), None)];
    assert_eq!(chosen(&all, 0, &[tag(1), tag(2)], Choice::Animate), Some(vec![(tag(1), Some(2)), (tag(2), Some(2))]));
    // One already animated keeps its step.
    assert_eq!(chosen(&all, 0, &[tag(0), tag(2)], Choice::Animate), Some(vec![(tag(2), Some(2))]));
    assert_eq!(chosen(&all, 0, &[tag(0)], Choice::Animate), None);
}

#[test]
fn removing_the_middle_of_three_closes_the_gap() {
    let all = [stepped(tag(0), Some(1)), stepped(tag(1), Some(2)), stepped(tag(2), Some(3))];
    assert_eq!(chosen(&all, 0, &[tag(1)], Choice::Remove), Some(vec![(tag(1), None), (tag(2), Some(2))]));
    // An area shown one at a time takes the first two: a click under them is left be.
    let all = [stepped(tag(0), Some(1)), stepped(tag(1), Some(3)), stepped(tag(2), Some(4))];
    assert_eq!(chosen(&all, 2, &[tag(1)], Choice::Remove), Some(vec![(tag(1), None), (tag(2), Some(3))]));
}

#[test]
fn earlier_at_click_one_does_nothing() {
    let all = [stepped(tag(0), Some(1)), stepped(tag(1), Some(2))];
    assert_eq!(chosen(&all, 0, &[tag(0)], Choice::Earlier), None);
    // Nor later at the last step, alone there.
    assert_eq!(chosen(&all, 0, &[tag(1)], Choice::Later), None);
    // Nor either on a thing that isn't animated.
    let all = [stepped(tag(0), None)];
    assert_eq!(chosen(&all, 0, &[tag(0)], Choice::Earlier), None);
    assert_eq!(chosen(&all, 0, &[tag(0)], Choice::Later), None);
}

#[test]
fn earlier_and_later_swap_with_the_next_step() {
    let all = [stepped(tag(0), Some(1)), stepped(tag(1), Some(2)), stepped(tag(2), Some(3))];
    assert_eq!(chosen(&all, 0, &[tag(0)], Choice::Later), Some(vec![(tag(0), Some(2)), (tag(1), Some(1))]));
    assert_eq!(chosen(&all, 0, &[tag(2)], Choice::Earlier), Some(vec![(tag(1), Some(3)), (tag(2), Some(2))]));
    // Sharing its step, it goes on to the next with what's there.
    let all = [stepped(tag(0), Some(1)), stepped(tag(1), Some(1)), stepped(tag(2), Some(2))];
    assert_eq!(chosen(&all, 0, &[tag(0)], Choice::Later), Some(vec![(tag(0), Some(2))]));
    // All of a step goes past the next.
    assert_eq!(
        chosen(&all, 0, &[tag(0), tag(1)], Choice::Later),
        Some(vec![(tag(0), Some(2)), (tag(1), Some(2)), (tag(2), Some(1))])
    );
    assert_eq!(chosen(&all, 0, &[tag(1)], Choice::Earlier), None);
    // Sharing the last step, it gets one of its own.
    let all = [stepped(tag(0), Some(1)), stepped(tag(1), Some(2)), stepped(tag(2), Some(2))];
    assert_eq!(chosen(&all, 0, &[tag(1)], Choice::Later), Some(vec![(tag(1), Some(3))]));
}

#[test]
fn a_gap_a_person_wrote_is_left_until_a_choice_touches_it() {
    let all = [stepped(tag(0), Some(1)), stepped(tag(1), Some(3)), stepped(tag(2), Some(5))];
    assert_eq!(chosen(&all, 0, &[tag(0)], Choice::Earlier), None);
    assert_eq!(chosen(&all, 0, &[tag(1)], Choice::Animate), None);
    assert_eq!(
        chosen(&all, 0, &[tag(0)], Choice::Remove),
        Some(vec![(tag(0), None), (tag(1), Some(1)), (tag(2), Some(2))])
    );
}

#[test]
fn one_at_a_time_takes_the_areas_numbered_click() {
    let all = [stepped(area("body"), Some(2)), stepped(tag(1), Some(1)), stepped(tag(2), Some(3))];
    assert_eq!(chosen(&all, 0, &[area("body")], Choice::Each(true)), Some(vec![(area("body"), None), (tag(2), Some(2))]));
    let all = [Stepped { target: area("body"), click: None, each: true }, stepped(tag(1), Some(4))];
    assert_eq!(chosen(&all, 3, &[area("body")], Choice::Each(true)), None);
    assert_eq!(chosen(&all, 3, &[area("body")], Choice::Each(false)), None);
    // Shown one at a time, it's animated already.
    assert_eq!(chosen(&all, 3, &[area("body")], Choice::Animate), None);
}

#[test]
fn nothing_goes_past_the_last_step() {
    // Every step is taken, by an area shown a piece at a time.
    let all = [Stepped { target: area("body"), click: None, each: true }, stepped(tag(1), None), stepped(tag(2), Some(MAX_STEP))];
    assert_eq!(chosen(&all, MAX_STEP, &[tag(1)], Choice::Animate), None);
    assert_eq!(chosen(&all, MAX_STEP, &[tag(2)], Choice::Later), None);
    assert_eq!(steps_shown(&all, MAX_STEP), MAX_STEP);
    assert_eq!(steps_shown(&all, 4), MAX_STEP);
    assert_eq!(steps_shown(&[], 4), 4);
}

#[test]
fn a_click_written_at_the_last_step_is_closed_up_to_make_room() {
    let all = [stepped(tag(0), Some(MAX_STEP)), stepped(tag(1), None), stepped(tag(2), Some(MAX_STEP))];
    assert_eq!(chosen(&all, 0, &[tag(1)], Choice::Animate), Some(vec![(tag(0), Some(1)), (tag(1), Some(2)), (tag(2), Some(1))]));
    assert_eq!(chosen(&all, 3, &[tag(1)], Choice::Animate), Some(vec![(tag(0), Some(4)), (tag(1), Some(5)), (tag(2), Some(4))]));
}
