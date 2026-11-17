//! Animating what's selected: the step each thing and area comes in on, how it comes in, an
//! area shown a piece at a time, and how a slide is arrived at.

use slides_engine::{Enter, is_positioned};
use slides_format::{region_value, set_transition};

use super::*;
use crate::animate::{Choice, MAX_STEP, Stepped, chosen, click_of, closed_up, steps_shown};

/// What Undo shows for a choice made on the Animate tab, and for a slide's transition.
pub const ANIMATE: &str = "Change animation";
pub const TRANSITION: &str = "Change transition";

/// What the Animate tab's tools are about: what they act on, and what it has now.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct AnimateHere {
    /// The things and areas chosen: those selected, or the area whose text is being typed.
    pub targets: Vec<Target>,
    /// The step they come in on, when they share one.
    pub click: Option<u32>,
    /// How many steps the slide takes.
    pub steps: u32,
    /// How the first of them that's animated comes in, if any is.
    pub enter: Option<Enter>,
    /// Whether the one area chosen is shown a piece at a time; `None` unless one area is.
    pub each: Option<bool>,
    /// Whether the one area chosen holds a list, or more than one block, to show that way.
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

impl Document {
    /// What the Animate tab's tools are about, on the current slide.
    pub fn animate_here(&mut self) -> AnimateHere {
        let slide = self.current;
        let transition = if slide < self.slide_count() { self.deck.timeline(slide).transition } else { Transition::None };
        let (all, fixed) = self.stepped(slide);
        let steps = steps_shown(&all, fixed);
        let targets = match self.animate_targets() {
            Ok(targets) => targets,
            Err(why) => return AnimateHere { steps, transition, why_not: Some(why), ..AnimateHere::default() },
        };
        let picked: Vec<&Stepped> = all.iter().filter(|stepped| targets.contains(&stepped.target)).collect();
        let first = picked.first().and_then(|stepped| stepped.click);
        let click = first.filter(|_| picked.iter().all(|stepped| stepped.click == first));
        let enter = picked
            .iter()
            .find(|stepped| stepped.click.is_some() || stepped.each)
            .map(|stepped| self.entrance_of(slide, &stepped.target));
        let (each, can_each) = match targets.as_slice() {
            [Target::Region(name)] => {
                let layout = self.text_layout(slide);
                let is_list = layout.region(name).is_some_and(|region| region.is_list);
                (Some(picked.iter().any(|stepped| stepped.each)), is_list)
            }
            _ => (None, false),
        };
        AnimateHere { targets, click, steps, enter, each, can_each, transition, why_not: None }
    }

    /// Makes `choice` for what [`Self::animate_here`] says the tools are about, as one step
    /// named "Change animation", and returns the slides to show again. Making it still takes
    /// its entrance away too, and an area's showing a piece at a time. Refused with nothing to
    /// animate, with a thing written in the slide's text, or for a thing to animate on a slide
    /// with as many steps as it can have.
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

