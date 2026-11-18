//! A video playing on a slide: its file opened, and its pictures decoded, on threads of their
//! own, and the picture due given to the canvas each time the window draws.
//!
//! Nothing here runs while nothing plays: a player asks the window for another frame only while
//! it plays, and a video that has ended, or was stopped, has no thread left.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use futures::StreamExt as _;
use futures::channel::mpsc;
use gpui_kit::{App, AppContext as _, BackgroundExecutor, Entity, EventEmitter, RenderImage};
use slides_engine::{MadeStill, Pixmap, Rect, StillJob, VideoOn, bgra_frame, frame_layer, to_bgra_frame};
use slides_video::{CantPlay, Frame, Playing};

use crate::document::{Document, LargeOpen};
use crate::large_files::{HeldReader, NotOpened, ReaderWatch};

/// Sent by a talk when a video's still has come, and the slides showing it are to be drawn again.
pub struct StillsTaken;

impl EventEmitter<StillsTaken> for Document {}

/// What came of making a still.
#[doc(hidden)]
pub enum Making {
    Made(MadeStill),
    /// It couldn't be made yet: while a save replaces the pack, or before a video a pack holds
    /// compressed is unpacked to play.
    Waits(StillJob),
}

/// Makes `job`, reading the video with `open`: for a background task.
fn make_one(job: StillJob, open: Option<LargeOpen>) -> Making {
    let Some(open) = open else {
        return Making::Made(job.make(None));
    };
    match open.open_for_still() {
        Ok(reader) => make_still_from(job, reader),
        Err(NotOpened::Unreadable) => Making::Made(job.make(None)),
        Err(NotOpened::Saving | NotOpened::NotUnpacked) => Making::Waits(job),
    }
}

/// Makes `job` from `reader`. One that failed because a save let go of its file waits to be
/// made again after the save, rather than saying it can't be read.
#[doc(hidden)]
pub fn make_still_from(job: StillJob, reader: HeldReader) -> Making {
    let watch = reader.watch();
    let made = job.make(Some(Box::new(reader)));
    if made.failed() && watch.is_let_go() {
        return Making::Waits(made.again());
    }
    Making::Made(made)
}

/// Has `document`'s videos' stills made on background tasks, one at a time, as each is wanted,
/// and taken in, the talk saying so with [`StillsTaken`]. Nothing runs while none is wanted. A
/// talk's stills are made once, whoever asks first.
pub fn make_stills(document: &Entity<Document>, cx: &mut App) {
    if document.read(cx).stills_made_aside() {
        return;
    }
    let (wanted, mut asked) = mpsc::unbounded::<()>();
    document.update(cx, |document, _| {
        document.make_stills_aside(Arc::new(move || {
            wanted.unbounded_send(()).ok();
        }))
    });
    // Held weakly, as the talk holds what tells this.
    let document = document.downgrade();
    cx.spawn(async move |cx| {
        while asked.next().await.is_some() {
            let Ok(jobs) = document.update(cx, |document, _| document.still_jobs()) else { return };
            for (job, open) in jobs {
                let making = cx.background_spawn(async move { make_one(job, open) }).await;
                let taken = document.update(cx, |document, cx| match making {
                    Making::Made(made) => {
                        if document.take_still(made) {
                            cx.emit(StillsTaken);
                            cx.notify();
                        }
                    }
                    Making::Waits(job) => document.still_waits(job),
                });
                if taken.is_err() {
                    return;
                }
            }
        }
    })
    .detach();
}

/// Where a picture of a video is shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Placed {
    /// Over its layer's rectangle as the layer is placed now, stretched to it.
    Rect,
    /// Already turned with its layer and drawn at `scale`, its corner at (`x`, `y`) in the
    /// device pixels of that scale, as a layer is, for the layer placed at `for_rect`: one moved
    /// since is shown moved as far.
    Drawn { x: i32, y: i32, width: u32, height: u32, scale: f32, for_rect: Rect },
}

