mod support;

use std::time::{Duration, Instant};

use slides_video::*;
use support::*;

/// A little past the time frame `i` of a video at `rate` is due.
fn due(i: u32, rate: u32) -> f64 {
    i as f64 / rate as f64 + 0.001
}

#[test]
fn every_frame_comes_out_in_order() {
    let video = open(reader(coloured_video(64, 48, 90, 30))).unwrap();
    let mut playing = video.play(0.0, false);
    let mut last = -1.0;
    for i in 0..90 {
        let frame = next_frame(&mut playing, due(i, 30)).unwrap_or_else(|| panic!("no frame {i}"));
        assert!(frame.at > last, "frame {i} at {} after {last}", frame.at);
        assert!((frame.at - i as f64 / 30.0).abs() < 0.0005, "frame {i} at {}", frame.at);
        assert_eq!((frame.width, frame.height), (64, 48));
        assert!(is_colour(&frame, colour_of(i), 8), "frame {i} is {:?}, not {:?}", middle(&frame), colour_of(i));
        last = frame.at;
    }
    assert_eq!(playing.frame_for(1000.0).map(|f| f.at), None);
    assert!(wait_for(Duration::from_secs(2), || playing.ended()));
    assert_eq!(playing.failed(), None);
}

#[test]
fn frames_with_reordering_come_out_in_order() {
    // The encoder here can't make B-frames, so the file says its pictures are shown in another
    // order than they're stored, as a file with B-frames does: of every three, the first stored
    // is shown last. Whatever order the file claims, every frame comes out and the times only rise.
    let made = coloured_video(64, 48, 30, 30);
    let reordered = edit(&made, |boxes| {
        let stbl = find(boxes, "moov/trak/mdia/minf/stbl");
        let mut fields = vec![30];
        for i in 0..30 {
            fields.extend([1, if i % 3 == 0 { 3000 } else { 0 }]);
        }
        let ctts = Mp4Box { kind: *b"ctts", head: Vec::new(), data: table(0, &fields), inside: Vec::new() };
        stbl.inside.insert(2, ctts);
    });
    let video = open(reader(reordered)).unwrap();
    assert_eq!(video.info().frames, 30);
    let mut playing = video.play(0.0, false);
    let mut last = -1.0;
    for i in 0..30 {
        let frame = next_frame(&mut playing, due(i, 30)).unwrap_or_else(|| panic!("no frame {i}"));
        assert!(frame.at > last, "frame {i} at {} after {last}", frame.at);
        assert!((frame.at - i as f64 / 30.0).abs() < 0.0005, "frame {i} at {}", frame.at);
        last = frame.at;
    }
    assert!(wait_for(Duration::from_secs(2), || playing.ended()));
}

#[test]
fn frame_for_skips_what_is_late() {
    let video = open(reader(coloured_video(64, 48, 30, 30))).unwrap();
    let mut playing = video.play(0.0, false);
    assert!(wait_for(Duration::from_secs(2), || playing.most_queued() >= 12));
    let frame = playing.frame_for(due(5, 30)).unwrap();
    assert!((frame.at - 5.0 / 30.0).abs() < 0.0005, "{}", frame.at);
    assert!(is_colour(&frame, colour_of(5), 8));
    // Nothing newer is due yet.
    assert!(playing.frame_for(due(5, 30)).is_none());
    let frame = next_frame(&mut playing, due(6, 30)).unwrap();
    assert!(is_colour(&frame, colour_of(6), 8));
    // Those already decoded are passed over too.
    assert!(wait_for(Duration::from_secs(2), || playing.queued() == 12));
    let frame = playing.frame_for(due(12, 30)).unwrap();
    assert!(is_colour(&frame, colour_of(12), 8));
}

#[test]
fn looped_goes_round() {
    let video = open(reader(coloured_video(64, 48, 10, 30))).unwrap();
    let mut playing = video.play(0.0, true);
    for i in 0..35 {
        let frame = next_frame(&mut playing, due(i, 30)).unwrap_or_else(|| panic!("no frame {i}"));
        assert!((frame.at - (i % 10) as f64 / 30.0).abs() < 0.0005, "frame {i} at {}", frame.at);
        assert!(is_colour(&frame, colour_of(i % 10), 8), "frame {i}");
    }
    assert!(!playing.ended());
    assert_eq!(playing.failed(), None);
}

