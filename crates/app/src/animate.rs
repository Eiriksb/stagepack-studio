//! Click steps as the Animate tab chooses them: which step each thing and area of a slide comes
//! in on, once a thing is animated, made still, or moved earlier or later. A slide's steps are
//! counted from 1. The first few can be fixed ones, taken by an area shown a piece at a time or
//! a component that steps: no click is renumbered into or out of those. Above them, every step
//! has something come in on it once a choice is made, so that no click shows nothing.

use slides_format::Target;

/// The last step a slide can have.
pub const MAX_STEP: u32 = slides_engine::MAX_CLICK;

/// What one thing or area of a slide has now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stepped {
    pub target: Target,
    /// The step it comes in on: only a whole number from 1 to [`MAX_STEP`] counts.
    pub click: Option<u32>,
    /// Whether it's an area shown a piece at a time, whose steps are fixed ones.
    pub each: bool,
}

/// What's chosen for the targets chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Each with no step comes in on a step of its own after all the others.
    Animate,
    /// None comes in on a step: each is there from the start.
    Remove,
    /// Each comes in a step earlier, or a step later.
    Earlier,
    Later,
    /// The one area is shown a piece at a time, or not.
    Each(bool),
}

/// How many steps the slide whose things and areas are `all` takes, `fixed` of them fixed.
pub fn steps_shown(all: &[Stepped], fixed: u32) -> u32 {
    all.iter().filter_map(|stepped| stepped.click).fold(fixed.min(MAX_STEP), u32::max)
}

/// The new step of every target of `all` whose step changes once `choice` is made for those of
/// `chosen`, `None` taking its step away: those chosen as `choice` says, and then any other
/// above the `fixed` steps lowered to close a step nothing comes in on, or moved a step as one
/// chosen takes its place. Those at or under the fixed steps are never moved. `None` when nothing
/// would change, and so a gap a person wrote is left until a choice changes something.
///
/// Moving earlier or later, one chosen goes a step down, to 1 at least, or up, to one past the
/// last. Where none but the chosen came in on the step they left, what came in on the step they
/// went to takes its place, so that one alone on its step swaps with the next.
pub fn chosen(all: &[Stepped], fixed: u32, chosen: &[Target], choice: Choice) -> Option<Vec<(Target, Option<u32>)>> {
    let fixed = fixed.min(MAX_STEP);
    let was: Vec<Option<u32>> = all.iter().map(|stepped| stepped.click).collect();
    let picked: Vec<bool> = all.iter().map(|stepped| chosen.contains(&stepped.target)).collect();
    let mut now = was.clone();
    match choice {
        Choice::Animate => {
            // After every other step, once the steps nothing comes in on are closed: past
            // the last there can be, until they are.
            for (at, stepped) in all.iter().enumerate() {
                if picked[at] && stepped.click.is_none() && !stepped.each {
                    now[at] = Some(MAX_STEP + 1);
                }
            }
        }
        Choice::Remove | Choice::Each(true) => {
            for at in (0..all.len()).filter(|at| picked[*at]) {
                now[at] = None;
            }
        }
        Choice::Each(false) => {}
        Choice::Earlier | Choice::Later => {
            let later = choice == Choice::Later;
            // Whether only chosen things come in on `step`, and one of them moves from it.
            let left_to_chosen = |step: u32| {
                (0..all.len()).all(|at| picked[at] || was[at] != Some(step))
                    && (0..all.len()).any(|at| picked[at] && was[at] == Some(step) && moves(step, later))
            };
            for at in 0..all.len() {
                let Some(step) = was[at] else { continue };
                if picked[at] {
                    if moves(step, later) {
                        now[at] = Some(if later { step + 1 } else { step - 1 });
                    }
                    continue;
                }
                // What's not chosen takes the place of the chosen beside it, when they leave
                // their steps to nothing else: past all of them, where they come one after another.
                if step <= fixed {
                    continue;
                }
                let mut place = step;
                loop {
                    match if later { place.checked_sub(1) } else { place.checked_add(1) } {
                        Some(next) if next > fixed && next <= MAX_STEP && left_to_chosen(next) => place = next,
                        _ => break,
                    }
                }
                now[at] = Some(place);
            }
        }
    }
    if now == was {
        return None;
    }
    let changes = changed(all, &now, fixed);
    // A slide with every step taken has none for another.
    if changes.iter().any(|(_, click)| click.is_some_and(|click| click > MAX_STEP)) {
        return None;
    }
    (!changes.is_empty()).then_some(changes)
}

/// The new step of every target of `all` above the `fixed` steps whose step is lowered to close
/// a step above them that nothing comes in on.
pub fn closed_up(all: &[Stepped], fixed: u32) -> Vec<(Target, Option<u32>)> {
    let now: Vec<Option<u32>> = all.iter().map(|stepped| stepped.click).collect();
    changed(all, &now, fixed.min(MAX_STEP))
}

/// Each target of `all` whose step, once it's `now` and the steps above `fixed` that nothing
/// comes in on are closed, isn't what it has.
fn changed(all: &[Stepped], now: &[Option<u32>], fixed: u32) -> Vec<(Target, Option<u32>)> {
    // Every step above the fixed ones with something on it, in order, is closed up.
    let mut used: Vec<u32> = now.iter().flatten().copied().filter(|step| *step > fixed).collect();
    used.sort_unstable();
    used.dedup();
    let closed = |step: u32| if step > fixed { fixed + 1 + used.partition_point(|used| *used < step) as u32 } else { step };
    all.iter()
        .zip(now)
        .filter_map(|(stepped, now)| {
            let now = now.map(closed);
            (now != stepped.click).then(|| (stepped.target.clone(), now))
        })
        .collect()
}

/// Whether one chosen at `step` moves at all, later or earlier.
fn moves(step: u32, later: bool) -> bool {
    if later { step < MAX_STEP } else { step > 1 }
}
