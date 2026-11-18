//! The things selected on a slide: picking them, their settings, and taking them away.

use super::*;

impl Document {
    /// The text being typed on a slide, if any.
    pub fn text(&self) -> Option<&EditingText> {
        self.editing.as_ref()
    }

    /// Which of its slide's `text_flows` the text being typed is, if it's in the talk yet: an
    /// empty region's isn't until something is typed in it.
    pub fn text_flow(&self) -> Option<usize> {
        let content = self.content_tags();
        let editing = self.editing.as_ref()?;
        let flows = guarded(|| flows_in(self.talk(), editing.slide, &content)).ok()?;
        editing.flow.find(&flows)
    }

    /// The one block selected on a slide, if that's all that's selected. It's the editor's view
    /// of the talk, not part of it: nothing of it is written, and it's no change to save or undo.
    pub fn selected(&self) -> Option<&Selected> {
        self.selected.as_ref()
    }

    /// Everything selected on a slide, if anything is: its blocks and its layout's areas.
    pub fn selection(&self) -> Option<&selection::Selection> {
        self.selection.as_ref()
    }

    /// Selects the thing on slide `slide` whose whole source is `source`, which must be one the
    /// slide's layout can pick, and makes `slide` the current one. Any typing ends first. Says
    /// whether there's such a thing.
    pub fn select(&mut self, slide: usize, source: Range<usize>) -> bool {
        self.pick(slide, vec![Thing::Block(source)])
    }

    /// Adds `thing` on slide `slide` to what's selected, or takes it away if it's selected
    /// already; a thing on another slide is selected alone. Says whether there's such a thing.
    pub fn select_also(&mut self, slide: usize, thing: Thing) -> bool {
        if !self.can_pick(slide, &thing) {
            return false;
        }
        let mut things = match &self.selection {
            Some(selection) if selection.slide == slide => selection.things.clone(),
            _ => Vec::new(),
        };
        match things.iter().position(|picked| *picked == thing) {
            Some(at) => {
                things.remove(at);
            }
            None => things.push(thing),
        }
        if things.is_empty() {
            self.deselect();
            return true;
        }
        self.pick(slide, things)
    }

    /// Selects area `region` of slide `slide`'s layout alone. Says whether its layout has one.
    pub fn select_region(&mut self, slide: usize, region: &str) -> bool {
        self.pick(slide, vec![Thing::Region(region.to_string())])
    }

    /// Selects every thing on the current slide, in the order they're written, but none of its
    /// layout's areas. Says whether there's anything to select.
    pub fn select_all_things(&mut self) -> bool {
        let slide = self.current;
        let things: Vec<Thing> = self.tab_order(slide).into_iter().filter(|thing| matches!(thing, Thing::Block(_))).collect();
        !things.is_empty() && self.pick(slide, things)
    }

    /// Every thing on slide `slide` in the order Tab goes through them: its things in the order
    /// they're written, an outer one before what's in it, and then its layout's areas.
    pub fn tab_order(&mut self, slide: usize) -> Vec<Thing> {
        if slide >= self.slide_count() {
            return Vec::new();
        }
        let layout = self.text_layout(slide);
        let mut blocks: Vec<Range<usize>> = layout.elements().iter().map(|element| element.source.clone()).collect();
        blocks.sort_by_key(|source| (source.start, std::cmp::Reverse(source.end)));
        blocks.dedup();
        let areas = layout.regions().iter().map(|region| Thing::Region(region.name.clone()));
        blocks.into_iter().map(Thing::Block).chain(areas).collect()
    }

    /// Whether `thing` is one slide `slide`'s layout can pick.
    fn can_pick(&mut self, slide: usize, thing: &Thing) -> bool {
        if slide >= self.slide_count() {
            return false;
        }
        let layout = self.text_layout(slide);
        match thing {
            Thing::Block(source) => layout.element(source).is_some(),
            Thing::Region(name) => layout.region(name).is_some(),
        }
    }

