use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell as Counter;
use std::time::{Duration, Instant};

use proptest::prelude::*;
use slides_terminal::*;

/// Counts, for each thread, the bytes it holds and the largest single allocation it made since
/// asked last, so a test can tell what feeding a screen kept apart from the tests beside it.
struct Counting;

thread_local! {
    static HELD: Counter<isize> = const { Counter::new(0) };
    static LARGEST: Counter<usize> = const { Counter::new(0) };
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
    HELD.with(Counter::get)
}

/// A screen of the given size, fed `text`.
fn fed(width: u16, height: u16, text: &str) -> Screen {
    let mut screen = Screen::new(width, height);
    screen.feed(text);
    screen
}

/// A row's text as it shows, the right halves of wide characters left out and the spaces at
/// its end taken off.
fn line(screen: &Screen, row: u16) -> String {
    let cells = screen.rows().nth(usize::from(row)).unwrap();
    let text: String = cells.iter().filter(|cell| cell.wide != Wide::Right).map(|cell| cell.text).collect();
    text.trim_end().to_owned()
}

fn lines(screen: &Screen) -> Vec<String> {
    (0..screen.height()).map(|row| line(screen, row)).collect()
}

const ESC: &str = "\u{1b}";

/// `ESC [` followed by `rest`.
fn csi(rest: &str) -> String {
    format!("{ESC}[{rest}")
}

#[test]
fn text_wraps_and_scrolls() {
    let screen = fed(5, 3, "abcdefgh");
    assert_eq!(lines(&screen), ["abcde", "fgh", ""]);
    assert_eq!(screen.cursor(), Some((3, 1)));
    assert_eq!((screen.width(), screen.height()), (5, 3));
    assert_eq!(screen.rows().count(), 3);

    // Filling a row leaves the cursor on its last cell until the next character comes, so a new
    // line after a full row doesn't leave an empty one.
    let mut screen = fed(5, 3, "abcde");
    assert_eq!(screen.cursor(), Some((4, 0)));
    screen.feed("\r\nx");
    assert_eq!(lines(&screen), ["abcde", "x", ""]);

    // Lines past the last scroll the screen up.
    let screen = fed(5, 3, "1\r\n2\r\n3\r\n4");
    assert_eq!(lines(&screen), ["2", "3", "4"]);
    assert_eq!(screen.cursor(), Some((1, 2)));
    // A line feed alone keeps the column.
    let screen = fed(5, 3, "ab\ncd");
    assert_eq!(lines(&screen), ["ab", "  cd", ""]);

    // Without wrapping, the last cell is written over.
    let screen = fed(5, 2, &format!("{}abcdefg", csi("?7l")));
    assert_eq!(lines(&screen), ["abcdg", ""]);
    // And with it again, text wraps.
    let screen = fed(5, 2, &format!("{}{}abcdefg", csi("?7l"), csi("?7h")));
    assert_eq!(lines(&screen), ["abcde", "fg"]);

    // Back a cell, and tab stops every eight.
    let screen = fed(20, 2, "abc\u{8}\u{8}x\r\nab\tc\td");
    assert_eq!(lines(&screen), ["axc", "ab      c       d"]);
    // A tab stops at the last cell.
    let screen = fed(10, 1, "\t\t\tx");
    assert_eq!(lines(&screen), ["         x"]);
    // Back from the left edge stays there.
    let screen = fed(5, 1, "\u{8}\u{8}x");
    assert_eq!(lines(&screen), ["x"]);
}

