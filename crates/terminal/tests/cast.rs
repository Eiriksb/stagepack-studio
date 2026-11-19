use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::{Duration, Instant};

use proptest::prelude::*;
use slides_terminal::*;

/// Counts, for each thread, the bytes it holds and the largest single allocation it made since
/// asked last, so a test can tell what reading kept and whether a size claimed by a file was
/// ever allocated, apart from what the test itself and the tests beside it hold.
struct Counting;

thread_local! {
    static HELD: Cell<isize> = const { Cell::new(0) };
    static LARGEST: Cell<usize> = const { Cell::new(0) };
}

fn allocated(size: usize) {
    let _ = HELD.try_with(|held| held.set(held.get() + size as isize));
    let _ = LARGEST.try_with(|largest| largest.set(largest.get().max(size)));
}

fn released(size: usize) {
    let _ = HELD.try_with(|held| held.set(held.get() - size as isize));
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
        released(layout.size());
        allocated(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        released(layout.size());
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

fn held() -> isize {
    HELD.with(Cell::get)
}

/// Runs `work`, saying what it returned, the most it allocated at once, and how much more is
/// held after it than before.
fn measured<T>(work: impl FnOnce() -> T) -> (T, usize, isize) {
    LARGEST.with(|largest| largest.set(0));
    let before = held();
    let out = work();
    (out, LARGEST.with(Cell::get), held() - before)
}

/// The most allocated at once while reading.
const MOST_ALLOCATED: usize = 64 * 1024 * 1024;
const MOST_TIME: Duration = Duration::from_secs(2);

/// Says what `bytes` is and reads it whole, each within two seconds, allocating no more than
/// 64 MB at once and keeping no more than the most a recording keeps; finds its played times,
/// keeping no more than a time an event. What `read` said, after checking `facts` agreed.
fn facts_and_read(bytes: &[u8]) -> Result<Recording, CantPlay> {
    let started = Instant::now();
    let (facts, largest, kept) = measured(|| facts(bytes));
    assert!(started.elapsed() < MOST_TIME, "facts took {:?}", started.elapsed());
    assert!(largest <= MOST_ALLOCATED, "facts allocated {largest} at once");
    assert_eq!(kept, 0, "facts kept something");
    let started = Instant::now();
    let (recording, largest, kept) = measured(|| read(bytes));
    assert!(started.elapsed() < MOST_TIME, "reading took {:?}", started.elapsed());
    assert!(largest <= MOST_ALLOCATED, "reading allocated {largest} at once");
    assert!(kept <= (MOST_KEPT + 1024) as isize, "reading kept {kept}");
    match (&facts, &recording) {
        (Ok(facts), Ok(recording)) => {
            agree(facts, recording);
            let (times, _, added) = measured(|| played(recording));
            assert_eq!(times.len(), recording.events.len());
            assert!(added <= (times.len() * size_of::<Duration>()) as isize + 64, "played kept {added}");
        }
        (Err(why), _) => assert_eq!(recording.as_ref().err(), Some(why), "read and facts disagree"),
        (Ok(_), Err(why)) => panic!("facts read it and read said {why}"),
    }
    recording
}

fn agree(facts: &RecordingFacts, recording: &Recording) {
    assert_eq!(facts.width, recording.width);
    assert_eq!(facts.height, recording.height);
    assert_eq!(facts.length, recording.length);
    assert_eq!(facts.marks, recording.marks.len());
}

fn secs(seconds: f64) -> Duration {
    Duration::from_secs_f64(seconds)
}

fn close(a: Duration, b: Duration) -> bool {
    a.abs_diff(b) < Duration::from_micros(10)
}

fn made() -> Vec<u8> {
    make_test_cast(
        80,
        24,
        &[
            (0.5, "o", "$ "),
            (0.7, "i", "l"),
            (0.75, "o", "l"),
            (0.9, "o", "s\r\n"),
            (1.2, "m", "listed"),
            // A gap of three seconds, played as one.
            (4.2, "o", "\u{1b}[32mdone\u{1b}[0m\r\n"),
            (4.4, "r", "100x30"),
            (4.6, "m", ""),
        ],
    )
}

#[test]
fn a_made_recording_reads_back() {
    let recording = facts_and_read(&made()).unwrap();
    assert_eq!((recording.width, recording.height), (80, 24));
    let whats: Vec<&What> = recording.events.iter().map(|event| &event.what).collect();
    assert_eq!(
        whats,
        [
            &What::Output("$ ".into()),
            &What::Output("l".into()),
            &What::Output("s\r\n".into()),
            &What::Mark("listed".into()),
            &What::Output("\u{1b}[32mdone\u{1b}[0m\r\n".into()),
            &What::Resize(100, 30),
            &What::Mark(String::new()),
        ]
    );
    // The events keep the times they were recorded at.
    assert!(close(recording.events[4].at, secs(4.2)));
    // The length and the marks are played times: the three seconds became one.
    assert!(close(recording.length, secs(2.6)), "{:?}", recording.length);
    assert_eq!(recording.marks.len(), 2);
    assert_eq!((recording.marks[0].event, recording.label(&recording.marks[0])), (3, "listed"));
    assert!(close(recording.marks[0].at, secs(1.2)));
    assert_eq!((recording.marks[1].event, recording.label(&recording.marks[1])), (6, ""));
    assert!(close(recording.marks[1].at, secs(2.6)));

    let times = played(&recording);
    assert_eq!(times.len(), recording.events.len());
    for (at, want) in times.iter().zip([0.5, 0.75, 0.9, 1.2, 2.2, 2.4, 2.6]) {
        assert!(close(*at, secs(want)), "{times:?}");
    }
    assert_eq!(*times.last().unwrap(), recording.length);
    assert_eq!(times[3], recording.marks[0].at);

    // The first wait is shortened too, and a recording of nothing is no length.
    let late = facts_and_read(&make_test_cast(10, 5, &[(30.0, "o", "x")])).unwrap();
    assert!(close(late.length, secs(1.0)));
    let empty = facts_and_read(&make_test_cast(10, 5, &[])).unwrap();
    assert!(empty.events.is_empty() && empty.length == Duration::ZERO);

    // The demo reads, with its mark.
    let demo = facts_and_read(&make_demo_cast()).unwrap();
    assert_eq!(demo.marks.len(), 1);
    assert!(demo.events.iter().any(|event| matches!(&event.what, What::Output(out) if out.contains("\u{1b}[?1049h"))));
    assert!(demo.events.iter().any(|event| matches!(&event.what, What::Output(out) if out.contains("\u{1b}[?1049l"))));
    assert!(demo.length > secs(3.0) && demo.length < secs(60.0));
}

#[test]
fn input_and_unknown_codes_are_left_out() {
    let cast = make_test_cast(20, 10, &[(0.1, "i", "secret"), (0.2, "o", "a"), (0.3, "x", "who knows"), (0.4, "", ""), (0.5, "o", "b")]);
    let recording = facts_and_read(&cast).unwrap();
    let whats: Vec<&What> = recording.events.iter().map(|event| &event.what).collect();
    assert_eq!(whats, [&What::Output("a".into()), &What::Output("b".into())]);
    assert!(close(recording.length, secs(0.5)));
    // A line that isn't an event of three parts is damage, whatever its code.
    let extra = [make_test_cast(20, 10, &[]), b"[0.1, \"i\", \"x\", 4]\n".to_vec()].concat();
    assert_eq!(facts_and_read(&extra).err(), Some(CantPlay::Damaged));
    // Blank lines are passed over, and a line may end as Windows ends one.
    let blanks = [make_test_cast(20, 10, &[]), b"\n  \r\n[0.1, \"o\", \"x\"]\r\n\n".to_vec()].concat();
    assert_eq!(facts_and_read(&blanks).unwrap().events.len(), 1);
}

#[test]
fn version_one_and_three_are_kinds_not_played() {
    let one = b"{\"version\": 1, \"width\": 80, \"height\": 24, \"duration\": 1.5, \"stdout\": [[0.5, \"hi\"]]}\n";
    assert_eq!(facts_and_read(one).err(), Some(CantPlay::KindNotPlayed));
    // As asciinema wrote them, over many lines.
    let pretty = b"{\n  \"version\": 1,\n  \"width\": 80,\n  \"height\": 24,\n  \"stdout\": [\n    [0.5, \"hi\"]\n  ]\n}\n";
    assert_eq!(facts_and_read(pretty).err(), Some(CantPlay::KindNotPlayed));
    let three = b"{\"version\": 3, \"term\": {\"cols\": 80, \"rows\": 24}}\n[0.5, \"o\", \"hi\"]\n";
    assert_eq!(facts_and_read(three).err(), Some(CantPlay::KindNotPlayed));
}

/// Ways a made recording can be damaged.
#[derive(Clone, Debug)]
enum Damage {
    Random(Vec<u8>),
    RandomEvents(Vec<u8>),
    Byte(usize, u8),
    Removed(usize),
    Repeated(usize),
    Cut(usize),
}

fn damage() -> impl Strategy<Value = Damage> {
    prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..2048).prop_map(Damage::Random),
        proptest::collection::vec(any::<u8>(), 0..2048).prop_map(Damage::RandomEvents),
        (any::<usize>(), any::<u8>()).prop_map(|(at, value)| Damage::Byte(at, value)),
        any::<usize>().prop_map(Damage::Removed),
        any::<usize>().prop_map(Damage::Repeated),
        any::<usize>().prop_map(Damage::Cut),
    ]
}