    /// Selects `things` on slide `slide`, each of which must be one its layout can pick, and
    /// makes `slide` the current one. Any typing ends first. Says whether they were selected.
    pub(super) fn pick(&mut self, slide: usize, mut things: Vec<Thing>) -> bool {
        let mut seen = Vec::new();
        things.retain(|thing| !seen.contains(thing) && {
            seen.push(thing.clone());
            true
        });
        if things.is_empty() || !things.iter().all(|thing| self.can_pick(slide, thing)) {
            return false;
        }
        // Ending the typing can move the things, and the selection moves with them.
        self.set_selection(Some(selection::Selection { slide, things }));
        self.picks += 1;
        if let Err(refused) = self.end_text() {
            self.say(refused);
        }
        let Some(slide) = self.selection.as_ref().map(|selection| selection.slide) else {
            return false;
        };
        self.go_to(slide);
        true
    }

    /// Lets go of everything selected, if anything is.
    pub fn deselect(&mut self) {
        if self.selection.is_some() {
            self.set_selection(None);
            self.picks += 1;
        }
    }

    /// Makes `selection` what's selected, without counting it as a pick.
    pub(super) fn set_selection(&mut self, selection: Option<selection::Selection>) {
        self.selected = selection
            .as_ref()
            .and_then(|selection| Some(Selected { slide: selection.slide, source: selection.one_block()?.clone() }));
        self.selection_slide = selection.as_ref().and_then(|selection| Some(self.talk().slides.get(selection.slide)?.range.clone()));
        self.selection = selection;
    }

    /// Changes each time a thing is selected or let go of, but not as the thing selected is
    /// followed through an edit: what's shown for one thing is never taken for another's.
    pub fn selection_pick(&self) -> u64 {
        self.picks
    }

    /// What the tag selected takes, from its component: `None` when nothing's selected, the
    /// thing isn't a tag, the talk has no such component, or it has no working build.
    pub fn schema_of_selected(&mut self) -> Option<ComponentSchema> {
        let name = self.selected_tag()?.name.clone();
        let deck = &mut self.deck;
        guarded(|| deck.component_schema(&name)).ok().flatten()
    }

    /// Sets attribute `name` of the tag selected to `value`, or takes it away with `None`, as
    /// one step named "Change setting", and returns the slides to show again. Only that
    /// attribute's bytes change. A value the same as the one there writes nothing and makes no
    /// step. It's refused when the value can't be written, as one holding both kinds of quote
    /// mark or a new line, or when the slide wouldn't read the same but for it.
    pub fn set_selected_attr(&mut self, name: &str, value: Option<&str>) -> Result<Vec<usize>, EditRefused> {
        // Typing is finished first, which can move the thing.
        self.finish_before_edit()?;
        let Some(selected) = self.selected.clone() else {
            return Ok(Vec::new());
        };
        self.set_attr_of(&selected, name, value)
    }

    /// Sets attribute `name` of `thing`, a tag on a slide, whether or not it's the one selected,
    /// as [`Self::set_selected_attr`] does. It's refused if there's no tag at exactly `thing`'s
    /// place, as once typing that ends first has moved it.
    pub fn set_attr_of(&mut self, thing: &Selected, name: &str, value: Option<&str>) -> Result<Vec<usize>, EditRefused> {
        self.finish_before_edit()?;
        let Selected { slide, source } = thing.clone();
        let change = built(self.talk(), |talk| set_tag_attr(talk, slide, &source, name, value))?
            .ok_or(EditRefused::CouldNotBeMade)?;
        self.make_setting(thing, change)
    }

    /// Sets each attribute of `values` on the tag selected, as [`Self::set_selected_attr`] does
    /// one, all in one step.
    pub fn set_selected_attrs(&mut self, values: &[(&str, Option<&str>)]) -> Result<Vec<usize>, EditRefused> {
        self.finish_before_edit()?;
        let Some(selected) = self.selected.clone() else {
            return Ok(Vec::new());
        };
        self.set_attrs_of(&selected, values)
    }

