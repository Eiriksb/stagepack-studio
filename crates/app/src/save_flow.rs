//! Where the window's saving stands: whether a save is running, and what's to happen once it's
//! done. Each event says what the window should do about it, so that what happens in what
//! order can be checked without a window.

use crate::saving::SaveError;

/// What Save As says while a save is running.
pub const STILL_SAVING: &str = "The talk is still being saved. Try Save As again in a moment.";
/// What the window says when it was to close once saved, but no place to save in was chosen.
pub const NOT_SAVED_STILL_OPEN: &str = "The presentation wasn't saved, so its window was left open.";

/// Whether a save is running, and what's been asked for while it runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SaveFlow {
    #[default]
    Idle,
    /// An untitled talk is to be saved once a place for it is chosen.
    Choosing {
        /// The window closes once the talk is saved.
        then_close: bool,
    },
    Saving {
        /// This is the talk's first save, which gives it a place. If it fails, the talk still
        /// has none, so nothing follows it but what's said.
        first: bool,
        /// Another save was asked for while this one ran. It's made once this one is done, if
        /// the talk still has changes unsaved.
        again: bool,
        /// The window closes once the talk is saved.
        then_close: bool,
        /// A read of the file came in while this ran. It's read again once the save is done:
        /// what came in may be older than what the save wrote.
        reload_after: bool,
    },
}

/// How a save came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved,
    /// Nothing was written: the file changed outside the app since it was last read.
    ChangedOutside,
    /// Nothing was written, for the reason given.
    Failed(String),
}

/// What the dialog asking before the window closes was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAnswer {
    Save,
    DontSave,
    Cancel,
}

/// What the window should do, in the order given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Start the save that was asked for.
    StartSave,
    /// Take the read of the file that just came in.
    TakeRead,
    /// Read the file again, and take what it holds.
    Reload,
    /// Take what the save found in the file as a change from outside: with the talk's own
    /// changes unsaved, it's asked about.
    AskAboutOutsideChange,
    /// Close the window, once the changes to its working copies are made.
    Close,
    /// Ask whether to save before the window closes.
    AskBeforeClosing,
    /// Ask where to save an untitled talk, and in which form.
    AskWhereToSave,
    /// Let the unsaved changes and their working copies go.
    LetGo,
    /// Put this on the status line.
    Say(String),
}

impl SaveFlow {
    /// Whether a save is running.
    pub fn is_saving(&self) -> bool {
        matches!(self, SaveFlow::Saving { .. })
    }

    /// Save is asked for. While a save runs, another follows it.
    pub fn ask_save(&mut self) -> Vec<Effect> {
        self.save(false)
    }

    /// Save is asked for on an untitled talk, which has nowhere to be saved until a place is
    /// chosen. While its first save runs, another follows it, once it has a place.
    pub fn ask_save_untitled(&mut self) -> Vec<Effect> {
        self.save_untitled(false)
    }

    /// Save As is asked for, once its place is chosen. While a save runs, it isn't made.
    pub fn ask_save_as(&mut self) -> Vec<Effect> {
        self.place_chosen(false)
    }

    /// A place to save the talk in has been chosen, by Save As or for an untitled talk's Save,
    /// for the talk's `first` save or not. While a save runs, it isn't made.
    pub fn place_chosen(&mut self, first: bool) -> Vec<Effect> {
        let then_close = match *self {
            SaveFlow::Saving { .. } => return vec![Effect::Say(STILL_SAVING.to_string())],
            SaveFlow::Choosing { then_close } => then_close,
            SaveFlow::Idle => false,
        };
        *self = SaveFlow::Idle;
        let effects = self.save(then_close);
        if let SaveFlow::Saving { first: was_first, .. } = self {
            *was_first = first;
        }
        effects
    }

    /// No place to save the talk in was chosen. If the window was to close once it was saved,
    /// it stays open, and says why.
    pub fn place_not_chosen(&mut self) -> Vec<Effect> {
        let SaveFlow::Choosing { then_close } = *self else {
            return Vec::new();
        };
        *self = SaveFlow::Idle;
        if then_close {
            vec![Effect::Say(NOT_SAVED_STILL_OPEN.to_string())]
        } else {
            Vec::new()
        }
    }