#[test]
fn the_cursor_moves_where_it_is_told() {
    let at = |text: &str| fed(20, 10, text).cursor().unwrap();
    assert_eq!(at(&csi("5;7H")), (6, 4));
    assert_eq!(at(&csi("5;7f")), (6, 4));
    assert_eq!(at(&csi("H")), (0, 0));
    assert_eq!(at(&format!("{}{}", csi("5;7H"), csi("2A"))), (6, 2));
    assert_eq!(at(&format!("{}{}", csi("5;7H"), csi("B"))), (6, 5));
    assert_eq!(at(&format!("{}{}", csi("5;7H"), csi("3C"))), (9, 4));
    assert_eq!(at(&format!("{}{}", csi("5;7H"), csi("0D"))), (5, 4));
    assert_eq!(at(&format!("{}{}", csi("5;7H"), csi("2E"))), (0, 6));
    assert_eq!(at(&format!("{}{}", csi("5;7H"), csi("2F"))), (0, 2));
    assert_eq!(at(&format!("{}{}", csi("5;7H"), csi("12G"))), (11, 4));
    assert_eq!(at(&format!("{}{}", csi("5;7H"), csi("9d"))), (6, 8));
    // Moves stop at the screen's edges, however far they ask.
    assert_eq!(at(&csi("999;999H")), (19, 9));
    assert_eq!(at(&format!("{}{}", csi("5;7H"), csi("65535A"))), (6, 0));
    assert_eq!(at(&format!("{}{}", csi("5;7H"), csi("99999999999999999999999999999C"))), (19, 4));
    assert_eq!(at(&format!("{}{}", csi("5;7H"), csi("65535B"))), (6, 9));
    assert_eq!(at(&format!("{}{}", csi("5;7H"), csi("65535D"))), (0, 4));

    // Saved and brought back, both ways of asking.
    assert_eq!(at(&format!("{}{ESC}7{}{ESC}8", csi("3;4H"), csi("9;9H"))), (3, 2));
    assert_eq!(at(&format!("{}{}{}{}", csi("3;4H"), csi("s"), csi("9;9H"), csi("u"))), (3, 2));
    // The style is saved with the place.
    let screen = fed(20, 2, &format!("{}{ESC}7{}{ESC}8x", csi("31m"), csi("0m")));
    assert_eq!(screen.cell(0, 0).style.fg, Colour::Indexed(1));
    // Brought back with nothing saved, it's the top left.
    assert_eq!(at(&format!("{}{ESC}8", csi("5;5H"))), (0, 0));

    // Hidden and shown again.
    let mut screen = fed(20, 2, &csi("?25l"));
    assert_eq!(screen.cursor(), None);
    screen.feed(&csi("?25h"));
    assert_eq!(screen.cursor(), Some((0, 0)));
}

#[test]
fn colours_and_styles_are_kept() {
    let screen = fed(
        30,
        2,
        &format!(
            "{}R{}G{}I{}T{}B{}V{}N{}C",
            csi("31m"),
            csi("1;92m"),
            csi("38;5;200m"),
            csi("38;2;1;2;3m"),
            csi("48;5;17m"),
            csi("7m"),
            csi("0m"),
            csi("38:2::10:20:30;48:5:100m"),
        ),
    );
    let style = |col: u16| screen.cell(col, 0).style;
    assert_eq!(screen.cell(0, 0).text, 'R');
    assert_eq!(style(0), Style { fg: Colour::Indexed(1), ..Style::default() });
    assert_eq!(style(1), Style { fg: Colour::Indexed(10), bold: true, ..Style::default() });
    assert_eq!(style(2), Style { fg: Colour::Indexed(200), bold: true, ..Style::default() });
    assert_eq!(style(3), Style { fg: Colour::Rgb(1, 2, 3), bold: true, ..Style::default() });
    assert_eq!(style(4), Style { fg: Colour::Rgb(1, 2, 3), bg: Colour::Indexed(17), bold: true, ..Style::default() });
    assert_eq!(style(5), Style { fg: Colour::Rgb(1, 2, 3), bg: Colour::Indexed(17), bold: true, inverse: true, ..Style::default() });
    assert_eq!(style(6), Style::default());
    assert_eq!(style(7), Style { fg: Colour::Rgb(10, 20, 30), bg: Colour::Indexed(100), ..Style::default() });

    // Each style on and off.
    let screen = fed(
        30,
        1,
        &format!("{}a{}b{}c{}d{}e", csi("2;3;4m"), csi("22m"), csi("23m"), csi("24m"), csi("1;3;4;7;22;23;24;27m")),
    );
    assert_eq!(screen.cell(0, 0).style, Style { faint: true, italic: true, underline: true, ..Style::default() });
    assert_eq!(screen.cell(1, 0).style, Style { italic: true, underline: true, ..Style::default() });
    assert_eq!(screen.cell(2, 0).style, Style { underline: true, ..Style::default() });
    assert_eq!(screen.cell(3, 0).style, Style::default());
    assert_eq!(screen.cell(4, 0).style, Style::default());
    // Bright backgrounds, and back to the default colours; an empty one resets all.
    let screen = fed(30, 1, &format!("{}a{}b{}c{}d", csi("31;103m"), csi("39m"), csi("49m"), csi("1;31;m")));
    assert_eq!(screen.cell(0, 0).style, Style { fg: Colour::Indexed(1), bg: Colour::Indexed(11), ..Style::default() });
    assert_eq!(screen.cell(1, 0).style, Style { bg: Colour::Indexed(11), ..Style::default() });
    assert_eq!(screen.cell(2, 0).style, Style::default());
    assert_eq!(screen.cell(3, 0).style, Style::default());
    // A colour out of range is passed over without spoiling what follows it.
    let screen = fed(30, 1, &format!("{}a", csi("38;5;300;1m")));
    assert_eq!(screen.cell(0, 0).style, Style { bold: true, ..Style::default() });

    // The colours as they're drawn.
    assert_eq!(rgb(Colour::Default, true), [0xcc, 0xcc, 0xcc]);
    assert_eq!(rgb(Colour::Default, false), [0x18, 0x18, 0x18]);
    assert_eq!(rgb(Colour::Indexed(196), true), [255, 0, 0]);
    assert_eq!(rgb(Colour::Indexed(16), false), [0, 0, 0]);
    assert_eq!(rgb(Colour::Indexed(231), true), [255, 255, 255]);
    assert_eq!(rgb(Colour::Indexed(244), true), [128, 128, 128]);
    assert_eq!(rgb(Colour::Rgb(1, 2, 3), true), [1, 2, 3]);
    assert_ne!(rgb(Colour::Indexed(1), true), rgb(Colour::Indexed(9), true));
}