    /// Sets each attribute of `values` on `thing`, a tag on a slide at any depth, as
    /// [`Self::set_attr_of`] does one, all in one step. Each is set in turn, and each refused as
    /// that one would be.
    pub fn set_attrs_of(&mut self, thing: &Selected, values: &[(&str, Option<&str>)]) -> Result<Vec<usize>, EditRefused> {
        if let [(name, value)] = values {
            return self.set_attr_of(thing, name, *value);
        }
        self.finish_before_edit()?;
        let Selected { slide, source } = thing.clone();
        let mut tag = source;
        let mut after: Option<Talk> = None;
        for &(name, value) in values {
            let talk = after.as_ref().unwrap_or(self.talk());
            let step = built(talk, |talk| set_tag_attr(talk, slide, &tag, name, value))?.ok_or(EditRefused::CouldNotBeMade)?;
            let text = step.apply(&talk.source);
            // Only the tag's own text changed, after its start.
            tag.end = (tag.end + text.len()).checked_sub(talk.source.len()).ok_or(EditRefused::CouldNotBeMade)?;
            after = Some(read_after(talk, text)?);
        }
        let Some(after) = after else {
            return Ok(Vec::new());
        };
        let change = one_edit(&self.talk().source, &after.source);
        self.make_setting(thing, change)
    }

    /// Makes `change` to the settings of `thing` as one step named "Change setting".
    fn make_setting(&mut self, thing: &Selected, change: Change) -> Result<Vec<usize>, EditRefused> {
        let Selected { slide, source } = thing.clone();
        let selected = self.selected.as_ref() == Some(thing);
        let depth = self.history.depth();
        let changed = self.make(CHANGE_SETTING, change, Holds::Slide(slide), slide, slide, None)?;
        if selected && self.history.depth() > depth {
            let after = self.selected.as_ref().map(|selected| selected.source.clone());
            self.history.select_mark_last(SelectMark::blocks(slide, Some(source), after));
        }
        Ok(changed)
    }

    /// The tag selected, if what's selected is one.
    pub fn selected_tag(&self) -> Option<&Tag> {
        let selected = self.selected.as_ref()?;
        guarded(|| find_tag(self.talk(), selected.slide, &selected.source)).ok().flatten()
    }

    /// Where the thing selected is drawn, if one block is all that's selected.
    pub fn selected_box(&mut self) -> Option<ElementBox> {
        let selected = self.selected.clone()?;
        self.text_layout(selected.slide).element(&selected.source)
    }

    /// Where each thing selected is drawn, in the order they were picked: its rectangle before
    /// it's turned, and the degrees it's turned by. A thing the layout doesn't draw is left out.
    pub fn selected_boxes(&mut self) -> Vec<(Thing, Rect, f32)> {
        let Some(selection) = self.selection.clone() else {
            return Vec::new();
        };
        self.boxes_of(selection)
    }

    /// Where each thing of `selection` is drawn, as [`Self::selected_boxes`] says of what's
    /// selected.
    pub(super) fn boxes_of(&mut self, selection: selection::Selection) -> Vec<(Thing, Rect, f32)> {
        let layout = self.text_layout(selection.slide);
        selection
            .things
            .into_iter()
            .filter_map(|thing| {
                let (rect, rotate) = match &thing {
                    Thing::Block(source) => layout.element(source).map(|element| (element.rect, element.rotate))?,
                    Thing::Region(name) => layout.region(name).map(|region| (region.rect, region.rotate))?,
                };
                Some((thing, rect, rotate))
            })
            .collect()
    }

    /// The blocks selected, in the order they're written, but none inside another selected:
    /// what deleting, copying or duplicating the selection acts on. Refused when only areas are
    /// selected.
    pub(super) fn selected_blocks(&self) -> Result<Option<SlideBlocks>, EditRefused> {
        let Some(selection) = &self.selection else {
            return Ok(None);
        };
        let mut blocks: Vec<Range<usize>> = selection.blocks().cloned().collect();
        if blocks.is_empty() {
            return Err(EditRefused::AreaStays);
        }
        blocks.sort_by_key(|source| (source.start, std::cmp::Reverse(source.end)));
        let outer = blocks.clone();
        blocks.retain(|block| !outer.iter().any(|other| other != block && other.start <= block.start && block.end <= other.end));
        Ok(Some((selection.slide, blocks)))
    }

    /// Takes the things selected away, each one's own lines and one blank line beside them, as
    /// one step named "Delete", and returns the slides to show again. Undone, they're selected
    /// again. It's refused when what's around any of them would read differently without it,
    /// or when only a layout's areas are selected; the areas among things selected stay.
    pub fn delete_selected(&mut self) -> Result<Vec<usize>, EditRefused> {
        self.remove_selected(DELETE)
    }

