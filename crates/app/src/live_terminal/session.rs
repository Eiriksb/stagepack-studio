//! One live terminal session: the helper started with the program to run, what it prints fed into a
//! bounded screen, what's typed sent back, and pictures of the screen drawn off the window's thread.
//!
//! The window's thread never waits for any of it. Starting, typing, resizing and ending only
//! queue what's to be said; the helper's output is read on a thread of its own, what's said to the
//! helper is written on another, and pictures are drawn on a third. A helper that is missing, isn't
//! the helper, stalls or crashes ends the session in a sentence, and no thread outlives it.

use std::io::{BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use gpui_kit::{Keystroke, RenderImage};
use slides_engine::{Fonts, FontsSeed, Rect};
use slides_terminal::{MOST_BYTES, MOST_EVENTS, Screen};
use super::permission::{Permit, ScratchFolder};
use slides_terminal_helper::wire::{self, Exit, FromHelper, MOST_COLS, MOST_ROWS, ToHelper};

use super::keys::key_bytes_in;
use crate::replay::{MOST_PICTURES_A_SECOND, draw_screen};
use crate::video_player::Shown;

/// What a live session's threads are called.
pub const THREAD_NAME: &str = "live terminal";
/// The most output a session takes in a second; what comes faster is dropped, not held.
pub const MOST_FLOOD_A_SECOND: usize = 4 * 1024 * 1024;
/// How much output is shown at a time, the screen being let go of between one slice and the next.
const SLICE: usize = 8 * 1024;
/// The most bytes of keys and answers waiting to be sent; more are dropped.
const MOST_QUEUED: usize = 1 << 20;
/// How long the helper has to say the program ended after being told to end: its own two seconds
/// of politeness, and room beyond them.
const ENDING_WAIT: Duration = Duration::from_secs(5);
/// How long the helper is given to leave once it has said all it will.
const LEAVING_WAIT: Duration = Duration::from_secs(5);
const BETWEEN_PICTURES: Duration = Duration::from_nanos(1_000_000_000 / MOST_PICTURES_A_SECOND as u64);

const COULDNT_START: &str = "The live terminal couldn't start.";
const BROKEN: &str = "The live terminal's helper didn't answer as it should.";
const STOPPED: &str = "The live terminal stopped unexpectedly.";
const DIDNT_END: &str = "The terminal's program didn't end when asked, so it was stopped.";
const FLOODED: &str = "The program printed faster than can be shown, so some of it was dropped.";
const RECORDING_FULL: &str = "The recording is full, so it stopped recording.";

/// Where a session is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionState {
    /// The helper is being started.
    Starting,
    /// The program is running.
    Running,
    /// The program ended, or was ended.
    Ended(Exit),
    /// It couldn't be run, or was lost, and why, in a sentence.
    Failed(String),
}

impl SessionState {
    /// Whether nothing more will happen.
    pub fn is_over(&self) -> bool {
        matches!(self, SessionState::Ended(_) | SessionState::Failed(_))
    }
}

/// Where a session's pictures are drawn: its box, how far it's turned, the scale the canvas's layers
/// are drawn at, and the talk's fonts.
#[derive(Clone)]
pub struct Look {
    pub rect: Rect,
    pub rotate: f32,
    pub scale: f32,
    pub fonts: FontsSeed,
}

impl Look {
    fn key(&self) -> [u32; 6] {
        let Rect { x, y, w, h } = self.rect;
        [x, y, w, h, self.rotate, self.scale].map(f32::to_bits)
    }
}

/// How many of a session's threads are running now: for tests that none is left.
static RUNNING: AtomicUsize = AtomicUsize::new(0);

#[doc(hidden)]
pub fn threads_running() -> usize {
    RUNNING.load(Ordering::SeqCst)
}

/// Counts a thread while it exists, however it ends.
struct Counted;

impl Counted {
    fn new() -> Counted {
        RUNNING.fetch_add(1, Ordering::SeqCst);
        Counted
    }
}

