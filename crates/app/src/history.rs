//! The edits made to an open talk, so they can be undone and done again. Each step is a
//! [`Change`] to the talk's text, and is kept as the change that takes it back.

use std::collections::VecDeque;

use slides_format::{Change, SourceEdit};

/// The most steps kept.
const MAX_STEPS: usize = 1_000;
/// The most bytes of edit text kept, over all the steps.
const MAX_BYTES: usize = 64 * 1024 * 1024;

/// What undoing or redoing a step does: the change to make to the talk's text as it is now,
/// and the slide to show afterwards.
pub struct Step {
    pub change: Change,
    pub current: usize,
}

struct Entry {
    label: &'static str,
    /// The change that makes this step's undo or redo, whichever comes next for it.
    change: Change,
    /// The slide to show once it's undone.
    before: usize,
    /// The slide to show once it's done again.
    after: usize,
}

impl Entry {
    fn bytes(&self) -> usize {
        size_of(&self.change)
    }
}

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
        }
    }

    /// How many steps from the start the text is now.
    fn position(&self) -> usize {
        self.dropped + self.undo.len()
    }

    /// Records an edit, named `label`, by the change `undo` that takes it back. `before` is
    /// the slide to show once it's undone, and `after` the one to show once it's made again.
    /// Whatever could be redone can't be any more. An empty change records nothing.
    pub fn record(&mut self, label: &'static str, undo: Change, before: usize, after: usize) {
        if undo.is_empty() {
            return;
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
        };
        self.bytes += entry.bytes();
        self.undo.push_back(entry);
        self.trim();
    }

    /// Takes an edit made straight after the last step, which turned the text `before` into
    /// `after`, into that step, so that one undo takes both back.
    pub fn extend_last(&mut self, before: &str, after: &str) {
        if self.is_saved() {
            // The text it was saved as is the one between the two, and no step leads there now.
            self.saved = None;
        }
        let Some(last) = self.undo.back_mut() else {
            return;
        };
        let start = last.change.apply(before);
        let joined = one_edit(after, &start);
        self.bytes -= last.bytes();
        if joined.is_empty() {
            self.undo.pop_back();
        } else {
            last.change = joined;
            self.bytes += size_of(&last.change);
        }
        self.trim();
    }

    /// Takes the last step back. `source` is the talk's text now, which the step's change
    /// is made to.
    pub fn undo(&mut self, source: &str) -> Option<Step> {
        let mut entry = self.undo.pop_back()?;
        let change = std::mem::replace(&mut entry.change, Change { edits: Vec::new() });
        self.bytes -= size_of(&change);
        entry.change = change.inverse(source);
        self.bytes += entry.bytes();
        let current = entry.before;
        self.redo.push(entry);
        self.trim();
        Some(Step { change, current })
    }

    /// Makes the last step undone again. `source` is the talk's text now.
    pub fn redo(&mut self, source: &str) -> Option<Step> {
        let mut entry = self.redo.pop()?;
        let change = std::mem::replace(&mut entry.change, Change { edits: Vec::new() });
        self.bytes -= size_of(&change);
        entry.change = change.inverse(source);
        self.bytes += entry.bytes();
        let current = entry.after;
        self.undo.push_back(entry);
        self.trim();
        Some(Step { change, current })
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

    /// The text as it is now is the one saved.
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.position());
    }

    /// The saved text is one no step leads to: the file was written or changed from elsewhere.
    pub fn forget_saved(&mut self) {
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
            if self.saved.is_some_and(|saved| saved < self.dropped) {
                self.saved = None;
            }
        }
    }
}

/// The change that turns `from` into `to` with a single edit: what lies between all they
/// start and end with alike.
fn one_edit(from: &str, to: &str) -> Change {
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