    /// Takes the things selected away, as [`Self::delete_selected`] does, as one step named "Cut".
    pub fn cut_selected(&mut self) -> Result<Vec<usize>, EditRefused> {
        self.remove_selected(CUT)
    }

    fn remove_selected(&mut self, label: &'static str) -> Result<Vec<usize>, EditRefused> {
        // Typing is finished first, which can move the things.
        self.finish_before_edit()?;
        let Some((slide, blocks)) = self.selected_blocks()? else {
            return Ok(Vec::new());
        };
        let before = self.selection.clone();
        let change = built(self.talk(), |talk| {
            removed_at_once(talk, slide, &blocks).or_else(|| each_in_turn(talk, &blocks, |talk, block| delete_block(talk, slide, block)))
        })?
        .ok_or(EditRefused::CouldNotBeRemoved)?;
        let depth = self.history.depth();
        let changed = self.make(label, change, Holds::Slide(slide), slide, slide, None)?;
        if self.history.depth() > depth {
            self.history.select_mark_last(SelectMark { before, after: self.selection.clone() });
        }
        Ok(changed)
    }

    /// Sets the word `name` on the settings line of area `region` of slide `slide` to `value`,
    /// or takes it away with `None`, as one step named "Change setting", and returns the slides
    /// to show again. Only that word's bytes change; a value the same as the one there writes
    /// nothing and makes no step.
    pub fn set_region_value(&mut self, slide: usize, region: &str, name: &str, value: Option<&str>) -> Result<Vec<usize>, EditRefused> {
        self.set_region_values(CHANGE_SETTING, slide, region, &[(name, value)])
    }

    /// Puts area `region` of slide `slide` back where its layout has it, by taking its place
    /// and its turn off its settings line, as one step named "Put back". Nothing is written, and
    /// no step made, when it's there already.
    pub fn put_back_region(&mut self, slide: usize, region: &str) -> Result<Vec<usize>, EditRefused> {
        let values = PLACE_WORDS.map(|name| (name, None));
        self.set_region_values(PUT_BACK, slide, region, &values)
    }

    fn set_region_values(&mut self, label: &'static str, slide: usize, region: &str, values: &[(&str, Option<&str>)]) -> Result<Vec<usize>, EditRefused> {
        self.finish_before_edit()?;
        let edits: Vec<ValueEdit> =
            values.iter().map(|&(name, value)| ValueEdit { target: Target::Region(region.to_string()), name, value }).collect();
        let change = built(self.talk(), |talk| set_values(talk, slide, &edits))?.ok_or(EditRefused::CouldNotBeMade)?;
        let before = self.selection.clone().filter(|selection| selection.slide == slide);
        let depth = self.history.depth();
        let changed = self.make(label, change, Holds::Slide(slide), slide, slide, None)?;
        if before.is_some() && self.history.depth() > depth {
            self.history.select_mark_last(SelectMark { before, after: self.selection.clone() });
        }
        Ok(changed)
    }

    /// Moves the selection along with `change`, just made to the talk's text: each block to
    /// where its text went, or away if that went, and each area with its slide.
    pub(super) fn follow(&mut self, change: &Change) {
        // The box being typed in moves as the thing selected does; if it can't be followed,
        // typing there ends.
        if let Some(editing) = &mut self.editing {
            match editing.flow.moved(change) {
                Some(flow) => editing.flow = flow,
                None => self.editing = None,
            }
        }
        let Some(old) = self.selection.take() else {
            return;
        };
        let anchor = self.selection_slide.take();
        let talk = self.talk();
        // A slide whose settings were written or taken away at its start is found by its end.
        let slide_now = anchor
            .and_then(|range| selection::moved(&range, change).or_else(|| selection::moved(&(range.end.saturating_sub(1)..range.end), change)))
            .and_then(|range| selection::slide_of(talk, &range));
        let moved: Vec<Option<Thing>> = old
            .things
            .into_iter()
            .map(|thing| match thing {
                Thing::Block(source) => selection::moved(&source, change).map(Thing::Block),
                region => Some(region),
            })
            .collect();
        let followed = picked_on(talk, moved, slide_now);
        let followed = self.without_lost_areas(followed);
        self.set_selection(followed);
    }

