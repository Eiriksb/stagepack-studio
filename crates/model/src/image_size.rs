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

/// Of a JPEG of `len` bytes, only what's needed to decode its picture, read a piece at a time:
/// its tables, its frame and its scans, up to its end. What else is kept in a JPEG (notes,
/// colour profiles, a smaller copy of the picture, anything after its end) is left out, so
/// neither its size nor the decoder's copies of it count against the memory decoding takes.
pub(crate) fn jpeg_needed(len: u64, read: &mut dyn FnMut(u64, u64) -> Result<Vec<u8>, CantDraw>) -> Result<Vec<u8>, CantDraw> {
    const PIECE: u64 = 64 * 1024;
    let mut out = vec![0xFF, 0xD8];
    let mut at = 2;
    let mut parts = 0;
    while len - at >= 2 {
        let head = read(at, 2)?;
        if head[0] != 0xFF {
            return Err(CantDraw::Damaged);
        }
        let marker = head[1];
        if marker == 0xFF {
            at += 1;
            continue;
        }
        parts += 1;
        if parts > MOST_JPEG_PARTS {
            return Err(CantDraw::Damaged);
        }
        if marker == 0xD9 {
            break;
        }
        if matches!(marker, 0x01 | 0xD0..=0xD7) {
            out.extend_from_slice(&head);
            at += 2;
            continue;
        }
        if len - at < 4 {
            return Err(CantDraw::Damaged);
        }
        let part = u64::from(u16::from_be_bytes(read(at + 2, 2)?.try_into().map_err(|_| CantDraw::Damaged)?));
        if part < 2 || len - at - 2 < part {
            return Err(CantDraw::Damaged);
        }
        // Kept: everything but notes and kept data (APP1 to APP13, APP15, comments). The
        // first and fourteenth, which say how its colours are stored, are kept.
        let left_out = matches!(marker, 0xE1..=0xED | 0xEF | 0xFE);
        if !left_out {
            out.extend_from_slice(&read(at, 2 + part)?);
        }
        at += 2 + part;
        if marker == 0xDA {
            // A scan: its coded data runs to the next marker that isn't a restart.
            loop {
                let n = PIECE.min(len - at);
                if n == 0 {
                    break;
                }
                let piece = read(at, n)?;
                let marker_at = (0..piece.len().saturating_sub(1))
                    .find(|&i| piece[i] == 0xFF && !matches!(piece[i + 1], 0x00 | 0xD0..=0xD7));
                match marker_at {
                    Some(i) => {
                        out.extend_from_slice(&piece[..i]);
                        at += i as u64;
                        break;
                    }
                    None => {
                        // A last byte that may start a marker is looked at again with what
                        // follows it.
                        let keep = if piece.len() > 1 && piece[piece.len() - 1] == 0xFF && at + n < len { n - 1 } else { n };
                        out.extend_from_slice(&piece[..keep as usize]);
                        at += keep;
                    }
                }
            }
        }
    }
    out.extend_from_slice(&[0xFF, 0xD9]);
    // Room grown for it is given back before the picture is decoded beside it.
    out.shrink_to_fit();
    Ok(out)
}
