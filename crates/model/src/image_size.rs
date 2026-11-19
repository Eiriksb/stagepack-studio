//! A texture's size from its header, read before anything of it is decoded.

use crate::limits::{MOST_JPEG_PARTS, MOST_TEXTURE_PIXELS, MOST_TEXTURE_SIDE};
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

/// Walks a JPEG's parts to the one that gives its size.
fn jpeg_size(len: u64, peek: &mut dyn FnMut(u64, u64) -> Result<Vec<u8>, CantDraw>) -> Result<Size, CantDraw> {
    let mut at = 2;
    for _ in 0..MOST_JPEG_PARTS {
        if len < at + 4 {
            return Err(CantDraw::Damaged);
        }
        let head = peek(at, 4)?;
        if head[0] != 0xFF {
            return Err(CantDraw::Damaged);
        }
        let marker = head[1];
        if marker == 0xFF {
            // Padding before a marker.
            at += 1;
            continue;
        }
        if matches!(marker, 0x01 | 0xD0..=0xD8) {
            // Markers standing alone, with no length after them.
            at += 2;
            continue;
        }
        let part = u64::from(u16::from_be_bytes([head[2], head[3]]));
        if part < 2 || len - at - 2 < part {
            return Err(CantDraw::Damaged);
        }
        // The starts of frames, of every kind but those that aren't frames.
        if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            if part < 8 {
                return Err(CantDraw::Damaged);
            }
            let frame = peek(at + 4, 6)?;
            let height = u16::from_be_bytes([frame[1], frame[2]]);
            let width = u16::from_be_bytes([frame[3], frame[4]]);
            let channels = frame[5];
            if !matches!(channels, 1 | 3 | 4) {
                return Err(CantDraw::Damaged);
            }
            return Ok(Size { width: u32::from(width), height: u32::from(height), channels, bits: frame[0] });
        }
        if marker == 0xD9 || marker == 0xDA {
            // The end, or the pictures themselves, before any size.
            return Err(CantDraw::Damaged);
        }
        at += 2 + part;
    }
    Err(CantDraw::Damaged)
}
