//! A terminal recording playing on a slide: read from its file, stepped through time, and each
//! screen drawn with the engine's own items and drawing, so that it looks just as its still does,
//! all on a thread of its own; and the newest picture given to the canvas each time the window
//! draws.
//!
//! The window's thread never waits for it: it says what it wants, and looks, each time it draws,
//! whether a new picture has come. A picture is drawn only when the screen has changed, or its box
//! or scale has, and no more than [`MOST_PICTURES_A_SECOND`] a second. Nothing here runs while
//! nothing plays: a replay held at a mark waits, drawing nothing and asking the window for no
//! frame, until it's asked for more. Its thread ends once it's told to stop, between one event and
//! the next, and nothing waits for it to.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use gpui_kit::RenderImage;
use slides_engine::{Enter, Fonts, FontsSeed, LayerSpec, Rect, TerminalOn, render_layer, screen_items, to_bgra_frame};
use slides_terminal::{CantPlay, Player, Recording, Screen};

use crate::document::LargeOpen;
use crate::large_files::NotOpened;
use crate::video_player::{Placed, Shown};

/// The most pictures a replay draws a second.
pub const MOST_PICTURES_A_SECOND: u32 = 60;
const BETWEEN_PICTURES: Duration = Duration::from_nanos(1_000_000_000 / MOST_PICTURES_A_SECOND as u64);
/// How long a replay waits before reading its file again, while a save replaces the talk's pack.
const SAVING_WAIT: Duration = Duration::from_millis(20);
/// What a replay's thread is called.
pub const THREAD_NAME: &str = "playing a recording";
/// The most recordings a window plays, or has readied, at once: each holds its own screens, and
/// more would show no more of a slide than their last screens do.
pub const MOST_PLAYING: usize = 4;

/// Where a replay goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aim {
    /// On from where it is to stop `n`, counting from none, and held there: past its last stop,
    /// or with none, to its end.
    PlayTo(Option<usize>),
    /// The screen at stop `n`, held there, or its end past its last.
    At(usize),
    /// The screen as it starts, held there: a replay readied for a slide not shown yet.
    Start,
}

/// What the window wants of a replay's thread.
struct Wanted {
    aim: Aim,
    /// How many aims have been asked for, so that the thread takes each in once.
    aims: u64,
    look: Look,
    /// How many times anything here has changed, so that the thread looks again.
    changes: u64,
}

/// Where a replay's pictures are drawn: its box, how far it's turned, and the scale the canvas's
/// layers are drawn at, once there is one.
#[derive(Clone, Copy)]
struct Look {
    rect: Rect,
    rotate: f32,
    scale: Option<f32>,
}

impl Look {
    /// What tells one look from another, NaN and all.
    fn key(&self) -> [u32; 6] {
        let Rect { x, y, w, h } = self.rect;
        [x, y, w, h, self.rotate, self.scale.unwrap_or(f32::NAN)].map(f32::to_bits)
    }
}

/// What a recording is played from: its file, read on the replay's thread, and the talk's fonts
/// it's drawn with there.
pub struct Source {
    pub open: LargeOpen,
    pub fonts: FontsSeed,
}

/// What a replay's thread and the window share.
struct Shared {
    wanted: Mutex<Wanted>,
    changed: Condvar,
    /// Set to end the thread.
    stop: AtomicBool,
    /// The newest picture drawn, until the window takes it.
    newest: Mutex<Option<Shown>>,
    /// Whether pictures are still to come for what's wanted.
    moving: AtomicBool,
    /// Whether the thread has ended.
    ended: AtomicBool,
    /// Why it couldn't be played, if it couldn't.
    failed: Mutex<Option<CantPlay>>,
}

fn lock<T>(held: &Mutex<T>) -> MutexGuard<'_, T> {
    held.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// How many replays' threads are running, how many screens they've drawn in all, and what tests
/// have them do.
static RUNNING: AtomicUsize = AtomicUsize::new(0);
static DRAWN: AtomicUsize = AtomicUsize::new(0);
static FASTER: AtomicU32 = AtomicU32::new(1);
static SLOWED_NANOS: AtomicU64 = AtomicU64::new(0);
static READ: AtomicUsize = AtomicUsize::new(0);

