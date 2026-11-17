//! The thing selected on a slide: picking it, its settings, and taking it away.

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

    /// The thing selected on a slide, if any. It's the editor's view of the talk, not part of
    /// it: nothing of it is written, and it's no change to save or undo.
    pub fn selected(&self) -> Option<&Selected> {
        self.selected.as_ref()
    }

    /// Selects the thing on slide `slide` whose whole source is `source`, which must be one the
    /// slide's layout can pick, and makes `slide` the current one. Any typing ends first. Says
    /// whether there's such a thing.
    pub fn select(&mut self, slide: usize, source: Range<usize>) -> bool {
        if slide >= self.slide_count() || self.text_layout(slide).element(&source).is_none() {
            return false;
        }
        // Ending the typing can move the thing, and the selection moves with it.
        self.selected = Some(Selected { slide, source });
        self.picks += 1;
        if let Err(refused) = self.end_text() {
            self.say(refused);
        }
        let Some(slide) = self.selected.as_ref().map(|selected| selected.slide) else {
            return false;
        };
        self.go_to(slide);
        true
    }

    /// Lets go of the thing selected, if any.
    pub fn deselect(&mut self) {
        if self.selected.take().is_some() {
            self.picks += 1;
        }
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
        let selected = self.selected.as_ref() == Some(thing);
        let depth = self.history.depth();
        let changed = self.make(CHANGE_SETTING, change, Holds::Slide(slide), slide, slide, None)?;
        if selected && self.history.depth() > depth {
            let after = self.selected.as_ref().map(|selected| selected.source.clone());
            self.history.select_mark_last(SelectMark { before: Some(source), after });
        }
        Ok(changed)
    }

    /// The tag selected, if what's selected is one.
    pub fn selected_tag(&self) -> Option<&Tag> {
        let selected = self.selected.as_ref()?;
        guarded(|| find_tag(self.talk(), selected.slide, &selected.source)).ok().flatten()
    }

    /// Where the thing selected is drawn, if anything is.
    pub fn selected_box(&mut self) -> Option<ElementBox> {
        let selected = self.selected.clone()?;
        self.text_layout(selected.slide).element(&selected.source)
    }

    /// Takes the thing selected away, its own lines and one blank line beside them, as one step
    /// named "Delete", and returns the slides to show again. Undone, it's selected again. It's
    /// refused when what's around it would read differently without it.
    pub fn delete_selected(&mut self) -> Result<Vec<usize>, EditRefused> {
        self.remove_selected(DELETE)
    }

    /// Takes the thing selected away, as [`Self::delete_selected`] does, as one step named "Cut".
    pub fn cut_selected(&mut self) -> Result<Vec<usize>, EditRefused> {
        self.remove_selected(CUT)
    }

    fn remove_selected(&mut self, label: &'static str) -> Result<Vec<usize>, EditRefused> {
        // Typing is finished first, which can move the thing.
        self.finish_before_edit()?;
        let Some(Selected { slide, source }) = self.selected.clone() else {
            return Ok(Vec::new());
        };
        let change = built(self.talk(), |talk| delete_block(talk, slide, &source))?
            .ok_or(EditRefused::CouldNotBeRemoved)?;
        let depth = self.history.depth();
        let changed = self.make(label, change, Holds::Slide(slide), slide, slide, None)?;
        if self.history.depth() > depth {
            self.history.select_mark_last(SelectMark { before: Some(source), after: None });
        }
        Ok(changed)
    }

    /// Moves the selection along with `change`, just made to the talk's text: to where its
    /// thing's text went, or away if that went.
    pub(super) fn follow(&mut self, change: &Change) {
        // The box being typed in moves as the thing selected does; if it can't be followed,
        // typing there ends.
        if let Some(editing) = &mut self.editing {
            match editing.flow.moved(change) {
                Some(flow) => editing.flow = flow,
                None => self.editing = None,
            }
        }
        let Some(selected) = self.selected.take() else {
            return;
        };
        let talk = self.talk();
        self.selected = selection::moved(&selected.source, change).and_then(|source| {
            let slide = selection::slide_of(talk, &source)?;
            selection::has_block(talk, slide, &source).then_some(Selected { slide, source })
        });
    }

    /// Lets go of the thing selected if text is being typed on its slide outside it: typing
    /// in the thing keeps it selected, and typing on another slide leaves it be. Whatever starts
    /// or brings back typing, a click, an undo or a redo, ends with this.
    pub(super) fn keep_selection_with_typing(&mut self) {
        let (Some(editing), Some(selected)) = (&self.editing, &self.selected) else {
            return;
        };
        if editing.slide != selected.slide {
            return;
        }
        if editing.flow.tag_start() == Some(selected.source.start) {
            return;
        }
        let para = editing.edit.selection.head.para;
        let inside = self
            .text_flow()
            .is_some_and(|flow| selection::holds_para(&self.text_flows(self.talk(), editing.slide), flow, para, &selected.source));
        if !inside {
            self.selected = None;
        }
    }

    /// Selects what a step of the history marked, once it's undone or made again: the thing at
    /// `source`, or nothing.
    pub(super) fn select_marked(&mut self, source: Option<Option<Range<usize>>>) {
        let Some(source) = source else {
            return;
        };
        let talk = self.talk();
        let selected = source.and_then(|source| {
            let slide = selection::slide_of(talk, &source)?;
            selection::has_block(talk, slide, &source).then_some(Selected { slide, source })
        });
        if selected != self.selected {
            self.selected = selected;
            self.picks += 1;
        }
    }
}
