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

/// The size a picture of `len` bytes says it is, checked against the limits. `peek(at, n)` gives
/// the `n` bytes at `at` of it, which the caller checks lie inside it.
pub(crate) fn checked_size(
    kind: Picture,
    len: u64,
    peek: &mut dyn FnMut(u64, u64) -> Result<Vec<u8>, CantDraw>,
) -> Result<(u32, u32), CantDraw> {
    let (width, height) = match kind {
        Picture::Png => png_size(len, peek)?,
        Picture::Jpeg => jpeg_size(len, peek)?,
    };
    if width == 0 || height == 0 {
        return Err(CantDraw::Damaged);
    }
    if width > MOST_TEXTURE_SIDE || height > MOST_TEXTURE_SIDE || u64::from(width) * u64::from(height) > MOST_TEXTURE_PIXELS {
        return Err(CantDraw::TooLarge);
    }
    Ok((width, height))
}

fn png_size(len: u64, peek: &mut dyn FnMut(u64, u64) -> Result<Vec<u8>, CantDraw>) -> Result<(u32, u32), CantDraw> {
    if len < 24 {
        return Err(CantDraw::Damaged);
    }
    let head = peek(0, 24)?;
    if &head[12..16] != b"IHDR" {
        return Err(CantDraw::Damaged);
    }
    let number = |at: usize| u32::from_be_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
    Ok((number(16), number(20)))
}

/// Walks a JPEG's parts to the one that gives its size.
fn jpeg_size(len: u64, peek: &mut dyn FnMut(u64, u64) -> Result<Vec<u8>, CantDraw>) -> Result<(u32, u32), CantDraw> {
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
        let part = u64::from(u16::from_be_bytes([head[2], head[3]]));
        if part < 2 || len - at - 2 < part {
            return Err(CantDraw::Damaged);
        }
        // The starts of frames, of every kind but those that aren't frames.
        if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            if part < 7 {
                return Err(CantDraw::Damaged);
            }
            let frame = peek(at + 4, 5)?;
            let height = u16::from_be_bytes([frame[1], frame[2]]);
            let width = u16::from_be_bytes([frame[3], frame[4]]);
            return Ok((u32::from(width), u32::from(height)));
        }
        if marker == 0xD9 || marker == 0xDA {
            // The end, or the pictures themselves, before any size.
            return Err(CantDraw::Damaged);
        }
        at += 2 + part;
    }
    Err(CantDraw::Damaged)
}
