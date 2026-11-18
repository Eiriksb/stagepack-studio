//! Decoding a video's pictures: the first one on the caller's thread, or one after another on a
//! thread of its own into a short queue.
//!
//! The decoder is used as the video spike found it must be: told not to flush after each
//! picture, which drops most pictures of a stream with B-frames, and flushed at the end of each
//! pass instead; and never with threads of its own, which crash. One thread decodes one video.

use std::collections::{BTreeMap, VecDeque};
use std::io::SeekFrom;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::Duration;

use openh264::OpenH264API;
use openh264::decoder::{DecodedYUV, Decoder, DecoderConfig, Flush};
use openh264::formats::YUVSource;
use yuvutils_rs::{YuvPlanarImage, YuvRange, yuv420_to_bgra};

use crate::open::{Index, Video};
use crate::sps::{self, Matrix};
use crate::{CantPlay, Frame, ReadSeek};

/// The most decoded pictures waiting to be shown.
const QUEUE: usize = 12;

/// How many pictures the first may be looked for in before a video is called damaged.
const FIRST_FRAME_WITHIN: usize = 256;

/// The most bytes read with no picture coming out before a video is called damaged: far more
/// than any well made stream reads between two pictures, and little enough that no table,
/// however made, keeps decoding going for long.
const MOST_UNPICTURED: u64 = 64 * 1024 * 1024;

/// How long dropping a playing video waits for its thread to let go of the file.
const MOST_DROP_WAIT: Duration = Duration::from_millis(250);

impl Video {
    /// Decodes the video's first picture, on this thread.
    pub fn first_frame(&mut self) -> Result<Frame, CantPlay> {
        let index = self.index.clone();
        let reader = &mut *self.reader;
        catch_unwind(AssertUnwindSafe(|| {
            let mut decoding = Decoding::new(index.clone())?;
            decoding.start_at(0, index.samples.len().min(FIRST_FRAME_WITHIN));
            let (rank, bgra) = decoding.next_frame(reader, &AtomicBool::new(false))?.ok_or(CantPlay::Damaged)?;
            let ticks = *index.times.get(rank).ok_or(CantPlay::Damaged)?;
            Ok(Frame { width: index.width, height: index.height, at: index.seconds(ticks), bgra })
        }))
        .unwrap_or(Err(CantPlay::Damaged))
    }

    /// Starts decoding on a thread of its own, from the picture shown at `from` seconds, and
    /// round again from the start at the end if `looped`.
    pub fn play(self, from: f64, looped: bool) -> Playing {
        let Video { reader, index } = self;
        let from = if from.is_finite() { from.max(0.0) } else { 0.0 };
        let from_ticks = (from * f64::from(index.ticks_a_second)).min(index.length as f64) as i64;
        // The picture being shown at `from`, and the last one decoding can start from before it.
        let first = index.times.partition_point(|t| *t <= from_ticks).saturating_sub(1);
        let shown = index.times[first];
        let start = index.samples.iter().enumerate().rev().find(|(i, s)| s.sync && index.first_shown(*i) <= shown);
        let start = start.map_or(0, |(i, _)| i);

        static THREADS: AtomicU64 = AtomicU64::new(0);
        let name = format!("video {}", THREADS.fetch_add(1, Ordering::Relaxed));
        let shared = Arc::new(Shared::default());
        let alive = Arc::new(());
        let token = Arc::downgrade(&alive);
        let theirs = shared.clone();
        let spawned = std::thread::Builder::new().name(name.clone()).spawn(move || {
            let _alive = alive;
            run(&theirs, reader, index, start, first, looped);
        });
        if spawned.is_err() {
            let mut state = shared.lock();
            state.done = true;
            state.failed = Some(CantPlay::CouldNotRead);
        }
        Playing { shared, name, alive: token }
    }

    /// How long each picture took to read, decode and turn into window colours, in order.
    #[cfg(feature = "make")]
    #[doc(hidden)]
    pub fn time_each_frame(mut self) -> Vec<Duration> {
        let Ok(mut decoding) = Decoding::new(self.index.clone()) else { return Vec::new() };
        decoding.start_at(0, self.index.samples.len());
        let mut times = Vec::new();
        let mut started = std::time::Instant::now();
        while let Ok(Some(_)) = decoding.next_frame(&mut *self.reader, &AtomicBool::new(false)) {
            times.push(started.elapsed());
            started = std::time::Instant::now();
        }
        times
    }
}