#[test]
fn wide_and_combining_characters_take_their_cells() {
    let screen = fed(10, 2, "\u{4e2d}a");
    assert_eq!((screen.cell(0, 0).text, screen.cell(0, 0).wide), ('\u{4e2d}', Wide::Left));
    assert_eq!(screen.cell(1, 0).wide, Wide::Right);
    assert_eq!((screen.cell(2, 0).text, screen.cell(2, 0).wide), ('a', Wide::Narrow));
    assert_eq!(screen.cursor(), Some((3, 0)));

    // A combining mark joins the cell before and takes no cell of its own.
    let screen = fed(10, 2, "e\u{301}x");
    assert_eq!(screen.cell(0, 0).text, 'e');
    assert_eq!(screen.cell(1, 0).text, 'x');
    assert_eq!(screen.cursor(), Some((2, 0)));

    // A wide character that doesn't fit on the row starts the next.
    let screen = fed(5, 2, "abcd\u{4e2d}");
    assert_eq!(line(&screen, 0), "abcd");
    assert_eq!(screen.cell(0, 1).wide, Wide::Left);
    assert_eq!(screen.cell(1, 1).wide, Wide::Right);

    // Writing over either half of a wide character leaves no half behind.
    let screen = fed(10, 1, "\u{4e2d}\rx");
    assert_eq!((screen.cell(0, 0).text, screen.cell(1, 0).text, screen.cell(1, 0).wide), ('x', ' ', Wide::Narrow));
    let screen = fed(10, 1, &format!("\u{4e2d}{}x", csi("2G")));
    assert_eq!((screen.cell(0, 0).text, screen.cell(0, 0).wide), (' ', Wide::Narrow));
    assert_eq!(screen.cell(1, 0).text, 'x');
    // On a screen one cell wide, a wide character has nowhere to go.
    let screen = fed(1, 2, "\u{4e2d}a");
    assert_eq!(lines(&screen), ["a", ""]);
}