fn damaged(damage: &Damage, which: usize) -> Vec<u8> {
    let mut bytes = if which.is_multiple_of(2) { made() } else { make_demo_cast() };
    let lines: Vec<&[u8]> = bytes.split_inclusive(|b| *b == b'\n').collect();
    match damage {
        Damage::Random(random) => return random.clone(),
        Damage::RandomEvents(random) => return [lines[0], random].concat(),
        Damage::Byte(at, value) => {
            let at = at % bytes.len();
            bytes[at] = *value;
        }
        Damage::Removed(at) => {
            let at = at % lines.len();
            return [&lines[..at], &lines[at + 1..]].concat().concat();
        }
        Damage::Repeated(at) => {
            let at = at % lines.len();
            return [&lines[..=at], &lines[at..]].concat().concat();
        }
        Damage::Cut(at) => bytes.truncate(at % bytes.len()),
    }
    bytes
}

/// A modest number of cases, so this runs in seconds with the rest; `PROPTEST_CASES` asks for
/// more.
fn cases() -> u32 {
    std::env::var("PROPTEST_CASES").ok().and_then(|cases| cases.parse().ok()).unwrap_or(256)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), ..ProptestConfig::default() })]

    #[test]
    fn any_bytes_read_or_say_why(damage in damage(), which in 0usize..2) {
        let _ = facts_and_read(&damaged(&damage, which));
    }
}

