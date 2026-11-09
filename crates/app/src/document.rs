//! One open talk as a pure state machine: what the sidecar and the page said,
//! turned into the state the window shows and the effects it must apply.
//! No GPUI here, so every transition is tested headlessly.

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
    LoadCanvas { base_url: String },
    ShowMessage(String),
    CloseWindow,
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
            SidecarEvent::Saved { .. } => {
                self.dirty = false;
                if std::mem::take(&mut self.close_after_save) {
                    vec![Effect::CloseWindow]
                } else {
                    Vec::new()
                }
            }
            SidecarEvent::Conflict { message } | SidecarEvent::SaveFailed { message, .. } => {
                self.close_after_save = false;
                vec![Effect::ShowMessage(message)]
            }
            // Busy's baseUrl is empty while the other launch is still starting;
            // the message is the same either way.
            SidecarEvent::Busy { .. } => self.fail(BUSY_MESSAGE.into(), String::new()),
            SidecarEvent::Error { message, log } => self.fail(message, log),
            // An error is followed by `stopped` and an exit; it stays on screen.
            _ if self.is_failed() => Vec::new(),
            SidecarEvent::Stopped => {
                self.state = DocState::Closed;
                Vec::new()
            }
            SidecarEvent::Exited { code } => match code {
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
            },
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
            PageMessage::Edited { .. } => self.dirty = true,
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

    /// The close prompt's Save: the next `Saved` closes the window, and a
    /// failed save keeps it open.
    pub fn close_after_save(&mut self) {
        self.close_after_save = true;
    }

    pub fn closing_after_save(&self) -> bool {
        self.close_after_save
    }

    pub fn wants_close_prompt(&self) -> bool {
        self.dirty && matches!(self.state, DocState::Ready { .. })
    }

    /// Whether Save does anything: the talk is open and has unsaved edits.
    pub fn can_save(&self) -> bool {
        self.wants_close_prompt()
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
