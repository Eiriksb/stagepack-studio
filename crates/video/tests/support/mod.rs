//! What the video tests share: a small editor for the boxes of an MP4, so a made video can be
//! damaged or changed in exact ways, and the pictures the made videos show.

#![allow(dead_code)]

use std::io::Cursor;
use std::time::{Duration, Instant};

use slides_video::{Frame, Playing, ReadSeek};

/// The boxes that hold other boxes, and how many bytes of their own come before them.
const HOLDERS: [(&[u8; 4], usize); 9] = [
    (b"moov", 0),
    (b"trak", 0),
    (b"mdia", 0),
    (b"minf", 0),
    (b"stbl", 0),
    (b"dinf", 0),
    (b"edts", 0),
    (b"stsd", 8),
    (b"avc1", 78),
];

/// One box of an MP4: what it is, and either its bytes or the boxes inside it.
#[derive(Clone, Debug)]
pub struct Mp4Box {
    pub kind: [u8; 4],
    pub head: Vec<u8>,
    pub data: Vec<u8>,
    pub inside: Vec<Mp4Box>,
}

/// Reads the boxes of `bytes`, which must be well made.
pub fn parse(bytes: &[u8]) -> Vec<Mp4Box> {
    let mut boxes = Vec::new();
    let mut at = 0;
    while at + 8 <= bytes.len() {
        let mut size = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = bytes[at + 4..at + 8].try_into().unwrap();
        let mut start = at + 8;
        if size == 1 {
            size = u64::from_be_bytes(bytes[at + 8..at + 16].try_into().unwrap()) as usize;
            start = at + 16;
        } else if size == 0 {
            size = bytes.len() - at;
        }
        let body = &bytes[start..at + size];
        boxes.push(match HOLDERS.iter().find(|(name, _)| **name == kind) {
            Some((_, own)) => {
                Mp4Box { kind, head: body[..*own].to_vec(), data: Vec::new(), inside: parse(&body[*own..]) }
            }
            None => Mp4Box { kind, head: Vec::new(), data: body.to_vec(), inside: Vec::new() },
        });
        at += size;
    }
    boxes
}

/// Writes `boxes` back out, with their sizes worked out again.
pub fn write(boxes: &[Mp4Box]) -> Vec<u8> {
    let mut out = Vec::new();
    for one in boxes {
        let mut body = one.head.clone();
        body.extend_from_slice(&one.data);
        body.extend_from_slice(&write(&one.inside));
        out.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
        out.extend_from_slice(&one.kind);
        out.extend_from_slice(&body);
    }
    out
}

/// The first box at `path`, such as `moov/trak/mdia/mdhd`.
pub fn find<'a>(boxes: &'a mut [Mp4Box], path: &str) -> &'a mut Mp4Box {
    let (first, rest) = path.split_once('/').unwrap_or((path, ""));
    let found = boxes.iter_mut().find(|b| b.kind == first.as_bytes()).unwrap_or_else(|| panic!("no {first}"));
    if rest.is_empty() { found } else { find(&mut found.inside, rest) }
}

/// Changes `video` with `edit`, on its boxes.
pub fn edit(video: &[u8], edit: impl FnOnce(&mut Vec<Mp4Box>)) -> Vec<u8> {
    let mut boxes = parse(video);
    edit(&mut boxes);
    write(&boxes)
}

pub fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(data[at..at + 4].try_into().unwrap())
}

