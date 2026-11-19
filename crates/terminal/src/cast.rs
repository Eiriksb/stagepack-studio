//! Reading a recording, line by line. Each line's length is checked before it's parsed, and
//! what it says is checked against the limits before it's kept.

use std::borrow::Cow;
use std::fmt;
use std::time::Duration;

use serde::de::{self, Deserialize, Deserializer, Visitor};
use serde_json::Value;

use crate::CantPlay;

/// The most a recording's file may hold: the talk's ordinary file limit.
pub const MOST_BYTES: usize = 32 * 1024 * 1024;
/// The widest and tallest screen a recording may have.
pub const MOST_WIDTH: u16 = 500;
pub const MOST_HEIGHT: u16 = 200;
/// The most events a recording may hold, counting those that are left out.
pub const MOST_EVENTS: usize = 1_000_000;
/// The most output, in bytes, one event may hold.
pub const MOST_OUTPUT: usize = 1024 * 1024;
/// The longest a recording may be, as it was recorded.
pub const MOST_LENGTH: Duration = Duration::from_secs(24 * 60 * 60);
/// The longest wait between two events as it's played.
pub const MOST_GAP: Duration = Duration::from_secs(1);

/// The longest line a recording's header may be.
const MOST_HEADER: usize = 1024 * 1024;
/// The longest line an event may be: the most output written all as escapes of six bytes a
/// byte, with room for its time and code.
const MOST_LINE: usize = MOST_OUTPUT * 6 + 4096;
/// How much of the start of a file is looked through for the version of a recording whose
/// header doesn't read.
const MOST_LOOKED_FOR_VERSION: usize = 4096;

/// A recording, read and checked.
#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    /// The screen's size, in columns and rows, as the recording starts.
    pub width: u16,
    pub height: u16,
    /// What happened, at the times it was recorded; `played` gives the times it's played at.
    pub events: Vec<Event>,
    /// How long it plays, to its last event.
    pub length: Duration,
    /// Where it stops until the next click, at the times they're played at.
    pub marks: Vec<Mark>,
}

/// One thing that happened in a recording.
#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub at: Duration,
    pub what: What,
}

/// What happened. Input is left out as a recording is read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum What {
    /// What a program wrote to the screen.
    Output(String),
    /// The screen became this many columns and rows.
    Resize(u16, u16),
    /// A place to stop, with its label.
    Mark(String),
}

/// A place a recording stops until the next click.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub at: Duration,
    pub label: String,
}

/// What a recording is, without its output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordingFacts {
    pub width: u16,
    pub height: u16,
    pub length: Duration,
    pub marks: usize,
}

/// Reads a recording whole.
pub fn read(bytes: &[u8]) -> Result<Recording, CantPlay> {
    let mut events = Vec::new();
    let mut marks = Vec::new();
    let read = walk(bytes, |at, played, seen| {
        let what = match seen {
            Seen::Output(data) => What::Output(text(data)),
            Seen::Resize(width, height) => What::Resize(width, height),
            Seen::Mark(data) => {
                let label = text(data);
                marks.push(Mark { at: played, label: label.clone() });
                What::Mark(label)
            }
        };
        events.push(Event { at, what });
    })?;
    Ok(Recording { width: read.width, height: read.height, events, length: read.length, marks })
}

/// Says what a recording is, from one pass over it that keeps no output.
pub fn facts(bytes: &[u8]) -> Result<RecordingFacts, CantPlay> {
    let mut marks = 0;
    let read = walk(bytes, |_, _, seen| {
        if let Seen::Mark(_) = seen {
            marks += 1;
        }
    })?;
    Ok(RecordingFacts { width: read.width, height: read.height, length: read.length, marks })
}

/// The events at the times they're played at: each wait longer than a second is a second.
pub fn played(recording: &Recording) -> Vec<Event> {
    let mut timing = Timing::default();
    let mut events = Vec::with_capacity(recording.events.len());
    for event in &recording.events {
        events.push(Event { at: timing.step(event.at), what: event.what.clone() });
    }
    events
}

/// Turns recorded times into played ones, one event after another.
#[derive(Default)]
struct Timing {
    recorded: Duration,
    played: Duration,
}

impl Timing {
    fn step(&mut self, at: Duration) -> Duration {
        self.played += at.saturating_sub(self.recorded).min(MOST_GAP);
        self.recorded = at;
        self.played
    }
}

