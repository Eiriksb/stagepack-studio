//! One open talk as a pure state machine: what the sidecar and the page said,
//! turned into the state the window shows and the effects it must apply.
//! No GPUI here, so every transition is tested headlessly.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;

use studio_protocol::{PageMessage, SelectionInfo};
use studio_sidecar::SidecarEvent;

pub const MISMATCH_MESSAGE: &str = "This copy of the editor and its runtime don't match: ";
pub const BUSY_MESSAGE: &str =
    "This presentation is already open in Stagepack. Close it there, then open it here.";
pub const NOT_TRUSTED_MESSAGE: &str = "The presentation was not opened.";
/// The start of the message for a sidecar that exits without saying why.
pub const UNEXPECTED_EXIT_MESSAGE: &str = "Stagepack stopped unexpectedly";
/// How a `result` says the page and the app don't speak the same protocol.
const MISMATCH_RESULTS: [&str; 3] = [
    "Unsupported bridge version:",
    "Unknown command:",
    "Bad command:",
];
/// For a refusal that came without a reason.
const NO_REASON_MESSAGE: &str = "That could not be done.";

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
    /// Ask the page to apply its inline editor; say what went out with
    /// [`Document::finish_edit_sent`], and call
    /// [`Document::finish_edit_timed_out`] if no answer comes in time.
    FinishEdit,
    /// Send `save` to serve; call [`Document::save_sent`] first.
    SendSave,
}

/// What a Save or close that waits for the page does when the page has answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfterFinish {
    Save,
    Close,
}

