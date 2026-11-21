//! Opening an MP4, saying what it is, and decoding its H.264 pictures on a thread of its own
//! into a short queue. This knows nothing of windows: what it gives out is pictures in memory.
//!
//! A video is data. Every size, count and place a file claims is checked against the file's own
//! length before anything is allocated or read, and a file that isn't a playable video says why
//! in plain words rather than crashing or hanging.

mod decode;
#[cfg(feature = "make")]
mod make;
mod open;
mod size;
mod sps;

use std::fmt;
use std::io::{Read, Seek};

pub use decode::Playing;
#[cfg(feature = "make")]
pub use make::{MadeColours, make_test_video, make_test_video_in};
pub use open::{Video, open};
pub use size::{MOST_READ, video_size};

/// Something a video can be read from in pieces, from anywhere in it.
pub trait ReadSeek: Read + Seek + Send {}

impl<T: Read + Seek + Send> ReadSeek for T {}

/// What a video is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub seconds: f64,
    pub frames: u64,
    pub frames_a_second: f32,
}

/// Why a video can't be played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CantPlay {
    NotAVideo,
    Damaged,
    NoPicture,
    KindNotPlayed,
    TooLarge,
    CouldNotRead,
}

impl fmt::Display for CantPlay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            CantPlay::NotAVideo => "This file isn't a video.",
            CantPlay::Damaged => "This video is damaged, so it can't be played.",
            CantPlay::NoPicture => "This video has no pictures to show.",
            CantPlay::KindNotPlayed => {
                "This kind of video can't be played here. An MP4 made with the usual settings (H.264) can."
            }
            CantPlay::TooLarge => {
                "This video is too large to play here. It can be 4096 by 2304, 120 frames a second and 6 hours long at most."
            }
            CantPlay::CouldNotRead => "This video couldn't be read.",
        })
    }
}

impl std::error::Error for CantPlay {}

/// One picture of a video: straight-alpha BGRA, opaque, `at` seconds into it.
#[derive(Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub at: f64,
    pub bgra: Vec<u8>,
}

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame").field("width", &self.width).field("height", &self.height).field("at", &self.at).finish()
    }
}
