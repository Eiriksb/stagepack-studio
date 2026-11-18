//! Making an edit: checking what it does to the talk, taking it into the history, and undo
//! and redo.

use super::*;

/// Whether an edit to one slide joins the run of edits open in the history, so that one undo
/// takes the run back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditRun {
    /// The edit is a step of its own, which opens a run that later edits can join.
    New,
    /// The edit joins this run, while it's open. Once it isn't, the edit is a new step.
    Continue(RunId),
}

/// Which step of the history an edit's change goes into.
#[derive(Clone, Copy)]
pub(super) enum Join {
    /// A step of its own, which opens no run.
    Alone,
    /// A step of its own that opens a run, or the run's step, as [`EditRun`] says.
    Run(EditRun),
    /// The newest step, if [`History::fold`] can take it in; otherwise a step of its own.
    Last,
}

/// How many slides an edit is meant to leave, and which of them it may change.
#[derive(Clone, Copy)]
pub(super) enum Holds {
    /// The number of slides changes by this, and any slide may change.
    Count(isize),
    /// Only this slide may change, and the number of slides stays the same.
    Slide(usize),
    /// A slide is added at this index, and every other slide keeps its text, but for the line
    /// ending a slide that ended the file without one gains.
    Inserted(usize),
}

impl Document {
    /// Makes `change` to the talk's text, named `label` for Undo, and returns the slides to
    /// show again. `slides` is how the number of slides is meant to change: 0, 1 or -1.
    ///
    /// The change is refused, and nothing applied or recorded, if it would leave a slide that
    /// could be read as one that can't, or change the number of slides by anything else. An
    /// empty change does nothing.
    pub fn edit(
        &mut self,
        label: &'static str,
        change: Change,
        slides: isize,
    ) -> Result<Vec<usize>, EditRefused> {
        let current = self.current;
        self.make(label, change, Holds::Count(slides), current, current, None)
    }

    /// Makes `change` to slide `index`'s text, named `label` for Undo, joining the run `run`
    /// says, makes `index` the current slide, and returns the slides to show again.
    ///
    /// The change is refused, and nothing applied or recorded, unless every other slide's
    /// bytes are the same after it as before and the talk has as many slides, or if it would
    /// leave a slide that could be read as one that can't. An empty change does nothing.
    pub fn edit_slide(
        &mut self,
        label: &'static str,
        index: usize,
        change: Change,
        run: EditRun,
    ) -> Result<Vec<usize>, EditRefused> {
        self.make(label, change, Holds::Slide(index), index, index, Some(run))
    }

    /// The run of edits that's open in the history, which an edit can join with
    /// [`EditRun::Continue`].
    pub fn open_run(&self) -> Option<RunId> {
        self.history.open_run()
    }

    /// The change `build` works out from the talk as it is, for one of the commands. Text being
    /// typed on a slide is finished first, so the change is worked out from the talk as tidied.
    /// A command that's refused ends any typing in the notes.
    pub(super) fn build<T>(&mut self, build: impl FnOnce(&Talk) -> T) -> Result<T, EditRefused> {
        let built = self
            .finish_before_edit()
            .and_then(|_| built(self.talk(), build));
        if built.is_err() {
            self.typing_notes = None;
        }
        built
    }

    /// Makes `change` as [`Self::edit`] does, held to what `holds` says. `before` is the slide
    /// it's about in the text as it is, and `after` the one it's about once made, which becomes
    /// the current slide. With `run`, the step opens a run, or joins the one given; without, it
    /// opens none.
    ///
    /// Every edit made to the talk's text comes this way, or through [`Self::build`] first, but
    /// typing on a slide itself and what the history or the file gives back: undo, redo,
    /// recovering and reloading. Text being typed is finished before it; if that tidied the
    /// talk, `change`, worked out from the talk as it was, no longer fits it, and is refused.
    pub(super) fn make(
        &mut self,
        label: &'static str,
        change: Change,
        holds: Holds,
        before: usize,
        after: usize,
        run: Option<EditRun>,
    ) -> Result<Vec<usize>, EditRefused> {
        let empty = change.is_empty();
        let join = run.map_or(Join::Alone, Join::Run);
        let made = match self.finish_before_edit() {
            Ok(false) => self.try_make(label, change, holds, before, after, join, None),
            Ok(true) => Err(EditRefused::CouldNotBeMade),
            Err(refused) => Err(refused),
        };
        if made.is_err() {
            self.typing_notes = None;
        }
        if made.is_ok() && !empty {
            // The text being typed may have moved, or gone: it's typed into again once clicked.
            self.editing = None;
            self.held_color = None;
        }
        made
    }

    /// Finishes the text being typed on a slide, before another edit, and says whether tidying
    /// what typing left empty changed the talk. Emptied text that can't be taken away is left
    /// where it is, said, and doesn't stop the edit.
    pub(super) fn finish_before_edit(&mut self) -> Result<bool, EditRefused> {
        if self.editing.is_none() {
            return Ok(false);
        }
        match self.finish_text() {
            Ok((_, removed)) => Ok(removed.is_some()),
            Err(EditRefused::CouldNotTidy) => {
                self.message = Some(EditRefused::CouldNotTidy.to_string());
                Ok(false)
            }
            Err(refused) => Err(refused),
        }
    }