/// A file read for replays, by what tells it from others.
type FileIdentity = (usize, String, Option<(u64, std::time::SystemTime)>);

/// A recording read for replays, kept only as long as a replay holds it; replays wanting it while
/// it's read wait for it.
type Kept = Arc<Mutex<Weak<Recording>>>;

/// The recordings being played, by their file, so that replays of one file share what's read of it.
static BEING_PLAYED: Mutex<Vec<(FileIdentity, Kept)>> = Mutex::new(Vec::new());

/// How many times replays have read a recording's file: for tests that replays of one share it.
#[doc(hidden)]
pub fn recordings_read() -> usize {
    READ.load(Ordering::SeqCst)
}

/// How many replays' threads are running now: for tests that every one ends.
#[doc(hidden)]
pub fn threads_running() -> usize {
    RUNNING.load(Ordering::SeqCst)
}

/// How many screens replays have drawn, in all: for tests that nothing is drawn for a slide not
/// shown, or a replay waiting at a mark.
#[doc(hidden)]
pub fn screens_drawn() -> usize {
    DRAWN.load(Ordering::SeqCst)
}

/// For tests: has every replay play `times` as fast as it was recorded, from now on, as a long
/// recording is gone through in a test.
#[doc(hidden)]
pub fn play_faster_for_tests(times: u32) {
    FASTER.store(times.max(1), Ordering::SeqCst);
}

/// For tests: has every replay take `longer` for each picture it draws, as a slow computer would.
#[doc(hidden)]
pub fn slow_for_tests(longer: Duration) {
    SLOWED_NANOS.store(longer.as_nanos().min(u128::from(u64::MAX)) as u64, Ordering::SeqCst);
}

fn faster() -> u32 {
    FASTER.load(Ordering::Relaxed).max(1)
}

/// Counts a replay's thread while it runs, and says it's ended once it has, however it did.
struct Running(Arc<Shared>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.moving.store(false, Ordering::SeqCst);
        self.0.ended.store(true, Ordering::SeqCst);
        RUNNING.fetch_sub(1, Ordering::SeqCst);
    }
}

/// One terminal recording being played.
pub struct Replay {
    /// The place of the recording's layer among the canvas's layers.
    pub(crate) layer: usize,
    pub(crate) on: TerminalOn,
    aim: Aim,
    /// The scale last asked for.
    scale: Option<f32>,
    shared: Arc<Shared>,
    shown: Option<Shown>,
}

impl Replay {
    /// Plays `on`, the recording of layer `layer`, from its start to `aim`, reading it and drawing
    /// it as `source` says on a thread of its own, at `scale`. One that `stays`
    /// waits at its end to be sent back to a mark, as in the show; one that doesn't ends there,
    /// its last picture showing, as in the editor.
    pub(crate) fn start(layer: usize, on: TerminalOn, aim: Aim, source: Source, stays: bool, scale: Option<f32>) -> Replay {
        let Source { open, fonts } = source;
        let look = Look { rect: on.rect, rotate: on.rotate, scale };
        let shared = Arc::new(Shared {
            wanted: Mutex::new(Wanted { aim, aims: 1, look, changes: 1 }),
            changed: Condvar::new(),
            stop: AtomicBool::new(false),
            newest: Mutex::new(None),
            moving: AtomicBool::new(true),
            ended: AtomicBool::new(false),
            failed: Mutex::new(None),
        });
        RUNNING.fetch_add(1, Ordering::SeqCst);
        let theirs = shared.clone();
        let spawned = std::thread::Builder::new().name(THREAD_NAME.to_string()).spawn(move || run(theirs, open, fonts, stays));
        if spawned.is_err() {
            // With no thread to be had, it shows its still.
            *lock(&shared.failed) = Some(CantPlay::CouldNotRead);
            drop(Running(shared.clone()));
        }
        Replay { layer, on, aim, scale, shared, shown: None }
    }

