mod support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use proptest::prelude::*;
use slides_video::*;
use support::*;

/// Counts the largest single allocation any test here makes, so a test can tell whether a size
/// claimed by a file was ever allocated.
struct Counting;

static LARGEST: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LARGEST.fetch_max(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        LARGEST.fetch_max(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LARGEST.fetch_max(new_size, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

const MOST_ALLOCATED: usize = 64 * 1024 * 1024;
const MOST_TIME: Duration = Duration::from_secs(2);

/// A small made video for damaging: 12 frames of 48 by 32.
fn small() -> &'static [u8] {
    static SMALL: OnceLock<Vec<u8>> = OnceLock::new();
    SMALL.get_or_init(|| coloured_video(48, 32, 12, 30))
}

/// A made video of one frame, about a kilobyte.
fn tiny() -> &'static [u8] {
    static TINY: OnceLock<Vec<u8>> = OnceLock::new();
    TINY.get_or_init(|| coloured_video(16, 16, 1, 30))
}

/// Opens `bytes`, takes its first frame and plays it through, each within two seconds, and
/// allocating no more than 64 MB at once. What `open` said.
fn open_and_play(bytes: Vec<u8>) -> Result<VideoInfo, CantPlay> {
    let started = Instant::now();
    let opened = open(reader(bytes));
    assert!(started.elapsed() < MOST_TIME, "opening took {:?}", started.elapsed());
    let mut video = match opened {
        Ok(video) => video,
        Err(why) => {
            assert!(LARGEST.load(Ordering::Relaxed) <= MOST_ALLOCATED);
            return Err(why);
        }
    };
    let info = video.info();
    let started = Instant::now();
    let _ = video.first_frame();
    assert!(started.elapsed() < MOST_TIME, "the first frame took {:?}", started.elapsed());
    let started = Instant::now();
    let mut playing = video.play(0.0, false);
    let mut last = f64::NEG_INFINITY;
    while !playing.ended() && playing.failed().is_none() {
        if let Some(frame) = playing.frame_for(f64::INFINITY) {
            assert!(frame.at >= last, "times went back: {} after {last}", frame.at);
            last = frame.at;
        }
        assert!(started.elapsed() < MOST_TIME, "playing took {:?}", started.elapsed());
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(LARGEST.load(Ordering::Relaxed) <= MOST_ALLOCATED, "allocated {}", LARGEST.load(Ordering::Relaxed));
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

fn damaged(damage: &Damage) -> Vec<u8> {
    let mut video = small().to_vec();
    match damage {
        Damage::Random(bytes) => return bytes.clone(),
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
    fn any_bytes_open_or_say_why(damage in damage()) {
        let _ = open_and_play(damaged(&damage));
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
        let offsets: Vec<u32> = (0..12u32).flat_map(|i| [1, if i % 2 == 0 { 5000 } else { (-7000i32) as u32 }]).collect();
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
