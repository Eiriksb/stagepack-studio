//! One open talk as a pure state machine: what the sidecar and the page said,
//! turned into the state the window shows and the effects it must apply.
//! No GPUI here, so every transition is tested headlessly.

use std::collections::VecDeque;
use std::path::PathBuf;

use studio_protocol::{PageMessage, SelectionInfo};
use studio_sidecar::SidecarEvent;

pub const MISMATCH_MESSAGE: &str = "This copy of the editor and its runtime don't match: ";
pub const BUSY_MESSAGE: &str =
    "This presentation is already open in Stagepack. Close it there, then open it here.";
pub const NOT_TRUSTED_MESSAGE: &str = "The presentation was not opened.";
/// The start of the message for a sidecar that exits without saying why.
pub const UNEXPECTED_EXIT_MESSAGE: &str = "Stagepack stopped unexpectedly";

#[derive(Debug, Clone, PartialEq)]
pub enum DocState {
    Opening,
    AwaitingTrust { question: String },
    Ready { base_url: String },
    Failed { message: String, log: String },
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    AskTrust(String),
    LoadCanvas {
        base_url: String,
    },
    ShowMessage(String),
    CloseWindow,
    /// A save-then-close's save didn't hold every edit: ask again.
    AskSaveChanges,
}

#[derive(Debug, Clone)]
pub struct Document {
    pub path: PathBuf,
    pub title: String,
    pub state: DocState,
    pub dirty: bool,
    pub selection: Option<SelectionInfo>,
    pub can_undo: bool,
    pub can_redo: bool,
    /// The close prompt's Save was chosen: close once the save lands.
    close_after_save: bool,
    /// How many `edited` messages the page has sent.
    edits: u64,
    /// For each save sent and not yet answered, oldest first: `edits` when it
    /// was sent. Serve packs the folder as it is when it gets there, so an
    /// edit that lands after the save was sent may not be in it.
    saves_in_flight: VecDeque<u64>,
    /// Serve said `stopped` or exited. Its work folder, and with it any
    /// unsaved edit, is gone.
    sidecar_ended: bool,
}

impl Document {
    pub fn new(path: PathBuf) -> Self {
        Self {
            title: file_name(&path),
            path,
            state: DocState::Opening,
            dirty: false,
            selection: None,
            can_undo: false,
            can_redo: false,
            close_after_save: false,
            edits: 0,
            saves_in_flight: VecDeque::new(),
            sidecar_ended: false,
        }
    }

    /// The talk's file name, as the user knows it.
    pub fn file_name(&self) -> String {
        file_name(&self.path)
    }

    pub fn on_sidecar(&mut self, e: SidecarEvent) -> Vec<Effect> {
        match e {
            SidecarEvent::Unpacked { title, .. } => {
                if !title.trim().is_empty() {
                    self.title = title;
                }
                Vec::new()
            }
            SidecarEvent::Confirm { question } => {
                self.state = DocState::AwaitingTrust {
                    question: question.clone(),
                };
                vec![Effect::AskTrust(question)]
            }
            SidecarEvent::Ready { base_url, .. } => {
                self.state = DocState::Ready {
                    base_url: base_url.clone(),
                };
                vec![Effect::LoadCanvas { base_url }]
            }
            SidecarEvent::Saved { .. } => self.on_saved(),
            SidecarEvent::Conflict { message } | SidecarEvent::SaveFailed { message, .. } => {
                self.saves_in_flight.pop_front();
                self.close_after_save = false;
                vec![Effect::ShowMessage(message)]
            }
            // Busy's baseUrl is empty while the other launch is still starting;
            // the message is the same either way.
            SidecarEvent::Busy { .. } => self.fail(BUSY_MESSAGE.into(), String::new()),
            SidecarEvent::Error { message, log } => self.fail(message, log),
            SidecarEvent::Stopped => {
                self.sidecar_gone();
                // An error is followed by `stopped` and an exit; it stays on screen.
                if !self.is_failed() {
                    self.state = DocState::Closed;
                }
                Vec::new()
            }
            SidecarEvent::Exited { code } => {
                self.sidecar_gone();
                if self.is_failed() {
                    return Vec::new();
                }
                match code {
                    Some(0) => {
                        self.state = DocState::Closed;
                        Vec::new()
                    }
                    Some(3) => self.fail(NOT_TRUSTED_MESSAGE.into(), String::new()),
                    // Serve said `stopped` first; how it exited adds nothing.
                    _ if self.state == DocState::Closed => Vec::new(),
                    Some(code) => self.fail(
                        format!("{UNEXPECTED_EXIT_MESSAGE} (exit code {code})."),
                        String::new(),
                    ),
                    None => self.fail(format!("{UNEXPECTED_EXIT_MESSAGE}."), String::new()),
                }
            }
        }
    }

