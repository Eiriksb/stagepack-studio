//! Moving things and a layout's areas on a slide: showing them at a place the talk doesn't say
//! yet, as while they're dragged, and writing their place once they're let go of.

use slides_engine::{Placing, is_positioned};
use slides_format::{Order, number_text, reorder};

use super::*;
use crate::arrange::{AlignTo, Placed, aligned, distributed};

/// What Undo shows for a thing moved, made bigger or smaller, turned, or moved with the keys.
pub const MOVE: &str = "Move";
pub const RESIZE: &str = "Resize";
pub const TURN: &str = "Turn";
pub const NUDGE: &str = "Move";
/// What Undo shows for things aligned, spaced out, or put in front or behind.
pub const ALIGN: &str = "Align";
pub const DISTRIBUTE: &str = "Distribute";
pub const ORDER: &str = "Change order";

/// What can be done to arrange what's selected, for the tools that do it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ArrangeHere {
    /// How many things are selected, areas among them.
    pub count: usize,
    /// How many of them have a place of their own: a thing with one, or an area.
    pub placed: usize,
    /// Whether the one thing selected can go each way, in [`Order`]'s order: forward,
    /// backward, to the front and to the back.
    pub can: [bool; 4],
    /// Why what's selected can't be put in front or behind at all, if it can't and there's a
    /// reason to say.
    pub order_refused: Option<EditRefused>,
}

impl ArrangeHere {
    /// Why what's selected can't be aligned, if there's something selected and it can't.
    pub fn align_refused(&self) -> Option<EditRefused> {
        (self.placed < self.count).then_some(EditRefused::InTheText)
    }

    /// Whether what's selected can be aligned.
    pub fn can_align(&self) -> bool {
        self.count > 0 && self.align_refused().is_none()
    }

    /// Why what's selected can't be spaced out, if there's something selected and it can't.
    pub fn distribute_refused(&self) -> Option<EditRefused> {
        if self.count == 0 {
            None
        } else if self.count < 3 {
            Some(EditRefused::NeedsThree)
        } else {
            self.align_refused()
        }
    }

    /// Whether what's selected can be spaced out.
    pub fn can_distribute(&self) -> bool {
        self.count >= 3 && self.distribute_refused().is_none()
    }

    /// Whether the one thing selected can go `order`.
    pub fn can_order(&self, order: Order) -> bool {
        self.can[order_index(order)]
    }
}

/// Where `order` is in [`ArrangeHere::can`].
fn order_index(order: Order) -> usize {
    match order {
        Order::Forward => 0,
        Order::Backward => 1,
        Order::Front => 2,
        Order::Back => 3,
    }
}

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
    /// the slide has the thing now are written, positions and sizes to a whole unit, and a turn
    /// back to upright is taken away rather than written as 0; a thing
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
        // Each value to write, or `None` to take away: an upright turn isn't written.
        let mut words: Vec<(Target, &'static str, Option<String>)> = Vec::new();
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
            let first = words.len();
            for (name, asked, value, old) in numbers {
                if value.is_finite() && (asked - old).abs() > MOVED && number_text(value) != number_text(old) {
                    let value = (name != "rotate" || value != 0.).then(|| number_text(value));
                    words.push((target_of(thing), name, value));
                }
            }
            // A tag with only one of its width and height, or neither, has the other worked out
            // from what it shows, as a picture's from its proportions: given one, it's given both,
            // so that it reads back the size it was made.
            if let Thing::Block(source) = thing {
                let sizes = [("w", to.w), ("h", to.h)];
                let written: Vec<&str> = words[first..].iter().map(|(_, name, _)| *name).filter(|name| sizes.iter().any(|(size, _)| size == name)).collect();
                let tag = guarded(|| find_tag(self.talk(), slide, source)).ok().flatten();
                if !written.is_empty() && let Some(tag) = tag {
                    for (name, value) in sizes {
                        if !written.contains(&name) && tag.attr(name).is_none() && value.is_finite() {
                            words.push((target_of(thing), name, Some(number_text(value.round()))));
                        }
                    }
                }
            }
        }
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let edits: Vec<ValueEdit> = words.iter().map(|(target, name, value)| ValueEdit { target: target.clone(), name, value: value.as_deref() }).collect();
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