/// A video playing: a thread decoding it, and the pictures it has decoded ahead, 12 at most.
/// Dropping it ends the thread and frees the pictures.
pub struct Playing {
    shared: Arc<Shared>,
    name: String,
    alive: Weak<()>,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    /// Told when there's room in the queue, or the thread is to stop.
    room: Condvar,
    /// Set when the thread is to stop, looked at between every two pictures read.
    stop: AtomicBool,
    /// Told when the thread is done, having let go of the file.
    finished: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Default)]
struct State {
    queue: VecDeque<Queued>,
    done: bool,
    failed: Option<CantPlay>,
    most: usize,
}

struct Queued {
    /// When it's due, on the clock of the whole playing: past the video's length once it has
    /// gone round.
    due: f64,
    frame: Frame,
}

impl Playing {
    /// The newest picture due at `seconds`, skipping any older; `None` if none newer than the
    /// last given is due. The clock is the video's own: `from` when playing starts, going on
    /// rising past the video's length when it goes round.
    pub fn frame_for(&mut self, seconds: f64) -> Option<Frame> {
        let mut state = self.shared.lock();
        let mut newest = None;
        while state.queue.front().is_some_and(|q| q.due <= seconds) {
            newest = state.queue.pop_front();
        }
        drop(state);
        if newest.is_some() {
            self.shared.room.notify_one();
        }
        newest.map(|q| q.frame)
    }

    /// Whether every picture has been given out, and there are no more.
    pub fn ended(&self) -> bool {
        let state = self.shared.lock();
        state.done && state.failed.is_none() && state.queue.is_empty()
    }

    /// Why the video stopped playing, if it couldn't go on.
    pub fn failed(&self) -> Option<CantPlay> {
        self.shared.lock().failed
    }

    /// How many pictures the queue holds.
    #[doc(hidden)]
    pub fn queued(&self) -> usize {
        self.shared.lock().queue.len()
    }

    /// The most pictures the queue has held at once.
    #[doc(hidden)]
    pub fn most_queued(&self) -> usize {
        self.shared.lock().most
    }

    /// The name of the decoding thread.
    #[doc(hidden)]
    pub fn thread_name(&self) -> &str {
        &self.name
    }

    /// Something that lives exactly as long as the decoding thread.
    #[doc(hidden)]
    pub fn thread_alive(&self) -> Weak<()> {
        self.alive.clone()
    }
}

impl Drop for Playing {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        let mut state = self.shared.lock();
        let frames = std::mem::take(&mut state.queue);
        drop(state);
        self.shared.room.notify_all();
        drop(frames);
        // Waits, briefly, for the thread to let go of the file, so the talk can be saved over it
        // straight away. A picture being decoded can't be cut short.
        let state = self.shared.lock();
        let _ = self.shared.finished.wait_timeout_while(state, MOST_DROP_WAIT, |state| !state.done);
    }
}

/// The decoding thread: says when it's done, and why it stopped if it couldn't go on. A panic
/// inside decoding is caught and called damage.
fn run(shared: &Shared, mut reader: Box<dyn ReadSeek>, index: Arc<Index>, start: usize, first: usize, looped: bool) {
    let result = catch_unwind(AssertUnwindSafe(|| decode_all(shared, &mut *reader, &index, start, first, looped)));
    // Why it stopped is said before the file is let go of, so that whatever sees it let go of
    // knows already whether it failed.
    shared.lock().failed = match result {
        Ok(Ok(())) => None,
        Ok(Err(why)) => Some(why),
        Err(_) => Some(CantPlay::Damaged),
    };
    drop(reader);
    shared.lock().done = true;
    shared.finished.notify_all();
}

fn decode_all(
    shared: &Shared,
    reader: &mut dyn ReadSeek,
    index: &Arc<Index>,
    start: usize,
    first: usize,
    looped: bool,
) -> Result<(), CantPlay> {
    let mut decoding = Decoding::new(index.clone())?;
    let count = index.samples.len();
    decoding.start_at(start, count);
    let length = index.seconds(index.length);
    let mut round = 0u32;
    let mut skip_below = first;
    let mut pictures_this_round = false;
    loop {
        let next = decoding.next_frame(reader, &shared.stop)?;
        if shared.stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        match next {
            Some((rank, bgra)) => {
                pictures_this_round = true;
                let Some(ticks) = index.times.get(rank).filter(|_| rank >= skip_below) else { continue };
                let at = index.seconds(*ticks);
                let frame = Frame { width: index.width, height: index.height, at, bgra };
                if !push(shared, Queued { due: at + f64::from(round) * length, frame }) {
                    return Ok(());
                }
            }
            // A video in which nothing could be decoded is damaged, and isn't gone round again.
            None if !pictures_this_round => return Err(CantPlay::Damaged),
            None if !looped => return Ok(()),
            None => {
                round += 1;
                skip_below = 0;
                pictures_this_round = false;
                decoding.start_at(0, count);
            }
        }
    }
}

