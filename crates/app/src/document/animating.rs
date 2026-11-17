//! Animating what's selected: the step each thing and area comes in on, how it comes in, an
//! area shown a piece at a time, and how a slide is arrived at.

use slides_engine::{Enter, enter_word, is_positioned, read_click, read_enter};
use slides_format::{region_value, set_transition};

use super::*;
use crate::animate::{Choice, Stepped, chosen, closed_up, steps_shown};

/// What Undo shows for a choice made on the Animate tab, and for a slide's transition.
pub const ANIMATE: &str = "Change animation";
pub const TRANSITION: &str = "Change transition";

/// What the Animate tab's tools are about: what they act on, and what it has now.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct AnimateHere {
    /// The things and areas chosen: those selected, or with nothing selected, the area or the box
    /// whose text is being typed.
    pub targets: Vec<Target>,
    /// The step they come in on, when they share one.
    pub click: Option<u32>,
    /// How many steps the slide takes.
    pub steps: u32,
    /// How the first of them that's animated comes in, if any is.
    pub enter: Option<Enter>,
    /// Whether some of them are animated and some aren't.
    pub mixed: bool,
    /// Whether the one area chosen is shown a piece at a time; `None` unless one area is.
    pub each: Option<bool>,
    /// Whether the one area chosen holds a list, or more than one block, to show that way. A
    /// turned area can be too: the show has it all on click 1 while it's turned, and a piece at a
    /// time once it's upright again.
    pub can_each: bool,
    /// How the slide is arrived at.
    pub transition: Transition,
    /// Why nothing can be animated, if it can't.
    pub why_not: Option<EditRefused>,
}

/// A small number shown at the top-left corner of a thing or an area that's animated: the step
/// it comes in on, or the steps of an area shown a piece at a time.
#[derive(Clone, Debug, PartialEq)]
pub struct StepBadge {
    pub words: String,
    /// Where the thing is, in slide units, and the degrees it's turned by.
    pub rect: Rect,
    pub rotate: f32,
}