pub fn set_u32(data: &mut [u8], at: usize, value: u32) {
    data[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

/// A table box (stts, ctts, stsz, stco and the like) with `entries`, after its version and flags.
pub fn table(version: u8, fields: &[u32]) -> Vec<u8> {
    let mut data = vec![version, 0, 0, 0];
    for field in fields {
        data.extend_from_slice(&field.to_be_bytes());
    }
    data
}

/// The same video with its index first, before the pictures, and every chunk's place moved to match.
pub fn index_first(video: &[u8]) -> Vec<u8> {
    let mut boxes = parse(video);
    let moov_at = boxes.iter().position(|b| &b.kind == b"moov").unwrap();
    let mut moov = boxes.remove(moov_at);
    let moved = write(std::slice::from_ref(&moov)).len() as u32;
    let stco = find(&mut moov.inside, "trak/mdia/minf/stbl/stco");
    let count = u32_at(&stco.data, 4) as usize;
    for i in 0..count {
        let at = 8 + i * 4;
        let offset = u32_at(&stco.data, at);
        set_u32(&mut stco.data, at, offset + moved);
    }
    boxes.insert(1, moov);
    write(&boxes)
}

/// The kinds of the boxes at the top of `video`, in order.
pub fn top_kinds(video: &[u8]) -> Vec<String> {
    parse(video).iter().map(|b| String::from_utf8_lossy(&b.kind).into_owned()).collect()
}

/// Puts `sps` in place of the video's own sequence parameters, as a damaged or odd file might.
pub fn with_sps(video: &[u8], sps: &[u8]) -> Vec<u8> {
    edit(video, |boxes| {
        let avcc = find(boxes, "moov/trak/mdia/minf/stbl/stsd/avc1/avcC");
        let old = &avcc.data;
        let sps_len = u16::from_be_bytes([old[6], old[7]]) as usize;
        let rest = old[8 + sps_len..].to_vec();
        let mut data = vec![old[0], sps[1], sps[2], sps[3], old[4], 0xE1];
        data.extend_from_slice(&(sps.len() as u16).to_be_bytes());
        data.extend_from_slice(sps);
        data.extend_from_slice(&rest);
        avcc.data = data;
    })
}

/// Writes the bits of a sequence parameter set.
struct Bits {
    bytes: Vec<u8>,
    used: u32,
}

impl Bits {
    fn bit(&mut self, on: bool) {
        if self.used.is_multiple_of(8) {
            self.bytes.push(0);
        }
        if on {
            *self.bytes.last_mut().unwrap() |= 0x80 >> (self.used % 8);
        }
        self.used += 1;
    }

    fn bits(&mut self, value: u32, count: u32) {
        for i in (0..count).rev() {
            self.bit(value >> i & 1 == 1);
        }
    }

    fn ue(&mut self, value: u32) {
        let coded = value + 1;
        let width = 32 - coded.leading_zeros();
        self.bits(0, width - 1);
        self.bits(coded, width);
    }
}

/// A sequence parameter set: what a stream says of its pictures' size and kind.
pub struct Sps {
    pub profile: u8,
    pub width_blocks: u32,
    pub height_blocks: u32,
    pub chroma: u32,
    pub bit_depth: u32,
    pub progressive: bool,
}

impl Sps {
    pub fn sized(width: u32, height: u32) -> Sps {
        Sps { profile: 66, width_blocks: width / 16, height_blocks: height / 16, chroma: 1, bit_depth: 8, progressive: true }
    }

    pub fn nal(&self) -> Vec<u8> {
        let mut bits = Bits { bytes: Vec::new(), used: 0 };
        bits.bits(self.profile as u32, 8);
        bits.bits(0, 8);
        bits.bits(51, 8);
        bits.ue(0);
        if self.profile >= 100 {
            bits.ue(self.chroma);
            if self.chroma == 3 {
                bits.bit(false);
            }
            bits.ue(self.bit_depth - 8);
            bits.ue(self.bit_depth - 8);
            bits.bit(false);
            bits.bit(false);
        }
        bits.ue(0);
        bits.ue(2);
        bits.ue(1);
        bits.bit(false);
        bits.ue(self.width_blocks - 1);
        bits.ue(self.height_blocks - 1);
        bits.bit(self.progressive);
        if !self.progressive {
            bits.bit(false);
        }
        bits.bit(true);
        bits.bit(false);
        bits.bit(false);
        bits.bit(true);
        while !bits.used.is_multiple_of(8) {
            bits.bit(false);
        }
        // The bytes a stream puts in so its contents never look like the start of a unit.
        let mut nal = vec![0x67];
        let mut zeros = 0;
        for byte in bits.bytes {
            if zeros >= 2 && byte <= 3 {
                nal.push(3);
                zeros = 0;
            }
            zeros = if byte == 0 { zeros + 1 } else { 0 };
            nal.push(byte);
        }
        nal
    }
}

/// Opens `bytes` as a video file would be opened.
pub fn reader(bytes: Vec<u8>) -> Box<dyn ReadSeek> {
    Box::new(Cursor::new(bytes))
}

/// A solid picture of `colour`, as blue, green, red.
pub fn solid(width: u32, height: u32, colour: [u8; 3]) -> Vec<u8> {
    let mut bgra = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..width * height {
        bgra.extend_from_slice(&[colour[0], colour[1], colour[2], 255]);
    }
    bgra
}

/// The colour of picture `i` of a made video: each different from those beside it by more than
/// an encoding's errors could account for, and inside what every colour range can hold.
pub fn colour_of(i: u32) -> [u8; 3] {
    [(40 + i * 53 % 180) as u8, (40 + i * 97 % 180) as u8, (40 + i * 31 % 180) as u8]
}

/// A made video of `frames` solid pictures, each `colour_of` its number.
pub fn coloured_video(width: u32, height: u32, frames: u32, frames_a_second: u32) -> Vec<u8> {
    slides_video::make_test_video(width, height, frames, frames_a_second, |i| solid(width, height, colour_of(i)))
}

/// Whether every pixel of `frame` is `colour` within `levels`, and opaque.
pub fn is_colour(frame: &Frame, colour: [u8; 3], levels: u8) -> bool {
    frame.bgra.len() == (frame.width * frame.height * 4) as usize
        && frame.bgra.as_chunks::<4>().0.iter().all(|px| px[3] == 255 && (0..3).all(|c| px[c].abs_diff(colour[c]) <= levels))
}

/// The colour in the middle of `frame`.
pub fn middle(frame: &Frame) -> [u8; 3] {
    let at = ((frame.height / 2 * frame.width + frame.width / 2) * 4) as usize;
    [frame.bgra[at], frame.bgra[at + 1], frame.bgra[at + 2]]
}

/// Asks `playing` for the frame due at `seconds` until one comes, for as long as two seconds.
pub fn next_frame(playing: &mut Playing, seconds: f64) -> Option<Frame> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if let Some(frame) = playing.frame_for(seconds) {
            return Some(frame);
        }
        if playing.ended() || playing.failed().is_some() {
            return None;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    None
}

/// Waits up to `limit` for `done`.
pub fn wait_for(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}