/// The picture of a video shown, in the window's image type.
#[derive(Clone)]
pub(crate) struct Shown {
    pub image: Arc<RenderImage>,
    /// When in the video it's from, in seconds.
    pub at: f64,
    pub placed: Placed,
}

enum State {
    /// Its file is being opened, and its decoding started, on a background task.
    Opening,
    Playing { playing: Playing, watch: ReaderWatch },
    /// Every picture has been shown, or it was stopped where it was: the last stays.
    Ended,
    /// It couldn't go on, and shows its still.
    Failed(CantPlay),
    /// A save let go of its file, or was running when it was to be opened: it's opened again
    /// once the save is done.
    LetGo,
}

/// How many players have been opened, and how many of their decodings were ended on a
/// background task, in all: for tests that nothing ends one on the window's thread.
static OPENED: AtomicUsize = AtomicUsize::new(0);
static ENDED_ASIDE: AtomicUsize = AtomicUsize::new(0);

/// How many videos' decodings were started, and how many were ended on a background task.
#[doc(hidden)]
pub fn decodings_started_and_ended_aside() -> (usize, usize) {
    (OPENED.load(Ordering::SeqCst), ENDED_ASIDE.load(Ordering::SeqCst))
}

/// One video being played.
pub struct Player {
    /// The place of the video's layer among the canvas's layers.
    pub(crate) layer: usize,
    pub(crate) on: VideoOn,
    state: State,
    /// Where in the video it starts, in seconds.
    from: f64,
    /// When its first picture was shown, on the window's clock.
    started: Option<Instant>,
    shown: Option<Shown>,
    /// The name of its decoding thread, while it has one.
    thread: Option<String>,
    /// Whether a picture is being turned off the window's thread.
    turning: bool,
    /// The picture last turned, as the turning reads it, kept to be filled again.
    turned_from: Arc<Mutex<Option<Pixmap>>>,
    /// What ends its decoding when it's done with, off the window's thread.
    background: BackgroundExecutor,
}

/// Why a video wasn't opened to play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[doc(hidden)]
pub enum Unopened {
    /// A save replacing the talk's pack was running: it's opened once that's done.
    Saving,
    Cannot(CantPlay),
}

/// What came of opening a video: it playing, or why not.
#[doc(hidden)]
pub type Opened = Result<(Playing, ReaderWatch), Unopened>;

/// Opens the video `open` reads and starts it decoding from `from` seconds: for a background
/// task, as opening reads the file.
pub(crate) fn open_and_play(open: LargeOpen, from: f64, looped: bool) -> Opened {
    let reader = open.open_unless_saving().map_err(|why| match why {
        NotOpened::Saving => Unopened::Saving,
        NotOpened::Unreadable | NotOpened::NotUnpacked => Unopened::Cannot(CantPlay::CouldNotRead),
    })?;
    play_from(reader, from, looped)
}

/// Reads the index of the video `reader` reads, and starts it decoding. One that failed because
/// a save let go of its file waits to be opened again after the save.
#[doc(hidden)]
pub fn play_from(reader: HeldReader, from: f64, looped: bool) -> Opened {
    let watch = reader.watch();
    let video = slides_video::open(Box::new(reader)).map_err(|why| if watch.is_let_go() { Unopened::Saving } else { Unopened::Cannot(why) })?;
    OPENED.fetch_add(1, Ordering::SeqCst);
    Ok((video.play(from, looped), watch))
}

/// Ends a video's decoding on a short thread of its own: dropping it waits for its thread to
/// let go of the file, which neither the window's thread nor the background tasks that make
/// stills and turn pictures should wait for. With no thread to be had, a background task does it.
pub(crate) fn stop_aside(playing: Playing, background: &BackgroundExecutor) {
    let held = Arc::new(Mutex::new(Some(playing)));
    let theirs = held.clone();
    let spawned = std::thread::Builder::new().name("stopping a video".to_string()).spawn(move || {
        drop(theirs.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take());
        ENDED_ASIDE.fetch_add(1, Ordering::SeqCst);
    });
    if spawned.is_err() {
        let playing = held.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
        background
            .spawn(async move {
                drop(playing);
                ENDED_ASIDE.fetch_add(1, Ordering::SeqCst);
            })
            .detach();
    }
}