impl Drop for Counted {
    fn drop(&mut self) {
        RUNNING.fetch_sub(1, Ordering::SeqCst);
    }
}

fn lock<T>(held: &Mutex<T>) -> MutexGuard<'_, T> {
    held.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) -> bool {
    let counted = Counted::new();
    std::thread::Builder::new()
        .name(format!("{THREAD_NAME} {name}"))
        .spawn(move || {
            let _counted = counted;
            work();
        })
        .is_ok()
}

/// What pictures are wanted of the screen.
#[derive(Default)]
struct Drawing {
    look: Option<Look>,
    dirty: bool,
    /// Whether a drawing thread is there.
    running: bool,
}

/// What a session's threads and the window share.
struct Shared {
    state: Mutex<SessionState>,
    changed: Condvar,
    screen: Mutex<Screen>,
    /// What's to be said to the helper; none once the session is over.
    to_helper: Mutex<Option<Sender<ToHelper>>>,
    /// How many bytes of input are queued.
    queued: AtomicUsize,
    /// Set once the session has been told to end.
    ending: AtomicBool,
    /// The helper, once it's started, for whoever must end it.
    child: Mutex<Option<Arc<Mutex<Child>>>>,
    /// Set when the app ended the helper because the program didn't end when asked.
    killed: AtomicBool,
    /// Set when the window has let go of the session.
    abandoned: AtomicBool,
    notices: Mutex<Vec<String>>,
    cast: Mutex<Option<Cast>>,
    drawing: Mutex<Drawing>,
    draw_wanted: Condvar,
    shown: Mutex<Option<Shown>>,
    pictures: AtomicUsize,
    born: Instant,
}

impl Shared {
    fn state(&self) -> SessionState {
        lock(&self.state).clone()
    }

    /// Settles where the session ended up, if it hasn't already.
    fn finish(self: &Arc<Self>, state: SessionState) {
        {
            let mut now = lock(&self.state);
            if now.is_over() {
                return;
            }
            *now = state;
        }
        self.changed.notify_all();
        self.draw_again();
    }

    fn running(&self) {
        let mut now = lock(&self.state);
        if *now == SessionState::Starting {
            *now = SessionState::Running;
        }
        drop(now);
        self.changed.notify_all();
    }

    /// Waits up to `most` for the session to be over, and says whether it is.
    fn wait_over(&self, most: Duration) -> bool {
        let state = lock(&self.state);
        let (state, _) = self.changed.wait_timeout_while(state, most, |state| !state.is_over()).unwrap_or_else(|e| e.into_inner());
        state.is_over()
    }

    fn notice(&self, sentence: &str) {
        let mut notices = lock(&self.notices);
        if !notices.iter().any(|said| said == sentence) {
            notices.push(sentence.to_string());
        }
    }

    /// Says something to the helper, if there's anyone to say it to.
    fn say(&self, what: ToHelper) {
        let weight = if let ToHelper::Input(bytes) = &what { bytes.len() } else { 0 };
        if weight > 0 && self.queued.fetch_add(weight, Ordering::SeqCst) + weight > MOST_QUEUED {
            self.queued.fetch_sub(weight, Ordering::SeqCst);
            return;
        }
        let sent = lock(&self.to_helper).as_ref().is_some_and(|to| to.send(what).is_ok());
        if !sent && weight > 0 {
            self.queued.fetch_sub(weight, Ordering::SeqCst);
        }
    }

    /// Has the screen drawn again, if anyone is waiting for pictures of it.
    fn draw_again(self: &Arc<Self>) {
        let mut drawing = lock(&self.drawing);
        drawing.dirty = true;
        if drawing.look.is_some() {
            self.ensure_drawer(&mut drawing);
        }
        self.draw_wanted.notify_all();
    }

    fn ensure_drawer(self: &Arc<Self>, drawing: &mut Drawing) {
        if drawing.running || self.abandoned.load(Ordering::SeqCst) {
            return;
        }
        drawing.running = true;
        let shared = self.clone();
        if !spawn("drawing", move || draw_pictures(&shared)) {
            drawing.running = false;
        }
    }
}

