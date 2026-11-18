mod support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use proptest::prelude::*;
use slides_video::*;
use support::*;

/// Counts the largest single allocation any test here makes, so a test can tell whether a size
/// claimed by a file was ever allocated, and the most in use at once. Only what Rust allocates is
/// seen: the decoder's own allocations, made in C, aren't.
struct Counting;

static LARGEST: AtomicUsize = AtomicUsize::new(0);
static IN_USE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn allocated(size: usize) {
    LARGEST.fetch_max(size, Ordering::Relaxed);
    let now = IN_USE.fetch_add(size, Ordering::Relaxed) + size;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        allocated(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        allocated(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        IN_USE.fetch_sub(layout.size(), Ordering::Relaxed);
        allocated(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        IN_USE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

const MOST_ALLOCATED: usize = 64 * 1024 * 1024;
/// The most in use at once while a case runs. The other tests here run beside it and count too,
/// so this is the whole test program's peak, a little more than one case's.
const MOST_IN_USE: usize = 192 * 1024 * 1024;
const MOST_TIME: Duration = Duration::from_secs(2);

/// A small made video for damaging: 12 frames of 48 by 32.
fn small() -> &'static [u8] {
    static SMALL: OnceLock<Vec<u8>> = OnceLock::new();
    SMALL.get_or_init(|| coloured_video(48, 32, 12, 30))
}

/// The same, with its index at the start, so damage can reach the pictures with the index whole.
fn small_index_first() -> &'static [u8] {
    static SMALL: OnceLock<Vec<u8>> = OnceLock::new();
    SMALL.get_or_init(|| index_first(small()))
}

/// A made video of one frame, about a kilobyte.
fn tiny() -> &'static [u8] {
    static TINY: OnceLock<Vec<u8>> = OnceLock::new();
    TINY.get_or_init(|| coloured_video(16, 16, 1, 30))
}

fn check_memory() {
    assert!(LARGEST.load(Ordering::Relaxed) <= MOST_ALLOCATED, "allocated {} at once", LARGEST.load(Ordering::Relaxed));
    assert!(PEAK.load(Ordering::Relaxed) <= MOST_IN_USE, "{} in use at once", PEAK.load(Ordering::Relaxed));
}

/// Opens `bytes`, takes its first frame and plays it through, each within two seconds, and
/// allocating no more than 64 MB at once. What `open` said.
fn open_and_play(bytes: Vec<u8>) -> Result<VideoInfo, CantPlay> {
    open_and_play_looped(bytes, false)
}

/// The same, playing round twice if `looped`.
fn open_and_play_looped(bytes: Vec<u8>, looped: bool) -> Result<VideoInfo, CantPlay> {
    PEAK.store(IN_USE.load(Ordering::Relaxed), Ordering::Relaxed);
    let started = Instant::now();
    let opened = open(reader(bytes));
    assert!(started.elapsed() < MOST_TIME, "opening took {:?}", started.elapsed());
    let mut video = match opened {
        Ok(video) => video,
        Err(why) => {
            check_memory();
            return Err(why);
        }
    };
    let info = video.info();
    let started = Instant::now();
    let _ = video.first_frame();
    assert!(started.elapsed() < MOST_TIME, "the first frame took {:?}", started.elapsed());
    let started = Instant::now();
    let mut playing = video.play(0.0, looped);
    let mut last = f64::NEG_INFINITY;
    let mut frames = 0;
    while !playing.ended() && playing.failed().is_none() && frames < 2 * info.frames {
        if let Some(frame) = playing.frame_for(f64::INFINITY) {
            if !looped {
                assert!(frame.at >= last, "times went back: {} after {last}", frame.at);
            }
            last = frame.at;
            frames += 1;
        }
        assert!(started.elapsed() < MOST_TIME, "playing took {:?}", started.elapsed());
        std::thread::sleep(Duration::from_millis(1));
    }
    drop(playing);
    check_memory();
    Ok(info)
}

#[test]
fn a_made_video_says_what_it_is() {
    let video = open(reader(coloured_video(64, 48, 90, 30))).unwrap();
    let info = video.info();
    assert_eq!((info.width, info.height, info.frames), (64, 48, 90));
    assert!((info.frames_a_second - 30.0).abs() < 0.001, "{}", info.frames_a_second);
    assert!((info.seconds - 3.0).abs() < 0.001, "{}", info.seconds);

    let other = open(reader(coloured_video(96, 64, 25, 25))).unwrap().info();
    assert_eq!((other.width, other.height, other.frames), (96, 64, 25));
    assert!((other.frames_a_second - 25.0).abs() < 0.001);
    assert!((other.seconds - 1.0).abs() < 0.001);
}

#[derive(Debug, Clone)]
enum Damage {
    /// Anything at all after the file's type, which every made video starts with.
    Random(Vec<u8>),
    Byte(usize, u8),
    Zeroed(usize, usize),
    Cut(usize),
}

fn damage() -> impl Strategy<Value = Damage> {
    prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..2048).prop_map(Damage::Random),
        (any::<usize>(), any::<u8>()).prop_map(|(at, value)| Damage::Byte(at, value)),
        (any::<usize>(), 1usize..256).prop_map(|(at, len)| Damage::Zeroed(at, len)),
        any::<usize>().prop_map(Damage::Cut),
    ]
}

