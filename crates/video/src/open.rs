//! Opening a video: reading its index, and nothing more, and saying what it is.
//!
//! The container library trusts the sizes a file gives, so it's never shown the file. The index
//! is read here, every box in it checked to lie inside the one holding it, and only the boxes
//! needed are rebuilt into a small, well made index for the library to read. The table of where
//! each picture lies is then worked out here, each one checked against the file's real length.

use std::io::{Cursor, SeekFrom};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use mp4::{Mp4Reader, Mp4Track};
use yuvutils_rs::YuvRange;

use crate::sps::{self, Matrix};
use crate::{CantPlay, ReadSeek, VideoInfo};

const MOST_FRAMES_A_SECOND: f64 = 120.0;
const MOST_SECONDS: f64 = 6.0 * 3600.0;
/// Six hours at 120 frames a second.
const MOST_FRAMES: u32 = 6 * 3600 * 120;
/// The largest index read. Six hours at 120 frames a second needs about this.
const MOST_INDEX: u64 = 40 * 1024 * 1024;
/// The largest single picture read: far more than one 4096 by 2304 picture needs.
const MOST_SAMPLE: u32 = 16 * 1024 * 1024;
/// The most boxes looked at in the top of a file, so one of nothing but tiny boxes can't keep
/// the opening reading.
const MOST_TOP_BOXES: u32 = 4096;

/// Where one picture lies in the file, and when it's shown.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sample {
    pub offset: u64,
    pub size: u32,
    pub sync: bool,
    /// When it's shown, in the file's own ticks.
    pub shown: i64,
}

/// What opening a video found: what it is, and where its pictures lie.
pub(crate) struct Index {
    pub info: VideoInfo,
    pub width: u32,
    pub height: u32,
    /// In the order they're stored, which is the order they're decoded in.
    pub samples: Vec<Sample>,
    /// When each picture is shown, in the order shown, from 0, in ticks.
    pub times: Vec<i64>,
    pub ticks_a_second: u32,
    /// The whole video's length, in ticks.
    pub length: i64,
    /// Every sequence and picture parameter set the index gives, each after a start code.
    pub headers: Vec<u8>,
    /// How many bytes give the length of each unit in a picture.
    pub length_bytes: usize,
    pub matrix: Matrix,
    pub range: YuvRange,
}

impl Index {
    pub fn seconds(&self, ticks: i64) -> f64 {
        ticks as f64 / f64::from(self.ticks_a_second)
    }

    /// When the first picture shown is, of those decoding can give starting at `sample`: the
    /// earliest of those up to the next picture decoding can start from.
    pub fn first_shown(&self, sample: usize) -> i64 {
        let rest = &self.samples[sample..];
        let group = 1 + rest[1..].iter().position(|s| s.sync).unwrap_or(rest.len() - 1);
        rest[..group].iter().map(|s| s.shown).min().unwrap_or(0)
    }

    /// Where in the shown order the picture shown at `ticks` is.
    pub fn rank(&self, ticks: i64) -> usize {
        self.times.partition_point(|t| *t < ticks)
    }
}

/// An opened video, read from where it lies.
pub struct Video {
    pub(crate) reader: Box<dyn ReadSeek>,
    pub(crate) index: Arc<Index>,
}

impl Video {
    pub fn info(&self) -> VideoInfo {
        self.index.info
    }
}

/// Opens a video, reading its index only. Says why if it isn't one that can be played.
pub fn open(mut reader: Box<dyn ReadSeek>) -> Result<Video, CantPlay> {
    let len = reader.seek(SeekFrom::End(0)).map_err(|_| CantPlay::CouldNotRead)?;
    let moov = read_moov(&mut *reader, len)?;
    let index = catch_unwind(AssertUnwindSafe(|| describe(&moov, len))).unwrap_or(Err(CantPlay::Damaged))?;
    Ok(Video { reader, index: Arc::new(index) })
}

/// Where a track's pictures are kept, said as the usual way of saying "in this file".
const DINF: [u8; 36] = [
    0, 0, 0, 36, b'd', b'i', b'n', b'f', 0, 0, 0, 28, b'd', b'r', b'e', b'f', 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 12,
    b'u', b'r', b'l', b' ', 0, 0, 0, 1,
];

/// One box's header: what it is, how long its header is, and how long it is in all.
struct Head {
    kind: [u8; 4],
    header: u64,
    size: u64,
}

