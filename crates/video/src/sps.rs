//! Reading what an H.264 stream says of its pictures: their size, kind and colours, from its
//! sequence parameter set.

use yuvutils_rs::{YuvRange, YuvStandardMatrix};

use crate::CantPlay;

/// The largest pictures played, either way round.
const MOST_WIDE: u64 = 4096;
const MOST_HIGH: u64 = 2304;

/// What a sequence parameter set says.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sps {
    pub width: u32,
    pub height: u32,
    pub matrix: Matrix,
    pub range: YuvRange,
}

/// How a picture's colours are worked out from what's stored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Matrix {
    Standard(YuvStandardMatrix),
    /// Stored as green, blue and red, with no matrix at all.
    Identity,
}

/// The matrix that a file's number for it names. Those not played here are read as BT.601,
/// as is a file that says none.
pub(crate) fn matrix_of(code: u32) -> Matrix {
    match code {
        0 => Matrix::Identity,
        1 => Matrix::Standard(YuvStandardMatrix::Bt709),
        7 => Matrix::Standard(YuvStandardMatrix::Smpte240),
        9 | 10 => Matrix::Standard(YuvStandardMatrix::Bt2020),
        _ => Matrix::Standard(YuvStandardMatrix::Bt601),
    }
}

/// The profiles the decoder plays: Baseline, Main and High, at 8 bits and 4:2:0.
const PLAYED_PROFILES: [u8; 3] = [66, 77, 100];

/// The profiles whose parameter sets say their chroma and bit depth.
const HIGH_PROFILES: [u8; 12] = [100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134];

/// Reads a sequence parameter set, a whole unit with its one byte of header. A stream of a kind
/// the decoder can't play is `KindNotPlayed`; one too large, as stored or as shown, `TooLarge`;
/// one that can't be read, or that crops a whole block or more from an edge, `Damaged`.
pub(crate) fn read(nal: &[u8]) -> Result<Sps, CantPlay> {
    if nal.first().is_none_or(|header| header & 0x1F != 7) {
        return Err(CantPlay::Damaged);
    }
    let rbsp = unescape(&nal[1..]);
    let mut bits = Bits { bytes: &rbsp, at: 0 };
    let profile = bits.u(8)? as u8;
    bits.u(16)?; // constraints and level
    bits.ue()?; // which set this is
    let (mut chroma, mut depth_luma, mut depth_chroma) = (1, 0, 0);
    if HIGH_PROFILES.contains(&profile) {
        chroma = bits.ue()?;
        if chroma == 3 {
            bits.u(1)?;
        }
        depth_luma = bits.ue()?;
        depth_chroma = bits.ue()?;
        bits.u(1)?;
        if bits.u(1)? == 1 {
            for list in 0..if chroma == 3 { 12 } else { 8 } {
                if bits.u(1)? == 1 {
                    bits.skip_scaling_list(if list < 6 { 16 } else { 64 })?;
                }
            }
        }
    }
    if chroma != 1 || depth_luma != 0 || depth_chroma != 0 || !PLAYED_PROFILES.contains(&profile) {
        return Err(CantPlay::KindNotPlayed);
    }
    bits.ue()?; // the largest frame number
    match bits.ue()? {
        0 => {
            bits.ue()?;
        }
        1 => {
            bits.u(1)?;
            bits.se()?;
            bits.se()?;
            let cycle = bits.ue()?;
            if cycle > 255 {
                return Err(CantPlay::Damaged);
            }
            for _ in 0..cycle {
                bits.se()?;
            }
        }
        2 => {}
        _ => return Err(CantPlay::Damaged),
    }
    bits.ue()?; // reference frames
    bits.u(1)?;
    let width_blocks = u64::from(bits.ue()?) + 1;
    let height_units = u64::from(bits.ue()?) + 1;
    if bits.u(1)? == 0 {
        // Interlaced: fields, which the decoder doesn't play.
        return Err(CantPlay::KindNotPlayed);
    }
    bits.u(1)?;
    let (mut left, mut right, mut top, mut bottom) = (0, 0, 0, 0);
    if bits.u(1)? == 1 {
        left = u64::from(bits.ue()?);
        right = u64::from(bits.ue()?);
        top = u64::from(bits.ue()?);
        bottom = u64::from(bits.ue()?);
    }
    // The decoder works on the pictures as stored, so it's their size that's limited, and a crop
    // is only ever what rounds a picture up to whole blocks.
    let (stored_width, stored_height) = (width_blocks * 16, height_units * 16);
    if stored_width.max(stored_height) > MOST_WIDE || stored_width.min(stored_height) > MOST_HIGH {
        return Err(CantPlay::TooLarge);
    }
    if 2 * (left + right) >= 16 || 2 * (top + bottom) >= 16 {
        return Err(CantPlay::Damaged);
    }
    let width = stored_width - 2 * (left + right);
    let height = stored_height - 2 * (top + bottom);
    let (mut matrix, mut range) = (Matrix::Standard(YuvStandardMatrix::Bt601), YuvRange::Limited);
    // What follows is optional, and a stream cut short in it plays with the usual colours.
    if bits.u(1).unwrap_or(0) == 1 {
        let _ = colours(&mut bits, &mut matrix, &mut range);
    }
    Ok(Sps { width: width as u32, height: height as u32, matrix, range })
}

