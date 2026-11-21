//! A texture's size from its header, read before anything of it is decoded.

use crate::limits::{MOST_JPEG_PARTS, MOST_JPEG_PASSES, MOST_TEXTURE_PIXELS, MOST_TEXTURE_SIDE};

/// How much of a JPEG is read at once when walking through runs of it.
const JPEG_PIECE: u64 = 64 * 1024;
use crate::CantDraw;

/// The kinds of picture a texture may be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Picture {
    Png,
    Jpeg,
}

/// The kind of picture `start` begins, if it's one a texture may be.
pub(crate) fn picture_kind(start: &[u8]) -> Option<Picture> {
    if start.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Picture::Png)
    } else if start.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Picture::Jpeg)
    } else {
        None
    }
}

/// What a picture's header says of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Size {
    pub width: u32,
    pub height: u32,
    /// How many numbers each pixel is stored as: the colours, and alpha if it has one.
    pub channels: u8,
    /// How many bits each of those numbers has.
    pub bits: u8,
}

/// The size a picture of `len` bytes says it is, checked against the limits. `peek(at, n)` gives
/// the `n` bytes at `at` of it, which the caller checks lie inside it.
pub(crate) fn checked_size(
    kind: Picture,
    len: u64,
    peek: &mut dyn FnMut(u64, u64) -> Result<Vec<u8>, CantDraw>,
) -> Result<Size, CantDraw> {
    let size = match kind {
        Picture::Png => png_size(len, peek)?,
        Picture::Jpeg => jpeg_size(len, peek)?,
    };
    let Size { width, height, .. } = size;
    if width == 0 || height == 0 {
        return Err(CantDraw::Damaged);
    }
    if width > MOST_TEXTURE_SIDE || height > MOST_TEXTURE_SIDE || u64::from(width) * u64::from(height) > MOST_TEXTURE_PIXELS {
        return Err(CantDraw::TooLarge);
    }
    Ok(size)
}

fn png_size(len: u64, peek: &mut dyn FnMut(u64, u64) -> Result<Vec<u8>, CantDraw>) -> Result<Size, CantDraw> {
    if len < 26 {
        return Err(CantDraw::Damaged);
    }
    let head = peek(0, 26)?;
    if &head[12..16] != b"IHDR" {
        return Err(CantDraw::Damaged);
    }
    let number = |at: usize| u32::from_be_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
    let (bits, colour) = (head[24], head[25]);
    // The kinds of pixel the format allows, by their colour type: grey, colour, a palette, grey
    // with alpha, colour with alpha.
    let channels = match (colour, bits) {
        (0, 1 | 2 | 4 | 8 | 16) => 1,
        (2, 8 | 16) => 3,
        (3, 1 | 2 | 4 | 8) => 1,
        (4, 8 | 16) => 2,
        (6, 8 | 16) => 4,
        _ => return Err(CantDraw::Damaged),
    };
    Ok(Size { width: number(16), height: number(20), channels, bits })
}

/// How far the fill bytes starting at `at` run: where the last of them is, just before the marker
/// they come before, or `len` if they run to the end. Read a piece at a time.
fn after_fill(len: u64, at: u64, read: &mut dyn FnMut(u64, u64) -> Result<Vec<u8>, CantDraw>, passes: &mut u32) -> Result<u64, CantDraw> {
    let mut at = at;
    while at < len {
        *passes += 1;
        if *passes > MOST_JPEG_PASSES {
            return Err(CantDraw::Damaged);
        }
        let piece = read(at, JPEG_PIECE.min(len - at))?;
        match piece.iter().position(|b| *b != 0xFF) {
            // The run ended with the piece before: its last fill byte is the one just before.
            Some(0) => return Ok(at - 1),
            Some(k) => return Ok(at + k as u64 - 1),
            None => at += piece.len() as u64,
        }
    }
    Ok(len)
}

/// One step of walking a JPEG's parts, the same for every walk here and as the decoder walks
/// them: fill bytes are passed over, a marker that stands alone outside the pictures' coded data
/// is damage, and every other part has its length.
enum Step {
    /// A part: its marker, where it starts and its length after the marker.
    Part(u8, u64, u64),
    End,
}

fn step(len: u64, at: &mut u64, read: &mut dyn FnMut(u64, u64) -> Result<Vec<u8>, CantDraw>, passes: &mut u32) -> Result<Step, CantDraw> {
    *passes += 1;
    if *passes > MOST_JPEG_PASSES {
        return Err(CantDraw::Damaged);
    }
    if len < *at + 2 {
        return Err(CantDraw::Damaged);
    }
    let head = read(*at, 2)?;
    if head[0] != 0xFF {
        return Err(CantDraw::Damaged);
    }
    if head[1] == 0xFF {
        *at = after_fill(len, *at, read, passes)?;
        return step(len, at, read, passes);
    }
    match head[1] {
        0xD9 => return Ok(Step::End),
        // Restarts and the like belong only inside a picture's coded data, and a second start
        // isn't a part of one picture.
        0x01 | 0xD0..=0xD8 => return Err(CantDraw::Damaged),
        _ => {}
    }
    if len - *at < 4 {
        return Err(CantDraw::Damaged);
    }
    let size = read(*at + 2, 2)?;
    let part = u64::from(u16::from_be_bytes([size[0], size[1]]));
    if part < 2 || len - *at - 2 < part {
        return Err(CantDraw::Damaged);
    }
    Ok(Step::Part(head[1], *at, part))
}