/// Puts a picture in the queue once there's room. False if the thread is to stop.
fn push(shared: &Shared, queued: Queued) -> bool {
    let mut state = shared.lock();
    while state.queue.len() >= QUEUE && !shared.stop.load(Ordering::Relaxed) {
        state = shared.room.wait(state).unwrap_or_else(|poisoned| poisoned.into_inner());
    }
    if shared.stop.load(Ordering::Relaxed) {
        return false;
    }
    state.queue.push_back(queued);
    state.most = state.most.max(state.queue.len());
    true
}

/// Decoding a video's pictures in turn.
struct Decoding {
    index: Arc<Index>,
    decoder: Decoder,
    /// The next picture to read, in the order stored, and where this pass ends.
    next: usize,
    end: usize,
    /// Where the next picture to come out of the decoder is in the order shown.
    rank: usize,
    /// The times of pictures this pass that decoded to nothing, which no picture out is given.
    lost: BTreeMap<i64, u32>,
    /// The bytes read since the last picture came out.
    unpictured: u64,
    headers_due: bool,
    flushed: bool,
    /// Pictures the decoder gave up at the end of the pass, not yet taken.
    held: VecDeque<Vec<u8>>,
    sample: Vec<u8>,
    packet: Vec<u8>,
}

impl Decoding {
    fn new(index: Arc<Index>) -> Result<Decoding, CantPlay> {
        let config = DecoderConfig::new().flush_after_decode(Flush::NoFlush);
        let decoder = Decoder::with_api_config(OpenH264API::from_source(), config).map_err(|_| CantPlay::Damaged)?;
        Ok(Decoding {
            index,
            decoder,
            next: 0,
            end: 0,
            rank: 0,
            lost: BTreeMap::new(),
            unpictured: 0,
            headers_due: true,
            flushed: false,
            held: VecDeque::new(),
            sample: Vec::new(),
            packet: Vec::new(),
        })
    }

    /// Starts a pass at picture `sample` in the order stored, which must be one decoding can
    /// start from, ending before picture `end`.
    fn start_at(&mut self, sample: usize, end: usize) {
        self.next = sample;
        self.end = end;
        self.rank = self.index.rank(self.index.first_shown(sample));
        self.lost.clear();
        self.headers_due = true;
        self.flushed = false;
        self.held.clear();
    }

    /// The next picture in the order shown, with its place in that order, or `None` at the end
    /// of the pass, or as soon as `stop` is set. Pictures that can't be decoded are passed over,
    /// and the times of those after them stay their own. More than 64 MB read with no picture
    /// out is damage.
    fn next_frame(&mut self, reader: &mut dyn ReadSeek, stop: &AtomicBool) -> Result<Option<(usize, Vec<u8>)>, CantPlay> {
        loop {
            if stop.load(Ordering::Relaxed) {
                return Ok(None);
            }
            if let Some(bgra) = self.held.pop_front() {
                return Ok(Some(self.stamp(bgra)));
            }
            if self.next >= self.end {
                if self.flushed {
                    return Ok(None);
                }
                self.flushed = true;
                if let Ok(pictures) = self.decoder.flush_remaining() {
                    for yuv in &pictures {
                        if let Some(bgra) = to_bgra(&self.index, yuv) {
                            self.held.push_back(bgra);
                        }
                    }
                }
                continue;
            }
            let sample = self.index.samples[self.next];
            self.next += 1;
            self.unpictured += u64::from(sample.size);
            if self.unpictured > MOST_UNPICTURED {
                return Err(CantPlay::Damaged);
            }
            self.read(reader, sample.offset, sample.size)?;
            let decoded = if self.unpack() && !self.packet.is_empty() {
                match self.decoder.decode(&self.packet) {
                    Ok(Some(yuv)) => Ok(to_bgra(&self.index, &yuv)),
                    Ok(None) => Ok(None),
                    Err(_) => Err(()),
                }
            } else {
                Err(())
            };
            match decoded {
                Ok(Some(bgra)) => return Ok(Some(self.stamp(bgra))),
                // Held back by the decoder, to come out later in the order shown.
                Ok(None) => {}
                Err(()) => self.lose(sample.shown),
            }
        }
    }

