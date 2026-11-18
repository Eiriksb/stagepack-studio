//! A video's width and height, read from the headers of its index alone: no table of where its
//! pictures lie is read, and no more than [`MOST_READ`] bytes of the file, so this is cheap
//! enough for whatever lays a slide out.

use std::io::SeekFrom;

use crate::{CantPlay, ReadSeek};

/// The most bytes read to find a video's size.
pub const MOST_READ: u64 = 64 * 1024;

/// The largest side believed.
const MOST_SIDE: u32 = 16384;

/// Reads from a file, counting what it reads, and refusing to read more than [`MOST_READ`].
struct Counted<'a> {
    reader: &'a mut dyn ReadSeek,
    len: u64,
    read: u64,
}

impl Counted<'_> {
    fn read_at(&mut self, at: u64, buf: &mut [u8]) -> Result<(), CantPlay> {
        self.read += buf.len() as u64;
        if self.read > MOST_READ {
            return Err(CantPlay::Damaged);
        }
        self.reader.seek(SeekFrom::Start(at)).map_err(|_| CantPlay::CouldNotRead)?;
        self.reader.read_exact(buf).map_err(|_| CantPlay::CouldNotRead)
    }

    /// The box at `at`, which must end by `end`: its kind, where its body starts and where it
    /// ends. `None` if no box fits there.
    fn head(&mut self, at: u64, end: u64) -> Result<Option<([u8; 4], u64, u64)>, CantPlay> {
        if end.saturating_sub(at) < 8 {
            return Ok(None);
        }
        let mut bytes = [0u8; 8];
        self.read_at(at, &mut bytes)?;
        let kind: [u8; 4] = bytes[4..].try_into().unwrap_or_default();
        let (header, size) = match u32::from_be_bytes(bytes[..4].try_into().unwrap_or_default()) {
            0 => (8, end - at),
            1 => {
                if end - at < 16 {
                    return Ok(None);
                }
                let mut large = [0u8; 8];
                self.read_at(at + 8, &mut large)?;
                (16, u64::from_be_bytes(large))
            }
            size => (8, u64::from(size)),
        };
        if size < header || size > end - at {
            return Ok(None);
        }
        Ok(Some((kind, at + header, at + size)))
    }

    /// The first box of `kind` among the boxes from `at` to `end`.
    fn find(&mut self, kind: &[u8; 4], mut at: u64, end: u64) -> Result<Option<(u64, u64)>, CantPlay> {
        while let Some((found, body, next)) = self.head(at, end)? {
            if &found == kind {
                return Ok(Some((body, next)));
            }
            at = next;
        }
        Ok(None)
    }
}

/// The width and height of the first video track of the MP4 `reader` reads, from its sample
/// description, reading no table of its pictures and no more than [`MOST_READ`] bytes.
pub fn video_size(reader: &mut dyn ReadSeek) -> Result<(u32, u32), CantPlay> {
    let len = reader.seek(SeekFrom::End(0)).map_err(|_| CantPlay::CouldNotRead)?;
    let mut file = Counted { reader, len, read: 0 };
    let end = file.len;
    let (moov, moov_end) = file.find(b"moov", 0, end)?.ok_or(CantPlay::NotAVideo)?;
    let mut at = moov;
    while let Some((kind, body, next)) = file.head(at, moov_end)? {
        if &kind == b"trak"
            && let Some(size) = track_size(&mut file, body, next)?
        {
            return Ok(size);
        }
        at = next;
    }
    Err(CantPlay::NoPicture)
}

/// The width and height of the track whose body runs from `at` to `end`, if it's a video's.
fn track_size(file: &mut Counted, at: u64, end: u64) -> Result<Option<(u32, u32)>, CantPlay> {
    let Some((mdia, mdia_end)) = file.find(b"mdia", at, end)? else { return Ok(None) };
    let Some((hdlr, hdlr_end)) = file.find(b"hdlr", mdia, mdia_end)? else { return Ok(None) };
    // A full box's version and flags, a word set aside, then the kind of track.
    if hdlr_end - hdlr < 12 {
        return Ok(None);
    }
    let mut handler = [0u8; 4];
    file.read_at(hdlr + 8, &mut handler)?;
    if &handler != b"vide" {
        return Ok(None);
    }
    let Some((minf, minf_end)) = file.find(b"minf", mdia, mdia_end)? else { return Ok(None) };
    let Some((stbl, stbl_end)) = file.find(b"stbl", minf, minf_end)? else { return Ok(None) };
    let Some((stsd, stsd_end)) = file.find(b"stsd", stbl, stbl_end)? else { return Ok(None) };
    // The description's version, flags and count, then its first entry: a visual one has its
    // width and height 24 bytes into what follows its own header.
    let entry = stsd + 8;
    if stsd_end.saturating_sub(entry) < 36 {
        return Err(CantPlay::Damaged);
    }
    let mut sides = [0u8; 4];
    file.read_at(entry + 32, &mut sides)?;
    let width = u32::from(u16::from_be_bytes([sides[0], sides[1]]));
    let height = u32::from(u16::from_be_bytes([sides[2], sides[3]]));
    if width == 0 || height == 0 || width > MOST_SIDE || height > MOST_SIDE {
        return Err(CantPlay::Damaged);
    }
    Ok(Some((width, height)))
}