/// Walks a JPEG's parts to the one that gives its size.
fn jpeg_size(len: u64, peek: &mut dyn FnMut(u64, u64) -> Result<Vec<u8>, CantDraw>) -> Result<Size, CantDraw> {
    let mut at = 2;
    let mut passes = 0;
    loop {
        let Step::Part(marker, start, part) = step(len, &mut at, peek, &mut passes)? else { return Err(CantDraw::Damaged) };
        // The starts of frames, of every kind but those that aren't frames.
        if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            if part < 8 {
                return Err(CantDraw::Damaged);
            }
            let frame = peek(start + 4, 6)?;
            let height = u16::from_be_bytes([frame[1], frame[2]]);
            let width = u16::from_be_bytes([frame[3], frame[4]]);
            let channels = frame[5];
            if !matches!(channels, 1 | 3 | 4) {
                return Err(CantDraw::Damaged);
            }
            return Ok(Size { width: u32::from(width), height: u32::from(height), channels, bits: frame[0] });
        }
        if marker == 0xDA {
            // The pictures themselves, before any size.
            return Err(CantDraw::Damaged);
        }
        at = start + 2 + part;
    }
}

/// Of a JPEG of `len` bytes, only what's needed to decode its picture, read a piece at a time:
/// its tables, its frame and its scans, up to its end. What else is kept in a JPEG (notes,
/// colour profiles, a smaller copy of the picture, anything after its end) is left out, so
/// neither its size nor the decoder's copies of it count against the memory decoding takes. It's
/// walked twice: once to find how long what's needed is, so it's then held in one allocation of
/// just that. A JPEG that stops before its end is damaged.
pub(crate) fn jpeg_needed(len: u64, read: &mut dyn FnMut(u64, u64) -> Result<Vec<u8>, CantDraw>) -> Result<Vec<u8>, CantDraw> {
    let mut needed = 0u64;
    walk_needed(len, read, &mut |bytes| {
        needed += bytes.len() as u64;
        Ok(())
    })?;
    let mut out = Vec::new();
    out.try_reserve_exact(usize::try_from(needed).map_err(|_| CantDraw::TooLarge)?).map_err(|_| CantDraw::TooLarge)?;
    walk_needed(len, read, &mut |bytes| {
        if out.len() + bytes.len() > out.capacity() {
            // Changed between the walks.
            return Err(CantDraw::CouldNotRead);
        }
        out.extend_from_slice(bytes);
        Ok(())
    })?;
    Ok(out)
}

fn walk_needed(
    len: u64,
    read: &mut dyn FnMut(u64, u64) -> Result<Vec<u8>, CantDraw>,
    keep: &mut dyn FnMut(&[u8]) -> Result<(), CantDraw>,
) -> Result<(), CantDraw> {
    keep(&[0xFF, 0xD8])?;
    let mut at = 2;
    let mut passes = 0;
    let mut parts = 0;
    while let Step::Part(marker, start, part) = step(len, &mut at, read, &mut passes)? {
        parts += 1;
        if parts > MOST_JPEG_PARTS {
            return Err(CantDraw::Damaged);
        }
        // Kept: everything but notes and kept data (APP1 to APP13, APP15, comments). The
        // first and fourteenth, which say how its colours are stored, are kept.
        if !matches!(marker, 0xE1..=0xED | 0xEF | 0xFE) {
            keep(&read(start, 2 + part)?)?;
        }
        at = start + 2 + part;
        if marker == 0xDA {
            // A scan: its coded data runs to the next marker that isn't a restart.
            loop {
                passes += 1;
                if passes > MOST_JPEG_PASSES {
                    return Err(CantDraw::Damaged);
                }
                let n = JPEG_PIECE.min(len - at);
                if n == 0 {
                    // Cut short in the middle of a picture.
                    return Err(CantDraw::Damaged);
                }
                let piece = read(at, n)?;
                let marker_at = (0..piece.len().saturating_sub(1))
                    .find(|&i| piece[i] == 0xFF && !matches!(piece[i + 1], 0x00 | 0xD0..=0xD7));
                match marker_at {
                    Some(i) => {
                        keep(&piece[..i])?;
                        at += i as u64;
                        break;
                    }
                    None => {
                        // A last byte that may start a marker is looked at again with what
                        // follows it.
                        let whole = if piece.len() > 1 && piece[piece.len() - 1] == 0xFF && at + n < len { n - 1 } else { n };
                        keep(&piece[..whole as usize])?;
                        at += whole;
                    }
                }
            }
        }
    }
    keep(&[0xFF, 0xD9])
}
