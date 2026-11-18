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
    assert!(wait_for(Duration::from_millis(200), || alive.upgrade().is_none()), "the thread is still running");
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
    // The same pictures said to be in the other matrix come out visibly different, so the
    // check above can tell.
    let video = make_test_video_in(MadeColours::Bt709, 64, 48, 2, 30, |_| solid(64, 48, colour));
    let unsaid = make_test_video(64, 48, 2, 30, |_| solid(64, 48, colour));
    let unsaid_sps = {
        let mut boxes = parse(&unsaid);
        find(&mut boxes, "moov/trak/mdia/minf/stbl/stsd/avc1/avcC").data.clone()
    };
    let relabelled = edit(&video, |boxes| find(boxes, "moov/trak/mdia/minf/stbl/stsd/avc1/avcC").data = unsaid_sps);
    let frame = open(reader(relabelled)).unwrap().first_frame().unwrap();
    assert!(!is_colour(&frame, colour, 8), "{:?}", middle(&frame));
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