/// An event that's kept, as it lies in the file.
enum Seen<'a> {
    Output(Cow<'a, [u8]>),
    Resize(u16, u16),
    Mark(Cow<'a, [u8]>),
}

/// What reading the whole recording found besides its events.
struct Read {
    width: u16,
    height: u16,
    length: Duration,
}

/// Reads the header and each event in turn, checking every limit before handing an event on
/// with its recorded and played times.
fn walk<'a>(bytes: &'a [u8], mut each: impl FnMut(Duration, Duration, Seen<'a>)) -> Result<Read, CantPlay> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    // The header's end is looked for no further than the longest header can be.
    let looked = &bytes[..bytes.len().min(MOST_HEADER + 1)];
    let header_end = looked.iter().position(|b| *b == b'\n').unwrap_or(looked.len());
    let header = bytes[..header_end].trim_ascii();
    if header.first() != Some(&b'{') {
        return Err(CantPlay::NotARecording);
    }
    if bytes.len() > MOST_BYTES {
        return Err(CantPlay::TooLarge);
    }
    if header.len() > MOST_HEADER {
        return Err(if older_version(bytes) { CantPlay::KindNotPlayed } else { CantPlay::TooLarge });
    }
    let Ok(header) = serde_json::from_slice::<Header>(header) else {
        return Err(if older_version(bytes) { CantPlay::KindNotPlayed } else { CantPlay::Damaged });
    };
    match header.version {
        None => return Err(CantPlay::NotARecording),
        Some(version) if version.as_u64() != Some(2) => return Err(CantPlay::KindNotPlayed),
        Some(_) => {}
    }
    let width = side(header.width.as_ref(), MOST_WIDTH)?;
    let height = side(header.height.as_ref(), MOST_HEIGHT)?;

    let mut events = 0;
    let mut last = 0.0;
    let mut timing = Timing::default();
    for line in bytes[header_end..].split(|b| *b == b'\n') {
        if line.len() > MOST_LINE {
            return Err(CantPlay::TooLarge);
        }
        let line = line.trim_ascii();
        if line.is_empty() {
            continue;
        }
        events += 1;
        if events > MOST_EVENTS {
            return Err(CantPlay::TooLarge);
        }
        let (time, code, data) = serde_json::from_slice::<(f64, Data<'a>, Data<'a>)>(line).map_err(|_| CantPlay::Damaged)?;
        if time.is_nan() || time < last {
            return Err(CantPlay::Damaged);
        }
        if time > MOST_LENGTH.as_secs_f64() {
            return Err(CantPlay::TooLarge);
        }
        last = time;
        if data.0.len() > MOST_OUTPUT {
            return Err(CantPlay::TooLarge);
        }
        let seen = match &*code.0 {
            b"o" => Seen::Output(data.0),
            b"m" => Seen::Mark(data.0),
            b"r" => {
                let (width, height) = size(&data.0)?;
                Seen::Resize(width, height)
            }
            // Input, and codes not known here.
            _ => continue,
        };
        let at = Duration::from_secs_f64(time);
        each(at, timing.step(at), seen);
    }
    Ok(Read { width, height, length: timing.played })
}

/// The header's fields that matter; the others are passed over unread.
#[derive(serde::Deserialize)]
struct Header {
    version: Option<Value>,
    width: Option<Value>,
    height: Option<Value>,
}

/// A side of the screen as the header gives it.
fn side(value: Option<&Value>, most: u16) -> Result<u16, CantPlay> {
    match value.and_then(Value::as_u64) {
        None | Some(0) => Err(CantPlay::Damaged),
        Some(side) if side > u64::from(most) => Err(CantPlay::TooLarge),
        Some(side) => Ok(side as u16),
    }
}

/// A resize's size, written as columns, `x`, rows.
fn size(data: &[u8]) -> Result<(u16, u16), CantPlay> {
    let mut parts = data.split(|b| *b == b'x');
    let (Some(width), Some(height), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(CantPlay::Damaged);
    };
    let number = |digits: &[u8]| {
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            return Err(CantPlay::Damaged);
        }
        Ok(digits.iter().fold(0u32, |n, digit| n.saturating_mul(10).saturating_add(u32::from(digit - b'0'))))
    };
    let (width, height) = (number(width)?, number(height)?);
    if width == 0 || height == 0 {
        return Err(CantPlay::Damaged);
    }
    if width > u32::from(MOST_WIDTH) || height > u32::from(MOST_HEIGHT) {
        return Err(CantPlay::TooLarge);
    }
    Ok((width as u16, height as u16))
}

/// Whether the start of a file whose header doesn't read names a version other than the
/// second, as asciinema's first format does when written over many lines.
fn older_version(bytes: &[u8]) -> bool {
    let start = &bytes[..bytes.len().min(MOST_LOOKED_FOR_VERSION)];
    let key = b"\"version\"";
    let Some(found) = start.windows(key.len()).position(|w| w == key) else {
        return false;
    };
    let rest = start[found + key.len()..].trim_ascii_start();
    let Some(rest) = rest.strip_prefix(b":") else {
        return false;
    };
    let rest = rest.trim_ascii_start();
    let digits = &rest[..rest.iter().position(|b| !b.is_ascii_digit()).unwrap_or(rest.len())];
    !digits.is_empty() && digits != b"2"
}

/// A string of an event, as the bytes it holds, borrowed from the line where it has no escapes.
/// Its bytes needn't be UTF-8.
struct Data<'a>(Cow<'a, [u8]>);