#[test]
fn erasing_and_inserting_move_the_right_cells() {
    let row = |text: &str| line(&fed(10, 1, &format!("abcdefghij{}{text}", csi("1;4H"))), 0);
    assert_eq!(row(&csi("K")), "abc");
    assert_eq!(row(&csi("1K")), "    efghij");
    assert_eq!(row(&csi("2K")), "");
    assert_eq!(row(&csi("2@")), "abc  defgh");
    assert_eq!(row(&csi("2P")), "abcfghij");
    assert_eq!(row(&csi("99P")), "abc");
    assert_eq!(row(&csi("2X")), "abc  fghij");
    assert_eq!(row(&csi("99X")), "abc");

    let screen = |text: &str| lines(&fed(4, 4, &format!("aaaa\r\nbbbb\r\ncccc\r\ndddd{}{text}", csi("2;3H"))));
    assert_eq!(screen(&csi("J")), ["aaaa", "bb", "", ""]);
    assert_eq!(screen(&csi("1J")), ["", "   b", "cccc", "dddd"]);
    assert_eq!(screen(&csi("2J")), ["", "", "", ""]);
    // There's no scrollback for the third kind to clear.
    assert_eq!(screen(&csi("3J")), ["aaaa", "bbbb", "cccc", "dddd"]);
    assert_eq!(screen(&csi("L")), ["aaaa", "", "bbbb", "cccc"]);
    assert_eq!(screen(&csi("2M")), ["aaaa", "dddd", "", ""]);
    assert_eq!(screen(&csi("9L")), ["aaaa", "", "", ""]);
    // Inserting a line goes to the row's start.
    assert_eq!(fed(4, 4, &format!("{}{}", csi("2;3H"), csi("L"))).cursor(), Some((0, 1)));

    // What's erased takes the background colour then in use.
    let screen = fed(4, 2, &format!("ab{}{}", csi("44m"), csi("2J")));
    assert!(screen.rows().flatten().all(|cell| cell.text == ' ' && cell.style == Style { bg: Colour::Indexed(4), ..Style::default() }));
    let screen = fed(4, 1, &format!("abcd{}{}{}", csi("41m"), csi("2G"), csi("K")));
    assert_eq!(screen.cell(0, 0).style.bg, Colour::Default);
    assert_eq!(screen.cell(1, 0).style.bg, Colour::Indexed(1));

    // Erasing or shifting half of a wide character takes the whole of it.
    let screen = fed(6, 1, &format!("a\u{4e2d}b{}{}", csi("3G"), csi("K")));
    assert_eq!(line(&screen, 0), "a");
    let screen = fed(6, 1, &format!("a\u{4e2d}b{}{}", csi("3G"), csi("P")));
    assert_eq!(line(&screen, 0), "a b");
    assert!(screen.rows().flatten().all(|cell| cell.wide == Wide::Narrow));
    let screen = fed(4, 1, &format!("ab\u{4e2d}{}{}", csi("G"), csi("@")));
    assert_eq!(line(&screen, 0), " ab");
    assert!(screen.rows().flatten().all(|cell| cell.wide == Wide::Narrow));
}

#[test]
fn a_scroll_region_scrolls_only_itself() {
    let start = format!("1\r\n2\r\n3\r\n4\r\n5{}", csi("2;4r"));
    let after = |text: &str| lines(&fed(3, 5, &format!("{start}{text}")));
    // Setting the region goes to the top left.
    assert_eq!(fed(3, 5, &start).cursor(), Some((0, 0)));
    assert_eq!(after(&format!("{}\n", csi("4;1H"))), ["1", "3", "4", "", "5"]);
    assert_eq!(after(&format!("{}{ESC}D", csi("4;1H"))), ["1", "3", "4", "", "5"]);
    assert_eq!(after(&format!("{}{ESC}E", csi("4;1H"))), ["1", "3", "4", "", "5"]);
    assert_eq!(after(&format!("{}{ESC}M", csi("2;1H"))), ["1", "", "2", "3", "5"]);
    assert_eq!(after(&csi("S")), ["1", "3", "4", "", "5"]);
    assert_eq!(after(&csi("2T")), ["1", "", "", "2", "5"]);
    assert_eq!(after(&format!("{}{}", csi("3;1H"), csi("L"))), ["1", "2", "", "3", "5"]);
    assert_eq!(after(&format!("{}{}", csi("2;1H"), csi("M"))), ["1", "3", "4", "", "5"]);
    // Outside the region, inserting does nothing; the last row doesn't scroll anything.
    assert_eq!(after(&format!("{}{}", csi("5;1H"), csi("L"))), ["1", "2", "3", "4", "5"]);
    assert_eq!(after(&format!("{}\n\nx", csi("5;1H"))), ["1", "2", "3", "4", "x"]);
    // Moving up and down stops at the region's edges from within it.
    assert_eq!(fed(3, 5, &format!("{start}{}{}", csi("3;1H"), csi("9A"))).cursor(), Some((0, 1)));
    assert_eq!(fed(3, 5, &format!("{start}{}{}", csi("3;1H"), csi("9B"))).cursor(), Some((0, 3)));
    // A region that isn't one is passed over; an empty one is the whole screen again.
    assert_eq!(after(&format!("{}{}\n", csi("4;2r"), csi("4;1H"))), ["1", "3", "4", "", "5"]);
    assert_eq!(after(&format!("{}{}\n", csi("r"), csi("5;1H"))), ["2", "3", "4", "5", ""]);
}

