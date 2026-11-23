//! The words the app and the helper speak, one frame at a time over the helper's standard input and
//! output: `[kind: u8][length: u32, little-endian][bytes]`. A frame is never longer than
//! [`MOST_FRAME`], and a length over that is refused before anything is allocated for it. This module
//! has no pseudo-terminal in it, so the app can share it without linking one.
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::path::PathBuf;

/// The most bytes one frame may carry.
pub const MOST_FRAME: u32 = 1 << 20;
/// The most bytes of the program's output in one `Output` frame.
pub const MOST_OUTPUT_CHUNK: usize = 64 * 1024;

/// Frame kinds, app to helper.
pub const TO_START: u8 = 1;
pub const TO_INPUT: u8 = 2;
pub const TO_RESIZE: u8 = 3;
pub const TO_END: u8 = 4;
/// Frame kinds, helper to app.
pub const FROM_STARTED: u8 = 1;
pub const FROM_OUTPUT: u8 = 2;
pub const FROM_EXITED: u8 = 3;
pub const FROM_FAILED: u8 = 4;

/// What to run, and how big its terminal is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub cols: u16,
    pub rows: u16,
    pub env: Vec<(String, String)>,
}

/// How the program ended: its exit code, or the signal that ended it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exit {
    pub code: Option<i32>,
    pub signal: Option<String>,
}

/// What the app says to the helper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToHelper {
    Start(StartSpec),
    Input(Vec<u8>),
    Resize(u16, u16),
    End,
}

/// What the helper says to the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FromHelper {
    Started,
    Output(Vec<u8>),
    Exited(Exit),
    Failed(String),
}

fn bad(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.to_string())
}

/// Writes one frame (in a single write, so two threads sharing a lock never interleave).
pub fn write_frame(out: &mut impl Write, kind: u8, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > MOST_FRAME as usize {
        return Err(bad("a frame longer than the most allowed"));
    }
    let mut frame = Vec::with_capacity(5 + bytes.len());
    frame.push(kind);
    frame.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    frame.extend_from_slice(bytes);
    out.write_all(&frame)?;
    out.flush()
}

/// Reads one frame. `Ok(None)` is the input ending cleanly between frames; ending inside a frame, or a
/// length over [`MOST_FRAME`], is an error (the length is checked before any room is made for it).
pub fn read_frame(input: &mut impl Read) -> io::Result<Option<(u8, Vec<u8>)>> {
    let mut head = [0u8; 5];
    let mut got = 0;
    while got < head.len() {
        match input.read(&mut head[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]);
    if len > MOST_FRAME {
        return Err(bad("a frame claims more than the most allowed"));
    }
    let mut bytes = vec![0u8; len as usize];
    input.read_exact(&mut bytes)?;
    Ok(Some((head[0], bytes)))
}

impl ToHelper {
    pub fn encode(&self) -> (u8, Vec<u8>) {
        match self {
            ToHelper::Start(spec) => (TO_START, serde_json::to_vec(spec).unwrap_or_default()),
            ToHelper::Input(bytes) => (TO_INPUT, bytes.clone()),
            ToHelper::Resize(cols, rows) => {
                let mut b = cols.to_le_bytes().to_vec();
                b.extend_from_slice(&rows.to_le_bytes());
                (TO_RESIZE, b)
            }
            ToHelper::End => (TO_END, Vec::new()),
        }
    }

    pub fn decode(kind: u8, bytes: &[u8]) -> io::Result<ToHelper> {
        match kind {
            TO_START => serde_json::from_slice(bytes)
                .map(ToHelper::Start)
                .map_err(|_| bad("a start frame that isn't a start")),
            TO_INPUT => Ok(ToHelper::Input(bytes.to_vec())),
            TO_RESIZE if bytes.len() == 4 => Ok(ToHelper::Resize(
                u16::from_le_bytes([bytes[0], bytes[1]]),
                u16::from_le_bytes([bytes[2], bytes[3]]),
            )),
            TO_RESIZE => Err(bad("a resize frame of the wrong size")),
            TO_END if bytes.is_empty() => Ok(ToHelper::End),
            TO_END => Err(bad("an end frame with something in it")),
            _ => Err(bad("a frame of a kind nobody knows")),
        }
    }

    pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
        let (kind, bytes) = self.encode();
        write_frame(out, kind, &bytes)
    }
}

impl FromHelper {
    pub fn encode(&self) -> (u8, Vec<u8>) {
        match self {
            FromHelper::Started => (FROM_STARTED, Vec::new()),
            FromHelper::Output(bytes) => (FROM_OUTPUT, bytes.clone()),
            FromHelper::Exited(exit) => (FROM_EXITED, serde_json::to_vec(exit).unwrap_or_default()),
            FromHelper::Failed(why) => (FROM_FAILED, why.as_bytes().to_vec()),
        }
    }

    pub fn decode(kind: u8, bytes: &[u8]) -> io::Result<FromHelper> {
        match kind {
            FROM_STARTED if bytes.is_empty() => Ok(FromHelper::Started),
            FROM_STARTED => Err(bad("a started frame with something in it")),
            FROM_OUTPUT => Ok(FromHelper::Output(bytes.to_vec())),
            FROM_EXITED => serde_json::from_slice(bytes)
                .map(FromHelper::Exited)
                .map_err(|_| bad("an exited frame that isn't one")),
            FROM_FAILED => Ok(FromHelper::Failed(String::from_utf8_lossy(bytes).into_owned())),
            _ => Err(bad("a frame of a kind nobody knows")),
        }
    }

    pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
        let (kind, bytes) = self.encode();
        write_frame(out, kind, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let mut buf = Vec::new();
        ToHelper::Resize(120, 40).write(&mut buf).unwrap();
        FromHelper::Failed("no".into()).write(&mut buf).unwrap();
        let mut r = &buf[..];
        let (k, b) = read_frame(&mut r).unwrap().unwrap();
        assert_eq!(ToHelper::decode(k, &b).unwrap(), ToHelper::Resize(120, 40));
        let (k, b) = read_frame(&mut r).unwrap().unwrap();
        assert_eq!(FromHelper::decode(k, &b).unwrap(), FromHelper::Failed("no".into()));
        assert!(read_frame(&mut r).unwrap().is_none());
    }

    #[test]
    fn a_huge_claim_is_refused_without_reading_on() {
        let mut head = vec![TO_INPUT];
        head.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_frame(&mut &head[..]).is_err());
    }

    #[test]
    fn a_cut_frame_is_an_error() {
        let mut head = vec![TO_INPUT];
        head.extend_from_slice(&10u32.to_le_bytes());
        head.extend_from_slice(b"abc");
        assert!(read_frame(&mut &head[..]).is_err());
        assert!(read_frame(&mut &[TO_END, 0][..]).is_err());
    }
}
