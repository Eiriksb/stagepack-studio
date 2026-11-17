//! The edits made to an open talk, so they can be undone and done again. Each step is a
//! [`Change`] to the talk's text, and is kept as the change that takes it back.

use std::collections::VecDeque;

use slides_format::{Change, SourceEdit};

use crate::selection::SelectMark;
use crate::text_editing::TextMark;

/// The most steps kept.
const MAX_STEPS: usize = 1_000;
/// The most bytes of edit text kept, over all the steps.
const MAX_BYTES: usize = 64 * 1024 * 1024;

/// What undoing or redoing a step does: the change to make to the talk's text as it is now,
/// and the slide to show afterwards.
pub struct Step {
    pub change: Change,
    pub current: usize,
    /// Where the step's text was typed, if it was typed on a slide.
    pub mark: Option<TextMark>,
    /// What the step selects, if it's about a thing on a slide.
    pub selects: Option<SelectMark>,
}

struct Entry {
    label: &'static str,
    /// The change that makes this step's undo or redo, whichever comes next for it.
    change: Change,
    /// The slide to show once it's undone.
    before: usize,
    /// The slide to show once it's done again.
    after: usize,
    /// Where its text was typed, if it was typed on a slide.
    mark: Option<TextMark>,
    /// What it selects once undone or made again, if it's about a thing on a slide.
    selects: Option<SelectMark>,
}

impl Entry {
    fn bytes(&self) -> usize {
        size_of(&self.change)
    }
}

/// A run of edits that join one step, such as notes typed a key at a time. It's open from the
/// step [`History::record`] opens it with until anything else happens to the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunId(u64);

fn size_of(change: &Change) -> usize {
    change.edits.iter().map(|edit| edit.text.len()).sum()
}

pub struct History {
    undo: VecDeque<Entry>,
    /// The next to redo last.
    redo: Vec<Entry>,
    /// How many of the oldest steps have gone.
    dropped: usize,
    /// How many steps from the start the saved text is, while it can still be reached.
    saved: Option<usize>,
    bytes: usize,
    /// The run the last step is, while later edits can still join it.
    run: Option<RunId>,
    /// The number the next run gets.
    next_run: u64,
}

impl Default for History {
    fn default() -> History {
        History::new()
    }
}

impl History {
    /// A history with nothing in it, of a talk as it was saved.
    pub fn new() -> History {
        History {
            undo: VecDeque::new(),
            redo: Vec::new(),
            dropped: 0,
            saved: Some(0),
            bytes: 0,
            run: None,
            next_run: 0,
        }
    }

    /// How many steps from the start the text is now.
    fn position(&self) -> usize {
        self.dropped + self.undo.len()
    }

    /// Records an edit, named `label`, by the change `undo` that takes it back. `before` is
    /// the slide to show once it's undone, and `after` the one to show once it's made again.
    /// Whatever could be redone can't be any more. An empty change records nothing.
    ///
    /// Any run that was open ends. With `open_run`, the step starts a run of its own, which
    /// later edits can join with [`Self::extend`].
    pub fn record(
        &mut self,
        label: &'static str,
        undo: Change,
        before: usize,
        after: usize,
        open_run: bool,
    ) -> Option<RunId> {
        self.run = None;
        if undo.is_empty() {
            return None;
        }
        if !self.redo.is_empty() {
            for entry in self.redo.drain(..) {
                self.bytes -= entry.bytes();
            }
            if self.saved.is_some_and(|saved| saved > self.position()) {
                self.saved = None;
            }
        }
        let entry = Entry {
            label,
            change: undo,
            before,
            after,
            mark: None,
            selects: None,
        };
        self.bytes += entry.bytes();
        self.undo.push_back(entry);
        self.trim();
        if !open_run || self.undo.is_empty() {
            return None;
        }
        let run = RunId(self.next_run);
        self.next_run += 1;
        self.run = Some(run);
        Some(run)
    }

    /// Takes an edit that turned the text `before` into `after` into the step of `run`, so
    /// that one undo takes both back, and says whether it did: only while the run is open.
    /// An edit that brings the text back to where the run started takes its step away, and
    /// ends the run.
    pub fn extend(&mut self, run: RunId, before: &str, after: &str) -> bool {
        if self.run != Some(run) {
            return false;
        }
        let joined = self.join_last(before, after);
        if !joined {
            self.run = None;
        }
        joined
    }

    /// Takes an edit that turned the text `before` into `after` into the newest step, whether
    /// or not its run is open, and says whether it did. It doesn't while there's something to
    /// redo, or when the newest step leads to the saved text, which would no longer be one a
    /// step leads to. An edit that brings the text back to where the step started takes the
    /// step away. Any run that was open ends.
    pub fn fold(&mut self, before: &str, after: &str) -> bool {
        if !self.redo.is_empty() || self.saved == Some(self.position()) {
            return false;
        }
        self.run = None;
        self.join_last(before, after)
    }

    /// Takes an edit from `before` to `after` into the newest step, if there is one.
    fn join_last(&mut self, before: &str, after: &str) -> bool {
        let Some(last) = self.undo.back_mut() else {
            return false;
        };
        let start = last.change.apply(before);
        let joined = one_edit(after, &start);
        self.bytes -= last.bytes();
        if joined.is_empty() {
            self.undo.pop_back();
            self.run = None;
        } else {
            last.change = joined;
            self.bytes += size_of(&last.change);
        }
        self.trim();
        true
    }

