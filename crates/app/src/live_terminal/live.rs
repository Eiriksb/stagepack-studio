//! The live terminals a window has running: at most four, each told apart by where its tag is
//! written and the command it runs, with the one place the keys go and the ending of all of them.
//!
//! Nothing here starts a program: a session is put in only once the window's yes has started it.
//! One window's show, its editor and the window itself share one of these.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::{Rc, Weak};

use gpui_kit::Keystroke;

use super::{Session, SessionState};

/// The most live terminals that run in a window at once.
pub const MOST_LIVE: usize = 4;

/// What the status line says when a program didn't end when asked and the helper was stopped.
pub const DIDNT_END: &str = "The terminal's program didn't end when asked, so it was stopped.";
/// What it says, once, when a slide that runs a command is shown and the add-on isn't there.
pub const NOT_INSTALLED: &str = "The live terminal add-on isn't installed, so its recording is shown.";
/// What it says when a fifth terminal would have run.
pub const TOO_MANY: &str = "At most 4 live terminals run at once, so this one shows its still.";
/// What it says, in front of the reason, when a terminal couldn't start.
pub const COULDNT_START: &str = "The live terminal couldn't start: ";
/// What shows in small text at the foot of a terminal whose program has ended.
pub const PROGRAM_ENDED: &str = "The program ended";

/// Which terminal: where its tag is written in the talk, and the command it runs, as written.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LiveId {
    source: Range<usize>,
    command: String,
}

impl LiveId {
    pub fn new(source: Range<usize>, command: impl Into<String>) -> LiveId {
        LiveId { source, command: command.into() }
    }

    /// Where the tag is written in the talk.
    pub fn source(&self) -> &Range<usize> {
        &self.source
    }

    /// The command as the tag writes it.
    pub fn command(&self) -> &str {
        &self.command
    }
}

struct Entry {
    id: LiveId,
    session: Rc<Session>,
    /// Whether it was ever seen running, which tells a program that ended from one that never
    /// started.
    was_running: bool,
    /// Whether it was seen over, which is what a window draws again for.
    was_over: bool,
}

#[derive(Default)]
struct Inner {
    running: Vec<Entry>,
    /// Told to end, and kept until each is over, so that one that won't end is said so.
    ending: Vec<Rc<Session>>,
    keys: Option<LiveId>,
    /// What has been said once already.
    said: Vec<String>,
    /// Whether a show of the talk is on, whose window draws the terminals, as the editor's doesn't
    /// then.
    show_on: bool,
}

/// The live terminals of a window. Clones are the same set.
#[derive(Clone, Default)]
pub struct LiveTerminals(Rc<RefCell<Inner>>);

impl LiveTerminals {
    pub fn new() -> LiveTerminals {
        LiveTerminals::default()
    }

    /// The session for `id`, if one was started and hasn't been let go of.
    pub fn get(&self, id: &LiveId) -> Option<Rc<Session>> {
        self.0.borrow().running.iter().find(|entry| &entry.id == id).map(|entry| entry.session.clone())
    }

    /// Whether `id` has a session whose program hasn't ended.
    pub fn is_running(&self, id: &LiveId) -> bool {
        self.get(id).is_some_and(|session| !session.state().is_over())
    }

    /// How many programs are running now.
    pub fn running(&self) -> usize {
        self.0.borrow().running.iter().filter(|entry| !entry.session.state().is_over()).count()
    }

    /// How many programs are running or still being ended: those count toward the most there may be.
    pub fn alive(&self) -> usize {
        let inner = self.0.borrow();
        inner.running.iter().map(|entry| &entry.session).chain(inner.ending.iter()).filter(|session| !session.state().is_over()).count()
    }

    /// Whether no more may be started.
    pub fn is_full(&self) -> bool {
        self.alive() >= MOST_LIVE
    }

    /// How many sessions there are in all, those ending included.
    pub fn sessions(&self) -> usize {
        let inner = self.0.borrow();
        inner.running.len() + inner.ending.len()
    }