impl Document {
    /// What can be done to arrange what's selected, from the talk as it's written.
    pub fn arrange_here(&self) -> ArrangeHere {
        let Some(selection) = &self.selection else {
            return ArrangeHere::default();
        };
        let Some(slide) = self.talk().slides.get(selection.slide) else {
            return ArrangeHere::default();
        };
        // The tags at the top level of the slide with a place of their own, in the order
        // they're drawn, from the back.
        let placed: Vec<&Range<usize>> = slide
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Tag(tag) if is_positioned(tag) => Some(block.range()),
                _ => None,
            })
            .collect();
        let has_place = |thing: &Thing| match thing {
            Thing::Region(_) => true,
            Thing::Block(source) => placed.contains(&source),
        };
        let mut here = ArrangeHere {
            count: selection.things.len(),
            placed: selection.things.iter().filter(|thing| has_place(thing)).count(),
            ..ArrangeHere::default()
        };
        match selection.things.as_slice() {
            [Thing::Region(_)] => here.order_refused = Some(EditRefused::AreaOrder),
            [Thing::Block(source)] => match placed.iter().position(|range| *range == source) {
                Some(at) => {
                    let (back, front) = (at > 0, at + 1 < placed.len());
                    here.can = [front, back, front, back];
                }
                None => here.order_refused = Some(EditRefused::InTheText),
            },
            _ => here.order_refused = Some(EditRefused::OneToOrder),
        }
        here
    }

    /// Lines the things selected up on the line `to`, as one step named "Align", and returns
    /// the slides to show again: one thing on the slide's line, several on that of the box
    /// that holds them all. Refused if one of them is written in the slide's text.
    pub fn align_things(&mut self, to: AlignTo) -> Result<Vec<usize>, EditRefused> {
        let (things, left_out) = self.movable_selected();
        if left_out {
            return Err(EditRefused::InTheText);
        }
        if things.is_empty() {
            return Ok(Vec::new());
        }
        let places: Vec<Placed> = things.iter().map(|(_, placed)| *placed).collect();
        let lined: Vec<(Thing, Placed)> = things.into_iter().map(|(thing, _)| thing).zip(aligned(&places, to)).collect();
        self.place_things(ALIGN, &lined, EditRun::New)
    }

    /// Spaces the things selected out across, or down, with the same gap between them, as one
    /// step named "Distribute", and returns the slides to show again. Refused for fewer than
    /// three, or if one of them is written in the slide's text.
    pub fn distribute_things(&mut self, across: bool) -> Result<Vec<usize>, EditRefused> {
        let count = self.selection.as_ref().map_or(0, |selection| selection.things.len());
        if count == 0 {
            return Ok(Vec::new());
        }
        if count < 3 {
            return Err(EditRefused::NeedsThree);
        }
        let (things, left_out) = self.movable_selected();
        if left_out {
            return Err(EditRefused::InTheText);
        }
        let places: Vec<Placed> = things.iter().map(|(_, placed)| *placed).collect();
        let spaced = distributed(&places, across).ok_or(EditRefused::NeedsThree)?;
        let spaced: Vec<(Thing, Placed)> = things.into_iter().map(|(thing, _)| thing).zip(spaced).collect();
        self.place_things(DISTRIBUTE, &spaced, EditRun::New)
    }

    /// Puts the one thing selected in front of the next, behind the one before, in front of
    /// all or behind all, as `order` says, by moving its own lines, as one step named "Change
    /// order", and returns the slides to show again. It stays selected. Nothing is written
    /// where it's there already. Refused for several things, an area, or a thing written in
    /// the slide's text.
    pub fn reorder_selected(&mut self, order: Order) -> Result<Vec<usize>, EditRefused> {
        // Typing is finished first, which can move the thing.
        self.finish_before_edit()?;
        let Some(selection) = self.selection.clone() else {
            return Ok(Vec::new());
        };
        if let Some(refused) = self.arrange_here().order_refused {
            return Err(refused);
        }
        let [Thing::Block(source)] = selection.things.as_slice() else {
            return Err(EditRefused::CouldNotBeMade);
        };
        let slide = selection.slide;
        let change = built(self.talk(), |talk| reorder(talk, slide, source, order))?.ok_or(EditRefused::CouldNotBeMade)?;
        if change.is_empty() {
            return Ok(Vec::new());
        }
        // Every other block keeps its bytes, and is found where the change takes it: the one
        // block on the slide that isn't one of those is the thing moved.
        let blocks = self.talk().slides.get(slide).map(|written| written.blocks.as_slice()).unwrap_or_default();
        let others: Vec<Range<usize>> = blocks
            .iter()
            .map(|block| block.range().clone())
            .filter(|range| range != source)
            .filter_map(|range| selection::moved(&range, &change))
            .collect();
        let depth = self.history.depth();
        let changed = self.make(ORDER, change, Holds::Slide(slide), slide, slide, None)?;
        if self.history.depth() > depth {
            let moved = self.talk().slides.get(slide).and_then(|written| {
                written.blocks.iter().map(|block| block.range().clone()).find(|range| !others.contains(range))
            });
            let after = moved.map(|range| selection::Selection { slide, things: vec![Thing::Block(range)] });
            self.set_selection(after.clone());
            self.history.select_mark_last(SelectMark { before: Some(selection), after });
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
