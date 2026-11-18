//! Ending typing, tidying away text it left with no words, and taking it up again after an
//! undo or a redo.

use super::*;

impl Document {
    /// Stops typing on the slide, and returns the slides to show again. Everything typed is in
    /// the talk already; text left with no words at all is taken away, so that its region
    /// offers to add text again. Typing ends even if that's refused.
    pub fn end_text(&mut self) -> Result<Vec<usize>, EditRefused> {
        self.finish_text().map(|(changed, _)| changed)
    }

    /// Leaves the talk tidy while typing goes on, as before a save, a show or an export, and
    /// returns the slides to show again. Text that typing left with no words at all is taken
    /// away, as [`Self::end_text`] does; typing then goes on in its region, empty again, or
    /// ends if the region still has other text. Any other typing goes on as it was.
    pub fn settle_text(&mut self) -> Result<Vec<usize>, EditRefused> {
        let content = self.content_tags();
        // What the system was composing isn't text yet, and isn't kept.
        self.compose(None);
        let given = self.give_held_first();
        let Some(editing) = self.editing.clone() else {
            return Ok(given);
        };
        if editing.flow.new_box().is_some() {
            // A new box nothing is typed in yet is dropped, so that nothing of it is saved,
            // shown or exported.
            self.editing = None;
            return Ok(given);
        }
        let (changed, removed) = self.tidy(&editing)?;
        let changed = joined(given, changed);
        if removed.is_some() {
            let region = editing.flow.region().unwrap_or_default();
            let flows = guarded(|| flows_in(self.talk(), editing.slide, &content)).ok();
            self.editing = flows
                .filter(|flows| !region_has_text(flows, region))
                .map(|_| EditingText {
                    session: editing.session,
                    ..EditingText::in_empty_region(editing.slide, region)
                });
        }
        Ok(changed)
    }

    /// [`Self::end_text`], which also gives the slide and the flow it took away, if it did.
    pub(super) fn finish_text(&mut self) -> Result<EndedText, EditRefused> {
        let given = self.give_held_first();
        let Some(editing) = self.editing.take() else {
            return Ok((given, None));
        };
        let (changed, removed) = self.tidy(&editing)?;
        Ok((joined(given, changed), removed))
    }

    /// Takes away the region text `editing` is typing in, if it has no words at all, and gives
    /// the slides to show again, and the slide and flow taken away. That's no step of its own:
    /// it joins the step before it typed in the same time of typing, where the history can
    /// take it, and if that leaves the step doing nothing, the step goes. A box of text is
    /// never taken away, as an empty box would be no text at all.
    fn tidy(&mut self, editing: &EditingText) -> Result<EndedText, EditRefused> {
        let content = self.content_tags();
        let slide = editing.slide;
        if editing.flow.region().is_none() || slide >= self.slide_count() {
            return Ok((Vec::new(), None));
        }
        let (change, index, count) = {
            let talk = self.talk();
            let flows = guarded(|| flows_in(talk, slide, &content))?;
            let Some(index) = editing.flow.find(&flows) else {
                return Ok((Vec::new(), None));
            };
            if !holds_no_words(&flows[index].paras) {
                return Ok((Vec::new(), None));
            }
            (write_flow(talk, slide, &flows[index], &[], &content), index, flows.len())
        };
        let join = match editing.run {
            Some(run) if self.history.is_open(run) => Join::Run(EditRun::Continue(run)),
            _ if self
                .history
                .last_mark()
                .is_some_and(|mark| mark.session == editing.session) =>
            {
                Join::Last
            }
            _ => Join::Alone,
        };
        // Where this typing's own step put the text there, taking it away gives back the
        // slide's bytes from before it, so that the two together are no change at all.
        let change = match join {
            Join::Alone => change,
            _ => self.before_typed_in(editing, index).or(change),
        };
        let change = change.ok_or(EditRefused::CouldNotTidy)?;
        if change.is_empty() {
            return Ok((Vec::new(), None));
        }
        let depth = self.history.depth();
        let emptied = |candidate: &Talk| flows_in(candidate, slide, &content).len() + 1 == count;
        let changed = self
            .make_text(TYPING, slide, change, join, &emptied)
            .map_err(|_| EditRefused::CouldNotTidy)?;
        // Made again, the step leaves nothing to type in.
        let now = self.history.depth();
        if now > depth {
            self.history.mark_last(TextMark {
                slide,
                flow: editing.flow.clone(),
                before: editing.edit.selection,
                after: None,
                flow_after: None,
                session: editing.session,
            });
        } else if now == depth
            && let Some(last) = self.history.last_mark_mut()
        {
            last.after = None;
        }
        Ok((changed, Some((slide, index))))
    }

