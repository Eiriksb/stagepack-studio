//! Where the window's saving stands: whether a save is running, and what's to happen once it's
//! done. Each event says what the window should do about it, so that what happens in what
//! order can be checked without a window.

use crate::saving::SaveError;

/// What Save As says while a save is running.
pub const STILL_SAVING: &str = "The talk is still being saved. Try Save As again in a moment.";

/// Whether a save is running, and what's been asked for while it runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SaveFlow {
    #[default]
    Idle,
    Saving {
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

    /// Save As is asked for, once its place is chosen. While a save runs, it isn't made.
    pub fn ask_save_as(&mut self) -> Vec<Effect> {
        if self.is_saving() {
            return vec![Effect::Say(STILL_SAVING.to_string())];
        }
        self.save(false)
    }

    /// The window is asked to close, and the talk has changes `unsaved` or not. With changes
    /// unsaved it asks first.
    pub fn ask_close(&mut self, unsaved: bool) -> Vec<Effect> {
        if unsaved {
            vec![Effect::AskBeforeClosing]
        } else {
            vec![Effect::Close]
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

    /// A read of the talk's file has come in. While a save runs it's held back, and the file
    /// is read again once the save is done.
    pub fn read_arrived(&mut self) -> Vec<Effect> {
        match self {
            SaveFlow::Idle => vec![Effect::TakeRead],
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
            again,
            then_close,
            reload_after,
        } = std::mem::take(self)
        else {
            return Vec::new();
        };
        let mut effects = Vec::new();
        let (again, then_close) = match outcome {
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
                    again: false,
                    then_close: close,
                    reload_after: false,
                };
                vec![Effect::StartSave]
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
