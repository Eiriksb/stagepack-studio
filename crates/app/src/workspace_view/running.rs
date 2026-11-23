//! Running the talk's live terminals from the window: starting one once there's a yes for it,
//! ending them with the show, the talk and the window, and saying what became of them.

use super::*;

use slides_engine::TerminalLayer;

use crate::live_terminal::{COULDNT_START, LiveId, LiveTerminals, NOT_INSTALLED, TOO_MANY, find_helper, split_command, start_after_yes};

/// What came of trying to have a terminal run.
#[derive(Debug, PartialEq, Eq)]
pub enum LiveStep {
    /// It runs, or ran: nothing more to do now.
    Going,
    /// It has no yes yet: whoever has a window to ask in asks this command.
    Ask(Vec<String>),
    /// It doesn't run, and the still or the recording shows.
    Nothing,
}

/// How often the sessions are looked at, to say what became of them.
const WATCH_EVERY: Duration = Duration::from_millis(200);

impl WorkspaceView {
    /// The show of this talk, while one is on.
    pub fn current_show(&self) -> Option<Entity<Presentation>> {
        self.show.as_ref()?.upgrade()
    }

    /// The window's live terminals, shared with its show and its editor.
    pub fn live(&self) -> LiveTerminals {
        self.live.clone()
    }

    /// Has the terminal `terminal` of slide `slide` run, if it may: the helper must be found, the
    /// command must be one that can run, there must be a yes for exactly it, and no more than four
    /// may run. This never asks: a command with no yes yet comes back as [`LiveStep::Ask`], for
    /// whoever has a window to ask in, and what starts a program is only
    /// [`start_after_yes`](crate::live_terminal::start_after_yes), which looks again.
    pub fn try_start_live(&mut self, slide: usize, terminal: &TerminalLayer, cx: &mut Context<Self>) -> LiveStep {
        let (Some(document), Some(run), Some(source)) = (self.document.clone(), terminal.on.run.clone(), terminal.source.clone()) else {
            return LiveStep::Nothing;
        };
        let id = LiveId::new(source.clone(), run.clone());
        if self.live.get(&id).is_some() {
            return LiveStep::Going;
        }
        // One whose tag or screen is wrong never starts, and says why.
        if let Some(problem) = document.update(cx, |document, _| document.terminal_run_problem_on(slide, &source)) {
            if self.live.first_time(&problem) {
                self.say(problem, cx);
            }
            return LiveStep::Nothing;
        }
        let Some(helper) = find_helper() else {
            if self.live.first_time(NOT_INSTALLED) {
                self.say(NOT_INSTALLED, cx);
            }
            return LiveStep::Nothing;
        };
        let command = match split_command(&run) {
            Ok(command) => command,
            Err(why) => {
                if self.live.first_time(&why) {
                    self.say(why, cx);
                }
                return LiveStep::Nothing;
            }
        };
        // A program that isn't there, or a command that can't be run, is said, once.
        if let Err(why) = plan(document.read(cx).place(), &command) {
            let sentence = format!("{COULDNT_START}{why}");
            if self.live.first_time(&sentence) {
                self.say(sentence, cx);
            }
            return LiveStep::Nothing;
        }
        match self.run_decision(&command, cx) {
            Decision::Refused => LiveStep::Nothing,
            Decision::Ask => LiveStep::Ask(command),
            Decision::Run => {
                if self.live.is_full() {
                    if self.live.first_time(TOO_MANY) {
                        self.say(TOO_MANY, cx);
                    }
                    return LiveStep::Nothing;
                }
                let none = RunPermissions::load(PathBuf::new());
                let remembered = self.run_permissions.as_ref().unwrap_or(&none);
                let place = document.read(cx).place().clone();
                let size = (terminal.on.cols, terminal.on.rows);
                match start_after_yes(&place, &command, &self.run_yes, remembered, &helper, size) {
                    Ok(session) => {
                        self.live.add(id, session);
                        self.watch_live(cx);
                        cx.notify();
                        LiveStep::Going
                    }
                    Err(_) => LiveStep::Nothing,
                }
            }
        }
    }

    /// Has the terminal selected on the slide shown run, asking first if it has no yes. This is
    /// *Start*: it's the only thing in the editor that starts a program, and only when pressed.
    pub fn start_selected_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else { return };
        let (slide, terminal) = document.update(cx, |document, _| (document.current(), document.selected_terminal()));
        let Some(terminal) = terminal else { return };
        match self.try_start_live(slide, &terminal, cx) {
            LiveStep::Ask(command) => {
                if self.ask_to_run(command, window, cx) {
                    self.start_when_yes = Some((slide, terminal));
                }
            }
            LiveStep::Going | LiveStep::Nothing => {}
        }
        cx.notify();
    }

    /// Has `live` end when `window` is closed, however it's closed.
    pub(super) fn end_live_with_window(window: &Window, live: LiveTerminals, cx: &mut Context<Self>) -> Subscription {
        let id = window.window_handle().window_id();
        cx.on_window_closed(move |_, closed| {
            if closed == id {
                live.clone().end_all();
            }
        })
    }

    /// Ends every live terminal: the show ended, the talk or the window is closing, or another
    /// talk takes the window. What can't be ended says so on the status line.
    pub fn end_live(&mut self, cx: &mut Context<Self>) {
        self.start_when_yes = None;
        let said = self.live.end_all();
        for sentence in said {
            self.say(sentence, cx);
        }
        self.watch_live(cx);
        cx.notify();
    }

    /// Ends the terminals whose tag was taken away, or whose `run` was changed, by an edit.
    pub(super) fn sync_live(&mut self, cx: &mut Context<Self>) {
        if self.live.is_idle() {
            return;
        }
        let Some(document) = self.document.clone() else {
            return;
        };
        let tags: Vec<LiveId> = document.update(cx, |document, _| {
            (0..document.slide_count())
                .flat_map(|slide| document.terminals_on(slide))
                .filter_map(|terminal| Some(LiveId::new(terminal.source?, terminal.on.run?)))
                .collect()
        });
        self.live.keep_only(&tags);
        self.watch_live(cx);
    }

    /// Looks at the sessions now and then, while there are any, to say what became of them and to
    /// let go of those that are over.
    fn watch_live(&mut self, cx: &mut Context<Self>) {
        if self.live_watch.is_some() || !self.live.busy() {
            return;
        }
        self.live_watch = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(WATCH_EVERY).await;
                let more = this
                    .update(cx, |this, cx| {
                        let polled = this.live.poll();
                        for sentence in polled.sayings {
                            this.say(sentence, cx);
                        }
                        // A program that has ended is shown as such.
                        let more = this.live.busy();
                        if !more {
                            this.live_watch = None;
                        }
                        cx.notify();
                        more
                    })
                    .unwrap_or(false);
                if !more {
                    break;
                }
            }
        }));
    }
}

impl Drop for WorkspaceView {
    /// The window going, or another talk taking its place, ends what ran in it.
    fn drop(&mut self) {
        self.live.end_all();
    }
}