/// The recording of a session so far: an asciinema recording in its second format.
struct Cast {
    bytes: Vec<u8>,
    events: usize,
}

impl Cast {
    fn new(cols: u16, rows: u16) -> Cast {
        Cast { bytes: format!("{{\"version\":2,\"width\":{cols},\"height\":{rows}}}\n").into_bytes(), events: 0 }
    }

    /// Adds an event; says whether there was room.
    fn add(&mut self, at: Duration, code: &str, data: &str) -> bool {
        let line = format!("[{:.6},\"{code}\",{}]\n", at.as_secs_f64(), serde_json::to_string(data).unwrap_or_default());
        if self.events >= MOST_EVENTS || self.bytes.len() + line.len() > MOST_BYTES {
            return false;
        }
        self.bytes.extend_from_slice(line.as_bytes());
        self.events += 1;
        true
    }
}

/// One live terminal session. Dropping it ends the program.
pub struct Session {
    shared: Arc<Shared>,
    /// The folder the program runs in, if the session made one: removed with the session.
    _scratch: Option<ScratchFolder>,
}

impl Session {
    /// Starts the helper at `helper` with `spec`, on threads of its own: this never waits. If the helper
    /// can't be started, or isn't one, the session says so in its state.
    pub fn start(helper: &Path, permit: Permit) -> Session {
        let (spec, scratch) = permit.into_parts();
        let (cols, rows) = (spec.cols.clamp(1, MOST_COLS), spec.rows.clamp(1, MOST_ROWS));
        let (to_helper, queue) = mpsc::channel();
        let shared = Arc::new(Shared {
            state: Mutex::new(SessionState::Starting),
            changed: Condvar::new(),
            screen: Mutex::new(Screen::live(cols, rows)),
            to_helper: Mutex::new(Some(to_helper)),
            queued: AtomicUsize::new(0),
            ending: AtomicBool::new(false),
            killed: AtomicBool::new(false),
            child: Mutex::new(None),
            abandoned: AtomicBool::new(false),
            notices: Mutex::new(Vec::new()),
            cast: Mutex::new(Some(Cast::new(cols, rows))),
            drawing: Mutex::new(Drawing::default()),
            draw_wanted: Condvar::new(),
            shown: Mutex::new(None),
            pictures: AtomicUsize::new(0),
            born: Instant::now(),
        });
        shared.say(ToHelper::Start(spec));
        let (theirs, helper) = (shared.clone(), helper.to_path_buf());
        if !spawn("reading", move || read_side(&theirs, &helper, queue)) {
            *lock(&shared.to_helper) = None;
            shared.finish(SessionState::Failed(COULDNT_START.to_string()));
        }
        Session { shared, _scratch: scratch }
    }

    /// Where the session is.
    pub fn state(&self) -> SessionState {
        self.shared.state()
    }

    /// Sends bytes to the program, as typed. Never waits; what's sent faster than the program
    /// takes it, beyond a megabyte, is dropped.
    pub fn send_keys(&self, bytes: &[u8]) {
        for chunk in bytes.chunks(wire::MOST_OUTPUT_CHUNK) {
            self.shared.say(ToHelper::Input(chunk.to_vec()));
        }
    }

    /// Sends what `key` sends the program, with the arrows as the program has asked for them; says
    /// whether the key sent anything.
    pub fn send_key(&self, key: &Keystroke) -> bool {
        let application = lock(&self.shared.screen).application_cursor();
        match key_bytes_in(key, application) {
            Some(bytes) => {
                self.send_keys(&bytes);
                true
            }
            None => false,
        }
    }

    /// Makes the program's terminal this many columns and rows, kept within the most there may be.
    pub fn resize(&self, cols: u16, rows: u16) {
        let (cols, rows) = (cols.clamp(1, MOST_COLS), rows.clamp(1, MOST_ROWS));
        lock(&self.shared.screen).resize(cols, rows);
        if let Some(cast) = lock(&self.shared.cast).as_mut() {
            cast.add(self.shared.born.elapsed(), "r", &format!("{cols}x{rows}"));
        }
        self.shared.say(ToHelper::Resize(cols, rows));
        self.shared.draw_again();
    }