    /// [`Self::make`], but for ending the typing, into the step `join` says. With `check`, the
    /// talk the change gives is refused unless `check` holds for it, before anything is taken.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn try_make(
        &mut self,
        label: &'static str,
        change: Change,
        holds: Holds,
        before: usize,
        after: usize,
        join: Join,
        check: Option<&dyn Fn(&Talk) -> bool>,
    ) -> Result<Vec<usize>, EditRefused> {
        if change.is_empty() {
            return Ok(Vec::new());
        }
        let old = self.source();
        let old_count = self.slide_count();
        let broken_before = guarded(|| broken_bodies(self.talk()))?;
        // The candidate is worked out and read before the deck takes it, so a refusal, or a
        // panic, leaves the deck untouched.
        let (undo, text) = guarded(|| (change.inverse(old), change.apply(old)))?;
        let candidate = read_after(self.talk(), text)?;
        let slides = match holds {
            Holds::Count(slides) => slides,
            Holds::Slide(_) => 0,
            Holds::Inserted(_) => 1,
        };
        if old_count.checked_add_signed(slides) != Some(candidate.slides.len()) {
            return Err(EditRefused::WouldChangeTheSlides);
        }
        if let Holds::Slide(index) = holds
            && !others_alike(self.talk(), &candidate, index)
        {
            return Err(EditRefused::WouldChangeAnotherSlide);
        }
        if let Holds::Inserted(index) = holds
            && !others_kept(self.talk(), &candidate, index)
        {
            return Err(EditRefused::WouldChangeAnotherSlide);
        }
        if guarded(|| broken_bodies(&candidate))?
            .iter()
            .any(|body| !broken_before.contains(body))
        {
            return Err(EditRefused::WouldBreakASlide);
        }
        if let Some(check) = check
            && !guarded(|| check(&candidate))?
        {
            return Err(EditRefused::CouldNotBeTyped);
        }
        let old = old.to_string();
        let mut changed = self.deck.update_text(candidate);
        self.follow(&change);
        let joined = match join {
            Join::Run(EditRun::Continue(run)) => {
                self.history.extend(run, &old, self.deck.source())
            }
            Join::Last => self.history.fold(&old, self.deck.source()),
            Join::Run(EditRun::New) | Join::Alone => false,
        };
        if !joined {
            let opens_run = matches!(join, Join::Run(_));
            self.history.record(label, undo, before, after, opens_run);
        }
        // Typing in the notes goes on only through set_notes, which says so itself.
        self.typing_notes = None;
        self.show(after, &mut changed);
        Ok(changed)
    }

    /// Makes `index` the current slide after an edit, and adds it to `changed` if it isn't
    /// there: its text may be as it was, but it's now the slide to show.
    fn show(&mut self, index: usize, changed: &mut Vec<usize>) {
        let index = index.min(self.slide_count() - 1);
        if index != self.current {
            self.current = index;
            if !changed.contains(&index) {
                changed.push(index);
            }
        }
    }

    /// Takes the last edit back, and returns the slides to show again: `None` if there was
    /// nothing to undo. If reading the text it gives goes wrong, it's refused, and the talk
    /// and its history stay as they were.
    pub fn undo(&mut self) -> Result<Option<Vec<usize>>, EditRefused> {
        let Some(change) = self.history.next_undo() else {
            return Ok(None);
        };
        let talk = self.read_changed(change)?;
        let step = self
            .history
            .undo(self.deck.source())
            .expect("there's a step to undo");
        let changed = self.take_step(step.current, &step.change, talk);
        self.resume_text(step.mark, true);
        self.select_marked(step.selects.map(|selects| selects.before));
        self.keep_selection_with_typing();
        Ok(Some(changed))
    }

    /// Makes the last undone edit again, and returns the slides to show again: `None` if
    /// there was nothing to redo. It's refused as [`Self::undo`] is.
    pub fn redo(&mut self) -> Result<Option<Vec<usize>>, EditRefused> {
        let Some(change) = self.history.next_redo() else {
            return Ok(None);
        };
        let talk = self.read_changed(change)?;
        let step = self
            .history
            .redo(self.deck.source())
            .expect("there's a step to redo");
        let changed = self.take_step(step.current, &step.change, talk);
        self.resume_text(step.mark, false);
        self.select_marked(step.selects.map(|selects| selects.after));
        self.keep_selection_with_typing();
        Ok(Some(changed))
    }

    /// The talk as `change` leaves it.
    fn read_changed(&self, change: &Change) -> Result<Talk, EditRefused> {
        let text = guarded(|| change.apply(self.source()))?;
        read_after(self.talk(), text)
    }

    fn take_step(&mut self, current: usize, change: &Change, talk: Talk) -> Vec<usize> {
        let mut changed = self.deck.update_text(talk);
        self.follow(change);
        self.typing_notes = None;
        self.editing = None;
        self.held_color = None;
        self.show(current, &mut changed);
        changed
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// What Undo would take back, such as "Add slide".
    pub fn undo_label(&self) -> Option<&'static str> {
        self.history.undo_label()
    }

    /// What Redo would make again.
    pub fn redo_label(&self) -> Option<&'static str> {
        self.history.redo_label()
    }

    /// Whether the talk has edits that its file doesn't.
    /// A file added to the talk that a save would write counts too, as the file holds neither.
    pub fn has_unsaved_changes(&self) -> bool {
        !self.history.is_saved() || self.has_files_to_save()
    }

    /// Whether a save would write a file added to the talk. Worked out once for each version of
    /// the text and of the files added, as it's asked on every frame and keystroke.
    fn has_files_to_save(&self) -> bool {
        if self.pending.is_empty() {
            return false;
        }
        let key = (self.deck.text_version(), self.source().len(), self.added.stamp(), self.pending.len());
        if let Some((known, answer)) = self.files_to_save.get()
            && known == key
        {
            return answer;
        }
        self.files_to_save_walks.set(self.files_to_save_walks.get() + 1);
        let talk = self.talk();
        let answer = self.pending.iter().any(|path| talk_uses(talk, &*self.files, path));
        self.files_to_save.set(Some((key, answer)));
        answer
    }

    /// How many times whether there are files to save has been worked out, for its tests.
    #[doc(hidden)]
    pub fn files_to_save_walks(&self) -> u64 {
        self.files_to_save_walks.get()
    }

    /// Takes `text`, the working copy of edits that weren't saved, as the talk's text, and
    /// returns the slides to show again. It's taken as it is, whatever it does to the slides,
    /// and one undo gives back the text as it was. Nothing is written until the talk is saved.
    /// If reading it goes wrong, it's refused, and the talk stays as it was.
    pub fn recover(&mut self, text: String) -> Result<Vec<usize>, EditRefused> {
        self.replace_all(RECOVER_CHANGES, text)
    }

    /// Takes `text` as the talk's whole text, as one step named `label`, and returns the
    /// slides to show again. A whole-text step's ranges fit any text, so the steps before it
    /// can still be undone.
    pub(super) fn replace_all(
        &mut self,
        label: &'static str,
        text: String,
    ) -> Result<Vec<usize>, EditRefused> {
        let old = self.source().to_string();
        if text == old {
            return Ok(Vec::new());
        }
        let talk = read_after(self.talk(), text.clone())?;
        let change = Change::single(SourceEdit {
            range: 0..old.len(),
            text,
        });
        let undo = change.inverse(&old);
        // Taken from outside the app, so its pictures are looked at again too.
        let mut changed = self.deck.update_parsed(talk);
        self.follow(&one_edit(&old, self.deck.source()));
        let at = self.current;
        self.history.record(label, undo, at, at, false);
        self.typing_notes = None;
        self.editing = None;
        self.held_color = None;
        self.show(at, &mut changed);
        Ok(changed)
    }
}