    /// `selection` without the areas its slide's layout no longer has, as once the slide is given
    /// another layout: nothing, if that leaves nothing.
    fn without_lost_areas(&mut self, selection: Option<selection::Selection>) -> Option<selection::Selection> {
        let selection = selection?;
        if !selection.things.iter().any(|thing| matches!(thing, Thing::Region(_))) {
            return Some(selection);
        }
        let layout = self.text_layout(selection.slide);
        let things: Vec<Thing> = selection
            .things
            .into_iter()
            .filter(|thing| match thing {
                Thing::Region(name) => layout.region(name).is_some(),
                Thing::Block(_) => true,
            })
            .collect();
        (!things.is_empty()).then_some(selection::Selection { slide: selection.slide, things })
    }

    /// Lets go of what's selected if text is being typed on its slide outside it: typing in the
    /// one block selected keeps it selected, and typing on another slide leaves it be. Whatever
    /// starts or brings back typing, a click, an undo or a redo, ends with this.
    pub(super) fn keep_selection_with_typing(&mut self) {
        let (Some(editing), Some(selection)) = (&self.editing, &self.selection) else {
            return;
        };
        if editing.slide != selection.slide {
            return;
        }
        let Some(source) = selection.one_block().cloned() else {
            self.set_selection(None);
            return;
        };
        if editing.flow.tag_start() == Some(source.start) {
            return;
        }
        let para = editing.edit.selection.head.para;
        let inside = self
            .text_flow()
            .is_some_and(|flow| selection::holds_para(&self.text_flows(self.talk(), editing.slide), flow, para, &source));
        if !inside {
            self.set_selection(None);
        }
    }

    /// Selects what a step of the history marked, once it's undone or made again: its things
    /// that are still there, or nothing.
    pub(super) fn select_marked(&mut self, marked: Option<Option<selection::Selection>>) {
        let Some(marked) = marked else {
            return;
        };
        let talk = self.talk();
        let selection = marked.and_then(|marked| {
            let slide = (marked.slide < talk.slides.len()).then_some(marked.slide);
            picked_on(talk, marked.things.into_iter().map(Some).collect(), slide)
        });
        // An area the slide's layout no longer has isn't selected.
        let selection = self.without_lost_areas(selection);
        if selection != self.selection {
            self.set_selection(selection);
            self.picks += 1;
        }
    }
}

/// A slide, and blocks on it by their whole source.
pub(super) type SlideBlocks = (usize, Vec<Range<usize>>);

/// The words of an area's settings line that place and turn it.
const PLACE_WORDS: [&str; 5] = ["x", "y", "w", "h", "rotate"];

/// What's selected on `talk` of `things`, each as it is now or gone: the blocks still there, on
/// the slide the first of them is on, and the areas, if that's `slide`, the slide the areas are
/// on. Nothing if none are left.
fn picked_on(talk: &Talk, things: Vec<Option<Thing>>, slide: Option<usize>) -> Option<selection::Selection> {
    let block_slide = |source: &Range<usize>| selection::slide_of(talk, source).filter(|&at| selection::has_block(talk, at, source));
    let on = things
        .iter()
        .flatten()
        .find_map(|thing| match thing {
            Thing::Block(source) => block_slide(source),
            Thing::Region(_) => None,
        })
        .or(slide)?;
    let things: Vec<Thing> = things
        .into_iter()
        .flatten()
        .filter(|thing| match thing {
            Thing::Block(source) => block_slide(source) == Some(on),
            Thing::Region(_) => slide == Some(on),
        })
        .collect();
    (!things.is_empty()).then_some(selection::Selection { slide: on, things })
}

/// The one change that makes `each`, for every range of `ranges`, to `talk`, in turn from the
/// last written to the first, each worked out on the talk as the ones before it leave it. A
/// single range gets the change `each` gives for it. `None` if any of them is.
pub(super) fn each_in_turn(talk: &Talk, ranges: &[Range<usize>], each: impl Fn(&Talk, &Range<usize>) -> Option<Change>) -> Option<Change> {
    if let [range] = ranges {
        return each(talk, range);
    }
    let mut left: Vec<Range<usize>> = ranges.to_vec();
    left.sort_by_key(|range| std::cmp::Reverse(range.start));
    let mut now = talk.clone();
    while !left.is_empty() {
        let range = left.remove(0);
        let change = each(&now, &range)?;
        made_in_turn(&mut now, &change, &mut left)?;
    }
    Some(one_edit(&talk.source, &now.source))
}