fn damaged(damage: &Damage, index_first: bool) -> Vec<u8> {
    let mut video = if index_first { small_index_first() } else { small() }.to_vec();
    match damage {
        Damage::Random(bytes) => {
            let ftyp = write(&parse(small())[..1]);
            return [ftyp, bytes.clone()].concat();
        }
        Damage::Byte(at, value) => {
            let at = at % video.len();
            video[at] = *value;
        }
        Damage::Zeroed(at, len) => {
            let at = at % video.len();
            let end = (at + len).min(video.len());
            video[at..end].fill(0);
        }
        Damage::Cut(at) => video.truncate(at % video.len()),
    }
    video
}

proptest! {
    #[test]
    fn any_bytes_open_or_say_why(damage in damage(), index_first in any::<bool>(), looped in any::<bool>()) {
        let _ = open_and_play_looped(damaged(&damage, index_first), looped);
    }
}

#[test]
fn any_bytes_open_or_say_why_named_cases() {
    assert_eq!(open_and_play(Vec::new()), Err(CantPlay::NotAVideo));
    assert_eq!(open_and_play(b"This is a note, named as a video by mistake.\n".repeat(40)), Err(CantPlay::NotAVideo));

    let made = small();
    let made_info = open_and_play(made.to_vec()).unwrap();

    // No video track: the only track says it is sound.
    let no_video = edit(made, |boxes| find(boxes, "moov/trak/mdia/hdlr").data[8..12].copy_from_slice(b"soun"));
    assert_eq!(open_and_play(no_video), Err(CantPlay::NoPicture));
    let no_tracks = edit(made, |boxes| find(boxes, "moov").inside.retain(|b| &b.kind != b"trak"));
    assert_eq!(open_and_play(no_tracks), Err(CantPlay::NoPicture));

    // Two video tracks: the first plays.
    let two = edit(made, |boxes| {
        let moov = find(boxes, "moov");
        let mut second = moov.inside.iter().find(|b| &b.kind == b"trak").unwrap().clone();
        set_u32(&mut find(&mut second.inside, "tkhd").data, 12, 2);
        moov.inside.push(second);
    });
    assert_eq!(open_and_play(two), Ok(made_info));

    // Sizes claiming 4 GB, in a file of about a kilobyte.
    let tiny = tiny();
    assert!(tiny.len() < 1500, "the tiny video is {} bytes", tiny.len());
    let mut whole_box = tiny.to_vec();
    let mdat = top_kinds(tiny).iter().position(|k| k == "mdat").unwrap();
    let mdat_at: usize = parse(tiny)[..mdat].iter().map(|b| write(std::slice::from_ref(b)).len()).sum();
    set_u32(&mut whole_box, mdat_at, u32::MAX);
    assert!(open_and_play(whole_box).is_err());
    let mut large_size = tiny.to_vec();
    let moov_at = tiny.len() - write(&parse(tiny)[mdat + 1..]).len();
    let moov = large_size.split_off(moov_at);
    large_size.extend_from_slice(&1u32.to_be_bytes());
    large_size.extend_from_slice(b"moov");
    large_size.extend_from_slice(&(4u64 << 30).to_be_bytes());
    large_size.extend_from_slice(&moov[8..]);
    assert!(open_and_play(large_size).is_err());
    let inner_size = {
        let mut bytes = tiny.to_vec();
        let stsz = bytes.windows(4).position(|w| w == b"stsz").unwrap() - 4;
        set_u32(&mut bytes, stsz, 0xFFFF_0000);
        bytes
    };
    assert_eq!(open_and_play(inner_size), Err(CantPlay::Damaged));
    let many_sizes = edit(tiny, |boxes| set_u32(&mut find(boxes, "moov/trak/mdia/minf/stbl/stsz").data, 8, 1_000_000_000));
    assert!(matches!(open_and_play(many_sizes), Err(CantPlay::Damaged | CantPlay::TooLarge)));
    let one_size = edit(tiny, |boxes| {
        let stsz = &mut find(boxes, "moov/trak/mdia/minf/stbl/stsz").data;
        *stsz = table(0, &[u32::MAX - 1, 1]);
    });
    assert_eq!(open_and_play(one_size), Err(CantPlay::Damaged));
    let many_chunks = edit(tiny, |boxes| set_u32(&mut find(boxes, "moov/trak/mdia/minf/stbl/stco").data, 4, 1_000_000_000));
    assert_eq!(open_and_play(many_chunks), Err(CantPlay::Damaged));
    let many_counts = edit(tiny, |boxes| {
        find(boxes, "moov/trak/mdia/minf/stbl/stts").data = table(0, &[1, u32::MAX, 1000]);
    });
    assert!(open_and_play(many_counts).is_ok());
    let long_sps = {
        let mut bytes = tiny.to_vec();
        let avcc = bytes.windows(4).position(|w| w == b"avcC").unwrap() + 4;
        bytes[avcc + 6..avcc + 8].copy_from_slice(&u16::MAX.to_be_bytes());
        bytes
    };
    assert!(open_and_play(long_sps).is_err());

    // A sample table pointing past the end of the file.
    let past_end = edit(made, |boxes| {
        let len = made.len() as u32;
        set_u32(&mut find(boxes, "moov/trak/mdia/minf/stbl/stco").data, 8, len + 10);
    });
    assert_eq!(open_and_play(past_end), Err(CantPlay::Damaged));
    let past_end_by_size = edit(made, |boxes| {
        let stsz = &mut find(boxes, "moov/trak/mdia/minf/stbl/stsz").data;
        let last = 12 + 11 * 4;
        set_u32(stsz, last, 1 << 20);
    });
    assert_eq!(open_and_play(past_end_by_size), Err(CantPlay::Damaged));
    let no_chunks = edit(made, |boxes| find(boxes, "moov/trak/mdia/minf/stbl/stco").data = table(0, &[0]));
    assert_eq!(open_and_play(no_chunks), Err(CantPlay::Damaged));
    let empty_chunks = edit(made, |boxes| find(boxes, "moov/trak/mdia/minf/stbl/stsc").data = table(0, &[1, 1, 0, 1]));
    assert_eq!(open_and_play(empty_chunks), Err(CantPlay::Damaged));

    // Timestamps going backwards, and overflowing.
    let backwards = edit(made, |boxes| {
        let stbl = find(boxes, "moov/trak/mdia/minf/stbl");
        let offsets: Vec<u32> = (0..12u32).flat_map(|i| [1, if i % 2 == 0 { 1500 } else { (-500i32) as u32 }]).collect();
        let mut fields = vec![12];
        fields.extend(offsets);
        let ctts = Mp4Box { kind: *b"ctts", head: Vec::new(), data: table(1, &fields), inside: Vec::new() };
        stbl.inside.insert(2, ctts);
    });
    let info = open_and_play(backwards).unwrap();
    assert_eq!(info.frames, 12);
    let overflowing = edit(made, |boxes| {
        find(boxes, "moov/trak/mdia/minf/stbl/stts").data = table(0, &[2, 6, u32::MAX, 6, u32::MAX]);
    });
    assert_eq!(open_and_play(overflowing), Err(CantPlay::TooLarge));
    let still = edit(made, |boxes| find(boxes, "moov/trak/mdia/minf/stbl/stts").data = table(0, &[1, 12, 0]));
    assert_eq!(open_and_play(still), Err(CantPlay::Damaged));

    // The index at the end of the file, as the made videos have it, and at the start.
    assert_eq!(top_kinds(made).last().map(String::as_str), Some("moov"));
    let first = index_first(made);
    assert_eq!(top_kinds(&first)[1], "moov");
    assert_eq!(open_and_play(first), Ok(made_info));

    // 8K, and pictures of no size.
    assert_eq!(open_and_play(with_sps(made, &Sps::sized(7680, 4320).nal())), Err(CantPlay::TooLarge));
    assert_eq!(open_and_play(with_sps(made, &Sps::sized(4112, 2304).nal())), Err(CantPlay::TooLarge));
    assert!(open_and_play(with_sps(made, &Sps::sized(4096, 2304).nal())).is_ok());
    let zero_sized = edit(made, |boxes| {
        let stsz = &mut find(boxes, "moov/trak/mdia/minf/stbl/stsz").data;
        *stsz = table(0, &[0, 12]).into_iter().chain(std::iter::repeat_n(0, 48)).collect();
    });
    assert!(matches!(open_and_play(zero_sized), Ok(_) | Err(CantPlay::Damaged)));

    // 10,000 frames a second: the clock counts 30,000 a second, and each frame lasts 3.
    let fast = edit(made, |boxes| find(boxes, "moov/trak/mdia/minf/stbl/stts").data = table(0, &[1, 12, 3]));
    assert_eq!(open_and_play(fast), Err(CantPlay::TooLarge));
    // More than six hours.
    let long = edit(made, |boxes| find(boxes, "moov/trak/mdia/minf/stbl/stts").data = table(0, &[1, 12, 30_000 * 1900]));
    assert_eq!(open_and_play(long), Err(CantPlay::TooLarge));

    // Kinds that aren't played: 10-bit, 4:4:4, interlaced, and H.265.
    let ten_bit = Sps { profile: 110, bit_depth: 10, ..Sps::sized(48, 32) };
    assert_eq!(open_and_play(with_sps(made, &ten_bit.nal())), Err(CantPlay::KindNotPlayed));
    let full_chroma = Sps { profile: 244, chroma: 3, ..Sps::sized(48, 32) };
    assert_eq!(open_and_play(with_sps(made, &full_chroma.nal())), Err(CantPlay::KindNotPlayed));
    let interlaced = Sps { progressive: false, ..Sps::sized(48, 32) };
    assert_eq!(open_and_play(with_sps(made, &interlaced.nal())), Err(CantPlay::KindNotPlayed));
    let h265 = edit(made, |boxes| find(boxes, "moov/trak/mdia/minf/stbl/stsd/avc1").kind = *b"hvc1");
    assert_eq!(open_and_play(h265), Err(CantPlay::KindNotPlayed));
    let fragmented = edit(made, |boxes| {
        boxes.push(Mp4Box { kind: *b"moof", head: Vec::new(), data: vec![0; 8], inside: Vec::new() });
    });
    assert_eq!(open_and_play(fragmented), Err(CantPlay::KindNotPlayed));

    // Pictures that share their bytes: every one at the same place, so decoding would read the
    // same bytes for ever.
    let shared = edit(made, |boxes| {
        let stbl = find(boxes, "moov/trak/mdia/minf/stbl");
        let first = u32_at(&find(&mut stbl.inside, "stco").data, 8);
        find(&mut stbl.inside, "stsc").data = table(0, &[1, 1, 1, 1]);
        let mut chunks = vec![12];
        chunks.extend([first; 12]);
        find(&mut stbl.inside, "stco").data = table(0, &chunks);
    });
    assert_eq!(open_and_play(shared), Err(CantPlay::Damaged));
    // As many as can be claimed in a small index, each as large as the file allows, all at once.
    let piled = edit(made, |boxes| {
        let len = made.len() as u32;
        let stbl = find(boxes, "moov/trak/mdia/minf/stbl");
        let count = 100_000;
        find(&mut stbl.inside, "stsz").data = table(0, &[len - 100, count]);
        find(&mut stbl.inside, "stsc").data = table(0, &[1, 1, 1, 1]);
        let mut chunks = vec![count];
        chunks.extend(std::iter::repeat_n(50, count as usize));
        find(&mut stbl.inside, "stco").data = table(0, &chunks);
        find(&mut stbl.inside, "stts").data = table(0, &[1, count, 1000]);
    });
    assert_eq!(open_and_play(piled), Err(CantPlay::Damaged));
    // Overlapping by one byte.
    let places = samples_of(made);
    let overlapping = edit(made, |boxes| {
        let stbl = find(boxes, "moov/trak/mdia/minf/stbl");
        find(&mut stbl.inside, "stsc").data = table(0, &[1, 1, 1, 1]);
        let mut chunks = vec![12];
        chunks.extend(places.iter().map(|(at, _)| *at as u32));
        find(&mut stbl.inside, "stco").data = table(0, &chunks);
        let sizes = &mut find(&mut stbl.inside, "stsz").data;
        let first = u32_at(sizes, 12);
        set_u32(sizes, 12, first + 1);
    });
    let apart = edit(made, |boxes| {
        let stbl = find(boxes, "moov/trak/mdia/minf/stbl");
        find(&mut stbl.inside, "stsc").data = table(0, &[1, 1, 1, 1]);
        let mut chunks = vec![12];
        chunks.extend(places.iter().map(|(at, _)| *at as u32));
        find(&mut stbl.inside, "stco").data = table(0, &chunks);
    });
    assert_eq!(open_and_play(apart), Ok(made_info));
    assert_eq!(open_and_play(overlapping), Err(CantPlay::Damaged));

    // A picture shown long after the video ends.
    let late = edit(made, |boxes| {
        let stbl = find(boxes, "moov/trak/mdia/minf/stbl");
        let ctts = Mp4Box { kind: *b"ctts", head: Vec::new(), data: table(1, &[2, 11, 0, 1, 50_000_000]), inside: Vec::new() };
        stbl.inside.insert(2, ctts);
    });
    assert_eq!(open_and_play(late), Err(CantPlay::Damaged));
    let early = edit(made, |boxes| {
        let stbl = find(boxes, "moov/trak/mdia/minf/stbl");
        let ctts = Mp4Box { kind: *b"ctts", head: Vec::new(), data: table(1, &[2, 1, (-50_000_000i32) as u32, 11, 0]), inside: Vec::new() };
        stbl.inside.insert(2, ctts);
    });
    assert_eq!(open_and_play(early), Err(CantPlay::Damaged));

    // Pictures stored larger than the limit, cropped to within it; and crops of a block or more.
    let cropped_large = Sps { crop_right: 8, ..Sps::sized(4112, 2304) };
    assert!(open_and_play(with_sps(made, &cropped_large.nal())).is_err());
    let tall = Sps { crop_bottom: 4, ..Sps::sized(2304, 4112) };
    assert_eq!(open_and_play(with_sps(made, &tall.nal())), Err(CantPlay::TooLarge));
    let whole_block = Sps { crop_right: 8, ..Sps::sized(64, 32) };
    assert_eq!(open_and_play(with_sps(made, &whole_block.nal())), Err(CantPlay::Damaged));
    let rounded = Sps { crop_bottom: 4, ..Sps::sized(48, 48) };
    assert_eq!(open(reader(with_sps(made, &rounded.nal()))).unwrap().info().height, 40);
    // Every parameter set is checked, not only the first.
    let second_too_large = edit(made, |boxes| {
        let avcc = &mut find(boxes, "moov/trak/mdia/minf/stbl/stsd/avc1/avcC").data;
        let sps_len = u16::from_be_bytes([avcc[6], avcc[7]]) as usize;
        let large = Sps::sized(7680, 4320).nal();
        let mut data = avcc[..8 + sps_len].to_vec();
        data[5] = 0xE2;
        data.extend_from_slice(&(large.len() as u16).to_be_bytes());
        data.extend_from_slice(&large);
        data.extend_from_slice(&avcc[8 + sps_len..]);
        *avcc = data;
    });
    assert_eq!(open_and_play(second_too_large), Err(CantPlay::TooLarge));
}