    pub fn on_page(&mut self, m: &PageMessage) -> Vec<Effect> {
        match m {
            // A new page, after a load or Slidev's own reload: nothing is selected.
            PageMessage::Ready { .. } => self.selection = None,
            PageMessage::Selection { selection } => self.selection = selection.clone(),
            PageMessage::History {
                can_undo, can_redo, ..
            } => {
                self.can_undo = *can_undo;
                self.can_redo = *can_redo;
            }
            PageMessage::Edited { .. } => {
                self.edits += 1;
                self.dirty = true;
            }
            PageMessage::Error { message } => return vec![Effect::ShowMessage(message.clone())],
            PageMessage::Slide { .. } | PageMessage::Result { .. } => {}
        }
        Vec::new()
    }

    pub fn on_protocol_error(&mut self, e: &str) -> Vec<Effect> {
        self.fail(format!("{MISMATCH_MESSAGE}{e}"), String::new())
    }

    /// The trust question was answered. Serve says `ready` next, or exits 3.
    pub fn answered_trust(&mut self) -> Vec<Effect> {
        if matches!(self.state, DocState::AwaitingTrust { .. }) {
            self.state = DocState::Opening;
        }
        Vec::new()
    }

    /// The close prompt's Save: the `Saved` that holds every edit closes the
    /// window, one that misses an edit asks again, and a failed save keeps it.
    pub fn close_after_save(&mut self) {
        self.close_after_save = true;
    }

    /// A save command went to serve. Call it before sending, so a failed
    /// send can be answered with a `SaveFailed` like any other refused save.
    pub fn save_sent(&mut self) {
        self.saves_in_flight.push_back(self.edits);
    }

    pub fn closing_after_save(&self) -> bool {
        self.close_after_save
    }

    /// Whether closing must ask first: there are unsaved edits and serve
    /// still has them. That holds in `Failed` too: a protocol error or an
    /// unreadable line leaves serve running, and stopping it deletes the edits.
    pub fn wants_close_prompt(&self) -> bool {
        self.dirty && !self.sidecar_ended
    }

    /// Whether Save does anything: the same condition, in `Failed` as well.
    pub fn can_save(&self) -> bool {
        self.wants_close_prompt()
    }

    /// A save came back. The talk is clean only if no edit came after the
    /// save was sent. A save-then-close closes only over a clean talk; while
    /// another save is still out, that one may hold the rest.
    fn on_saved(&mut self) -> Vec<Effect> {
        let covered = self.saves_in_flight.pop_front().unwrap_or(self.edits);
        self.dirty = self.edits != covered;
        if !self.close_after_save {
            return Vec::new();
        }
        if !self.dirty {
            self.close_after_save = false;
            vec![Effect::CloseWindow]
        } else if self.saves_in_flight.is_empty() {
            self.close_after_save = false;
            vec![Effect::AskSaveChanges]
        } else {
            Vec::new()
        }
    }

    /// Serve stopped or exited: no save can land any more.
    fn sidecar_gone(&mut self) {
        self.sidecar_ended = true;
        self.close_after_save = false;
        self.saves_in_flight.clear();
    }

    fn is_failed(&self) -> bool {
        matches!(self.state, DocState::Failed { .. })
    }

    fn fail(&mut self, message: String, log: String) -> Vec<Effect> {
        self.state = DocState::Failed { message, log };
        self.close_after_save = false;
        Vec::new()
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}
