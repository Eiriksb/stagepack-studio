//! Terminal recordings: reading an asciinema recording (`.cast`, its second format) into a
//! checked, bounded list of timed events, the screen they're played on, and a player that
//! steps the screen through time. This knows nothing of windows and draws nothing.
//!
//! A recording is data. Every limit is checked as the file is read, before anything is kept:
//! the file's length, the screen's size, the number of events, each event's output, the
//! recording's length and all the text it keeps. Nothing in a recording is run, sent anywhere or read
//! as a path. A file that isn't a recording that can be played says why in plain words rather
//! than crashing or hanging.

mod cast;
mod parse;
mod player;
mod screen;

use std::fmt;

pub use cast::{
    Event, MOST_BYTES, MOST_EVENTS, MOST_GAP, MOST_HEIGHT, MOST_TEXT, MOST_LENGTH, MOST_OUTPUT, MOST_WIDTH, Mark, Recording,
    RecordingFacts, What, facts, played, read,
};
#[cfg(feature = "make")]
pub use cast::{make_demo_cast, make_test_cast};
pub use player::{KEPT_EVERY, MOST_KEPT_CELLS, MOST_KEPT_SCREENS, Player};
pub use screen::{Cell, Colour, Screen, Style, Wide, rgb};

/// Why a recording can't be played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CantPlay {
    NotARecording,
    Damaged,
    TooLarge,
    KindNotPlayed,
    CouldNotRead,
}

impl fmt::Display for CantPlay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            CantPlay::NotARecording => "This file isn't a terminal recording.",
            CantPlay::Damaged => "This terminal recording is damaged.",
            CantPlay::TooLarge => {
                "This terminal recording is too large to play here. Recordings of up to 32 MB, 24 hours and screens of 500 by 200 can be played."
            }
            CantPlay::KindNotPlayed => {
                "This kind of terminal recording can't be played here. Recordings made by asciinema in its second format can."
            }
            CantPlay::CouldNotRead => "This terminal recording couldn't be read.",
        })
    }
}

impl std::error::Error for CantPlay {}
