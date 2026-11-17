//! Moving things and a layout's areas on a slide: showing them at a place the talk doesn't say
//! yet, as while they're dragged, and writing their place once they're let go of.

use slides_engine::Placing;
use slides_format::number_text;

use super::*;
use crate::arrange::Placed;

/// What Undo shows for a thing moved, made bigger or smaller, turned, or moved with the keys.
pub const MOVE: &str = "Move";
pub const RESIZE: &str = "Resize";
pub const TURN: &str = "Turn";
pub const NUDGE: &str = "Move";

/// How far a number has to change, in slide units or degrees, to be written again.
const MOVED: f32 = 0.01;

impl Document {
    /// Shows each of `places` on the current slide where it says, without writing anything,
    /// until [`Self::drop_preview`] or any edit. Only a thing with a place of its own, or an
    /// area of the slide's layout, is shown moved.
    pub fn preview_places(&mut self, places: &[(Thing, Placed)]) {
        let slide = self.current;
        let layout = self.text_layout(slide);
        let placing = places
            .iter()
            .filter(|(thing, _)| movable(&layout, thing))
            .map(|(thing, placed)| Placing { target: target_of(thing), rect: placed.rect, rotate: placed.rotate })
            .collect();
        let deck = &mut self.deck;
        if guarded(|| deck.set_placing(slide, placing)).is_err() {
            self.deck.clear_placing();
        }
    }

    /// Shows the slide as the talk has it again.
    pub fn drop_preview(&mut self) {
        self.deck.clear_placing();
    }

    /// What's shown at a place the talk doesn't say yet: its slide, and each thing's place.
    pub fn placing(&self) -> Option<(usize, &[Placing])> {
        self.deck.placing()
    }

    /// Changes each time the talk's text does, by any edit, an undo or a reload among them.
    pub fn text_version(&self) -> u64 {
        self.deck.text_version()
    }

    /// Where each thing selected that can be moved as it's written is, in the order they were
    /// picked, and whether any selected can't be: one written in the slide's text, with no place
    /// of its own.
    pub fn movable_selected(&mut self) -> (Vec<(Thing, Placed)>, bool) {
        let Some(selection) = self.selection.clone() else {
            return (Vec::new(), false);
        };
        let layout = self.text_layout(selection.slide);
        let boxes = self.selected_boxes();
        let all = boxes.len();
        let movable: Vec<(Thing, Placed)> = boxes
            .into_iter()
            .filter(|(thing, ..)| movable(&layout, thing))
            .map(|(thing, rect, rotate)| (thing, Placed { rect, rotate }))
            .collect();
        let left_out = movable.len() < all;
        (movable, left_out)
    }

    /// Writes each of `places` on the current slide, as one step named `label` joining the run
    /// `run` says, and returns the slides to show again. Only the numbers that differ from where
    /// the slide has the thing now are written, positions and sizes to a whole unit; a thing
    /// written in the slide's text, with no place of its own, is left be. Where nothing differs,
    /// nothing is written and no step made. It's refused if finishing the typing first changes
    /// the talk.
    pub fn place_things(&mut self, label: &'static str, places: &[(Thing, Placed)], run: EditRun) -> Result<Vec<usize>, EditRefused> {
        self.drop_preview();
        // Typing is finished first. That can move the things, and then where `places` says
        // they are written is no longer where they are: it's refused.
        let version = self.text_version();
        self.finish_before_edit()?;
        if self.text_version() != version {
            return Err(EditRefused::CouldNotBeMade);
        }
        let slide = self.current;
        let layout = self.text_layout(slide);
        let mut words: Vec<(Target, &'static str, String)> = Vec::new();
        for (thing, placed) in places {
            let now = match thing {
                Thing::Block(source) => layout.element(source).filter(|element| element.placed).map(|element| (element.rect, element.rotate)),
                Thing::Region(name) => layout.region(name).map(|region| (region.rect, region.rotate)),
            };
            let Some((rect, rotate)) = now else {
                continue;
            };
            let to = placed.rect;
            // Each number: what's asked, what's written for it, and what the slide has now.
            let numbers = [
                ("x", to.x, to.x.round(), rect.x),
                ("y", to.y, to.y.round(), rect.y),
                ("w", to.w, to.w.round(), rect.w),
                ("h", to.h, to.h.round(), rect.h),
                ("rotate", placed.rotate, placed.rotate, rotate),
            ];
            for (name, asked, value, old) in numbers {
                if value.is_finite() && (asked - old).abs() > MOVED && number_text(value) != number_text(old) {
                    words.push((target_of(thing), name, number_text(value)));
                }
            }
        }
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let edits: Vec<ValueEdit> = words.iter().map(|(target, name, value)| ValueEdit { target: target.clone(), name, value: Some(value) }).collect();
        let change = built(self.talk(), |talk| set_values(talk, slide, &edits))?.ok_or(EditRefused::CouldNotBeMade)?;
        let before = self.selection.clone().filter(|selection| selection.slide == slide);
        let depth = self.history.depth();
        let changed = self.make(label, change, Holds::Slide(slide), slide, slide, Some(run))?;
        if before.is_some() && self.history.depth() > depth {
            self.history.select_mark_last(SelectMark { before, after: self.selection.clone() });
        }
        Ok(changed)
    }
}

/// Whether `thing` can be moved where it's written: a thing with a place of its own, or an area.
fn movable(layout: &TextLayout, thing: &Thing) -> bool {
    match thing {
        Thing::Block(source) => layout.element(source).is_some_and(|element| element.placed),
        Thing::Region(name) => layout.region(name).is_some(),
    }
}

/// What a value is set on for `thing`.
fn target_of(thing: &Thing) -> Target {
    match thing {
        Thing::Block(source) => Target::Tag(source.clone()),
        Thing::Region(name) => Target::Region(name.clone()),
    }
}