#[test]
fn the_alternate_screen_comes_and_goes() {
    let mut screen = fed(10, 3, &format!("main{}", csi("2;3H")));
    screen.feed(&csi("?1049h"));
    assert_eq!(lines(&screen), ["", "", ""]);
    screen.feed(&format!("{}alt", csi("3;1H")));
    assert_eq!(lines(&screen), ["", "", "alt"]);
    screen.feed(&csi("?1049l"));
    assert_eq!(lines(&screen), ["main", "", ""]);
    assert_eq!(screen.cursor(), Some((2, 1)));
    // Coming back to it, it's cleared again.
    screen.feed(&csi("?1049h"));
    assert_eq!(lines(&screen), ["", "", ""]);
    screen.feed(&csi("?1049l"));

    // The older ways: 47 keeps what was on it, 1047 clears it on leaving.
    let mut screen = fed(10, 2, &format!("main{}x", csi("?47h")));
    assert_eq!(lines(&screen), ["    x", ""]);
    screen.feed(&csi("?47l"));
    assert_eq!(lines(&screen), ["main", ""]);
    screen.feed(&csi("?47h"));
    assert_eq!(lines(&screen), ["    x", ""]);
    screen.feed(&format!("{}{}", csi("?1047l"), csi("?1047h")));
    assert_eq!(lines(&screen), ["", ""]);
    screen.feed(&csi("?1047l"));
    assert_eq!(lines(&screen), ["main", ""]);

    // A resize while on it resizes both.
    let mut screen = fed(10, 3, &format!("main{}{}alt", csi("?1049h"), csi("H")));
    screen.resize(6, 2);
    assert_eq!(lines(&screen), ["alt", ""]);
    screen.feed(&csi("?1049l"));
    assert_eq!(lines(&screen), ["main", ""]);
    assert!(screen.rows().all(|row| row.len() == 6));
}

#[test]
fn a_resize_keeps_what_fits() {
    let mut screen = fed(6, 3, "abcdef\r\nghi");
    screen.resize(4, 4);
    assert_eq!((screen.width(), screen.height()), (4, 4));
    assert_eq!(lines(&screen), ["abcd", "ghi", "", ""]);
    assert!(screen.rows().all(|row| row.len() == 4));
    // Shrinking past the cursor's row keeps the cursor's row on the screen.
    let mut screen = fed(4, 4, "1\r\n2\r\n3\r\n4");
    screen.resize(4, 2);
    assert_eq!(lines(&screen), ["3", "4"]);
    assert_eq!(screen.cursor(), Some((1, 1)));
    // A wide character cut in half by the new edge goes.
    let mut screen = fed(4, 1, "a\u{4e2d}");
    screen.resize(2, 1);
    assert_eq!(line(&screen, 0), "a");
    assert_eq!(screen.cell(1, 0).wide, Wide::Narrow);
    // Sizes are kept within what a screen can be.
    let mut screen = Screen::new(0, 60000);
    assert_eq!((screen.width(), screen.height()), (1, 200));
    screen.resize(60000, 0);
    assert_eq!((screen.width(), screen.height()), (500, 1));
}

