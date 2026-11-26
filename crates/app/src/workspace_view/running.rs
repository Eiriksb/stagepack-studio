//! Running the talk's live terminals from the window: starting one once there's a yes for it,
//! ending them with the show, the talk and the window, and saying what became of them.

use super::*;

use slides_engine::{BUTTON_IN_NOTHING, TerminalLayer, duplicate_id_notice};

use crate::live_terminal::{
    COULDNT_START, LiveId, LiveTerminals, NOT_INSTALLED, TOO_MANY, TalkFolder, TypeKey, find_helper, may_type, spelled_plainly, split_command, start_after_yes,
    type_key,
};

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

/// What's said of a button whose terminal can't start as 4 already run.
pub const BUTTON_TERMINAL_FULL: &str = "This button's terminal can't start: 4 are already running.";
/// What's said of a button whose terminal's command was said no to.
pub const BUTTON_TERMINAL_REFUSED: &str = "This button's terminal can't start: its command was said no to.";
/// What's said of a button whose terminal's program has ended.
pub const BUTTON_TERMINAL_ENDED: &str = "This button's terminal has ended, so nothing is typed.";
/// What's said of a button that types into a terminal the slide doesn't show yet.
pub const BUTTON_TERMINAL_NOT_SHOWN: &str = "This button's terminal isn't on the slide yet.";
/// What's said of a button whose text was said no to.
pub const BUTTON_TEXT_REFUSED: &str = "This button's text was said no to, so it isn't typed.";

/// Why a terminal doesn't run.
enum NotLive {
    /// For this, which is said.
    Said(String),
    /// Its command was said no to.
    Refused,
    /// Its program couldn't be started.
    Failed,
}

impl NotLive {
    /// What's said of a button whose terminal doesn't run for this.
    fn for_a_button(self) -> String {
        match self {
            NotLive::Said(why) if why == TOO_MANY => BUTTON_TERMINAL_FULL.to_string(),
            NotLive::Said(why) => format!("This button's terminal can't start. {why}"),
            NotLive::Refused => BUTTON_TERMINAL_REFUSED.to_string(),
            NotLive::Failed => "This button's terminal couldn't be started.".to_string(),
        }
    }
}

/// A button's press that types its text into a terminal, waiting for a yes: on the talk's slide
/// `slide`, into the terminal whose `id` is `terminal`.
#[derive(Clone, Debug)]
pub(crate) struct ButtonRun {
    slide: usize,
    /// Which visit of the show to that slide it was made in.
    visit: u64,
    /// The click step the slide was at, which its terminal must be shown by.
    step: u32,
    terminal: String,
    text: String,
}

/// What came of a button's press that types its text into a terminal.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RunPressed {
    /// It was typed.
    Typed,
    /// It waits for a yes, which [`WorkspaceView::ask_for_button`] asks.
    Ask,
    /// It can't be, for this, which the status line says.
    Refused(String),
}