#[test]
fn each_refusal_reads_in_plain_words() {
    let all = [
        CantPlay::NotAVideo,
        CantPlay::Damaged,
        CantPlay::NoPicture,
        CantPlay::KindNotPlayed,
        CantPlay::TooLarge,
        CantPlay::CouldNotRead,
    ];
    let mut seen = Vec::new();
    for why in all {
        let words = why.to_string();
        assert!(words.starts_with(|c: char| c.is_uppercase()) && words.ends_with('.'), "{words}");
        assert!(!words.contains(['<', '>', '/', '\\', '_', '`']), "{words}");
        // A codec is only ever named beside plain words, in brackets.
        for (at, _) in words.match_indices("H.264") {
            assert!(words[..at].ends_with('('), "{words}");
        }
        for name in ["H.265", "HEVC", "AVC", "avc1", "moov", "codec", "SPS", "mp4"] {
            assert!(!words.contains(name), "{words}");
        }
        assert!(!seen.contains(&words), "{words}");
        seen.push(words);
    }
    assert_eq!(
        CantPlay::KindNotPlayed.to_string(),
        "This kind of video can't be played here. An MP4 made with the usual settings (H.264) can."
    );
}

/// A file that counts the bytes read from it.
struct CountedFile {
    inner: std::io::Cursor<Vec<u8>>,
    read: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl std::io::Read for CountedFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.read.fetch_add(read as u64, std::sync::atomic::Ordering::SeqCst);
        Ok(read)
    }
}