/// Reads the colours a stream says its pictures are in, from its usability information.
fn colours(bits: &mut Bits, matrix: &mut Matrix, range: &mut YuvRange) -> Result<(), CantPlay> {
    if bits.u(1)? == 1 && bits.u(8)? == 255 {
        bits.u(32)?; // the shape of a pixel
    }
    if bits.u(1)? == 1 {
        bits.u(1)?;
    }
    if bits.u(1)? == 1 {
        bits.u(3)?;
        if bits.u(1)? == 1 {
            *range = YuvRange::Full;
        }
        if bits.u(1)? == 1 {
            bits.u(16)?; // primaries and transfer
            *matrix = matrix_of(bits.u(8)?);
        }
    }
    Ok(())
}

/// Whether a unit's body is free of anything that would read as the start of another, so the
/// decoder sees the same units as were checked here. A well made stream never has one.
pub(crate) fn safe_unit(unit: &[u8]) -> bool {
    !unit.windows(3).any(|w| w[0] == 0 && w[1] == 0 && w[2] <= 2)
}

/// The bytes of a unit without those put in so its contents never look like the start of one.
fn unescape(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut zeros = 0;
    for &byte in bytes {
        if zeros >= 2 && byte == 3 {
            zeros = 0;
            continue;
        }
        zeros = if byte == 0 { zeros + 1 } else { 0 };
        out.push(byte);
    }
    out
}

struct Bits<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn u(&mut self, count: u32) -> Result<u32, CantPlay> {
        let mut value = 0u64;
        for _ in 0..count {
            let byte = *self.bytes.get(self.at / 8).ok_or(CantPlay::Damaged)?;
            value = value << 1 | u64::from(byte >> (7 - self.at % 8) & 1);
            self.at += 1;
        }
        Ok(value as u32)
    }

    fn ue(&mut self) -> Result<u32, CantPlay> {
        let mut zeros = 0;
        while self.u(1)? == 0 {
            zeros += 1;
            if zeros > 31 {
                return Err(CantPlay::Damaged);
            }
        }
        let rest = self.u(zeros)?;
        Ok(((1u64 << zeros) - 1 + u64::from(rest)).min(u64::from(u32::MAX)) as u32)
    }

    fn se(&mut self) -> Result<i64, CantPlay> {
        let coded = i64::from(self.ue()?);
        Ok(if coded % 2 == 1 { (coded + 1) / 2 } else { -coded / 2 })
    }

    fn skip_scaling_list(&mut self, size: usize) -> Result<(), CantPlay> {
        let (mut last, mut next) = (8i64, 8i64);
        for _ in 0..size {
            if next != 0 {
                next = (last + self.se()? + 256).rem_euclid(256);
            }
            if next != 0 {
                last = next;
            }
        }
        Ok(())
    }
}