#[test]
fn not_looped_ends_and_holds_the_last() {
    let video = open(reader(coloured_video(64, 48, 10, 30))).unwrap();
    let mut playing = video.play(0.0, false);
    let mut last = None;
    for i in 0..10 {
        last = next_frame(&mut playing, due(i, 30));
        assert!(last.is_some(), "no frame {i}");
    }
    assert!(is_colour(last.as_ref().unwrap(), colour_of(9), 8));
    assert!(wait_for(Duration::from_secs(2), || playing.ended()));
    // After the end nothing newer comes, so the last frame given is the one that stays.
    assert!(playing.frame_for(100.0).is_none());
    assert!(playing.ended());
    assert_eq!(playing.failed(), None);
}

#[test]
fn play_from_the_middle_starts_at_the_frame_before() {
    // A picture of its own every second, so the middle isn't one.
    let video = open(reader(coloured_video(64, 48, 90, 30))).unwrap();
    let mut playing = video.play(1.51, false);
    let first = next_frame(&mut playing, 1.51).unwrap();
    assert!((first.at - 1.5).abs() < 0.0005, "{}", first.at);
    assert!(is_colour(&first, colour_of(45), 8), "{:?}", middle(&first));
    let next = next_frame(&mut playing, due(46, 30)).unwrap();
    assert!(is_colour(&next, colour_of(46), 8));

    // Just before a frame's time is still the frame before.
    let video = open(reader(coloured_video(64, 48, 90, 30))).unwrap();
    let mut playing = video.play(2.0 - 0.001, false);
    let first = next_frame(&mut playing, 2.0 - 0.001).unwrap();
    assert!(is_colour(&first, colour_of(59), 8), "{:?}", middle(&first));
}

#[cfg(target_os = "linux")]
fn thread_is_running(name: &str) -> bool {
    std::fs::read_dir("/proc/self/task").unwrap().flatten().any(|task| {
        std::fs::read_to_string(task.path().join("comm")).is_ok_and(|comm| comm.trim_end() == name)
    })
}

#[test]
fn dropping_it_ends_the_thread_quickly() {
    let video = open(reader(coloured_video(64, 48, 90, 30))).unwrap();
    let playing = video.play(0.0, true);
    let name = playing.thread_name().to_owned();
    #[cfg(target_os = "linux")]
    assert!(thread_is_running(&name), "no thread named {name}");
    let alive = playing.thread_alive();
    std::thread::sleep(Duration::from_millis(50));
    let dropped = Instant::now();
    drop(playing);
    // Dropping waits for the thread to let go of the file.
    assert!(alive.upgrade().is_none(), "the thread is still running");
    #[cfg(target_os = "linux")]
    assert!(wait_for(Duration::from_millis(200), || !thread_is_running(&name)), "the thread is still running");
    assert!(dropped.elapsed() < Duration::from_millis(200), "{:?}", dropped.elapsed());
}