#[test]
fn starting_again_clears_everything() {
    let screen = fed(10, 3, &format!("{}{}{}text{}{ESC}c", csi("31m"), csi("?25l"), csi("2;3r"), csi("?1049h")));
    let fresh = Screen::new(10, 3);
    assert_eq!(screen, fresh);
    assert_eq!(screen.cursor(), Some((0, 0)));
}

#[test]
fn line_drawing_is_drawn_as_lines() {
    let screen = fed(10, 1, &format!("{ESC}(0lqk{ESC}(Bq"));
    assert_eq!(line(&screen, 0), "\u{250c}\u{2500}\u{2510}q");
}

/// The sequences a recording might use to reach beyond its screen: set a title, the clipboard
/// or a link, change colours, ask a question, resize a window, ring the bell, and some no
/// terminal knows.
fn reaching_out() -> Vec<(&'static str, String)> {
    let st = format!("{ESC}\\");
    vec![
        ("title with a bell", format!("{ESC}]0;a title\u{7}")),
        ("icon name", format!("{ESC}]1;a name{st}")),
        ("window title", format!("{ESC}]2;a title{st}")),
        ("title by eight-bit codes", "\u{9d}2;a title\u{9c}".to_owned()),
        ("clipboard", format!("{ESC}]52;c;cm0gLXJmIH4=\u{7}")),
        ("clipboard asked for", format!("{ESC}]52;c;?\u{7}")),
        ("link", format!("{ESC}]8;;file:///etc/passwd{st}")),
        ("link ended", format!("{ESC}]8;;{st}")),
        ("palette", format!("{ESC}]4;1;rgb:ff/00/00\u{7}")),
        ("palette asked for", format!("{ESC}]4;1;?\u{7}")),
        ("foreground", format!("{ESC}]10;#ffffff\u{7}")),
        ("background", format!("{ESC}]11;?\u{7}")),
        ("palette reset", format!("{ESC}]104\u{7}")),
        ("working folder", format!("{ESC}]7;file://host/home{st}")),
        ("a file sent", format!("{ESC}]1337;File=name=eA==:eA=={st}")),
        ("notification", format!("{ESC}]9;hello\u{7}")),
        ("device attributes", csi("c")),
        ("secondary attributes", csi(">c")),
        ("tertiary attributes", csi("=c")),
        ("identify", format!("{ESC}Z")),
        ("status report", csi("5n")),
        ("cursor position report", csi("6n")),
        ("private status report", csi("?6n")),
        ("setting asked for", format!("{ESC}P$qm{st}")),
        ("capability asked for", format!("{ESC}P+q544e{st}")),
        ("version", csi(">q")),
        ("mode asked for", csi("?25$p")),
        ("keyboard asked for", csi("?u")),
        ("window resized", csi("8;50;100t")),
        ("window size asked for", csi("14t")),
        ("window title reported", csi("21t")),
        ("window moved", csi("3;0;0t")),
        ("bell", "\u{7}".to_owned()),
        ("application program command", format!("{ESC}_Gf=24;AAAA{st}")),
        ("privacy message", format!("{ESC}^secret{st}")),
        ("start of string", format!("{ESC}Xsomething{st}")),
        ("device control", format!("{ESC}Pq#0;2;0;0;0#0~~@@vv{st}")),
        ("mouse reporting", csi("?1000h")),
        ("focus reporting", csi("?1004h")),
        ("pasting", csi("?2004h")),
        ("unknown control sequence", csi("99;1z")),
        ("unknown private sequence", csi("?9999h")),
        ("unknown escape", format!("{ESC}#8")),
        ("soft reset", csi("!p")),
        ("cursor shape", csi("2 q")),
        ("eight-bit device control", "\u{90}$qm\u{9c}".to_owned()),
        ("eight-bit application command", "\u{9f}anything\u{9c}".to_owned()),
    ]
}