    /// The window is asked to close, and the talk has changes `unsaved` or not. With changes
    /// unsaved it asks first. With none, but a save running, it closes once that's done: a write
    /// cut short by the app going would leave a file half made.
    pub fn ask_close(&mut self, unsaved: bool) -> Vec<Effect> {
        if unsaved {
            return vec![Effect::AskBeforeClosing];
        }
        match self {
            SaveFlow::Saving { then_close, .. } => {
                *then_close = true;
                Vec::new()
            }
            SaveFlow::Idle | SaveFlow::Choosing { .. } => vec![Effect::Close],
        }
    }

    /// The dialog asking before the window closes is answered. With Save, the window closes
    /// once the talk is saved, edits made while a save runs among them.
    pub fn answer_close(&mut self, answer: CloseAnswer) -> Vec<Effect> {
        match answer {
            CloseAnswer::Save => self.save(true),
            CloseAnswer::DontSave => vec![Effect::LetGo, Effect::Close],
            CloseAnswer::Cancel => Vec::new(),
        }
    }

    /// The dialog asking before the window closes is answered, for an untitled talk. With Save,
    /// where to save it is asked first, and the window closes once it's saved.
    pub fn answer_close_untitled(&mut self, answer: CloseAnswer) -> Vec<Effect> {
        match answer {
            CloseAnswer::Save => self.save_untitled(true),
            answer => self.answer_close(answer),
        }
    }

    /// Asks where to save an untitled talk, closing the window once it's saved if `close`. While
    /// its first save runs, another follows it, as for any talk.
    fn save_untitled(&mut self, close: bool) -> Vec<Effect> {
        match self {
            SaveFlow::Idle => {
                *self = SaveFlow::Choosing { then_close: close };
                vec![Effect::AskWhereToSave]
            }
            // Asked already: the dialogs are up, and their answer makes the save.
            SaveFlow::Choosing { then_close } => {
                *then_close |= close;
                Vec::new()
            }
            SaveFlow::Saving { .. } => self.save(close),
        }
    }

    /// A read of the talk's file has come in. While a save runs it's held back, and the file
    /// is read again once the save is done.
    pub fn read_arrived(&mut self) -> Vec<Effect> {
        match self {
            SaveFlow::Idle | SaveFlow::Choosing { .. } => vec![Effect::TakeRead],
            SaveFlow::Saving { reload_after, .. } => {
                *reload_after = true;
                Vec::new()
            }
        }
    }

    /// The running save is done, as `outcome` says, and the talk has changes `still_unsaved`
    /// after it or not.
    pub fn finished(&mut self, outcome: SaveOutcome, still_unsaved: bool) -> Vec<Effect> {
        let SaveFlow::Saving {
            first,
            again,
            then_close,
            reload_after,
        } = std::mem::take(self)
        else {
            return Vec::new();
        };
        let mut effects = Vec::new();
        let (again, then_close) = match outcome {
            // Still untitled: there's nowhere to save it until a place is chosen again.
            SaveOutcome::Failed(problem) if first => {
                effects.push(Effect::Say(problem));
                (false, false)
            }
            SaveOutcome::Saved if then_close && !still_unsaved => return vec![Effect::Close],
            // Edited while it saved: those are saved too before the window goes.
            SaveOutcome::Saved if then_close => (true, true),
            SaveOutcome::Saved => (again, false),
            SaveOutcome::ChangedOutside => {
                effects.push(Effect::Say(
                    SaveError::ChangedOutside(String::new()).to_string(),
                ));
                // Asked about as any change from outside is: nothing has been written.
                effects.push(Effect::AskAboutOutsideChange);
                (false, false)
            }
            SaveOutcome::Failed(problem) => {
                effects.push(Effect::Say(problem));
                (again, false)
            }
        };
        if reload_after {
            effects.push(Effect::Reload);
        }
        if again && still_unsaved {
            effects.extend(self.save(then_close));
        }
        effects
    }

    /// Starts a save, or has one follow the running one. With `close`, the window closes
    /// once the talk is saved.
    fn save(&mut self, close: bool) -> Vec<Effect> {
        match self {
            SaveFlow::Idle => {
                *self = SaveFlow::Saving {
                    first: false,
                    again: false,
                    then_close: close,
                    reload_after: false,
                };
                vec![Effect::StartSave]
            }
            // The place is still being chosen, and the save is made once it is.
            SaveFlow::Choosing { then_close } => {
                *then_close |= close;
                Vec::new()
            }
            SaveFlow::Saving {
                again, then_close, ..
            } => {
                *again = true;
                *then_close |= close;
                Vec::new()
            }
        }
    }
}