    /// Tells the program to end. Never waits: if the helper hasn't said it ended after
    /// five seconds, the helper is killed, and the session says the program didn't end.
    pub fn end(&self) {
        end(&self.shared);
    }

    /// A copy of the screen as it is now.
    pub fn screen(&self) -> Screen {
        lock(&self.shared.screen).clone()
    }

    /// What's to be said beside the screen: that output was dropped, or that the recording is full.
    pub fn notices(&self) -> Vec<String> {
        lock(&self.shared.notices).clone()
    }

    /// Says where the session's pictures are to be drawn, from now on. They're drawn on a thread of
    /// its own, no more than `MOST_PICTURES_A_SECOND` a second, and only while the screen changes.
    pub fn set_look(&self, look: Look) {
        let mut drawing = lock(&self.shared.drawing);
        if drawing.look.as_ref().is_some_and(|had| had.key() == look.key()) {
            return;
        }
        drawing.look = Some(look);
        drawing.dirty = true;
        self.shared.ensure_drawer(&mut drawing);
        self.shared.draw_wanted.notify_all();
    }

    /// The newest picture drawn, if one has been.
    pub fn latest_picture(&self) -> Option<Arc<RenderImage>> {
        self.latest_shown().map(|shown| shown.image)
    }

    /// The newest picture drawn, and where it was drawn.
    pub(crate) fn latest_shown(&self) -> Option<Shown> {
        lock(&self.shared.shown).clone()
    }

    /// How many pictures have been drawn, so that a window can tell a new one.
    pub fn pictures_drawn(&self) -> usize {
        self.shared.pictures.load(Ordering::SeqCst)
    }

    /// The session so far as an asciinema recording (its output with times, what was typed left out),
    /// or nothing once it passed the most a recording may hold.
    pub fn recorded(&self) -> Option<Vec<u8>> {
        lock(&self.shared.cast).as_ref().map(|cast| cast.bytes.clone())
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shared.abandoned.store(true, Ordering::SeqCst);
        end(&self.shared);
        let _drawing = lock(&self.shared.drawing);
        self.shared.draw_wanted.notify_all();
    }
}

/// Tells the program to end, once, and starts the clock on it: the helper is killed if the session isn't
/// over five seconds from now, whether or not the helper takes what's said to it.
fn end(shared: &Arc<Shared>) {
    if shared.state().is_over() || shared.ending.swap(true, Ordering::SeqCst) {
        return;
    }
    shared.say(ToHelper::End);
    let theirs = shared.clone();
    spawn("ending", move || {
        if theirs.wait_over(ENDING_WAIT) {
            return;
        }
        // The helper's own end of the program takes two seconds; this is the end of the helper.
        theirs.killed.store(true, Ordering::SeqCst);
        if let Some(child) = lock(&theirs.child).clone() {
            kill(&child);
        }
    });
}