#[test]
fn the_queue_never_holds_more_than_twelve() {
    let video = open(reader(coloured_video(64, 48, 90, 30))).unwrap();
    let mut playing = video.play(0.0, false);
    assert!(wait_for(Duration::from_secs(2), || playing.most_queued() >= 12));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(playing.most_queued(), 12);
    for i in 0..60 {
        next_frame(&mut playing, due(i, 30)).unwrap();
        if i % 10 == 0 {
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    assert_eq!(playing.most_queued(), 12);
}

#[test]
fn a_damaged_video_never_hangs_the_decoder() {
    let made = coloured_video(64, 48, 30, 30);
    let mdat = parse(&made).iter().take_while(|b| &b.kind != b"mdat").map(|b| write(std::slice::from_ref(b)).len()).sum::<usize>();
    let mdat_len = write(&parse(&made)[top_kinds(&made).iter().position(|k| k == "mdat").unwrap()..][..1]).len();
    let mut seed = 0x2545_f491_u32;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    let mut cases = Vec::new();
    // Every byte of the pictures turned to noise, so nothing can be decoded.
    let mut noise = made.clone();
    for byte in &mut noise[mdat + 16..mdat + mdat_len] {
        *byte = next() as u8;
    }
    cases.push(noise);
    // Runs of noise through the pictures, and pictures cut short.
    for _ in 0..24 {
        let mut bytes = made.clone();
        let at = mdat + 16 + next() as usize % (mdat_len - 16);
        let end = (at + 1 + next() as usize % 400).min(mdat + mdat_len);
        for byte in &mut bytes[at..end] {
            *byte = next() as u8;
        }
        cases.push(bytes);
    }
    for (i, bytes) in cases.into_iter().enumerate() {
        for looped in [false, true] {
            let Ok(mut video) = open(reader(bytes.clone())) else { continue };
            let started = Instant::now();
            let _ = video.first_frame();
            assert!(started.elapsed() < Duration::from_secs(2), "case {i}: the first frame took {:?}", started.elapsed());
            let mut playing = video.play(0.0, looped);
            let started = Instant::now();
            let mut frames = 0;
            // Played through twice over when looped; a video that gives nothing says so.
            while !playing.ended() && playing.failed().is_none() && frames < 60 {
                if playing.frame_for(f64::INFINITY).is_some() {
                    frames += 1;
                }
                assert!(started.elapsed() < Duration::from_secs(2), "case {i}, looped {looped}: still decoding");
                std::thread::sleep(Duration::from_millis(1));
            }
            let alive = playing.thread_alive();
            drop(playing);
            assert!(wait_for(Duration::from_millis(200), || alive.upgrade().is_none()), "case {i}: the thread hangs");
        }
    }
    // Nothing decodable says it is damaged, looped or not, and doesn't go round for ever.
    let mut noise = made.clone();
    for byte in &mut noise[mdat + 16..mdat + mdat_len] {
        *byte = 0x55;
    }
    for looped in [false, true] {
        let mut playing = open(reader(noise.clone())).unwrap().play(0.0, looped);
        assert!(wait_for(Duration::from_secs(2), || {
            playing.frame_for(f64::INFINITY);
            playing.failed().is_some() || playing.ended()
        }));
    }
    let mut video = open(reader(noise)).unwrap();
    assert_eq!(video.first_frame().err(), Some(CantPlay::Damaged));
}

#[test]
fn colours_follow_the_files_matrix() {
    // A colour far enough from grey that reading it with the wrong matrix or range is off by
    // more than the 8 levels allowed.
    let colour = [40, 200, 90];
    for kind in [MadeColours::Unsaid, MadeColours::Bt709, MadeColours::Bt709Full] {
        let video = make_test_video_in(kind, 64, 48, 2, 30, |_| solid(64, 48, colour));
        let frame = open(reader(video)).unwrap().first_frame().unwrap();
        assert!(is_colour(&frame, colour, 8), "{kind:?}: {:?}", middle(&frame));
        assert_eq!(frame.at, 0.0);
    }
    // Stored as green, blue and red, with no matrix.
    let identity = make_test_video_in(MadeColours::Identity, 64, 48, 2, 30, |_| solid(64, 48, colour));
    let frame = open(reader(identity)).unwrap().first_frame().unwrap();
    assert!(is_colour(&frame, colour, 8), "identity: {:?}", middle(&frame));
    // The same pictures said to be in the other matrix come out visibly different, so the
    // check above can tell.
    let video = make_test_video_in(MadeColours::Bt709, 64, 48, 2, 30, |_| solid(64, 48, colour));
    let unsaid = make_test_video(64, 48, 2, 30, |_| solid(64, 48, colour));
    let unsaid_sps = {
        let mut boxes = parse(&unsaid);
        find(&mut boxes, "moov/trak/mdia/minf/stbl/stsd/avc1/avcC").data.clone()
    };
    let relabelled = edit(&video, |boxes| find(boxes, "moov/trak/mdia/minf/stbl/stsd/avc1/avcC").data = unsaid_sps);
    let frame = open(reader(relabelled.clone())).unwrap().first_frame().unwrap();
    assert!(!is_colour(&frame, colour, 8), "{:?}", middle(&frame));
    // The sample entry's own words on colours count over the stream's.
    let said = edit(&relabelled, |boxes| {
        let avc1 = find(boxes, "moov/trak/mdia/minf/stbl/stsd/avc1");
        let mut nclx = b"nclx".to_vec();
        nclx.extend_from_slice(&[0, 1, 0, 1, 0, 1, 0]);
        avc1.inside.push(Mp4Box { kind: *b"colr", head: Vec::new(), data: nclx, inside: Vec::new() });
    });
    let frame = open(reader(said)).unwrap().first_frame().unwrap();
    assert!(is_colour(&frame, colour, 8), "said in the entry: {:?}", middle(&frame));
}

#[test]
fn looped_with_reordering_goes_round() {
    let made = coloured_video(64, 48, 30, 30);
    let reordered = edit(&made, |boxes| {
        let stbl = find(boxes, "moov/trak/mdia/minf/stbl");
        let mut fields = vec![30];
        for i in 0..30 {
            fields.extend([1, if i % 3 == 0 { 3000 } else { 0 }]);
        }
        let ctts = Mp4Box { kind: *b"ctts", head: Vec::new(), data: table(0, &fields), inside: Vec::new() };
        stbl.inside.insert(2, ctts);
    });
    let mut playing = open(reader(reordered)).unwrap().play(0.0, true);
    for i in 0..75 {
        let frame = next_frame(&mut playing, due(i, 30)).unwrap_or_else(|| panic!("no frame {i}"));
        assert!((frame.at - (i % 30) as f64 / 30.0).abs() < 0.0005, "frame {i} at {}", frame.at);
    }
}

/// Counts the bytes read through it.
struct Counted {
    inner: std::io::Cursor<Vec<u8>>,
    read: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl std::io::Read for Counted {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read.fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
        Ok(n)
    }
}

impl std::io::Seek for Counted {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(to)
    }
}

#[test]
fn pictures_that_never_decode_stop_the_reading() {
    // 96 pictures of a megabyte each, each its own bytes, none of them a picture.
    let made = coloured_video(64, 48, 96, 30);
    let noise = with_samples(&made, |i, _| picture_of(&[[0x65, 0x88, (i as u8) | 0x80, 0x10].repeat(1 << 18)]));
    let read = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let counted = Counted { inner: std::io::Cursor::new(noise.clone()), read: read.clone() };
    let mut video = open(Box::new(counted)).unwrap();
    let started = Instant::now();
    assert_eq!(video.first_frame().err(), Some(CantPlay::Damaged));
    let first = read.load(std::sync::atomic::Ordering::Relaxed);
    assert!(first <= 65 << 20, "read {first} bytes for no picture");
    let mut playing = video.play(0.0, true);
    assert!(wait_for(Duration::from_secs(10), || playing.failed().is_some()));
    assert_eq!(playing.failed(), Some(CantPlay::Damaged));
    assert!(read.load(std::sync::atomic::Ordering::Relaxed) - first <= 65 << 20);
    println!("gave up after {:?}", started.elapsed());
    playing.frame_for(1.0);
}

#[test]
fn a_hidden_start_code_damages_only_its_picture() {
    // Picture 29 is the last before a picture of its own at one second, so nothing after needs it.
    let made = coloured_video(64, 48, 60, 30);
    let smuggled = with_samples(&made, |i, sample| {
        if i != 29 {
            return sample;
        }
        let mut units = units(&sample);
        let slice = units.iter_mut().find(|u| u[0] & 0x1F == 1).expect("a slice");
        assert!(slice.len() > 6, "the slice is {} bytes", slice.len());
        // What would read as a new unit, a sequence parameter set, inside the slice.
        slice.splice(3..3, [0, 0, 1, 0x67]);
        picture_of(&units)
    });
    let mut playing = open(reader(smuggled)).unwrap().play(0.0, false);
    for i in 0..29 {
        let frame = next_frame(&mut playing, due(i, 30)).unwrap();
        assert!(is_colour(&frame, colour_of(i), 8), "frame {i}");
    }
    // The damaged picture is passed over, and the next keeps its own time.
    let frame = next_frame(&mut playing, due(30, 30)).unwrap();
    assert!(is_colour(&frame, colour_of(30), 8), "{:?}", middle(&frame));
    assert!((frame.at - 1.0).abs() < 0.0005, "{}", frame.at);
    for i in 31..60 {
        let frame = next_frame(&mut playing, due(i, 30)).unwrap();
        assert!((frame.at - i as f64 / 30.0).abs() < 0.0005, "frame {i} at {}", frame.at);
    }
}

#[test]
fn every_parameter_set_reaches_the_decoder() {
    let made = coloured_video(64, 48, 10, 30);
    let avcc = find(&mut parse(&made), "moov/trak/mdia/minf/stbl/stsd/avc1/avcC").data.clone();
    let sps_len = u16::from_be_bytes([avcc[6], avcc[7]]) as usize;
    let real_sps = avcc[8..8 + sps_len].to_vec();
    let pps_at = 8 + sps_len + 1;
    let pps_len = u16::from_be_bytes([avcc[pps_at], avcc[pps_at + 1]]) as usize;
    let real_pps = avcc[pps_at + 2..pps_at + 2 + pps_len].to_vec();

    // Another picture parameter set first: the pictures name the second.
    let other = pps_with_id(&real_pps, 1);
    let two_pps = edit(&made, |boxes| {
        let avcc = &mut find(boxes, "moov/trak/mdia/minf/stbl/stsd/avc1/avcC").data;
        let mut data = avcc[..pps_at - 1].to_vec();
        data.push(2);
        for pps in [&other, &real_pps] {
            data.extend_from_slice(&(pps.len() as u16).to_be_bytes());
            data.extend_from_slice(pps);
        }
        *avcc = data;
    });
    let frame = open(reader(two_pps)).unwrap().first_frame().unwrap();
    assert!(is_colour(&frame, colour_of(0), 8), "{:?}", middle(&frame));

    // A sequence parameter set in the first picture counts over the index's.
    let wrong = with_sps(&made, &Sps::sized(64, 48).nal());
    let in_picture = with_samples(&wrong, |i, sample| {
        if i == 0 { [picture_of(&[real_sps.clone(), real_pps.clone()]), sample].concat() } else { sample }
    });
    let frame = open(reader(in_picture)).unwrap().first_frame().unwrap();
    assert!(is_colour(&frame, colour_of(0), 8), "{:?}", middle(&frame));
    // Without it the index's wrong one is used, so the check above can tell.
    let frame = open(reader(wrong)).unwrap().first_frame();
    assert!(frame.is_err() || !is_colour(frame.as_ref().unwrap(), colour_of(0), 8));

    // One that wouldn't be played damages its picture.
    let large = with_samples(&made, |i, sample| {
        if i == 0 { [picture_of(&[Sps::sized(7680, 4320).nal()]), sample].concat() } else { sample }
    });
    let mut video = open(reader(large)).unwrap();
    assert_eq!(video.info().width, 64);
    if let Ok(frame) = video.first_frame() {
        assert_eq!((frame.width, frame.height), (64, 48));
    }
}

/// The time a frame takes to decode at 1920 by 1080. Run with `--release -- --ignored`.
#[test]
#[ignore]
fn decoding_1080p_takes() {
    let (width, height) = (1920, 1080);
    let video = make_test_video(width, height, 300, 30, |i| {
        let mut bgra = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let v = ((x + i * 4) ^ (y + i * 2)) as u8;
                bgra.extend_from_slice(&[v, v.wrapping_mul(3), (x / 8 + y / 8 + i) as u8, 255]);
            }
        }
        bgra
    });
    let mut times = open(reader(video)).unwrap().time_each_frame();
    assert_eq!(times.len(), 300);
    times.sort();
    let at = |p: f64| times[((times.len() - 1) as f64 * p).round() as usize];
    println!("1920 by 1080, 300 frames: decode median {:?}, 99th percentile {:?}", at(0.5), at(0.99));
}