    /// Notes where the newest step's text was typed, so that undoing or redoing it goes back
    /// to typing there.
    pub fn mark_last(&mut self, mark: TextMark) {
        if let Some(last) = self.undo.back_mut() {
            last.mark = Some(mark);
        }
    }

    /// Notes what the newest step selects once it's undone or made again.
    pub fn select_mark_last(&mut self, selects: SelectMark) {
        if let Some(last) = self.undo.back_mut() {
            last.selects = Some(selects);
        }
    }

    /// Where the newest step's text was typed, if it was typed on a slide.
    pub fn last_mark(&self) -> Option<&TextMark> {
        self.undo.back().and_then(|entry| entry.mark.as_ref())
    }

    /// How many steps from the start the text is now: one more after a step is recorded, one
    /// fewer once a step is taken away.
    pub fn depth(&self) -> usize {
        self.position()
    }

    /// Where the newest step's text was typed, if it was typed on a slide.
    pub fn last_mark_mut(&mut self) -> Option<&mut TextMark> {
        self.undo.back_mut().and_then(|entry| entry.mark.as_mut())
    }

    /// Whether `run` is open, so that edits can still join it.
    pub fn is_open(&self, run: RunId) -> bool {
        self.run == Some(run)
    }

    /// The run that's open, if any.
    pub fn open_run(&self) -> Option<RunId> {
        self.run
    }

    /// The change [`Self::undo`] would give, without taking the step back.
    pub fn next_undo(&self) -> Option<&Change> {
        self.undo.back().map(|entry| &entry.change)
    }

    /// The change [`Self::redo`] would give, without making the step again.
    pub fn next_redo(&self) -> Option<&Change> {
        self.redo.last().map(|entry| &entry.change)
    }

    /// Takes the last step back. `source` is the talk's text now, which the step's change
    /// is made to.
    pub fn undo(&mut self, source: &str) -> Option<Step> {
        self.run = None;
        let mut entry = self.undo.pop_back()?;
        let change = std::mem::replace(&mut entry.change, Change { edits: Vec::new() });
        self.bytes -= size_of(&change);
        entry.change = change.inverse(source);
        self.bytes += entry.bytes();
        let current = entry.before;
        let (mark, selects) = (entry.mark.clone(), entry.selects.clone());
        self.redo.push(entry);
        self.trim();
        Some(Step { change, current, mark, selects })
    }

    /// Makes the last step undone again. `source` is the talk's text now.
    pub fn redo(&mut self, source: &str) -> Option<Step> {
        self.run = None;
        let mut entry = self.redo.pop()?;
        let change = std::mem::replace(&mut entry.change, Change { edits: Vec::new() });
        self.bytes -= size_of(&change);
        entry.change = change.inverse(source);
        self.bytes += entry.bytes();
        let current = entry.after;
        let (mark, selects) = (entry.mark.clone(), entry.selects.clone());
        self.undo.push_back(entry);
        self.trim();
        Some(Step { change, current, mark, selects })
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// What Undo would take back.
    pub fn undo_label(&self) -> Option<&'static str> {
        self.undo.back().map(|entry| entry.label)
    }

    /// What Redo would make again.
    pub fn redo_label(&self) -> Option<&'static str> {
        self.redo.last().map(|entry| entry.label)
    }

    /// The text as it is now is the one saved. A run ends here, so that the saved text stays
    /// one a step leads to.
    pub fn mark_saved(&mut self) {
        self.run = None;
        self.saved = Some(self.position());
    }

    /// The saved text is one no step leads to: the file was written or changed from elsewhere.
    pub fn forget_saved(&mut self) {
        self.run = None;
        self.saved = None;
    }

    /// Whether the text is as it was last saved.
    pub fn is_saved(&self) -> bool {
        self.saved == Some(self.position())
    }

    /// Lets the oldest steps go until what's kept is within bounds.
    fn trim(&mut self) {
        while self.undo.len() + self.redo.len() > MAX_STEPS || self.bytes > MAX_BYTES {
            let Some(oldest) = self.undo.pop_front() else {
                break;
            };
            self.bytes -= oldest.bytes();
            self.dropped += 1;
            if self.undo.is_empty() {
                // The run's own step has gone.
                self.run = None;
            }
            if self.saved.is_some_and(|saved| saved < self.dropped) {
                self.saved = None;
            }
        }
    }
}

/// The change that turns `from` into `to` with a single edit: what lies between all they
/// start and end with alike.
pub(crate) fn one_edit(from: &str, to: &str) -> Change {
    let mut start = from
        .bytes()
        .zip(to.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !from.is_char_boundary(start) {
        start -= 1;
    }
    let most = from.len().min(to.len()) - start;
    let mut end = from
        .bytes()
        .rev()
        .zip(to.bytes().rev())
        .take(most)
        .take_while(|(a, b)| a == b)
        .count();
    while !from.is_char_boundary(from.len() - end) {
        end -= 1;
    }
    if start == from.len() - end && start == to.len() - end {
        return Change { edits: Vec::new() };
    }
    Change::single(SourceEdit {
        range: start..from.len() - end,
        text: to[start..to.len() - end].to_string(),
    })
}
