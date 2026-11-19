use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell as Counter;
use std::sync::Arc;
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
    let last = player.last();
    assert_eq!((last.width(), last.height(), line(&last, 0)), (4, 1, "xyz".to_owned()));
    assert_eq!(&last, player.screen_at(secs(2.6)));
    // A recording of nothing is a blank screen of its size.
    let mut empty = Player::new(recording(7, 3, &[]));
    assert_eq!(empty.last(), Screen::new(7, 3));
    assert_eq!(empty.screen_at(secs(5.0)), &Screen::new(7, 3));
    assert_eq!(empty.stops(), &[] as &[Duration]);
}

#[test]
fn marks_are_stops() {
    let recording = recording(
        10,
        2,
        &[(0.5, "o", "a"), (1.0, "m", "first"), (1.5, "o", "b"), (9.0, "m", "second"), (9.2, "o", "c"), (9.2, "m", "")],
    );
    let mut player = Player::new(recording.clone());
    let stops: Vec<Duration> = recording.marks.iter().map(|mark| mark.at).collect();
    assert_eq!(player.stops(), stops.as_slice());
    assert_eq!(player.stops().len(), 3);
    assert_eq!(player.next_stop(Duration::ZERO), Some(stops[0]));
    assert_eq!(player.next_stop(stops[0]), Some(stops[1]));
    assert_eq!(player.next_stop(stops[1] - secs(0.01)), Some(stops[1]));
    assert_eq!(player.next_stop(stops[1]), Some(stops[2]));
    assert_eq!(player.next_stop(stops[2]), None);
    // At a stop the screen is as it was when the mark was made.
    assert_eq!(line(player.screen_at(stops[0]), 0), "a");
    assert_eq!(line(player.screen_at(stops[1]), 0), "ab");
    assert_eq!(line(player.screen_at(stops[2]), 0), "abc");
    assert_eq!(line(player.screen_at(stops[0]), 0), "a");
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
    assert_eq!(player.last(), *player.screen_at(recording.length));
    assert_eq!(player.last(), Player::new(recording).last());
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

    assert_eq!(size_of::<Cell>(), 20);
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
    let last = player.last();
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