    /// Whether anything is running or ending, so that the sessions are to be looked at.
    pub fn busy(&self) -> bool {
        let inner = self.0.borrow();
        !inner.ending.is_empty() || inner.running.iter().any(|entry| !entry.session.state().is_over())
    }

    /// Whether there is nothing running, ending or kept.
    pub fn is_idle(&self) -> bool {
        self.sessions() == 0
    }

    /// Keeps `session` as the one for `id`. A session already there for it ends.
    pub fn add(&self, id: LiveId, session: Session) {
        let session = Rc::new(session);
        let mut inner = self.0.borrow_mut();
        if let Some(at) = inner.running.iter().position(|entry| entry.id == id) {
            let old = inner.running.remove(at);
            old.session.end();
            inner.ending.push(old.session);
        }
        inner.running.push(Entry { id, session, was_running: false, was_over: false });
    }

    /// Every id there's a session for, ended or not.
    pub fn ids(&self) -> Vec<LiveId> {
        self.0.borrow().running.iter().map(|entry| entry.id.clone()).collect()
    }

    /// What the window shows of `id`: a hold on its session that doesn't keep it alive.
    pub fn weak(&self, id: &LiveId) -> Option<Weak<Session>> {
        self.get(id).map(|session| Rc::downgrade(&session))
    }

    /// Ends the program of `id`, if there is one, and lets go of its session once it's over.
    pub fn end(&self, id: &LiveId) {
        let mut inner = self.0.borrow_mut();
        if let Some(at) = inner.running.iter().position(|entry| &entry.id == id) {
            let entry = inner.running.remove(at);
            entry.session.end();
            inner.ending.push(entry.session);
        }
        if inner.keys.as_ref() == Some(id) {
            inner.keys = None;
        }
    }

    /// Ends every program, as the show or the window or the talk ends. Says, for any that are
    /// already over without having ended when asked, what the status line says; those still
    /// ending are said of by [`Self::poll`].
    pub fn end_all(&mut self) -> Vec<String> {
        {
            let mut inner = self.0.borrow_mut();
            inner.keys = None;
            for entry in std::mem::take(&mut inner.running) {
                entry.session.end();
                inner.ending.push(entry.session);
            }
        }
        self.poll().sayings
    }

    /// Follows every tag through `change`, made to the talk's text: a tag moved by an edit before it
    /// keeps running; one the edit took away ends.
    pub fn follow(&self, change: &slides_format::Change) {
        let mut gone = Vec::new();
        {
            let mut inner = self.0.borrow_mut();
            let mut keys = inner.keys.clone();
            for entry in &mut inner.running {
                let was = entry.id.clone();
                match crate::selection::moved(&entry.id.source, change) {
                    Some(source) => entry.id.source = source,
                    None => gone.push(was.clone()),
                }
                if keys.as_ref() == Some(&was) && !gone.contains(&was) {
                    keys = Some(entry.id.clone());
                }
            }
            inner.keys = keys;
        }
        for id in gone {
            self.end(&id);
        }
    }

    /// Ends the programs of every session whose tag isn't among `tags` any more: the tag was
    /// taken away, or its `run` changed.
    pub fn keep_only(&self, tags: &[LiveId]) {
        for id in self.ids() {
            if !tags.contains(&id) {
                self.end(&id);
            }
        }
    }