/// What the slides of `talk` that can't be read hold, past any settings, without the blank
/// lines around it. A slide like that moves, or is copied, as it is, and is still one of these.
fn broken_bodies(talk: &Talk) -> Vec<String> {
    if talk.slides.iter().all(|slide| slide.error.is_none()) {
        return Vec::new();
    }
    split_slides(&talk.source)
        .iter()
        .zip(&talk.slides)
        .filter(|(_, slide)| slide.error.is_some())
        .map(|(raw, _)| talk.source[raw.body.clone()].trim().to_string())
        .collect()
}

/// Whether every slide of `after` but `index` has the bytes it had in `before`, given both
/// have as many slides.
pub(super) fn others_alike(before: &Talk, after: &Talk, index: usize) -> bool {
    fn text(talk: &Talk, at: usize) -> Option<&str> {
        talk.source.get(talk.slides[at].range.clone())
    }
    before.slides.len() == after.slides.len()
        && (0..before.slides.len())
            .filter(|&at| at != index)
            .all(|at| text(before, at) == text(after, at))
}

/// Whether every slide of `before` keeps its text in `after`, which has a slide added at
/// `index`, but for the line endings at a slide's end, which a slide that ended the file gains
/// once another follows it.
fn others_kept(before: &Talk, after: &Talk, index: usize) -> bool {
    fn text(talk: &Talk, at: usize) -> Option<&str> {
        let range = talk.slides.get(at)?.range.clone();
        Some(talk.source.get(range)?.trim_end_matches(['\r', '\n']))
    }
    after.slides.len() == before.slides.len() + 1
        && (0..before.slides.len()).all(|at| {
            let moved_to = if at < index { at } else { at + 1 };
            text(before, at).is_some() && text(before, at) == text(after, moved_to)
        })
}
