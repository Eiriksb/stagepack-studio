//! A video playing on a slide: its file opened, and its pictures decoded, on threads of their
//! own, and the picture due given to the canvas each time the window draws.
//!
//! Nothing here runs while nothing plays: a player asks the window for another frame only while
//! it plays, and a video that has ended, or was stopped, has no thread left.

use std::sync::Arc;
use std::time::Instant;

use gpui_kit::RenderImage;
use slides_engine::{Rect, VideoOn, bgra_frame, frame_layer, to_bgra_frame};
use slides_video::{CantPlay, Frame, Playing};

use crate::document::LargeOpen;
use crate::large_files::ReaderWatch;

/// Where a picture of a video is shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Placed {
    /// Over the rectangle of its layer, in slide units, stretched to it.
    Rect(Rect),
    /// Already turned with its layer and drawn at `scale`, its corner at (`x`, `y`) in the
    /// device pixels of that scale, as a layer is.
    Drawn { x: i32, y: i32, width: u32, height: u32, scale: f32 },
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
    /// Every picture has been shown: the last stays.
    Ended,
    /// It couldn't go on, and shows its still.
    Failed(CantPlay),
    /// A save let go of its file, which it opens again once the save is done.
    LetGo,
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
}

/// What came of opening a video: it playing, or why it can't.
pub(crate) type Opened = Result<(Playing, ReaderWatch), CantPlay>;

/// Opens the video `open` reads and starts it decoding from `from` seconds: for a background
/// task, as opening reads the file.
pub(crate) fn open_and_play(open: LargeOpen, from: f64, looped: bool) -> Opened {
    let reader = open.open().ok_or(CantPlay::CouldNotRead)?;
    let watch = reader.watch();
    let video = slides_video::open(Box::new(reader))?;
    Ok((video.play(from, looped), watch))
}

/// Ends a video's decoding on a thread of its own: dropping it waits for its thread to let go
/// of the file, which the window's thread mustn't wait for.
fn stop_aside(playing: Playing) {
    // With no thread to be had, it's let go of here, with the thread's work, which waits a
    // moment at most.
    std::thread::Builder::new().name("stopping a video".to_string()).spawn(move || drop(playing)).ok();
}

impl Player {
    /// A player of `on`, the video of layer `layer`, from `from` seconds, being opened.
    pub(crate) fn opening(layer: usize, on: VideoOn, from: f64) -> Player {
        Player { layer, on, state: State::Opening, from, started: None, shown: None, thread: None }
    }

    /// Takes in what came of opening it. The thread of a player no longer wanted is ended.
    pub(crate) fn opened(&mut self, opened: Opened) {
        match opened {
            Ok((playing, watch)) => {
                self.thread = Some(playing.thread_name().to_string());
                self.state = State::Playing { playing, watch };
            }
            Err(why) => self.state = State::Failed(why),
        }
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
    }

    /// Takes the picture due `now`, if a new one is, skipping any that are late, and says the
    /// picture it replaces, which the window must let go of. A turned video's picture is drawn
    /// turned at `scale`, the scale the canvas's layers are drawn at.
    pub(crate) fn advance(&mut self, now: Instant, scale: Option<f32>) -> Advanced {
        let State::Playing { playing, watch } = &mut self.state else {
            return Advanced::default();
        };
        let seconds = match self.started {
            Some(started) => self.from + now.saturating_duration_since(started).as_secs_f64(),
            None => self.from,
        };
        let mut advanced = Advanced::default();
        if let Some(frame) = playing.frame_for(seconds) {
            self.started.get_or_insert(now);
            if let Some(shown) = show(frame, &self.on, scale) {
                advanced.made = true;
                advanced.replaced = self.shown.replace(shown).map(|shown| shown.image);
            }
        }
        let next = if playing.ended() {
            Some(State::Ended)
        } else {
            playing.failed().map(|why| if watch.is_let_go() { State::LetGo } else { State::Failed(why) })
        };
        if let Some(next) = next {
            if let State::Failed(_) = next {
                // It shows its still again.
                advanced.replaced = advanced.replaced.or_else(|| self.shown.take().map(|shown| shown.image));
                self.shown = None;
            }
            if let State::Playing { playing, .. } = std::mem::replace(&mut self.state, next) {
                stop_aside(playing);
            }
            self.thread = None;
        }
        advanced
    }

    /// Takes the picture shown away, for the window to let go of.
    pub(crate) fn take_shown(&mut self) -> Option<Arc<RenderImage>> {
        self.shown.take().map(|shown| shown.image)
    }

    /// Plays at `on` from now on, as its layer is placed now. A picture drawn turned for the
    /// old place is drawn again with the next.
    pub(crate) fn place(&mut self, on: VideoOn) {
        self.on = on;
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let State::Playing { playing, .. } = std::mem::replace(&mut self.state, State::Ended) {
            stop_aside(playing);
        }
    }
}

/// What a player's advance came to.
#[derive(Default)]
pub(crate) struct Advanced {
    /// Whether a new picture was made to show.
    pub made: bool,
    /// The picture no longer shown.
    pub replaced: Option<Arc<RenderImage>>,
}

/// `frame` as the window shows it over the layer of `on`: as it is, stretched to the layer's
/// rectangle; or, turned, drawn turned at `scale`.
fn show(frame: Frame, on: &VideoOn, scale: Option<f32>) -> Option<Shown> {
    let Frame { width, height, at, bgra } = frame;
    if on.rotate == 0.0 {
        let image = RenderImage::new(vec![bgra_frame(width, height, bgra)?]);
        return Some(Shown { image: Arc::new(image), at, placed: Placed::Rect(on.rect) });
    }
    let scale = scale?;
    let layer = frame_layer(width, height, &bgra, on.rect, on.rotate, scale)?;
    let placed = Placed::Drawn { x: layer.x, y: layer.y, width: layer.pixmap.width(), height: layer.pixmap.height(), scale };
    let image = RenderImage::new(vec![to_bgra_frame(&layer.pixmap)]);
    Some(Shown { image: Arc::new(image), at, placed })
}