/// A file whose reading fails once `failing` is set, and that says when it's let go of.
struct FailingFile {
    inner: std::io::Cursor<Vec<u8>>,
    failing: std::sync::Arc<std::sync::atomic::AtomicBool>,
    dropped: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl std::io::Read for FailingFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.failing.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(std::io::Error::other("the disk went away"));
        }
        self.inner.read(buf)
    }
}

impl std::io::Seek for FailingFile {
    fn seek(&mut self, to: std::io::SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(to)
    }
}

impl Drop for FailingFile {
    fn drop(&mut self) {
        self.dropped.store(true, std::sync::atomic::Ordering::SeqCst);
        // Closing a file can take a moment.
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_video_failing_part_way_says_why_before_it_lets_go_of_the_file() {
    use std::sync::atomic::{AtomicBool, Ordering};
    for _ in 0..5 {
        let (failing, dropped) = (std::sync::Arc::new(AtomicBool::new(false)), std::sync::Arc::new(AtomicBool::new(false)));
        let file = FailingFile { inner: std::io::Cursor::new(coloured_video(64, 48, 300, 30)), failing: failing.clone(), dropped: dropped.clone() };
        let mut playing = open(Box::new(file)).unwrap().play(0.0, false);
        assert!(next_frame(&mut playing, 0.0).is_some(), "it plays");
        failing.store(true, Ordering::SeqCst);
        let mut at = 0.0;
        let let_go = wait_for(Duration::from_secs(5), || {
            at += 1.0 / 30.0;
            playing.frame_for(at);
            // Seen let go of, the file's thread has said why it stopped already.
            if dropped.load(Ordering::SeqCst) {
                assert_eq!(playing.failed(), Some(CantPlay::CouldNotRead), "let go of the file before saying why");
                return true;
            }
            false
        });
        assert!(let_go, "the thread let go of the file");
    }
}