/// The thread that starts the helper and reads what it says.
fn read_side(shared: &Arc<Shared>, helper: &Path, queue: Receiver<ToHelper>) {
    let mut command = Command::new(helper.canonicalize().unwrap_or_else(|_| helper.to_path_buf()));
    // The helper starts nowhere near the talk: the program it runs is given its own folder.
    command.current_dir(std::env::temp_dir());
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window of its own.
        command.creation_flags(0x0800_0000);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return gave_up(shared, COULDNT_START),
    };
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return gave_up(shared, COULDNT_START);
    };
    let child = Arc::new(Mutex::new(child));
    *lock(&shared.child) = Some(child.clone());
    if shared.killed.load(Ordering::SeqCst) {
        kill(&child);
    }
    {
        let theirs = shared.clone();
        if !spawn("writing", move || write_side(&theirs, stdin, queue)) {
            kill(&child);
            reap(&child, Duration::ZERO);
            return gave_up(shared, COULDNT_START);
        }
    }
    let (how, started) = read_frames(shared, stdout);
    // However it ended, nothing more is said to the helper.
    *lock(&shared.to_helper) = None;
    let ended = shared.state().is_over();
    let sentence = match how {
        _ if ended => None,
        Reading::Broken => Some(BROKEN.to_string()),
        Reading::Closed if shared.killed.load(Ordering::SeqCst) => Some(DIDNT_END.to_string()),
        Reading::Closed if !started && shared.ending.load(Ordering::SeqCst) => {
            shared.finish(SessionState::Ended(Exit { code: None, signal: None }));
            None
        }
        Reading::Closed if !started => Some(COULDNT_START.to_string()),
        Reading::Closed => Some(STOPPED.to_string()),
    };
    if let Some(sentence) = sentence {
        shared.finish(SessionState::Failed(sentence));
    }
    // A helper that spoke nonsense, or never started, is no helper to wait for.
    let leaving = if matches!(how, Reading::Broken) || !started { Duration::ZERO } else { LEAVING_WAIT };
    reap(&child, leaving);
}

fn gave_up(shared: &Arc<Shared>, why: &str) {
    *lock(&shared.to_helper) = None;
    shared.finish(SessionState::Failed(why.to_string()));
}

fn kill(child: &Mutex<Child>) {
    let _ = lock(child).kill();
}

/// Waits up to `most` for the helper to be gone, then kills it, and gathers it either way.
fn reap(child: &Mutex<Child>, most: Duration) {
    let until = Instant::now() + most;
    loop {
        match lock(child).try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) => {}
        }
        if Instant::now() >= until {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut child = lock(child);
    let _ = child.kill();
    let _ = child.wait();
}

/// How the helper's output ended.
#[derive(Clone, Copy)]
enum Reading {
    /// A frame made no sense.
    Broken,
    /// The helper's output closed.
    Closed,
}

/// Reads what the helper says until it ends, feeding the screen and settling the state.
fn read_frames(shared: &Arc<Shared>, stdout: impl Read) -> (Reading, bool) {
    let mut input = BufReader::with_capacity(wire::MOST_OUTPUT_CHUNK + 16, stdout);
    let mut started = false;
    let mut pending: Vec<u8> = Vec::new();
    // What was taken in the second now going.
    let (mut window, mut taken) = (Instant::now(), 0usize);
    loop {
        let (kind, bytes) = match wire::read_frame(&mut input) {
            Ok(Some(frame)) => frame,
            Ok(None) | Err(_) => return (Reading::Closed, started),
        };
        match FromHelper::decode(kind, &bytes) {
            Ok(FromHelper::Started) if !started => {
                started = true;
                shared.running();
            }
            Ok(FromHelper::Output(bytes)) if started => {
                if window.elapsed() >= Duration::from_secs(1) {
                    (window, taken) = (Instant::now(), 0);
                }
                // Past the most bytes a second, what comes is dropped, whole frames at a time, and what
                // was half read is let go so that the next frame starts clean.
                if taken + bytes.len() > MOST_FLOOD_A_SECOND {
                    pending.clear();
                    lock(&shared.screen).cancel_sequence();
                    shared.notice(FLOODED);
                    continue;
                }
                taken += bytes.len();
                output(shared, &mut pending, &bytes);
            }
            Ok(FromHelper::Exited(exit)) if started => {
                shared.finish(SessionState::Ended(exit));
                return (Reading::Closed, started);
            }
            Ok(FromHelper::Failed(why)) if !started => {
                let why: String = why.chars().filter(|c| !c.is_control()).take(300).collect();
                let why = if why.trim().is_empty() { COULDNT_START.to_string() } else { why };
                shared.finish(SessionState::Failed(why));
                return (Reading::Closed, started);
            }
            _ => return (Reading::Broken, started),
        }
    }
}