/// Lets go of what came of opening a video no one wants any more, off the window's thread.
pub(crate) fn discard(opened: Opened, background: &BackgroundExecutor) {
    if let Ok((playing, _)) = opened {
        stop_aside(playing, background);
    }
}

/// A video's picture to be turned with its layer, off the window's thread.
pub(crate) struct Turning {
    frame: Frame,
    on: VideoOn,
    scale: f32,
    from: Arc<Mutex<Option<Pixmap>>>,
}

impl Turning {
    /// The picture turned, in the window's image type: for a background task.
    pub(crate) fn turn(self) -> Option<Shown> {
        let Turning { frame, on, scale, from } = self;
        let mut from = from.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let layer = frame_layer(frame.width, frame.height, &frame.bgra, on.rect, on.rotate, scale, &mut from)?;
        let placed = Placed::Drawn { x: layer.x, y: layer.y, width: layer.pixmap.width(), height: layer.pixmap.height(), scale, for_rect: on.rect };
        let image = RenderImage::new(vec![to_bgra_frame(&layer.pixmap)]);
        Some(Shown { image: Arc::new(image), at: frame.at, placed })
    }
}

impl Player {
    /// A player of `on`, the video of layer `layer`, from `from` seconds, being opened.
    pub(crate) fn opening(layer: usize, on: VideoOn, from: f64, background: BackgroundExecutor) -> Player {
        Player {
            layer,
            on,
            state: State::Opening,
            from,
            started: None,
            shown: None,
            thread: None,
            turning: false,
            turned_from: Arc::default(),
            background,
        }
    }

    /// Takes in what came of opening it, and says whether it waits for a save to be done.
    pub(crate) fn opened(&mut self, opened: Opened) -> bool {
        match opened {
            Ok((playing, watch)) => {
                self.thread = Some(playing.thread_name().to_string());
                self.state = State::Playing { playing, watch };
            }
            Err(Unopened::Saving) => self.state = State::LetGo,
            Err(Unopened::Cannot(why)) => self.state = State::Failed(why),
        }
        self.was_let_go()
    }

    /// Whether it's playing, or being opened to: either way, the window draws again soon.
    pub fn is_playing(&self) -> bool {
        matches!(self.state, State::Opening | State::Playing { .. })
    }

    /// Whether it's decoding, and so wants the window to draw again soon for its next picture.
    pub(crate) fn wants_frames(&self) -> bool {
        matches!(self.state, State::Playing { .. })
    }

    /// Whether a save let go of its file, so that it waits to open it again.
    pub(crate) fn was_let_go(&self) -> bool {
        matches!(self.state, State::LetGo)
    }

    /// Why it stopped, if it couldn't go on.
    pub fn failed(&self) -> Option<CantPlay> {
        match self.state {
            State::Failed(why) => Some(why),
            _ => None,
        }
    }

    /// The picture shown now, if one is.
    pub(crate) fn shown(&self) -> Option<&Shown> {
        self.shown.as_ref()
    }

    /// The name of its decoding thread, if it has one.
    pub(crate) fn thread(&self) -> Option<&str> {
        self.thread.as_deref()
    }

    /// How many pictures it has decoded ahead, waiting to be shown.
    pub(crate) fn queued(&self) -> usize {
        match &self.state {
            State::Playing { playing, .. } => playing.queued(),
            _ => 0,
        }
    }

    /// The seconds into the video to start from if it's opened again: where it had got to.
    pub(crate) fn resume_from(&self) -> f64 {
        self.shown.as_ref().map_or(self.from, |shown| shown.at)
    }