    /// Looks at every session: lets go of those that are over, and says what is to be said of
    /// them. Also says which ones never started.
    pub fn poll(&self) -> Polled {
        let mut polled = Polled::default();
        let mut inner = self.0.borrow_mut();
        let mut gone = Vec::new();
        for entry in &mut inner.running {
            let over = entry.session.state().is_over();
            if over != entry.was_over {
                entry.was_over = over;
                polled.changed = true;
            }
            match entry.session.state() {
                SessionState::Running => entry.was_running = true,
                SessionState::Failed(sentence) if !entry.was_running => {
                    polled.sayings.push(format!("{COULDNT_START}{sentence}"));
                    polled.failed.push(entry.id.clone());
                    gone.push(entry.id.clone());
                }
                SessionState::Failed(sentence) if sentence == DIDNT_END => {
                    if !polled.sayings.iter().any(|said| said == DIDNT_END) {
                        polled.sayings.push(DIDNT_END.to_string());
                    }
                    gone.push(entry.id.clone());
                }
                _ => {}
            }
        }
        for id in gone {
            inner.running.retain(|entry| entry.id != id);
            if inner.keys.as_ref() == Some(&id) {
                inner.keys = None;
            }
        }
        let ending = std::mem::take(&mut inner.ending);
        for session in ending {
            match session.state() {
                SessionState::Failed(sentence) if sentence == DIDNT_END => {
                    if !polled.sayings.iter().any(|said| said == DIDNT_END) {
                        polled.sayings.push(DIDNT_END.to_string());
                    }
                }
                state if state.is_over() => {}
                _ => inner.ending.push(session),
            }
        }
        polled
    }

    /// Says whether `sentence` is being said for the first time, and notes that it was.
    pub fn first_time(&self, sentence: &str) -> bool {
        let mut inner = self.0.borrow_mut();
        if inner.said.iter().any(|said| said == sentence) {
            return false;
        }
        inner.said.push(sentence.to_string());
        true
    }

    /// Lets what was said once be said again, as for another talk.
    pub fn forget_said(&self) {
        self.0.borrow_mut().said.clear();
    }

    /// Which terminal has the keys, if one does and its program hasn't ended.
    pub fn keys(&self) -> Option<LiveId> {
        let mut inner = self.0.borrow_mut();
        let held = inner.keys.clone()?;
        let alive = inner.running.iter().any(|entry| entry.id == held && !entry.session.state().is_over());
        if !alive {
            inner.keys = None;
            return None;
        }
        inner.keys.clone()
    }

    /// Gives the keys to the terminal `id`, if its program is running. The one that had them
    /// loses them.
    pub fn give_keys(&self, id: &LiveId) -> bool {
        if !self.is_running(id) {
            return false;
        }
        self.0.borrow_mut().keys = Some(id.clone());
        true
    }

    /// Takes the keys back from whichever has them.
    pub fn release_keys(&self) -> bool {
        self.0.borrow_mut().keys.take().is_some()
    }

    /// Sends `key` to the terminal that has the keys, as the program has asked for the arrows.
    /// Says whether it sent anything. A key that sends nothing, as the one that gives the keys
    /// back does, isn't the terminal's.
    pub fn send(&self, key: &Keystroke) -> bool {
        let Some(id) = self.keys() else { return false };
        self.get(&id).is_some_and(|session| session.send_key(key))
    }

    /// Sets whether a show of the talk is on.
    pub fn set_show_on(&self, on: bool) {
        let mut inner = self.0.borrow_mut();
        if inner.show_on != on {
            // Whoever had the keys doesn't have them in the other place.
            inner.keys = None;
        }
        inner.show_on = on;
    }

    /// Whether a show of the talk is on.
    pub fn show_on(&self) -> bool {
        self.0.borrow().show_on
    }
}

/// What looking at the sessions found.
#[derive(Default)]
pub struct Polled {
    /// What the status line is to say.
    pub sayings: Vec<String>,
    /// Whether a program ended, so that what shows it is to be drawn again.
    pub changed: bool,
    /// The sessions that never started, and have been let go of.
    pub failed: Vec<LiveId>,
}

/// Whether `key` is the one that gives the keys back to the show: Ctrl+Shift+Escape.
pub fn gives_keys_back(key: &Keystroke) -> bool {
    let m = key.modifiers;
    m.control && m.shift && !m.alt && !m.platform && key.key == "escape"
}