/// The one change that takes every one of `blocks`, each written at the top level of slide
/// `slide` on lines of its own with a blank line after it, away at once, each as
/// [`delete_block`] takes it from the slide as it is, where those edits don't overlap and the
/// slide then reads as exactly its other blocks: one reading of the talk, however many there
/// are. Each is worked out on the slide's own text, read alone, as the slide it is. `None`
/// otherwise, for them to be taken in turn.
fn removed_at_once(talk: &Talk, slide: usize, blocks: &[Range<usize>]) -> Option<Change> {
    let top = &talk.slides.get(slide)?.blocks;
    // Each on lines of its own with a blank line after it, so that the blank line taking it
    // away takes is its own, and never one a block beside it would take too.
    let source = talk.source.as_bytes();
    let own_lines =
        |block: &Range<usize>| (block.start == 0 || source[block.start - 1] == b'\n') && source.get(block.end).is_none_or(|next| matches!(next, b'\n' | b'\r'));
    let blank_after = |block: &Range<usize>| {
        let rest = &talk.source[block.end..];
        let Some(rest) = rest.strip_prefix("\r\n").or_else(|| rest.strip_prefix('\n')) else {
            return false;
        };
        rest.find('\n').is_some_and(|end| rest[..end].trim().is_empty())
    };
    if blocks.len() < 2 || !blocks.iter().all(|block| own_lines(block) && blank_after(block) && top.iter().any(|written| written.range() == block)) {
        return None;
    }
    let (alone, at) = slide_alone(talk, slide)?;
    let shifted = |range: &Range<usize>| range.start - at..range.end - at;
    let mut edits: Vec<SourceEdit> = Vec::new();
    for block in blocks {
        let change = delete_block(&alone, 0, &shifted(block))?;
        edits.extend(change.edits.into_iter().map(|edit| SourceEdit { range: edit.range.start + at..edit.range.end + at, text: edit.text }));
    }
    edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
    if edits.windows(2).any(|pair| pair[0].range.end > pair[1].range.start) {
        return None;
    }
    let change = Change { edits };
    let after = reparse(talk, change.apply(&talk.source));
    let texts = |talk: &Talk, blocks: &mut dyn Iterator<Item = Range<usize>>| -> Option<Vec<String>> {
        blocks.map(|range| talk.source.get(range).map(str::to_string)).collect()
    };
    let kept = texts(talk, &mut top.iter().map(|block| block.range().clone()).filter(|range| !blocks.contains(range)))?;
    let left = texts(&after, &mut after.slides.get(slide)?.blocks.iter().map(|block| block.range().clone()))?;
    (after.slides.len() == talk.slides.len() && kept == left).then_some(change)
}

/// Slide `slide` of `talk` read alone from its own text, and how far into the talk's text that
/// starts, where it reads as the one slide with the same blocks, each that much earlier.
fn slide_alone(talk: &Talk, slide: usize) -> Option<(Talk, usize)> {
    let written = talk.slides.get(slide)?;
    let at = written.range.start;
    let alone = parse_talk(talk.source.get(written.range.clone())?);
    let shifted = |range: &Range<usize>| range.start - at..range.end - at;
    let same = alone.slides.len() == 1
        && alone.slides[0].error.is_none()
        && alone.slides[0].blocks.len() == written.blocks.len()
        && alone.slides[0].blocks.iter().zip(&written.blocks).all(|(alone, written)| *alone.range() == shifted(written.range()));
    same.then_some((alone, at))
}

/// Makes `change`, one of several made in turn, to `now`, which is read again, and moves each
/// of `ranges` along with it. `None` if any of them goes, which leaves `ranges` as they were.
pub(super) fn made_in_turn(now: &mut Talk, change: &Change, ranges: &mut Vec<Range<usize>>) -> Option<()> {
    let text = change.apply(&now.source);
    *now = reparse(now, text);
    *ranges = ranges.iter().map(|range| selection::moved(range, change)).collect::<Option<_>>()?;
    Some(())
}