    /// The change back to the talk from before the newest step, if that step is what put
    /// flow `index` of `editing`'s slide there: before it, the slide had all its other text as
    /// it is now, and not that flow.
    fn before_typed_in(&self, editing: &EditingText, index: usize) -> Option<Change> {
        let content = self.content_tags();
        let undo = self.history.next_undo()?;
        let slide = editing.slide;
        let alike = guarded(|| {
            let before = reparse(self.talk(), undo.apply(self.source()));
            let mut now = flows_in(self.talk(), slide, &content);
            now.remove(index);
            let then = flows_in(&before, slide, &content);
            before.slides.len() == self.slide_count()
                && others_alike(self.talk(), &before, slide)
                && then.len() == now.len()
                && then
                    .iter()
                    .zip(&now)
                    .all(|(then, now)| same_place(&then.place, &now.place) && then.paras == now.paras)
        });
        alike.unwrap_or(false).then(|| undo.clone())
    }

    /// Takes `edit` as the text being typed, in the run `run`.
    pub(super) fn keep_typed(&mut self, edit: TextEdit, run: Option<RunId>) {
        if let Some(editing) = &mut self.editing {
            editing.edit = edit;
            editing.run = run;
        }
    }

    /// Makes `change` to slide `slide`, typed on it, into the step `join` says, unless the talk
    /// it gives fails `check`. The text being typed goes on.
    pub(super) fn make_text(
        &mut self,
        label: &'static str,
        slide: usize,
        change: Change,
        join: Join,
        check: &dyn Fn(&Talk) -> bool,
    ) -> Result<Vec<usize>, EditRefused> {
        self.try_make(label, change, Holds::Slide(slide), slide, slide, join, Some(check))
            .map_err(|_| EditRefused::CouldNotBeTyped)
    }

    /// Notes where the step just made by typing in run `run` was typed: on a step of its own,
    /// `mark`; on a run's step, how far the typing has gone.
    pub(super) fn note_step(&mut self, run: EditRun, mark: TextMark) {
        match (run, self.history.open_run()) {
            (EditRun::Continue(run), Some(open)) if run == open => {
                if let Some(last) = self.history.last_mark_mut() {
                    last.after = mark.after;
                }
            }
            (_, Some(_)) => self.history.mark_last(mark),
            // The run came back to where it started, and its step went with it.
            (_, None) => {}
        }
    }

    /// Goes back to typing where `mark` says, after an undo, with `before`, or a redo of the
    /// step typed there. A step that took the text away leaves nothing to type in once it's
    /// made again, and text found where only an empty region was is someone else's: in both,
    /// typing stays ended.
    pub(super) fn resume_text(&mut self, mark: Option<TextMark>, before: bool) {
        let content = self.content_tags();
        let Some(mark) = mark else {
            return;
        };
        let selection = if before { Some(mark.before) } else { mark.after };
        let Some(selection) = selection else {
            return;
        };
        let mut mark = mark;
        if !before && let Some(flow) = mark.flow_after.take() {
            mark.flow = flow;
        }
        let slide = mark.slide;
        if slide >= self.slide_count() {
            return;
        }
        let Ok(flows) = guarded(|| flows_in(self.talk(), slide, &content)) else {
            return;
        };
        self.editing = match (mark.flow.find(&flows), mark.flow.region()) {
            (Some(index), _) => {
                let edit = TextEdit::new(flows[index].paras.clone())
                    .move_to(selection.anchor, false)
                    .move_to(selection.head, true);
                Some(EditingText {
                    slide,
                    flow: mark.flow,
                    edit,
                    run: None,
                    session: mark.session,
                    composing: None,
                })
            }
            (None, Some(region)) if !region_has_text(&flows, region) => Some(EditingText {
                session: mark.session,
                ..EditingText::in_empty_region(slide, region)
            }),
            (None, None) => mark
                .flow
                .tag_start()
                .filter(|&start| tag_starting_at(self.talk(), slide, start).is_some_and(|tag| waits_for_text(tag, &content)))
                .map(|start| EditingText {
                    session: mark.session,
                    ..EditingText::in_empty_tag(slide, start)
                }),
            _ => None,
        };
    }
}

/// What ending the text being typed did: the slides to show again, and the slide and flow it
/// took away, if it emptied one.
type EndedText = (Vec<usize>, Option<(usize, usize)>);

/// Whether `a` and `b` are the same kind of place: the same region, both boxes, or the same
/// cell of a table, whose ranges move with the text before them.
fn same_place(a: &slides_format::FlowPlace, b: &slides_format::FlowPlace) -> bool {
    use slides_format::FlowPlace::{Cell, Region, Tag};
    match (a, b) {
        (Region(a), Region(b)) => a == b,
        (Tag(_), Tag(_)) => true,
        (Cell { row: r1, column: c1, .. }, Cell { row: r2, column: c2, .. }) => (r1, c1) == (r2, c2),
        _ => false,
    }
}