/// Reads the header of the box at `at`, or `None` if there isn't room for one.
fn read_head(reader: &mut dyn ReadSeek, at: u64, len: u64) -> Result<Option<Head>, CantPlay> {
    if len - at < 8 {
        return Ok(None);
    }
    let mut bytes = [0u8; 16];
    reader.seek(SeekFrom::Start(at)).map_err(|_| CantPlay::CouldNotRead)?;
    reader.read_exact(&mut bytes[..8]).map_err(|_| CantPlay::CouldNotRead)?;
    let kind = bytes[4..8].try_into().unwrap_or_default();
    let (header, size) = match u32::from_be_bytes(bytes[..4].try_into().unwrap_or_default()) {
        0 => (8, len - at),
        1 => {
            if len - at < 16 {
                return Ok(None);
            }
            reader.read_exact(&mut bytes[8..]).map_err(|_| CantPlay::CouldNotRead)?;
            (16, u64::from_be_bytes(bytes[8..].try_into().unwrap_or_default()))
        }
        size => (8, u64::from(size)),
    };
    Ok(Some(Head { kind, header, size }))
}

/// Walks the top of the file to find its index, and reads it: the body of its `moov` box.
fn read_moov(reader: &mut dyn ReadSeek, len: u64) -> Result<Vec<u8>, CantPlay> {
    let mut at = 0;
    let mut seen = 0;
    let mut moov = None;
    while at < len {
        let head = read_head(reader, at, len)?;
        let fits = head.as_ref().is_some_and(|head| head.size >= head.header && head.size <= len - at);
        if seen == 0 && !(fits && head.as_ref().is_some_and(|head| &head.kind == b"ftyp" && head.size <= 4096)) {
            return Err(CantPlay::NotAVideo);
        }
        seen += 1;
        if seen > MOST_TOP_BOXES {
            return Err(CantPlay::Damaged);
        }
        let Some(head) = head.filter(|_| fits) else {
            // A box that runs past the end: a file cut short. What came before may still play.
            break;
        };
        match &head.kind {
            b"moov" if moov.is_some() => return Err(CantPlay::Damaged),
            b"moov" => {
                let body = head.size - head.header;
                if body > MOST_INDEX {
                    return Err(CantPlay::TooLarge);
                }
                let mut bytes = vec![0; body as usize];
                reader.read_exact(&mut bytes).map_err(|_| CantPlay::CouldNotRead)?;
                moov = Some(bytes);
            }
            b"moof" => return Err(CantPlay::KindNotPlayed),
            _ => {}
        }
        at += head.size.max(8);
    }
    if seen == 0 {
        return Err(CantPlay::NotAVideo);
    }
    moov.ok_or(CantPlay::Damaged)
}

/// A box's kind and its body.
type Child<'a> = ([u8; 4], &'a [u8]);

/// The boxes in `body`, each checked to lie inside it, as their kind and their own body.
fn children(body: &[u8]) -> Result<Vec<Child<'_>>, CantPlay> {
    let mut boxes = Vec::new();
    let mut at = 0;
    // A few bytes left over at the end, as some programs write, aren't a box.
    while body.len() - at >= 8 {
        let rest = &body[at..];
        let kind = rest[4..8].try_into().unwrap_or_default();
        let (header, size) = match u32::from_be_bytes(rest[..4].try_into().unwrap_or_default()) {
            0 => (8, rest.len()),
            1 if rest.len() >= 16 => {
                let size = u64::from_be_bytes(rest[8..16].try_into().unwrap_or_default());
                (16, usize::try_from(size).map_err(|_| CantPlay::Damaged)?)
            }
            1 => return Err(CantPlay::Damaged),
            size => (8, size as usize),
        };
        if size < header || size > rest.len() {
            return Err(CantPlay::Damaged);
        }
        boxes.push((kind, &rest[header..size]));
        at += size;
    }
    Ok(boxes)
}

fn child<'a>(boxes: &[Child<'a>], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes.iter().find(|(k, _)| k == kind).map(|(_, body)| *body)
}

