use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell as Counter;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use slides_terminal::*;

/// Counts, for each thread, the bytes it holds, so a test can tell what a player keeps apart
/// from the tests beside it.
struct Counting;

thread_local! {
    static HELD: Counter<isize> = const { Counter::new(0) };
}

fn changed(by: isize) {
    let _ = HELD.try_with(|held| held.set(held.get() + by));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        changed(layout.size() as isize);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        changed(layout.size() as isize);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        changed(new_size as isize - layout.size() as isize);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        changed(-(layout.size() as isize));
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

fn held() -> isize {
    HELD.with(Counter::get)
}

fn secs(seconds: f64) -> Duration {
    Duration::from_secs_f64(seconds)
}

fn recording(width: u16, height: u16, lines: &[(f64, &str, &str)]) -> Arc<Recording> {
    Arc::new(read(&make_test_cast(width, height, lines)).unwrap())
}

/// A row's text as it shows, the spaces at its end taken off.
fn line(screen: &Screen, row: u16) -> String {
    let cells = screen.rows().nth(usize::from(row)).unwrap();
    let text: String = cells.iter().filter(|cell| cell.wide != Wide::Right).map(|cell| cell.text).collect();
    text.trim_end().to_owned()
}

#[test]
fn a_recording_plays_at_its_own_times() {
    let mut player = Player::new(recording(
        10,
        2,
        &[(0.5, "o", "a"), (1.0, "o", "b"), (1.0, "o", "c"), (5.0, "o", "d"), (5.5, "r", "4x1"), (5.6, "o", "\r\nxyz")],
    ));
    assert_eq!(line(player.screen_at(Duration::ZERO), 0), "");
    assert_eq!(line(player.screen_at(secs(0.49)), 0), "");
    assert_eq!(line(player.screen_at(secs(0.5)), 0), "a");
    assert_eq!(line(player.screen_at(secs(0.99)), 0), "a");
    // Both events at a second come together.
    assert_eq!(line(player.screen_at(secs(1.0)), 0), "abc");
    // The wait of four seconds is played as one.
    assert_eq!(line(player.screen_at(secs(1.99)), 0), "abc");
    assert_eq!(line(player.screen_at(secs(2.0)), 0), "abcd");
    // The resize, and what came after it.
    let screen = player.screen_at(secs(2.5));
    assert_eq!((screen.width(), screen.height()), (4, 1));
    let screen = player.screen_at(secs(60.0));
    assert_eq!(line(screen, 0), "xyz");
    // Back to the start again.
    let screen = player.screen_at(secs(0.7));
    assert_eq!((screen.width(), screen.height(), line(screen, 0)), (10, 2, "a".to_owned()));
    // The screen as it ends, whatever's been played.
    let last = player.last().unwrap();
    assert_eq!((last.width(), last.height(), line(&last, 0)), (4, 1, "xyz".to_owned()));
    assert_eq!(&last, player.screen_at(secs(2.6)));
    // A recording of nothing is a blank screen of its size.
    let mut empty = Player::new(recording(7, 3, &[]));
    assert_eq!(empty.last().unwrap(), Screen::new(7, 3));
    assert_eq!(empty.screen_at(secs(5.0)), &Screen::new(7, 3));
    assert_eq!(empty.stops(), &[] as &[Duration]);
}

#[test]
fn marks_are_stops() {
    let recording = recording(
        10,
        2,
        &[
            (0.5, "o", "a"),
            (1.0, "m", "first"),
            (1.5, "o", "b"),
            (9.0, "m", "second"),
            (9.2, "o", "c"),
            (9.2, "m", ""),
            (9.2, "o", "d"),
            (9.2, "m", "again"),
        ],
    );
    let mut player = Player::new(recording.clone());
    // Two marks at the same time are one stop.
    assert_eq!(recording.marks.len(), 4);
    let stops: Vec<Duration> = recording.marks[..3].iter().map(|mark| mark.at).collect();
    assert_eq!(player.stops(), stops.as_slice());
    assert_eq!(player.next_stop(Duration::ZERO), Some(stops[0]));
    assert_eq!(player.next_stop(stops[0]), Some(stops[1]));
    assert_eq!(player.next_stop(stops[1] - secs(0.01)), Some(stops[1]));
    assert_eq!(player.next_stop(stops[1]), Some(stops[2]));
    assert_eq!(player.next_stop(stops[2]), None);
    // At a stop the screen is as it was when the mark was made.
    assert_eq!(line(player.screen_at(stops[0]), 0), "a");
    assert_eq!(line(player.screen_at(stops[1]), 0), "ab");
    // What came after a mark at its own time isn't shown at it, but is just after.
    assert_eq!(line(player.screen_at(stops[2]), 0), "abc");
    assert_eq!(line(player.screen_at(stops[2] + Duration::from_nanos(1)), 0), "abcd");
    assert_eq!(line(player.screen_at(stops[0]), 0), "a");
    assert_eq!(line(player.screen_at(stops[2]), 0), "abc");
    assert_eq!(line(&player.last().unwrap(), 0), "abcd");
}

#[test]
fn playing_can_be_stopped_part_way() {
    let recording = busy();
    let stop = AtomicBool::new(true);
    let mut player = Player::new(recording.clone());
    assert!(player.screen_at_within(recording.length, &stop).is_none());
    assert!(player.last_within(&stop).is_none());
    // Where it stopped, it goes on from.
    stop.store(false, Ordering::Relaxed);
    let want = Player::new(recording.clone()).screen_at(recording.length).clone();
    assert_eq!(player.screen_at_within(recording.length, &stop), Some(&want));
    assert_eq!(player.last_within(&stop), Some(Ok(want)));
}

/// A recording on the largest screen with every cell of both screens written, then `marks`
/// marks, each after a character if `written`, a thousandth of a second apart.
fn marked(marks: usize, written: bool) -> Arc<Recording> {
    let mut lines: Vec<(f64, &str, String)> = vec![
        (0.0, "o", "y".repeat(500 * 200)),
        (0.0, "o", format!("\u{1b}[?1049h{}", "x".repeat(500 * 200))),
    ];
    for i in 0..marks {
        let at = i as f64 * 0.001;
        if written {
            lines.push((at, "o", "z".to_owned()));
        }
        lines.push((at, "m", String::new()));
    }
    let lines: Vec<(f64, &str, &str)> = lines.iter().map(|(at, code, text)| (*at, *code, text.as_str())).collect();
    Arc::new(read(&make_test_cast(500, 200, &lines)).unwrap())
}

#[test]
fn many_marks_on_the_largest_screen_are_quick() {
    let release = !cfg!(debug_assertions);
    for (marks, written) in [(if release { 200_000 } else { 5_000 }, false), (if release { 50_000 } else { 3_000 }, true)] {
        let recording = marked(marks, written);
        let started = Instant::now();
        let before = held();
        let mut player = Player::new(recording.clone());
        let end = player.screen_at(recording.length).clone();
        let kept = held() - before;
        if release {
            assert!(started.elapsed() < Duration::from_secs(2), "{marks} marks took {:?}", started.elapsed());
        }
        assert!(kept <= (100_000_000 + recording.events.len() * 64) as isize, "{marks} marks kept {kept}");
        assert_eq!(player.last().unwrap(), end);
        // Back to marks here and there, and forward again.
        for mark in [marks / 2, 1, marks - 1, marks / 3, 0] {
            let at = recording.marks[mark].at;
            let started = Instant::now();
            let screen = player.screen_at(at).clone();
            if release {
                assert!(started.elapsed() < Duration::from_millis(50), "back to mark {mark} took {:?}", started.elapsed());
            }
            if mark % 3 == 0 {
                assert_eq!(screen, *Player::new(recording.clone()).screen_at(at), "mark {mark}");
            }
        }
    }
}

/// A recording on the largest screen with every cell of both screens written, then `unit`
/// written and a mark made, `times` times, a thousandth of a second apart.
fn stop_after_stop(unit: &str, times: usize) -> Arc<Recording> {
    let mut lines: Vec<(f64, &str, String)> = vec![
        (0.0, "o", "y".repeat(500 * 200)),
        (0.0, "o", format!("\u{1b}[?1049h{}", "x".repeat(500 * 200))),
    ];
    for i in 0..times {
        let at = i as f64 * 0.001;
        lines.push((at, "o", unit.to_owned()));
        lines.push((at, "m", String::new()));
    }
    let lines: Vec<(f64, &str, &str)> = lines.iter().map(|(at, code, text)| (*at, *code, text.as_str())).collect();
    Arc::new(read(&make_test_cast(500, 200, &lines)).unwrap())
}

/// Plays a recording to its end and back to marks here and there, each within a second, the
/// screen the same whichever way it's reached, and says whether it was too costly.
fn quick_both_ways(recording: &Arc<Recording>) -> bool {
    let started = Instant::now();
    let last = Player::new(recording.clone()).last();
    assert!(started.elapsed() < Duration::from_secs(1), "the last screen took {:?}", started.elapsed());
    let mut player = Player::new(recording.clone());
    let started = Instant::now();
    let costly = player.screen_at(recording.length).too_costly();
    assert!(started.elapsed() < Duration::from_secs(1), "playing to the end took {:?}", started.elapsed());
    assert_eq!(last.is_err(), costly, "the last screen and the end disagree");
    let marks = recording.marks.len();
    for mark in [marks / 2, marks / 3, 1, marks - 1, marks / 7] {
        let at = recording.marks[mark].at;
        player.screen_at(recording.length);
        let started = Instant::now();
        let back = player.screen_at(at).clone();
        assert!(started.elapsed() < Duration::from_secs(1), "back to mark {mark} took {:?}", started.elapsed());
        if mark != marks / 7 {
            assert_eq!(back, *Player::new(recording.clone()).screen_at(at), "mark {mark}");
        }
    }
    costly
}

#[test]
#[cfg_attr(debug_assertions, ignore = "this checks how quick a release build is")]
fn costly_output_between_stops_is_quick_both_ways() {
    // The screen erased in two colours before each of 499,000 stops: too costly to play, and
    // said so the same both ways.
    assert!(quick_both_ways(&stop_after_stop("\u{9b}41m\u{9b}2J\u{9b}42m\u{9b}2J", 499_000)));
    // A row and a half of characters before each of 100,000 stops: within the limit.
    assert!(!quick_both_ways(&stop_after_stop(&"z".repeat(270), 100_000)));
}

#[test]
#[cfg_attr(debug_assertions, ignore = "this plays as much as the most costly recording")]
fn a_recording_too_costly_to_play_says_so() {
    let fill = "\u{1b}[?1049h".to_owned() + &"x".repeat(500 * 200);
    // Each event within the most output one may hold, and all of them as much work as about
    // four seconds of it.
    let costly = "\u{9b}41m\u{9b}2J\u{9b}42m\u{9b}2J".repeat(50_000);
    let mut lines: Vec<(f64, &str, &str)> = vec![(0.0, "o", &fill)];
    for i in 0..20 {
        lines.push((f64::from(i), "o", &costly));
    }
    lines.push((31.0, "m", ""));
    let recording = Arc::new(read(&make_test_cast(500, 200, &lines)).unwrap());
    let started = Instant::now();
    assert_eq!(Player::new(recording.clone()).last(), Err(CantPlay::TooLarge));
    assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
    let mut player = Player::new(recording.clone());
    assert!(player.screen_at(recording.length).too_costly());
    // A million resizes, the most events a recording may have.
    let mut lines: Vec<(f64, &str, &str)> = vec![(0.0, "o", &fill)];
    for i in 0..999_999 {
        lines.push((0.0, "r", if i % 2 == 0 { "499x199" } else { "500x200" }));
    }
    let recording = Arc::new(read(&make_test_cast(500, 200, &lines)).unwrap());
    let started = Instant::now();
    let last = Player::new(recording).last();
    assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
    assert_eq!(last.err(), Some(CantPlay::TooLarge));
}

/// A recording of about a minute and a half with something of every kind in it: text, colours,
/// the cursor sent about, a scroll region, the alternate screen, resizes and marks.
fn busy() -> Arc<Recording> {
    let mut lines: Vec<(f64, &str, String)> = Vec::new();
    let mut at = 0.0;
    for i in 0..600 {
        at += [0.05, 0.1, 0.3, 0.02][i % 4];
        let text = match i % 9 {
            0 => format!("\u{1b}[3{}mline {i}\u{1b}[0m\r\n", i % 8),
            1 => format!("\u{1b}[{};{}H\u{4e2d}{i}", i % 13 + 1, i % 37 + 1),
            2 => "\u{1b}[2;10r\u{1b}[10;1H\n\n".to_owned(),
            3 => format!("\u{1b}[?1049h\u{1b}[H\u{1b}[7m top {i}\u{1b}[K\u{1b}[0m"),
            4 => format!("\u{1b}[{}L\u{1b}[2P\u{1b}[r", i % 3),
            5 => "\u{1b}[?1049l".to_owned(),
            6 => format!("\u{1b}[38;2;{};{};{}m{}\u{1b}[0m", i % 256, (i * 7) % 256, (i * 13) % 256, "x".repeat(i % 50)),
            7 => "\u{1b}]2;a title\u{7}\u{1b}[J".to_owned(),
            _ => format!("$ {i}\r\n"),
        };
        lines.push((at, "o", text));
        if i % 97 == 50 {
            lines.push((at, "m", format!("mark {i}")));
        }
        if i % 150 == 149 {
            lines.push((at, "r", format!("{}x{}", 30 + i % 40, 10 + i % 7)));
        }
    }
    let lines: Vec<(f64, &str, &str)> = lines.iter().map(|(at, code, text)| (*at, *code, text.as_str())).collect();
    Arc::new(read(&make_test_cast(40, 14, &lines)).unwrap())
}

#[test]
fn going_back_is_the_same_as_going_forward() {
    let recording = busy();
    assert!(recording.length > secs(60.0), "{:?}", recording.length);
    assert!(recording.marks.len() >= 5);
    let mut times: Vec<Duration> = (0..200).map(|i| recording.length.mul_f64(f64::from(i) / 199.0)).collect();
    times.extend(recording.marks.iter().map(|mark| mark.at));
    times.extend(played(&recording));

    let mut back = Player::new(recording.clone());
    back.screen_at(recording.length);
    let mut wandering = Player::new(recording.clone());
    for (i, at) in times.iter().enumerate() {
        let mut forward = Player::new(recording.clone());
        let want = forward.screen_at(*at).clone();
        assert_eq!(back.screen_at(*at), &want, "going back to {at:?}");
        back.screen_at(recording.length);
        // Back and forth in any order.
        let other = times[(i * 7919) % times.len()];
        wandering.screen_at(other);
        assert_eq!(wandering.screen_at(*at), &want, "going from {other:?} to {at:?}");
    }
    let mut player = Player::new(recording.clone());
    assert_eq!(player.last().unwrap(), *player.screen_at(recording.length));
    assert_eq!(player.last().unwrap(), Player::new(recording).last().unwrap());
}

#[test]
fn what_a_player_keeps_is_bounded() {
    // Every cell of both screens written before each of 500 marks: kept at every mark, the
    // screens would hold 600 MB.
    let fill = format!("\u{1b}[?1049h\u{1b}[H{}", "x".repeat(300 * 100));
    let mut lines: Vec<(f64, &str, String)> = vec![(0.0, "o", "y".repeat(300 * 100))];
    for i in 0..500 {
        let at = f64::from(i) * 0.1;
        lines.push((at, "o", format!("{fill}\u{1b}[H{i}")));
        lines.push((at, "m", String::new()));
    }
    let lines: Vec<(f64, &str, &str)> = lines.iter().map(|(at, code, text)| (*at, *code, text.as_str())).collect();
    let recording = Arc::new(read(&make_test_cast(300, 100, &lines)).unwrap());

    assert_eq!(size_of::<Cell>(), 28);
    let before = held();
    let mut player = Player::new(recording.clone());
    player.screen_at(recording.length);
    let kept = held() - before;
    // The kept screens' 100 MB, the screen played and the time of each event.
    let most = 100_000_000 + 2 * 300 * 100 * size_of::<Cell>() * 2 + recording.events.len() * 16;
    assert!(kept <= most as isize, "the player kept {kept}");
    // What was let go is played again from what was kept.
    for mark in [0, 1, 99, 250, 498, 499] {
        let at = recording.marks[mark].at;
        let want = Player::new(recording.clone()).screen_at(at).clone();
        assert_eq!(player.screen_at(at), &want, "going back to mark {mark}");
        assert!(line(&want, 0).starts_with(&mark.to_string()));
    }
}

/// A recording of two hours and 500,000 events, with a mark every minute or so.
fn long() -> Vec<u8> {
    let mut lines: Vec<(f64, &str, String)> = Vec::with_capacity(500_100);
    let step = 7200.0 / 500_000.0;
    for i in 0..500_000u32 {
        let at = f64::from(i) * step;
        let text = match i % 1000 {
            0 => "\u{1b}[?1049h\u{1b}[2J\u{1b}[H".to_owned(),
            1..=200 => format!("\u{1b}[{};1H\u{1b}[1;3{}m{i:>8} build \u{1b}[0m{}", i % 40 + 1, i % 8, "=".repeat((i % 60) as usize)),
            201 => "\u{1b}[?1049l".to_owned(),
            _ => match i % 5 {
                0 => format!("\u{1b}[32mok\u{1b}[0m test {i} \u{4e2d}\r\n"),
                1 => "$ ".to_owned(),
                2 => "l".to_owned(),
                3 => "s".to_owned(),
                _ => "\r\n".to_owned(),
            },
        };
        lines.push((at, "o", text));
        if i % 4000 == 3999 {
            lines.push((at, "m", "next".to_owned()));
        }
    }
    let lines: Vec<(f64, &str, &str)> = lines.iter().map(|(at, code, text)| (*at, *code, text.as_str())).collect();
    make_test_cast(160, 48, &lines)
}

#[test]
#[cfg_attr(debug_assertions, ignore = "this checks how quick a release build is")]
fn a_long_recording_is_stepped_quickly() {
    let recording = Arc::new(read(&long()).unwrap());
    assert!(recording.events.len() >= 500_000);
    assert!(recording.length > secs(7000.0));
    assert!(recording.marks.len() >= 100);

    let player = Player::new(recording.clone());
    let started = Instant::now();
    let last = player.last().unwrap();
    assert!(started.elapsed() < Duration::from_secs(2), "the last screen took {:?}", started.elapsed());

    let mut player = Player::new(recording.clone());
    assert_eq!(player.screen_at(recording.length), &last);
    // Back to each mark, latest first, then in any order.
    let marks: Vec<Duration> = recording.marks.iter().rev().map(|mark| mark.at).collect();
    let mut order = marks.clone();
    order.extend((0..marks.len()).map(|i| marks[(i * 37) % marks.len()]));
    let mut slowest = Duration::ZERO;
    for at in order {
        player.screen_at(recording.length);
        let started = Instant::now();
        player.screen_at(at);
        slowest = slowest.max(started.elapsed());
    }
    assert!(slowest < Duration::from_millis(50), "going back to a mark took {slowest:?}");
}

#[test]
fn a_player_says_when_its_screen_changes_and_ends() {
    let cast = make_test_cast(20, 4, &[(0.5, "o", "a"), (1.0, "o", "b"), (1.0, "m", ""), (1.0, "o", "c"), (1.5, "o", "d")]);
    let player = Player::new(Arc::new(read(&cast).unwrap()));
    assert_eq!(player.events_at(Duration::ZERO), 0);
    assert_eq!(player.events_at(Duration::from_millis(500)), 1);
    // At the stop, up to its mark: what's at the same time after it comes once it's played on.
    assert_eq!(player.events_at(Duration::from_secs(1)), 3);
    assert_eq!(player.events_at(Duration::from_millis(1001)), 4);
    assert_eq!(player.time_of(0), Some(Duration::from_millis(500)));
    assert_eq!(player.time_of(4), Some(Duration::from_millis(1500)));
    assert_eq!(player.time_of(5), None);
    assert_eq!(player.end(), Duration::from_millis(1500));
    assert_eq!(player.events_at(player.end()), 5);
}