#[test]
fn any_bytes_read_or_say_why_named_cases() {
    let header = |rest: &str| format!("{{\"version\": 2, {rest}}}\n").into_bytes();
    let events = |lines: &str| [make_test_cast(80, 24, &[]), lines.as_bytes().to_vec()].concat();

    assert_eq!(facts_and_read(b"").err(), Some(CantPlay::NotARecording));
    assert_eq!(facts_and_read(b"\n\n").err(), Some(CantPlay::NotARecording));
    // A picture named as a recording by mistake.
    let picture = [b"\x89PNG\r\n\x1a\n".as_slice(), &[0u8; 64]].concat();
    assert_eq!(facts_and_read(&picture).err(), Some(CantPlay::NotARecording));
    assert_eq!(facts_and_read(b"[0.5, \"o\", \"hi\"]\n").err(), Some(CantPlay::NotARecording));
    assert_eq!(facts_and_read(b"{\"width\": 80, \"height\": 24}\n").err(), Some(CantPlay::NotARecording));
    // A header that isn't JSON, or lacks a width or a height, or has a strange one.
    assert_eq!(facts_and_read(b"{version 2 width 80}\n").err(), Some(CantPlay::Damaged));
    assert_eq!(facts_and_read(&header("\"width\": 80")).err(), Some(CantPlay::Damaged));
    assert_eq!(facts_and_read(&header("\"height\": 24")).err(), Some(CantPlay::Damaged));
    for odd in ["0", "-3", "80.5", "\"80\"", "null", "[80]"] {
        assert_eq!(facts_and_read(&header(&format!("\"width\": {odd}, \"height\": 24"))).err(), Some(CantPlay::Damaged), "{odd}");
    }
    // A header that says 100,000 columns, or a screen a row or a column past the most.
    assert_eq!(facts_and_read(&header("\"width\": 100000, \"height\": 24")).err(), Some(CantPlay::TooLarge));
    assert_eq!(facts_and_read(&header("\"width\": 501, \"height\": 24")).err(), Some(CantPlay::TooLarge));
    assert_eq!(facts_and_read(&header("\"width\": 80, \"height\": 201")).err(), Some(CantPlay::TooLarge));
    assert!(facts_and_read(&header("\"width\": 500, \"height\": 200, \"title\": \"x\", \"env\": {\"TERM\": \"xterm\"}, \"timestamp\": 1")).is_ok());
    // A resize past the most, or one that isn't a size.
    assert_eq!(facts_and_read(&events("[0.1, \"r\", \"501x24\"]\n")).err(), Some(CantPlay::TooLarge));
    assert_eq!(facts_and_read(&events("[0.1, \"r\", \"99999999999999999999x24\"]\n")).err(), Some(CantPlay::TooLarge));
    for odd in ["0x24", "80", "80x", "x24", "80x24x1", " 80x24", "-1x24"] {
        assert_eq!(facts_and_read(&events(&format!("[0.1, \"r\", \"{odd}\"]\n"))).err(), Some(CantPlay::Damaged), "{odd}");
    }

    // Lines that aren't arrays.
    assert_eq!(facts_and_read(&events("{\"time\": 1}\n")).err(), Some(CantPlay::Damaged));
    assert_eq!(facts_and_read(&events("hello\n")).err(), Some(CantPlay::Damaged));
    assert_eq!(facts_and_read(&events("[0.1, \"o\", 5]\n")).err(), Some(CantPlay::Damaged));
    assert_eq!(facts_and_read(&events("[0.1, \"o\"]\n")).err(), Some(CantPlay::Damaged));
    // Times NaN, negative, too large to be a number, written as words, decreasing, past 24 hours.
    for odd in ["NaN", "-0.5", "1e999", "\"0.5\"", "null", "Infinity"] {
        assert_eq!(facts_and_read(&events(&format!("[{odd}, \"o\", \"x\"]\n"))).err(), Some(CantPlay::Damaged), "{odd}");
    }
    assert_eq!(facts_and_read(&events("[2.0, \"o\", \"a\"]\n[1.0, \"o\", \"b\"]\n")).err(), Some(CantPlay::Damaged));
    // Going backwards is damage even when the line going back is input.
    assert_eq!(facts_and_read(&events("[2.0, \"o\", \"a\"]\n[1.0, \"i\", \"b\"]\n")).err(), Some(CantPlay::Damaged));
    assert!(facts_and_read(&events("[1.0, \"o\", \"a\"]\n[1.0, \"o\", \"b\"]\n")).is_ok());
    assert_eq!(facts_and_read(&events("[86400.5, \"o\", \"x\"]\n")).err(), Some(CantPlay::TooLarge));
    assert_eq!(facts_and_read(&events("[1e300, \"o\", \"x\"]\n")).err(), Some(CantPlay::TooLarge));
    assert!(facts_and_read(&events("[86400, \"o\", \"x\"]\n")).is_ok());
    assert!(facts_and_read(&events("[-0.0, \"o\", \"x\"]\n")).is_ok());
    // A version that isn't a whole number isn't asciinema's.
    for odd in ["2.0", "\"1.0.0\"", "\"2\"", "null"] {
        let header = format!("{{\"version\": {odd}, \"width\": 80, \"height\": 24}}\n");
        assert_eq!(facts_and_read(header.as_bytes()).err(), Some(CantPlay::NotARecording), "{odd}");
        let pretty = format!("{{\n  \"version\": {odd},\n  \"width\": 80\n}}\n");
        assert_eq!(facts_and_read(pretty.as_bytes()).err(), Some(CantPlay::Damaged), "{odd}");
    }
    // A header longer than any header is, with or without a line's end.
    let long_header = header(&format!("\"width\": 80, \"height\": 24, \"title\": \"{}\"", "a".repeat(2 * 1024 * 1024)));
    assert_eq!(facts_and_read(&long_header).err(), Some(CantPlay::TooLarge));
    assert_eq!(facts_and_read(&long_header[..long_header.len() - 1]).err(), Some(CantPlay::TooLarge));
    // A header padded past the longest, with an event on the same line.
    let padded = [&header("\"width\": 80, \"height\": 24")[..], &vec![b' '; 2 * 1024 * 1024], b"[0.1, \"o\", \"x\"]\n"].concat();
    let padded = [&padded[..padded.iter().position(|b| *b == b'\n').unwrap()], &padded[padded.iter().position(|b| *b == b'\n').unwrap() + 1..]].concat();
    assert_eq!(facts_and_read(&padded).err(), Some(CantPlay::TooLarge));

    // A single line of 30 MB is too large, and is never parsed.
    let started = Instant::now();
    let huge = events(&format!("[0.1, \"o\", \"{}\"]\n", "a".repeat(30 * 1024 * 1024)));
    assert_eq!(facts_and_read(&huge).err(), Some(CantPlay::TooLarge));
    drop(huge);
    // Output of a byte past 1 MB is too large; of 1 MB written with escapes, it plays.
    let past = events(&format!("[0.1, \"o\", \"{}\"]\n", "a".repeat(MOST_OUTPUT + 1)));
    assert_eq!(facts_and_read(&past).err(), Some(CantPlay::TooLarge));
    let escaped = events(&format!("[0.1, \"o\", \"{}\"]\n", "\\u001b".repeat(MOST_OUTPUT)));
    assert_eq!(facts_and_read(&escaped).unwrap().events.len(), 1);
    // A file past 32 MB is too large, whatever it holds.
    let long = [make_test_cast(80, 24, &[]), vec![b'\n'; MOST_BYTES]].concat();
    assert_eq!(facts_and_read(&long).err(), Some(CantPlay::TooLarge));
    drop(long);

    // A million events play, and one more is too many.
    let many = events(&"[0,\"o\",\"\"]\n".repeat(MOST_EVENTS));
    assert_eq!(facts(&many).unwrap().length, Duration::ZERO);
    let too_many = [many, b"[0,\"o\",\"\"]\n".to_vec()].concat();
    assert_eq!(facts_and_read(&too_many).err(), Some(CantPlay::TooLarge));
    drop(too_many);
    assert!(started.elapsed() < Duration::from_secs(20));

    // Output that is half of a UTF-8 character, or not UTF-8 at all, keeps the replacement
    // character; so does half of a character written as an escape.
    let half = [make_test_cast(80, 24, &[]), b"[0.1, \"o\", \"ab\xe2\x82\"]\n[0.2, \"o\", \"\xff\"]\n[0.3, \"o\", \"\\ud83d!\"]\n".to_vec()].concat();
    let recording = facts_and_read(&half).unwrap();
    let whats: Vec<&What> = recording.events.iter().map(|event| &event.what).collect();
    assert_eq!(whats, [&What::Output("ab\u{fffd}".into()), &What::Output("\u{fffd}".into()), &What::Output("\u{fffd}!".into())]);

    // A header begun with a byte order mark reads.
    assert!(facts_and_read(&[b"\xef\xbb\xbf".as_slice(), &make_test_cast(80, 24, &[(0.1, "o", "x")])].concat()).is_ok());
}