impl std::io::Seek for CountedFile {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(to)
    }
}

fn size_and_read(bytes: Vec<u8>) -> (Result<(u32, u32), CantPlay>, u64) {
    let read = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mut file = CountedFile { inner: std::io::Cursor::new(bytes), read: read.clone() };
    (video_size(&mut file), read.load(std::sync::atomic::Ordering::SeqCst))
}

#[test]
fn a_videos_size_is_read_from_its_headers_alone() {
    // A long one, whose tables of pictures are far larger than its headers.
    let video = coloured_video(64, 48, 3000, 30);
    for bytes in [video.clone(), index_first(&video)] {
        let (size, read) = size_and_read(bytes);
        assert_eq!(size, Ok((64, 48)));
        assert!(read < 4096, "{read} bytes read");
    }
    assert_eq!(size_and_read(b"just words".to_vec()).0, Err(CantPlay::NotAVideo));
}

#[test]
fn finding_a_videos_size_never_reads_more_than_its_limit() {
    // A file of nothing but tiny boxes before its index.
    let video = small().to_vec();
    let mut crafted = Vec::new();
    for _ in 0..100_000 {
        crafted.extend_from_slice(&[0, 0, 0, 8, b'f', b'r', b'e', b'e']);
    }
    crafted.extend_from_slice(&video);
    let (size, read) = size_and_read(crafted);
    assert!(size.is_err());
    assert!(read <= MOST_READ, "{read} bytes read");
}