    /// Says no picture will come out for the one shown at `shown`.
    fn lose(&mut self, shown: i64) {
        if self.index.times.get(self.rank).is_some_and(|next| *next <= shown) {
            *self.lost.entry(shown).or_default() += 1;
        }
    }

    fn stamp(&mut self, bgra: Vec<u8>) -> (usize, Vec<u8>) {
        self.unpictured = 0;
        while let Some(next) = self.index.times.get(self.rank) {
            match self.lost.get_mut(next) {
                Some(count) if *count > 0 => {
                    *count -= 1;
                    self.rank += 1;
                }
                _ => break,
            }
        }
        self.rank += 1;
        (self.rank - 1, bgra)
    }

    /// Reads one picture's bytes, which opening checked lie inside the file.
    fn read(&mut self, reader: &mut dyn ReadSeek, offset: u64, size: u32) -> Result<(), CantPlay> {
        self.sample.resize(size as usize, 0);
        reader.seek(SeekFrom::Start(offset)).map_err(|_| CantPlay::CouldNotRead)?;
        reader.read_exact(&mut self.sample).map_err(|_| CantPlay::CouldNotRead)
    }

    /// Turns the picture's units, each after its length, into what the decoder reads: each
    /// after a start code, with the index's parameter sets first at the start of a pass. False
    /// if the picture is damaged: a unit holding what would read as the start of another, or a
    /// parameter set that wouldn't be played.
    fn unpack(&mut self) -> bool {
        self.packet.clear();
        if self.headers_due {
            self.packet.extend_from_slice(&self.index.headers);
            self.headers_due = false;
        }
        let width = self.index.length_bytes;
        let data = &self.sample;
        let mut at = 0;
        while data.len() - at >= width {
            let size = data[at..at + width].iter().fold(0usize, |size, byte| size << 8 | usize::from(*byte));
            at += width;
            if size > data.len() - at {
                return false;
            }
            let unit = &data[at..at + size];
            at += size;
            let Some(header) = unit.first() else { continue };
            if !sps::safe_unit(unit) {
                return false;
            }
            match header & 0x1F {
                // A sequence parameter set in a picture is checked as the index's were.
                7 if sps::read(unit).is_err() => return false,
                // Those for streams of several layers or views, which the decoder doesn't play.
                13 | 15 => continue,
                _ => {}
            }
            self.packet.extend_from_slice(&[0, 0, 0, 1]);
            self.packet.extend_from_slice(unit);
        }
        true
    }
}

/// A decoded picture in window colours, read with the file's own matrix and range, or `None`
/// if it isn't the size the file said.
fn to_bgra(index: &Index, yuv: &DecodedYUV<'_>) -> Option<Vec<u8>> {
    let (width, height) = yuv.dimensions();
    if (width, height) != (index.width as usize, index.height as usize) {
        return None;
    }
    let (y_stride, u_stride, v_stride) = yuv.strides();
    let mut bgra = vec![0; width * height * 4];
    match index.matrix {
        Matrix::Standard(matrix) => {
            let image = YuvPlanarImage {
                y_plane: yuv.y(),
                y_stride: u32::try_from(y_stride).ok()?,
                u_plane: yuv.u(),
                u_stride: u32::try_from(u_stride).ok()?,
                v_plane: yuv.v(),
                v_stride: u32::try_from(v_stride).ok()?,
                width: index.width,
                height: index.height,
            };
            yuv420_to_bgra(&image, &mut bgra, index.width * 4, index.range, matrix).ok()?;
        }
        Matrix::Identity => {
            // Green, blue and red stored as they are; in the usual range, each scaled as brightness is.
            let level = |v: u8| match index.range {
                YuvRange::Full => v,
                YuvRange::Limited => ((i32::from(v) - 16).clamp(0, 219) * 255 / 219) as u8,
            };
            let (y, u, v) = (yuv.y(), yuv.u(), yuv.v());
            for row in 0..height {
                for column in 0..width {
                    let half = row / 2 * u_stride + column / 2;
                    let at = (row * width + column) * 4;
                    let green = *y.get(row * y_stride + column)?;
                    let blue = *u.get(half)?;
                    let red = *v.get(row / 2 * v_stride + column / 2)?;
                    bgra[at..at + 4].copy_from_slice(&[level(blue), level(green), level(red), 255]);
                }
            }
        }
    }
    Some(bgra)
}