#[test]
fn nothing_in_a_recording_reaches_out() {
    let plain = fed(20, 3, "AB");
    for (what, sequence) in reaching_out() {
        let screen = fed(20, 3, &format!("A{sequence}B"));
        assert_eq!(screen, plain, "{what} changed the screen");
        // Split anywhere across two feeds, as across a recording's events.
        let chars: Vec<char> = format!("A{sequence}B").chars().collect();
        for at in 0..chars.len() {
            let mut screen = Screen::new(20, 3);
            screen.feed(&chars[..at].iter().collect::<String>());
            screen.feed(&chars[at..].iter().collect::<String>());
            assert_eq!(screen, plain, "{what} split at {at} changed the screen");
        }
    }
    // One that never ends keeps nothing, however long it goes on, and the screen is itself
    // again once it's ended.
    let mut screen = fed(20, 3, "A");
    screen.feed(&format!("{ESC}]2;"));
    let before = held();
    for _ in 0..1000 {
        screen.feed(&"x".repeat(1000));
    }
    assert!(held() - before <= 0, "a string that never ended kept {}", held() - before);
    screen.feed("\u{7}B");
    assert_eq!(screen, plain);
    // Nor does a sequence with ten thousand parameters, or one too large to count.
    let screen = fed(20, 3, &format!("A{}B", csi(&format!("{}m", "1;".repeat(10_000)))));
    assert_eq!(line(&screen, 0), "AB");
    let screen = fed(20, 3, &format!("A{}B", csi(&format!("{}C", "9".repeat(30)))));
    assert_eq!(line(&screen, 0), "A                  B");
    // The screen has nothing to send: what it offers is its size, its cells and its cursor,
    // and all a recording can do is feed it or resize it within its limits.
    let screen = fed(20, 3, &csi("8;200;600t"));
    assert_eq!((screen.width(), screen.height()), (20, 3));
}

/// A piece of what a program might write: text, controls, a sequence known here with any
/// parameters, a string, or bytes of any kind.
fn piece() -> impl Strategy<Value = String> {
    let number = prop_oneof![0u32..10, 0u32..300, Just(65535), Just(4_000_000_000)];
    let params = prop::collection::vec(number, 0..6).prop_map(|numbers| {
        numbers.iter().map(u32::to_string).collect::<Vec<_>>().join(";")
    });
    let finals = prop::sample::select(b"ABCDEFGHJKLMPSTXZ@`abcdefghlmnqrstu".to_vec());
    prop_oneof![
        "[a-z ]{0,12}",
        prop::sample::select(vec!["\u{4e2d}", "\u{1f600}", "e\u{301}", "\u{301}", "\u{200b}", "\t", "\r", "\n", "\u{8}", "\u{7}", "\u{1b}", "\u{9b}", "\u{9c}", "\u{9d}", "\u{85}", "\u{18}"])
            .prop_map(str::to_owned),
        (prop::sample::select(vec!["", "?", ">", "="]), params.clone(), finals).prop_map(|(mark, params, end)| format!(
            "\u{1b}[{mark}{params}{}",
            end as char
        )),
        params.clone().prop_map(|params| format!("\u{1b}[?{params}h")),
        params.clone().prop_map(|params| format!("\u{1b}[?{params}l")),
        params.prop_map(|params| format!("\u{1b}[{params}r")),
        prop::sample::select(vec!["\u{1b}7", "\u{1b}8", "\u{1b}D", "\u{1b}E", "\u{1b}M", "\u{1b}c", "\u{1b}(0", "\u{1b}(B", "\u{1b}[?1049h", "\u{1b}[?1049l", "\u{1b}[?47h", "\u{1b}[?1047l", "\u{1b}[?7l", "\u{1b}[?25l"])
            .prop_map(str::to_owned),
        "[ -~]{0,20}".prop_map(|text| format!("\u{1b}]0;{text}\u{7}")),
        "[ -~]{0,20}".prop_map(|text| format!("\u{1b}P{text}\u{1b}\\")),
        prop::collection::vec(any::<u8>(), 0..24).prop_map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
        prop::collection::vec(any::<char>(), 0..8).prop_map(|chars| chars.into_iter().collect()),
    ]
}

/// What a recording might do to a screen: write something, or resize it.
#[derive(Debug, Clone)]
enum Step {
    Feed(String),
    Resize(u16, u16),
}

fn steps() -> impl Strategy<Value = Vec<Step>> {
    let feed = prop::collection::vec(piece(), 1..12).prop_map(|pieces| Step::Feed(pieces.concat()));
    let resize = (0u16..40, 0u16..20).prop_map(|(width, height)| Step::Resize(width, height));
    prop::collection::vec(prop_oneof![8 => feed, 1 => resize], 1..40)
}