/// Feeds the screen what the program printed, records it, and sends back what the screen answers.
fn output(shared: &Arc<Shared>, pending: &mut Vec<u8>, bytes: &[u8]) {
    let text = decode(pending, bytes);
    if text.is_empty() {
        return;
    }
    let mut rest = &text[..];
    while !rest.is_empty() {
        let mut at = SLICE.min(rest.len());
        while !rest.is_char_boundary(at) {
            at += 1;
        }
        let (slice, after) = rest.split_at(at);
        rest = after;
        let (answers, costly) = {
            let mut screen = lock(&shared.screen);
            screen.feed(slice);
            (screen.take_answers(), screen.too_costly())
        };
        if costly {
            shared.notice(FLOODED);
        }
        if !answers.is_empty() {
            shared.say(ToHelper::Input(answers));
        }
    }
    {
        let mut cast = lock(&shared.cast);
        if let Some(recording) = cast.as_mut()
            && !recording.add(shared.born.elapsed(), "o", &text)
        {
            *cast = None;
            shared.notice(RECORDING_FULL);
        }
    }
    shared.draw_again();
}

/// `bytes` as text, after what was left over of a character cut at the end of the last; a character cut
/// at the end of these is left over again. What isn't text is a replacement character.
fn decode(pending: &mut Vec<u8>, bytes: &[u8]) -> String {
    pending.extend_from_slice(bytes);
    let mut text = String::with_capacity(pending.len());
    let mut rest = &pending[..];
    loop {
        match std::str::from_utf8(rest) {
            Ok(valid) => {
                text.push_str(valid);
                rest = &[];
                break;
            }
            Err(error) => {
                let (valid, after) = rest.split_at(error.valid_up_to());
                text.push_str(std::str::from_utf8(valid).unwrap_or_default());
                match error.error_len() {
                    Some(bad) => {
                        text.push('\u{fffd}');
                        rest = &after[bad..];
                    }
                    None => {
                        rest = after;
                        break;
                    }
                }
            }
        }
    }
    let left = rest.to_vec();
    *pending = left;
    text
}

/// The thread that says things to the helper, and sees to the end of a program that won't.
fn write_side(shared: &Arc<Shared>, mut stdin: impl std::io::Write, queue: Receiver<ToHelper>) {
    for what in queue {
        if let ToHelper::Input(bytes) = &what {
            shared.queued.fetch_sub(bytes.len(), Ordering::SeqCst);
        }
        // Told to end before the program was started: it never is. The helper is left to see its input close.
        if matches!(what, ToHelper::Start(_)) && shared.ending.load(Ordering::SeqCst) {
            return;
        }
        if what.write(&mut stdin).is_err() {
            return;
        }
    }
}

/// The thread that draws pictures of the screen while there's a look to draw them in and something
/// new to draw, and ends once the session is over and its last screen is drawn.
fn draw_pictures(shared: &Arc<Shared>) {
    let mut fonts: Option<Fonts> = None;
    let mut last: Option<Instant> = None;
    loop {
        let look = {
            let mut drawing = lock(&shared.drawing);
            loop {
                if shared.abandoned.load(Ordering::SeqCst) {
                    drawing.running = false;
                    return;
                }
                if drawing.dirty && drawing.look.is_some() {
                    break;
                }
                if shared.state().is_over() {
                    drawing.running = false;
                    return;
                }
                drawing = shared.draw_wanted.wait(drawing).unwrap_or_else(|e| e.into_inner());
            }
            drawing.dirty = false;
            drawing.look.clone()
        };
        let Some(look) = look else { continue };
        if let Some(last) = last {
            let wait = (last + BETWEEN_PICTURES).saturating_duration_since(Instant::now());
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }
        }
        let screen = lock(&shared.screen).clone();
        let fonts = fonts.get_or_insert_with(|| look.fonts.clone().fonts());
        let shown = draw_screen(&screen, look.rect, look.rotate, look.scale, fonts, shared.born.elapsed().as_secs_f64());
        *lock(&shared.shown) = Some(shown);
        shared.pictures.fetch_add(1, Ordering::SeqCst);
        last = Some(Instant::now());
    }
}