#[test]
fn what_a_recording_keeps_is_bounded() {
    let not_utf8 = |code: &str, count: usize, each: usize| {
        let line = [format!("[0,\"{code}\",\"").as_bytes(), &vec![0xff; each], b"\"]\n"].concat();
        [make_test_cast(80, 24, &[]), line.repeat(count)].concat()
    };
    // Output of bytes that aren't UTF-8 becomes three times as much text. A file of 31 such
    // outputs of 1 MB would keep 93 MB, so it's too large; one of 20 keeps three times the file.
    let most = not_utf8("o", 31, MOST_OUTPUT);
    assert!(most.len() <= MOST_BYTES);
    assert_eq!(facts_and_read(&most).err(), Some(CantPlay::TooLarge));
    drop(most);
    let twenty = not_utf8("o", 20, MOST_OUTPUT);
    let (recording, _, kept) = measured(|| read(&twenty));
    let recording = recording.unwrap();
    assert!(kept <= 3 * twenty.len() as isize + 4096, "kept {kept} of {}", twenty.len());
    let (_, _, added) = measured(|| played(&recording));
    assert!(added <= 20 * 16 + 64, "played kept {added}");
    assert_eq!(facts_and_read(&twenty).unwrap(), recording);
    drop((twenty, recording));

    // A million marks of labels that aren't UTF-8 would keep more than the most.
    let marks = not_utf8("m", MOST_EVENTS, 20);
    assert!(marks.len() <= MOST_BYTES);
    assert_eq!(facts_and_read(&marks).err(), Some(CantPlay::TooLarge));
    drop(marks);
    // A hundred thousand keep each label once, held at its length.
    let marks = not_utf8("m", 100_000, 20);
    let (recording, _, kept) = measured(|| read(&marks));
    let recording = recording.unwrap();
    let each = size_of::<Event>() + size_of::<Mark>() + 60;
    assert!(kept <= (100_000 * each) as isize + 4096, "kept {kept}, {each} a mark");
    assert_eq!(recording.label(&recording.marks[99_999]), "\u{fffd}".repeat(20));
}