proptest! {
    #[test]
    fn any_bytes_give_a_size_or_say_why(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let (_, read) = size_and_read(bytes);
        prop_assert!(read <= MOST_READ);
    }

    #[test]
    fn a_damaged_video_gives_a_size_or_says_why(damage in damage()) {
        let (_, read) = size_and_read(damaged(&damage, false));
        prop_assert!(read <= MOST_READ);
    }
}

/// `video` with the height its sample description gives changed to `height`, as a file whose
/// pictures are coded taller than they're shown says.
fn described_as(video: &[u8], height: u16) -> Vec<u8> {
    let mut bytes = video.to_vec();
    let at = bytes.windows(4).position(|kind| kind == b"avc1").expect("a sample description") - 4;
    bytes[at + 34..at + 36].copy_from_slice(&height.to_be_bytes());
    bytes
}

#[test]
fn a_videos_size_is_the_size_it_is_shown_at() {
    let video = coloured_video(64, 48, 3, 30);
    // Its pictures coded 64 rows tall, shown 48 tall, as 1088 for 1080.
    let (size, _) = size_and_read(described_as(&video, 64));
    assert_eq!(size, Ok((64, 48)));
    assert_eq!(open(reader(described_as(&video, 64))).map(|video| (video.info().width, video.info().height)), Ok((64, 48)), "as it plays");
}