    /// A badge for each thing and area on the current slide that's animated, areas first.
    pub fn step_badges(&mut self) -> Vec<StepBadge> {
        let slide = self.current;
        let (all, _) = self.stepped(slide);
        if all.iter().all(|stepped| stepped.click.is_none() && !stepped.each) {
            return Vec::new();
        }
        let layout = self.text_layout(slide);
        let pieces = if all.iter().any(|stepped| stepped.each) { self.pieces(slide) } else { Vec::new() };
        all.iter()
            .filter_map(|stepped| {
                let words = match (stepped.click, &stepped.target) {
                    (Some(click), _) => click.to_string(),
                    (None, Target::Region(name)) if stepped.each => {
                        let count = pieces.iter().find(|(region, _)| region == name).map_or(1, |(_, count)| *count);
                        if count > 1 { format!("1\u{2013}{count}") } else { "1".to_string() }
                    }
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
        let targets = self.animate_targets()?;
        // Only one area is shown a piece at a time.
        if matches!(choice, Choice::Each(_)) && !matches!(targets.as_slice(), [Target::Region(_)]) {
            return Ok(Vec::new());
        }
        // Text being typed in an area is finished first: the area is chosen by its name, which
        // that doesn't change.
        let typed_in = self.editing.as_ref().and_then(|editing| editing.flow.region().map(str::to_string));
        let typed_in = typed_in.filter(|_| self.selection.is_none());
        self.finish_before_edit()?;
        let slide = self.current;
        let made = self.animation_made(slide, &targets, choice, enter);
        // The area typed in stays the one the tools act on, whatever came of it.
        if let Some(name) = typed_in {
            self.select_region(slide, &name);
        }
        made
    }

    /// Makes `choice` for `targets` on slide `slide`, as [`Self::animated`] says.
    fn animation_made(&mut self, slide: usize, targets: &[Target], choice: Choice, enter: Option<Enter>) -> Result<Vec<usize>, EditRefused> {
        let (all, fixed) = self.stepped(slide);
        let steps = chosen(&all, fixed, targets, choice);
        let still = all.iter().any(|stepped| targets.contains(&stepped.target) && stepped.click.is_none() && !stepped.each);
        if choice == Choice::Animate && steps.is_none() && still && steps_shown(&all, fixed) >= MAX_STEP {
            return Err(EditRefused::TooManySteps);
        }
        let mut words: Vec<(Target, &'static str, Option<String>)> = Vec::new();
        let mut word = |target: &Target, name: &'static str, value: Option<String>| {
            match words.iter_mut().find(|(at, named, _)| at == target && *named == name) {
                Some(found) => found.2 = value,
                None => words.push((target.clone(), name, value)),
            }
        };
        for (target, click) in steps.iter().flatten() {
            word(target, "click", click.map(|click| click.to_string()));
        }
        for stepped in all.iter().filter(|stepped| targets.contains(&stepped.target)) {
            let target = &stepped.target;
            match choice {
                Choice::Each(on) => word(target, "click", on.then(|| "each".to_string())),
                Choice::Remove => {
                    if stepped.each {
                        word(target, "click", None);
                    }
                    if self.entrance_written(slide, target) {
                        word(target, "enter", None);
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
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let mut changed = self.write_values(slide, &words, EditRun::New)?;
        // An area no longer shown a piece at a time can leave steps that nothing comes in on.
        if self.deck.fixed_steps(slide) < fixed {
            let (all, fixed) = self.stepped(slide);
            let closing: Vec<(Target, &'static str, Option<String>)> =
                closed_up(&all, fixed).into_iter().map(|(target, click)| (target, "click", click.map(|click| click.to_string()))).collect();
            if !closing.is_empty() {
                let run = self.open_run().map_or(EditRun::New, EditRun::Continue);
                changed.extend(self.write_values(slide, &closing, run)?);
            }
        }
        Ok(changed)
    }

    /// Writes each of `words` on slide `slide` as one step joining the run `run` says, keeping
    /// what's selected.
    fn write_values(&mut self, slide: usize, words: &[(Target, &'static str, Option<String>)], run: EditRun) -> Result<Vec<usize>, EditRefused> {
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

    /// The things and areas the Animate tab acts on: those selected on the current slide, or
    /// with nothing selected, the area whose text is being typed.
    fn animate_targets(&self) -> Result<Vec<Target>, EditRefused> {
        let slide = self.current;
        let Some(written) = self.talk().slides.get(slide) else {
            return Err(EditRefused::NothingToAnimate);
        };
        if let Some(selection) = self.selection.as_ref().filter(|selection| selection.slide == slide && !selection.things.is_empty()) {
            return selection
                .things
                .iter()
                .map(|thing| match thing {
                    Thing::Region(name) => Ok(Target::Region(name.clone())),
                    Thing::Block(source) => {
                        let placed = written.blocks.iter().any(|block| matches!(block, Block::Tag(tag) if is_positioned(tag) && tag.range == *source));
                        if placed { Ok(Target::Tag(source.clone())) } else { Err(EditRefused::InTheText) }
                    }
                })
                .collect();
        }
        match self.editing.as_ref().filter(|editing| editing.slide == slide).and_then(|editing| editing.flow.region()) {
            Some(name) => Ok(vec![Target::Region(name.to_string())]),
            None => Err(EditRefused::NothingToAnimate),
        }
    }

    /// What each thing with a place of its own and each area of slide `slide` has now, and how
    /// many of its steps are fixed ones.
    fn stepped(&mut self, slide: usize) -> (Vec<Stepped>, u32) {
        if slide >= self.slide_count() {
            return (Vec::new(), 0);
        }
        let layout = self.text_layout(slide);
        let fixed = self.deck.fixed_steps(slide);
        let Some(written) = self.talk().slides.get(slide) else {
            return (Vec::new(), fixed);
        };
        let areas = layout.regions().iter().map(|region| {
            let value = guarded(|| region_value(written, &region.name, "click")).ok().flatten();
            Stepped { target: Target::Region(region.name.clone()), click: value.and_then(click_of), each: value == Some("each") }
        });
        let things = written.blocks.iter().filter_map(|block| match block {
            Block::Tag(tag) if is_positioned(tag) => Some(Stepped { target: Target::Tag(tag.range.clone()), click: tag.attr("click").and_then(click_of), each: false }),
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

    /// Whether `target` has an entrance written, readable or not.
    fn entrance_written(&self, slide: usize, target: &Target) -> bool {
        self.written(slide, target, "enter").is_some()
    }

    /// How `target` comes in, as the engine reads it.
    fn entrance_of(&self, slide: usize, target: &Target) -> Enter {
        match self.written(slide, target, "enter").as_deref().map(str::trim) {
            Some("fade") => Enter::Fade,
            Some("fly-up") => Enter::FlyUp,
            _ => Enter::Appear,
        }
    }

    /// How many pieces each area of slide `slide` shown a piece at a time is shown in, by its
    /// name: the layers the engine draws it in, one for each step from 1.
    fn pieces(&mut self, slide: usize) -> Vec<(String, u32)> {
        let layout = self.text_layout(slide);
        let content = self.content_tags();
        let flows = guarded(|| flows_in(self.talk(), slide, &content)).unwrap_or_default();
        // Where each area's text is written, to tell its pieces from the next area's layer.
        let text_of = |name: &str| -> Vec<Range<usize>> {
            flows
                .iter()
                .enumerate()
                .filter(|(at, _)| layout.region_of_flow(*at) == Some(name))
                .flat_map(|(_, flow)| flow.source.iter().map(|source| source.block.clone()))
                .collect()
        };
        let Some(written) = self.talk().slides.get(slide) else {
            return Vec::new();
        };
        // Each area that holds anything, in the layout's order, and whether it's shown a piece
        // at a time: a turned one is shown whole.
        let areas: Vec<(&str, bool)> = layout
            .regions()
            .iter()
            .filter(|region| !region.empty)
            .map(|region| {
                let line = |name: &str| guarded(|| region_value(written, &region.name, name).map(str::to_string)).ok().flatten();
                let turned = line("rotate").and_then(|value| value.parse::<f32>().ok()).is_some_and(|turn| turn.is_finite() && turn != 0.);
                (region.name.as_str(), line("click").as_deref() == Some("each") && !turned)
            })
            .collect();
        let deck = &mut self.deck;
        let Ok(layers) = guarded(|| deck.specs_fonts_and_files(slide).0.iter().map(|spec| (spec.source.clone(), spec.click)).collect::<Vec<_>>()) else {
            return Vec::new();
        };
        let mut pieces = Vec::new();
        // The background comes first, then each area that holds anything, in the layout's order.
        let mut at = 1;
        for (name, each) in areas {
            if !each {
                at += 1;
                continue;
            }
            let others: Vec<Range<usize>> = layout
                .regions()
                .iter()
                .filter(|other| other.name != name)
                .flat_map(|other| text_of(&other.name))
                .collect();
            let mut count = 0;
            while let Some((source, click)) = layers.get(at) {
                let of_another = source.as_ref().is_some_and(|source| others.iter().any(|text| source.start <= text.start && text.end <= source.end));
                if *click != Some(count + 1) || of_another {
                    break;
                }
                count += 1;
                at += 1;
            }
            if count == 0 {
                at += 1;
            }
            pieces.push((name.to_string(), count.max(1)));
        }
        pieces
    }
}

/// How `enter` is written.
fn enter_word(enter: Enter) -> &'static str {
    match enter {
        Enter::Appear => "appear",
        Enter::Fade => "fade",
        Enter::FlyUp => "fly-up",
    }
}