/// A `finish-edit` in flight. Words being typed live in the page until the
/// editor is applied, and neither a native Save nor a close ever reaches it.
#[derive(Debug, Clone)]
struct FinishingEdit {
    /// `None` until [`Document::finish_edit_sent`].
    id: Option<i64>,
    then: AfterFinish,
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
    /// The last click hit the slide but nothing traceable.
    pub missed: bool,
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
    /// The page has said `ready` since the canvas last loaded.
    page_ready: bool,
    finishing: Option<FinishingEdit>,
    /// The page refused a `finish-edit` that a close was waiting for, and none
    /// has succeeded since. The next close doesn't ask it again: a page that
    /// refuses every time would keep the window open for good.
    finish_refused: bool,
    /// `error` messages from the page so far, and the text of the last.
    page_errors: u64,
    last_page_error: Option<String>,
    /// For each command at the page: `page_errors` when it went out.
    commands_out: HashMap<i64, u64>,
    /// How many commands the page has refused.
    refusals: u64,
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
            missed: false,
            close_after_save: false,
            edits: 0,
            saves_in_flight: VecDeque::new(),
            sidecar_ended: false,
            page_ready: false,
            finishing: None,
            finish_refused: false,
            page_errors: 0,
            last_page_error: None,
            commands_out: HashMap::new(),
            refusals: 0,
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
                self.page_ready = false;
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
            PageMessage::Ready { .. } => {
                self.selection = None;
                self.missed = false;
                self.page_ready = true;
            }
            PageMessage::Selection { selection, missed } => {
                self.selection = selection.clone();
                self.missed = *missed;
            }
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
            PageMessage::Error { message } => {
                self.page_errors += 1;
                self.last_page_error = Some(message.clone());
                return vec![Effect::ShowMessage(message.clone())];
            }
            PageMessage::Result { id, ok, error } => {
                return self.on_result(*id, *ok, error.as_deref());
            }
            PageMessage::Slide { .. } => {}
        }
        Vec::new()
    }

    /// A command's answer: says why it was refused, if it was, and lets go of
    /// a Save or close that was waiting for it.
    fn on_result(&mut self, id: i64, ok: bool, error: Option<&str>) -> Vec<Effect> {
        let errors_when_sent = self.commands_out.remove(&id);
        let mut effects = Vec::new();
        if !ok {
            let reason = error.unwrap_or(NO_REASON_MESSAGE);
            if MISMATCH_RESULTS.iter().any(|m| reason.starts_with(m)) {
                effects.extend(self.on_protocol_error(reason));
            } else {
                self.refusals += 1;
                // The page posts `error` first for what it reports; once is enough.
                let already_shown = errors_when_sent.is_some_and(|sent| self.page_errors > sent)
                    && self.last_page_error.as_deref() == Some(reason);
                if !already_shown {
                    effects.push(Effect::ShowMessage(reason.to_string()));
                }
            }
        }
        if self.finishing.as_ref().is_some_and(|f| f.id == Some(id)) {
            effects.extend(self.finish_edit_done(ok));
        }
        effects
    }

    /// A command went to the page under `id`.
    pub fn command_sent(&mut self, id: i64) {
        self.commands_out.insert(id, self.page_errors);
    }

    /// How many commands the page has refused, so far: each refusal puts the
    /// inspector's fields back on the selection.
    pub fn refusals(&self) -> u64 {
        self.refusals
    }

    /// Whether the page can be asked to apply its inline editor.
    fn page_up(&self) -> bool {
        self.page_ready && matches!(self.state, DocState::Ready { .. })
    }

    /// Ctrl+S or the Save button. With a page up, first whatever is being
    /// typed on it goes into the slide, so a talk still clean can have
    /// something to save once it has.
    pub fn request_save(&mut self) -> Vec<Effect> {
        if self.finishing.is_some() {
            return Vec::new();
        }
        if self.page_up() {
            self.finishing = Some(FinishingEdit {
                id: None,
                then: AfterFinish::Save,
            });
            return vec![Effect::FinishEdit];
        }
        self.save_effects()
    }

    /// The window was asked to close. Waits as [`Self::request_save`] does,
    /// then closes or asks; a close wins over a save that is still waiting.
    pub fn request_close(&mut self) -> Vec<Effect> {
        if let Some(finishing) = &mut self.finishing {
            finishing.then = AfterFinish::Close;
            return Vec::new();
        }
        if self.page_up() && !self.finish_refused {
            self.finishing = Some(FinishingEdit {
                id: None,
                then: AfterFinish::Close,
            });
            return vec![Effect::FinishEdit];
        }
        self.close_effects()
    }

    /// The `finish-edit` a [`Effect::FinishEdit`] asked for went out under `id`.
    pub fn finish_edit_sent(&mut self, id: i64) {
        self.command_sent(id);
        if let Some(finishing) = &mut self.finishing {
            finishing.id = Some(id);
        }
    }

    /// `id` has had its time and a dead page can't hold up a save or a close.
    /// A timer for a command already answered does nothing.
    pub fn finish_edit_timed_out(&mut self, id: i64) -> Vec<Effect> {
        if self.finishing.as_ref().is_some_and(|f| f.id == Some(id)) {
            self.finish_edit_done(true)
        } else {
            Vec::new()
        }
    }

    /// The page has answered, or isn't going to. A refused edit calls a close
    /// off: the reason is on screen and the window would take it away, and the
    /// next close doesn't ask the page again, so it goes through.
    fn finish_edit_done(&mut self, ok: bool) -> Vec<Effect> {
        if ok {
            self.finish_refused = false;
        }
        match self.finishing.take().map(|finishing| finishing.then) {
            Some(AfterFinish::Save) => self.save_effects(),
            Some(AfterFinish::Close) if ok => self.close_effects(),
            Some(AfterFinish::Close) => {
                self.finish_refused = true;
                Vec::new()
            }
            None => Vec::new(),
        }
    }

    fn save_effects(&self) -> Vec<Effect> {
        if self.can_save() {
            vec![Effect::SendSave]
        } else {
            Vec::new()
        }
    }

    fn close_effects(&self) -> Vec<Effect> {
        if self.wants_close_prompt() {
            vec![Effect::AskSaveChanges]
        } else {
            vec![Effect::CloseWindow]
        }
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

    /// There were unsaved edits and serve, which had them, has ended.
    pub fn changes_lost(&self) -> bool {
        self.dirty && self.sidecar_ended
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