impl<'de> Deserialize<'de> for Data<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Bytes;

        impl<'de> Visitor<'de> for Bytes {
            type Value = Data<'de>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string")
            }

            fn visit_borrowed_bytes<E: de::Error>(self, bytes: &'de [u8]) -> Result<Self::Value, E> {
                Ok(Data(Cow::Borrowed(bytes)))
            }

            fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<Self::Value, E> {
                Ok(Data(Cow::Owned(bytes.to_vec())))
            }

            fn visit_byte_buf<E: de::Error>(self, bytes: Vec<u8>) -> Result<Self::Value, E> {
                Ok(Data(Cow::Owned(bytes)))
            }
        }

        deserializer.deserialize_bytes(Bytes)
    }
}

/// An event's bytes as text. What isn't UTF-8, such as half of a character, becomes the
/// replacement character, once for each half a character written as an escape.
fn text(data: Cow<'_, [u8]>) -> String {
    let mut bytes = match data {
        Cow::Owned(bytes) => match String::from_utf8(bytes) {
            Ok(text) => return text,
            Err(not) => not.into_bytes(),
        },
        Cow::Borrowed(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) => return text.to_owned(),
            Err(_) => bytes.to_vec(),
        },
    };
    let mut out = String::with_capacity(bytes.len());
    let mut rest = &mut bytes[..];
    loop {
        match std::str::from_utf8(rest) {
            Ok(text) => {
                out.push_str(text);
                return out;
            }
            Err(not) => {
                let (good, bad) = rest.split_at_mut(not.valid_up_to());
                out.push_str(std::str::from_utf8(good).unwrap_or_default());
                out.push(char::REPLACEMENT_CHARACTER);
                // Half of a character written as an escape is kept as three bytes, which
                // aren't UTF-8 on their own.
                let skip = match bad {
                    [0xed, 0xa0..=0xbf, 0x80..=0xbf, ..] => 3,
                    _ => not.error_len().unwrap_or(bad.len()),
                };
                rest = &mut bad[skip..];
            }
        }
    }
}

/// Makes a recording of the given screen and events, each its time in seconds, its code and
/// its data.
#[cfg(feature = "make")]
pub fn make_test_cast(width: u16, height: u16, lines: &[(f64, &str, &str)]) -> Vec<u8> {
    let mut out = format!("{{\"version\": 2, \"width\": {width}, \"height\": {height}}}\n");
    for line in lines {
        out.push_str(&serde_json::to_string(line).unwrap_or_default());
        out.push('\n');
    }
    out.into_bytes()
}

/// A session being made: its events so far, and the time of the last.
#[cfg(feature = "make")]
struct Session {
    at: f64,
    lines: Vec<(f64, &'static str, String)>,
}

#[cfg(feature = "make")]
impl Session {
    fn add(&mut self, wait: f64, code: &'static str, data: &str) {
        self.at += wait;
        self.lines.push(((self.at * 1000.0).round() / 1000.0, code, data.to_owned()));
    }

    fn typed(&mut self, command: &str) {
        for c in command.chars() {
            self.add(0.09, "o", c.encode_utf8(&mut [0; 4]));
        }
        self.add(0.4, "o", "\r\n");
    }
}

/// Makes a short recording of a session: a prompt, a command typed a character at a time,
/// coloured output, a mark, a second command and a full-screen program in the alternate screen
/// that exits.
#[cfg(feature = "make")]
pub fn make_demo_cast() -> Vec<u8> {
    let prompt = "\u{1b}[1;32mdemo\u{1b}[0m:\u{1b}[1;34m~/talk\u{1b}[0m$ ";
    let screen = |load: &str| {
        let mut screen = format!("\u{1b}[H\u{1b}[7m top - load average: {load}\u{1b}[K\u{1b}[0m\r\n\r\n");
        screen.push_str("\u{1b}[1m  PID USER      %CPU COMMAND\u{1b}[0m\r\n");
        for (pid, cpu, command) in [(101, "12.0", "build"), (202, " 3.5", "editor"), (303, " 0.4", "shell")] {
            screen.push_str(&format!("  {pid} demo      {cpu} {command}\u{1b}[K\r\n"));
        }
        screen.push_str("\u{1b}[J");
        screen
    };
    let mut session = Session { at: 0.2, lines: Vec::new() };
    session.add(0.0, "o", prompt);
    session.typed("ls --color");
    session.add(0.1, "o", "\u{1b}[1;34mnotes\u{1b}[0m  \u{1b}[1;32mrun.sh\u{1b}[0m  slides.md  \u{1b}[31mold.tar\u{1b}[0m\r\n");
    session.add(0.05, "o", prompt);
    session.add(0.3, "m", "listed");
    session.typed("top");
    session.add(0.2, "o", &format!("\u{1b}[?1049h\u{1b}[?25l\u{1b}[2J{}", screen("0.52")));
    session.add(1.0, "o", &screen("0.61"));
    session.add(1.0, "o", &screen("0.58"));
    session.add(0.6, "o", "\u{1b}[?25h\u{1b}[?1049l");
    session.add(0.05, "o", prompt);
    let lines: Vec<(f64, &str, &str)> = session.lines.iter().map(|(at, code, data)| (*at, *code, data.as_str())).collect();
    make_test_cast(80, 24, &lines)
}