    /// Starts it again from where it had got to, being opened anew.
    pub(crate) fn reopening(&mut self) {
        self.from = self.resume_from();
        self.started = None;
        self.state = State::Opening;
        // A picture being turned for it before is for a player that's gone.
        self.turning = false;
    }

    /// Ends its decoding where it is, its picture staying, as for a slide being left.
    pub(crate) fn hold(&mut self) {
        self.end_as(State::Ended);
    }

    fn end_as(&mut self, next: State) {
        if let State::Playing { playing, .. } = std::mem::replace(&mut self.state, next) {
            stop_aside(playing, &self.background);
        }
        self.thread = None;
    }

    /// Takes the picture due `now`, if a new one is, skipping any that are late, and says the
    /// picture it replaces, which the window must let go of. A turned video's picture is given
    /// to be turned, at `scale`, the scale the canvas's layers are drawn at, off the window's
    /// thread, one at a time: until that's done, the next isn't taken.
    pub(crate) fn advance(&mut self, now: Instant, scale: Option<f32>) -> Advanced {
        let State::Playing { playing, watch } = &mut self.state else {
            return Advanced::default();
        };
        let seconds = match self.started {
            Some(started) => self.from + now.saturating_duration_since(started).as_secs_f64(),
            None => self.from,
        };
        let mut advanced = Advanced::default();
        let turned = self.on.rotate != 0.0;
        if !(turned && (self.turning || scale.is_none()))
            && let Some(frame) = playing.frame_for(seconds)
        {
            self.started.get_or_insert(now);
            match scale.filter(|_| turned) {
                Some(scale) => {
                    self.turning = true;
                    advanced.turning = Some(Turning { frame, on: self.on.clone(), scale, from: self.turned_from.clone() });
                }
                None => {
                    if let Some(shown) = show(frame) {
                        advanced.made = true;
                        advanced.replaced = self.shown.replace(shown).map(|shown| shown.image);
                    }
                }
            }
        }
        let next = if playing.ended() {
            Some(State::Ended)
        } else {
            // Only a save lets go of a file: one that failed otherwise fails.
            playing.failed().map(|why| if watch.is_let_go() { State::LetGo } else { State::Failed(why) })
        };
        if let Some(next) = next {
            if let State::Failed(_) = next {
                // It shows its still again.
                advanced.replaced = advanced.replaced.or_else(|| self.shown.take().map(|shown| shown.image));
                self.shown = None;
            }
            self.end_as(next);
        }
        advanced
    }

    /// Takes in a picture turned, and gives the one it replaces, for the window to let go of.
    pub(crate) fn turned(&mut self, shown: Option<Shown>) -> Option<Arc<RenderImage>> {
        self.turning = false;
        // One that couldn't go on shows its still.
        if matches!(self.state, State::Failed(_)) {
            return shown.map(|shown| shown.image);
        }
        self.shown.replace(shown?).map(|shown| shown.image)
    }

    /// Takes the picture shown away, for the window to let go of.
    pub(crate) fn take_shown(&mut self) -> Option<Arc<RenderImage>> {
        self.shown.take().map(|shown| shown.image)
    }

    /// Plays at `on` from now on, as its layer is placed now. Its picture moves with it.
    pub(crate) fn place(&mut self, on: VideoOn) {
        self.on = on;
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.end_as(State::Ended);
    }
}

/// What a player's advance came to.
#[derive(Default)]
pub(crate) struct Advanced {
    /// Whether a new picture was made to show.
    pub made: bool,
    /// The picture no longer shown.
    pub replaced: Option<Arc<RenderImage>>,
    /// A picture to turn off the window's thread, and then show.
    pub turning: Option<Turning>,
}

/// `frame` as the window shows it over its layer, stretched to the layer's rectangle.
fn show(frame: Frame) -> Option<Shown> {
    let Frame { width, height, at, bgra } = frame;
    let image = RenderImage::new(vec![bgra_frame(width, height, bgra)?]);
    Some(Shown { image: Arc::new(image), at, placed: Placed::Rect })
}