    /// Changes what the thread is to do, and tells it.
    fn ask(&self, change: impl FnOnce(&mut Wanted)) {
        let mut wanted = lock(&self.shared.wanted);
        change(&mut wanted);
        wanted.changes += 1;
        self.shared.moving.store(true, Ordering::SeqCst);
        self.shared.changed.notify_all();
    }

    /// Sends it on to `aim`, if it isn't going there already.
    pub(crate) fn aim(&mut self, aim: Aim) {
        if aim == self.aim {
            return;
        }
        self.aim = aim;
        self.ask(|wanted| {
            wanted.aim = aim;
            wanted.aims += 1;
        });
    }

    /// Where it's been sent.
    pub(crate) fn aimed(&self) -> Aim {
        self.aim
    }

    /// Plays at `on` from now on, as its layer is placed now: its next picture is drawn there.
    pub(crate) fn place(&mut self, on: TerminalOn) {
        if (on.rect, on.rotate) != (self.on.rect, self.on.rotate) {
            let (rect, rotate) = (on.rect, on.rotate);
            self.ask(|wanted| {
                wanted.look.rect = rect;
                wanted.look.rotate = rotate;
            });
        }
        self.on = on;
    }

    /// Takes the newest picture, if one has come, at once and without waiting for one, and asks
    /// for the next at `scale`, the scale the canvas's layers are drawn at, if that has changed.
    /// Says whether a picture was taken, and gives the one it replaces, for the window to let go
    /// of.
    pub(crate) fn advance(&mut self, scale: Option<f32>) -> (bool, Option<Arc<RenderImage>>) {
        if scale.map(f32::to_bits) != self.scale.map(f32::to_bits) {
            self.scale = scale;
            self.ask(|wanted| wanted.look.scale = scale);
        }
        let newest = self.shared.newest.try_lock().ok().and_then(|mut newest| newest.take());
        match newest {
            Some(shown) => (true, self.shown.replace(shown).map(|shown| shown.image)),
            None => (false, None),
        }
    }

    /// Whether a picture is still to come, and so the window should draw again soon.
    pub(crate) fn wants_frames(&self) -> bool {
        // Looked at in this order, as the thread leaves its picture before it says it's done.
        let moving = self.shared.moving.load(Ordering::SeqCst) && !self.shared.ended.load(Ordering::SeqCst);
        moving || self.shared.newest.try_lock().map_or(true, |newest| newest.is_some())
    }

    /// Whether its thread is still playing it, or waiting to be sent on.
    pub(crate) fn is_playing(&self) -> bool {
        !self.shared.stop.load(Ordering::SeqCst) && !self.shared.ended.load(Ordering::SeqCst)
    }

    /// Why it couldn't be played, if it couldn't.
    pub(crate) fn failed(&self) -> Option<CantPlay> {
        *lock(&self.shared.failed)
    }

    /// The picture shown now, if one is.
    pub(crate) fn shown(&self) -> Option<&Shown> {
        self.shown.as_ref()
    }

    /// Takes the picture shown away, for the window to let go of.
    pub(crate) fn take_shown(&mut self) -> Option<Arc<RenderImage>> {
        self.shown.take().map(|shown| shown.image)
    }

    /// Ends its thread where it is, its picture staying, as for a slide being left.
    pub(crate) fn hold(&mut self) {
        self.end();
    }

    /// Tells its thread to end, and goes on: the thread ends between one event and the next.
    fn end(&self) {
        let _wanted = lock(&self.shared.wanted);
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.changed.notify_all();
    }
}

impl Drop for Replay {
    fn drop(&mut self) {
        self.end();
    }
}

/// Where a replay's thread has got to.
#[derive(Clone, Copy)]
enum Going {
    /// Held at a played time.
    Held(Duration),
    /// Playing from `from`, since `since`, to `until`, where it's held.
    Playing { from: Duration, since: Instant, until: Duration },
}

impl Going {
    /// The played time it's at, `now`.
    fn at(self, now: Instant) -> Duration {
        match self {
            Going::Held(at) => at,
            Going::Playing { from, since, until } => from.saturating_add(now.saturating_duration_since(since).saturating_mul(faster())).min(until),
        }
    }
}