/// Writes a box of `kind` holding `body`.
fn put(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) -> Result<(), CantPlay> {
    let size = u32::try_from(body.len() + 8).map_err(|_| CantPlay::TooLarge)?;
    out.extend_from_slice(&size.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    Ok(())
}

/// Rebuilds the index with only the first video track, and only the boxes needed of it, each
/// checked to lie inside the one holding it. Says which kind of failure it found, if any.
fn rebuild(moov: &[u8]) -> Result<(Vec<u8>, Option<Colr>), CantPlay> {
    let boxes = children(moov)?;
    if child(&boxes, b"mvex").is_some() {
        // Made in pieces, as for streaming: not what a video added to a talk is.
        return Err(CantPlay::KindNotPlayed);
    }
    let mvhd = child(&boxes, b"mvhd").ok_or(CantPlay::Damaged)?;
    let mut trak = None;
    let mut damaged = None;
    for (kind, body) in &boxes {
        if kind == b"trak" && trak.is_none() {
            match video_trak(body) {
                Ok(found) => trak = found,
                Err(why) => damaged = damaged.or(Some(why)),
            }
        }
    }
    let (trak, colr) = trak.ok_or(damaged.unwrap_or(CantPlay::NoPicture))?;
    let mut out = Vec::with_capacity(moov.len().min(MOST_INDEX as usize) + 256);
    // The library wants a file type first.
    put(&mut out, b"ftyp", b"isom\0\0\0\0")?;
    let mut body = Vec::with_capacity(trak.len() + mvhd.len() + 16);
    put(&mut body, b"mvhd", mvhd)?;
    put(&mut body, b"trak", &trak)?;
    put(&mut out, b"moov", &body)?;
    Ok((out, colr))
}

/// A video track rebuilt, and what it says of its colours.
type Track = (Vec<u8>, Option<Colr>);

/// The track in `trak` rebuilt, if it's a video track, and what it says of its colours.
fn video_trak(trak: &[u8]) -> Result<Option<Track>, CantPlay> {
    let boxes = children(trak)?;
    let mdia = children(child(&boxes, b"mdia").ok_or(CantPlay::Damaged)?)?;
    let hdlr = child(&mdia, b"hdlr").ok_or(CantPlay::Damaged)?;
    if hdlr.get(8..12) != Some(b"vide") {
        return Ok(None);
    }
    let tkhd = child(&boxes, b"tkhd").ok_or(CantPlay::Damaged)?;
    let mdhd = child(&mdia, b"mdhd").ok_or(CantPlay::Damaged)?;
    let minf = children(child(&mdia, b"minf").ok_or(CantPlay::Damaged)?)?;
    let stbl = children(child(&minf, b"stbl").ok_or(CantPlay::Damaged)?)?;

    let mut new_stbl = Vec::new();
    let (stsd, colr) = sample_description(child(&stbl, b"stsd").ok_or(CantPlay::Damaged)?)?;
    put(&mut new_stbl, b"stsd", &stsd)?;
    for kind in [b"stts", b"ctts", b"stss", b"stsc", b"stsz", b"stco", b"co64"] {
        if let Some(body) = child(&stbl, kind) {
            put(&mut new_stbl, kind, body)?;
        }
    }
    let mut new_minf = Vec::new();
    // Where the pictures are kept: always in this file, so the file's own words aren't needed.
    new_minf.extend_from_slice(&DINF);
    put(&mut new_minf, b"stbl", &new_stbl)?;
    let mut new_mdia = Vec::new();
    put(&mut new_mdia, b"mdhd", mdhd)?;
    // The handler without its name.
    put(&mut new_mdia, b"hdlr", &[&hdlr[..12], &[0; 13]].concat())?;
    put(&mut new_mdia, b"minf", &new_minf)?;
    let mut new_trak = Vec::new();
    put(&mut new_trak, b"tkhd", tkhd)?;
    put(&mut new_trak, b"mdia", &new_mdia)?;
    Ok(Some((new_trak, colr)))
}

/// What a `colr` box says of a picture's colours: the matrix, and the range if it says one.
#[derive(Clone, Copy, Debug)]
struct Colr {
    matrix: u16,
    full: Option<bool>,
}

fn read_colr(body: &[u8]) -> Option<Colr> {
    let matrix = u16::from_be_bytes(body.get(8..10)?.try_into().ok()?);
    match body.get(..4)? {
        b"nclx" => Some(Colr { matrix, full: Some(body.get(10)? & 0x80 != 0) }),
        b"nclc" => Some(Colr { matrix, full: None }),
        _ => None,
    }
}

/// The description of the track's pictures rebuilt with only its first entry, which must be
/// H.264 in the usual form, and only its configuration; and what the entry says of its colours.
fn sample_description(stsd: &[u8]) -> Result<(Vec<u8>, Option<Colr>), CantPlay> {
    let entries = children(stsd.get(8..).ok_or(CantPlay::Damaged)?)?;
    let (kind, avc1) = entries.first().ok_or(CantPlay::Damaged)?;
    if kind != b"avc1" {
        return Err(CantPlay::KindNotPlayed);
    }
    let fixed = avc1.get(..78).ok_or(CantPlay::Damaged)?;
    let inside = children(&avc1[78..])?;
    let avcc = child(&inside, b"avcC").ok_or(CantPlay::Damaged)?;
    let colr = child(&inside, b"colr").and_then(read_colr);
    let mut entry = fixed.to_vec();
    put(&mut entry, b"avcC", avcc)?;
    let mut out = vec![0, 0, 0, 0, 0, 0, 0, 1];
    put(&mut out, b"avc1", &entry)?;
    Ok((out, colr))
}

/// Says what the video with this index is, and where its pictures lie in a file of `len` bytes.
fn describe(moov: &[u8], len: u64) -> Result<Index, CantPlay> {
    let (rebuilt, colr) = rebuild(moov)?;
    let size = rebuilt.len() as u64;
    let parsed = Mp4Reader::read_header(Cursor::new(rebuilt), size).map_err(|_| CantPlay::Damaged)?;
    let track = parsed.tracks().values().next().ok_or(CantPlay::NoPicture)?;
    let mdia = &track.trak.mdia;
    let stbl = &mdia.minf.stbl;
    let avcc = &stbl.stsd.avc1.as_ref().ok_or(CantPlay::KindNotPlayed)?.avcc;

    // Every parameter set is checked, as any of them may be the one the pictures name. The
    // pictures are said to be the size and colours of the first.
    let sps = sps::read(&avcc.sequence_parameter_sets.first().ok_or(CantPlay::Damaged)?.bytes)?;
    for other in &avcc.sequence_parameter_sets[1..] {
        sps::read(&other.bytes)?;
    }
    if avcc.picture_parameter_sets.is_empty() {
        return Err(CantPlay::Damaged);
    }
    let mut headers = Vec::new();
    for nal in avcc.sequence_parameter_sets.iter().chain(&avcc.picture_parameter_sets) {
        if !sps::safe_unit(&nal.bytes) {
            return Err(CantPlay::Damaged);
        }
        headers.extend_from_slice(&[0, 0, 0, 1]);
        headers.extend_from_slice(&nal.bytes);
    }
    // The sample entry's own words on colours come before the stream's.
    let (mut matrix, mut range) = (sps.matrix, sps.range);
    if let Some(colr) = colr {
        matrix = sps::matrix_of(u32::from(colr.matrix));
        if let Some(full) = colr.full {
            range = if full { YuvRange::Full } else { YuvRange::Limited };
        }
    }
    let length_bytes = match avcc.length_size_minus_one {
        0 => 1,
        1 => 2,
        3 => 4,
        _ => return Err(CantPlay::Damaged),
    };

    let count = stbl.stsz.sample_count;
    if count == 0 {
        return Err(CantPlay::NoPicture);
    }
    if count > MOST_FRAMES {
        return Err(CantPlay::TooLarge);
    }
    let ticks_a_second = mdia.mdhd.timescale;
    if ticks_a_second == 0 {
        return Err(CantPlay::Damaged);
    }
    let count = count as usize;
    let mut samples = Vec::with_capacity(count);
    place_samples(track, count, len, &mut samples)?;
    check_apart(&samples)?;
    let (mut length, last_start) = time_samples(track, &mut samples)?;
    if let Some(stss) = &stbl.stss {
        for sample in &mut samples {
            sample.sync = false;
        }
        for id in &stss.entries {
            if let Some(sample) = (*id as usize).checked_sub(1).and_then(|i| samples.get_mut(i)) {
                sample.sync = true;
            }
        }
        samples[0].sync = true;
    }
    let mut times = Vec::with_capacity(count);
    times.extend(samples.iter().map(|s| s.shown));
    times.sort_unstable();
    let first = times[0];
    for t in &mut times {
        *t -= first;
    }
    for sample in &mut samples {
        sample.shown -= first;
    }

    // Some programs give the last picture no time of its own, so it's shown at the very end: it's
    // given an average frame's time, the header's length where that says no more than one more
    // frame. A picture shown later than that is a damaged table: it would never be due, and a
    // looped video would wait for it.
    let last = times[count - 1];
    if last >= length {
        let frame = if count > 1 { last_start / (count as i64 - 1) } else { i64::try_from(mdia.mdhd.duration).unwrap_or(0) };
        if frame <= 0 || last - length > frame {
            return Err(CantPlay::Damaged);
        }
        let said = i64::try_from(mdia.mdhd.duration).unwrap_or(0);
        length = if said > last && said <= last + frame { said } else { last + frame };
    }
    let seconds = length as f64 / f64::from(ticks_a_second);
    if seconds <= 0.0 {
        return Err(CantPlay::Damaged);
    }
    let frames_a_second = count as f64 / seconds;
    if seconds > MOST_SECONDS || frames_a_second > MOST_FRAMES_A_SECOND {
        return Err(CantPlay::TooLarge);
    }
    Ok(Index {
        info: VideoInfo {
            width: sps.width,
            height: sps.height,
            seconds,
            frames: count as u64,
            frames_a_second: frames_a_second as f32,
        },
        width: sps.width,
        height: sps.height,
        samples,
        times,
        ticks_a_second,
        length,
        headers,
        length_bytes,
        matrix,
        range,
    })
}

/// Works out where each of the `count` pictures lies, from the tables of chunks and sizes, and
/// checks each lies inside the file.
fn place_samples(track: &Mp4Track, count: usize, len: u64, samples: &mut Vec<Sample>) -> Result<(), CantPlay> {
    let stbl = &track.trak.mdia.minf.stbl;
    let sizes = &stbl.stsz;
    if sizes.sample_size == 0 && sizes.sample_sizes.len() < count {
        return Err(CantPlay::Damaged);
    }
    let chunk_count = match (&stbl.stco, &stbl.co64) {
        (Some(stco), _) => stco.entries.len(),
        (None, Some(co64)) => co64.entries.len(),
        (None, None) => return Err(CantPlay::Damaged),
    };
    let chunk_offset = |chunk: usize| match (&stbl.stco, &stbl.co64) {
        (Some(stco), _) => u64::from(stco.entries[chunk]),
        (None, Some(co64)) => co64.entries[chunk],
        (None, None) => 0,
    };
    let runs = &stbl.stsc.entries;
    if runs.first().is_none_or(|run| run.first_chunk != 1) {
        return Err(CantPlay::Damaged);
    }
    let mut run = 0;
    for chunk in 0..chunk_count {
        // Chunk numbers count from 1.
        while runs.get(run + 1).is_some_and(|next| next.first_chunk as usize <= chunk + 1) {
            if runs[run + 1].first_chunk <= runs[run].first_chunk {
                return Err(CantPlay::Damaged);
            }
            run += 1;
        }
        let in_chunk = runs[run].samples_per_chunk;
        if in_chunk == 0 {
            return Err(CantPlay::Damaged);
        }
        let mut offset = chunk_offset(chunk);
        for _ in 0..in_chunk {
            if samples.len() == count {
                return Ok(());
            }
            let size = if sizes.sample_size > 0 { sizes.sample_size } else { sizes.sample_sizes[samples.len()] };
            let end = offset.checked_add(u64::from(size)).ok_or(CantPlay::Damaged)?;
            if size > MOST_SAMPLE || end > len {
                return Err(CantPlay::Damaged);
            }
            samples.push(Sample { offset, size, sync: true, shown: 0 });
            offset = end;
        }
    }
    if samples.len() < count {
        return Err(CantPlay::Damaged);
    }
    Ok(())
}

/// Checks no two pictures share any bytes, so every byte of the file is read at most once a
/// pass however the tables are made.
fn check_apart(samples: &[Sample]) -> Result<(), CantPlay> {
    let mut order: Vec<u32> = (0..samples.len() as u32).filter(|i| samples[*i as usize].size > 0).collect();
    order.sort_unstable_by_key(|i| samples[*i as usize].offset);
    let mut end = 0;
    for i in order {
        let sample = samples[i as usize];
        if sample.offset < end {
            return Err(CantPlay::Damaged);
        }
        end = sample.offset + u64::from(sample.size);
    }
    Ok(())
}

/// Works out when each picture is shown, from the tables of durations and offsets. Gives the
/// whole video's length in ticks, and when the last picture starts before its offset.
fn time_samples(track: &Mp4Track, samples: &mut [Sample]) -> Result<(i64, i64), CantPlay> {
    let stbl = &track.trak.mdia.minf.stbl;
    let mut decoded: i64 = 0;
    let mut last_start = 0;
    let mut i = 0;
    'runs: for run in &stbl.stts.entries {
        for _ in 0..run.sample_count {
            let Some(sample) = samples.get_mut(i) else { break 'runs };
            sample.shown = decoded;
            last_start = decoded;
            decoded = decoded.checked_add(i64::from(run.sample_delta)).ok_or(CantPlay::Damaged)?;
            i += 1;
        }
    }
    if i < samples.len() {
        return Err(CantPlay::Damaged);
    }
    if let Some(ctts) = &stbl.ctts {
        let mut i = 0;
        'offsets: for run in &ctts.entries {
            for _ in 0..run.sample_count {
                let Some(sample) = samples.get_mut(i) else { break 'offsets };
                // The offset is signed in the newer form of the table and unsigned in the older,
                // where a large one is as damaged as a negative.
                sample.shown += i64::from(run.sample_offset);
                i += 1;
            }
        }
    }
    Ok((decoded, last_start))
}