/// The screen is sound: its size is its rows and columns, each wide character has its right
/// half, and the cursor is on it.
fn sound(screen: &Screen) {
    let (width, height) = (screen.width(), screen.height());
    assert!((1..=500).contains(&width) && (1..=200).contains(&height));
    assert_eq!(screen.rows().count(), usize::from(height));
    for row in screen.rows() {
        assert_eq!(row.len(), usize::from(width));
        for (col, cell) in row.iter().enumerate() {
            match cell.wide {
                Wide::Left => assert_eq!(row.get(col + 1).map(|cell| cell.wide), Some(Wide::Right), "{row:?}"),
                Wide::Right => assert_eq!(col.checked_sub(1).map(|col| row[col].wide), Some(Wide::Left), "{row:?}"),
                Wide::Narrow => {}
            }
        }
    }
    // The cursor, shown, is on the screen.
    let mut shown = screen.clone();
    shown.feed("\u{18}\u{1b}[?25h");
    let (col, row) = shown.cursor().unwrap();
    assert!(col < width && row < height, "the cursor is at {col}, {row} on {width} by {height}");
}

fn cases() -> u32 {
    std::env::var("PROPTEST_CASES").ok().and_then(|cases| cases.parse().ok()).unwrap_or(128)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), ..ProptestConfig::default() })]

    #[test]
    fn any_output_keeps_the_screen_sound(width in 0u16..30, height in 0u16..12, steps in steps(), split in any::<prop::sample::Index>()) {
        let started = Instant::now();
        LARGEST.with(|largest| largest.set(0));
        let mut screen = Screen::new(width, height);
        for step in &steps {
            match step {
                Step::Feed(text) => {
                    // Fed in two parts split anywhere, as the same output across two events.
                    let at = split.index(text.chars().count() + 1);
                    let at = text.char_indices().nth(at).map_or(text.len(), |(at, _)| at);
                    screen.feed(&text[..at]);
                    sound(&screen);
                    screen.feed(&text[at..]);
                }
                Step::Resize(width, height) => screen.resize(*width, *height),
            }
            sound(&screen);
        }
        prop_assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
        prop_assert!(LARGEST.with(Counter::get) <= 64 * 1024 * 1024);
    }
}

#[test]
fn any_output_keeps_the_screen_sound_named_cases() {
    let mut screen = Screen::new(500, 200);
    let hostile = [
        // Ten thousand parameters, and one of thirty digits.
        csi(&format!("{}H", "7;".repeat(10_000))),
        csi(&format!("{}L", "9".repeat(30))),
        // Cut off at the end of an event.
        format!("{ESC}["),
        "12;".to_owned(),
        "4H".to_owned(),
        format!("{ESC}]2;never ends"),
        "x".repeat(100_000),
        "\u{7}".to_owned(),
        // Wide characters filling every row, and the screen cleared and scrolled a great deal.
        "\u{4e2d}".repeat(500 * 200),
        format!("{}{}", csi("2;199r"), "\n".repeat(100_000)),
        csi("2J").repeat(1000),
        csi("65535@").repeat(1000),
        csi("65535M").repeat(1000),
        "\u{fffd}".repeat(10_000),
    ];
    let started = Instant::now();
    LARGEST.with(|largest| largest.set(0));
    let before = held();
    for text in &hostile {
        screen.feed(text);
        sound(&screen);
    }
    for (width, height) in [(1, 1), (500, 200), (3, 200), (500, 2), (0, 0), (65535, 65535)] {
        screen.resize(width, height);
        screen.feed("\u{4e2d}\u{4e2d}ab\n");
        sound(&screen);
    }
    assert!(started.elapsed() < Duration::from_secs(2), "took {:?}", started.elapsed());
    assert!(LARGEST.with(Counter::get) <= 64 * 1024 * 1024);
    // A screen of 500 by 200 cells holds a few megabytes, whatever it's fed.
    assert!(held() - before <= 16 * 1024 * 1024, "kept {}", held() - before);
}