/// What a button's press that types does next.
enum ButtonNext {
    Typed,
    AskRun(Vec<String>),
    AskType(TypeKey, RunQuestion),
    Refused(String),
}

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
        self.start_live(slide, terminal, false, cx)
    }

    /// [`Self::try_start_live`], and with `explicit` (*Start* was pressed, or its question answered)
    /// a session that is over, ended by itself or failed, is run again in its place, through the
    /// same yes. The show's own looking never does: a program that ended stays as its last screen.
    pub(super) fn start_live(&mut self, slide: usize, terminal: &TerminalLayer, explicit: bool, cx: &mut Context<Self>) -> LiveStep {
        match self.live_step(slide, terminal, explicit, cx) {
            Ok(step) => step,
            Err(NotLive::Said(sentence)) => {
                // What stops it is said once.
                if self.live.first_time(&sentence) {
                    self.say(sentence, cx);
                }
                LiveStep::Nothing
            }
            Err(NotLive::Refused | NotLive::Failed) => LiveStep::Nothing,
        }
    }

    /// What [`Self::start_live`] does, saying nothing: why the terminal doesn't run comes back.
    fn live_step(&mut self, slide: usize, terminal: &TerminalLayer, explicit: bool, cx: &mut Context<Self>) -> Result<LiveStep, NotLive> {
        let (Some(document), Some(run), Some(source)) = (self.document.clone(), terminal.on.run.clone(), terminal.source.clone()) else {
            return Ok(LiveStep::Nothing);
        };
        let id = LiveId::new(source.clone(), run.clone());
        if self.live.get(&id).is_some_and(|session| !explicit || !session.state().is_over()) {
            return Ok(LiveStep::Going);
        }
        // One whose tag or screen is wrong never starts.
        if let Some(problem) = document.update(cx, |document, _| document.terminal_run_problem_on(slide, &source)) {
            return Err(NotLive::Said(problem));
        }
        let Some(helper) = find_helper() else {
            return Err(NotLive::Said(NOT_INSTALLED.to_string()));
        };
        let command = split_command(&run).map_err(NotLive::Said)?;
        // A program that isn't there, or a command that can't be run.
        if let Err(why) = plan(document.read(cx).place(), &command) {
            return Err(NotLive::Said(format!("{COULDNT_START}{why}")));
        }
        match self.run_decision(&command, cx) {
            Decision::Refused => Err(NotLive::Refused),
            Decision::Ask => Ok(LiveStep::Ask(command)),
            Decision::Run => {
                if self.live.is_full() {
                    return Err(NotLive::Said(TOO_MANY.to_string()));
                }
                let none = RunPermissions::load(PathBuf::new());
                let remembered = self.run_permissions.as_ref().unwrap_or(&none);
                let place = document.read(cx).place().clone();
                let size = (terminal.on.cols, terminal.on.rows);
                match start_after_yes(&place, &command, &self.run_yes, remembered, &helper, size) {
                    Ok(session) => {
                        self.live.add(id, session);
                        document.update(cx, |document, _| document.keep_moves(true));
                        self.watch_live(cx);
                        cx.notify();
                        Ok(LiveStep::Going)
                    }
                    Err(_) => Err(NotLive::Failed),
                }
            }
        }
    }

    /// A button on the talk's slide `slide` was pressed in the show, to type `text`, then Enter, into
    /// the live terminal on that slide whose `id` is `terminal`, started first if it isn't running.
    /// Both the terminal's command and the text must have their yes: what has none is asked by
    /// [`Self::ask_for_button`] in a window of the show, and once it's answered yes the press goes
    /// on from where it stopped, looking at everything again. A new press takes the place of one
    /// still waiting.
    pub(crate) fn press_run_button(&mut self, slide: usize, visit: u64, step: u32, terminal: &str, text: &str, cx: &mut Context<Self>) -> RunPressed {
        let press = ButtonRun { slide, visit, step, terminal: terminal.to_string(), text: text.to_string() };
        let next = self.button_next(&press, cx);
        self.type_when_yes = matches!(next, ButtonNext::AskRun(_) | ButtonNext::AskType(..)).then_some(press);
        match next {
            ButtonNext::Typed => RunPressed::Typed,
            ButtonNext::AskRun(_) | ButtonNext::AskType(..) => RunPressed::Ask,
            ButtonNext::Refused(why) => RunPressed::Refused(why),
        }
    }

    /// Asks in `window` what the button pressed last waits for: the yes to run its terminal's command,
    /// or the yes to type its text. While another question is open nothing is asked, and the press
    /// waits for that one's answer.
    pub(crate) fn ask_for_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut press) = self.type_when_yes.clone() else { return };
        let Some(step) = self.shown_step(&press, cx) else {
            self.type_when_yes = None;
            return;
        };
        press.step = step;
        match self.button_next(&press, cx) {
            ButtonNext::Typed => self.type_when_yes = None,
            ButtonNext::Refused(why) => {
                self.type_when_yes = None;
                self.say(why, cx);
            }
            ButtonNext::AskRun(command) => {
                self.ask_to_run(command, window, cx);
            }
            ButtonNext::AskType(key, question) => {
                let view = cx.weak_entity();
                ask_to_run(window, cx, question, move |answer, window, cx| {
                    view.update(cx, |view, cx| view.typed_answered(&key, answer, window, cx)).ok();
                });
            }
        }
    }

    /// Takes in what was answered about the button pressed last typing what `key` is for, and types it
    /// on a yes.
    fn typed_answered(&mut self, key: &TypeKey, answer: RunAnswer, _window: &mut Window, cx: &mut Context<Self>) {
        match answer {
            RunAnswer::DontRun => {
                self.run_yes.refuse_typed(key);
                self.type_when_yes = None;
                return;
            }
            RunAnswer::Run => self.run_yes.allow_typed(key),
            RunAnswer::Always => {
                self.run_yes.allow_typed(key);
                let folder = self.document.as_ref().and_then(|document| TalkFolder::of(document.read(cx).place()));
                let kept = match (folder, &mut self.run_permissions) {
                    (Some(folder), Some(permissions)) => permissions.remember_typed(&folder, key.text(), key.terminal_command()).is_ok(),
                    _ => false,
                };
                if !kept {
                    self.say("The permission couldn't be kept, so it's only for now.".to_string(), cx);
                }
            }
        }
        // Everything is looked at again: the text is typed only if it still may be, and only while
        // the show is still where it was pressed.
        let Some(mut press) = self.type_when_yes.take() else { return };
        let Some(step) = self.shown_step(&press, cx) else { return };
        press.step = step;
        match self.button_next(&press, cx) {
            ButtonNext::Typed => {}
            ButtonNext::Refused(why) => self.say(why, cx),
            // The talk changed under the question: it's for the next press to ask.
            ButtonNext::AskRun(_) | ButtonNext::AskType(..) => {}
        }
        cx.notify();
    }

    /// A question about running a command was answered while a button waits: it goes on from where
    /// it stopped, in `window`, which asks about its text after a yes to its terminal, its terminal
    /// again if the question was about another, or says it can't after a no.
    pub(super) fn button_run_answered(&mut self, answered: &Plan, window: &mut Window, cx: &mut Context<Self>) {
        let Some(press) = self.type_when_yes.clone() else { return };
        if self.shown_step(&press, cx).is_none() {
            self.type_when_yes = None;
            return;
        }
        // A question about another command leaves it waiting for its own.
        if !self.press_runs(&press, answered, cx) {
            return;
        }
        // Once the question that was answered has gone.
        let view = cx.weak_entity();
        window.defer(cx, move |window, cx| {
            view.update(cx, |view, cx| view.ask_for_button(window, cx)).ok();
        });
    }

    /// The click step the show this window runs is at, if it's still on the visit to the slide
    /// `press` was made in.
    fn shown_step(&self, press: &ButtonRun, cx: &mut Context<Self>) -> Option<u32> {
        let show = self.current_show()?;
        if show.read(cx).has_ended() {
            return None;
        }
        let (visit, step) = show.update(cx, |show, cx| (show.visit(cx), show.place().step));
        (visit == (press.slide, press.visit)).then_some(step)
    }

    /// Whether `answered` is what a yes to run the terminal `press` types into is for.
    fn press_runs(&self, press: &ButtonRun, answered: &Plan, cx: &mut Context<Self>) -> bool {
        let Some(document) = self.document.clone() else { return false };
        let run = document.update(cx, |document, _| {
            document.terminals_on(press.slide).into_iter().find(|terminal| terminal.id.as_deref() == Some(press.terminal.as_str())).and_then(|terminal| terminal.on.run)
        });
        let Some(words) = run.and_then(|run| split_command(&run).ok()) else { return false };
        plan(document.read(cx).place(), &words).is_ok_and(|plan| plan.key() == answered.key())
    }

    /// What the button `press` does next: types its text now, waits for a yes, or can't, and why.
    fn button_next(&mut self, press: &ButtonRun, cx: &mut Context<Self>) -> ButtonNext {
        let Some(document) = self.document.clone() else { return ButtonNext::Refused(BUTTON_IN_NOTHING.to_string()) };
        let (twice, terminal) = document.update(cx, |document, _| {
            let twice = document.id_named_twice(press.slide, &press.terminal);
            (twice, document.terminals_on(press.slide).into_iter().find(|terminal| terminal.id.as_deref() == Some(press.terminal.as_str())))
        });
        // An `id` more than one thing has names neither.
        if twice {
            return ButtonNext::Refused(duplicate_id_notice(&press.terminal));
        }
        let Some((terminal, source, run)) = terminal.and_then(|terminal| Some((terminal.clone(), terminal.source?, terminal.on.run?))) else {
            return ButtonNext::Refused(BUTTON_IN_NOTHING.to_string());
        };
        // Only a terminal the slide shows by now is started, or typed into.
        if terminal.click.is_some_and(|click| click > press.step) {
            return ButtonNext::Refused(BUTTON_TERMINAL_NOT_SHOWN.to_string());
        }
        match self.live_step(press.slide, &terminal, false, cx) {
            Ok(LiveStep::Going) => {}
            Ok(LiveStep::Ask(command)) => return ButtonNext::AskRun(command),
            Ok(LiveStep::Nothing) => return ButtonNext::Refused(BUTTON_IN_NOTHING.to_string()),
            Err(why) => return ButtonNext::Refused(why.for_a_button()),
        }
        let Some(session) = self.live.get(&LiveId::new(source, run.clone())) else {
            return ButtonNext::Refused(BUTTON_TERMINAL_ENDED.to_string());
        };
        if session.state().is_over() {
            return ButtonNext::Refused(BUTTON_TERMINAL_ENDED.to_string());
        }
        // The terminal's command as the talk writes it, both for the yes and for the question.
        let Ok(words) = split_command(&run) else { return ButtonNext::Refused(BUTTON_IN_NOTHING.to_string()) };
        let place = document.read(cx).place().clone();
        let key = match type_key(&place, &press.text, &words) {
            Ok(key) => key,
            Err(why) => return ButtonNext::Refused(why),
        };
        let none = RunPermissions::load(PathBuf::new());
        let remembered = self.run_permissions.as_ref().unwrap_or(&none);
        match may_type(&place, &press.text, &words, &self.run_yes, remembered) {
            Decision::Run => {
                // Enter only once it may be typed.
                let mut bytes = press.text.clone().into_bytes();
                bytes.push(b'\r');
                session.send_keys(&bytes);
                ButtonNext::Typed
            }
            Decision::Ask => {
                let folder = TalkFolder::of(&place).map(|folder| spelled_plainly(folder.path()));
                let question = RunQuestion { command: words, program_in_talk: false, program_by_path: false, folder, typed: Some(press.text.clone()) };
                ButtonNext::AskType(key, question)
            }
            Decision::Refused => ButtonNext::Refused(BUTTON_TEXT_REFUSED.to_string()),
        }
    }

    /// Has the terminal selected on the slide shown run, asking first if it has no yes. This is
    /// *Start*: it's the only thing in the editor that starts a program, and only when pressed.
    pub fn start_selected_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else { return };
        let (slide, terminal) = document.update(cx, |document, _| (document.current(), document.selected_terminal()));
        let Some(terminal) = terminal else { return };
        match self.start_live(slide, &terminal, true, cx) {
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
        self.type_when_yes = None;
        let said = self.live.end_all();
        for sentence in said {
            self.say(sentence, cx);
        }
        self.watch_live(cx);
        cx.notify();
    }

    /// Ends the terminals whose tag was taken away, or whose `run` was changed, by an edit.
    pub(super) fn sync_live(&mut self, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        // Each tag goes along with the edits made before it.
        for change in document.update(cx, |document, _| document.take_moves()) {
            self.live.follow(&change);
        }
        if self.live.is_idle() {
            document.update(cx, |document, _| document.keep_moves(false));
            return;
        }
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
                        let had_sayings = !polled.sayings.is_empty();
                        for sentence in polled.sayings.clone() {
                            this.say(sentence, cx);
                        }
                        // A program that has ended is shown as such.
                        let more = this.live.busy();
                        if !more {
                            this.live_watch = None;
                        }
                        // Drawn again only for what changed: not for every look.
                        if !more || polled.changed || had_sayings {
                            cx.notify();
                        }
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