/// A replay's thread.
fn run(shared: Arc<Shared>, open: LargeOpen, fonts: FontsSeed, stays: bool) {
    let _running = Running(shared.clone());
    let played = catch_unwind(AssertUnwindSafe(|| play(&shared, open, fonts, stays)));
    let failed = match played {
        Ok(Ok(())) => None,
        Ok(Err(why)) => Some(why),
        Err(_) => Some(CantPlay::CouldNotRead),
    };
    if failed.is_some() {
        *lock(&shared.failed) = failed;
    }
}

/// The recording `open` reads, read whole and checked, or nothing if the replay is stopped first.
/// While a save replaces the talk's pack, it's read once that's done.
fn read(shared: &Shared, open: LargeOpen) -> Result<Option<Recording>, CantPlay> {
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            return Ok(None);
        }
        match open.clone().read_recording() {
            Ok(bytes) => {
                READ.fetch_add(1, Ordering::SeqCst);
                return slides_terminal::read(&bytes?).map(Some);
            }
            Err(NotOpened::Saving) => std::thread::sleep(SAVING_WAIT),
            Err(NotOpened::Unreadable | NotOpened::NotUnpacked) => return Err(CantPlay::CouldNotRead),
        }
    }
}

/// The recording `open` reads, shared with every other replay of the same file playing now, read
/// here if none is: or nothing if the replay is stopped first.
fn shared_recording(shared: &Shared, open: &LargeOpen) -> Result<Option<Arc<Recording>>, CantPlay> {
    let identity = open.identity();
    let kept = {
        let mut playing = lock(&BEING_PLAYED);
        // What no replay holds, or waits for, any more goes.
        playing.retain(|(_, kept)| Arc::strong_count(kept) > 1 || kept.try_lock().map_or(true, |recording| recording.strong_count() > 0));
        match playing.iter().find(|(of, _)| *of == identity) {
            Some((_, kept)) => kept.clone(),
            None => {
                let kept = Arc::new(Mutex::new(Weak::new()));
                playing.push((identity, kept.clone()));
                kept
            }
        }
    };
    let mut recording = lock(&kept);
    if let Some(read) = recording.upgrade() {
        return Ok(Some(read));
    }
    let Some(read) = read(shared, open.clone())? else { return Ok(None) };
    let read = Arc::new(read);
    *recording = Arc::downgrade(&read);
    Ok(Some(read))
}