/// A value to write: on what, its name, and what, or `None` to take it away.
type Word = (Target, &'static str, Option<String>);

/// The words that animate a thing or an area: the click it comes in on, and how.
const ANIMATION_WORDS: [&str; 2] = ["click", "enter"];

impl Document {
    /// What the Animate tab's tools are about, on the current slide.
    pub fn animate_here(&mut self) -> AnimateHere {
        let slide = self.current;
        let transition = if slide < self.slide_count() { self.deck.timeline(slide).transition } else { Transition::None };
        let (all, fixed) = self.stepped(slide);
        let steps = steps_shown(&all, fixed);
        let typed = self.typed_target();
        let targets = match self.animate_targets(typed) {
            Ok(targets) => targets,
            Err(why) => return AnimateHere { steps, transition, why_not: Some(why), ..AnimateHere::default() },
        };
        // An area with nothing in it can't be animated; but a click or an entrance written on it
        // can still be taken away.
        if self.holds_an_empty_area(slide, &targets) && !targets.iter().any(|target| self.has_animation_words(slide, target)) {
            return AnimateHere { steps, transition, why_not: Some(EditRefused::EmptyArea), ..AnimateHere::default() };
        }
        let picked: Vec<&Stepped> = all.iter().filter(|stepped| targets.contains(&stepped.target)).collect();
        let animated = |stepped: &Stepped| stepped.click.is_some() || stepped.each;
        let first = picked.first().and_then(|stepped| stepped.click);
        let click = first.filter(|_| picked.iter().all(|stepped| stepped.click == first));
        let enter = picked.iter().find(|stepped| animated(stepped)).map(|stepped| self.entrance_of(slide, &stepped.target));
        let mixed = picked.iter().any(|stepped| animated(stepped)) && !picked.iter().all(|stepped| animated(stepped));
        let (each, can_each) = match targets.as_slice() {
            [Target::Region(name)] => (Some(picked.iter().any(|stepped| stepped.each)), self.holds_a_list(slide, name)),
            _ => (None, false),
        };
        AnimateHere { targets, click, steps, enter, mixed, each, can_each, transition, why_not: None }
    }

    /// Makes `choice` for what [`Self::animate_here`] says the tools are about, as one step
    /// named "Change animation", and returns the slides to show again. Making it still takes
    /// its click and entrance away, whatever they're written as. Refused with nothing to
    /// animate, with a thing written in the slide's text, for an area that holds no list to
    /// show a piece at a time, or for a thing to animate on a slide with every step taken. A
    /// choice that changes nothing leaves the typing be.
    pub fn animate(&mut self, choice: Choice) -> Result<Vec<usize>, EditRefused> {
        self.animated(choice, None)
    }

    /// Has what the tools are about come in as `enter` says, as one step named "Change
    /// animation": what isn't animated yet is first animated, as [`Choice::Animate`] does.
    pub fn set_entrance(&mut self, enter: Enter) -> Result<Vec<usize>, EditRefused> {
        self.animated(Choice::Animate, Some(enter))
    }

    /// Has the current slide, or with `all` every slide, arrived at as `transition` says, as
    /// one step named "Change transition", and returns the slides to show again.
    pub fn set_slide_transition(&mut self, transition: Transition, all: bool) -> Result<Vec<usize>, EditRefused> {
        let value = match transition {
            Transition::None => None,
            Transition::Fade => Some("fade"),
            Transition::Push => Some("push"),
        };
        let current = self.current;
        let slides: Vec<usize> = if all { (0..self.slide_count()).collect() } else { vec![current] };
        let change = self.build(|talk| set_transition(talk, &slides, value))?.ok_or(EditRefused::CouldNotBeMade)?;
        let holds = if all { Holds::Count(0) } else { Holds::Slide(current) };
        self.make(TRANSITION, change, holds, current, current, None)
    }

    /// A badge for each thing and area on the current slide that's animated, areas first, where
    /// it's shown: at the place a drag shows it, while one does.
    pub fn step_badges(&mut self) -> Vec<StepBadge> {
        let slide = self.current;
        let (all, _) = self.stepped(slide);
        if all.iter().all(|stepped| stepped.click.is_none() && !stepped.each) {
            return Vec::new();
        }
        let layout = self.text_layout(slide);
        all.iter()
            .filter_map(|stepped| {
                let words = match (stepped.click, &stepped.target) {
                    (Some(click), _) => click.to_string(),
                    (None, Target::Region(name)) if stepped.each => match self.deck.each_steps(slide, name) {
                        Some(steps) if steps > 1 => format!("1\u{2013}{steps}"),
                        _ => "1".to_string(),
                    },
                    _ => return None,
                };
                let (rect, rotate) = match &stepped.target {
                    Target::Tag(source) => layout.element(source).map(|element| (element.rect, element.rotate))?,
                    Target::Region(name) => layout.region(name).map(|region| (region.rect, region.rotate))?,
                };
                Some(StepBadge { words, rect, rotate })
            })
            .collect()
    }

    /// What `choice` makes, with each target then coming in as `enter` says, if it says.
    fn animated(&mut self, choice: Choice, enter: Option<Enter>) -> Result<Vec<usize>, EditRefused> {
        let slide = self.current;
        // The area or box typed in is chosen by its name, or where its tag starts, which
        // finishing the typing doesn't change.
        let typed = self.typed_target();
        let words = self.animation_words(slide, choice, enter, typed.clone())?;
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let version = self.text_version();
        self.finish_before_edit()?;
        let words = if self.text_version() == version {
            words
        } else {
            // Finishing the typing tidied the talk: what's selected may no longer be what was
            // asked about, and what's typed in is worked out again.
            if typed.is_none() {
                return Err(EditRefused::CouldNotBeMade);
            }
            self.animation_words(slide, choice, enter, typed.clone())?
        };
        let fixed = self.deck.fixed_steps(slide);
        let depth = self.history.depth();
        let mut changed = if words.is_empty() { Vec::new() } else { self.write_values(slide, &words, EditRun::New)? };
        // An area no longer shown a piece at a time can leave steps that nothing comes in on.
        if !words.is_empty() && self.deck.fixed_steps(slide) < fixed {
            let (all, fixed) = self.stepped(slide);
            let closing: Vec<Word> = closed_up(&all, fixed).into_iter().map(|(target, click)| (target, "click", click.map(|click| click.to_string()))).collect();
            if !closing.is_empty() {
                let run = self.open_run().map_or(EditRun::New, EditRun::Continue);
                changed.extend(self.write_values(slide, &closing, run)?);
            }
        }
        // What was typed in is selected, so the tools stay on it; Undo goes back to nothing
        // selected, and Redo selects it again.
        if let Some(target) = typed {
            let picked = match &target {
                Target::Region(name) => self.select_region(slide, name),
                Target::Tag(source) => match self.tag_from(slide, source.start) {
                    Some(range) => self.select(slide, range),
                    None => false,
                },
            };
            if picked && self.history.depth() > depth {
                self.history.select_mark_last(SelectMark { before: None, after: self.selection.clone() });
            }
        }
        Ok(changed)
    }

    /// The values `choice` writes on slide `slide` for what the tools are about, with each
    /// chosen then coming in as `enter` says, if it says: none where nothing would change.
    fn animation_words(&mut self, slide: usize, choice: Choice, enter: Option<Enter>, typed: Option<Target>) -> Result<Vec<Word>, EditRefused> {
        let targets = self.animate_targets(typed)?;
        if choice != Choice::Remove && self.holds_an_empty_area(slide, &targets) {
            return Err(EditRefused::EmptyArea);
        }
        if let Choice::Each(on) = choice {
            // Only one area is shown a piece at a time, and only one that holds a list.
            let [Target::Region(name)] = targets.as_slice() else {
                return Ok(Vec::new());
            };
            if on && !self.holds_a_list(slide, name) {
                return Err(EditRefused::NotAList);
            }
        }
        let (all, fixed) = self.stepped(slide);
        let steps = chosen(&all, fixed, &targets, choice);
        let still = all.iter().any(|stepped| targets.contains(&stepped.target) && stepped.click.is_none() && !stepped.each);
        if choice == Choice::Animate && steps.is_none() && still {
            return Err(EditRefused::TooManySteps);
        }
        let mut words: Vec<Word> = Vec::new();
        let mut word = |target: &Target, name: &'static str, value: Option<String>| {
            match words.iter_mut().find(|(at, named, _)| at == target && *named == name) {
                Some(found) => found.2 = value,
                None => words.push((target.clone(), name, value)),
            }
        };
        for (target, click) in steps.iter().flatten() {
            word(target, "click", click.map(|click| click.to_string()));
        }
        for target in all.iter().map(|stepped| &stepped.target).filter(|target| targets.contains(target)) {
            match choice {
                Choice::Each(on) => word(target, "click", on.then(|| "each".to_string())),
                // Whatever is written, read or not.
                Choice::Remove => {
                    for name in ANIMATION_WORDS {
                        if self.written(slide, target, name).is_some() {
                            word(target, name, None);
                        }
                    }
                }
                _ => {}
            }
            if let Some(enter) = enter {
                word(target, "enter", Some(enter_word(enter).to_string()));
            }
        }
        // What's already as asked isn't written again.
        words.retain(|(target, name, value)| self.written(slide, target, name).as_deref() != value.as_deref());
        Ok(words)
    }

    /// Writes each of `words` on slide `slide` as one step joining the run `run` says, keeping
    /// what's selected.
    fn write_values(&mut self, slide: usize, words: &[Word], run: EditRun) -> Result<Vec<usize>, EditRefused> {
        let edits: Vec<ValueEdit> = words.iter().map(|(target, name, value)| ValueEdit { target: target.clone(), name, value: value.as_deref() }).collect();
        let change = built(self.talk(), |talk| set_values(talk, slide, &edits))?.ok_or(EditRefused::CouldNotBeMade)?;
        let before = self.selection.clone().filter(|selection| selection.slide == slide);
        let depth = self.history.depth();
        let changed = self.make(ANIMATE, change, Holds::Slide(slide), slide, slide, Some(run))?;
        if before.is_some() && self.history.depth() > depth {
            self.history.select_mark_last(SelectMark { before, after: self.selection.clone() });
        }
        Ok(changed)
    }

    /// With nothing selected on the current slide, what's typed in there: the area, or the
    /// top-level tag of the box or shape.
    fn typed_target(&self) -> Option<Target> {
        let slide = self.current;
        if self.selection.as_ref().is_some_and(|selection| selection.slide == slide && !selection.things.is_empty()) {
            return None;
        }
        let editing = self.editing.as_ref().filter(|editing| editing.slide == slide)?;
        if let Some(name) = editing.flow.region() {
            return Some(Target::Region(name.to_string()));
        }
        let start = editing.flow.tag_start()?;
        self.tag_from(slide, start).map(Target::Tag)
    }

    /// The top-level tag on slide `slide` that starts at byte `start`, by its whole range.
    fn tag_from(&self, slide: usize, start: usize) -> Option<Range<usize>> {
        self.talk().slides.get(slide)?.blocks.iter().find_map(|block| match block {
            Block::Tag(tag) if tag.range.start == start => Some(tag.range.clone()),
            _ => None,
        })
    }

    /// The things and areas the Animate tab acts on: those selected on the current slide, as
    /// arranging takes them, or with nothing selected, `typed`, what's typed in.
    fn animate_targets(&self, typed: Option<Target>) -> Result<Vec<Target>, EditRefused> {
        let slide = self.current;
        let Some(written) = self.talk().slides.get(slide) else {
            return Err(EditRefused::NothingToAnimate);
        };
        let placed = |source: &Range<usize>| written.blocks.iter().any(|block| matches!(block, Block::Tag(tag) if is_positioned(tag) && tag.range == *source));
        // A thing inside another selected goes with that, and one a box with a place holds
        // alone comes in with the box.
        let things: Vec<Target> = match self.arranged_here().filter(|selection| !selection.things.is_empty()) {
            Some(selection) => selection
                .things
                .iter()
                .map(|thing| match thing {
                    Thing::Region(name) => Target::Region(name.clone()),
                    Thing::Block(source) => Target::Tag(source.clone()),
                })
                .collect(),
            None => typed.into_iter().collect(),
        };
        if things.is_empty() {
            return Err(EditRefused::NothingToAnimate);
        }
        if things.iter().any(|target| matches!(target, Target::Tag(source) if !placed(source))) {
            return Err(EditRefused::InTheText);
        }
        Ok(things)
    }

    /// Whether any of `targets` is an area of slide `slide` with nothing in it.
    fn holds_an_empty_area(&mut self, slide: usize, targets: &[Target]) -> bool {
        let layout = self.text_layout(slide);
        targets.iter().any(|target| matches!(target, Target::Region(name) if layout.region(name).is_some_and(|region| region.empty)))
    }

    /// Whether `target` on slide `slide` has a click or an entrance written, read or not.
    fn has_animation_words(&self, slide: usize, target: &Target) -> bool {
        ANIMATION_WORDS.iter().any(|name| self.written(slide, target, name).is_some())
    }

    /// Whether area `name` of slide `slide` holds a list, or more than one block.
    fn holds_a_list(&mut self, slide: usize, name: &str) -> bool {
        self.text_layout(slide).region(name).is_some_and(|region| region.is_list)
    }

    /// What each thing with a place of its own and each area of slide `slide` has now, and how
    /// many of its steps are fixed ones. An area is shown a piece at a time as the engine shows
    /// it, so that its steps are among the fixed ones. A click written on an area with nothing in
    /// it isn't counted, as the show gives it no step.
    fn stepped(&mut self, slide: usize) -> (Vec<Stepped>, u32) {
        if slide >= self.slide_count() {
            return (Vec::new(), 0);
        }
        let layout = self.text_layout(slide);
        let fixed = self.deck.fixed_steps(slide);
        let each: Vec<bool> = layout.regions().iter().map(|region| self.deck.each_steps(slide, &region.name).is_some()).collect();
        let Some(written) = self.talk().slides.get(slide) else {
            return (Vec::new(), fixed);
        };
        let areas = layout.regions().iter().zip(each).map(|(region, each)| {
            let value = (!region.empty).then(|| guarded(|| region_value(written, &region.name, "click")).ok().flatten()).flatten();
            Stepped { target: Target::Region(region.name.clone()), click: value.and_then(read_click), each }
        });
        let things = written.blocks.iter().filter_map(|block| match block {
            Block::Tag(tag) if is_positioned(tag) => Some(Stepped { target: Target::Tag(tag.range.clone()), click: tag.attr("click").and_then(read_click), each: false }),
            _ => None,
        });
        (areas.chain(things).collect(), fixed)
    }

    /// The value `name` written for `target` on slide `slide`, as written.
    fn written(&self, slide: usize, target: &Target, name: &str) -> Option<String> {
        let written = self.talk().slides.get(slide)?;
        match target {
            Target::Region(region) => guarded(|| region_value(written, region, name)).ok().flatten().map(str::to_string),
            Target::Tag(source) => written.blocks.iter().find_map(|block| match block {
                Block::Tag(tag) if tag.range == *source => tag.attr(name).map(str::to_string),
                _ => None,
            }),
        }
    }

    /// How `target` comes in, as the engine reads it.
    fn entrance_of(&self, slide: usize, target: &Target) -> Enter {
        self.written(slide, target, "enter").as_deref().and_then(read_enter).unwrap_or_default()
    }
}