#[test]
fn facts_agree_with_read() {
    for cast in [made(), make_demo_cast(), make_test_cast(500, 200, &[(0.1, "m", "a"), (5.0, "m", "b"), (7.5, "o", "c")])] {
        let facts = facts(&cast).unwrap();
        let recording = read(&cast).unwrap();
        agree(&facts, &recording);
    }
    let three = make_test_cast(500, 200, &[(0.1, "m", "a"), (5.0, "m", "b"), (7.5, "o", "c")]);
    let facts = facts(&three).unwrap();
    assert_eq!((facts.width, facts.height, facts.marks), (500, 200, 2));
    assert!(close(facts.length, secs(2.1)));
}

#[test]
fn each_refusal_reads_in_plain_words() {
    let all = [CantPlay::NotARecording, CantPlay::Damaged, CantPlay::TooLarge, CantPlay::KindNotPlayed, CantPlay::CouldNotRead];
    let said: Vec<String> = all.iter().map(|why| why.to_string()).collect();
    for words in &said {
        assert!(words.starts_with(|c: char| c.is_uppercase()) && words.ends_with('.'), "{words}");
        assert!(words.len() < 200, "{words}");
        for odd in ["<", ">", "_", "::", "/", "\\", "Err", "{", "cast", "v2", "JSON"] {
            assert!(!words.contains(odd), "{words} has {odd}");
        }
        assert!(words.contains("terminal recording"), "{words}");
    }
    for (i, a) in said.iter().enumerate() {
        assert!(said[i + 1..].iter().all(|b| b != a), "{a} is said twice");
    }
    assert_eq!(said[0], "This file isn't a terminal recording.");
    assert_eq!(said[1], "This terminal recording is damaged.");
    assert_eq!(
        said[2],
        "This terminal recording is too large to play here. Recordings of up to 32 MB, 24 hours and screens of 500 by 200 can be played."
    );
    assert_eq!(
        said[3],
        "This kind of terminal recording can't be played here. Recordings made by asciinema in its second format can."
    );
    assert_eq!(said[4], "This terminal recording couldn't be read.");
}