/// Plays the recording `open` reads as `shared` asks, until it's stopped, or, unless it `stays`,
/// until it's at its end. The talk's files `open` reads are held while it plays, so that what
/// tells its file from others stays its own.
fn play(shared: &Shared, open: LargeOpen, fonts: FontsSeed, stays: bool) -> Result<(), CantPlay> {
    let Some(recording) = shared_recording(shared, &open)? else { return Ok(()) };
    let mut player = Player::new(recording);
    let mut fonts = fonts.fonts();
    let time_of_stop = |player: &Player, stop: Option<usize>| stop.and_then(|stop| player.stops().get(stop).copied()).unwrap_or_else(|| player.end());
    let mut going = Going::Held(Duration::ZERO);
    let mut aims = 0;
    let mut changes;
    // What was drawn last: the events its screen had had, and where it was drawn; and when.
    let mut drawn: Option<(usize, [u32; 6])> = None;
    let mut last_drawn: Option<Instant> = None;
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        let (aim, new_aim, look) = {
            let wanted = lock(&shared.wanted);
            let new_aim = wanted.aims != aims;
            (aims, changes) = (wanted.aims, wanted.changes);
            (wanted.aim, new_aim, wanted.look)
        };
        let now = Instant::now();
        if new_aim {
            let at = going.at(now);
            going = match aim {
                Aim::PlayTo(stop) => {
                    let until = time_of_stop(&player, stop);
                    match going {
                        _ if until <= at => Going::Held(until),
                        Going::Playing { from, since, .. } => Going::Playing { from, since, until },
                        Going::Held(_) => Going::Playing { from: at, since: now, until },
                    }
                }
                Aim::At(stop) => Going::Held(time_of_stop(&player, Some(stop))),
                Aim::Start => Going::Held(Duration::ZERO),
            };
        }
        let at = going.at(now);
        if let Going::Playing { until, .. } = going
            && at >= until
        {
            going = Going::Held(until);
        }
        let events = player.events_at(at);
        let shown = look.scale.is_none() || drawn == Some((events, look.key()));
        let mut due = None;
        if !shown {
            match last_drawn {
                Some(last) if now < last + BETWEEN_PICTURES => due = Some(last + BETWEEN_PICTURES),
                _ => {
                    let Some(screen) = player.screen_at_within(at, &shared.stop) else { return Ok(()) };
                    if let Some(picture) = draw(screen, look, &mut fonts, at) {
                        *lock(&shared.newest) = Some(picture);
                        DRAWN.fetch_add(1, Ordering::SeqCst);
                    }
                    drawn = Some((events, look.key()));
                    last_drawn = Some(Instant::now());
                    slowed(shared);
                    continue;
                }
            }
        } else if let Going::Playing { from, since, until } = going {
            // The next time the screen changes, or where it's held.
            let next = player.time_of(events).unwrap_or(until).min(until);
            due = Some(since + next.saturating_sub(from) / faster());
        }
        let mut wanted = lock(&shared.wanted);
        if wanted.changes != changes || shared.stop.load(Ordering::SeqCst) {
            continue;
        }
        match due {
            Some(due) => {
                let wait = due.saturating_duration_since(Instant::now());
                if !wait.is_zero() {
                    drop(shared.changed.wait_timeout(wanted, wait));
                }
            }
            None => {
                // At its end with nothing more to do, one that doesn't stay is done.
                if !stays && at >= player.end() && drawn.is_some() {
                    return Ok(());
                }
                shared.moving.store(false, Ordering::SeqCst);
                while wanted.changes == changes && !shared.stop.load(Ordering::SeqCst) {
                    wanted = shared.changed.wait(wanted).unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            }
        }
    }
}

/// Takes as much longer as a test asks, looking at whether it's to stop meanwhile.
fn slowed(shared: &Shared) {
    let mut left = Duration::from_nanos(SLOWED_NANOS.load(Ordering::Relaxed));
    while !left.is_zero() && !shared.stop.load(Ordering::SeqCst) {
        let nap = left.min(Duration::from_millis(5));
        std::thread::sleep(nap);
        left -= nap;
    }
}

/// `screen`, `at` into its recording, drawn as its layer is: laid out as the engine lays out a
/// recording's screen, and drawn as the engine draws a layer, turned with its box, at the scale
/// `look` gives.
fn draw(screen: &Screen, look: Look, fonts: &mut Fonts, at: Duration) -> Option<Shown> {
    let scale = look.scale?;
    Some(draw_screen(screen, look.rect, look.rotate, scale, fonts, at.as_secs_f64()))
}

/// `screen` drawn as a recording's layer is, in `rect`, turned by `rotate`, at `scale`: laid out as
/// the engine lays out a terminal's screen and drawn as the engine draws a layer. A replay and a live
/// terminal both draw their pictures here, so that they look alike. `at` is when it's from, in seconds.
pub(crate) fn draw_screen(screen: &Screen, rect: Rect, rotate: f32, scale: f32, fonts: &mut Fonts, at: f64) -> Shown {
    let items = screen_items(screen, Rect { x: 0.0, y: 0.0, w: rect.w, h: rect.h }, fonts);
    let spec = LayerSpec {
        source: None,
        rect,
        rotate,
        items: items.into(),
        click: None,
        enter: Enter::Appear,
        native: Vec::new(),
        is_title: false,
        varies: false,
        video: None,
        model: None,
        terminal: Some(TerminalOn { src: String::new(), run: None, cols: 100, rows: 30, rect, rotate, stops: 0, pending: false }),
        button: None,
        component: None,
        id: None,
    };
    let layer = render_layer(&spec, scale, fonts);
    let placed = Placed::Drawn { x: layer.x, y: layer.y, width: layer.pixmap.width(), height: layer.pixmap.height(), scale, for_rect: rect };
    let image = RenderImage::new(vec![to_bgra_frame(&layer.pixmap)]);
    Shown { image: Arc::new(image), at, placed }
}
